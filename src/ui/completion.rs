//! Autocomplete popup for the SQL editor. Suggestions come from `complete.rs`; this
//! file decides when the popup is open, handles its keys and draws it under the word.
//!
//! It opens while typing a name (or after `x.`) and with Ctrl+Space. Up/Down pick,
//! Enter/Tab or a click accept, Esc closes; moving the cursor or leaving the editor
//! closes it too.

use eframe::egui;

use super::theme;
use crate::complete::{Completion, apply, complete};
use crate::state::EditorState;

const WIDTH: f32 = 320.0;
const ROW_HEIGHT: f32 = 22.0;
const VISIBLE_ROWS: f32 = 8.0;

#[derive(Clone, Default)]
struct Popup {
    open: bool,
    /// Opened with Ctrl+Space, so it suggests before anything is typed.
    forced: bool,
    selected: usize,
    /// The selection moved by key, so scroll it into view.
    scroll: bool,
    /// Where it was drawn last frame: clicking it must not count as leaving the editor.
    rect: Option<egui::Rect>,
    /// Cursor (chars) and SQL length at the end of last frame, to tell typing from moving.
    last: Option<(usize, usize)>,
}

fn load(ctx: &egui::Context, id: egui::Id) -> Popup {
    ctx.data(|d| d.get_temp(id.with("completion"))).unwrap_or_default()
}

fn store(ctx: &egui::Context, id: egui::Id, popup: Popup) {
    ctx.data_mut(|d| d.insert_temp(id.with("completion"), popup));
}

/// The collapsed cursor of text edit `id`, in chars; `None` while text is selected.
fn cursor(ctx: &egui::Context, id: egui::Id) -> Option<usize> {
    let range = egui::text_edit::TextEditState::load(ctx, id)?.cursor.char_range()?;
    (range.primary == range.secondary).then_some(usize::from(range.primary.index))
}

fn suggestions(ctx: &egui::Context, id: egui::Id, ed: &EditorState, popup: &Popup) -> Option<Completion> {
    if !popup.open {
        return None;
    }
    complete(&ed.sql, cursor(ctx, id)?, ed.tables(), popup.forced)
}

/// Call before drawing text edit `id`: handles the popup's keys so the editor doesn't
/// also act on them. Returns whether the popup is open; the editor should then leave
/// Esc to it (see [`egui::EventFilter`]).
pub fn handle_keys(ui: &egui::Ui, id: egui::Id, ed: &mut EditorState) -> bool {
    let ctx = ui.ctx();
    let mut popup = load(ctx, id);
    if !ui.memory(|m| m.has_focus(id)) {
        return popup.open;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space)) {
        popup = Popup { open: true, forced: true, last: popup.last, ..Default::default() };
    }
    let Some(completion) = suggestions(ctx, id, ed, &popup) else {
        store(ctx, id, popup);
        return false;
    };
    let n = completion.items.len();
    let mut accept = false;
    ui.input_mut(|i| {
        let mut key = |key| i.consume_key(egui::Modifiers::NONE, key);
        if key(egui::Key::ArrowDown) {
            popup.selected = (popup.selected + 1) % n;
            popup.scroll = true;
        }
        if key(egui::Key::ArrowUp) {
            popup.selected = (popup.selected + n - 1) % n;
            popup.scroll = true;
        }
        if key(egui::Key::Escape) {
            popup.open = false;
        }
        accept = key(egui::Key::Enter) || key(egui::Key::Tab);
    });
    if accept {
        insert(ctx, id, ed, &completion, popup.selected.min(n - 1), &mut popup);
    }
    let open = popup.open;
    store(ctx, id, popup);
    open
}

/// Call after drawing text edit `id`: opens or closes the popup depending on what
/// happened in the editor, and draws it.
pub fn show(ui: &egui::Ui, id: egui::Id, out: &egui::text_edit::TextEditOutput, ed: &mut EditorState) {
    let ctx = ui.ctx();
    let mut popup = load(ctx, id);
    let cursor = cursor(ctx, id);
    let now = cursor.map(|c| (c, ed.sql.chars().count()));
    if out.response.response.changed() {
        // Typing a name, or `alias.`, opens it; typing anything else closes it.
        let before = cursor.and_then(|c| c.checked_sub(1)).and_then(|c| ed.sql.chars().nth(c));
        popup.open = before.is_some_and(|ch| ch == '.' || ch == '_' || ch.is_alphanumeric());
        popup.forced &= popup.open;
        popup.selected = 0;
    } else if now != popup.last {
        popup.open = false;
    }
    let over_popup = popup.rect.zip(ctx.pointer_hover_pos()).is_some_and(|(r, p)| r.contains(p));
    if !ui.memory(|m| m.has_focus(id)) && !over_popup {
        popup.open = false;
    }
    popup.last = now;

    let Some(completion) = suggestions(ctx, id, ed, &popup) else {
        popup.rect = None;
        store(ctx, id, popup);
        return;
    };
    popup.selected = popup.selected.min(completion.items.len() - 1);
    let word_start = out.galley.pos_from_cursor(egui::text::CCursor::new(completion.replace.start));
    let pos = out.galley_pos + word_start.left_bottom().to_vec2() + egui::vec2(0.0, 2.0);

    let mut clicked = None;
    let area = egui::Area::new(id.with("completion_popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            theme::popup_frame(ui.style()).show(ui, |ui| {
                ui.set_width(WIDTH);
                // An area's content only gets last frame's size, so ask for the height.
                let height = ROW_HEIGHT * VISIBLE_ROWS.min(completion.items.len() as f32);
                let scroll = egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height);
                scroll.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, item) in completion.items.iter().enumerate() {
                        let selected = i == popup.selected;
                        let row = row(ui, &item.text, &item.detail, selected);
                        if selected && popup.scroll {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        });
    popup.rect = Some(area.response.rect);
    popup.scroll = false;
    if let Some(i) = clicked {
        insert(ctx, id, ed, &completion, i, &mut popup);
        ctx.memory_mut(|m| m.request_focus(id));
        ctx.request_repaint();
    }
    store(ctx, id, popup);
}

/// One suggestion: the name on the left, its type or kind on the right.
fn row(ui: &mut egui::Ui, text: &str, detail: &str, selected: bool) -> egui::Response {
    let size = egui::vec2(ui.available_width(), ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text));
    let visuals = ui.visuals();
    let radius = visuals.widgets.hovered.corner_radius;
    if selected {
        ui.painter().rect_filled(rect, radius, visuals.selection.bg_fill);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, visuals.widgets.hovered.weak_bg_fill);
    }
    let inner = rect.shrink2(egui::vec2(6.0, 0.0));
    let name_font = egui::TextStyle::Monospace.resolve(ui.style());
    let detail_font = egui::TextStyle::Small.resolve(ui.style());
    ui.painter().text(inner.left_center(), egui::Align2::LEFT_CENTER, text, name_font, visuals.text_color());
    ui.painter().text(
        inner.right_center(),
        egui::Align2::RIGHT_CENTER,
        detail,
        detail_font,
        visuals.weak_text_color(),
    );
    response
}

/// Accepts suggestion `index`: replaces the typed part and puts the cursor after it.
fn insert(
    ctx: &egui::Context,
    id: egui::Id,
    ed: &mut EditorState,
    completion: &Completion,
    index: usize,
    popup: &mut Popup,
) {
    let Some(item) = completion.items.get(index) else { return };
    let (sql, cursor) = apply(&ed.sql, &completion.replace, &item.text);
    ed.sql = sql;
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor))));
    state.store(ctx, id);
    popup.open = false;
    popup.last = Some((cursor, ed.sql.chars().count()));
}
