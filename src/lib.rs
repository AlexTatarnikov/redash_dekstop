//! Redash desktop client. See AGENTS.md for the architecture overview.

pub mod api;
pub mod app;
pub mod config;
pub mod mock;
pub mod state;
mod ui;

pub use app::RedashApp;
