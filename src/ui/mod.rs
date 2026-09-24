//! Drawing only: each screen renders its state and returns the [`Event`] the
//! user triggered, if any. Logic lives in `state.rs`.

mod editor;
mod results;
mod setup;

use eframe::egui;

use crate::state::{AppState, Event, Screen};

pub fn show(ui: &mut egui::Ui, state: &mut AppState) -> Option<Event> {
    match &mut state.screen {
        Screen::Setup(s) => setup::show(ui, s),
        Screen::Editor(ed) => editor::show(ui, ed),
    }
}
