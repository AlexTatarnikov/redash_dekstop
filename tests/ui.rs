//! End-to-end UI tests: drive the real app headlessly (no window) against the
//! in-process mock Redash. Screenshots are compared with `tests/snapshots/*.png`;
//! on mismatch kittest writes `*.new.png` and `*.diff.png` next to them.
//! Accept intended visual changes with `UPDATE_SNAPSHOTS=1 cargo test`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use redash_desktop::RedashApp;
use redash_desktop::config::{Config, ConfigStore};
use redash_desktop::mock::{MOCK_API_KEY, MockRedash};
use redash_desktop::state::Screen;
use redash_desktop::vars::{Definition, Variable};

fn harness(app: RedashApp) -> Harness<'static, RedashApp> {
    Harness::builder()
        .with_size([1100.0, 750.0])
        .wgpu()
        .build_ui_state(|ui, app: &mut RedashApp| app.show(ui), app)
}

/// Steps frames until `done` holds; network calls run on background threads.
fn wait_for(h: &mut Harness<'_, RedashApp>, what: &str, done: impl Fn(&Harness<'_, RedashApp>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    // Settle layout (e.g. fitted column widths) before asserting or snapshotting.
    h.run_steps(3);
}

/// Snapshot with run-specific details hidden: the cursor, and the mock's random
/// port wherever the host is shown.
fn snapshot(h: &mut Harness<'_, RedashApp>, name: &str) {
    h.remove_cursor();
    h.run_steps(2);
    // The host label's width depends on the port's digits (Inter digits vary in width),
    // so mask a fixed-width area ending at its right edge (it is right-aligned).
    let mut rects: Vec<_> = h
        .query_all_by_label_contains("http://127.0.0.1")
        .map(|n| {
            let r = n.rect();
            egui::Rect::from_min_max(egui::pos2(r.max.x - 160.0, r.min.y), egui::pos2(r.max.x + 3.0, r.max.y))
        })
        .collect();
    rects.extend(h.query_by_label("API host").map(|n| n.rect()));
    for rect in rects {
        h.mask(rect);
    }
    h.snapshot(name);
}

fn sources_loaded(h: &Harness<'_, RedashApp>) -> bool {
    matches!(&h.state().state().screen, Screen::Editor(ed) if !ed.data_sources.is_empty())
}

fn variable(name: &str, def: Definition) -> Variable {
    Variable { id: 0, name: name.into(), def, run: None }
}

/// Settings in memory, with the variables `SAMPLE_SQL` uses saved.
fn store(config: Option<Config>) -> ConfigStore {
    let store = ConfigStore::memory(config);
    let vars = [
        variable("start", Definition::Value { value: "2026-09-01".into() }),
        variable("limit", Definition::Value { value: "100".into() }),
    ];
    store.save_variables(&vars).unwrap();
    store
}

fn mock_config(mock: &MockRedash) -> Option<Config> {
    Some(Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() })
}

/// Exercises every highlight colour in the editor snapshots.
const SAMPLE_SQL: &str = "-- Paying users\nSELECT id, email, plan, count(*) AS n, 'pro' AS tier\nFROM users\nWHERE mrr > 0 AND signed_up >= '{{ start }}' AND deleted IS NULL\nLIMIT {{ limit }}";

fn set_sql(h: &mut Harness<'_, RedashApp>, sql: &str) {
    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.sql = sql.into();
}

fn type_into(h: &mut Harness<'_, RedashApp>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
    h.get_by_label(label).type_text(text);
    h.run_steps(2);
}

/// Like `type_into`, but replaces the field's text.
fn replace_text(h: &mut Harness<'_, RedashApp>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(label).type_text(text);
    h.run_steps(2);
}

#[test]
fn connect_run_query_and_disconnect() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(store(None)));
    snapshot(&mut h, "setup_empty");

    type_into(&mut h, "API host", mock.url());
    type_into(&mut h, "API key", MOCK_API_KEY);
    h.get_by_label("Connect").click();
    wait_for(&mut h, "editor", sources_loaded);

    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    h.get_by_label("user1@example.com");
    snapshot(&mut h, "editor_results");

    h.get_by_label("Disconnect").click();
    h.run_steps(3);
    assert!(matches!(h.state().state().screen, Screen::Setup(ref s) if s.host == mock.url()));
}

#[test]
fn wrong_api_key_shows_error() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(ConfigStore::memory(None)));
    type_into(&mut h, "API host", mock.url());
    type_into(&mut h, "API key", "wrong");
    h.key_press(egui::Key::Enter);
    wait_for(&mut h, "error", |h| h.query_by_label_contains("Invalid API key").is_some());
    snapshot(&mut h, "setup_error");
}

#[test]
fn saved_config_opens_editor_and_shows_query_errors() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);

    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.sql = "select fail".into();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    wait_for(&mut h, "error", |h| h.query_by_label_contains("syntax error").is_some());
    snapshot(&mut h, "editor_query_error");

    h.get_by_label("Copy Markdown").click();
    h.step();
    let copied = h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(copied.as_deref(), Some("```\nsyntax error at or near \"fail\"\n```\n"));
    wait_for(&mut h, "notice", |h| h.query_by_label_contains("Copied the error as Markdown").is_some());
}

#[test]
fn busy_toolbar_keeps_its_layout() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(store(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    let rects = |h: &Harness<'_, RedashApp>| {
        // "Variables" is also the variables panel's heading.
        ["Reload", "▶ Execute", "Variables"]
            .iter()
            .flat_map(|label| h.query_all_by_label(label).map(|n| n.rect()))
            .collect::<Vec<_>>()
    };
    let idle = rects(&h);

    let set_busy = |h: &mut Harness<'_, RedashApp>, busy: bool| {
        let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
        ed.running = busy;
        ed.loading_sources = busy;
    };
    set_busy(&mut h, true);
    h.run_steps(2);
    assert_eq!(rects(&h), idle, "toolbar buttons moved while busy");
    h.get_by_label("Running…");
    set_busy(&mut h, false);
    h.run_steps(2);
}

#[test]
fn light_theme() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(store(Some(config))));
    h.ctx.set_theme(egui::Theme::Light);
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    snapshot(&mut h, "editor_results_light");
}

#[test]
fn pages_through_results() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(store(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("1–25 of 40").is_some());
    assert!(h.query_by_label("user26@example.com").is_none(), "row 26 is on page 2");

    h.get_by_label("›").click();
    wait_for(&mut h, "page 2", |h| h.query_by_label_contains("26–40 of 40").is_some());
    h.get_by_label("user26@example.com");
    assert!(h.query_by_label("user25@example.com").is_none());

    h.get_by_label("«").click();
    wait_for(&mut h, "page 1", |h| h.query_by_label_contains("1–25 of 40").is_some());

    h.get_by(|n| n.value().as_deref() == Some("25 / page")).click();
    h.run_steps(2);
    h.get_by_label("50 / page").click();
    wait_for(&mut h, "one page", |h| h.query_by_label_contains("1–40 of 40").is_some());
}

#[test]
fn cmd_slash_toggles_sql_comment() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);
    let sql = |h: &Harness<'_, RedashApp>| match &h.state().state().screen {
        Screen::Editor(ed) => ed.sql.clone(),
        Screen::Setup(_) => panic!("expected editor"),
    };

    h.query_all_by(|n| n.value().as_deref() == Some("SELECT 1")).next().unwrap().click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Slash);
    h.run_steps(2);
    assert_eq!(sql(&h), "-- SELECT 1");

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Slash);
    h.run_steps(2);
    assert_eq!(sql(&h), "SELECT 1");
}

#[test]
fn commented_out_sql_cannot_run() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);

    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.sql = "-- SELECT 1".into();
    h.run_steps(2);
    assert!(h.get_by_label("▶ Execute").accesskit_node().is_disabled());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
    h.run_steps(2);
    assert!(!mock.requests().iter().any(|r| r.contains("POST /api/query_results")));
}

#[test]
fn autocompletes_tables_and_columns() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(
        &mut h,
        "schema",
        |h| matches!(&h.state().state().screen, Screen::Editor(ed) if !ed.tables().is_empty()),
    );
    let sql = |h: &Harness<'_, RedashApp>| match &h.state().state().screen {
        Screen::Editor(ed) => ed.sql.clone(),
        Screen::Setup(_) => panic!("expected editor"),
    };
    let type_text = |h: &mut Harness<'_, RedashApp>, text: &str| {
        let current = sql(h);
        h.query_all_by(|n| n.value().as_deref() == Some(current.as_str())).next().unwrap().type_text(text);
        h.run_steps(2);
    };

    // Clicking below the text puts the cursor at the end.
    h.query_all_by(|n| n.value().as_deref() == Some("SELECT 1")).next().unwrap().click();
    h.run_steps(2);
    type_text(&mut h, " FROM us");
    h.get_by_label("users");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(sql(&h), "SELECT 1 FROM users");
    assert!(h.query_by_label("users").is_none(), "closed after accepting");

    type_text(&mut h, " u WHERE u.em");
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(sql(&h), "SELECT 1 FROM users u WHERE u.email");

    type_text(&mut h, " > '' AND u.");
    h.get_by_label("mrr");
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.query_by_label("mrr").is_none(), "Esc closes");

    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Space);
    h.run_steps(2);
    snapshot(&mut h, "editor_autocomplete");
    h.key_press(egui::Key::ArrowDown);
    h.key_press(egui::Key::ArrowDown);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(sql(&h), "SELECT 1 FROM users u WHERE u.email > '' AND u.plan");
}

#[test]
fn copies_page_as_markdown_and_exports_csv() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let dir = tempfile::tempdir().unwrap();
    let csv_path = dir.path().join("out.csv");
    let chosen = csv_path.clone();
    let app = RedashApp::new(store(Some(config))).with_save_dialog(move |name| {
        assert_eq!(name, "query_result.csv");
        Some(chosen.clone())
    });
    let mut h = harness(app);
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("1–25 of 40").is_some());

    h.get_by_label("›").click();
    wait_for(&mut h, "page 2", |h| h.query_by_label_contains("26–40 of 40").is_some());
    h.get_by_label("Copy Markdown").click();
    h.step();
    let copied = h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    let copied = copied.expect("copied to the clipboard");
    assert_eq!(copied.lines().count(), 2 + 15, "header, separator and page 2's rows");
    assert!(copied.contains("user26@example.com") && !copied.contains("user25@example.com"));
    wait_for(&mut h, "notice", |h| h.query_by_label_contains("Copied 15 rows as Markdown").is_some());

    h.get_by_label("Export CSV").click();
    wait_for(&mut h, "saved", |h| h.query_by_label_contains("Saved").is_some());
    let csv = std::fs::read_to_string(&csv_path).unwrap();
    assert_eq!(csv.lines().count(), 1 + 40, "header and every row");
    assert!(csv.contains("user1@example.com") && csv.contains("user40@example.com"));
}

#[test]
fn search_finds_and_cycles_through_matches() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(store(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());

    // "pro" is a `plan` in every third row; only the page is searched, all columns stay.
    type_into(&mut h, "Search", "pro");
    h.get_by_label("1 of 9 matches · 9 of 25 rows");
    h.get_by_label("1–25 of 40");
    h.get_by_label("user1@example.com");
    assert!(h.query_by_label("user2@example.com").is_none(), "only matching rows");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    h.get_by_label("2 of 9 matches · 9 of 25 rows");
    snapshot(&mut h, "editor_results_search");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run_steps(2);
    h.get_by_label("1 of 9 matches · 9 of 25 rows");

    h.get_by_label("Copy Markdown").click();
    h.step();
    let copied = h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    let copied = copied.expect("copied to the clipboard");
    assert!(copied.starts_with("| id | email | plan | signed_up | mrr |\n"), "all columns");
    assert_eq!(copied.lines().count(), 2 + 9, "only the rows shown");

    // Cmd+F selects the search to type over it; the previous match wraps to the page's last.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::F);
    h.run_steps(2);
    assert!(h.get_by_label("Search").is_focused());
    h.get_by_label("Search").type_text("example");
    h.run_steps(2);
    h.get_by_label("1 of 25 matches · 25 of 25 rows");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run_steps(2);
    h.get_by_label("25 of 25 matches · 25 of 25 rows");
    h.get_by_label("user25@example.com");

    // The next page is searched when shown.
    h.get_by_label("›").click();
    wait_for(&mut h, "page 2", |h| h.query_by_label("26–40 of 40").is_some());
    h.get_by_label("1 of 15 matches · 15 of 15 rows");

    replace_text(&mut h, "Search", "zzz");
    h.get_by_label("No matches");
    h.get_by_label("26–40 of 40");
    assert!(h.query_by_label("user26@example.com").is_none());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.get_by_label("user26@example.com");
    assert!(h.query_by_label("No matches").is_none(), "Esc clears the search");
}

#[test]
fn query_variables_feed_the_query() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(ConfigStore::memory(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    assert!(h.query_by_label("+ Query").is_none(), "hidden without variables");

    h.get_by_label("Variables").click();
    h.run_steps(2);
    h.get_by_label("+ Value").click();
    h.run_steps(2);
    h.get_by_label("+ Query").click();
    h.run_steps(2);
    {
        let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
        ed.variables[0].name = "n".into();
        ed.variables[0].def = Definition::Value { value: "5".into() };
        ed.variables[1].name = "paying".into();
        ed.variables[1].def =
            Definition::Query { data_source_id: 2, sql: "SELECT id FROM users LIMIT {{ n }}".into() };
    }
    set_sql(&mut h, "SELECT email FROM users\nWHERE id IN ({{ paying }})\nLIMIT {{ n }}");
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label("user1@example.com").is_some());

    let ids = (1..=40).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    assert_eq!(
        mock.queries(),
        [
            "SELECT id FROM users LIMIT 5".to_string(),
            format!("SELECT email FROM users\nWHERE id IN ({ids})\nLIMIT 5")
        ]
    );
    h.get_by_label_contains("40 rows: 1, 2, 3");
    snapshot(&mut h, "editor_variables");
}

#[test]
fn history_restores_a_past_run() {
    let mock = MockRedash::start().unwrap();
    let store = store(mock_config(&mock));
    let mut h = harness(RedashApp::new(store));
    wait_for(&mut h, "data sources", sources_loaded);
    h.get_by_label("Nothing run yet");
    h.get_by_label("Toggle sidebar").click();
    h.run_steps(2);
    assert!(h.query_by_label("Nothing run yet").is_none(), "collapsed");
    h.get_by_label("Toggle sidebar").click();
    h.run_steps(2);
    h.get_by_label("Nothing run yet");

    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    set_sql(&mut h, "SELECT 1");
    h.get_by_label("▶ Execute").click();
    wait_for(
        &mut h,
        "second run",
        |h| matches!(&h.state().state().screen, Screen::Editor(ed) if ed.history.len() == 2 && !ed.running),
    );
    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.variables.clear();
    h.run_steps(2);
    snapshot(&mut h, "editor_history");

    h.get_by_label_contains("SELECT id, email").click();
    h.run_steps(2);
    let Screen::Editor(ed) = &h.state().state().screen else { panic!("expected editor") };
    assert_eq!(ed.sql, SAMPLE_SQL);
    assert_eq!(ed.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["start", "limit"]);
}

#[test]
fn saves_renames_restores_and_deletes_queries() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(store(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    let saved_names = |h: &Harness<'_, RedashApp>| match &h.state().state().screen {
        Screen::Editor(ed) => ed.saved.iter().map(|q| q.name.clone()).collect::<Vec<_>>(),
        Screen::Setup(_) => panic!("expected editor"),
    };
    h.get_by_label("Saved").click();
    h.run_steps(2);
    h.get_by_label_contains("No saved queries");

    // Saving opens the name, selected, to type over it.
    set_sql(&mut h, SAMPLE_SQL);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    h.run_steps(3);
    assert!(h.get_by_label("Name").is_focused());
    h.get_by_label("Name").type_text("Paying users");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(saved_names(&h), ["Paying users"]);

    set_sql(&mut h, "SELECT 1");
    h.get_by_label("Save").click();
    h.run_steps(3);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert_eq!(saved_names(&h), ["SELECT 1", "Paying users"], "Esc keeps the first line as name");
    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.variables.clear();
    h.run_steps(2);
    snapshot(&mut h, "editor_saved");

    h.get_by_label("Paying users").click();
    h.run_steps(2);
    let Screen::Editor(ed) = &h.state().state().screen else { panic!("expected editor") };
    assert_eq!(ed.sql, SAMPLE_SQL);
    assert_eq!(ed.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["start", "limit"]);

    h.get_by_label("SELECT 1").click_secondary();
    h.run_steps(2);
    h.get_by_label("Rename").click();
    h.run_steps(3);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label("Name").type_text("One");
    h.run_steps(2);
    h.get_by_label("Paying users").click(); // clicking away keeps the name
    h.run_steps(2);
    assert_eq!(saved_names(&h), ["One", "Paying users"]);

    // The variables panel has Delete buttons too.
    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.show_variables = false;
    h.get_by_label("Paying users").click_secondary();
    h.run_steps(2);
    h.get_by_label("Delete").click();
    h.run_steps(2);
    assert_eq!(saved_names(&h), ["One"]);
}

#[test]
fn schema_panel_lists_tables_and_filters_columns() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(ConfigStore::memory(mock_config(&mock))));
    wait_for(
        &mut h,
        "schema",
        |h| matches!(&h.state().state().screen, Screen::Editor(ed) if !ed.tables().is_empty()),
    );
    h.get_by_label("Schema").click();
    h.run_steps(2);
    h.get_by_label("3 tables");
    assert!(h.query_by_label("signed_up").is_none(), "collapsed");

    h.get_by_label("users").click();
    h.run_steps(3);
    h.get_by_label("signed_up");
    h.get_by_label("boolean");
    snapshot(&mut h, "editor_schema");

    type_into(&mut h, "Filter", "paid");
    h.get_by_label("1 of 3 tables");
    h.get_by_label("paid_at");
    assert!(h.query_by_label("order_id").is_none(), "only matching columns");

    h.get_by_label("Refresh").click();
    wait_for(
        &mut h,
        "refreshed schema",
        |h| matches!(&h.state().state().screen, Screen::Editor(ed) if !ed.tables().is_empty()),
    );
    assert!(mock.requests().contains(&"GET /api/data_sources/1/schema?refresh=true".to_string()));

    // The panel follows the toolbar's data source; source 2 loads through a job.
    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.schema_filter.clear();
    h.get_by(|n| n.value().as_deref() == Some("Analytics DB")).click();
    h.run_steps(2);
    h.get_by_label("Events (clickhouse)").click();
    wait_for(&mut h, "events schema", |h| h.query_by_label("Tables in Events").is_some());
    wait_for(&mut h, "events table", |h| h.query_by_label("1 table").is_some());
}
