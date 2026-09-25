use eframe::egui;

use super::{completion, history, results, saved, schema, theme, variables};
use crate::api::normalize_host;
use crate::sql::tokenize;
use crate::state::{EditorState, Event, SidebarTab, insert_name, line_at, toggle_line_comment};

pub fn show(ui: &mut egui::Ui, ed: &mut EditorState) -> Option<Event> {
    let mut event = None;
    if ed.can_execute() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
        event = Some(Event::Execute);
    }
    if ed.can_save() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
        event = Some(Event::SaveQuery);
    }

    // Keel's shell: the sidebar on the canvas, everything else on a raised surface
    // inset from the window edge.
    if ed.show_sidebar {
        egui::Panel::left("sidebar")
            .frame(theme::sidebar_frame(ui.style()))
            .show_separator_line(false)
            .resizable(true)
            .default_size(240.0)
            .min_size(180.0)
            .show(ui, |ui| {
                if let Some(e) = sidebar(ui, ed) {
                    event = Some(e);
                }
            });
    }
    let shell = theme::shell_frame(ui.style(), ed.show_sidebar);
    egui::CentralPanel::default_margins().frame(shell).show(ui, |ui| {
        theme::workspace_frame(ui.style()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if let Some(e) = workspace(ui, ed) {
                event = Some(e);
            }
        });
    });

    event
}

/// The work area: toolbar, status bar, variables, SQL editor and results.
fn workspace(ui: &mut egui::Ui, ed: &mut EditorState) -> Option<Event> {
    let mut event = None;
    egui::Panel::top("toolbar").frame(theme::bar_frame(ui.style())).show(ui, |ui| {
        ui.horizontal(|ui| {
            let tip = if ed.show_sidebar { "Hide sidebar" } else { "Show history, saved queries and schema" };
            if theme::sidebar_toggle(ui, ed.show_sidebar, "Toggle sidebar").on_hover_text(tip).clicked() {
                event = Some(Event::ToggleSidebar);
            }
            let selected = ed
                .data_sources
                .iter()
                .find(|s| Some(s.id) == ed.selected_source)
                .map_or_else(|| "Select data source".into(), |s| s.name.clone());
            let sources = egui::ComboBox::from_id_salt("data_source")
                .selected_text(selected)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for s in &ed.data_sources {
                        let label = format!("{} ({})", s.name, s.kind);
                        if theme::option(ui, &label, ed.selected_source == Some(s.id)).clicked() {
                            event = Some(Event::SelectSource(s.id));
                        }
                    }
                });
            theme::pointer(&sources.response);
            // Busy states disable buttons rather than swapping them for a spinner, so the
            // toolbar never shifts; progress is shown in the status bar.
            let reload = ui.add_enabled(!ed.loading_sources, egui::Button::new("Reload"));
            if reload.on_hover_text("Reload data sources").clicked() {
                event = Some(Event::ReloadDataSources);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Disconnect").clicked() {
                    event = Some(Event::Disconnect);
                }
                // Elided rather than pushing the buttons out of a narrow window.
                let host = egui::RichText::new(normalize_host(&ed.config.host)).weak();
                ui.add(egui::Label::new(host).truncate());
            });
        });
    });

    egui::Panel::bottom("status").frame(theme::bar_frame(ui.style())).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ed.running || ed.loading_sources {
                ui.spinner();
                ui.weak(if ed.running { "Running…" } else { "Loading data sources…" });
            } else if let Some(err) = &ed.error {
                ui.colored_label(ui.visuals().error_fg_color, err);
            } else if ed.query_error.is_some() {
                let mut status = "Query failed".to_string();
                if let Some(notice) = &ed.notice {
                    status += &format!(" · {notice}");
                }
                ui.label(status);
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let action = match (&ed.result, &ed.query_error) {
                    (Some(view), _) => results::pager(ui, view, ed.page_size),
                    (None, Some(_)) => results::error_actions(ui),
                    (None, None) => None,
                };
                if let Some(e) = action {
                    event = Some(e);
                }
            });
        });
    });

    if ed.show_variables {
        egui::Panel::right("variables")
            .frame(theme::bar_frame(ui.style()))
            .resizable(true)
            .default_size(320.0)
            .min_size(220.0)
            .show(ui, |ui| {
                if let Some(e) = variables::show(ui, ed) {
                    event = Some(e);
                }
            });
    }

    egui::Panel::top("sql_editor")
        .frame(theme::bar_frame(ui.style()))
        .resizable(true)
        .default_size(240.0)
        .min_size(80.0)
        .show(ui, |ui| {
            let id = sql_id();
            let asked = ui.data(|d| d.get_temp::<u64>(focus_later_id()));
            if asked.is_some_and(|pass| pass < ui.ctx().cumulative_pass_nr()) {
                ui.data_mut(|d| d.remove::<u64>(focus_later_id()));
                ui.memory_mut(|m| m.request_focus(id));
            }
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
                    // Reserved before the text so the current line's highlight goes under it.
                    let highlight_slot = ui.painter().add(egui::Shape::Noop);
                    let out = egui::TextEdit::multiline(&mut ed.sql)
                        .id(id)
                        .code_editor()
                        // A custom frame replaces `margin`, so the padding goes on it.
                        .frame(egui::Frame::NONE.inner_margin(theme::EDITOR_PADDING))
                        .margin(theme::EDITOR_PADDING)
                        .layouter(&mut layouter)
                        .event_filter(keys)
                        .hint_text("Write your SQL here…")
                        .desired_width(f32::INFINITY)
                        .min_size(egui::vec2(0.0, ui.available_height()))
                        .show(ui);
                    if out.response.response.has_focus()
                        && let Some(range) = out.cursor_range
                    {
                        let range = (usize::from(range.primary.index), usize::from(range.secondary.index));
                        ui.data_mut(|d| d.insert_temp(selection_id(), range));
                    }
                    let current = current_line(&out, &ed.sql);
                    if let Some(rect) = current.and_then(|line| line_rect(ui, &out, line)) {
                        let fill = theme::current_line_fill(ui.visuals().dark_mode);
                        ui.painter().set(highlight_slot, egui::Shape::rect_filled(rect, 0.0, fill));
                    }
                    paint_line_numbers(ui, &out, current);
                    completion::show(ui, id, &out, ed);
                });
            });
        });

    // Under the query: what to do with it on the left, finding in its results on the right.
    egui::Panel::top("query_bar").frame(theme::bar_frame(ui.style())).show_separator_line(false).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                let run = ui
                    .add_enabled(ed.can_execute(), theme::primary_button("▶ Execute"))
                    .on_hover_text("Cmd/Ctrl + Enter");
                if run.clicked() {
                    event = Some(Event::Execute);
                }
                let save = ui
                    .add_enabled_ui(ed.can_save(), |ui| {
                        theme::outlined_icon_button(ui, theme::Icon::Save, "Save", false)
                    })
                    .inner
                    .on_hover_text("Save: keep this query with its data source and variables (Cmd/Ctrl + S)");
                if save.clicked() {
                    event = Some(Event::SaveQuery);
                }
                let vars =
                    theme::outlined_icon_button(ui, theme::Icon::Variables, "Variables", ed.show_variables)
                        .on_hover_text("Variables: values and queries to use as {{ name }}");
                if vars.clicked() {
                    event = Some(Event::ToggleVariables);
                }
                if let Some(view) = &ed.result {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(e) = results::search(ui, view, ed.page_size) {
                            event = Some(e);
                        }
                    });
                }
            });
        },
    );

    let results_frame = egui::Frame::new().inner_margin(egui::Margin::same(8));
    egui::CentralPanel::default_margins().frame(results_frame).show(ui, |ui| {
        match (&mut ed.result, &ed.query_error) {
            (Some(view), _) => results::show(ui, view),
            (None, Some(err)) => results::error(ui, err),
            (None, None) => {
                ui.centered_and_justified(|ui| ui.weak("Run a query to see results"));
            }
        }
    });

    event
}

/// The left sidebar: tabs for history, saved queries and schema, the current tab's
/// buttons (if any), then its content.
fn sidebar(ui: &mut egui::Ui, ed: &mut EditorState) -> Option<Event> {
    let mut event = None;
    ui.horizontal(|ui| {
        let tabs =
            [(SidebarTab::History, "History"), (SidebarTab::Saved, "Saved"), (SidebarTab::Schema, "Schema")];
        for (tab, label) in tabs {
            if theme::tab(ui, label, ed.sidebar_tab == tab).clicked() {
                event = Some(Event::ShowSidebarTab(tab));
            }
        }
    });
    ui.add_space(4.0);
    let shown = match ed.sidebar_tab {
        SidebarTab::History => history::show(ui, ed),
        SidebarTab::Saved => saved::show(ui, ed),
        SidebarTab::Schema => schema::show(ui, ed),
    };
    event.or(shown)
}

pub(super) fn highlight(ui: &egui::Ui, sql: &str) -> egui::text::LayoutJob {
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

/// The logical line (from 0) holding the cursor while the editor is focused and
/// nothing is selected, like code editors' current line.
fn current_line(out: &egui::text_edit::TextEditOutput, sql: &str) -> Option<usize> {
    if !out.response.response.has_focus() {
        return None;
    }
    let range = out.cursor_range?;
    (range.primary == range.secondary).then(|| line_at(sql, usize::from(range.primary.index)))
}

/// The rows of logical line `line` (from 0), across the whole editor including
/// the gutter.
fn line_rect(ui: &egui::Ui, out: &egui::text_edit::TextEditOutput, line: usize) -> Option<egui::Rect> {
    let mut current = 0;
    let mut rows = None::<egui::Rect>;
    for row in &out.galley.rows {
        if current == line {
            let rect = row.rect().translate(out.galley_pos.to_vec2());
            rows = Some(rows.map_or(rect, |r| r.union(rect)));
        }
        if row.ends_with_newline {
            current += 1;
        }
    }
    Some(egui::Rect::from_x_y_ranges(ui.clip_rect().x_range(), rows?.y_range()))
}

/// Paints a number to the left of the first row of every logical line of the
/// text edit, so wrapped lines keep a single number. The current line's is brighter.
fn paint_line_numbers(ui: &egui::Ui, out: &egui::text_edit::TextEditOutput, current: Option<usize>) {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let right = out.response.response.rect.left() - GUTTER_PADDING / 2.0;
    let mut line = 0;
    let mut line_start = true;
    for row in &out.galley.rows {
        if line_start {
            let color = if current == Some(line) {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().weak_text_color()
            };
            let pos = egui::pos2(right, out.galley_pos.y + row.pos.y);
            ui.painter().text(pos, egui::Align2::RIGHT_TOP, (line + 1).to_string(), font.clone(), color);
            line += 1;
        }
        line_start = row.ends_with_newline;
    }
}

/// Id of the SQL editor's text edit, whose state holds the cursor.
fn sql_id() -> egui::Id {
    egui::Id::new("sql_text")
}

/// Where the editor's cursor and selection (primary, secondary char indices) were
/// last seen while it had the focus. egui collapses the selection as soon as the
/// editor loses the focus, e.g. to a click on the schema, so it is kept here.
fn selection_id() -> egui::Id {
    egui::Id::new("sql_text_selection")
}

/// Puts `name` in the SQL at the editor's cursor, or over its selection (at the end
/// if it was never focused), then focuses it with the cursor after `name`.
pub(super) fn insert_at_cursor(ctx: &egui::Context, sql: &mut String, name: &str) {
    let id = sql_id();
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let end = sql.chars().count();
    let (a, b) = ctx.data(|d| d.get_temp::<(usize, usize)>(selection_id())).unwrap_or((end, end));
    let (a, b) = (a.min(end), b.min(end));
    let (text, cursor) = insert_name(sql, a.min(b)..a.max(b), name);
    *sql = text;
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor))));
    state.store(ctx, id);
    ctx.data_mut(|d| d.insert_temp(selection_id(), (cursor, cursor)));
    // Next frame: in this one, the click that inserted would make the editor give up
    // the focus again (egui drops it on clicks elsewhere).
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(focus_later_id(), pass));
}

/// Where `insert_at_cursor` notes the pass in which it asked for the editor's focus.
fn focus_later_id() -> egui::Id {
    egui::Id::new("sql_text_focus_later")
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
