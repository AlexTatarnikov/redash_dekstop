mod api;
mod app;
mod config;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Redash")
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Redash",
        options,
        Box::new(|cc| Ok(Box::new(app::RedashApp::new(cc)))),
    )
}
