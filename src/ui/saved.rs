//! The saved queries panel (a sidebar tab): queries the user kept with Save, newest
//! first. Clicking one restores its SQL, data source and variables; right-clicking
//! renames (in place) or deletes it.

use eframe::egui;

use super::history;
use crate::saved::SavedQuery;
use crate::state::{EditorState, Event, Renaming};

pub fn show(ui: &mut egui::Ui, ed: &mut EditorState) -> Option<Event> {
    let mut event = None;
    ui.weak("Click one to restore it. Right-click to rename or delete.");
    ui.add_space(4.0);
    if ed.saved.is_empty() {
        ui.weak("No saved queries. Save the editor's query with Save (Cmd/Ctrl + S).");
        return event;
    }

    let now = (ed.clock)();
    // The fields directly, so the name being edited can change while `saved` is borrowed.
    let (saved, renaming) = (&ed.saved, &mut ed.renaming);
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (i, query) in saved.iter().enumerate() {
            let e = match renaming {
                Some(r) if r.index == i => rename_field(ui, r),
                _ => {
                    let age = crate::history::ago(query.saved_at, now);
                    let meta = history::meta(&ed.data_sources, query.data_source_id, &query.variables, age);
                    item(ui, &meta, query, i, !ed.running)
                }
            };
            event = event.take().or(e);
        }
    });
    event
}

/// One query: its name, then data source, variable count and age; right-click for actions.
fn item(ui: &mut egui::Ui, meta: &str, query: &SavedQuery, i: usize, can_restore: bool) -> Option<Event> {
    let mut event = None;
    let title = egui::RichText::new(&query.name);
    let response =
        ui.add_enabled_ui(can_restore, |ui| history::row(ui, ("saved", i), title, meta, &query.sql)).inner;
    if response.clicked() {
        event = Some(Event::RestoreSaved(i));
    }
    if response.double_clicked() {
        event = Some(Event::StartRename(i));
    }
    response.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            event = Some(Event::StartRename(i));
        }
        if ui.button("Delete").clicked() {
            event = Some(Event::DeleteSaved(i));
        }
    });
    event
}

/// The name being edited, focused with its text selected. Enter or clicking away keeps
/// it, Esc cancels.
fn rename_field(ui: &mut egui::Ui, renaming: &mut Renaming) -> Option<Event> {
    let id = egui::Id::new(("rename_saved", renaming.index));
    let (label, out) = ui
        .horizontal(|ui| {
            let label = ui.label("Name");
            let out = egui::TextEdit::singleline(&mut renaming.name)
                .id(id)
                .margin(egui::Margin::symmetric(8, 4))
                .vertical_align(egui::Align::Center)
                .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                .desired_width(f32::INFINITY)
                .show(ui);
            (label, out)
        })
        .inner;
    let response = out.response.response.clone().labelled_by(label.id);
    if response.lost_focus() {
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
        return Some(if cancelled { Event::CancelRename } else { Event::FinishRename });
    }
    if !response.has_focus() {
        // Just started: focus the field with the whole name selected, to type over it.
        response.request_focus();
        let mut state = out.state;
        let end = egui::text::CCursor::new(renaming.name.chars().count());
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
        state.store(ui.ctx(), id);
    }
    None
}
