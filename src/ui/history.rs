//! The history panel (a sidebar tab, shown by default): past runs of the editor's query, newest first. Clicking one
//! restores its SQL, data source and variables.

use eframe::egui;

use crate::api::DataSource;
use crate::history::{self, Entry};
use crate::state::{EditorState, Event};
use crate::vars::Variable;

/// The panel's header buttons, drawn right to left.
pub fn actions(ui: &mut egui::Ui, ed: &EditorState) -> Option<Event> {
    ui.add_enabled(!ed.history.is_empty(), egui::Button::new("Clear"))
        .clicked()
        .then_some(Event::ClearHistory)
}

pub fn show(ui: &mut egui::Ui, ed: &EditorState) -> Option<Event> {
    let mut event = None;
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
                if item(ui, ed, entry, now, i).clicked() {
                    event = Some(Event::RestoreHistory(i));
                }
            }
        });
    });
    event
}

/// One entry: its first SQL line, then data source, variable count and age.
fn item(ui: &mut egui::Ui, ed: &EditorState, entry: &Entry, now: u64, i: usize) -> egui::Response {
    let meta =
        meta(&ed.data_sources, entry.data_source_id, &entry.variables, history::ago(entry.executed_at, now));
    row(ui, ("history", i), egui::RichText::new(entry.title()).monospace(), &meta, &entry.sql)
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

/// A clickable snapshot: `title` over the weak `meta` line; hovering shows `sql`.
pub(super) fn row(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: egui::RichText,
    meta: &str,
    sql: &str,
) -> egui::Response {
    let builder = egui::UiBuilder::new().id_salt(id_salt).sense(egui::Sense::click());
    ui.scope_builder(builder, |ui| {
        let widgets = &ui.visuals().widgets;
        let (fill, radius) = if ui.response().hovered() && ui.is_enabled() {
            (widgets.hovered.weak_bg_fill, widgets.hovered.corner_radius)
        } else {
            (egui::Color32::TRANSPARENT, widgets.inactive.corner_radius)
        };
        egui::Frame::new().fill(fill).corner_radius(radius).inner_margin(egui::Margin::symmetric(6, 4)).show(
            ui,
            |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(title).truncate().selectable(false));
                let meta = egui::RichText::new(meta).weak();
                ui.add(egui::Label::new(meta).truncate().selectable(false));
            },
        );
    })
    .response
    .on_hover_ui(|ui| {
        ui.label(egui::RichText::new(sql).monospace());
    })
}
