//! Runtime: owns the [`AppState`], performs [`Effect`]s (network calls on
//! background threads, settings I/O) and feeds their results back as [`Event`]s.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use eframe::egui;

use crate::api::Client;
use crate::config::ConfigStore;
use crate::state::{AppState, Effect, Event};
use crate::ui;

/// Asks where to save a file, given a suggested name; `None` means cancelled.
pub type SaveDialog = Box<dyn FnMut(&str) -> Option<PathBuf>>;

pub struct RedashApp {
    state: AppState,
    store: ConfigStore,
    save_dialog: SaveDialog,
    /// Effects produced before the first frame, when no `egui::Context` exists yet.
    pending: Vec<Effect>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    themed: bool,
}

impl RedashApp {
    pub fn new(store: ConfigStore) -> Self {
        let (state, pending) = AppState::new(store.load());
        let (tx, rx) = channel();
        Self { state, store, save_dialog: Box::new(native_save_dialog), pending, tx, rx, themed: false }
    }

    /// Replaces the native save dialog, e.g. so tests can save without a window.
    pub fn with_save_dialog(mut self, dialog: impl FnMut(&str) -> Option<PathBuf> + 'static) -> Self {
        self.save_dialog = Box::new(dialog);
        self
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Draws one frame. Used by both eframe and the UI tests.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            ui::theme::apply(&ctx);
            self.themed = true;
            ctx.request_repaint(); // the theme applies from the next frame
        }
        for effect in std::mem::take(&mut self.pending) {
            self.perform(&ctx, effect);
        }
        while let Ok(event) = self.rx.try_recv() {
            self.dispatch(&ctx, event);
        }
        if let Some(event) = ui::show(ui, &mut self.state) {
            self.dispatch(&ctx, event);
        }
    }

    fn dispatch(&mut self, ctx: &egui::Context, event: Event) {
        for effect in self.state.update(event) {
            self.perform(ctx, effect);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, effect: Effect) {
        match effect {
            Effect::LoadDataSources(config) => self.spawn(ctx, move || {
                Event::DataSourcesLoaded(Client::new(&config).data_sources().map_err(|e| e.to_string()))
            }),
            Effect::Execute { config, data_source_id, sql } => self.spawn(ctx, move || {
                Event::QueryFinished(
                    Client::new(&config).execute(data_source_id, &sql).map_err(|e| e.to_string()),
                )
            }),
            Effect::LoadSchema { config, data_source_id } => self.spawn(ctx, move || Event::SchemaLoaded {
                data_source_id,
                result: Client::new(&config).schema(data_source_id).map_err(|e| e.to_string()),
            }),
            Effect::SaveConfig(config) => {
                if let Err(e) = self.store.save(&config) {
                    self.dispatch(ctx, Event::ConfigError(format!("Could not save settings: {e}")));
                }
            }
            Effect::ClearConfig => {
                if let Err(e) = self.store.clear() {
                    self.dispatch(ctx, Event::ConfigError(format!("Could not remove settings: {e}")));
                }
            }
            Effect::CopyToClipboard(text) => ctx.copy_text(text),
            Effect::SaveCsv { file_name, contents } => match (self.save_dialog)(&file_name) {
                Some(path) => self.spawn(ctx, move || {
                    Event::CsvSaved(
                        std::fs::write(&path, contents).map(|()| Some(path)).map_err(|e| e.to_string()),
                    )
                }),
                None => self.dispatch(ctx, Event::CsvSaved(Ok(None))),
            },
        }
    }

    /// Runs `job` on a background thread and wakes the UI when it finishes.
    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce() -> Event + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            // The receiver only disappears when the app is shutting down.
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }
}

/// The OS "Save" dialog. Modal: it blocks the UI until the user answers.
fn native_save_dialog(file_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_file_name(file_name).add_filter("CSV", &["csv"]).save_file()
}

impl eframe::App for RedashApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
