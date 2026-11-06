use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use govee::backend::GoveeBackend;
use govee::backend::local::LocalBackend;
use ratatui::{
    DefaultTerminal,
    layout::{Position, Rect},
    style::Color as TuiColor,
    widgets::ListState,
};
use tui_color_picker::RectState;

use crate::devices::AppDevice;
use crate::input::TextInput;
use crate::store::{self, Rgb, SavedDevice};
use crate::ui;

/// Favorites map to keys 1-9.
pub const MAX_FAVORITES: usize = 9;

pub const MIN_KELVIN: u32 = 2000;
pub const MAX_KELVIN: u32 = 9000;
const KELVIN_STEP: u32 = 100;
const KELVIN_BIG_STEP: u32 = 500;
const DEFAULT_KELVIN: u32 = 4000;

pub enum Mode {
    Normal,
    Renaming(TextInput),
    ChangingColor(ColorPicker),
    ChangingTemp(ColorTemp),
}

pub struct ColorTemp {
    pub current_kelvin: u32,
    pub bar: Rect,
}

impl ColorTemp {
    pub fn kelvin_at(&self, column: u16, row: u16) -> Option<u32> {
        if !self.bar.contains(Position::new(column, row)) {
            return None;
        }
        Some(kelvin_for_offset(column - self.bar.x, self.bar.width))
    }
}

pub fn kelvin_for_offset(offset: u16, width: u16) -> u32 {
    if width <= 1 {
        return MIN_KELVIN;
    }
    let span = MAX_KELVIN - MIN_KELVIN;
    let raw = MIN_KELVIN + offset as u32 * span / (width as u32 - 1);
    snap_kelvin(raw)
}

pub fn offset_for_kelvin(kelvin: u32, width: u16) -> u16 {
    if width <= 1 {
        return 0;
    }
    let k = kelvin.clamp(MIN_KELVIN, MAX_KELVIN) - MIN_KELVIN;
    let span = MAX_KELVIN - MIN_KELVIN;
    ((k * (width as u32 - 1) + span / 2) / span) as u16
}

fn snap_kelvin(kelvin: u32) -> u32 {
    let snapped = (kelvin + KELVIN_STEP / 2) / KELVIN_STEP * KELVIN_STEP;
    snapped.clamp(MIN_KELVIN, MAX_KELVIN)
}

pub struct ColorPicker {
    pub wheel: RectState,
}

pub struct App {
    pub backend: LocalBackend,
    pub devices: Vec<AppDevice>,
    pub selected: ListState,
    pub mode: Mode,
    pub path: PathBuf,
    pub running: bool,
    /// Last color sent from the wheel or a favorite; `p` applies it again.
    pub last_color: Option<Rgb>,
    pub favorites: Vec<Rgb>,
}

impl App {
    pub async fn new() -> Result<Self> {
        let path = store::config_path();
        let saved = store::load(&path).await?;

        let mut devices: Vec<AppDevice> = saved
            .devices
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
            last_color: None,
            favorites: saved.favorites,
        };
        app.save().await?;
        Ok(app)
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| ui::render(self, frame))?;

            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key).await?,
                Event::Mouse(mouse) => self.handle_mouse(mouse).await?,
                _ => {}
            }
        }
        Ok(())
    }

    async fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        match self.mode {
            Mode::ChangingColor(ref picker) => {
                if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
                    return Ok(());
                }

                let click = Position::new(mouse.column, mouse.row);
                if !picker.wheel.rect().contains(click) {
                    return Ok(());
                }

                let TuiColor::Rgb(r, g, b) = picker.wheel.color(mouse.column, mouse.row) else {
                    return Ok(());
                };
                self.apply_color((r, g, b)).await
            }

            Mode::ChangingTemp(ref mut temp) => {
                let pressed = matches!(
                    mouse.kind,
                    MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
                );
                if !pressed {
                    return Ok(());
                }
                let Some(kelvin) = temp.kelvin_at(mouse.column, mouse.row) else {
                    return Ok(());
                };
                if kelvin == temp.current_kelvin {
                    return Ok(());
                }
                temp.current_kelvin = kelvin;
                self.apply_temp(kelvin).await
            }

            _ => Ok(()),
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.running = false,
                KeyCode::Down | KeyCode::Char('j') => self.selected.select_next(),
                KeyCode::Up | KeyCode::Char('k') => self.selected.select_previous(),
                KeyCode::Char(' ') => self.toggle_selected().await?,
                KeyCode::Char('m') => self.toggle_mark(),
                KeyCode::Char('r') => self.start_rename(),
                KeyCode::Char('w') => self.start_color_picker(),
                KeyCode::Char('t') => self.start_temp_picker(),
                KeyCode::Char('p') => self.apply_last_color().await?,
                KeyCode::Char(c @ '1'..='9') => self.apply_favorite(c).await?,
                _ => {}
            },

            Mode::ChangingColor(_) => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Char('f') => self.save_favorite().await?,
                KeyCode::Char(c @ '1'..='9') => self.apply_favorite(c).await?,
                _ => {}
            },

            Mode::ChangingTemp(ref mut temp) => {
                let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    KELVIN_BIG_STEP
                } else {
                    KELVIN_STEP
                };
                let current = temp.current_kelvin;
                let next = match key.code {
                    KeyCode::Esc => {
                        self.mode = Mode::Normal;
                        return Ok(());
                    }
                    KeyCode::Left | KeyCode::Char('h') => current.saturating_sub(step),
                    KeyCode::Char('H') => current.saturating_sub(KELVIN_BIG_STEP),
                    KeyCode::Right | KeyCode::Char('l') => current + step,
                    KeyCode::Char('L') => current + KELVIN_BIG_STEP,
                    _ => return Ok(()),
                };
                let next = snap_kelvin(next);
                if next != current {
                    temp.current_kelvin = next;
                    self.apply_temp(next).await?;
                }
            }

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

    fn toggle_mark(&mut self) {
        if let Some(d) = self
            .selected
            .selected()
            .and_then(|i| self.devices.get_mut(i))
        {
            d.marked = !d.marked;
        }
    }

    /// Which lights a color action goes to: every marked light, or the
    /// selected one if nothing is marked.
    pub fn color_targets(&self) -> Vec<usize> {
        let marked: Vec<usize> = (0..self.devices.len())
            .filter(|&i| self.devices[i].marked)
            .collect();
        if !marked.is_empty() {
            return marked;
        }
        self.selected
            .selected()
            .filter(|&i| i < self.devices.len())
            .into_iter()
            .collect()
    }

    /// Send a color to the current targets and remember it as the last color.
    async fn apply_color(&mut self, rgb: Rgb) -> Result<()> {
        self.last_color = Some(rgb);
        let (r, g, b) = rgb;
        for i in self.color_targets() {
            if let Some(live) = &self.devices[i].live {
                self.backend
                    .set_color(&live.id, govee::types::Color::new(r, g, b))
                    .await?;
            }
            // offline: nothing to send
        }
        Ok(())
    }

    async fn apply_last_color(&mut self) -> Result<()> {
        match self.last_color {
            Some(rgb) => self.apply_color(rgb).await,
            None => Ok(()),
        }
    }

    async fn apply_favorite(&mut self, key: char) -> Result<()> {
        let Some(n) = key.to_digit(10) else {
            return Ok(());
        };
        match self.favorites.get(n as usize - 1) {
            Some(&rgb) => self.apply_color(rgb).await,
            None => Ok(()),
        }
    }

    /// Save the last color as a favorite (skipping duplicates and once full).
    async fn save_favorite(&mut self) -> Result<()> {
        let Some(rgb) = self.last_color else {
            return Ok(());
        };
        if self.favorites.contains(&rgb) || self.favorites.len() >= MAX_FAVORITES {
            return Ok(());
        }
        self.favorites.push(rgb);
        self.save().await
    }

    async fn apply_temp(&mut self, kelvin: u32) -> Result<()> {
        for i in self.color_targets() {
            if let Some(live) = &self.devices[i].live {
                self.backend.set_color_temp(&live.id, kelvin).await?;
                self.devices[i].kelvin = Some(kelvin);
            }
        }
        Ok(())
    }

    fn start_temp_picker(&mut self) {
        let targets = self.color_targets();
        let Some(&first) = targets.first() else {
            return;
        };
        let kelvin = self.devices[first]
            .kelvin
            .map(snap_kelvin)
            .unwrap_or(DEFAULT_KELVIN);
        self.mode = Mode::ChangingTemp(ColorTemp {
            current_kelvin: kelvin,
            bar: Rect::default(),
        });
    }

    fn start_color_picker(&mut self) {
        if !self.color_targets().is_empty() {
            self.mode = Mode::ChangingColor(ColorPicker {
                wheel: RectState::new_with_white(1.0),
            });
        }
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
        store::save(&self.path, &saved, &self.favorites).await
    }
}
