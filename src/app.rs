use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use anyhow::Result;
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

use crate::api::{Client, DataSource, QueryResult};
use crate::config::Config;

/// Results of background work, delivered back to the UI thread.
enum Msg {
    Connected(Result<Vec<DataSource>>),
    QueryDone(Result<QueryResult>),
}

enum Screen {
    Setup(SetupState),
    Editor(EditorState),
}

#[derive(Default)]
struct SetupState {
    host: String,
    api_key: String,
    connecting: bool,
    error: Option<String>,
}

struct EditorState {
    client: Client,
    data_sources: Vec<DataSource>,
    loading_sources: bool,
    selected_source: Option<i64>,
    sql: String,
    running: bool,
    result: Option<QueryResult>,
    /// Column widths fitted to the current result's contents.
    col_widths: Vec<f32>,
    /// Bumped per result so the table forgets widths from the previous one.
    result_seq: u64,
    error: Option<String>,
}

pub struct RedashApp {
    screen: Screen,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl RedashApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel();
        let mut app = Self {
            screen: Screen::Setup(SetupState::default()),
            tx,
            rx,
        };
        if let Some(config) = Config::load() {
            app.open_editor(&cc.egui_ctx, &config);
        }
        app
    }

    /// Runs `job` on a background thread and wakes the UI when it finishes.
    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }

    fn open_editor(&mut self, ctx: &egui::Context, config: &Config) {
        let client = Client::new(&config.host, &config.api_key);
        self.load_data_sources(ctx, &client);
        self.screen = Screen::Editor(EditorState {
            client,
            data_sources: Vec::new(),
            loading_sources: true,
            selected_source: None,
            sql: "SELECT 1".into(),
            running: false,
            result: None,
            col_widths: Vec::new(),
            result_seq: 0,
            error: None,
        });
    }

    fn load_data_sources(&self, ctx: &egui::Context, client: &Client) {
        let client = client.clone();
        self.spawn(ctx, move || Msg::Connected(client.data_sources()));
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        let mut open = None;
        while let Ok(msg) = self.rx.try_recv() {
            match (&mut self.screen, msg) {
                (Screen::Setup(setup), Msg::Connected(res)) => {
                    setup.connecting = false;
                    match res {
                        Ok(_) => {
                            // Credentials work: persist them. The editor reloads
                            // data sources itself once it opens.
                            let config = Config {
                                host: setup.host.trim().to_string(),
                                api_key: setup.api_key.trim().to_string(),
                            };
                            if let Err(e) = config.save() {
                                setup.error = Some(format!("Could not save settings: {e}"));
                                continue;
                            }
                            open = Some(config);
                        }
                        Err(e) => setup.error = Some(e.to_string()),
                    }
                }
                (Screen::Editor(ed), Msg::Connected(res)) => {
                    ed.loading_sources = false;
                    match res {
                        Ok(sources) => {
                            if ed.selected_source.is_none() {
                                ed.selected_source = sources.first().map(|s| s.id);
                            }
                            ed.data_sources = sources;
                        }
                        Err(e) => ed.error = Some(format!("Failed to load data sources: {e}")),
                    }
                }
                (Screen::Editor(ed), Msg::QueryDone(res)) => {
                    ed.running = false;
                    match res {
                        Ok(r) => {
                            ed.col_widths = fit_column_widths(ctx, &r);
                            ed.result_seq += 1;
                            ed.result = Some(r);
                            ed.error = None;
                        }
                        Err(e) => ed.error = Some(e.to_string()),
                    }
                }
                _ => {}
            }
        }
        if let Some(config) = open {
            self.open_editor(ctx, &config);
        }
    }
}

impl eframe::App for RedashApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_messages(&ctx);

        let mut action = None;
        match &mut self.screen {
            Screen::Setup(setup) => setup_ui(ui, setup, &mut action),
            Screen::Editor(ed) => editor_ui(ui, ed, &mut action),
        }

        match action {
            Some(Action::Connect) => {
                if let Screen::Setup(setup) = &mut self.screen {
                    setup.connecting = true;
                    setup.error = None;
                    let client = Client::new(&setup.host, &setup.api_key);
                    self.load_data_sources(&ctx, &client);
                }
            }
            Some(Action::Run) => {
                if let Screen::Editor(ed) = &mut self.screen
                    && let Some(source) = ed.selected_source
                {
                    ed.running = true;
                    ed.error = None;
                    let client = ed.client.clone();
                    let sql = ed.sql.clone();
                    self.spawn(&ctx, move || Msg::QueryDone(client.execute(source, &sql)));
                }
            }
            Some(Action::ReloadSources) => {
                if let Screen::Editor(ed) = &mut self.screen {
                    ed.loading_sources = true;
                    ed.error = None;
                    let client = ed.client.clone();
                    self.load_data_sources(&ctx, &client);
                }
            }
            Some(Action::Disconnect) => {
                // Keep the host prefilled so reconnecting is quick.
                let host = match &self.screen {
                    Screen::Editor(ed) => ed.client.base_url().to_string(),
                    Screen::Setup(s) => s.host.clone(),
                };
                let error = Config::clear()
                    .err()
                    .map(|e| format!("Could not remove settings: {e}"));
                self.screen = Screen::Setup(SetupState {
                    host,
                    error,
                    ..Default::default()
                });
            }
            None => {}
        }
    }
}

enum Action {
    Connect,
    Run,
    ReloadSources,
    Disconnect,
}

fn setup_ui(ui: &mut egui::Ui, s: &mut SetupState, action: &mut Option<Action>) {
    egui::CentralPanel::default_margins().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.2);
            ui.heading("Connect to Redash");
            ui.add_space(16.0);

            // Grid doesn't center itself, so pad it to the middle of the window.
            const FORM_WIDTH: f32 = 400.0;
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - FORM_WIDTH) / 2.0).max(0.0));
                egui::Grid::new("setup_form")
                    .num_columns(2)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        ui.label("API host");
                        ui.add(
                            egui::TextEdit::singleline(&mut s.host)
                                .hint_text("https://redash.example.com")
                                .desired_width(320.0),
                        );
                        ui.end_row();

                        ui.label("API key");
                        let key = ui.add(
                            egui::TextEdit::singleline(&mut s.api_key)
                                .password(true)
                                .hint_text("Profile > Settings > API Key")
                                .desired_width(320.0),
                        );
                        ui.end_row();

                        let submit =
                            key.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let ready = !s.host.trim().is_empty()
                            && !s.api_key.trim().is_empty()
                            && !s.connecting;
                        ui.label("");
                        ui.horizontal(|ui| {
                            let clicked = ui
                                .add_enabled(ready, egui::Button::new("Connect"))
                                .clicked();
                            if ready && (clicked || submit) {
                                *action = Some(Action::Connect);
                            }
                            if s.connecting {
                                ui.spinner();
                            }
                        });
                        ui.end_row();
                    });
            });

            if let Some(err) = &s.error {
                ui.add_space(12.0);
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
        });
    });
}

fn editor_ui(ui: &mut egui::Ui, ed: &mut EditorState, action: &mut Option<Action>) {
    let can_run = ed.selected_source.is_some() && !ed.running && !ed.sql.trim().is_empty();
    if can_run && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
        *action = Some(Action::Run);
    }

    egui::Panel::top("toolbar").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let selected = ed
                .data_sources
                .iter()
                .find(|s| Some(s.id) == ed.selected_source)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Select data source".into());
            egui::ComboBox::from_id_salt("data_source")
                .selected_text(selected)
                .width(220.0)
                .show_ui(ui, |ui| {
                    for s in &ed.data_sources {
                        ui.selectable_value(
                            &mut ed.selected_source,
                            Some(s.id),
                            format!("{} ({})", s.name, s.kind),
                        );
                    }
                });
            if ed.loading_sources {
                ui.spinner();
            } else if ui
                .small_button("Reload")
                .on_hover_text("Reload data sources")
                .clicked()
            {
                *action = Some(Action::ReloadSources);
            }

            let run = ui
                .add_enabled(can_run, egui::Button::new("▶ Execute"))
                .on_hover_text("Cmd/Ctrl + Enter");
            if run.clicked() {
                *action = Some(Action::Run);
            }
            if ed.running {
                ui.spinner();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Disconnect").clicked() {
                    *action = Some(Action::Disconnect);
                }
                ui.weak(ed.client.base_url());
            });
        });
        ui.add_space(4.0);
    });

    egui::Panel::bottom("status").show(ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(err) = &ed.error {
                ui.colored_label(ui.visuals().error_fg_color, err);
            } else if let Some(r) = &ed.result {
                ui.label(format!("{} rows · {:.3}s", r.data.rows.len(), r.runtime));
            } else {
                ui.weak("Ready");
            }
        });
    });

    egui::Panel::top("sql_editor")
        .resizable(true)
        .default_size(240.0)
        .min_size(80.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_sized(
                    ui.available_size(),
                    egui::TextEdit::multiline(&mut ed.sql)
                        .code_editor()
                        .hint_text("Write your SQL here…"),
                );
            });
        });

    egui::CentralPanel::default_margins().show(ui, |ui| match &ed.result {
        Some(r) => results_table(ui, r, &ed.col_widths, ed.result_seq),
        None => {
            ui.centered_and_justified(|ui| ui.weak("Run a query to see results"));
        }
    });
}

// Bounds for fitted column widths; wider values are clipped but can be resized by hand.
const MIN_COL_WIDTH: f32 = 40.0;
const MAX_COL_WIDTH: f32 = 400.0;
// Only this many rows are measured, to keep huge results cheap.
const MEASURED_ROWS: usize = 1000;

/// Width that fits each column's header and (up to `MEASURED_ROWS`) values.
fn fit_column_widths(ctx: &egui::Context, r: &QueryResult) -> Vec<f32> {
    let header_font = egui::TextStyle::Body.resolve(&ctx.global_style());
    let cell_font = egui::TextStyle::Monospace.resolve(&ctx.global_style());
    let padding = ctx.global_style().spacing.item_spacing.x * 2.0;
    ctx.fonts_mut(|fonts| {
        r.data
            .columns
            .iter()
            .map(|c| {
                let header = fonts
                    .layout_no_wrap(c.name.clone(), header_font.clone(), egui::Color32::WHITE)
                    .size()
                    .x;
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

fn results_table(ui: &mut egui::Ui, r: &QueryResult, widths: &[f32], seq: u64) {
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
    egui::ScrollArea::horizontal()
        .auto_shrink(false)
        .show(ui, |ui| {
            let mut table = TableBuilder::new(ui)
                .id_salt(("results", seq))
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
                            ui.strong(&c.name);
                        });
                    }
                })
                .body(|body| {
                    body.rows(row_height, r.data.rows.len(), |mut row| {
                        let data = &r.data.rows[row.index()];
                        for c in &r.data.columns {
                            row.col(|ui| {
                                ui.monospace(cell_text(data.get(&c.name)));
                            });
                        }
                    });
                });
        });
}

fn cell_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "NULL".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}
