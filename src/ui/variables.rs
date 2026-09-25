//! The variables panel: define values and queries used in SQL as `{{ name }}`.
//! Fields are edited in place; the returned event saves them or runs a query.

use eframe::egui;

use super::{editor, theme};
use crate::state::{EditorState, Event, VariableKind};
use crate::vars::{Definition, Run, valid_name};

/// Longest value preview, in chars, before it is cut with `…`.
const PREVIEW_CHARS: usize = 60;

pub fn show(ui: &mut egui::Ui, ed: &mut EditorState) -> Option<Event> {
    let mut event = None;
    ui.horizontal(|ui| {
        ui.strong("Variables");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+ Query").on_hover_text("A query whose result is the value").clicked() {
                event = Some(Event::AddVariable(VariableKind::Query));
            }
            if ui.button("+ Value").clicked() {
                event = Some(Event::AddVariable(VariableKind::Value));
            }
        });
    });
    ui.weak("Use as {{ name }} in SQL. Query results become a list of the first column's values.");
    ui.add_space(4.0);

    let can_run = ed.can_run_variable();
    let names: Vec<String> = ed.variables.iter().map(|v| v.name.clone()).collect();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for var in &mut ed.variables {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut edited = false;
                ui.horizontal(|ui| {
                    let name = egui::TextEdit::singleline(&mut var.name)
                        .id_salt(("var_name", var.id))
                        .hint_text("name")
                        .desired_width(140.0);
                    edited |= ui.add(name).changed();
                    ui.weak(match var.def {
                        Definition::Value { .. } => "value",
                        Definition::Query { .. } => "query",
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(can_run, egui::Button::new("Delete")).clicked() {
                            event = Some(Event::RemoveVariable(var.id));
                        }
                    });
                });
                if !valid_name(&var.name) {
                    ui.colored_label(ui.visuals().error_fg_color, "Use letters, digits and _");
                } else if names.iter().filter(|n| **n == var.name).count() > 1 {
                    ui.colored_label(ui.visuals().error_fg_color, "Another variable has this name");
                }

                match &mut var.def {
                    Definition::Value { value } => {
                        let input = egui::TextEdit::singleline(value)
                            .id_salt(("var_value", var.id))
                            .code_editor()
                            .hint_text("e.g. '2026-01-01'")
                            .desired_width(f32::INFINITY);
                        edited |= ui.add(input).changed();
                    }
                    Definition::Query { data_source_id, sql } => {
                        ui.horizontal(|ui| {
                            let selected = ed
                                .data_sources
                                .iter()
                                .find(|s| s.id == *data_source_id)
                                .map_or_else(|| "Select data source".into(), |s| s.name.clone());
                            let sources = egui::ComboBox::from_id_salt(("var_source", var.id))
                                .selected_text(selected)
                                .width(160.0)
                                .show_ui(ui, |ui| {
                                    for s in &ed.data_sources {
                                        if theme::option(ui, &s.name, *data_source_id == s.id).clicked() {
                                            *data_source_id = s.id;
                                            edited = true;
                                        }
                                    }
                                });
                            theme::pointer(&sources.response);
                            let running = matches!(var.run, Some(Run::Running { .. }));
                            if running {
                                ui.spinner();
                            } else if ui
                                .add_enabled(can_run, egui::Button::new("Run"))
                                .on_hover_text("Run now; otherwise it runs when first needed")
                                .clicked()
                            {
                                event = Some(Event::RunVariable(var.id));
                            }
                        });
                        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                            let mut job = editor::highlight(ui, text.as_str());
                            job.wrap.max_width = wrap_width;
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                        let input = egui::TextEdit::multiline(sql)
                            .id_salt(("var_sql", var.id))
                            .code_editor()
                            .layouter(&mut layouter)
                            .hint_text("SELECT id FROM …")
                            .desired_rows(3)
                            .desired_width(f32::INFINITY);
                        edited |= ui.add(input).changed();
                    }
                }
                status(ui, var.fresh_value().is_some(), var.run.as_ref());
                if edited && event.is_none() {
                    event = Some(Event::VariablesEdited);
                }
            });
        }
    });
    event
}

/// The last run of a query variable: its value, or why it has none.
fn status(ui: &mut egui::Ui, fresh: bool, run: Option<&Run>) {
    match run {
        Some(Run::Done { value, rows, .. }) => {
            let mut preview: String = value.chars().take(PREVIEW_CHARS).collect();
            if preview.len() < value.len() {
                preview.push('…');
            }
            let rows = format!("{rows} {}", if *rows == 1 { "row" } else { "rows" });
            let text =
                if fresh { format!("{rows}: {preview}") } else { "Changed; runs again when used".into() };
            if fresh {
                format!("= {preview}  ({rows})")
            } else {
                "Changed; runs again when used".into()
            };
            ui.add(egui::Label::new(egui::RichText::new(text).weak()).truncate()).on_hover_text(value);
        }
        Some(Run::Failed(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        Some(Run::Running { .. }) | None => {}
    }
}
