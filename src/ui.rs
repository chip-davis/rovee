use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, List, ListItem, Padding, Paragraph},
};

use tui_color_picker::ColorWheel;

use crate::app::{App, ColorPicker, Mode};
use crate::input::TextInput;

const RENAME_WIDTH: u16 = 40;
const RENAME_HEIGHT: u16 = 3;

const WHEEL_ROWS: u16 = 20;
const WHEEL_COLS: u16 = WHEEL_ROWS * 2;
const PICKER_WIDTH: u16 = WHEEL_COLS + 6;
const PICKER_HEIGHT: u16 = WHEEL_ROWS + 2 + 4;

pub fn render(app: &mut App, frame: &mut Frame) {
    render_list(app, frame, frame.area());

    if let Mode::Renaming(input) = &app.mode {
        render_rename_popup(input, frame);
    }

    let device_name = app
        .selected
        .selected()
        .and_then(|i| app.devices.get(i))
        .map(|d| d.saved.name.clone())
        .unwrap_or_default();
    if let Mode::ChangingColor(picker) = &mut app.mode {
        render_color_picker(picker, &device_name, frame);
    }
}

fn render_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .devices
        .iter()
        .map(|d| {
            let icon = if !d.online() {
                "?"
            } else if d.on {
                "●"
            } else {
                "○"
            };
            let line = Line::from(format!("{icon}  {}  {}", d.saved.name, d.saved.model));
            ListItem::new(if d.online() { line } else { line.dim() })
        })
        .collect();

    let hints = match app.mode {
        Mode::Normal => " rovee · ↑/↓ select · space toggle · c color · r rename · q quit ",
        Mode::Renaming(_) => " rovee ",
        Mode::ChangingColor(_) => " color ",
    };

    let list = List::new(items)
        .block(Block::bordered().title(hints))
        .highlight_style(Style::new().reversed());
    frame.render_stateful_widget(list, area, &mut app.selected);
}

fn render_rename_popup(input: &TextInput, frame: &mut Frame) {
    let area = popup_area(frame.area(), RENAME_WIDTH, RENAME_HEIGHT);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(input.value.as_str())
            .block(Block::bordered().title(" rename · enter save · esc cancel ")),
        area,
    );
    frame.set_cursor_position(Position::new(area.x + 1 + input.cursor as u16, area.y + 1));
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

fn render_color_picker(picker: &mut ColorPicker, device_name: &str, frame: &mut Frame) {
    let area = popup_area(frame.area(), PICKER_WIDTH, PICKER_HEIGHT);

    let accent = match picker.picked {
        Some((r, g, b)) => Style::new().fg(Color::Rgb(r, g, b)),
        None => Style::new(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(accent)
        .padding(Padding::horizontal(2))
        .title(Line::from(format!(" color · {device_name} ")).bold())
        .title_bottom(Line::from(" click to set · esc close ").dim().centered());

    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [wheel_row, _, swatch_row] = Layout::vertical([
        Constraint::Length(WHEEL_ROWS),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let [wheel_area] = Layout::horizontal([Constraint::Length(WHEEL_COLS)])
        .flex(Flex::Center)
        .areas(wheel_row);

    frame.render_stateful_widget(ColorWheel::new(), wheel_area, &mut picker.wheel);

    let swatch = match picker.picked {
        Some((r, g, b)) => Line::from(vec![
            Span::styled("██████", Style::new().fg(Color::Rgb(r, g, b))),
            Span::raw(format!("  #{r:02x}{g:02x}{b:02x}")),
        ]),
        None => Line::from("click the wheel to pick a color").dim(),
    };
    frame.render_widget(Paragraph::new(swatch.centered()), swatch_row);
}
