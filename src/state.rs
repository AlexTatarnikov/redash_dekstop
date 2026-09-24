//! All application logic, with no UI or I/O.
//!
//! The UI turns user actions into [`Event`]s, [`AppState::update`] applies them and
//! returns [`Effect`]s (network calls, saving settings) for the runtime in `app.rs`
//! to perform. Results of those effects come back as more events. Keeping this
//! module pure means every behaviour can be covered by plain unit tests.

use std::collections::HashMap;
use std::ops::Range;

use crate::api::{DataSource, QueryResult, Table};
use crate::config::Config;

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
    // Results of effects, delivered by the runtime.
    DataSourcesLoaded(Result<Vec<DataSource>, String>),
    QueryFinished(Result<QueryResult, String>),
    SchemaLoaded {
        data_source_id: i64,
        result: Result<Vec<Table>, String>,
    },
    ConfigError(String),
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
    /// Rows per page; kept across results.
    pub page_size: usize,
    /// Per data source; fetched when a source is first selected, refetched on reload.
    pub schemas: HashMap<i64, Schema>,
    /// Counts results so each one gets fresh table state in the UI.
    results_shown: u64,
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
            page_size: PAGE_SIZES[0],
            schemas: HashMap::new(),
            results_shown: 0,
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
                (Self { screen: Screen::Editor(Box::new(editor)) }, vec![Effect::LoadDataSources(config)])
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
                        let mut effects = vec![Effect::SaveConfig(config)];
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
                ed.running = true;
                ed.error = None;
                vec![Effect::Execute {
                    config: ed.config.clone(),
                    data_source_id: ed.selected_source.unwrap_or_default(),
                    sql: ed.sql.clone(),
                }]
            }
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
        assert_eq!(effects, [Effect::LoadDataSources(config())]);
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
        assert_eq!(effects, [Effect::SaveConfig(config()), load_schema(1)]);
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
        let effects = state.update(Event::Execute);
        assert_eq!(
            effects,
            [Effect::Execute { config: config(), data_source_id: 2, sql: "select 42".into() }]
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
}
