use eframe::egui;

use super::{results, theme};
use crate::api::normalize_host;
use crate::state::{EditorState, Event};

pub fn show(ui: &mut egui::Ui, ed: &mut EditorState) -> Option<Event> {
    let mut event = None;
    if ed.can_execute() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
        event = Some(Event::Execute);
    }

    egui::Panel::top("toolbar").frame(theme::bar_frame(ui.style())).show(ui, |ui| {
        ui.horizontal(|ui| {
            let selected = ed
                .data_sources
                .iter()
                .find(|s| Some(s.id) == ed.selected_source)
                .map_or_else(|| "Select data source".into(), |s| s.name.clone());
            egui::ComboBox::from_id_salt("data_source").selected_text(selected).width(220.0).show_ui(
                ui,
                |ui| {
                    for s in &ed.data_sources {
                        ui.selectable_value(
                            &mut ed.selected_source,
                            Some(s.id),
                            format!("{} ({})", s.name, s.kind),
                        );
                    }
                },
            );
            if ed.loading_sources {
                ui.spinner();
            } else if ui.small_button("Reload").on_hover_text("Reload data sources").clicked() {
                event = Some(Event::ReloadDataSources);
            }

            let run = ui
                .add_enabled(ed.can_execute(), theme::primary_button("▶ Execute"))
                .on_hover_text("Cmd/Ctrl + Enter");
            if run.clicked() {
                event = Some(Event::Execute);
            }
            if ed.running {
                ui.spinner();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Disconnect").clicked() {
                    event = Some(Event::Disconnect);
                }
                ui.weak(normalize_host(&ed.config.host));
            });
        });
    });

    egui::Panel::bottom("status").frame(theme::bar_frame(ui.style())).show(ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(err) = &ed.error {
                ui.colored_label(ui.visuals().error_fg_color, err);
            } else if let Some(view) = &ed.result {
                ui.label(format!("{} rows · {:.3}s", view.result.data.rows.len(), view.result.runtime));
            } else {
                ui.weak("Ready");
            }
        });
    });

    egui::Panel::top("sql_editor").resizable(true).default_size(240.0).min_size(80.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut ed.sql).code_editor().hint_text("Write your SQL here…"),
            );
        });
    });

    egui::CentralPanel::default_margins().show(ui, |ui| match &mut ed.result {
        Some(view) => results::show(ui, view),
        None => {
            ui.centered_and_justified(|ui| ui.weak("Run a query to see results"));
        }
    });

    event
}
