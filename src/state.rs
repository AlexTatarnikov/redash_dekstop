//! All application logic, with no UI or I/O.
//!
//! The UI turns user actions into [`Event`]s, [`AppState::update`] applies them and
//! returns [`Effect`]s (network calls, saving settings) for the runtime in `app.rs`
//! to perform. Results of those effects come back as more events. Keeping this
//! module pure means every behaviour can be covered by plain unit tests.

use crate::api::{DataSource, QueryResult};
use crate::config::Config;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    // User intents, emitted by the UI.
    Connect,
    Execute,
    ReloadDataSources,
    Disconnect,
    // Results of effects, delivered by the runtime.
    DataSourcesLoaded(Result<Vec<DataSource>, String>),
    QueryFinished(Result<QueryResult, String>),
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
    SaveConfig(Config),
    ClearConfig,
}

#[derive(Debug)]
pub enum Screen {
    Setup(SetupState),
    Editor(EditorState),
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
            results_shown: 0,
        }
    }

    pub fn can_execute(&self) -> bool {
        self.selected_source.is_some() && !self.running && !self.sql.trim().is_empty()
    }

    fn set_data_sources(&mut self, sources: Vec<DataSource>) {
        let still_exists = sources.iter().any(|s| Some(s.id) == self.selected_source);
        if !still_exists {
            self.selected_source = sources.first().map(|s| s.id);
        }
        self.data_sources = sources;
    }
}

#[derive(Debug)]
pub struct ResultView {
    pub result: QueryResult,
    /// Unique per result; the UI salts table ids with it.
    pub id: u64,
    /// Column widths fitted to the contents; filled in lazily by the UI because
    /// measuring text needs fonts.
    pub col_widths: Option<Vec<f32>>,
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
                (Self { screen: Screen::Editor(editor) }, vec![Effect::LoadDataSources(config)])
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
                        editor.set_data_sources(sources);
                        self.screen = Screen::Editor(editor);
                        vec![Effect::SaveConfig(config)]
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
                vec![Effect::LoadDataSources(ed.config.clone())]
            }
            (Screen::Editor(ed), Event::DataSourcesLoaded(res)) => {
                ed.loading_sources = false;
                match res {
                    Ok(sources) => ed.set_data_sources(sources),
                    Err(e) => ed.error = Some(format!("Failed to load data sources: {e}")),
                }
                Vec::new()
            }
            (Screen::Editor(ed), Event::QueryFinished(res)) if ed.running => {
                ed.running = false;
                match res {
                    Ok(result) => {
                        ed.results_shown += 1;
                        ed.result = Some(ResultView { result, id: ed.results_shown, col_widths: None });
                    }
                    Err(e) => ed.error = Some(e),
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
    use crate::api::{Column, QueryData};

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
        QueryResult {
            data: QueryData { columns: vec![Column { name: "n".into() }], rows: Vec::new() },
            runtime: 0.1,
        }
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

    #[test]
    fn connect_success_saves_trimmed_config_and_opens_editor() {
        let mut state = filled_setup();
        assert_eq!(state.update(Event::Connect), [Effect::LoadDataSources(config())]);
        assert!(setup(&state).connecting);
        assert!(state.update(Event::Connect).is_empty(), "no double connect");

        let effects = state.update(Event::DataSourcesLoaded(Ok(sources())));
        assert_eq!(effects, [Effect::SaveConfig(config())]);
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
}
