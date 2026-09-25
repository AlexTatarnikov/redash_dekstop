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
    snapshot_hovering(h, name);
}

/// Like `snapshot`, but leaves the pointer where it is, to show a hover state.
fn snapshot_hovering(h: &mut Harness<'_, RedashApp>, name: &str) {
    h.run_steps(2);
    // The host label's width depends on the port's digits (Geist digits vary in width),
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
fn highlights_the_cursor_line_and_pads_the_text_from_line_numbers() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.run_steps(2);
    // Clicking below the text puts the cursor at the end; up moves it into the
    // wrapped WHERE line, which is highlighted across both of its rows.
    h.query_all_by(|n| n.value().as_deref() == Some(SAMPLE_SQL)).next().unwrap().click();
    h.run_steps(2);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    snapshot(&mut h, "editor_current_line");
}

#[test]
fn hovering_sidebar_tabs_does_not_shift_them() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);
    let tabs = ["History", "Saved", "Schema"];
    let rects = |h: &Harness<'_, RedashApp>| tabs.map(|t| h.get_by_label(t).rect());
    let before = rects(&h);
    for tab in tabs {
        h.get_by_label(tab).hover();
        h.run_steps(2);
        assert_eq!(rects(&h), before, "hovering {tab} moved the tabs");
    }
}

#[test]
fn hovering_data_source_options_does_not_shift_them() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);
    h.get_by(|n| n.value().as_deref() == Some("Analytics DB")).click();
    h.run_steps(2);
    let options = ["Analytics DB (pg)", "Events (clickhouse)"];
    let rects = options.map(|o| h.get_by_label(o).rect());
    // Away from the list, so no option is hovered.
    h.hover_at(egui::pos2(700.0, 500.0));
    h.run_steps(2);
    let image = h.render().unwrap();
    let idle = rects.map(|r| text_origin(|x, y| image.get_pixel(x, y).0, r));
    for (option, rect) in options.iter().zip(rects) {
        h.get_by_label(option).hover();
        h.run_steps(2);
        assert_eq!(h.get_by_label(option).rect(), rect, "hovering {option} resized it");
        let image = h.render().unwrap();
        let i = options.iter().position(|o| o == option).unwrap();
        let origin = text_origin(|x, y| image.get_pixel(x, y).0, rect);
        assert_eq!(origin, idle[i], "hovering {option} moved its text");
    }
    // The hovered option stands out from the list (Events, below the selected one).
    h.get_by_label("Events (clickhouse)").hover();
    snapshot_hovering(&mut h, "editor_data_source_menu");
    h.get_by_label("Events (clickhouse)").click();
    h.run_steps(2);
    h.get_by(|n| n.value().as_deref() == Some("Events"));
}

/// Top-left corner of the bright (text) pixels inside `rect` of a dark-mode render,
/// given as a pixel lookup, to catch text moving without its widget's rect changing.
fn text_origin(pixel: impl Fn(u32, u32) -> [u8; 4], rect: egui::Rect) -> (u32, u32) {
    let (mut x0, mut y0) = (u32::MAX, u32::MAX);
    for y in rect.min.y as u32..rect.max.y as u32 {
        for x in rect.min.x as u32..rect.max.x as u32 {
            if pixel(x, y)[..3].iter().any(|&c| c > 150) {
                x0 = x0.min(x);
                y0 = y0.min(y);
            }
        }
    }
    (x0, y0)
}

#[test]
fn clickable_widgets_show_the_pointing_hand() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(store(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    let cursor_over = |h: &mut Harness<'_, RedashApp>, node: egui::Rect| {
        h.hover_at(node.center());
        h.run_steps(2);
        h.output().platform_output.cursor_icon
    };
    let hand = egui::CursorIcon::PointingHand;

    let clickable = [
        h.get_by_label("▶ Execute").rect(),
        h.get_by_label("Toggle sidebar").rect(),
        h.get_by_label("Saved").rect(),
        h.get_by(|n| n.value().as_deref() == Some("Analytics DB")).rect(),
        h.get_by(|n| n.value().as_deref() == Some("25 / page")).rect(),
        h.get_by_label_contains("Analytics DB · 2 variables").rect(),
    ];
    for rect in clickable {
        assert_eq!(cursor_over(&mut h, rect), hand, "at {rect:?}");
    }
    let editor = h.query_all_by(|n| n.value().as_deref() == Some(SAMPLE_SQL)).next().unwrap().rect();
    assert_eq!(cursor_over(&mut h, editor), egui::CursorIcon::Text);

    h.get_by_label("Schema").click();
    wait_for(&mut h, "schema", |h| h.query_by_label("users").is_some());
    let table = h.get_by_label("users").rect();
    assert_eq!(cursor_over(&mut h, table), hand, "schema table");
}

#[test]
fn spare_width_is_shared_by_the_result_columns() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(store(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    // Left edges of the columns' headers.
    let edges = |h: &Harness<'_, RedashApp>| {
        let x = |name: &str| h.get_by_label(name).rect().min.x;
        (x("email"), x("plan"), x("signed_up"), x("mrr"))
    };
    let (email, plan, signed_up, mrr) = edges(&h);

    // Closing the variables panel widens the results: every column gets the same share.
    // The toggle is the leftmost "Variables"; the other is the panel's heading.
    h.query_all_by_label("Variables")
        .min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap()
        .click();
    h.run_steps(4);
    let (email2, plan2, signed_up2, mrr2) = edges(&h);
    let share = (plan2 - email2) - (plan - email);
    assert!(share > 20.0, "email grew by {share}");
    assert!(((signed_up2 - plan2) - (signed_up - plan) - share).abs() <= 1.0, "plan grew like email");
    assert!(((mrr2 - signed_up2) - (mrr - signed_up) - share).abs() <= 1.0, "signed_up too");
    // Numbers are left-aligned like everything else, so they line up with the NULLs
    // in their column and with its header.
    let null = h.query_all_by_label("NULL").next().unwrap().rect().min.x;
    let number = h.get_by_label("12.5").rect().min.x;
    assert!(
        (null - mrr2).abs() <= 1.0 && (number - mrr2).abs() <= 1.0,
        "mrr at {mrr2}, 12.5 at {number}, NULL at {null}"
    );
    snapshot(&mut h, "editor_results_wide");
}

#[test]
fn schema_inserts_tables_and_columns_into_the_query() {
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
    // Clicking below the text puts the cursor at the end.
    set_sql(&mut h, "SELECT * FROM ");
    h.run_steps(2);
    h.query_all_by(|n| n.value().as_deref() == Some("SELECT * FROM ")).next().unwrap().click();
    h.run_steps(2);
    h.get_by_label("Schema").click();
    h.run_steps(2);

    // A table goes in at the cursor, and the editor gets the focus back.
    assert!(h.query_by_label("Insert users").is_none(), "hidden until hovered");
    let tables = |h: &Harness<'_, RedashApp>| {
        ["users", "orders", "billing.invoices"].map(|t| h.get_by_label(t).rect())
    };
    let before = tables(&h);
    h.get_by_label("users").hover();
    h.run_steps(2);
    assert_eq!(tables(&h), before, "hovering a table moves nothing");
    snapshot_hovering(&mut h, "editor_schema_insert");
    h.get_by_label("Insert users").click();
    h.run_steps(2);
    assert_eq!(sql(&h), "SELECT * FROM users");
    assert!(
        h.query_all_by(|n| n.value().as_deref() == Some("SELECT * FROM users")).next().unwrap().is_focused()
    );
    assert!(h.query_by_label("signed_up").is_none(), "inserting doesn't expand the table");

    // A column replaces the selected text.
    let id = egui::Id::new("sql_text");
    let mut state = egui::text_edit::TextEditState::load(&h.ctx, id).unwrap();
    let star = egui::text::CCursorRange::two(egui::text::CCursor::new(7), egui::text::CCursor::new(8));
    state.cursor.set_char_range(Some(star));
    state.store(&h.ctx, id);
    h.get_by_label("users").click();
    h.run_steps(2);
    let columns =
        |h: &Harness<'_, RedashApp>| ["id", "email", "plan", "orders"].map(|c| h.get_by_label(c).rect());
    let before = columns(&h);
    h.get_by_label("email").hover();
    h.run_steps(2);
    assert_eq!(columns(&h), before, "hovering a column moves nothing");
    let button = h.get_by_label("Insert email").rect();
    // email's type, on the button's row (plan is text too).
    let kind = h
        .query_all_by_label("text")
        .map(|n| n.rect())
        .find(|r| r.y_range().contains(button.center().y))
        .unwrap();
    assert!(button.min.x >= kind.max.x, "the button {button:?} is clear of the type {kind:?}");
    snapshot_hovering(&mut h, "editor_schema_insert_column");
    h.get_by_label("Insert email").click();
    h.run_steps(2);
    assert_eq!(sql(&h), "SELECT email FROM users");
}

#[test]
fn query_actions_sit_under_the_editor_with_search_on_the_right() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(store(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    let rect = |label: &str| {
        h.query_all_by_label(label).map(|n| n.rect()).min_by(|a, b| a.min.x.total_cmp(&b.min.x)).unwrap()
    };
    let (execute, save, variables, search) =
        (rect("▶ Execute"), rect("Save"), rect("Variables"), rect("Search"));
    let editor = h.query_all_by(|n| n.value().as_deref() == Some(SAMPLE_SQL)).next().unwrap().rect();
    assert!(execute.min.y > editor.max.y, "under the editor");
    for (name, r) in [("Save", save), ("Variables", variables), ("Search", search)] {
        assert!((r.center().y - execute.center().y).abs() < 1.0, "{name} on Execute's row");
    }
    assert!(execute.max.x < save.min.x && save.max.x < variables.min.x, "Execute, Save, Variables");
    // The box, labelled "Search" too, ends where the editor above does.
    let search_box =
        h.query_all_by_label("Search").map(|n| n.rect()).max_by(|a, b| a.max.x.total_cmp(&b.max.x)).unwrap();
    assert!(search.min.x > variables.max.x, "Search right of the buttons");
    assert!((search_box.max.x - editor.max.x).abs() <= 4.0, "at the right: {search_box:?} vs {editor:?}");
    // Icons, square like the bar's height, not text buttons.
    assert_eq!(save.width(), save.height());
    assert_eq!(variables.width(), variables.height());
    assert!(h.query_by_label("Reload").unwrap().rect().max.y < editor.min.y, "Reload stays in the toolbar");
}

#[test]
fn schema_with_long_names_stays_within_the_sidebar() {
    use redash_desktop::api::{Table, TableColumn};
    use redash_desktop::state::Schema;
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(
        &mut h,
        "schema",
        |h| matches!(&h.state().state().screen, Screen::Editor(ed) if !ed.tables().is_empty()),
    );
    let column = |name: &str, kind: &str| TableColumn { name: name.into(), kind: Some(kind.into()) };
    let long_table = "analytics_warehouse.customer_subscription_billing_events_daily_snapshot";
    let tables = vec![
        Table {
            name: long_table.into(),
            columns: vec![
                column("id", "integer"),
                column("subscription_billing_period_start_timestamp_utc", "timestamp without time zone"),
                column("customer_lifetime_value_estimate_usd", "double precision"),
            ],
        },
        Table { name: "users".into(), columns: vec![column("id", "integer")] },
    ];
    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.schemas.insert(1, Schema::Loaded(tables));
    h.get_by_label("Schema").click();
    h.run_steps(2);
    h.get_by_label(long_table).click();
    h.run_steps(3);
    let long_column = "subscription_billing_period_start_timestamp_utc";
    h.get_by_label(long_column).hover();
    h.run_steps(2);

    // Everything ends inside the sidebar (the work area starts at the toggle's panel),
    // and the column's name, its type and the insert button don't overlap.
    let sidebar_right = h.get_by_label("Toggle sidebar").rect().min.x - 8.0;
    let row = h.get_by_label(long_column).rect();
    let kind = h.get_by_label("timestamp without time zone").rect();
    let insert = h.get_by_label(&format!("Insert {long_column}")).rect();
    for (name, r) in [("table", h.get_by_label(long_table).rect()), ("type", kind), ("insert", insert)] {
        assert!(r.max.x <= sidebar_right, "{name} {r:?} spills past the sidebar at {sidebar_right}");
    }
    assert!(
        row.max.x <= kind.min.x && kind.max.x <= insert.min.x,
        "name {row:?}, type {kind:?}, insert {insert:?}"
    );
    snapshot_hovering(&mut h, "editor_schema_long_names");

    // The whole list, without the hovered row's tooltip over it; then autocompletion.
    h.get_by_label("users").click();
    h.run_steps(3);
    snapshot(&mut h, "editor_schema_long_names_list");
    set_sql(&mut h, "SELECT ");
    h.run_steps(2);
    h.query_all_by(|n| n.value().as_deref() == Some("SELECT ")).next().unwrap().click();
    h.run_steps(2);
    h.query_all_by(|n| n.value().as_deref() == Some("SELECT ")).next().unwrap().type_text("sub");
    h.run_steps(2);
    assert_eq!(h.query_all_by_label(long_column).count(), 2, "in the schema and the suggestions");
    snapshot(&mut h, "editor_autocomplete_long_names");
}

#[test]
fn results_with_long_column_names_stay_readable() {
    use redash_desktop::state::Event;
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    wait_for(&mut h, "data sources", sources_loaded);
    let long = "subscription_billing_period_start_timestamp_utc_with_a_very_long_suffix_for_testing";
    let rows: Vec<_> = (1..=5)
        .map(|i| serde_json::json!({ "id": i, long: format!("2026-09-0{i}T00:00:00"), "customer_lifetime_value_estimate_usd": i * 10 }))
        .collect();
    let result = serde_json::from_value(serde_json::json!({
        "data": {
            "columns": [{ "name": "id" }, { "name": long }, { "name": "customer_lifetime_value_estimate_usd" }],
            "rows": rows,
        },
        "runtime": 0.1,
    }))
    .unwrap();
    let state = h.state_mut().state_mut();
    let Screen::Editor(ed) = &mut state.screen else { panic!("expected editor") };
    ed.running = true;
    state.update(Event::QueryFinished(Ok(result)));
    h.run_steps(4);

    // Headers fit their column or are clipped inside it: none runs into the next.
    let (id, long_header, last) = (
        h.get_by_label("id").rect(),
        h.get_by_label(long).rect(),
        h.get_by_label("customer_lifetime_value_estimate_usd").rect(),
    );
    assert!(id.max.x < long_header.min.x && long_header.min.x < last.min.x);
    snapshot(&mut h, "editor_results_long_names");
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
    h.get_by_label("1 of 9 matches");
    h.get_by_label("1–25 of 40");
    h.get_by_label("user1@example.com");
    assert!(h.query_by_label("user2@example.com").is_none(), "only matching rows");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    h.get_by_label("2 of 9 matches");
    snapshot(&mut h, "editor_results_search");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run_steps(2);
    h.get_by_label("1 of 9 matches");

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
    h.get_by_label("1 of 25 matches");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run_steps(2);
    h.get_by_label("25 of 25 matches");
    h.get_by_label("user25@example.com");

    // The next page is searched when shown.
    h.get_by_label("›").click();
    wait_for(&mut h, "page 2", |h| h.query_by_label("26–40 of 40").is_some());
    h.get_by_label("1 of 15 matches");

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

    // An entry's remove button shows on hover, and removes it without restoring it.
    h.hover_at(egui::pos2(700.0, 500.0));
    h.run_steps(2);
    assert!(h.query_by_label("Remove from history").is_none(), "hidden until hovered");
    h.get_by_label("SELECT 1").hover();
    h.run_steps(2);
    snapshot_hovering(&mut h, "editor_history_hover");
    h.get_by_label("Remove from history").click();
    h.run_steps(2);
    let history = |h: &Harness<'_, RedashApp>| match &h.state().state().screen {
        Screen::Editor(ed) => (ed.history.iter().map(|e| e.sql.clone()).collect::<Vec<_>>(), ed.sql.clone()),
        Screen::Setup(_) => panic!("expected editor"),
    };
    assert_eq!(history(&h), (vec![SAMPLE_SQL.to_string()], SAMPLE_SQL.to_string()));

    // Clear all is at the bottom of the sidebar, well below the entries.
    let clear = h.get_by_label("Clear all").rect();
    assert!(clear.min.y > 650.0, "at the bottom: {clear:?}");
    h.get_by_label("Clear all").click();
    h.run_steps(2);
    assert!(history(&h).0.is_empty());
    h.get_by_label("Nothing run yet");
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

    // Or with the remove button of the hovered query.
    h.get_by_label("One").hover();
    h.run_steps(2);
    h.get_by_label("Delete query").click();
    h.run_steps(2);
    assert!(saved_names(&h).is_empty());
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

    h.get_by_label("Refresh schema").click();
    let refreshed = "GET /api/data_sources/1/schema?refresh=true".to_string();
    wait_for(&mut h, "refreshed schema", |h| {
        mock.requests().contains(&refreshed)
            && matches!(&h.state().state().screen, Screen::Editor(ed) if !ed.tables().is_empty())
    });

    // The panel follows the toolbar's data source; source 2 loads through a job.
    let Screen::Editor(ed) = &mut h.state_mut().state_mut().screen else { panic!("expected editor") };
    ed.schema_filter.clear();
    h.get_by(|n| n.value().as_deref() == Some("Analytics DB")).click();
    h.run_steps(2);
    h.get_by_label("Events (clickhouse)").click();
    wait_for(&mut h, "events schema", |h| h.query_by_label("Tables in Events").is_some());
    wait_for(&mut h, "events table", |h| h.query_by_label("1 table").is_some());
}

#[test]
fn search_scrolls_the_selected_match_into_view() {
    let mock = MockRedash::start().unwrap();
    // Narrow enough that the table scrolls sideways: `mrr`, the last column, starts out of view.
    let mut h = Harness::builder()
        .with_size([850.0, 750.0])
        .wgpu()
        .build_ui_state(|ui, app: &mut RedashApp| app.show(ui), RedashApp::new(store(mock_config(&mock))));
    wait_for(&mut h, "data sources", sources_loaded);
    set_sql(&mut h, SAMPLE_SQL);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    h.get_by(|n| n.value().as_deref() == Some("25 / page")).click();
    h.run_steps(2);
    h.get_by_label("50 / page").click();
    wait_for(&mut h, "one page", |h| h.query_by_label_contains("1–40 of 40").is_some());

    // ".5" is in the `mrr` of 13 rows; the previous match wraps to the last, in row 37.
    type_into(&mut h, "Search", ".5");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    h.run();
    h.get_by_label_contains("13 of 13 matches");
    let cell = h.get_by_label("462.5").rect();
    // The variables panel's heading, not the toolbar button to its left.
    let right = h.query_all_by_label("Variables").map(|n| n.rect().min.x).fold(0.0, f32::max);
    let bottom = h.get_by_label("Copy Markdown").rect().min.y;
    assert!(cell.max.x <= right && cell.max.y <= bottom, "match at {cell:?} is out of view");
}
