use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, List, ListItem, Padding, Paragraph},
};

use tui_color_picker::ColorWheel;

use crate::app::{App, ColorPicker, ColorTemp, Mode, kelvin_for_offset, offset_for_kelvin};
use crate::devices::AppDevice;
use crate::input::TextInput;
use crate::store::{Rgb, to_hex};

const ACCENT: Rgb = (0x6e, 0x94, 0xb2);
const PURPLE: Rgb = (0xbb, 0x9d, 0xbd);
const WARM: Rgb = (0xe8, 0xb5, 0x89);
const FG: Color = Color::Rgb(0xcd, 0xcd, 0xcd);
const MUTED: Color = Color::Rgb(0x87, 0x87, 0x87);
const DIM: Color = Color::Rgb(0x60, 0x60, 0x79);
const SURFACE: Color = Color::Rgb(0x14, 0x14, 0x15);
const LINE: Color = Color::Rgb(0x25, 0x25, 0x30);
const GREEN: Color = Color::Rgb(0x7f, 0xa5, 0x63);
const RED: Color = Color::Rgb(0xd8, 0x64, 0x7e);

const LOGO: [&str; 6] = [
    "██████╗  ██████╗ ██╗   ██╗███████╗███████╗",
    "██╔══██╗██╔═══██╗██║   ██║██╔════╝██╔════╝",
    "██████╔╝██║   ██║██║   ██║█████╗  █████╗  ",
    "██╔══██╗██║   ██║╚██╗ ██╔╝██╔══╝  ██╔══╝  ",
    "██║  ██║╚██████╔╝ ╚████╔╝ ███████╗███████╗",
    "╚═╝  ╚═╝ ╚═════╝   ╚═══╝  ╚══════╝╚══════╝",
];
const LOGO_WIDTH: u16 = 42;
const FULL_HEADER_MIN_HEIGHT: u16 = 30;
const DETAILS_MIN_WIDTH: u16 = 90;

const RENAME_WIDTH: u16 = 44;
const RENAME_HEIGHT: u16 = 3;

const WHEEL_ROWS: u16 = 20;
const WHEEL_COLS: u16 = WHEEL_ROWS * 2;
const PICKER_WIDTH: u16 = WHEEL_COLS + 6;
const PICKER_HEIGHT: u16 = WHEEL_ROWS + 4 + 2;

const TEMP_WIDTH: u16 = 64;
const TEMP_HEIGHT: u16 = 7;

pub fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let header_height = if area.height >= FULL_HEADER_MIN_HEIGHT && area.width >= LOGO_WIDTH + 4 {
        LOGO.len() as u16 + 2
    } else {
        1
    };

    let [header, body, palette, footer] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(6),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(app, frame, header);

    if body.width >= DETAILS_MIN_WIDTH {
        let [list_area, details_area] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(body);
        render_list(app, frame, list_area);
        render_details(app, frame, details_area);
    } else {
        render_list(app, frame, body);
    }

    render_palette(app.last_color, &app.favorites, frame, palette);
    render_footer(&app.mode, frame, footer);

    if let Mode::Renaming(input) = &app.mode {
        render_rename_popup(input, frame);
    }

    let target = target_label(app);
    if let Mode::ChangingColor(picker) = &mut app.mode {
        render_color_picker(picker, &target, app.last_color, &app.favorites, frame);
    }
    if let Mode::ChangingTemp(temp) = &mut app.mode {
        render_temp_picker(temp, &target, frame);
    }
}

fn render_header(app: &App, frame: &mut Frame, area: Rect) {
    let online = app.devices.iter().filter(|d| d.online()).count();
    let status = Line::from(vec![
        Span::raw("govee lan control").fg(MUTED),
        Span::raw("  ·  ").fg(DIM),
        Span::raw(format!("{online} of {} lights online", app.devices.len())).fg(MUTED),
    ]);

    if area.height == 1 {
        let mut spans = vec![Span::raw(" ◆ rovee  ").fg(rgb(ACCENT)).bold()];
        spans.extend(status.spans);
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    let mut lines: Vec<Line> = vec![Line::default()];
    lines.extend(LOGO.iter().map(|row| logo_line(row)));
    lines.push(status.centered());
    frame.render_widget(Paragraph::new(lines).centered(), area);
}

fn logo_line(row: &str) -> Line<'static> {
    let width = LOGO_WIDTH.max(2) as f32 - 1.0;
    row.chars()
        .enumerate()
        .map(|(i, c)| {
            let color = if c == '█' {
                rgb(gradient(i as f32 / width))
            } else {
                DIM
            };
            Span::styled(c.to_string(), Style::new().fg(color))
        })
        .collect::<Line>()
        .centered()
}

fn gradient(t: f32) -> Rgb {
    if t < 0.5 {
        lerp(ACCENT, PURPLE, t * 2.0)
    } else {
        lerp(PURPLE, WARM, (t - 0.5) * 2.0)
    }
}

fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)) as u8;
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

fn rgb((r, g, b): Rgb) -> Color {
    Color::Rgb(r, g, b)
}

fn panel(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DIM))
        .title(Line::from(format!(" {title} ")).fg(rgb(ACCENT)).bold())
}

fn device_rgb(d: &AppDevice) -> Option<Rgb> {
    if !d.online() || !d.on {
        return None;
    }
    match d.kelvin {
        Some(k) => Some(kelvin_to_rgb(k)),
        None => d.color,
    }
}

fn device_status(d: &AppDevice) -> Span<'static> {
    if !d.online() {
        return Span::raw("offline").fg(RED);
    }
    if !d.on {
        return Span::raw("off").fg(MUTED);
    }
    match (d.kelvin, d.color) {
        (Some(k), _) => Span::raw(format!("white {k}K")).fg(FG),
        (None, Some(c)) => Span::raw(format!("color {}", to_hex(c))).fg(FG),
        (None, None) => Span::raw("on").fg(GREEN),
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
            let mark = if d.marked {
                Span::raw("✓ ").fg(rgb(ACCENT)).bold()
            } else {
                Span::raw("  ")
            };
            let dot = match (d.online(), d.on) {
                (false, _) => Span::raw("?").fg(DIM),
                (true, false) => Span::raw("○").fg(MUTED),
                (true, true) => Span::raw("●").fg(device_rgb(d).map(rgb).unwrap_or(GREEN)),
            };
            let name_style = if d.online() {
                Style::new().fg(FG).bold()
            } else {
                Style::new().fg(DIM)
            };
            let title = Line::from(vec![
                mark,
                dot,
                Span::raw("  "),
                Span::styled(d.saved.name.clone(), name_style),
            ]);
            let detail = Line::from(vec![
                Span::raw("     "),
                Span::raw(d.saved.model.clone()).fg(DIM),
                Span::raw("  ·  ").fg(DIM),
                device_status(d),
            ]);
            ListItem::new(vec![title, detail, Line::default()])
        })
        .collect();

    let count = app.devices.len();
    let list = List::new(items)
        .block(
            panel("lights")
                .title(Line::from(format!(" {count} ")).fg(DIM).right_aligned())
                .padding(Padding::new(1, 1, 1, 0)),
        )
        .highlight_style(Style::new().bg(LINE))
        .highlight_symbol(Line::from("▌").fg(rgb(ACCENT)));
    frame.render_stateful_widget(list, area, &mut app.selected);
}

fn render_details(app: &App, frame: &mut Frame, area: Rect) {
    let Some(d) = app.selected.selected().and_then(|i| app.devices.get(i)) else {
        frame.render_widget(panel("details"), area);
        return;
    };

    let color = device_rgb(d);
    let block = panel(&d.saved.name)
        .border_style(Style::new().fg(color.map(rgb).unwrap_or(DIM)))
        .padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [swatch_area, _, info_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);

    let fill: Vec<Line> = (0..swatch_area.height)
        .map(|_| match color {
            Some(c) => swatch(c, swatch_area.width as usize).into(),
            None => Line::from("░".repeat(swatch_area.width as usize)).fg(LINE),
        })
        .collect();
    frame.render_widget(Paragraph::new(fill), swatch_area);

    let row = |label: &str, value: Span<'static>| {
        Line::from(vec![Span::raw(format!("{label:<8}")).fg(DIM), value])
    };
    let power = match (d.online(), d.on) {
        (false, _) => Span::raw("unreachable").fg(RED),
        (true, true) => Span::raw("on").fg(GREEN),
        (true, false) => Span::raw("off").fg(MUTED),
    };
    let mut info = vec![
        row("power", power),
        row("mode", device_status(d)),
        row("model", Span::raw(d.saved.model.clone()).fg(FG)),
        row("id", Span::raw(d.saved.id.clone()).fg(MUTED)),
    ];
    if d.marked {
        info.push(row("marked", Span::raw("yes").fg(rgb(ACCENT))));
    }
    frame.render_widget(Paragraph::new(info), info_area);
}

fn render_palette(last: Option<Rgb>, favorites: &[Rgb], frame: &mut Frame, area: Rect) {
    let mut spans = vec![Span::raw("last ").fg(DIM)];
    match last {
        Some(c) => spans.push(swatch(c, 4)),
        None => spans.push(Span::raw("····").fg(LINE)),
    }
    spans.push(Span::raw("   │  ").fg(LINE));
    if favorites.is_empty() {
        spans.push(Span::raw("no favorites yet · press f in the wheel to save one").fg(DIM));
    } else {
        spans.extend(favorite_spans(favorites, 4));
    }
    let block = panel("palette").padding(Padding::horizontal(1));
    frame.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn favorite_spans(favorites: &[Rgb], width: usize) -> Vec<Span<'static>> {
    favorites
        .iter()
        .enumerate()
        .flat_map(|(i, &c)| [Span::raw(format!("{} ", i + 1)).fg(MUTED), swatch(c, width), Span::raw("  ")])
        .collect()
}

fn swatch((r, g, b): Rgb, width: usize) -> Span<'static> {
    Span::styled("█".repeat(width), Style::new().fg(Color::Rgb(r, g, b)))
}

fn render_footer(mode: &Mode, frame: &mut Frame, area: Rect) {
    let keys: &[(&str, &str)] = match mode {
        Mode::Normal => &[
            ("↑↓", "select"),
            ("space", "power"),
            ("m", "mark"),
            ("w", "wheel"),
            ("t", "white"),
            ("p", "last"),
            ("1-9", "favorite"),
            ("r", "rename"),
            ("q", "quit"),
        ],
        Mode::Renaming(_) => &[("enter", "save"), ("esc", "cancel")],
        Mode::ChangingColor(_) => &[
            ("click", "set color"),
            ("f", "save favorite"),
            ("1-9", "favorite"),
            ("esc", "close"),
        ],
        Mode::ChangingTemp(_) => &[
            ("←→", "100K"),
            ("shift", "500K"),
            ("drag", "set"),
            ("esc", "close"),
        ],
    };
    let spans: Vec<Span> = keys
        .iter()
        .flat_map(|(key, label)| {
            [
                Span::styled(format!(" {key} "), Style::new().fg(SURFACE).bg(rgb(ACCENT)).bold()),
                Span::raw(format!(" {label}  ")).fg(MUTED),
            ]
        })
        .collect();
    frame.render_widget(Paragraph::new(Line::from(spans)).centered(), area);
}

fn popup(title: &str, border: Color) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border))
        .title(Line::from(format!(" {title} ")).fg(FG).bold())
}

fn render_rename_popup(input: &TextInput, frame: &mut Frame) {
    let area = popup_area(frame.area(), RENAME_WIDTH, RENAME_HEIGHT);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(input.value.as_str())
            .fg(FG)
            .block(popup("rename", rgb(ACCENT)).padding(Padding::horizontal(1))),
        area,
    );
    frame.set_cursor_position(Position::new(area.x + 2 + input.cursor as u16, area.y + 1));
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
    let border = last.map(rgb).unwrap_or(rgb(ACCENT));
    let block = popup(&format!("color · {target}"), border).padding(Padding::horizontal(2));

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
        Some(c) => Line::from(vec![swatch(c, 6), Span::raw(format!("  {}", to_hex(c))).fg(FG)]),
        None => Line::from("click the wheel to pick a color").fg(DIM),
    };
    frame.render_widget(Paragraph::new(current.centered()), swatch_row);

    let favs = if favorites.is_empty() {
        Line::from("no favorites yet").fg(DIM)
    } else {
        Line::from(favorite_spans(favorites, 2))
    };
    frame.render_widget(Paragraph::new(favs.centered()), favorites_row);
}

fn render_temp_picker(temp: &mut ColorTemp, target: &str, frame: &mut Frame) {
    let area = popup_area(frame.area(), TEMP_WIDTH, TEMP_HEIGHT);
    let current = kelvin_to_rgb(temp.current_kelvin);
    let block = popup(&format!("white · {target}"), rgb(current)).padding(Padding::horizontal(2));

    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [bar_area, marker_row, label_row] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    temp.bar = bar_area;

    let bar: Line = (0..bar_area.width)
        .map(|x| swatch(kelvin_to_rgb(kelvin_for_offset(x, bar_area.width)), 1))
        .collect();
    frame.render_widget(Paragraph::new(vec![bar.clone(), bar]), bar_area);

    let offset = offset_for_kelvin(temp.current_kelvin, bar_area.width);
    let marker = Line::from(vec![
        Span::raw(" ".repeat(offset as usize)),
        Span::raw("▲").fg(rgb(current)),
    ]);
    frame.render_widget(Paragraph::new(marker), marker_row);

    let label = Line::from(vec![
        Span::raw("warm  ").fg(rgb(WARM)),
        Span::raw(format!("{}K", temp.current_kelvin)).fg(FG).bold(),
        Span::raw("  cool").fg(rgb(ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(label.centered()), label_row);
}

fn kelvin_to_rgb(kelvin: u32) -> Rgb {
    let t = kelvin as f64 / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698727446 * (t - 60.0).powf(-0.1332047592)
    };
    let g = if t <= 66.0 {
        99.4708025861 * t.ln() - 161.1195681661
    } else {
        288.1221695283 * (t - 60.0).powf(-0.0755148492)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    let c = |v: f64| v.clamp(0.0, 255.0) as u8;
    (c(r), c(g), c(b))
}
