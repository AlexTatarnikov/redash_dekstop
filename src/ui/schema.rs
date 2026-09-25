//! The schema panel (a sidebar tab): the selected data source's tables, each
//! expanding to its columns and their types, narrowed by a filter.

use eframe::egui;

use super::{editor, theme};
use crate::schema;
use crate::state::{EditorState, Event, Schema};

/// Height of a column row: tighter than a control, so long tables stay scannable.
const ROW_HEIGHT: f32 = 18.0;
/// Height of a table's row, a little taller than a column's so it reads as a heading.
const TABLE_ROW_HEIGHT: f32 = 22.0;
/// Width kept at the right of every row for its insert button, shown on hover.
const INSERT_WIDTH: f32 = 20.0;

/// Icon button that re-reads the schema. Its label is also its tooltip.
fn refresh(ui: &mut egui::Ui) -> Option<Event> {
    let size = ui.spacing().interact_size.y;
    theme::icon_button(ui, theme::Icon::Refresh, size, "Refresh schema")
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
        return ui
            .horizontal(|ui| {
                ui.weak("This data source doesn't list its tables");
                refresh(ui)
            })
            .inner;
    }

    let mut event = None;
    ui.horizontal(|ui| {
        let label = ui.label("Filter");
        // As tall as the refresh button beside it, not a bare line of text.
        let height = ui.spacing().interact_size.y;
        let input = egui::TextEdit::singleline(&mut ed.schema_filter)
            .hint_text("Table or column name")
            .margin(egui::Margin::symmetric(8, 4))
            .vertical_align(egui::Align::Center)
            .min_size(egui::vec2(0.0, height))
            .desired_width(ui.available_width() - height - ui.spacing().item_spacing.x);
        ui.add(input).labelled_by(label.id);
        event = refresh(ui);
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
    let mut insert = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // Tables right under each other: their rows' height is enough to tell them apart.
        ui.spacing_mut().item_spacing.y = 0.0;
        for m in matches {
            let id = ui.make_persistent_id(("schema_table", source_id, &m.table.name));
            let mut state =
                egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
            // Show the columns that matched; otherwise keep what the user expanded.
            if filtering && m.by_column {
                state.set_open(true);
            }
            let (header, inserted) = table_row(ui, &m.table.name, state.is_open());
            // The full name too: a long one is cut short in the row.
            let columns = plural(m.table.columns.len(), "column");
            let header = header.on_hover_ui(|ui| {
                ui.label(egui::RichText::new(&m.table.name).monospace());
                ui.weak(&columns);
            });
            if inserted {
                insert = Some(m.table.name.clone());
            } else if header.clicked() {
                state.toggle(ui);
            }
            state.show_body_indented(&header, ui, |ui| {
                for column in m.columns {
                    if column_row(ui, &column.name, column.kind.as_deref()) {
                        insert = Some(column.name.clone());
                    }
                }
            });
        }
    });
    if let Some(name) = insert {
        editor::insert_at_cursor(ui.ctx(), &mut ed.sql, &name);
    }
    event
}

/// The insert button at the right end of a hovered row, putting `name` in the query.
/// Whether it was clicked.
fn insert_button(ui: &mut egui::Ui, row: egui::Rect, name: &str) -> bool {
    if !ui.rect_contains_pointer(row) || !ui.is_enabled() {
        return false;
    }
    let size = INSERT_WIDTH.min(row.height());
    let rect = egui::Rect::from_center_size(
        egui::pos2(row.right() - INSERT_WIDTH / 2.0, row.center().y),
        egui::Vec2::splat(size),
    );
    let label = format!("Insert {name}");
    theme::icon_button_at(ui, rect, theme::Icon::Insert, &label)
        .on_hover_text("Insert into the query at the cursor, or over the selected text")
        .clicked()
}

/// Paints `text` on one line from `left`, centred on the row, cut short with "…" at `right`.
fn paint_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    left: egui::Pos2,
    right: f32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left.x).max(0.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(left - egui::vec2(0.0, galley.size().y / 2.0), galley, color);
}

/// A table's name after a table icon, clicked to show or hide its columns. The icon
/// takes the accent colour while they are shown. Also whether its insert button was
/// clicked.
fn table_row(ui: &mut egui::Ui, name: &str, open: bool) -> (egui::Response, bool) {
    let highlight = theme::RowHighlight::reserve(ui);
    let size = egui::vec2(ui.available_width(), TABLE_ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), open, name));
    // Still hovered with the pointer on the insert button, which is above the row.
    highlight.fill(ui, rect);
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let icon = egui::Rect::from_min_size(rect.min, egui::vec2(TABLE_ROW_HEIGHT, TABLE_ROW_HEIGHT));
        let icon_color = if open { visuals.hyperlink_color } else { visuals.weak_text_color() };
        theme::paint_icon(ui.painter(), icon, theme::Icon::Table, icon_color);
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let left = egui::pos2(icon.right() + 2.0, rect.center().y);
        paint_line(ui, name, font, visuals.text_color(), left, rect.right() - INSERT_WIDTH);
    }
    theme::pointer(&response);
    let inserted = insert_button(ui, rect, name);
    (response, inserted)
}

/// A column's name, with its type (when Redash knows it) right-aligned. Whether its
/// insert button was clicked.
fn column_row(ui: &mut egui::Ui, name: &str, kind: Option<&str>) -> bool {
    let highlight = theme::RowHighlight::reserve(ui);
    let row = ui.scope(|ui| {
        let kind_width = kind.map_or(0.0, |kind| type_width(ui, name, kind));
        egui::Sides::new().height(ROW_HEIGHT).shrink_left().truncate().show(
            ui,
            |ui| {
                ui.add(egui::Label::new(egui::RichText::new(name).monospace()).truncate());
            },
            |ui| {
                // Room for the insert button and a gap, kept while it is hidden so nothing moves.
                ui.add_space(INSERT_WIDTH + 4.0);
                if let Some(kind) = kind {
                    ui.scope(|ui| {
                        ui.set_max_width(kind_width);
                        ui.add(egui::Label::new(egui::RichText::new(kind).weak()).truncate());
                    });
                }
            },
        );
    });
    let rect = row.response.rect;
    highlight.fill(ui, rect);
    insert_button(ui, rect, name)
}

/// Width for a column's type beside its name: all it needs if both fit, otherwise
/// what the name leaves, but at least `MIN_TYPE_WIDTH` (or its own width, if less):
/// the name matters more, but a long one shouldn't hide the type altogether.
fn type_width(ui: &egui::Ui, name: &str, kind: &str) -> f32 {
    const MIN_TYPE_WIDTH: f32 = 70.0;
    let measure = |text: &str, style: egui::TextStyle| {
        let font = style.resolve(ui.style());
        ui.fonts_mut(|f| f.layout_no_wrap(text.into(), font, egui::Color32::WHITE).size().x)
    };
    let (name, kind) = (measure(name, egui::TextStyle::Monospace), measure(kind, egui::TextStyle::Body));
    let gap = 2.0 * ui.spacing().item_spacing.x;
    let room = ui.available_width() - INSERT_WIDTH - 4.0 - gap;
    kind.min((room - name).max(MIN_TYPE_WIDTH))
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}
