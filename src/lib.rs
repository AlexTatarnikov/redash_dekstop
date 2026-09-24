//! Redash desktop client. See AGENTS.md for the architecture overview.

pub mod api;
pub mod app;
pub mod complete;
pub mod config;
pub mod export;
pub mod mock;
pub mod sql;
pub mod state;
mod ui;

pub use app::RedashApp;
