//! Connection settings, variables and execution history, and where they are persisted.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::history::Entry;
use crate::vars::Variable;

/// Connection settings persisted between app launches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub api_key: String,
}

/// Loads and saves [`Config`], and the user's variables and history next to it. Tests use
/// [`ConfigStore::memory`] so they never touch the user's real settings files.
#[derive(Debug)]
pub enum ConfigStore {
    /// The config file; variables go to `variables.json` and history to
    /// `history.json` in the same directory.
    File(PathBuf),
    Memory(Mutex<Option<Config>>, Mutex<Vec<Variable>>, Mutex<Vec<Entry>>),
}

impl ConfigStore {
    /// `~/Library/Application Support/redash-desktop/config.json` on macOS.
    pub fn default_file() -> Result<Self> {
        let dir = dirs::config_dir().context("could not determine config directory")?;
        Ok(Self::File(dir.join("redash-desktop").join("config.json")))
    }

    pub fn memory(initial: Option<Config>) -> Self {
        Self::Memory(Mutex::new(initial), Mutex::new(Vec::new()), Mutex::new(Vec::new()))
    }

    /// Returns `None` if nothing has been saved yet or the file is unreadable.
    pub fn load(&self) -> Option<Config> {
        match self {
            Self::File(path) => {
                let raw = fs::read_to_string(path).ok()?;
                serde_json::from_str(&raw).ok()
            }
            Self::Memory(slot, ..) => slot.lock().ok()?.clone(),
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
            Self::Memory(slot, ..) => *lock(slot)? = Some(config.clone()),
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        match self {
            Self::File(path) => match fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            },
            Self::Memory(slot, ..) => *lock(slot)? = None,
        }
        Ok(())
    }

    /// Saved variables; none if nothing has been saved yet.
    pub fn load_variables(&self) -> Result<Vec<Variable>> {
        match self {
            Self::File(path) => match fs::read_to_string(variables_file(path)) {
                Ok(raw) => Ok(serde_json::from_str(&raw)?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(e.into()),
            },
            Self::Memory(_, vars, _) => Ok(lock(vars)?.clone()),
        }
    }

    /// Kept when disconnecting: they don't hold credentials.
    pub fn save_variables(&self, variables: &[Variable]) -> Result<()> {
        match self {
            Self::File(path) => {
                let path = variables_file(path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, serde_json::to_string_pretty(variables)?)?;
            }
            Self::Memory(_, vars, _) => *lock(vars)? = variables.to_vec(),
        }
        Ok(())
    }

    /// Saved history, newest first; empty if nothing has been saved yet.
    pub fn load_history(&self) -> Result<Vec<Entry>> {
        match self {
            Self::File(path) => match fs::read_to_string(history_file(path)) {
                Ok(raw) => Ok(serde_json::from_str(&raw)?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(e.into()),
            },
            Self::Memory(.., history) => Ok(lock(history)?.clone()),
        }
    }

    /// Kept when disconnecting, like variables.
    pub fn save_history(&self, history: &[Entry]) -> Result<()> {
        match self {
            Self::File(path) => {
                let path = history_file(path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, serde_json::to_string_pretty(history)?)?;
            }
            Self::Memory(.., slot) => *lock(slot)? = history.to_vec(),
        }
        Ok(())
    }
}

fn variables_file(config_file: &std::path::Path) -> PathBuf {
    config_file.with_file_name("variables.json")
}

fn history_file(config_file: &std::path::Path) -> PathBuf {
    config_file.with_file_name("history.json")
}

fn lock<T>(slot: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>> {
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

    #[test]
    fn file_store_keeps_variables_next_to_config() {
        use crate::vars::Definition;

        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::File(dir.path().join("nested").join("config.json"));
        assert_eq!(store.load_variables().unwrap(), []);

        let var = Variable {
            id: 0,
            name: "start".into(),
            def: Definition::Value { value: "'2026-01-01'".into() },
            run: None,
        };
        store.save_variables(std::slice::from_ref(&var)).unwrap();
        assert!(dir.path().join("nested").join("variables.json").exists());
        store.clear().unwrap();
        assert_eq!(store.load_variables().unwrap(), [var], "survive disconnecting");
    }

    #[test]
    fn file_store_keeps_history_next_to_config() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::File(dir.path().join("config.json"));
        assert_eq!(store.load_history().unwrap(), []);

        let entry = Entry::new(5, 1, "select 1", &[]);
        store.save_history(std::slice::from_ref(&entry)).unwrap();
        assert!(dir.path().join("history.json").exists());
        assert_eq!(store.load_history().unwrap(), [entry]);
    }
}
