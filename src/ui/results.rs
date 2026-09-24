use eframe::egui;
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

use super::theme;
use crate::api::QueryResult;
use crate::state::ResultView;

// Bounds for fitted column widths; wider values are clipped but can be resized by hand.
const MIN_COL_WIDTH: f32 = 40.0;
const MAX_COL_WIDTH: f32 = 400.0;
// Only this many rows are measured, to keep huge results cheap.
const MEASURED_ROWS: usize = 1000;

pub fn show(ui: &mut egui::Ui, view: &mut ResultView) {
    let r = &view.result;
    let widths = view.col_widths.get_or_insert_with(|| fit_column_widths(ui.ctx(), r));
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 12.0;

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        let mut table = TableBuilder::new(ui)
            .id_salt(("results", view.id))
            .striped(true)
            .resizable(true)
            .auto_shrink(false);
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
                body.rows(row_height, r.data.rows.len(), |mut row| {
                    let data = &r.data.rows[row.index()];
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

fn cell_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "NULL".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}
