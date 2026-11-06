use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, Clear, List, ListItem, Paragraph},
};

use tui_color_picker::{ColorRect, ColorWheel, RectState};

use crate::app::{App, Mode};
use crate::input::TextInput;

const RENAME_WIDTH: u16 = 40;
const RENAME_HEIGHT: u16 = 3;

pub fn render(app: &mut App, frame: &mut Frame) {
    render_list(app, frame, frame.area());

    if let Mode::Renaming(input) = &app.mode {
        render_rename_popup(input, frame);
    }

    if let Mode::ChangingColor = &app.mode {
        render_color_picker_wheel(frame);
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
        Mode::ChangingColor => " color ",
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

fn render_color_picker_wheel(frame: &mut Frame) {
    let mut rec_no_grad = RectState::new(0.9, 0.8);
    frame.render_stateful_widget(
        ColorWheel::new(),
        Rect::new(35, 5, 12, 12),
        &mut rec_no_grad,
    );
}
