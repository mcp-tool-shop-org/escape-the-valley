//! Drawing: [`view`] turns a [`Model`] into a frame and does nothing else.
//!
//! Every sentence drawn here comes from the engine, the content or a constant that line mode
//! uses too; the rest are short labels. A selected or unaffordable item is never marked by
//! colour alone: a `>` marker or the reversed video of the terminal says it too, so the screens
//! read the same with colours off.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::play::StatusFacts;
use crate::screens::{about_screen_lines, settings_screen_lines, title_screen_lines, MODEL_PROMPT};

use super::layout::{self, wrap};
use super::model::{Model, Overlay, Screen, BUSY_LINE, MIN_HEIGHT, MIN_WIDTH, PACES};

/// Asked of the player when the window is too small to draw in.
pub const TOO_SMALL_LINE: &str = "Please make the window larger (80x24 or more).";

/// The key that picks each pace in the pace picker, in the order of [`PACES`].
const PACE_KEYS: [char; 3] = ['S', 'T', 'H'];

/// The key list of the help overlay.
pub const HELP_LINES: [&str; 6] = [
    "T Travel   R Rest   H Hunt   F Repair",
    "P Pace, then S slow, T steady or H hard",
    "X Abandon cargo   D Desperate repair   N Hard ration",
    "A-G or 1-7   Choose at an event or a fork",
    "Up Down PgUp PgDn Home End   Scroll",
    "O Settings   ? Help   Q Quit   Esc Close",
];

struct Theme {
    color: bool,
}

impl Theme {
    fn fg(&self, c: Color) -> Style {
        if self.color {
            Style::new().fg(c)
        } else {
            Style::new()
        }
    }
    fn border(&self) -> Style {
        self.fg(Color::Cyan)
    }
    fn heading(&self) -> Style {
        self.fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }
    fn key(&self) -> Style {
        self.fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }
    fn dead(&self) -> Style {
        self.fg(Color::Red).add_modifier(Modifier::DIM)
    }
    fn bar(&self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    }
    fn selected(&self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED)
    }
}

/// Draw the whole screen for `model`.
pub fn view(model: &Model, frame: &mut Frame) {
    let area = frame.area();
    if layout::too_small(area.width, area.height) {
        let _ = (MIN_WIDTH, MIN_HEIGHT);
        let row = Rect::new(
            area.x,
            area.y + area.height / 2,
            area.width,
            1.min(area.height),
        );
        frame.render_widget(
            Paragraph::new(TOO_SMALL_LINE).alignment(Alignment::Center),
            row,
        );
        return;
    }
    let th = Theme { color: model.color };
    match model.screen {
        Screen::Title => draw_title(model, &th, frame, area),
        Screen::About => draw_about(model, &th, frame, area),
        Screen::Settings => draw_settings(model, &th, frame, area),
        Screen::GameOver if model.resolved.is_none() => draw_game_over(model, &th, frame, area),
        Screen::Camp if model.resolved.is_none() => draw_camp(model, &th, frame, area),
        Screen::Camp | Screen::Event | Screen::Fork | Screen::GameOver => {
            draw_event(model, &th, frame, area)
        }
    }
    match model.overlay {
        Overlay::None => {}
        Overlay::Help => draw_help(&th, frame, area),
        Overlay::QuitConfirm => draw_quit(&th, frame, area),
        Overlay::Pace => draw_pace(model, &th, frame, area),
    }
}

fn screen_name(model: &Model) -> &'static str {
    if let Some(r) = &model.resolved {
        return if r.title == crate::play::FORK_LINE {
            "Fork"
        } else {
            "Event"
        };
    }
    match model.screen {
        Screen::Title => "",
        Screen::Camp => "Camp",
        Screen::Event => "Event",
        Screen::Fork => "Fork",
        Screen::GameOver => "Game over",
        Screen::Settings => "Settings",
        Screen::About => "About",
    }
}

fn draw_header(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let right = if model.busy {
        BUSY_LINE.to_string()
    } else if let Some(n) = &model.notice {
        n.clone()
    } else {
        screen_name(model).to_string()
    };
    let left = "ESCAPE THE VALLEY";
    let width = usize::from(area.width);
    let used = left.chars().count() + right.chars().count();
    let gap = width.saturating_sub(used + 2).max(1);
    let text = format!(" {left}{}{right} ", " ".repeat(gap));
    let text: String = text.chars().take(width).collect();
    frame.render_widget(Paragraph::new(text).style(th.bar()), area);
}

/// Lay key hints out in rows no wider than `width`, a bold key then its label.
fn key_rows(th: &Theme, items: &[(String, &str)], width: usize) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for (key, label) in items {
        let item_w = key.chars().count() + 1 + label.chars().count();
        let sep = if spans.is_empty() { 0 } else { 2 };
        if !spans.is_empty() && used + sep + item_w > width {
            rows.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
            used += 2;
        }
        spans.push(Span::styled(key.clone(), th.key()));
        spans.push(Span::raw(format!(" {label}")));
        used += item_w;
    }
    if !spans.is_empty() {
        rows.push(Line::from(spans));
    }
    rows
}

fn items(list: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    list.iter().map(|(k, l)| ((*k).to_string(), *l)).collect()
}

fn camp_key_items() -> Vec<(String, &'static str)> {
    items(&[
        ("T", "Travel"),
        ("R", "Rest"),
        ("H", "Hunt"),
        ("F", "Repair"),
        ("P", "Pace"),
        ("X", "Abandon cargo"),
        ("D", "Desperate repair"),
        ("N", "Hard ration"),
        ("O", "Settings"),
        ("?", "Help"),
        ("Q", "Quit"),
    ])
}

fn block<'a>(th: &Theme, title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .padding(Padding::horizontal(1))
        .border_style(th.border())
        .title(Span::styled(format!(" {title} "), th.heading()))
}

fn plain_lines(lines: &[String]) -> Vec<Line<'static>> {
    lines.iter().map(|l| Line::from(l.clone())).collect()
}

fn draw_title(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(area);
    let width = usize::from(area.width.saturating_sub(8)).min(72);
    let lines = wrap(&title_screen_lines(&model.version), width);
    let top = rows[0].height.saturating_sub(lines.len() as u16) / 2;
    let mut text: Vec<Line<'static>> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i == 0 {
            text.push(Line::from(Span::styled(
                l.clone(),
                th.heading().add_modifier(Modifier::BOLD),
            )));
        } else {
            text.push(Line::from(l.clone()));
        }
    }
    let body = Rect::new(
        rows[0].x,
        rows[0].y + top,
        rows[0].width,
        rows[0].height.saturating_sub(top),
    );
    frame.render_widget(
        Paragraph::new(Text::from(text)).alignment(Alignment::Center),
        body,
    );
    let keys = key_rows(
        th,
        &items(&[
            ("Enter", "Start"),
            ("O", "Settings"),
            ("A", "About"),
            ("?", "Help"),
            ("Q", "Quit"),
        ]),
        usize::from(area.width),
    );
    frame.render_widget(
        Paragraph::new(Text::from(keys)).alignment(Alignment::Center),
        rows[1],
    );
    let _ = model;
}

fn draw_about(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    draw_header(model, th, frame, rows[0]);
    let lines = wrap(
        &about_screen_lines(&model.version, &model.content_hash),
        layout::text_width(rows[1]),
    );
    frame.render_widget(
        Paragraph::new(Text::from(plain_lines(&lines))).block(block(th, "About")),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Text::from(key_rows(
            th,
            &items(&[("Enter", "Back"), ("?", "Help"), ("Q", "Quit")]),
            usize::from(area.width),
        ))),
        rows[2],
    );
}

fn draw_settings(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    draw_header(model, th, frame, rows[0]);
    let mut lines = settings_screen_lines(&model.settings.settings);
    lines.push(String::new());
    if let Some(entry) = &model.settings_entry {
        lines.push(MODEL_PROMPT.to_string());
        lines.push(format!("> {entry}_"));
    } else if !model.settings_note.is_empty() {
        lines.push(model.settings_note.clone());
    }
    let lines = wrap(&lines, layout::text_width(rows[1]));
    frame.render_widget(
        Paragraph::new(Text::from(plain_lines(&lines))).block(block(th, "Settings")),
        rows[1],
    );
    let keys = if model.settings_entry.is_some() {
        items(&[("Enter", "Done"), ("Esc", "Keep the current one")])
    } else {
        items(&[
            ("1-4", "Change"),
            ("S", "Save and leave"),
            ("Q", "Leave without saving"),
            ("?", "Help"),
        ])
    };
    frame.render_widget(
        Paragraph::new(Text::from(key_rows(th, &keys, usize::from(area.width)))),
        rows[2],
    );
}

/// The supplies laid out in as many columns as the rows allow, filled down each column in
/// turn. Every supply is named, with its count beside it.
fn supply_rows(supplies: &[(String, i32)], rows: usize, width: usize) -> Vec<String> {
    if supplies.is_empty() || rows == 0 {
        return Vec::new();
    }
    let cols = supplies.len().div_ceil(rows);
    let col_w = (width / cols).max(1);
    let key_w = supplies
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    (0..rows)
        .map(|r| {
            let mut line = String::new();
            for c in 0..cols {
                if let Some((k, v)) = supplies.get(c * rows + r) {
                    let cell = format!(" {k:<key_w$} {v:>3}");
                    line.push_str(&format!("{cell:<col_w$}"));
                }
            }
            line.trim_end().to_string()
        })
        .collect()
}

/// The party, as rows of text: name, health, condition. A dead member reads `(dead)`.
fn party_lines(th: &Theme, status: &StatusFacts) -> Vec<Line<'static>> {
    let name_w = status
        .party
        .iter()
        .map(|m| m.name.chars().count())
        .max()
        .unwrap_or(0);
    status
        .party
        .iter()
        .map(|m| {
            if m.alive {
                Line::from(format!(
                    "  {:<name_w$}  {:>3}  {}",
                    m.name, m.health, m.condition
                ))
            } else {
                Line::from(Span::styled(
                    format!("  {:<name_w$}  (dead)", m.name),
                    th.dead(),
                ))
            }
        })
        .collect()
}

fn draw_camp(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let a = layout::camp_areas(area);
    draw_header(model, th, frame, a.header);
    let s = &model.view.status;

    let mut left: Vec<Line<'static>> = vec![
        Line::from(format!("{:<10}{}", "Place", s.place)),
        Line::from(format!("{:<10}{}, {}", "Day", s.day, s.time_of_day)),
        Line::from(format!(
            "{:<10}{} of {}",
            "Miles", s.distance_traveled, s.total_distance
        )),
        Line::from(format!("{:<10}{}", "Next stop", s.distance_remaining)),
        Line::from(format!(
            "Wagon {}  Animals {}  Pace {}",
            s.wagon_condition, s.animals_health, s.pace
        )),
        Line::from(Span::styled(
            format!("{:<10}morale {}", "Party", s.morale),
            th.heading(),
        )),
    ];
    left.extend(party_lines(th, s));
    frame.render_widget(
        Paragraph::new(Text::from(left)).block(block(th, "Status")),
        a.status,
    );

    let inner_h = layout::text_rows(a.supplies);
    let inner_w = layout::text_width(a.supplies);
    let rows = supply_rows(&s.supplies, inner_h, inner_w);
    frame.render_widget(
        Paragraph::new(Text::from(plain_lines(&rows))).block(block(th, "Supplies")),
        a.supplies,
    );

    draw_log(model, th, frame, a.log);
    frame.render_widget(
        Paragraph::new(Text::from(key_rows(
            th,
            &camp_key_items(),
            usize::from(area.width),
        ))),
        a.keys,
    );
}

fn draw_log(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let width = layout::text_width(area);
    let rows = layout::text_rows(area);
    let wrapped = wrap(&model.log, width);
    let max = wrapped.len().saturating_sub(rows);
    let scroll = model.log_scroll.min(max);
    let end = wrapped.len() - scroll;
    let start = end.saturating_sub(rows);
    let title = if scroll > 0 { "Log (scrolled)" } else { "Log" };
    frame.render_widget(
        Paragraph::new(Text::from(plain_lines(&wrapped[start..end]))).block(block(th, title)),
        area,
    );
}

fn event_key_items(model: &Model) -> Vec<(String, &'static str)> {
    let mut v: Vec<(String, &'static str)> = Vec::new();
    if model.resolved.is_some() {
        v.push(("Enter".to_string(), "Continue"));
    } else {
        let ids = model.view.offered();
        let range = match (ids.first(), ids.last()) {
            (Some(a), Some(b)) if a != b => format!("{a}-{b}"),
            (Some(a), _) => a.clone(),
            _ => String::new(),
        };
        if !range.is_empty() {
            v.push((range, "Choose"));
        }
        v.push(("O".to_string(), "Settings"));
    }
    v.push(("?".to_string(), "Help"));
    v.push(("Q".to_string(), "Quit"));
    v
}

/// The event, the fork or the outcome just decided: the engine's title and text, then the
/// choices or what happened.
fn draw_event(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let keys = key_rows(th, &event_key_items(model), usize::from(area.width));
    let keys_h = (keys.len() as u16).max(1);
    let avail = area.height.saturating_sub(1 + keys_h);
    let (title, mut content) = match &model.resolved {
        Some(r) => {
            let mut c = Vec::new();
            if !r.text.is_empty() {
                c.push(r.text.clone());
                c.push(String::new());
            }
            c.extend(r.lines.iter().cloned());
            (r.title.clone(), c)
        }
        None if model.screen == Screen::Fork => (
            crate::play::FORK_LINE.to_string(),
            model.view.choices.iter().map(|c| c.text.clone()).collect(),
        ),
        None => {
            let mut c = vec![model.view.event_text.clone(), String::new()];
            c.extend(model.view.choices.iter().map(|ch| ch.text.clone()));
            (model.view.event_title.clone(), c)
        }
    };
    content = wrap(&content, layout::text_width(area));
    let need = content.len() as u16 + 2;
    let log_h = if model.resolved.is_some() {
        0
    } else {
        let spare = avail.saturating_sub(need);
        if spare >= 4 {
            spare.min(8)
        } else {
            0
        }
    };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(avail - log_h),
        Constraint::Length(log_h),
        Constraint::Length(keys_h),
    ])
    .split(area);
    draw_header(model, th, frame, rows[0]);
    frame.render_widget(
        Paragraph::new(Text::from(plain_lines(&content))).block(block(th, &title)),
        rows[1],
    );
    if log_h > 0 {
        draw_log(model, th, frame, rows[2]);
    }
    frame.render_widget(Paragraph::new(Text::from(keys)), rows[3]);
}

fn draw_game_over(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let a = layout::ledger_areas(area);
    draw_header(model, th, frame, a.header);
    let width = layout::text_width(a.ledger);
    let rows = layout::text_rows(a.ledger);
    let wrapped = wrap(&model.view.ledger, width);
    let max = wrapped.len().saturating_sub(rows);
    let start = model.ledger_scroll.min(max);
    let end = (start + rows).min(wrapped.len());
    let title = if max > 0 {
        format!("Trail Ledger {}-{} of {}", start + 1, end, wrapped.len())
    } else {
        "Trail Ledger".to_string()
    };
    frame.render_widget(
        Paragraph::new(Text::from(plain_lines(&wrapped[start..end]))).block(block(th, &title)),
        a.ledger,
    );
    if let Some(summary) = &model.narrator_summary {
        frame.render_widget(Paragraph::new(summary.clone()), a.summary);
    }
    let mut keys = Vec::new();
    if max > 0 {
        keys.push(("Up/Down".to_string(), "Scroll"));
        keys.push(("PgUp/PgDn".to_string(), "Page"));
    }
    keys.push(("?".to_string(), "Help"));
    keys.push(("Q".to_string(), "Quit"));
    frame.render_widget(
        Paragraph::new(Text::from(key_rows(th, &keys, usize::from(area.width)))),
        a.keys,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn draw_help(th: &Theme, frame: &mut Frame, area: Rect) {
    let lines: Vec<String> = HELP_LINES.iter().map(|s| (*s).to_string()).collect();
    let r = centered(area, 60, lines.len() as u16 + 2);
    frame.render_widget(Clear, r);
    frame.render_widget(
        Paragraph::new(Text::from(plain_lines(&lines))).block(block(th, "Help")),
        r,
    );
}

fn draw_quit(th: &Theme, frame: &mut Frame, area: Rect) {
    let r = centered(area, 30, 3);
    frame.render_widget(Clear, r);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Y", th.key()),
            Span::raw(" Yes   "),
            Span::styled("N", th.key()),
            Span::raw(" No"),
        ]))
        .alignment(Alignment::Center)
        .block(block(th, "Quit")),
        r,
    );
}

fn draw_pace(model: &Model, th: &Theme, frame: &mut Frame, area: Rect) {
    let r = centered(area, 30, PACES.len() as u16 + 2);
    frame.render_widget(Clear, r);
    let mut lines = Vec::new();
    for (i, pace) in PACES.iter().enumerate() {
        let hot = i == model.pace_cursor;
        let current = if *pace == model.view.status.pace {
            " (now)"
        } else {
            ""
        };
        let text = format!(
            "{} {} {pace}{current}",
            if hot { ">" } else { " " },
            PACE_KEYS[i]
        );
        let style = if hot { th.selected() } else { Style::new() };
        lines.push(Line::from(Span::styled(text, style)));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(block(th, "Pace")),
        r,
    );
}
