use eframe::egui;

use super::{completion, results, theme};
use crate::api::normalize_host;
use crate::sql::tokenize;
use crate::state::{EditorState, Event, toggle_line_comment};

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
                        let label = format!("{} ({})", s.name, s.kind);
                        if ui.selectable_label(ed.selected_source == Some(s.id), label).clicked() {
                            event = Some(Event::SelectSource(s.id));
                        }
                    }
                },
            );
            if ed.loading_sources {
                ui.spinner();
            } else if ui.button("Reload").on_hover_text("Reload data sources").clicked() {
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
                let mut status =
                    format!("{} rows · {:.3}s", view.result.data.rows.len(), view.result.runtime);
                if let Some(notice) = &ed.notice {
                    status += &format!(" · {notice}");
                }
                ui.label(status);
            } else {
                ui.weak("Ready");
            }
            if let Some(view) = &ed.result {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(e) = results::pager(ui, view, ed.page_size) {
                        event = Some(e);
                    }
                });
            }
        });
    });

    egui::Panel::top("sql_editor")
        .frame(theme::bar_frame(ui.style()))
        .resizable(true)
        .default_size(240.0)
        .min_size(80.0)
        .show(ui, |ui| {
            let id = egui::Id::new("sql_text");
            if ui.memory(|m| m.has_focus(id))
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Slash))
            {
                toggle_comment(ui.ctx(), id, &mut ed.sql);
            }
            let popup_open = completion::handle_keys(ui, id, ed);
            let keys = egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: popup_open,
            };
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                let mut job = highlight(ui, text.as_str());
                job.wrap.max_width = wrap_width;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let gutter = gutter_width(ui, &ed.sql);
                    ui.add_space(gutter);
                    let out = egui::TextEdit::multiline(&mut ed.sql)
                        .id(id)
                        .code_editor()
                        .margin(theme::EDITOR_PADDING)
                        .layouter(&mut layouter)
                        .event_filter(keys)
                        .hint_text("Write your SQL here…")
                        .desired_width(f32::INFINITY)
                        .min_size(egui::vec2(0.0, ui.available_height()))
                        .show(ui);
                    paint_line_numbers(ui, &out);
                    completion::show(ui, id, &out, ed);
                });
            });
        });

    egui::CentralPanel::default_margins().show(ui, |ui| match &mut ed.result {
        Some(view) => results::show(ui, view, ed.page_size),
        None => {
            ui.centered_and_justified(|ui| ui.weak("Run a query to see results"));
        }
    });

    event
}

fn highlight(ui: &egui::Ui, sql: &str) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let dark = ui.visuals().dark_mode;
    let mut job = egui::text::LayoutJob::default();
    for (token, range) in tokenize(sql) {
        let format = egui::TextFormat::simple(font.clone(), theme::syntax_color(token, dark));
        job.append(&sql[range], 0.0, format);
    }
    job
}

const GUTTER_PADDING: f32 = 8.0;

/// Width of the line-number gutter: room for the largest number (at least two digits).
fn gutter_width(ui: &egui::Ui, sql: &str) -> f32 {
    let digits = (sql.split('\n').count().to_string().len()).max(2);
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let char_width = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
    digits as f32 * char_width + GUTTER_PADDING
}

/// Paints a number to the left of the first row of every logical line of the
/// text edit, so wrapped lines keep a single number.
fn paint_line_numbers(ui: &egui::Ui, out: &egui::text_edit::TextEditOutput) {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let color = ui.visuals().weak_text_color();
    let right = out.response.response.rect.left() - GUTTER_PADDING / 2.0;
    let mut line = 1;
    let mut line_start = true;
    for row in &out.galley.rows {
        if line_start {
            let pos = egui::pos2(right, out.galley_pos.y + row.pos.y);
            ui.painter().text(pos, egui::Align2::RIGHT_TOP, line.to_string(), font.clone(), color);
            line += 1;
        }
        line_start = row.ends_with_newline;
    }
}

/// Cmd/Ctrl + /: toggle `-- ` on the lines under the cursor or selection of the
/// text edit `id`, keeping the selection on the same text.
fn toggle_comment(ctx: &egui::Context, id: egui::Id, sql: &mut String) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let end = sql.chars().count();
    let (a, b) = state.cursor.char_range().map_or((end, end), |r| {
        (usize::from(r.primary.index).min(end), usize::from(r.secondary.index).min(end))
    });
    let (text, sel) = toggle_line_comment(sql, a.min(b)..a.max(b));
    *sql = text;
    let (primary, secondary) = if a >= b { (sel.end, sel.start) } else { (sel.start, sel.end) };
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(secondary),
        egui::text::CCursor::new(primary),
    )));
    state.store(ctx, id);
}
