//! Where things go on the screen, and how text is wrapped to fit.
//!
//! The view draws into these rectangles and the update step measures scrolling against the same
//! ones, so a scroll key never moves past the end of what is drawn.

use ratatui::layout::Rect;

use super::model::{Model, MIN_HEIGHT, MIN_WIDTH};

/// Rows the camp status panel takes, borders included.
pub const STATUS_HEIGHT: u16 = 12;
/// Rows the camp key bar takes.
pub const CAMP_KEYS_HEIGHT: u16 = 2;

/// `true` when the window is too small to draw in.
pub fn too_small(width: u16, height: u16) -> bool {
    width < MIN_WIDTH || height < MIN_HEIGHT
}

/// The rectangles of the camp screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampAreas {
    pub header: Rect,
    pub status: Rect,
    pub supplies: Rect,
    pub log: Rect,
    pub keys: Rect,
}

/// Split a window for the camp screen: a header row, the status panel on the left and the
/// supplies on the right, the message log under them, and the key bar at the bottom. Wider
/// windows give the panels more columns and the log more rows.
pub fn camp_areas(area: Rect) -> CampAreas {
    let header = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    let keys_h = CAMP_KEYS_HEIGHT.min(area.height.saturating_sub(1));
    let keys = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(keys_h),
        area.width,
        keys_h,
    );
    let status_h = STATUS_HEIGHT.min(area.height.saturating_sub(1 + keys_h));
    let left_w = area.width * 48 / 100;
    let status = Rect::new(area.x, area.y + 1, left_w, status_h);
    let supplies = Rect::new(
        area.x + left_w,
        area.y + 1,
        area.width.saturating_sub(left_w),
        status_h,
    );
    let log_y = area.y + 1 + status_h;
    let log_h = keys.y.saturating_sub(log_y);
    let log = Rect::new(area.x, log_y, area.width, log_h);
    CampAreas {
        header,
        status,
        supplies,
        log,
        keys,
    }
}

/// The rectangles of the game-over screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerAreas {
    pub header: Rect,
    pub ledger: Rect,
    pub summary: Rect,
    pub keys: Rect,
}

/// Split a window for the game-over screen: a header row, the Trail Ledger, one row for the
/// narrator summary and one for the key bar.
pub fn ledger_areas(area: Rect) -> LedgerAreas {
    let h = area.height;
    let header = Rect::new(area.x, area.y, area.width, 1.min(h));
    let keys = Rect::new(area.x, area.y + h.saturating_sub(1), area.width, 1.min(h));
    let summary = Rect::new(
        area.x,
        area.y + h.saturating_sub(2),
        area.width,
        if h >= 2 { 1 } else { 0 },
    );
    let ledger_h = h.saturating_sub(3);
    let ledger = Rect::new(area.x, area.y + 1.min(h), area.width, ledger_h);
    LedgerAreas {
        header,
        ledger,
        summary,
        keys,
    }
}

fn window(model: &Model) -> Rect {
    Rect::new(0, 0, model.size.0, model.size.1)
}

/// Columns of text a bordered panel shows: its width less the border and one column of padding
/// on each side.
pub fn text_width(r: Rect) -> usize {
    usize::from(r.width.saturating_sub(4))
}

/// Rows of text a bordered panel shows.
pub fn text_rows(r: Rect) -> usize {
    usize::from(r.height.saturating_sub(2))
}

fn inner(r: Rect) -> (usize, usize) {
    (text_width(r), text_rows(r))
}

/// `(wrapped lines, rows showing)` of the camp message log at the model's window size.
pub fn log_metrics(model: &Model) -> (usize, usize) {
    let (w, rows) = inner(camp_areas(window(model)).log);
    (wrap(&model.log, w).len(), rows)
}

/// `(wrapped lines, rows showing)` of the Trail Ledger at the model's window size.
pub fn ledger_metrics(model: &Model) -> (usize, usize) {
    let (w, rows) = inner(ledger_areas(window(model)).ledger);
    (wrap(&model.view.ledger, w).len(), rows)
}

/// Leading columns a wrapped line keeps: the line's own indent, plus three more under a
/// `A. ` style choice marker so the label lines up.
fn hang(line: &str) -> usize {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest: Vec<char> = line.trim_start_matches(' ').chars().take(3).collect();
    let marker =
        rest.len() == 3 && rest[0].is_ascii_alphanumeric() && rest[1] == '.' && rest[2] == ' ';
    if marker {
        indent + 3
    } else {
        indent
    }
}

/// Wrap each line to `width` columns at spaces. Blank lines stay. A word longer than the
/// width is split. Continuation lines are indented as [`hang`] says.
pub fn wrap(lines: &[String], width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        wrap_line(line, width, &mut out);
    }
    out
}

fn wrap_line(line: &str, width: usize, out: &mut Vec<String>) {
    let width = width.max(1);
    let line = line.trim_end();
    if line.chars().count() <= width {
        out.push(line.to_string());
        return;
    }
    let hang = hang(line).min(width.saturating_sub(1));
    let lead: String = line.chars().take_while(|c| *c == ' ').collect();
    let pad = " ".repeat(hang);
    let mut current = lead.clone();
    let mut has_word = false;
    for word in line.split_whitespace() {
        let mut word: String = word.to_string();
        loop {
            let used = current.chars().count();
            let need = word.chars().count() + usize::from(has_word);
            if used + need <= width {
                if has_word {
                    current.push(' ');
                }
                current.push_str(&word);
                has_word = true;
                break;
            }
            if has_word {
                out.push(std::mem::take(&mut current));
                current = pad.clone();
                has_word = false;
                continue;
            }
            // A word wider than the room left on an empty line: split it.
            let room = width.saturating_sub(used).max(1);
            let head: String = word.chars().take(room).collect();
            let tail: String = word.chars().skip(room).collect();
            current.push_str(&head);
            out.push(std::mem::take(&mut current));
            current = pad.clone();
            word = tail;
            if word.is_empty() {
                break;
            }
        }
    }
    if has_word || !current.trim().is_empty() {
        out.push(current);
    }
}
