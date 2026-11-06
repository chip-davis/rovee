use std::path::PathBuf;
use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use govee::backend::GoveeBackend;
use govee::backend::local::LocalBackend;
use govee::scene::{SceneColor, SceneRegistry};
use ratatui::{
    DefaultTerminal,
    layout::{Position, Rect},
    style::Color as TuiColor,
    widgets::ListState,
};
use tui_color_picker::RectState;

use crate::devices::AppDevice;
use crate::input::TextInput;
use crate::store::{self, Rgb, SavedDevice, SavedGroup, SavedScene, to_hex};
use crate::ui;

/// Favorites map to keys 1-9.
pub const MAX_FAVORITES: usize = 9;

pub const MIN_KELVIN: u32 = 2000;
pub const MAX_KELVIN: u32 = 9000;
const KELVIN_STEP: u32 = 100;
const KELVIN_BIG_STEP: u32 = 500;
const DEFAULT_KELVIN: u32 = 4000;

const MIN_BRIGHTNESS: u8 = 1;
const MAX_BRIGHTNESS: u8 = 100;
const BRIGHTNESS_STEP: u8 = 10;

pub enum Mode {
    Normal,
    Renaming(TextInput),
    ChangingColor(ColorPicker),
    ChangingTemp(ColorTemp),
    Grouping(GroupMenu),
    NamingGroup { input: TextInput, device: usize },
    Scenes(SceneMenu),
    NamingScene(TextInput),
}

pub struct SceneMenu {
    pub list: ListState,
}

#[derive(Clone)]
pub struct SceneEntry {
    pub name: String,
    pub brightness: u8,
    pub color: SceneColor,
    pub builtin: bool,
}

impl SceneEntry {
    fn from_saved(s: &SavedScene) -> Option<Self> {
        let color = match (s.kelvin, s.rgb()) {
            (Some(k), _) => SceneColor::Temp(k),
            (None, Some((r, g, b))) => SceneColor::Rgb(govee::types::Color::new(r, g, b)),
            (None, None) => return None,
        };
        Some(Self {
            name: s.name.clone(),
            brightness: s.brightness,
            color,
            builtin: false,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Group(usize),
    Device(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    Top,
    Mid,
    Last,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Card,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Color,
    Power,
    Mode,
    Brightness,
    Scene,
}

impl Field {
    pub const ALL: [Field; 6] = [
        Field::Name,
        Field::Color,
        Field::Power,
        Field::Mode,
        Field::Brightness,
        Field::Scene,
    ];

    fn step(self, down: bool) -> Field {
        let i = Self::ALL.iter().position(|&f| f == self).unwrap_or(0);
        let next = if down {
            (i + 1).min(Self::ALL.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        Self::ALL[next]
    }
}

pub struct GroupMenu {
    pub list: ListState,
    pub device: usize,
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
    pub groups: Vec<SavedGroup>,
    pub user_scenes: Vec<SavedScene>,
    pub selected: ListState,
    pub mode: Mode,
    pub focus: Focus,
    pub field: Field,
    pub card_visible: bool,
    pub card_color_area: Rect,
    pub path: PathBuf,
    pub running: bool,
    pub scanning: bool,
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
            groups: saved.groups,
            user_scenes: saved.scenes,
            selected: ListState::default().with_selected(Some(0)),
            mode: Mode::Normal,
            focus: Focus::List,
            field: Field::Color,
            card_visible: false,
            card_color_area: Rect::default(),
            path,
            running: true,
            scanning: false,
            last_color: None,
            favorites: saved.favorites,
        };
        app.save().await?;
        Ok(app)
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| ui::render(self, frame))?;

            if self.scanning {
                self.rescan().await?;
                self.scanning = false;
                continue;
            }

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
            Mode::Normal => {
                let click = Position::new(mouse.column, mouse.row);
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && self.card_visible
                    && self.card_color_area.contains(click)
                {
                    self.focus = Focus::Card;
                    self.field = Field::Color;
                    self.start_color_picker();
                }
                Ok(())
            }

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

    async fn handle_list_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Down | KeyCode::Char('j') => self.selected.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.selected.select_previous(),
            KeyCode::Char(' ') => self.toggle_selected().await?,
            KeyCode::Tab => self.focus_card(),
            KeyCode::Enter => match self.selected_row() {
                Some(Row::Group(_)) => self.set_collapsed(None).await?,
                Some(Row::Device(_)) => self.focus_card(),
                None => {}
            },
            KeyCode::Left | KeyCode::Char('h') => self.set_collapsed(Some(true)).await?,
            KeyCode::Right | KeyCode::Char('l') => self.set_collapsed(Some(false)).await?,
            KeyCode::Char('g') => self.start_grouping(),
            KeyCode::Char('d') => self.delete_selected_group().await?,
            KeyCode::Char('r') => self.start_rename(),
            KeyCode::Char('s') | KeyCode::F(5) => self.scanning = true,
            KeyCode::Char('w') => self.start_color_picker(),
            KeyCode::Char('t') => self.start_temp_picker(),
            KeyCode::Char('c') => self.start_scene_menu(),
            KeyCode::Char('p') => self.apply_last_color().await?,
            KeyCode::Char('+') | KeyCode::Char('=') => self.step_brightness(true).await?,
            KeyCode::Char('-') | KeyCode::Char('_') => self.step_brightness(false).await?,
            KeyCode::Char(c @ '1'..='9') => self.apply_favorite(c).await?,
            _ => {}
        }
        Ok(())
    }

    async fn handle_card_key(&mut self, key: KeyEvent) -> Result<()> {
        let on_brightness = self.field == Field::Brightness;
        match key.code {
            KeyCode::Tab | KeyCode::Esc => self.focus = Focus::List,
            KeyCode::Up | KeyCode::Char('k') => self.field = self.field.step(false),
            KeyCode::Down | KeyCode::Char('j') => self.field = self.field.step(true),
            KeyCode::Enter => self.activate_field().await?,
            KeyCode::Left | KeyCode::Char('h') if on_brightness => {
                self.step_brightness(false).await?
            }
            KeyCode::Right | KeyCode::Char('l') if on_brightness => {
                self.step_brightness(true).await?
            }
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::List,
            KeyCode::Right | KeyCode::Char('l') => {}
            _ => self.handle_list_key(key).await?,
        }
        Ok(())
    }

    fn focus_card(&mut self) {
        if self.card_visible && self.selected_row().is_some() {
            self.focus = Focus::Card;
        }
    }

    async fn activate_field(&mut self) -> Result<()> {
        match self.field {
            Field::Name => self.start_rename(),
            Field::Color => self.start_color_picker(),
            Field::Mode => self.start_temp_picker(),
            Field::Power => self.toggle_selected().await?,
            Field::Brightness => {}
            Field::Scene => self.start_scene_menu(),
        }
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.mode {
            Mode::Normal => match self.focus {
                Focus::List => self.handle_list_key(key).await?,
                Focus::Card => self.handle_card_key(key).await?,
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

            Mode::Grouping(ref mut menu) => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Down | KeyCode::Char('j') => menu.list.select_next(),
                KeyCode::Up | KeyCode::Char('k') => menu.list.select_previous(),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let device = menu.device;
                    let choice = menu.list.selected().unwrap_or(0);
                    if choice < self.groups.len() {
                        self.toggle_membership(choice, device).await?;
                    } else {
                        self.mode = Mode::NamingGroup {
                            input: TextInput::new(String::new()),
                            device,
                        };
                    }
                }
                _ => {}
            },

            Mode::Scenes(ref mut menu) => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Down | KeyCode::Char('j') => menu.list.select_next(),
                KeyCode::Up | KeyCode::Char('k') => menu.list.select_previous(),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let choice = menu.list.selected().unwrap_or(0);
                    let scenes = self.scenes();
                    match scenes.get(choice) {
                        Some(scene) => self.apply_scene(scene.clone()).await?,
                        None => self.mode = Mode::NamingScene(TextInput::new(String::new())),
                    }
                }
                KeyCode::Char('d') => {
                    let choice = menu.list.selected().unwrap_or(0);
                    self.delete_scene(choice).await?;
                }
                _ => {}
            },

            Mode::NamingScene(ref mut input) => match key.code {
                KeyCode::Char(c) => input.insert(c),
                KeyCode::Backspace => input.backspace(),
                KeyCode::Left => input.left(),
                KeyCode::Right => input.right(),
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let name = input.value.clone();
                    self.mode = Mode::Normal;
                    self.save_scene(&name).await?;
                }
                _ => {}
            },

            Mode::NamingGroup {
                ref mut input,
                device,
            } => match key.code {
                KeyCode::Char(c) => input.insert(c),
                KeyCode::Backspace => input.backspace(),
                KeyCode::Left => input.left(),
                KeyCode::Right => input.right(),
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let name = input.value.trim().to_string();
                    self.mode = Mode::Normal;
                    self.create_group(name, device).await?;
                }
                _ => {}
            },
        }
        Ok(())
    }

    pub fn tree(&self) -> Vec<(Row, Branch)> {
        let mut rows = Vec::new();
        for g in 0..self.groups.len() {
            rows.push((Row::Group(g), Branch::Top));
            if !self.groups[g].collapsed {
                let members = self.group_members(g);
                let last = members.len().saturating_sub(1);
                rows.extend(members.into_iter().enumerate().map(|(n, i)| {
                    let branch = if n == last { Branch::Last } else { Branch::Mid };
                    (Row::Device(i), branch)
                }));
            }
        }
        rows.extend(
            (0..self.devices.len())
                .filter(|&i| {
                    !self
                        .groups
                        .iter()
                        .any(|g| g.members.contains(&self.devices[i].saved.id))
                })
                .map(|i| (Row::Device(i), Branch::Top)),
        );
        rows
    }

    pub fn rows(&self) -> Vec<Row> {
        self.tree().into_iter().map(|(row, _)| row).collect()
    }

    async fn set_collapsed(&mut self, collapsed: Option<bool>) -> Result<()> {
        let Some(Row::Group(g)) = self.selected_row() else {
            return Ok(());
        };
        let group = &mut self.groups[g];
        let next = collapsed.unwrap_or(!group.collapsed);
        if next == group.collapsed {
            return Ok(());
        }
        group.collapsed = next;
        self.select_row(Row::Group(g));
        self.save().await
    }

    pub fn selected_row(&self) -> Option<Row> {
        let i = self.selected.selected()?;
        self.rows().get(i).copied()
    }

    fn select_row(&mut self, row: Row) {
        if let Some(i) = self.rows().iter().position(|&r| r == row) {
            self.selected.select(Some(i));
        }
    }

    pub fn group_members(&self, group: usize) -> Vec<usize> {
        let Some(g) = self.groups.get(group) else {
            return Vec::new();
        };
        (0..self.devices.len())
            .filter(|&i| g.members.contains(&self.devices[i].saved.id))
            .collect()
    }

    pub fn color_targets(&self) -> Vec<usize> {
        match self.selected_row() {
            Some(Row::Group(g)) => self.group_members(g),
            Some(Row::Device(i)) => vec![i],
            None => Vec::new(),
        }
    }

    async fn toggle_selected(&mut self) -> Result<()> {
        let targets: Vec<usize> = self
            .color_targets()
            .into_iter()
            .filter(|&i| self.devices[i].online())
            .collect();
        if targets.is_empty() {
            return Ok(());
        }
        let new_state = !targets.iter().any(|&i| self.devices[i].on);

        for i in targets {
            if let Some(live) = &self.devices[i].live {
                self.backend.set_power(&live.id, new_state).await?;
                self.devices[i].on = new_state;
            }
        }
        Ok(())
    }

    fn start_grouping(&mut self) {
        let Some(Row::Device(device)) = self.selected_row() else {
            return;
        };
        self.mode = Mode::Grouping(GroupMenu {
            list: ListState::default().with_selected(Some(0)),
            device,
        });
    }

    async fn toggle_membership(&mut self, group: usize, device: usize) -> Result<()> {
        let id = self.devices[device].saved.id.clone();
        let members = &mut self.groups[group].members;
        match members.iter().position(|m| *m == id) {
            Some(pos) => {
                members.remove(pos);
            }
            None => members.push(id),
        }
        self.select_row(Row::Device(device));
        self.save().await
    }

    async fn create_group(&mut self, name: String, device: usize) -> Result<()> {
        if name.is_empty() {
            return Ok(());
        }
        let id = self.devices[device].saved.id.clone();
        match self.groups.iter_mut().find(|g| g.name == name) {
            Some(g) if !g.members.contains(&id) => g.members.push(id),
            Some(_) => {}
            None => self.groups.push(SavedGroup {
                name,
                members: vec![id],
                collapsed: false,
            }),
        }
        self.select_row(Row::Device(device));
        self.save().await
    }

    async fn delete_selected_group(&mut self) -> Result<()> {
        let Some(Row::Group(g)) = self.selected_row() else {
            return Ok(());
        };
        self.groups.remove(g);
        let last = self.rows().len().saturating_sub(1);
        if let Some(i) = self.selected.selected() {
            self.selected.select(Some(i.min(last)));
        }
        self.save().await
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
                self.devices[i].color = Some(rgb);
                self.devices[i].kelvin = None;
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

    async fn rescan(&mut self) -> Result<()> {
        self.backend.discover().await?;
        let found = self.backend.list_devices().await?;
        for d in &mut self.devices {
            d.live = None;
        }
        crate::devices::merge(&self.backend, &mut self.devices, found).await;
        self.save().await
    }

    async fn step_brightness(&mut self, up: bool) -> Result<()> {
        let targets = self.color_targets();
        let Some(&first) = targets.first() else {
            return Ok(());
        };
        let current = self.devices[first].brightness.unwrap_or(MAX_BRIGHTNESS);
        let next = if up {
            current.saturating_add(BRIGHTNESS_STEP)
        } else {
            current.saturating_sub(BRIGHTNESS_STEP)
        }
        .clamp(MIN_BRIGHTNESS, MAX_BRIGHTNESS);
        if next == current {
            return Ok(());
        }
        self.set_brightness(next).await
    }

    async fn set_brightness(&mut self, value: u8) -> Result<()> {
        for i in self.color_targets() {
            if let Some(live) = &self.devices[i].live {
                self.backend.set_brightness(&live.id, value).await?;
                self.devices[i].brightness = Some(value);
            }
        }
        Ok(())
    }

    pub fn scenes(&self) -> Vec<SceneEntry> {
        let mut scenes: Vec<SceneEntry> = SceneRegistry::new()
            .list()
            .into_iter()
            .map(|s| SceneEntry {
                name: s.name().to_string(),
                brightness: s.brightness(),
                color: *s.color(),
                builtin: true,
            })
            .collect();
        for entry in self.user_scenes.iter().filter_map(SceneEntry::from_saved) {
            match scenes
                .iter()
                .position(|s| s.name.eq_ignore_ascii_case(&entry.name))
            {
                Some(pos) => scenes[pos] = entry,
                None => scenes.push(entry),
            }
        }
        scenes
    }

    pub fn current_scene(&self) -> Option<String> {
        let &first = self.color_targets().first()?;
        let d = &self.devices[first];
        if !d.online() || !d.on {
            return None;
        }
        self.scenes()
            .into_iter()
            .find(|s| {
                d.brightness == Some(s.brightness.clamp(MIN_BRIGHTNESS, MAX_BRIGHTNESS))
                    && match s.color {
                        SceneColor::Temp(k) => d.kelvin == Some(k.clamp(MIN_KELVIN, MAX_KELVIN)),
                        SceneColor::Rgb(c) => {
                            d.kelvin.is_none() && d.color == Some((c.r, c.g, c.b))
                        }
                    }
            })
            .map(|s| s.name)
    }

    fn start_scene_menu(&mut self) {
        if !self.color_targets().is_empty() {
            self.mode = Mode::Scenes(SceneMenu {
                list: ListState::default().with_selected(Some(0)),
            });
        }
    }

    async fn apply_scene(&mut self, scene: SceneEntry) -> Result<()> {
        match scene.color {
            SceneColor::Rgb(c) => self.apply_color((c.r, c.g, c.b)).await?,
            SceneColor::Temp(k) => self.apply_temp(k.clamp(MIN_KELVIN, MAX_KELVIN)).await?,
        }
        self.set_brightness(scene.brightness.clamp(MIN_BRIGHTNESS, MAX_BRIGHTNESS))
            .await
    }

    async fn save_scene(&mut self, raw: &str) -> Result<()> {
        let name: String = raw
            .trim()
            .chars()
            .map(|c| if c.is_whitespace() { '-' } else { c })
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if name.is_empty() {
            return Ok(());
        }
        let Some(&first) = self.color_targets().first() else {
            return Ok(());
        };
        let d = &self.devices[first];
        let (color, kelvin) = match (d.kelvin, d.color) {
            (Some(k), _) => (None, Some(k)),
            (None, Some(c)) => (Some(to_hex(c)), None),
            (None, None) => return Ok(()),
        };
        let scene = SavedScene {
            name,
            brightness: d.brightness.unwrap_or(MAX_BRIGHTNESS),
            color,
            kelvin,
        };
        match self
            .user_scenes
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(&scene.name))
        {
            Some(pos) => self.user_scenes[pos] = scene,
            None => self.user_scenes.push(scene),
        }
        self.save().await
    }

    async fn delete_scene(&mut self, index: usize) -> Result<()> {
        let Some(scene) = self.scenes().into_iter().nth(index) else {
            return Ok(());
        };
        if scene.builtin {
            return Ok(());
        }
        self.user_scenes
            .retain(|s| !s.name.eq_ignore_ascii_case(&scene.name));
        self.save().await
    }

    async fn apply_temp(&mut self, kelvin: u32) -> Result<()> {
        for i in self.color_targets() {
            if let Some(live) = &self.devices[i].live {
                self.backend.set_color_temp(&live.id, kelvin).await?;
                self.devices[i].kelvin = Some(kelvin);
                self.devices[i].color = None;
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
        let current = match self.selected_row() {
            Some(Row::Group(g)) => self.groups[g].name.clone(),
            Some(Row::Device(i)) => self.devices[i].saved.name.clone(),
            None => return,
        };
        self.mode = Mode::Renaming(TextInput::new(current));
    }

    async fn finish_rename(&mut self, name: String) -> Result<()> {
        if name.is_empty() {
            return Ok(());
        }
        match self.selected_row() {
            Some(Row::Group(g)) => self.groups[g].name = name,
            Some(Row::Device(i)) => self.devices[i].saved.name = name,
            None => return Ok(()),
        }
        self.save().await
    }

    async fn save(&self) -> Result<()> {
        let saved: Vec<SavedDevice> = self.devices.iter().map(|d| d.saved.clone()).collect();
        store::save(
            &self.path,
            &saved,
            &self.groups,
            &self.user_scenes,
            &self.favorites,
        )
        .await
    }
}
