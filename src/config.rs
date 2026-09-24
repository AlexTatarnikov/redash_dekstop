use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Connection settings persisted between app launches.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub api_key: String,
}

impl Config {
    fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir().context("could not determine config directory")?;
        Ok(dir.join("redash-desktop").join("config.json"))
    }

    /// Returns `None` if nothing has been saved yet or the file is unreadable.
    pub fn load() -> Option<Config> {
        let raw = fs::read_to_string(Self::path().ok()?).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string_pretty(self)?)?;
        // The file holds an API key, so keep it readable by the owner only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn clear() -> Result<()> {
        match fs::remove_file(Self::path()?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}
