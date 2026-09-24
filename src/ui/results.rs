use eframe::egui;
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

use super::theme;
use crate::api::QueryResult;
use crate::export::cell_text;
use crate::state::{Event, PAGE_SIZES, ResultView};

// Bounds for fitted column widths; wider values are clipped but can be resized by hand.
const MIN_COL_WIDTH: f32 = 40.0;
const MAX_COL_WIDTH: f32 = 400.0;
// Only this many rows are measured, to keep huge results cheap.
const MEASURED_ROWS: usize = 1000;

pub fn show(ui: &mut egui::Ui, view: &mut ResultView, page_size: usize) {
    let rows = view.page_rows(page_size);
    // Widths are fitted to the whole result so columns don't jump between pages.
    let r = &view.result;
    let widths = view.col_widths.get_or_insert_with(|| fit_column_widths(ui.ctx(), r));
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 12.0;

    // Start each newly shown page at its top.
    let shown_page_id = egui::Id::new(("results_page", view.id));
    let page_changed = ui.data_mut(|d| {
        let changed = d.get_temp(shown_page_id) != Some(rows.start);
        d.insert_temp(shown_page_id, rows.start);
        changed
    });

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        let mut table = TableBuilder::new(ui)
            .id_salt(("results", view.id))
            .striped(true)
            .resizable(true)
            .auto_shrink(false);
        if page_changed {
            table = table.scroll_to_row(0, Some(egui::Align::TOP));
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
                for c in &r.data.columns {
                    header.col(|ui| {
                        ui.label(egui::RichText::new(&c.name).font(header_font()));
                    });
                }
            })
            .body(|body| {
                let page = &r.data.rows[rows];
                body.rows(row_height, page.len(), |mut row| {
                    let data = &page[row.index()];
                    for c in &r.data.columns {
                        row.col(|ui| match data.get(&c.name) {
                            None | Some(Value::Null) => {
                                ui.weak("NULL");
                            }
                            value => {
                                ui.label(cell_text(value));
                            }
                        });
                    }
                });
            });
    });
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

fn header_font() -> egui::FontId {
    theme::semibold(12.0)
}

/// Width that fits each column's header and (up to `MEASURED_ROWS`) values.
fn fit_column_widths(ctx: &egui::Context, r: &QueryResult) -> Vec<f32> {
    let style = ctx.global_style();
    let header_font = header_font();
    let cell_font = egui::TextStyle::Body.resolve(&style);
    let padding = style.spacing.item_spacing.x * 2.0;
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
