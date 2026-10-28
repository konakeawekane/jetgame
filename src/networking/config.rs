//! Python launcher configuration; network settings pass through without Rust duplication.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub python: Option<PathBuf>,
    pub agent_script: Option<PathBuf>,
    #[serde(flatten)]
    pub settings: Map<String, Value>,
    #[serde(skip)]
    pub root: PathBuf,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            python: None,
            agent_script: None,
            settings: Map::new(),
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl NetworkConfig {
    pub fn load_default() -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let parent = executable.parent().unwrap();
        let root = if parent.join("networking/agent_main.py").is_file() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()?
        };
        let local = root.join("networking.local.json");
        let path = if local.is_file() {
            local
        } else {
            root.join("networking.json")
        };
        let mut config = if path.is_file() {
            Self::from_json(&std::fs::read(path)?)?
        } else {
            Self::default()
        };
        config.root = root;
        if let Some(python) =
            std::env::var_os("JETGAME_PYTHON").or_else(|| std::env::var_os("JETGAME_SERVER_PYTHON"))
        {
            config.python = Some(python.into());
        }
        Ok(config)
    }
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.settings.insert(key.into(), value.into());
    }
    fn from_json(bytes: &[u8]) -> io::Result<Self> {
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
    pub(super) fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.into()
        } else {
            self.root.join(path)
        }
    }
    pub(super) fn python_executable(&self) -> io::Result<PathBuf> {
        if let Some(path) = &self.python {
            return Ok(self.resolve(path));
        }
        let bundled = self.root.join(if cfg!(windows) {
            "runtime/python/python.exe"
        } else {
            "runtime/python/bin/python3"
        });
        if bundled.is_file() {
            return Ok(bundled);
        }
        if cfg!(debug_assertions) {
            return Ok(if cfg!(windows) { "python" } else { "python3" }.into());
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Networking Python runtime is missing; configure --python or package runtime/python",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_bom_and_preserves_python_settings() {
        let config = NetworkConfig::from_json(b"\xef\xbb\xbf{\"host_port\":29012}").unwrap();
        assert_eq!(config.settings["host_port"], 29012);
    }
}
