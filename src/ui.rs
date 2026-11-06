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
use crate::store::{Rgb, to_hex};

const RENAME_WIDTH: u16 = 40;
const RENAME_HEIGHT: u16 = 3;

const WHEEL_ROWS: u16 = 20;
const WHEEL_COLS: u16 = WHEEL_ROWS * 2;
const PICKER_WIDTH: u16 = WHEEL_COLS + 6;
const PICKER_HEIGHT: u16 = WHEEL_ROWS + 4 + 2;

pub fn render(app: &mut App, frame: &mut Frame) {
    render_list(app, frame, frame.area());

    if let Mode::Renaming(input) = &app.mode {
        render_rename_popup(input, frame);
    }

    let target = target_label(app);
    if let Mode::ChangingColor(picker) = &mut app.mode {
        render_color_picker(picker, &target, app.last_color, &app.favorites, frame);
    }
}

fn target_label(app: &App) -> String {
    let targets = app.color_targets();
    let marked = app.devices.iter().filter(|d| d.marked).count();
    match (marked, targets.first()) {
        (0, Some(&i)) => app.devices[i].saved.name.clone(),
        (0, None) => String::new(),
        (n, _) => format!("{n} marked"),
    }
}

fn render_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .devices
        .iter()
        .map(|d| {
            let mark = if d.marked { "✓" } else { " " };
            let icon = if !d.online() {
                "?"
            } else if d.on {
                "●"
            } else {
                "○"
            };
            let line = Line::from(format!(
                "{mark} {icon}  {}  {}",
                d.saved.name, d.saved.model
            ));
            ListItem::new(if d.online() { line } else { line.dim() })
        })
        .collect();

    let hints = match app.mode {
        Mode::Normal => {
            " rovee · ↑/↓ select · space power · m mark · w wheel · p last color · 1-9 favorite · r rename · q quit "
        }
        Mode::Renaming(_) => " rovee ",
        Mode::ChangingColor(_) => " color ",
    };

    let list = List::new(items)
        .block(
            Block::bordered()
                .title(hints)
                .title_bottom(palette_line(app.last_color, &app.favorites)),
        )
        .highlight_style(Style::new().reversed());
    frame.render_stateful_widget(list, area, &mut app.selected);
}

fn palette_line(last: Option<Rgb>, favorites: &[Rgb]) -> Line<'static> {
    let mut spans = vec![Span::raw(" last ").dim()];
    match last {
        Some(c) => spans.push(swatch(c, 2)),
        None => spans.push(Span::raw("--").dim()),
    }
    spans.push(Span::raw(" ·").dim());
    if favorites.is_empty() {
        spans.push(Span::raw(" no favorites yet (f in wheel) ").dim());
    } else {
        spans.extend(favorite_spans(favorites));
        spans.push(Span::raw(" "));
    }
    Line::from(spans)
}

fn favorite_spans(favorites: &[Rgb]) -> Vec<Span<'static>> {
    favorites
        .iter()
        .enumerate()
        .flat_map(|(i, &c)| [Span::raw(format!(" {} ", i + 1)).dim(), swatch(c, 2)])
        .collect()
}

fn swatch((r, g, b): Rgb, width: usize) -> Span<'static> {
    Span::styled("█".repeat(width), Style::new().fg(Color::Rgb(r, g, b)))
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

fn render_color_picker(
    picker: &mut ColorPicker,
    target: &str,
    last: Option<Rgb>,
    favorites: &[Rgb],
    frame: &mut Frame,
) {
    let area = popup_area(frame.area(), PICKER_WIDTH, PICKER_HEIGHT);

    let accent = match last {
        Some((r, g, b)) => Style::new().fg(Color::Rgb(r, g, b)),
        None => Style::new(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(accent)
        .padding(Padding::horizontal(2))
        .title(Line::from(format!(" color · {target} ")).bold())
        .title_bottom(
            Line::from(" click set · f save favorite · 1-9 favorite · esc close ")
                .dim()
                .centered(),
        );

    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [wheel_row, _, swatch_row, _, favorites_row] = Layout::vertical([
        Constraint::Length(WHEEL_ROWS),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    let [wheel_area] = Layout::horizontal([Constraint::Length(WHEEL_COLS)])
        .flex(Flex::Center)
        .areas(wheel_row);

    frame.render_stateful_widget(ColorWheel::new(), wheel_area, &mut picker.wheel);

    let current = match last {
        Some(c) => Line::from(vec![swatch(c, 6), Span::raw(format!("  {}", to_hex(c)))]),
        None => Line::from("click the wheel to pick a color").dim(),
    };
    frame.render_widget(Paragraph::new(current.centered()), swatch_row);

    let favs = if favorites.is_empty() {
        Line::from("no favorites yet · f saves the current color").dim()
    } else {
        Line::from(favorite_spans(favorites))
    };
    frame.render_widget(Paragraph::new(favs.centered()), favorites_row);
}
