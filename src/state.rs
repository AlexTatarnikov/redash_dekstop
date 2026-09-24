//! All application logic, with no UI or I/O.
//!
//! The UI turns user actions into [`Event`]s, [`AppState::update`] applies them and
//! returns [`Effect`]s (network calls, saving settings) for the runtime in `app.rs`
//! to perform. Results of those effects come back as more events. Keeping this
//! module pure means every behaviour can be covered by plain unit tests.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use crate::api::{DataSource, QueryResult, Table};
use crate::config::Config;
use crate::export;
use crate::history::{self, Entry};
use crate::vars::{self, Definition, Run, Variable};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    // User intents, emitted by the UI.
    Connect,
    Execute,
    ReloadDataSources,
    Disconnect,
    SelectSource(i64),
    /// Show this page of the result (0-based); clamped to the last page.
    ShowPage(usize),
    /// Change rows per page, keeping the first visible row on screen.
    SetPageSize(usize),
    /// Copy the rows on the current page to the clipboard as a Markdown table.
    CopyPageMarkdown,
    /// Save the whole result as a CSV file chosen by the user.
    ExportCsv,
    /// Show or hide the variables panel.
    ToggleVariables,
    AddVariable(VariableKind),
    /// Remove the variable with this id.
    RemoveVariable(u64),
    /// A variable's name or definition was edited in place by the UI.
    VariablesEdited,
    /// Rerun this query variable (and any variables it needs that have no value).
    RunVariable(u64),
    /// Show or hide the history panel.
    ToggleHistory,
    /// Put this history entry's SQL, data source and variables back in the editor.
    RestoreHistory(usize),
    ClearHistory,
    // Results of effects, delivered by the runtime.
    DataSourcesLoaded(Result<Vec<DataSource>, String>),
    QueryFinished(Result<QueryResult, String>),
    SchemaLoaded {
        data_source_id: i64,
        result: Result<Vec<Table>, String>,
    },
    ConfigError(String),
    /// The CSV file was written; `Ok(None)` when the user cancelled the dialog.
    CsvSaved(Result<Option<PathBuf>, String>),
    VariablesLoaded(Result<Vec<Variable>, String>),
    HistoryLoaded(Result<Vec<Entry>, String>),
    VariableFinished {
        id: u64,
        result: Result<QueryResult, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Value,
    Query,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Fetch data sources; also acts as the credentials check. Answered by
    /// [`Event::DataSourcesLoaded`].
    LoadDataSources(Config),
    /// Run SQL ad hoc. Answered by [`Event::QueryFinished`].
    Execute {
        config: Config,
        data_source_id: i64,
        sql: String,
    },
    /// Fetch tables and columns for autocompletion. Answered by [`Event::SchemaLoaded`].
    LoadSchema {
        config: Config,
        data_source_id: i64,
    },
    SaveConfig(Config),
    ClearConfig,
    CopyToClipboard(String),
    /// Ask where to save (suggesting `file_name`) and write `contents` there.
    /// Answered by [`Event::CsvSaved`].
    SaveCsv {
        file_name: String,
        contents: String,
    },
    /// Read saved variables. Answered by [`Event::VariablesLoaded`].
    LoadVariables,
    SaveVariables(Vec<Variable>),
    /// Read saved history. Answered by [`Event::HistoryLoaded`].
    LoadHistory,
    SaveHistory(Vec<Entry>),
    /// Run a query variable's SQL (variables already substituted). Answered by
    /// [`Event::VariableFinished`].
    RunVariable {
        config: Config,
        id: u64,
        data_source_id: i64,
        sql: String,
    },
}

/// What a run is for; it waits until the variables it uses have values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Query,
    Variable(u64),
}

/// A data source's tables and columns, for autocompletion.
#[derive(Debug, PartialEq)]
pub enum Schema {
    Loading,
    Loaded(Vec<Table>),
}

/// Rows-per-page choices; the first is the default, as in Redash's web UI.
pub const PAGE_SIZES: [usize; 5] = [25, 50, 100, 250, 1000];

#[derive(Debug)]
pub enum Screen {
    Setup(SetupState),
    Editor(Box<EditorState>),
}

#[derive(Debug, Default)]
pub struct SetupState {
    pub host: String,
    pub api_key: String,
    pub connecting: bool,
    pub error: Option<String>,
}

impl SetupState {
    pub fn can_connect(&self) -> bool {
        !self.host.trim().is_empty() && !self.api_key.trim().is_empty() && !self.connecting
    }

    fn config(&self) -> Config {
        Config { host: self.host.trim().to_string(), api_key: self.api_key.trim().to_string() }
    }
}

#[derive(Debug)]
pub struct EditorState {
    pub config: Config,
    pub data_sources: Vec<DataSource>,
    pub loading_sources: bool,
    pub selected_source: Option<i64>,
    pub sql: String,
    pub running: bool,
    pub result: Option<ResultView>,
    pub error: Option<String>,
    /// Confirmation of the last action (e.g. an export), shown in the status bar.
    pub notice: Option<String>,
    /// Rows per page; kept across results.
    pub page_size: usize,
    /// Per data source; fetched when a source is first selected, refetched on reload.
    pub schemas: HashMap<i64, Schema>,
    /// Counts results so each one gets fresh table state in the UI.
    results_shown: u64,
    /// Referenced in SQL as `{{ name }}`; see `vars.rs`.
    pub variables: Vec<Variable>,
    pub show_variables: bool,
    next_variable_id: u64,
    /// The run waiting for variables; `running` is set meanwhile.
    pending: Option<Target>,
    /// Snapshots of past runs, newest first; see `history.rs`.
    pub history: Vec<Entry>,
    pub show_history: bool,
    /// The current Unix time in seconds; replaced in tests.
    pub clock: fn() -> u64,
}

impl EditorState {
    fn new(config: Config) -> Self {
        Self {
            config,
            data_sources: Vec::new(),
            loading_sources: false,
            selected_source: None,
            sql: "SELECT 1".into(),
            running: false,
            result: None,
            error: None,
            notice: None,
            page_size: PAGE_SIZES[0],
            schemas: HashMap::new(),
            results_shown: 0,
            variables: Vec::new(),
            show_variables: false,
            next_variable_id: 0,
            pending: None,
            history: Vec::new(),
            show_history: true,
            clock: history::now,
        }
    }

    pub fn can_execute(&self) -> bool {
        self.selected_source.is_some() && !self.running && has_statement(&self.sql)
    }

    /// Tables of the selected data source; empty until its schema has loaded.
    pub fn tables(&self) -> &[Table] {
        match self.selected_source.and_then(|id| self.schemas.get(&id)) {
            Some(Schema::Loaded(tables)) => tables,
            _ => &[],
        }
    }

    /// Whether a query variable can be run (or a variable removed) now.
    pub fn can_run_variable(&self) -> bool {
        !self.running
    }

    fn save_variables(&self) -> Vec<Effect> {
        vec![Effect::SaveVariables(self.variables.clone())]
    }

    /// Records the query about to run in the history.
    fn record_history(&mut self) -> Effect {
        let entry =
            Entry::new((self.clock)(), self.selected_source.unwrap_or_default(), &self.sql, &self.variables);
        history::record(&mut self.history, entry);
        Effect::SaveHistory(self.history.clone())
    }

    fn set_variables(&mut self, variables: Vec<Variable>) {
        self.variables.clear();
        for var in variables {
            self.add_variable(var.name, var.def);
        }
    }

    fn add_variable(&mut self, name: String, def: Definition) {
        self.next_variable_id += 1;
        self.variables.push(Variable { id: self.next_variable_id, name, def, run: None });
    }

    /// Starts `target` once the variables it uses have values; see [`Self::advance`].
    fn start(&mut self, target: Target) -> Vec<Effect> {
        self.running = true;
        self.error = None;
        self.notice = None;
        // Failed variables get another try.
        for var in &mut self.variables {
            if matches!(var.run, Some(Run::Failed(_))) {
                var.run = None;
            }
        }
        self.pending = Some(target);
        self.advance()
    }

    /// Moves the pending run forward: starts the variable queries it still needs
    /// (in parallel when independent), and the run itself once they all have values.
    fn advance(&mut self) -> Vec<Effect> {
        let Some(target) = self.pending else { return Vec::new() };
        let mut effects = Vec::new();
        let outcome = match target {
            Target::Query => {
                let sql = self.sql.clone();
                self.expand(&sql, &mut Vec::new(), &mut effects)
            }
            Target::Variable(id) => match self.variables.iter().find(|v| v.id == id) {
                Some(var) => {
                    let name = var.name.clone();
                    self.value(&name, &mut Vec::new(), &mut effects)
                }
                None => Err("The variable was removed".into()),
            },
        };
        match outcome {
            Ok(None) => {}
            Ok(Some(sql)) => {
                self.pending = None;
                match target {
                    Target::Query => effects.push(Effect::Execute {
                        config: self.config.clone(),
                        data_source_id: self.selected_source.unwrap_or_default(),
                        sql,
                    }),
                    Target::Variable(_) => self.running = false,
                }
            }
            Err(e) => {
                self.pending = None;
                self.running = false;
                self.error = Some(e);
            }
        }
        effects
    }

    /// `sql` with its variables substituted, or `None` while some are still
    /// running. Starts the runs it needs, adding them to `effects`.
    /// `stack` holds the variables being resolved, to catch cycles.
    fn expand(
        &mut self,
        sql: &str,
        stack: &mut Vec<String>,
        effects: &mut Vec<Effect>,
    ) -> Result<Option<String>, String> {
        let mut values = HashMap::new();
        let mut waiting = false;
        for (_, name) in vars::references(sql) {
            if values.contains_key(name) {
                continue;
            }
            match self.value(name, stack, effects)? {
                Some(value) => {
                    values.insert(name.to_string(), value);
                }
                None => waiting = true,
            }
        }
        Ok((!waiting).then(|| vars::substitute(sql, |name| values.get(name).cloned().unwrap_or_default())))
    }

    /// The value of variable `name`, or `None` while its query runs (starting it if needed).
    fn value(
        &mut self,
        name: &str,
        stack: &mut Vec<String>,
        effects: &mut Vec<Effect>,
    ) -> Result<Option<String>, String> {
        let Some(i) = self.variables.iter().position(|v| v.name == name) else {
            return Err(format!("Unknown variable {{{{ {name} }}}}: add it in Variables"));
        };
        let var = &self.variables[i];
        let source = match &var.def {
            Definition::Value { value } => return Ok(Some(value.clone())),
            Definition::Query { data_source_id, sql } => (*data_source_id, sql.clone()),
        };
        if let Some(value) = var.fresh_value() {
            return Ok(Some(value.to_string()));
        }
        match &var.run {
            // Once it finishes, a stale result is rerun.
            Some(Run::Running { .. }) => return Ok(None),
            Some(Run::Failed(e)) => return Err(format!("Variable {name}: {e}")),
            Some(Run::Done { .. }) | None => {}
        }
        if stack.iter().any(|n| n == name) {
            return Err(format!("Variable {name} refers to itself"));
        }
        let id = var.id;
        stack.push(name.to_string());
        let sql = self.expand(&source.1, stack, effects);
        stack.pop();
        if let Some(sql) = sql? {
            self.variables[i].run = Some(Run::Running { ran: source.clone() });
            effects.push(Effect::RunVariable {
                config: self.config.clone(),
                id,
                data_source_id: source.0,
                sql,
            });
        }
        Ok(None)
    }

    fn set_data_sources(&mut self, sources: Vec<DataSource>) -> Vec<Effect> {
        let still_exists = sources.iter().any(|s| Some(s.id) == self.selected_source);
        if !still_exists {
            self.selected_source = sources.first().map(|s| s.id);
        }
        self.data_sources = sources;
        self.load_schema()
    }

    /// Fetches the selected source's schema unless it is loaded or on its way.
    fn load_schema(&mut self) -> Vec<Effect> {
        match self.selected_source {
            Some(id) if !self.schemas.contains_key(&id) => {
                self.schemas.insert(id, Schema::Loading);
                vec![Effect::LoadSchema { config: self.config.clone(), data_source_id: id }]
            }
            _ => Vec::new(),
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { word.into() } else { format!("{word}s") }
}

/// Whether `sql` has anything besides whitespace and comments (`-- …` to the end of
/// the line, `/* … */`), i.e. whether a database would have something to run.
pub fn has_statement(sql: &str) -> bool {
    let mut rest = sql.trim_start();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("--") {
            rest = after.split_once('\n').map_or("", |(_, next)| next);
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.split_once("*/").map_or("", |(_, next)| next);
        } else {
            return true;
        }
        rest = rest.trim_start();
    }
    false
}

/// Toggles SQL line comments (`-- `) on the lines touched by `selection` (char
/// indices, `start <= end`), like Cmd+/ in code editors: if every non-blank line is
/// already commented they are uncommented, otherwise all are commented at their
/// common indentation. Returns the new text and the selection moved with its text.
pub fn toggle_line_comment(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        starts.push(offset);
        offset += line.chars().count() + 1;
    }
    let line_of = |pos: usize| starts.partition_point(|&s| s <= pos).saturating_sub(1);
    let first = line_of(selection.start);
    let mut last = line_of(selection.end);
    // A selection ending at the very start of a line doesn't include that line.
    if last > first && starts[last] == selection.end {
        last -= 1;
    }

    let indent = |line: &str| line.chars().take_while(|c| c.is_whitespace()).count();
    let touched = &lines[first..=last];
    let non_blank = || touched.iter().filter(|l| !l.trim().is_empty());
    let all_blank = non_blank().next().is_none();
    let uncomment = !all_blank && non_blank().all(|l| l.trim_start().starts_with("--"));
    let col = non_blank().map(|l| indent(l)).min().unwrap_or(0);

    // (char position in `text`, chars removed, chars inserted), in text order.
    let mut edits = Vec::new();
    let mut out = String::with_capacity(text.len() + 3 * touched.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let byte_at = |chars: usize| line.char_indices().nth(chars).map_or(line.len(), |(b, _)| b);
        if !(first..=last).contains(&i) {
            out.push_str(line);
        } else if uncomment && line.trim_start().starts_with("--") {
            let at = byte_at(indent(line));
            let removed = if line[at + 2..].starts_with(' ') { 3 } else { 2 };
            out.push_str(&line[..at]);
            out.push_str(&line[at + removed..]);
            edits.push((starts[i] + indent(line), removed, 0));
        } else if !uncomment && (all_blank || !line.trim().is_empty()) {
            let at = byte_at(col);
            out.push_str(&line[..at]);
            out.push_str("-- ");
            out.push_str(&line[at..]);
            edits.push((starts[i] + col, 0, 3));
        } else {
            out.push_str(line);
        }
    }

    let map = |pos: usize| {
        let mut new = pos;
        for &(at, removed, inserted) in &edits {
            if pos >= at + removed {
                new = new + inserted - removed;
            } else if pos > at {
                new -= pos - at;
            }
        }
        new
    };
    (out, map(selection.start)..map(selection.end))
}

#[derive(Debug)]
pub struct ResultView {
    pub result: QueryResult,
    /// Unique per result; the UI salts table ids with it.
    pub id: u64,
    /// Column widths fitted to the contents; filled in lazily by the UI because
    /// measuring text needs fonts.
    pub col_widths: Option<Vec<f32>>,
    /// Current page, 0-based.
    pub page: usize,
}

impl ResultView {
    /// Number of pages; an empty result still has one (empty) page.
    pub fn page_count(&self, page_size: usize) -> usize {
        self.result.data.rows.len().div_ceil(page_size.max(1)).max(1)
    }

    /// Indices of the rows on the current page.
    pub fn page_rows(&self, page_size: usize) -> Range<usize> {
        let len = self.result.data.rows.len();
        let start = (self.page * page_size).min(len);
        start..(start + page_size).min(len)
    }
}

#[derive(Debug)]
pub struct AppState {
    pub screen: Screen,
}

impl AppState {
    /// Starts in the editor when settings were saved earlier, otherwise in setup.
    pub fn new(saved: Option<Config>) -> (Self, Vec<Effect>) {
        match saved {
            Some(config) => {
                let mut editor = EditorState::new(config.clone());
                editor.loading_sources = true;
                let effects =
                    vec![Effect::LoadDataSources(config), Effect::LoadVariables, Effect::LoadHistory];
                (Self { screen: Screen::Editor(Box::new(editor)) }, effects)
            }
            None => (Self { screen: Screen::Setup(SetupState::default()) }, Vec::new()),
        }
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match (&mut self.screen, event) {
            (Screen::Setup(setup), Event::Connect) if setup.can_connect() => {
                setup.connecting = true;
                setup.error = None;
                vec![Effect::LoadDataSources(setup.config())]
            }
            // Ignore late answers, e.g. a reload that finished after Disconnect.
            (Screen::Setup(setup), Event::DataSourcesLoaded(res)) if setup.connecting => {
                setup.connecting = false;
                match res {
                    Ok(sources) => {
                        let config = setup.config();
                        let mut editor = EditorState::new(config.clone());
                        let mut effects =
                            vec![Effect::SaveConfig(config), Effect::LoadVariables, Effect::LoadHistory];
                        effects.extend(editor.set_data_sources(sources));
                        self.screen = Screen::Editor(Box::new(editor));
                        effects
                    }
                    Err(e) => {
                        setup.error = Some(e);
                        Vec::new()
                    }
                }
            }
            (Screen::Setup(setup), Event::ConfigError(e)) => {
                setup.error = Some(e);
                Vec::new()
            }

            (Screen::Editor(ed), Event::Execute) if ed.can_execute() => {
                let save = ed.record_history();
                let mut effects = ed.start(Target::Query);
                effects.push(save);
                effects
            }
            (Screen::Editor(ed), Event::RunVariable(id)) if ed.can_run_variable() => {
                let Some(var) = ed.variables.iter_mut().find(|v| v.id == id && v.source().is_some()) else {
                    return Vec::new();
                };
                var.run = None;
                ed.start(Target::Variable(id))
            }
            (Screen::Editor(ed), Event::VariableFinished { id, result }) => {
                let Some(var) = ed.variables.iter_mut().find(|v| v.id == id) else { return Vec::new() };
                let Some(Run::Running { ran }) = var.run.take() else { return Vec::new() };
                var.run = Some(match result {
                    Ok(r) => Run::Done { ran, value: vars::value_of(&r), rows: r.data.rows.len() },
                    Err(e) => Run::Failed(e),
                });
                ed.advance()
            }
            (Screen::Editor(ed), Event::VariablesLoaded(res)) => {
                match res {
                    Ok(loaded) => {
                        ed.set_variables(loaded);
                        ed.show_variables = !ed.variables.is_empty();
                    }
                    Err(e) => ed.error = Some(format!("Could not load variables: {e}")),
                }
                Vec::new()
            }
            (Screen::Editor(ed), Event::HistoryLoaded(res)) => {
                match res {
                    Ok(loaded) => {
                        ed.history = loaded;
                        ed.history.truncate(history::LIMIT);
                    }
                    Err(e) => ed.error = Some(format!("Could not load history: {e}")),
                }
                Vec::new()
            }
            (Screen::Editor(ed), Event::ToggleHistory) => {
                ed.show_history = !ed.show_history;
                Vec::new()
            }
            // Not mid-run, which may be waiting for the variables it would replace.
            (Screen::Editor(ed), Event::RestoreHistory(i)) if !ed.running => {
                let Some(entry) = ed.history.get(i).cloned() else { return Vec::new() };
                ed.sql = entry.sql;
                ed.set_variables(entry.variables);
                ed.show_variables |= !ed.variables.is_empty();
                ed.error = None;
                let mut effects = ed.save_variables();
                if ed.data_sources.iter().any(|s| s.id == entry.data_source_id) {
                    ed.selected_source = Some(entry.data_source_id);
                    effects.extend(ed.load_schema());
                } else {
                    ed.error = Some("This query's data source no longer exists; pick another".into());
                }
                effects
            }
            (Screen::Editor(ed), Event::ClearHistory) => {
                ed.history.clear();
                vec![Effect::SaveHistory(Vec::new())]
            }
            (Screen::Editor(ed), Event::ToggleVariables) => {
                ed.show_variables = !ed.show_variables;
                Vec::new()
            }
            (Screen::Editor(ed), Event::AddVariable(kind)) => {
                let name = (1..)
                    .map(|n| format!("var{n}"))
                    .find(|name| ed.variables.iter().all(|v| &v.name != name))
                    .unwrap_or_default();
                let def = match kind {
                    VariableKind::Value => Definition::Value { value: String::new() },
                    VariableKind::Query => Definition::Query {
                        data_source_id: ed.selected_source.unwrap_or_default(),
                        sql: String::new(),
                    },
                };
                ed.add_variable(name, def);
                ed.show_variables = true;
                ed.save_variables()
            }
            // Not mid-run, which may be waiting for it.
            (Screen::Editor(ed), Event::RemoveVariable(id)) if ed.can_run_variable() => {
                ed.variables.retain(|v| v.id != id);
                ed.save_variables()
            }
            (Screen::Editor(ed), Event::VariablesEdited) => ed.save_variables(),
            (Screen::Editor(ed), Event::ReloadDataSources) if !ed.loading_sources => {
                ed.loading_sources = true;
                ed.error = None;
                // Refetch schemas too, once the sources are back.
                ed.schemas.clear();
                vec![Effect::LoadDataSources(ed.config.clone())]
            }
            (Screen::Editor(ed), Event::DataSourcesLoaded(res)) => {
                ed.loading_sources = false;
                match res {
                    Ok(sources) => ed.set_data_sources(sources),
                    Err(e) => {
                        ed.error = Some(format!("Failed to load data sources: {e}"));
                        Vec::new()
                    }
                }
            }
            (Screen::Editor(ed), Event::SelectSource(id)) if ed.data_sources.iter().any(|s| s.id == id) => {
                ed.selected_source = Some(id);
                ed.load_schema()
            }
            // Ignore answers to requests made before a reload.
            (Screen::Editor(ed), Event::SchemaLoaded { data_source_id, result })
                if ed.schemas.get(&data_source_id) == Some(&Schema::Loading) =>
            {
                match result {
                    Ok(tables) => {
                        ed.schemas.insert(data_source_id, Schema::Loaded(tables));
                    }
                    Err(e) => {
                        // Forget it so selecting the source again retries.
                        ed.schemas.remove(&data_source_id);
                        if ed.selected_source == Some(data_source_id) {
                            ed.error = Some(format!("Failed to load schema: {e}"));
                        }
                    }
                }
                Vec::new()
            }
            (Screen::Editor(ed), Event::QueryFinished(res)) if ed.running => {
                ed.running = false;
                match res {
                    Ok(result) => {
                        ed.results_shown += 1;
                        ed.result =
                            Some(ResultView { result, id: ed.results_shown, col_widths: None, page: 0 });
                    }
                    Err(e) => ed.error = Some(e),
                }
                Vec::new()
            }
            (Screen::Editor(ed), Event::ShowPage(page)) => {
                if let Some(view) = &mut ed.result {
                    view.page = page.min(view.page_count(ed.page_size) - 1);
                }
                Vec::new()
            }
            (Screen::Editor(ed), Event::SetPageSize(size)) if size > 0 => {
                if let Some(view) = &mut ed.result {
                    view.page = view.page_rows(ed.page_size).start / size;
                }
                ed.page_size = size;
                Vec::new()
            }
            (Screen::Editor(ed), Event::CopyPageMarkdown) => match &ed.result {
                Some(view) => {
                    let rows = view.page_rows(ed.page_size);
                    let n = rows.len();
                    ed.notice = Some(format!("Copied {n} {} as Markdown", plural(n, "row")));
                    vec![Effect::CopyToClipboard(export::markdown_table(&view.result, rows))]
                }
                None => Vec::new(),
            },
            (Screen::Editor(ed), Event::ExportCsv) => match &ed.result {
                Some(view) => vec![Effect::SaveCsv {
                    file_name: "query_result.csv".into(),
                    contents: export::csv(&view.result),
                }],
                None => Vec::new(),
            },
            (Screen::Editor(ed), Event::CsvSaved(res)) => {
                match res {
                    Ok(Some(path)) => ed.notice = Some(format!("Saved {}", path.display())),
                    Ok(None) => {}
                    Err(e) => ed.error = Some(format!("Could not export CSV: {e}")),
                }
                Vec::new()
            }
            (Screen::Editor(ed), Event::ConfigError(e)) => {
                ed.error = Some(e);
                Vec::new()
            }
            (Screen::Editor(ed), Event::Disconnect) => {
                // Keep the host prefilled so reconnecting is quick.
                let host = ed.config.host.clone();
                self.screen = Screen::Setup(SetupState { host, ..Default::default() });
                vec![Effect::ClearConfig]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Column, QueryData, TableColumn};

    fn config() -> Config {
        Config { host: "https://redash.example.com".into(), api_key: "key".into() }
    }

    fn sources() -> Vec<DataSource> {
        vec![
            DataSource { id: 1, name: "A".into(), kind: "pg".into() },
            DataSource { id: 2, name: "B".into(), kind: "mysql".into() },
        ]
    }

    fn result() -> QueryResult {
        result_with_rows(0)
    }

    fn result_with_rows(n: usize) -> QueryResult {
        let rows = (0..n).map(|i| [("n".to_string(), i.into())].into_iter().collect()).collect();
        QueryResult { data: QueryData { columns: vec![Column { name: "n".into() }], rows }, runtime: 0.1 }
    }

    /// Editor showing a result with `n` rows.
    fn editor_with_result(n: usize) -> AppState {
        let mut state = connected_editor();
        state.update(Event::Execute);
        state.update(Event::QueryFinished(Ok(result_with_rows(n))));
        state
    }

    fn page_rows(state: &AppState) -> Range<usize> {
        let ed = editor(state);
        ed.result.as_ref().map(|r| r.page_rows(ed.page_size)).unwrap_or_default()
    }

    fn editor(state: &AppState) -> &EditorState {
        match &state.screen {
            Screen::Editor(ed) => ed,
            Screen::Setup(_) => panic!("expected editor screen"),
        }
    }

    fn setup(state: &AppState) -> &SetupState {
        match &state.screen {
            Screen::Setup(s) => s,
            Screen::Editor(_) => panic!("expected setup screen"),
        }
    }

    /// Setup screen with credentials typed in.
    fn filled_setup() -> AppState {
        let (mut state, _) = AppState::new(None);
        if let Screen::Setup(s) = &mut state.screen {
            s.host = "  https://redash.example.com ".into();
            s.api_key = "key".into();
        }
        state
    }

    fn connected_editor() -> AppState {
        let (mut state, _) = AppState::new(Some(config()));
        state.update(Event::DataSourcesLoaded(Ok(sources())));
        state
    }

    #[test]
    fn starts_in_setup_without_saved_config() {
        let (state, effects) = AppState::new(None);
        assert!(!setup(&state).can_connect());
        assert!(effects.is_empty());
    }

    #[test]
    fn starts_in_editor_and_loads_sources_with_saved_config() {
        let (state, effects) = AppState::new(Some(config()));
        assert!(editor(&state).loading_sources);
        assert_eq!(effects, [Effect::LoadDataSources(config()), Effect::LoadVariables, Effect::LoadHistory]);
    }

    fn load_schema(id: i64) -> Effect {
        Effect::LoadSchema { config: config(), data_source_id: id }
    }

    fn tables() -> Vec<Table> {
        vec![Table { name: "users".into(), columns: vec![TableColumn { name: "id".into(), kind: None }] }]
    }

    #[test]
    fn connect_success_saves_trimmed_config_and_opens_editor() {
        let mut state = filled_setup();
        assert_eq!(state.update(Event::Connect), [Effect::LoadDataSources(config())]);
        assert!(setup(&state).connecting);
        assert!(state.update(Event::Connect).is_empty(), "no double connect");

        let effects = state.update(Event::DataSourcesLoaded(Ok(sources())));
        assert_eq!(
            effects,
            [Effect::SaveConfig(config()), Effect::LoadVariables, Effect::LoadHistory, load_schema(1)]
        );
        let ed = editor(&state);
        assert_eq!(ed.selected_source, Some(1));
        assert_eq!(ed.data_sources.len(), 2);
    }

    #[test]
    fn connect_failure_shows_error_and_stays_on_setup() {
        let mut state = filled_setup();
        state.update(Event::Connect);
        state.update(Event::DataSourcesLoaded(Err("HTTP 403".into())));
        let s = setup(&state);
        assert_eq!(s.error.as_deref(), Some("HTTP 403"));
        assert!(s.can_connect());
    }

    #[test]
    fn late_data_sources_do_not_log_in_after_disconnect() {
        let mut state = connected_editor();
        state.update(Event::ReloadDataSources);
        assert_eq!(state.update(Event::Disconnect), [Effect::ClearConfig]);
        assert_eq!(setup(&state).host, config().host);

        assert!(state.update(Event::DataSourcesLoaded(Ok(sources()))).is_empty());
        setup(&state);
    }

    #[test]
    fn execute_runs_sql_on_selected_source() {
        let mut state = connected_editor();
        if let Screen::Editor(ed) = &mut state.screen {
            ed.selected_source = Some(2);
            ed.sql = "select 42".into();
        }
        edit(&mut state, |ed| ed.clock = || 1000);
        let effects = state.update(Event::Execute);
        let recorded = Entry::new(1000, 2, "select 42", &[]);
        assert_eq!(
            effects,
            [
                Effect::Execute { config: config(), data_source_id: 2, sql: "select 42".into() },
                Effect::SaveHistory(vec![recorded])
            ]
        );
        assert!(state.update(Event::Execute).is_empty(), "already running");

        state.update(Event::QueryFinished(Ok(result())));
        let ed = editor(&state);
        assert!(!ed.running);
        assert_eq!(ed.result.as_ref().map(|r| r.id), Some(1));
    }

    #[test]
    fn execute_needs_source_and_sql() {
        let (mut state, _) = AppState::new(Some(config()));
        assert!(state.update(Event::Execute).is_empty(), "no data source yet");

        let mut state = connected_editor();
        if let Screen::Editor(ed) = &mut state.screen {
            ed.sql = "   ".into();
        }
        assert!(state.update(Event::Execute).is_empty(), "blank SQL");
        if let Screen::Editor(ed) = &mut state.screen {
            ed.sql = "-- SELECT 1\n/* old */".into();
        }
        assert!(state.update(Event::Execute).is_empty(), "only comments");
    }

    #[test]
    fn has_statement_ignores_comments() {
        assert!(!has_statement(""));
        assert!(!has_statement("  -- a\n\t/* b\n c */ -- d"));
        assert!(!has_statement("/* unterminated"));
        assert!(has_statement("-- a\nSELECT 1"));
        assert!(has_statement("/* a */ SELECT 1 -- b"));
        assert!(has_statement("SELECT '--'"));
    }

    #[test]
    fn query_error_keeps_previous_result() {
        let mut state = connected_editor();
        state.update(Event::Execute);
        state.update(Event::QueryFinished(Ok(result())));
        state.update(Event::Execute);
        state.update(Event::QueryFinished(Err("syntax error".into())));
        let ed = editor(&state);
        assert_eq!(ed.error.as_deref(), Some("syntax error"));
        assert!(ed.result.is_some());
    }

    #[test]
    fn reload_keeps_selection_when_source_still_exists() {
        let mut state = connected_editor();
        if let Screen::Editor(ed) = &mut state.screen {
            ed.selected_source = Some(2);
        }
        state.update(Event::ReloadDataSources);
        state.update(Event::DataSourcesLoaded(Ok(sources())));
        assert_eq!(editor(&state).selected_source, Some(2));

        state.update(Event::ReloadDataSources);
        state.update(Event::DataSourcesLoaded(Ok(sources()[..1].to_vec())));
        assert_eq!(editor(&state).selected_source, Some(1));
    }

    #[test]
    fn loads_schema_of_each_selected_source_once() {
        let (mut state, _) = AppState::new(Some(config()));
        assert_eq!(state.update(Event::DataSourcesLoaded(Ok(sources()))), [load_schema(1)]);
        assert!(editor(&state).tables().is_empty(), "still loading");
        state.update(Event::SchemaLoaded { data_source_id: 1, result: Ok(tables()) });
        assert_eq!(editor(&state).tables(), tables());

        assert_eq!(state.update(Event::SelectSource(2)), [load_schema(2)]);
        assert!(editor(&state).tables().is_empty());
        assert!(state.update(Event::SelectSource(1)).is_empty(), "already loaded");
        assert_eq!(editor(&state).tables(), tables());
        assert!(state.update(Event::SelectSource(99)).is_empty(), "unknown source");
        assert_eq!(editor(&state).selected_source, Some(1));
    }

    #[test]
    fn schema_failure_shows_error_and_retries_on_next_select() {
        let mut state = connected_editor();
        state.update(Event::SelectSource(2));
        state.update(Event::SchemaLoaded { data_source_id: 1, result: Err("timeout".into()) });
        assert_eq!(editor(&state).error, None, "not the selected source");
        state.update(Event::SchemaLoaded { data_source_id: 2, result: Err("timeout".into()) });
        assert_eq!(editor(&state).error.as_deref(), Some("Failed to load schema: timeout"));

        state.update(Event::SelectSource(1));
        assert_eq!(state.update(Event::SelectSource(2)), [load_schema(2)]);
    }

    #[test]
    fn reload_refetches_schema_and_ignores_stale_answers() {
        let mut state = connected_editor();
        state.update(Event::SchemaLoaded { data_source_id: 1, result: Ok(tables()) });
        state.update(Event::ReloadDataSources);
        assert!(editor(&state).tables().is_empty());
        state.update(Event::SchemaLoaded { data_source_id: 1, result: Ok(tables()) });
        assert!(editor(&state).tables().is_empty(), "answer to a request made before the reload");

        assert_eq!(state.update(Event::DataSourcesLoaded(Ok(sources()))), [load_schema(1)]);
        state.update(Event::SchemaLoaded { data_source_id: 1, result: Ok(tables()) });
        assert_eq!(editor(&state).tables(), tables());
    }

    #[test]
    fn pages_through_result() {
        let mut state = editor_with_result(60);
        let ed = editor(&state);
        assert_eq!(ed.page_size, 25);
        assert_eq!(ed.result.as_ref().map(|r| r.page_count(ed.page_size)), Some(3));
        assert_eq!(page_rows(&state), 0..25);

        state.update(Event::ShowPage(1));
        assert_eq!(page_rows(&state), 25..50);
        state.update(Event::ShowPage(2));
        assert_eq!(page_rows(&state), 50..60, "last page is partial");
        state.update(Event::ShowPage(99));
        assert_eq!(page_rows(&state), 50..60, "clamped to the last page");
    }

    #[test]
    fn empty_result_has_one_empty_page() {
        let mut state = editor_with_result(0);
        state.update(Event::ShowPage(1));
        let ed = editor(&state);
        assert_eq!(ed.result.as_ref().map(|r| (r.page, r.page_count(ed.page_size))), Some((0, 1)));
        assert_eq!(page_rows(&state), 0..0);
    }

    #[test]
    fn page_size_change_keeps_first_visible_row() {
        let mut state = editor_with_result(300);
        state.update(Event::ShowPage(5)); // rows 125..150
        state.update(Event::SetPageSize(100));
        assert_eq!(page_rows(&state), 100..200);
        state.update(Event::SetPageSize(0));
        assert_eq!(editor(&state).page_size, 100, "zero is ignored");
    }

    #[test]
    fn new_result_starts_on_first_page_with_same_page_size() {
        let mut state = editor_with_result(300);
        state.update(Event::SetPageSize(50));
        state.update(Event::ShowPage(3));
        state.update(Event::Execute);
        state.update(Event::QueryFinished(Ok(result_with_rows(300))));
        assert_eq!(editor(&state).page_size, 50);
        assert_eq!(page_rows(&state), 0..50);
    }

    #[test]
    fn copies_current_page_as_markdown() {
        let mut state = connected_editor();
        assert!(state.update(Event::CopyPageMarkdown).is_empty(), "no result yet");

        let mut state = editor_with_result(30);
        state.update(Event::ShowPage(1));
        let effects = state.update(Event::CopyPageMarkdown);
        let rows: String = (25..30).map(|i| format!("| {i} |\n")).collect();
        assert_eq!(effects, [Effect::CopyToClipboard(format!("| n |\n| --- |\n{rows}"))]);
        assert_eq!(editor(&state).notice.as_deref(), Some("Copied 5 rows as Markdown"));

        state.update(Event::Execute);
        assert_eq!(editor(&state).notice, None, "cleared by the next run");
    }

    #[test]
    fn exports_whole_result_as_csv() {
        let mut state = connected_editor();
        assert!(state.update(Event::ExportCsv).is_empty(), "no result yet");

        let mut state = editor_with_result(30);
        let effects = state.update(Event::ExportCsv);
        let rows: String = (0..30).map(|i| format!("{i}\r\n")).collect();
        assert_eq!(
            effects,
            [Effect::SaveCsv { file_name: "query_result.csv".into(), contents: format!("n\r\n{rows}") }]
        );

        state.update(Event::CsvSaved(Ok(None)));
        assert_eq!((&editor(&state).notice, &editor(&state).error), (&None, &None), "cancelled");
        state.update(Event::CsvSaved(Ok(Some("/tmp/q.csv".into()))));
        assert_eq!(editor(&state).notice.as_deref(), Some("Saved /tmp/q.csv"));
        state.update(Event::CsvSaved(Err("disk full".into())));
        assert_eq!(editor(&state).error.as_deref(), Some("Could not export CSV: disk full"));
    }

    #[test]
    fn toggle_comment_on_cursor_line() {
        let (text, sel) = toggle_line_comment("SELECT 1\nFROM t", 3..3);
        assert_eq!((text.as_str(), sel), ("-- SELECT 1\nFROM t", 6..6));
        let (text, sel) = toggle_line_comment(&text, 6..6);
        assert_eq!((text.as_str(), sel), ("SELECT 1\nFROM t", 3..3));
    }

    #[test]
    fn toggle_comment_on_selected_lines_keeps_indentation() {
        let sql = "SELECT a,\n  b\n\n  c\nFROM t";
        // Select from inside "b" to the start of "FROM": lines 1..=3 (the blank one is skipped).
        let end = sql.find("FROM").unwrap();
        let (text, sel) = toggle_line_comment(sql, 12..end);
        assert_eq!(text, "SELECT a,\n  -- b\n\n  -- c\nFROM t");
        assert_eq!(sel, 15..end + 6);
        let (text, _) = toggle_line_comment(&text, sel);
        assert_eq!(text, sql);
    }

    #[test]
    fn toggle_comment_comments_all_when_some_lines_are_not() {
        let (text, _) = toggle_line_comment("--a\nb", 0..5);
        assert_eq!(text, "-- --a\n-- b");
        let (text, _) = toggle_line_comment("--a\n-- b", 0..5);
        assert_eq!(text, "a\nb");
    }

    #[test]
    fn toggle_comment_on_empty_text() {
        assert_eq!(toggle_line_comment("", 0..0), ("-- ".to_string(), 3..3));
    }

    fn edit(state: &mut AppState, f: impl FnOnce(&mut EditorState)) {
        if let Screen::Editor(ed) = &mut state.screen {
            f(ed);
        }
    }

    fn value_var(name: &str, value: &str) -> Variable {
        Variable { id: 0, name: name.into(), def: Definition::Value { value: value.into() }, run: None }
    }

    fn query_var(name: &str, source: i64, sql: &str) -> Variable {
        Variable {
            id: 0,
            name: name.into(),
            def: Definition::Query { data_source_id: source, sql: sql.into() },
            run: None,
        }
    }

    /// Connected editor with these variables loaded; ids are 1, 2, … in order.
    fn editor_with_vars(vars: Vec<Variable>, sql: &str) -> AppState {
        let mut state = connected_editor();
        state.update(Event::VariablesLoaded(Ok(vars)));
        edit(&mut state, |ed| ed.sql = sql.into());
        state
    }

    /// Executes the query, leaving out the history save.
    fn run(state: &mut AppState) -> Vec<Effect> {
        let mut effects = state.update(Event::Execute);
        effects.retain(|e| !matches!(e, Effect::SaveHistory(_)));
        effects
    }

    fn execute(source: i64, sql: &str) -> Effect {
        Effect::Execute { config: config(), data_source_id: source, sql: sql.into() }
    }

    fn run_var(id: u64, source: i64, sql: &str) -> Effect {
        Effect::RunVariable { config: config(), id, data_source_id: source, sql: sql.into() }
    }

    fn ids_result(ids: &[i64]) -> QueryResult {
        let rows = ids.iter().map(|&i| [("id".to_string(), i.into())].into_iter().collect()).collect();
        QueryResult { data: QueryData { columns: vec![Column { name: "id".into() }], rows }, runtime: 0.1 }
    }

    #[test]
    fn execute_substitutes_value_variables() {
        let vars = vec![value_var("start", "2026-01-01"), value_var("n", "10")];
        let mut state = editor_with_vars(vars, "SELECT * WHERE d > '{{ start }}' LIMIT {{n}} -- {{ gone }}");
        assert!(editor(&state).show_variables, "shown when there are some");
        assert_eq!(run(&mut state), [execute(1, "SELECT * WHERE d > '2026-01-01' LIMIT 10 -- {{ gone }}")]);
    }

    #[test]
    fn unknown_variable_is_an_error() {
        let mut state = editor_with_vars(vec![], "SELECT {{ nope }}");
        assert!(run(&mut state).is_empty());
        let ed = editor(&state);
        assert_eq!(ed.error.as_deref(), Some("Unknown variable {{ nope }}: add it in Variables"));
        assert!(!ed.running);
    }

    #[test]
    fn query_variables_run_first_then_the_query() {
        let vars =
            vec![query_var("ids", 2, "select id"), query_var("other", 1, "select 2"), value_var("n", "5")];
        let mut state = editor_with_vars(vars, "SELECT {{ids}} + {{ other }}, {{ ids }} LIMIT {{n}}");
        assert_eq!(run(&mut state), [run_var(1, 2, "select id"), run_var(2, 1, "select 2")]);
        assert!(editor(&state).running);
        assert!(state.update(Event::Execute).is_empty(), "already running");

        assert!(state.update(Event::VariableFinished { id: 1, result: Ok(ids_result(&[3, 4])) }).is_empty());
        let effects = state.update(Event::VariableFinished { id: 2, result: Ok(ids_result(&[])) });
        assert_eq!(effects, [execute(1, "SELECT 3, 4 + NULL, 3, 4 LIMIT 5")]);

        state.update(Event::QueryFinished(Ok(result())));
        assert!(!editor(&state).running);
        // Values are kept until the variable's definition changes.
        assert_eq!(run(&mut state), [execute(1, "SELECT 3, 4 + NULL, 3, 4 LIMIT 5")]);
        state.update(Event::QueryFinished(Ok(result())));
        edit(&mut state, |ed| {
            ed.variables[0].def = Definition::Query { data_source_id: 2, sql: "select 9".into() }
        });
        assert_eq!(run(&mut state), [run_var(1, 2, "select 9")]);
    }

    #[test]
    fn variables_can_use_other_variables() {
        let vars = vec![query_var("users", 1, "select id where d > {{ start }}"), value_var("start", "'x'")];
        let mut state = editor_with_vars(vars, "SELECT {{ users }}");
        assert_eq!(run(&mut state), [run_var(1, 1, "select id where d > 'x'")]);
        let effects = state.update(Event::VariableFinished { id: 1, result: Ok(ids_result(&[7])) });
        assert_eq!(effects, [execute(1, "SELECT 7")]);
    }

    #[test]
    fn variable_cycles_are_errors() {
        let vars = vec![query_var("a", 1, "select {{ b }}"), query_var("b", 1, "select {{a}}")];
        let mut state = editor_with_vars(vars, "SELECT {{ a }}");
        assert!(run(&mut state).is_empty());
        assert_eq!(editor(&state).error.as_deref(), Some("Variable a refers to itself"));
        assert!(!editor(&state).running);
    }

    #[test]
    fn failed_variable_stops_the_run_and_is_retried_next_time() {
        let vars = vec![query_var("a", 1, "select fail"), query_var("b", 1, "select 2")];
        let mut state = editor_with_vars(vars, "SELECT {{ a }}, {{ b }}");
        state.update(Event::Execute);
        let effects = state.update(Event::VariableFinished { id: 1, result: Err("syntax error".into()) });
        assert!(effects.is_empty());
        let ed = editor(&state);
        assert_eq!(ed.error.as_deref(), Some("Variable a: syntax error"));
        assert!(!ed.running);

        // b is still running; the retry waits for it instead of starting it again.
        assert_eq!(run(&mut state), [run_var(1, 1, "select fail")]);
        assert!(state.update(Event::VariableFinished { id: 2, result: Ok(ids_result(&[2])) }).is_empty());
        let effects = state.update(Event::VariableFinished { id: 1, result: Ok(ids_result(&[1])) });
        assert_eq!(effects, [execute(1, "SELECT 1, 2")]);
    }

    #[test]
    fn run_variable_refreshes_its_value() {
        let vars = vec![query_var("ids", 2, "select {{ n }}"), value_var("n", "1")];
        let mut state = editor_with_vars(vars, "SELECT 1");
        assert!(state.update(Event::RunVariable(2)).is_empty(), "plain values don't run");
        assert_eq!(state.update(Event::RunVariable(1)), [run_var(1, 2, "select 1")]);
        assert!(editor(&state).running);
        assert!(state.update(Event::VariableFinished { id: 1, result: Ok(ids_result(&[1])) }).is_empty());
        let ed = editor(&state);
        assert!(!ed.running);
        assert_eq!(ed.variables[0].fresh_value(), Some("1"));

        edit(&mut state, |ed| ed.variables[1].def = Definition::Value { value: "2".into() });
        assert_eq!(
            state.update(Event::RunVariable(1)),
            [run_var(1, 2, "select 2")],
            "reruns even when fresh"
        );
    }

    #[test]
    fn adds_edits_and_removes_variables() {
        let mut state = editor_with_vars(vec![value_var("var1", "")], "SELECT 1");
        edit(&mut state, |ed| ed.selected_source = Some(2));
        let effects = state.update(Event::AddVariable(VariableKind::Query));
        let added = query_var("var2", 2, "");
        let ed = editor(&state);
        assert_eq!((ed.variables[1].name.as_str(), &ed.variables[1].def), ("var2", &added.def));
        assert_eq!(effects, [Effect::SaveVariables(ed.variables.clone())]);

        assert_eq!(
            state.update(Event::VariablesEdited),
            [Effect::SaveVariables(editor(&state).variables.clone())]
        );
        state.update(Event::RemoveVariable(1));
        assert_eq!(editor(&state).variables.iter().map(|v| v.id).collect::<Vec<_>>(), [2]);

        state.update(Event::ToggleVariables);
        assert!(!editor(&state).show_variables);
    }

    #[test]
    fn no_removing_variables_mid_run() {
        let mut state = editor_with_vars(vec![query_var("a", 1, "select 1")], "SELECT {{ a }}");
        state.update(Event::Execute);
        assert!(state.update(Event::RemoveVariable(1)).is_empty());
        assert_eq!(editor(&state).variables.len(), 1);
    }

    #[test]
    fn each_execution_records_a_snapshot() {
        let mut state = editor_with_vars(vec![value_var("n", "1")], "SELECT {{ n }}");
        edit(&mut state, |ed| ed.clock = || 7);
        run(&mut state);
        state.update(Event::QueryFinished(Ok(result())));
        edit(&mut state, |ed| {
            ed.sql = "SELECT 2".into();
            ed.selected_source = Some(2);
            ed.variables[0].def = Definition::Value { value: "2".into() };
        });
        let effects = state.update(Event::Execute);
        let history = &editor(&state).history;
        assert_eq!(effects.last(), Some(&Effect::SaveHistory(history.clone())));
        assert_eq!(history.len(), 2);
        assert_eq!((history[0].sql.as_str(), history[0].data_source_id), ("SELECT 2", 2));
        assert_eq!(history[1], Entry::new(7, 1, "SELECT {{ n }}", &[value_var("n", "1")]));
    }

    #[test]
    fn failed_runs_are_recorded_too() {
        let mut state = editor_with_vars(vec![], "SELECT {{ nope }}");
        let effects = state.update(Event::Execute);
        assert!(matches!(effects.as_slice(), [Effect::SaveHistory(h)] if h.len() == 1));
    }

    #[test]
    fn restores_a_snapshot() {
        let mut state = editor_with_vars(vec![value_var("n", "1")], "SELECT {{ n }}");
        edit(&mut state, |ed| ed.selected_source = Some(2));
        run(&mut state);
        state.update(Event::QueryFinished(Ok(result())));
        edit(&mut state, |ed| {
            ed.sql = "other".into();
            ed.selected_source = Some(1);
            ed.variables.clear();
            ed.show_variables = false;
        });

        let effects = state.update(Event::RestoreHistory(0));
        let ed = editor(&state);
        assert_eq!((ed.sql.as_str(), ed.selected_source), ("SELECT {{ n }}", Some(2)));
        assert_eq!(ed.variables.iter().map(|v| (v.id, v.name.as_str())).collect::<Vec<_>>(), [(2, "n")]);
        assert!(ed.show_variables);
        assert_eq!(effects, [Effect::SaveVariables(ed.variables.clone()), load_schema(2)]);
        assert!(state.update(Event::RestoreHistory(5)).is_empty(), "no such entry");
    }

    #[test]
    fn restoring_keeps_source_when_it_is_gone_and_waits_for_runs() {
        let mut state = connected_editor();
        edit(&mut state, |ed| ed.history = vec![Entry::new(0, 9, "select 9", &[])]);
        state.update(Event::RestoreHistory(0));
        let ed = editor(&state);
        assert_eq!((ed.sql.as_str(), ed.selected_source), ("select 9", Some(1)));
        assert!(ed.error.is_some());

        edit(&mut state, |ed| ed.sql = "select 1".into());
        state.update(Event::Execute);
        assert!(state.update(Event::RestoreHistory(0)).is_empty(), "not mid-run");
        assert_eq!(editor(&state).sql, "select 1");
    }

    #[test]
    fn loads_toggles_and_clears_history() {
        let mut state = connected_editor();
        assert!(editor(&state).show_history, "shown by default");
        let saved: Vec<Entry> = (0..25).map(|i| Entry::new(i, 1, "select 1", &[])).collect();
        state.update(Event::HistoryLoaded(Ok(saved)));
        assert_eq!(editor(&state).history.len(), history::LIMIT);

        state.update(Event::ToggleHistory);
        assert!(!editor(&state).show_history, "collapsed");
        assert_eq!(state.update(Event::ClearHistory), [Effect::SaveHistory(vec![])]);
        assert!(editor(&state).history.is_empty());
    }
}
