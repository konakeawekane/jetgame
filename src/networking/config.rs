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
        if let Some(python) = std::env::var_os("JETGAME_PYTHON") {
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
            let path = self.resolve(path);
            if path.parent() == Some(self.root.join("runtime/python").as_path()) {
                validate_bundled_runtime(&path)?;
            }
            return Ok(path);
        }
        let bundled = self.root.join(if cfg!(windows) {
            "runtime/python/python.exe"
        } else {
            "runtime/python/bin/python3"
        });
        if bundled.is_file() {
            validate_bundled_runtime(&bundled)?;
            return Ok(bundled);
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Bundled networking Python is missing. Run scripts/setup-runtime.ps1, then launch again",
        ))
    }
}

fn validate_bundled_runtime(executable: &Path) -> io::Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    let folder = executable.parent().unwrap();
    let stem = std::fs::read_dir(folder)?.filter_map(Result::ok).find_map(|entry| {
        let name = entry.file_name();
        let stem = name.to_str()?.strip_suffix("._pth")?;
        let digits = stem.strip_prefix("python")?;
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| stem.to_owned())
    }).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound,
        "Python runtime is incomplete: missing python*._pth. Extract the entire game ZIP again"))?;
    for file in [
        "python3.dll".to_owned(),
        format!("{stem}.dll"),
        format!("{stem}.zip"),
        "vcruntime140.dll".to_owned(),
        "vcruntime140_1.dll".to_owned(),
        "_socket.pyd".to_owned(),
        "_overlapped.pyd".to_owned(),
        "select.pyd".to_owned(),
    ] {
        if !folder.join(&file).is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "Python runtime is incomplete: missing {file}. Extract the entire game ZIP again"
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_bom_and_preserves_python_settings() {
        let config = NetworkConfig::from_json(b"\xef\xbb\xbf{\"host_port\":29012}").unwrap();
        assert_eq!(config.settings["host_port"], 29012);
    }

    #[test]
    fn missing_bundle_never_falls_back_to_installed_python() {
        let config = NetworkConfig {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/no-bundled-python"),
            ..Default::default()
        };
        let error = config.python_executable().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("setup-runtime.ps1"));
    }

    #[test]
    #[cfg(windows)]
    fn incomplete_bundled_runtime_reports_the_missing_dll_before_starting_python() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/runtime-validation")
            .join(format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let folder = root.join("runtime/python");
        std::fs::create_dir_all(&folder).unwrap();
        for name in [
            "python.exe",
            "python314._pth",
            "python3.dll",
            "python314.zip",
            "vcruntime140.dll",
            "vcruntime140_1.dll",
            "_socket.pyd",
            "_overlapped.pyd",
            "select.pyd",
        ] {
            std::fs::write(folder.join(name), []).unwrap();
        }
        let config = NetworkConfig {
            root,
            ..Default::default()
        };
        let error = config.python_executable().unwrap_err();
        assert!(error.to_string().contains("python314.dll"));
        std::fs::write(folder.join("python314.dll"), []).unwrap();
        assert_eq!(
            config.python_executable().unwrap(),
            folder.join("python.exe")
        );
    }
}
