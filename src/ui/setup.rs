use eframe::egui;

use super::theme;
use crate::state::{Event, SetupState};

const FORM_WIDTH: f32 = 400.0;

pub fn show(ui: &mut egui::Ui, s: &mut SetupState) -> Option<Event> {
    let mut event = None;
    egui::CentralPanel::default_margins().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.2);
            ui.heading("Connect to Redash");
            ui.add_space(16.0);

            // Grid doesn't center itself, so pad it to the middle of the window.
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - FORM_WIDTH) / 2.0).max(0.0));
                egui::Grid::new("setup_form").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                    let label = ui.label("API host");
                    ui.add(
                        egui::TextEdit::singleline(&mut s.host)
                            .hint_text("https://redash.example.com")
                            .desired_width(320.0),
                    )
                    .labelled_by(label.id);
                    ui.end_row();

                    let label = ui.label("API key");
                    let key = ui
                        .add(
                            egui::TextEdit::singleline(&mut s.api_key)
                                .password(true)
                                .hint_text("Profile > Settings > API Key")
                                .desired_width(320.0),
                        )
                        .labelled_by(label.id);
                    ui.end_row();

                    let submit = key.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.label("");
                    ui.horizontal(|ui| {
                        let ready = s.can_connect();
                        let clicked = ui.add_enabled(ready, theme::primary_button("Connect")).clicked();
                        if ready && (clicked || submit) {
                            event = Some(Event::Connect);
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
    event
}
