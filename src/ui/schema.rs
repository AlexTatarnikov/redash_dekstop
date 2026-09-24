//! The schema panel (a sidebar tab): the selected data source's tables, each
//! expanding to its columns and their types, narrowed by a filter.

use eframe::egui;

use crate::schema;
use crate::state::{EditorState, Event, Schema};

/// Height of a column row: tighter than a control, so long tables stay scannable.
const ROW_HEIGHT: f32 = 18.0;

/// The panel's header buttons, drawn right to left.
pub fn actions(ui: &mut egui::Ui, ed: &EditorState) -> Option<Event> {
    let loading = matches!(ed.schema(), Some(Schema::Loading));
    ui.add_enabled(ed.selected_source.is_some() && !loading, egui::Button::new("Refresh"))
        .on_hover_text("Read the tables and columns from the database again")
        .clicked()
        .then_some(Event::RefreshSchema)
}

pub fn show(ui: &mut egui::Ui, ed: &mut EditorState) -> Option<Event> {
    let source = ed.data_sources.iter().find(|s| Some(s.id) == ed.selected_source);
    let Some(source) = source else {
        ui.weak(if ed.loading_sources { "Loading data sources…" } else { "Select a data source" });
        return None;
    };
    let source_id = source.id;
    ui.add(egui::Label::new(egui::RichText::new(format!("Tables in {}", source.name)).weak()).truncate());

    // The fields directly, so the filter below can be edited while `tables` is borrowed.
    let tables = match ed.schemas.get(&source_id) {
        Some(Schema::Loaded(tables)) => tables,
        Some(Schema::Failed(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, format!("Could not load the schema: {e}"));
            return ui.button("Retry").clicked().then_some(Event::RefreshSchema);
        }
        Some(Schema::Loading) | None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Loading schema…");
            });
            return None;
        }
    };
    if tables.is_empty() {
        ui.weak("This data source doesn't list its tables");
        return None;
    }

    ui.horizontal(|ui| {
        let label = ui.label("Filter");
        // As tall as the buttons around it, not a bare line of text.
        let input = egui::TextEdit::singleline(&mut ed.schema_filter)
            .hint_text("Table or column name")
            .margin(egui::Margin::symmetric(8, 4))
            .vertical_align(egui::Align::Center)
            .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
            .desired_width(f32::INFINITY);
        ui.add(input).labelled_by(label.id);
    });
    let matches = schema::filter(tables, &ed.schema_filter);
    let count = if matches.len() == tables.len() {
        plural(tables.len(), "table")
    } else {
        format!("{} of {}", matches.len(), plural(tables.len(), "table"))
    };
    ui.weak(count);
    ui.add_space(2.0);

    let filtering = !ed.schema_filter.trim().is_empty();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for m in matches {
            let name = egui::RichText::new(&m.table.name).monospace();
            egui::CollapsingHeader::new(name)
                .id_salt(("schema_table", source_id, &m.table.name))
                // Show the columns that matched; otherwise keep what the user expanded.
                .open((filtering && m.by_column).then_some(true))
                .show(ui, |ui| {
                    for column in m.columns {
                        column_row(ui, &column.name, column.kind.as_deref());
                    }
                })
                .header_response
                .on_hover_text(plural(m.table.columns.len(), "column"));
        }
    });
    None
}

/// A column's name, with its type (when Redash knows it) right-aligned.
fn column_row(ui: &mut egui::Ui, name: &str, kind: Option<&str>) {
    egui::Sides::new().height(ROW_HEIGHT).shrink_left().truncate().show(
        ui,
        |ui| {
            ui.add(egui::Label::new(egui::RichText::new(name).monospace()).truncate());
        },
        |ui| {
            if let Some(kind) = kind {
                ui.weak(kind);
            }
        },
    );
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}
