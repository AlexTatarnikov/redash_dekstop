//! Connection settings and where they are persisted.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Connection settings persisted between app launches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub api_key: String,
}

/// Loads and saves [`Config`]. Tests use [`ConfigStore::memory`] so they never
/// touch the user's real settings file.
#[derive(Debug)]
pub enum ConfigStore {
    File(PathBuf),
    Memory(Mutex<Option<Config>>),
}

impl ConfigStore {
    /// `~/Library/Application Support/redash-desktop/config.json` on macOS.
    pub fn default_file() -> Result<Self> {
        let dir = dirs::config_dir().context("could not determine config directory")?;
        Ok(Self::File(dir.join("redash-desktop").join("config.json")))
    }

    pub fn memory(initial: Option<Config>) -> Self {
        Self::Memory(Mutex::new(initial))
    }

    /// Returns `None` if nothing has been saved yet or the file is unreadable.
    pub fn load(&self) -> Option<Config> {
        match self {
            Self::File(path) => {
                let raw = fs::read_to_string(path).ok()?;
                serde_json::from_str(&raw).ok()
            }
            Self::Memory(slot) => slot.lock().ok()?.clone(),
        }
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        match self {
            Self::File(path) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, serde_json::to_string_pretty(config)?)?;
                // The file holds an API key, so keep it readable by the owner only.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
                }
            }
            Self::Memory(slot) => *lock(slot)? = Some(config.clone()),
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        match self {
            Self::File(path) => match fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            },
            Self::Memory(slot) => *lock(slot)? = None,
        }
        Ok(())
    }
}

fn lock(slot: &Mutex<Option<Config>>) -> Result<std::sync::MutexGuard<'_, Option<Config>>> {
    slot.lock().map_err(|_| anyhow::anyhow!("config store lock poisoned"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trips_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::File(dir.path().join("nested").join("config.json"));
        assert_eq!(store.load(), None);

        let config = Config { host: "https://redash.example.com".into(), api_key: "k".into() };
        store.save(&config).unwrap();
        assert_eq!(store.load(), Some(config));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let ConfigStore::File(path) = &store else { unreachable!() };
            let mode = fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        store.clear().unwrap();
        assert_eq!(store.load(), None);
        store.clear().unwrap(); // clearing twice is fine
    }
}
