use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, KeyCode, KeyEvent};
use govee::backend::GoveeBackend;
use govee::backend::local::LocalBackend;
use ratatui::{DefaultTerminal, widgets::ListState};

use crate::devices::AppDevice;
use crate::input::TextInput;
use crate::store::{self, SavedDevice};
use crate::ui;

pub enum Mode {
    Normal,
    Renaming(TextInput),
}

pub struct App {
    pub backend: LocalBackend,
    pub devices: Vec<AppDevice>,
    pub selected: ListState,
    pub mode: Mode,
    pub path: PathBuf,
    pub running: bool,
}

impl App {
    pub async fn new() -> Result<Self> {
        let path = store::config_path();

        let mut devices: Vec<AppDevice> = store::load(&path)
            .await?
            .into_iter()
            .map(AppDevice::from_saved)
            .collect();

        let backend = LocalBackend::new(Duration::from_secs(2), 60).await?;
        let found = backend.list_devices().await?;
        crate::devices::merge(&backend, &mut devices, found).await;

        let app = Self {
            backend,
            devices,
            selected: ListState::default().with_selected(Some(0)),
            mode: Mode::Normal,
            path,
            running: true,
        };
        app.save().await?;
        Ok(app)
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| ui::render(self, frame))?;

            if let Some(key) = event::read()?.as_key_press_event() {
                self.handle_key(key).await?;
            }
        }
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.running = false,
                KeyCode::Down | KeyCode::Char('j') => self.selected.select_next(),
                KeyCode::Up | KeyCode::Char('k') => self.selected.select_previous(),
                KeyCode::Char(' ') => self.toggle_selected().await?,
                KeyCode::Char('r') => self.start_rename(),
                _ => {}
            },

            Mode::Renaming(ref mut input) => match key.code {
                KeyCode::Char(c) => input.insert(c),
                KeyCode::Backspace => input.backspace(),
                KeyCode::Left => input.left(),
                KeyCode::Right => input.right(),
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let name = input.value.trim().to_string();
                    self.mode = Mode::Normal;
                    self.finish_rename(name).await?;
                }
                _ => {}
            },
        }
        Ok(())
    }

    async fn toggle_selected(&mut self) -> Result<()> {
        let Some(d) = self
            .selected
            .selected()
            .and_then(|i| self.devices.get_mut(i))
        else {
            return Ok(());
        };
        let Some(live) = &d.live else {
            return Ok(());
        };

        let new_state = !d.on;
        self.backend.set_power(&live.id, new_state).await?;
        d.on = new_state;
        Ok(())
    }

    fn start_rename(&mut self) {
        if let Some(d) = self.selected_device() {
            self.mode = Mode::Renaming(TextInput::new(d.saved.name.clone()));
        }
    }

    async fn finish_rename(&mut self, name: String) -> Result<()> {
        if name.is_empty() {
            return Ok(());
        }
        if let Some(d) = self
            .selected
            .selected()
            .and_then(|i| self.devices.get_mut(i))
        {
            d.saved.name = name;
            self.save().await?;
        }
        Ok(())
    }

    fn selected_device(&self) -> Option<&AppDevice> {
        self.selected.selected().and_then(|i| self.devices.get(i))
    }

    async fn save(&self) -> Result<()> {
        let saved: Vec<SavedDevice> = self.devices.iter().map(|d| d.saved.clone()).collect();
        store::save(&self.path, &saved).await
    }
}
