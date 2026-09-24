use redash_desktop::RedashApp;
use redash_desktop::config::{Config, ConfigStore};
use redash_desktop::mock::{MOCK_API_KEY, MockRedash};

fn main() -> anyhow::Result<()> {
    // `--mock` runs against an in-process fake Redash and never touches saved settings.
    let (store, _mock) = if std::env::args().any(|a| a == "--mock") {
        let mock = MockRedash::start()?;
        let config = Config { host: mock.url().into(), api_key: MOCK_API_KEY.into() };
        (ConfigStore::memory(Some(config)), Some(mock))
    } else {
        (ConfigStore::default_file()?, None)
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Redash")
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native("Redash", options, Box::new(|_cc| Ok(Box::new(RedashApp::new(store)))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}
