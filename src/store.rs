use std::path::{Path, PathBuf};

use color_eyre::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedDevice {
    pub id: String,
    pub model: String,
    pub name: String,
}

/// On-disk layout: a list of `[[device]]` tables.
#[derive(Serialize, Deserialize, Debug, Default)]
struct SavedFile {
    #[serde(default)]
    device: Vec<SavedDevice>,
}

/// `~/.config/rovee/devices.toml`
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .expect("no config directory")
        .join("rovee")
        .join("devices.toml")
}

/// Read saved devices; a missing file just means none saved yet.
pub async fn load(path: &Path) -> Result<Vec<SavedDevice>> {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => Ok(toml::from_str::<SavedFile>(&text)?.device),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

pub async fn save(path: &Path, devices: &[SavedDevice]) -> Result<()> {
    let file = SavedFile {
        device: devices.to_vec(),
    };
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(path, toml::to_string(&file)?).await?;
    Ok(())
}
