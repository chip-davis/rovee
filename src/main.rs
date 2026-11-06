use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use govee::backend::GoveeBackend;
use govee::backend::local::LocalBackend;
use govee::types::Device;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::{
    DefaultTerminal,
    style::{Style, Stylize},
    widgets::{Block, List, ListState},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::LinkedList,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::fs;

use crate::Mode::Normal;

const RENAME_WIDTH: u16 = 40;
const RENAME_HEIGHT: u16 = 3;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SavedDevice {
    id: String,
    model: String,
    name: String,
}
#[derive(Serialize, Deserialize, Debug, Default)]
struct SavedFile {
    device: LinkedList<SavedDevice>,
}

struct AppDevice {
    saved: SavedDevice,
    live: Option<govee::types::Device>,
    on: bool,
}

impl AppDevice {
    fn online(&self) -> bool {
        self.live.is_some()
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .expect("no config dir")
        .join("rovee")
        .join("devices.toml")
}

async fn load(path: &Path) -> color_eyre::Result<SavedFile> {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SavedFile::default()),
        Err(e) => Err(e.into()),
    }
}

async fn save(path: &Path, devices: &[AppDevice]) -> color_eyre::Result<()> {
    let file = SavedFile {
        device: devices.iter().map(|d| d.saved.clone()).collect(),
    };
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(path, toml::to_string(&file)?).await?;
    Ok(())
}

async fn merge(
    backend: &LocalBackend,
    devices: &mut Vec<AppDevice>,
    found: Vec<govee::types::Device>,
) {
    for f in found {
        let id = f.id.to_string();
        match devices.iter_mut().find(|d| d.saved.id == id) {
            Some(d) => d.live = Some(f),
            None => devices.push(AppDevice {
                saved: SavedDevice {
                    id,
                    model: f.model.clone(),
                    name: f.model.clone(),
                },
                live: Some(f),
                on: false,
            }),
        }
    }

    for d in devices.iter_mut() {
        if let Some(live) = &d.live {
            d.on = backend
                .get_state(&live.id)
                .await
                .map(|s| s.on)
                .unwrap_or(false);
        }
    }
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let path = config_path();
    let mut devices: Vec<AppDevice> = load(&path)
        .await?
        .device
        .into_iter()
        .map(|saved| AppDevice {
            saved,
            live: None,
            on: false,
        })
        .collect();

    let backend = LocalBackend::new(Duration::from_secs(2), 60).await?;
    let found = backend.list_devices().await?;
    merge(&backend, &mut devices, found).await;
    save(&path, &devices).await?;

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &backend, &mut devices).await;
    ratatui::restore();
    result
}

enum Mode {
    Normal,
    Renaming(TextInput),
}

struct TextInput {
    value: String,
    cursor: usize,
}

impl TextInput {
    fn new(value: String) -> Self {
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    fn byte_index(&self) -> usize {
        self.value
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.cursor)
            .unwrap_or(self.value.len())
    }

    fn insert(&mut self, c: char) {
        let i = self.byte_index();
        self.value.insert(i, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = self.value.chars().take(self.cursor - 1);
        let after = self.value.chars().skip(self.cursor);
        self.value = before.chain(after).collect();
        self.cursor -= 1;
    }

    fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.value.chars().count());
    }
}

fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    area
}

async fn run(
    terminal: &mut DefaultTerminal,
    backend: &LocalBackend,
    devices: &mut [AppDevice],
) -> color_eyre::Result<()> {
    let mut selected = ListState::default().with_selected(Some(0));
    let mut mode = Mode::Normal;

    loop {
        terminal.draw(|frame| {
            let items = devices.iter().map(|d| {
                let icon = if !d.online() {
                    "?"
                } else if d.on {
                    "●"
                } else {
                    "○"
                };
                format!("{icon}  {}  {}", d.saved.name, d.saved.model)
            });
            let list = List::new(items)
                .block(Block::bordered().title(" rovee · ↑/↓ select · space toggle · q quit "))
                .highlight_style(Style::new().reversed());
            frame.render_stateful_widget(list, frame.area(), &mut selected);

            if let Mode::Renaming(input) = &mode {
                let area = popup_area(frame.area(), RENAME_WIDTH, RENAME_HEIGHT);
                frame.render_widget(Clear, area);
                frame.render_widget(
                    Paragraph::new(input.value.as_str())
                        .block(Block::bordered().title(" rename - enter save - esc cancel ")),
                    area,
                );
                frame.set_cursor_position(Position::new(
                    area.x + 1 + input.cursor as u16,
                    area.y + 1,
                ));
            }
        })?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match (&mut mode, key.code) {
            (Mode::Normal, KeyCode::Char('q') | KeyCode::Esc) => return Ok(()),
            (Mode::Normal, KeyCode::Down | KeyCode::Char('j')) => selected.select_next(),
            (Mode::Normal, KeyCode::Up | KeyCode::Char('k')) => selected.select_previous(),
            (Mode::Normal, KeyCode::Char(' ')) => {
                if let Some(d) = selected.selected().and_then(|i| devices.get_mut(i)) {
                    if let Some(live) = &d.live {
                        let new_state = !d.on;
                        backend.set_power(&live.id, new_state).await?;
                        d.on = new_state;
                    }
                    // offline: nothing to send
                }
            }
            (Mode::Normal, KeyCode::Char('r')) => {
                if let Some(d) = selected.selected().and_then(|i| devices.get(i)) {
                    mode = Mode::Renaming(TextInput::new(d.saved.name.clone()));
                }
            }

            // ----- Renaming MOde ------
            (Mode::Renaming(input), KeyCode::Char(c)) => input.insert(c),
            (Mode::Renaming(input), KeyCode::Backspace) => input.backspace(),
            (Mode::Renaming(input), KeyCode::Left) => input.left(),
            (Mode::Renaming(input), KeyCode::Right) => input.right(),
            (Mode::Renaming(_), KeyCode::Esc) => mode = Mode::Normal,
            (Mode::Renaming(input), KeyCode::Enter) => {
                let name = input.value.trim().to_string();
                mode = Mode::Normal;
                if !name.is_empty() {
                    if let Some(d) = selected.selected().and_then(|i| devices.get_mut(i)) {
                        d.saved.name = name;
                        save(&config_path(), devices).await?;
                    }
                }
            }

            _ => {}
        }
    }
}
