//! End-to-end UI tests: drive the real app headlessly (no window) against the
//! in-process mock Redash. Screenshots are compared with `tests/snapshots/*.png`;
//! on mismatch kittest writes `*.new.png` and `*.diff.png` next to them.
//! Accept intended visual changes with `UPDATE_SNAPSHOTS=1 cargo test`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use redash_desktop::RedashApp;
use redash_desktop::config::{Config, ConfigStore};
use redash_desktop::mock::{MOCK_API_KEY, MockRedash};
use redash_desktop::state::Screen;

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

fn type_into(h: &mut Harness<'_, RedashApp>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
    h.get_by_label(label).type_text(text);
    h.run_steps(2);
}

#[test]
fn connect_run_query_and_disconnect() {
    let mock = MockRedash::start().unwrap();
    let mut h = harness(RedashApp::new(ConfigStore::memory(None)));
    snapshot(&mut h, "setup_empty");

    type_into(&mut h, "API host", mock.url());
    type_into(&mut h, "API key", MOCK_API_KEY);
    h.get_by_label("Connect").click();
    wait_for(&mut h, "editor", sources_loaded);

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
}

#[test]
fn light_theme() {
    let mock = MockRedash::start().unwrap();
    let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
    let mut h = harness(RedashApp::new(ConfigStore::memory(Some(config))));
    h.ctx.set_theme(egui::Theme::Light);
    wait_for(&mut h, "data sources", sources_loaded);
    h.get_by_label("▶ Execute").click();
    wait_for(&mut h, "results", |h| h.query_by_label_contains("40 rows").is_some());
    snapshot(&mut h, "editor_results_light");
}
