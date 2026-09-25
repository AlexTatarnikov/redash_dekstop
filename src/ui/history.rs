//! The history panel (a sidebar tab, shown by default): past runs of the editor's query, newest first. Clicking one
//! restores its SQL, data source and variables.

use eframe::egui;

use super::theme;
use crate::api::DataSource;
use crate::history::{self, Entry};
use crate::state::{EditorState, Event};
use crate::vars::Variable;

pub fn show(ui: &mut egui::Ui, ed: &EditorState) -> Option<Event> {
    let mut event = None;
    // At the bottom, away from the entries, so it isn't hit by accident.
    let footer = egui::Frame::new().inner_margin(egui::Margin { top: 8, ..Default::default() });
    egui::Panel::bottom("history_footer").frame(footer).show(ui, |ui| {
        if ui.add_enabled(!ed.history.is_empty(), egui::Button::new("Clear all")).clicked() {
            event = Some(Event::ClearHistory);
        }
    });
    ui.weak(format!(
        "Last {} runs. Click one to restore its SQL, data source and variables.",
        history::LIMIT
    ));
    ui.add_space(4.0);
    if ed.history.is_empty() {
        ui.weak("Nothing run yet");
        return event;
    }

    let now = (ed.clock)();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.add_enabled_ui(!ed.running, |ui| {
            for (i, entry) in ed.history.iter().enumerate() {
                if i > 0 {
                    divider(ui);
                }
                let (response, delete) = item(ui, ed, entry, now, i);
                if delete {
                    event = Some(Event::DeleteHistory(i));
                } else if response.clicked() {
                    event = Some(Event::RestoreHistory(i));
                }
            }
        });
    });
    event
}

/// One entry: its first SQL line, then data source, variable count and age. Also
/// whether its remove button was clicked.
fn item(ui: &mut egui::Ui, ed: &EditorState, entry: &Entry, now: u64, i: usize) -> (egui::Response, bool) {
    let meta =
        meta(&ed.data_sources, entry.data_source_id, &entry.variables, history::ago(entry.executed_at, now));
    let title = egui::RichText::new(entry.title()).monospace();
    row(ui, ("history", i), title, &meta, &entry.sql, "Remove from history")
}

/// A hairline between two rows of a sidebar list, in the panels' line colour.
pub(super) fn divider(ui: &mut egui::Ui) {
    ui.add(egui::Separator::default().horizontal().spacing(0.0));
}

/// "Source · N variables · `age`" under a snapshot's title.
pub(super) fn meta(
    sources: &[DataSource],
    data_source_id: i64,
    variables: &[Variable],
    age: String,
) -> String {
    let source =
        sources.iter().find(|s| s.id == data_source_id).map_or("Unknown source", |s| s.name.as_str());
    let mut meta = vec![source.to_string()];
    let n = variables.len();
    if n > 0 {
        meta.push(format!("{n} {}", if n == 1 { "variable" } else { "variables" }));
    }
    meta.push(age);
    meta.join(" · ")
}

/// Size of a row's remove button.
const REMOVE_SIZE: f32 = 20.0;

/// A clickable snapshot: `title` over the weak `meta` line; hovering shows `sql`, and
/// a remove button (named `remove`) at its top right. Also whether that was clicked.
pub(super) fn row(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: egui::RichText,
    meta: &str,
    sql: &str,
    remove: &str,
) -> (egui::Response, bool) {
    let builder = egui::UiBuilder::new().id_salt(id_salt).sense(egui::Sense::click());
    let mut removed = false;
    let highlight = theme::RowHighlight::reserve(ui);
    let response = ui
        .scope_builder(builder, |ui| {
            let frame = egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 4));
            let inner = frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Room for the remove button, kept while it is hidden so the text doesn't reflow.
                let text_width = ui.available_width() - REMOVE_SIZE;
                ui.scope(|ui| {
                    ui.set_max_width(text_width);
                    ui.add(egui::Label::new(title).truncate().selectable(false));
                    let meta = egui::RichText::new(meta).weak();
                    ui.add(egui::Label::new(meta).truncate().selectable(false));
                });
            });
            // Only on the hovered row, so the list stays calm; still hovered with the
            // pointer on the button, which is above the row.
            let row = inner.response.rect;
            if ui.rect_contains_pointer(row) && ui.is_enabled() {
                let top_right = row.right_top() + egui::vec2(-2.0, 2.0);
                let rect = egui::Rect::from_min_size(
                    top_right - egui::vec2(REMOVE_SIZE, 0.0),
                    egui::Vec2::splat(REMOVE_SIZE),
                );
                let button = theme::icon_button_at(ui, rect, theme::Icon::Remove, remove);
                removed = button.on_hover_text(remove).clicked();
            }
        })
        .response;
    highlight.fill(ui, response.rect);
    theme::pointer(&response);
    let response = response.on_hover_ui(|ui| {
        ui.label(egui::RichText::new(sql).monospace());
    });
    (response, removed)
}
