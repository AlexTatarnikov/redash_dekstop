//! Redash desktop client. See AGENTS.md for the architecture overview.

pub mod api;
pub mod app;
pub mod complete;
pub mod config;
pub mod export;
pub mod history;
pub mod mock;
pub mod schema;
pub mod sql;
pub mod state;
mod ui;
pub mod vars;

pub use app::RedashApp;
