use eframe::egui::{self, emath::GuiRounding as _};
use egui_extras::{Column, TableBuilder};

use super::theme;
use crate::api::QueryResult;
use crate::export::{ValueKind, cell_text};
use crate::search::Hit;
use crate::state::{Event, PAGE_SIZES, ResultView};

// Bounds for fitted column widths; wider values are clipped but can be resized by hand.
const MIN_COL_WIDTH: f32 = 40.0;
const MAX_COL_WIDTH: f32 = 400.0;
// Only this many rows are measured, to keep huge results cheap.
const MEASURED_ROWS: usize = 1000;

/// The search bar, then the table of the rows the search found.
pub fn show(ui: &mut egui::Ui, view: &mut ResultView, page_size: usize) -> Option<Event> {
    let event = search(ui, view, page_size);
    table(ui, view);
    event
}

/// Find on the shown page, like a browser's Cmd/Ctrl + F: typing hides the page's rows
/// without a match; Enter and Shift + Enter select the next and previous match; Esc clears the search.
fn search(ui: &mut egui::Ui, view: &ResultView, page_size: usize) -> Option<Event> {
    let id = egui::Id::new("results_search");
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
        focus_and_select_all(ui.ctx(), id, &view.search);
    }
    let mut text = view.search.clone();
    let hits = view.found.hits.len();
    let mut event = None;
    ui.horizontal(|ui| {
        let label = ui.label("Search");
        // As tall as the buttons elsewhere, not a bare line of text.
        let input = egui::TextEdit::singleline(&mut text)
            .id(id)
            .hint_text("Value to find")
            .margin(egui::Margin::symmetric(8, 4))
            .vertical_align(egui::Align::Center)
            .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
            // Narrower in a narrow panel, leaving room for the match count: a row wider than
            // the panel would widen the table's scroll area past it, so it couldn't scroll sideways.
            .desired_width((ui.available_width() - 200.0).max(ui.available_width() / 2.0).min(240.0))
            // Keep the focus on Enter, which selects the next match.
            .return_key(None);
        let tip = "Enter: next match · Shift + Enter: previous · Esc: clear";
        let input = ui.add(input).labelled_by(label.id).on_hover_text(tip);
        let (enter, escape, shift) = ui.input(|i| {
            (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.modifiers.shift)
        });
        if input.changed() {
            event = Some(Event::SearchResults(text));
        } else if input.has_focus() && enter && hits > 0 {
            event = Some(Event::NextMatch(!shift));
        } else if (input.has_focus() || input.lost_focus()) && escape && !view.search.is_empty() {
            event = Some(Event::SearchResults(String::new()));
        }

        if !view.search.trim().is_empty() {
            let rows = format!("{} of {} rows", view.found.rows.len(), view.page_rows(page_size).len());
            let status = match hits {
                0 => "No matches".to_string(),
                n => format!("{} of {n} matches · {rows}", view.current + 1),
            };
            ui.add(egui::Label::new(egui::RichText::new(status).weak()).truncate());
        }
    });
    ui.add_space(theme::CELL_PADDING);
    event
}

/// Focuses the text edit `id` with all of its `text` selected, ready to be typed over.
fn focus_and_select_all(ctx: &egui::Context, id: egui::Id, text: &str) {
    ctx.memory_mut(|m| m.request_focus(id));
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let all = egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(text.chars().count()),
    );
    state.cursor.set_char_range(Some(all));
    state.store(ctx, id);
}

fn table(ui: &mut egui::Ui, view: &mut ResultView) {
    // Widths are fitted to the whole result so columns don't jump between pages.
    if view.col_widths.is_none() {
        view.col_widths = Some(fit_column_widths(ui.ctx(), &view.result));
    }
    let view = &*view;
    let widths = view.col_widths.as_deref().unwrap_or_default();
    let rows = &view.found.rows;
    let r = &view.result;
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 12.0;
    let dark = ui.visuals().dark_mode;
    let numeric: Vec<bool> = r.data.columns.iter().map(|c| is_numeric(r, &c.name)).collect();

    // Bring the selected match into view when it changes; otherwise start each newly
    // shown page at its top. The match's cell scrolls sideways once it is drawn.
    let current = view.found.hits.get(view.current);
    let shown_id = egui::Id::new(("results_shown", view.id));
    let reveal_id = egui::Id::new(("results_reveal", view.id));
    let shown = (view.page, view.search.clone(), view.current);
    let changed = ui.data_mut(|d| {
        let changed = d.get_temp(shown_id).as_ref() != Some(&shown);
        d.insert_temp(shown_id, shown);
        if changed {
            d.insert_temp(reveal_id, current.is_some());
        }
        changed
    });
    let scroll_to = changed.then(|| match current.and_then(|h| rows.iter().position(|&r| r == h.row)) {
        Some(position) => (position, egui::Align::Center),
        None => (0, egui::Align::TOP),
    });

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        let area = ui.clip_rect();
        // Where the selected match was drawn, to scroll to after the table: the table's
        // own (vertical) scroll area would swallow a horizontal scroll requested inside it.
        let mut reveal = None;
        let mut table = TableBuilder::new(ui)
            .id_salt(("results", view.id))
            .striped(true)
            .resizable(true)
            .auto_shrink(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        if let Some((row, align)) = scroll_to {
            table = table.scroll_to_row(row, Some(align));
        }
        for (i, &w) in widths.iter().enumerate() {
            // The last column soaks up leftover space so the table spans the full width.
            let col = if i + 1 == widths.len() {
                Column::remainder().at_least(w)
            } else {
                Column::initial(w).at_least(MIN_COL_WIDTH)
            };
            table = table.column(col.clip(true));
        }
        table
            .header(row_height, |mut header| {
                for (c, &numeric) in r.data.columns.iter().zip(&numeric) {
                    header.col(|ui| {
                        // Gapless like egui_extras' stripes, so the row reads as one bar.
                        let fill = ui.max_rect().expand2(0.5 * ui.spacing().item_spacing).round_ui();
                        // Past the cell's own clip, which excludes the gaps.
                        let mut painter = ui.painter().clone();
                        painter.set_clip_rect(fill.intersect(area));
                        painter.rect_filled(fill, 0.0, theme::table_header_fill(dark));
                        let name = egui::RichText::new(&c.name).font(header_font());
                        cell(ui, numeric, |ui| {
                            ui.label(name);
                        });
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, rows.len(), |mut row| {
                    let index = rows[row.index()];
                    let data = &r.data.rows[index];
                    for (column, c) in r.data.columns.iter().enumerate() {
                        row.col(|ui| {
                            let value = data.get(&c.name);
                            let kind = ValueKind::of(value);
                            let color = theme::value_color(kind, dark);
                            let (first, hits) = view.hits_in(index, column);
                            cell(ui, kind == ValueKind::Number, |ui| {
                                if hits.is_empty() {
                                    let mut text = egui::RichText::new(cell_text(value)).color(color);
                                    if kind == ValueKind::Null {
                                        text = text.italics();
                                    }
                                    ui.label(text);
                                    return;
                                }
                                let job =
                                    highlighted(ui, &cell_text(value), color, hits, first, view.current);
                                let label = ui.label(job);
                                let selected = (first..first + hits.len()).contains(&view.current);
                                if selected
                                    && ui.data_mut(|d| d.remove_temp::<bool>(reveal_id)).unwrap_or(false)
                                {
                                    reveal = Some(label.rect);
                                }
                            });
                        });
                    }
                });
            });
        if let Some(rect) = reveal {
            ui.scroll_to_rect(rect, None);
        }
    });
}

/// A cell's text with its search matches marked; `first` is the index of `hits[0]`
/// among all hits, so the selected one (`current`) stands out.
fn highlighted(
    ui: &egui::Ui,
    text: &str,
    color: egui::Color32,
    hits: &[Hit],
    first: usize,
    current: usize,
) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let plain = egui::TextFormat::simple(font.clone(), color);
    let mut job = egui::text::LayoutJob::default();
    let mut end = 0;
    for (i, hit) in hits.iter().enumerate() {
        job.append(&text[end..hit.range.start], 0.0, plain.clone());
        let (background, color) = theme::search_match(first + i == current);
        let format = egui::TextFormat { background, ..egui::TextFormat::simple(font.clone(), color) };
        job.append(&text[hit.range.clone()], 0.0, format);
        end = hit.range.end;
    }
    job.append(&text[end..], 0.0, plain);
    job
}

/// Rows-per-page picker, the visible row range, page buttons and export buttons.
/// Laid out right to left.
pub fn pager(ui: &mut egui::Ui, view: &ResultView, page_size: usize) -> Option<Event> {
    let mut event = None;
    let last = view.page_count(page_size) - 1;
    let mut page_button = |ui: &mut egui::Ui, text: &str, hint: &str, enabled: bool, page: usize| {
        if ui.add_enabled(enabled, egui::Button::new(text)).on_hover_text(hint).clicked() {
            event = Some(Event::ShowPage(page));
        }
    };
    page_button(ui, "»", "Last page", view.page < last, last);
    page_button(ui, "›", "Next page", view.page < last, view.page + 1);
    page_button(ui, "‹", "Previous page", view.page > 0, view.page.saturating_sub(1));
    page_button(ui, "«", "First page", view.page > 0, 0);

    let rows = view.page_rows(page_size);
    let total = view.result.data.rows.len();
    if rows.is_empty() {
        ui.weak(format!("0 of {total}"));
    } else {
        ui.label(format!("{}–{} of {total}", rows.start + 1, rows.end));
    }

    let mut size = page_size;
    egui::ComboBox::from_id_salt("page_size").selected_text(format!("{size} / page")).show_ui(ui, |ui| {
        for s in PAGE_SIZES {
            ui.selectable_value(&mut size, s, format!("{s} / page"));
        }
    });
    if size != page_size {
        event = Some(Event::SetPageSize(size));
    }

    ui.separator();
    if ui.button("Export CSV").on_hover_text(format!("Save all {total} rows as a CSV file")).clicked() {
        event = Some(Event::ExportCsv);
    }
    if ui.button("Copy Markdown").on_hover_text("Copy the rows on this page as a Markdown table").clicked() {
        event = Some(Event::CopyPageMarkdown);
    }
    event
}

/// Why the query failed, in the results area; the text can be selected.
pub fn error(ui: &mut egui::Ui, error: &str) {
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        egui::Frame::new().inner_margin(theme::EDITOR_PADDING).show(ui, |ui| {
            ui.label(
                egui::RichText::new("Query failed").font(header_font()).color(ui.visuals().error_fg_color),
            );
            ui.add_space(theme::CELL_PADDING);
            ui.label(egui::RichText::new(error).monospace());
        });
    });
}

/// The status bar's action for a failed query. Laid out right to left.
pub fn error_actions(ui: &mut egui::Ui) -> Option<Event> {
    let copy = ui.button("Copy Markdown").on_hover_text("Copy the error as a Markdown code block");
    copy.clicked().then_some(Event::CopyPageMarkdown)
}

/// Draws a cell's content `CELL_PADDING` away from both of its edges (clipped text
/// stops short of the right one), right-aligned for numbers so their digits line up.
fn cell(ui: &mut egui::Ui, right: bool, add: impl FnOnce(&mut egui::Ui)) {
    let mut clip = ui.clip_rect();
    clip.max.x = clip.max.x.min(ui.max_rect().max.x - theme::CELL_PADDING);
    clip.min.x = clip.min.x.max(ui.max_rect().min.x + theme::CELL_PADDING);
    ui.shrink_clip_rect(clip);
    if right {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::CELL_PADDING);
            add(ui);
        });
    } else {
        ui.add_space(theme::CELL_PADDING);
        add(ui);
    }
}

/// Whether a column holds numbers, judged by its first non-null value.
fn is_numeric(r: &QueryResult, column: &str) -> bool {
    r.data
        .rows
        .iter()
        .take(MEASURED_ROWS)
        .map(|row| ValueKind::of(row.get(column)))
        .find(|&k| k != ValueKind::Null)
        == Some(ValueKind::Number)
}

fn header_font() -> egui::FontId {
    theme::semibold(12.0)
}

/// Width that fits each column's header and (up to `MEASURED_ROWS`) values.
fn fit_column_widths(ctx: &egui::Context, r: &QueryResult) -> Vec<f32> {
    let style = ctx.global_style();
    let header_font = header_font();
    let cell_font = egui::TextStyle::Body.resolve(&style);
    let padding = theme::CELL_PADDING * 2.0;
    ctx.fonts_mut(|fonts| {
        r.data
            .columns
            .iter()
            .map(|c| {
                let header =
                    fonts.layout_no_wrap(c.name.clone(), header_font.clone(), egui::Color32::WHITE).size().x;
                let widest_cell = r
                    .data
                    .rows
                    .iter()
                    .take(MEASURED_ROWS)
                    .map(|row| {
                        let mut width = 0.0;
                        for ch in cell_text(row.get(&c.name)).chars() {
                            width += fonts.glyph_width(&cell_font, ch);
                            if width > MAX_COL_WIDTH {
                                break;
                            }
                        }
                        width
                    })
                    .fold(0.0, f32::max);
                (header.max(widest_cell) + padding).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH)
            })
            .collect()
    })
}
