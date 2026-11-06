use std::path::{Path, PathBuf};

use color_eyre::Result;
use serde::{Deserialize, Serialize};

pub type Rgb = (u8, u8, u8);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedDevice {
    pub id: String,
    pub model: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedGroup {
    pub name: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedScene {
    pub name: String,
    pub brightness: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kelvin: Option<u32>,
}

impl SavedScene {
    pub fn rgb(&self) -> Option<Rgb> {
        self.color.as_deref().and_then(parse_hex)
    }
}

#[derive(Debug, Default)]
pub struct SavedData {
    pub devices: Vec<SavedDevice>,
    pub groups: Vec<SavedGroup>,
    pub scenes: Vec<SavedScene>,
    pub favorites: Vec<Rgb>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct SavedFile {
    #[serde(default)]
    favorites: Vec<String>,
    #[serde(default)]
    device: Vec<SavedDevice>,
    #[serde(default)]
    group: Vec<SavedGroup>,
    #[serde(default)]
    scene: Vec<SavedScene>,
}

/// `~/.config/rovee/devices.toml`
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .expect("no config directory")
        .join("rovee")
        .join("devices.toml")
}

pub async fn load(path: &Path) -> Result<SavedData> {
    let file: SavedFile = match tokio::fs::read_to_string(path).await {
        Ok(text) => toml::from_str(&text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SavedFile::default(),
        Err(e) => return Err(e.into()),
    };
    Ok(SavedData {
        devices: file.device,
        groups: file.group,
        scenes: file.scene,
        favorites: file.favorites.iter().filter_map(|s| parse_hex(s)).collect(),
    })
}

pub async fn save(
    path: &Path,
    devices: &[SavedDevice],
    groups: &[SavedGroup],
    scenes: &[SavedScene],
    favorites: &[Rgb],
) -> Result<()> {
    let file = SavedFile {
        favorites: favorites.iter().map(|&c| to_hex(c)).collect(),
        device: devices.to_vec(),
        group: groups.to_vec(),
        scene: scenes.to_vec(),
    };
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(path, toml::to_string(&file)?).await?;
    Ok(())
}

pub fn to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    Some((channel(0)?, channel(2)?, channel(4)?))
}
