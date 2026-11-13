//! Content-diff renderer: compares two content packs and renders the changes as a table
//! (Markdown or self-contained HTML) instead of a JSON diff.
//!
//! The renderer reads, formats and compares only. It generates no prose and passes no
//! judgement on prose quality; every flag is a mechanical check computed in code.
//!
//! Output is deterministic: events and choices keep file order, effects are sorted by key,
//! line endings are always `\n`, and nothing carries a timestamp.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::content::ContentPack;

/// Maximum choices per event that the content loader keeps (`worldgen::MAX_CHOICES`).
const MAX_CHOICES: usize = 4;

/// Flag code: a digit appears in the outcome sentence.
pub const FLAG_DIGIT: &str = "DIGIT";
/// Flag code: the outcome sentence is not 2-3 sentences long.
pub const FLAG_LEN: &str = "LEN";
/// Flag code: no outcome sentence.
pub const FLAG_EMPTY: &str = "EMPTY";
/// Flag code: the outcome sentence repeats the choice label.
pub const FLAG_ECHO: &str = "ECHO";
/// Flag code: the same outcome sentence appears on another choice in the new pack.
pub const FLAG_DUP: &str = "DUP";
/// Flag code: characters other than letters, spaces, apostrophes, periods and commas.
pub const FLAG_PUNCT: &str = "PUNCT";
/// Flag code: a hint ends in doubled punctuation such as `..` or `.)`.
pub const FLAG_HINT_DOUBLE_PUNCT: &str = "HINT_DOUBLE_PUNCT";

/// Options for [`diff_packs`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffOptions {
    /// List every choice of the new pack, not only the changed ones.
    pub all: bool,
}

/// An old/new pair of one text value. `None` means the value does not exist on that side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub old: Option<String>,
    pub new: Option<String>,
}

impl Pair {
    fn changed(&self) -> bool {
        self.old != self.new
    }
}

/// One choice row of the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRow {
    pub event_id: String,
    pub title: String,
    pub band: Option<i64>,
    pub tags: String,
    pub narration: String,
    pub choice_id: String,
    pub label: Pair,
    pub action: Pair,
    pub effects: Pair,
    pub cost_hint: Pair,
    pub risk_hint: Pair,
    pub sentence: Pair,
    pub words: usize,
    pub sentences: usize,
    pub flags: Vec<&'static str>,
    pub changed: bool,
}

/// An added or removed event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSummary {
    pub event_id: String,
    pub title: String,
    pub band: Option<i64>,
    pub choices: usize,
}

/// One changed non-choice field of an event that exists on both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub event_id: String,
    pub field: String,
    pub old: String,
    pub new: String,
}

/// The full comparison of two content packs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDiff {
    pub old_hash: String,
    pub new_hash: String,
    pub old_events: usize,
    pub new_events: usize,
    pub old_choices: usize,
    pub new_choices: usize,
    pub changed_choices: usize,
    pub all: bool,
    pub rows: Vec<ChoiceRow>,
    pub added: Vec<EventSummary>,
    pub removed: Vec<EventSummary>,
    pub field_changes: Vec<FieldChange>,
    pub mechanics_changed: bool,
}

struct ChoiceView {
    id: String,
    label: String,
    action: String,
    effects: String,
    cost_hint: String,
    risk_hint: String,
    sentence: Option<String>,
    raw: Value,
}

struct EventView {
    id: String,
    title: String,
    band: Option<i64>,
    tags: String,
    narration: String,
    choices: Vec<ChoiceView>,
    raw: Value,
}

fn str_field(obj: &serde_json::Map<String, Value>, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Render one JSON value as display text (strings without quotes, arrays joined by ", ").
fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(value_text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// Render an `engine_effect_profile` object as `key +n, key -n`, sorted by key.
fn effects_text(profile: Option<&Value>) -> String {
    let Some(Value::Object(map)) = profile else {
        return String::new();
    };
    let sorted: BTreeMap<&String, &Value> = map.iter().collect();
    let parts: Vec<String> = sorted
        .into_iter()
        .map(|(k, v)| match v.as_i64() {
            Some(n) => format!("{k} {n:+}"),
            None => format!("{k} {}", value_text(v)),
        })
        .collect();
    parts.join(", ")
}

fn events_array(root: &Value) -> Option<&Vec<Value>> {
    match root {
        Value::Array(items) => Some(items),
        Value::Object(map) => map.get("events").and_then(Value::as_array),
        _ => None,
    }
}

fn event_views(root: &Value) -> Vec<EventView> {
    let mut out = Vec::new();
    for raw in events_array(root).map(Vec::as_slice).unwrap_or(&[]) {
        let Some(obj) = raw.as_object() else {
            continue;
        };
        let Some(id) = obj
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let raw_choices = obj
            .get("choices")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut choices = Vec::new();
        for (i, c) in raw_choices.iter().take(MAX_CHOICES).enumerate() {
            let empty = serde_json::Map::new();
            let cobj = c.as_object().unwrap_or(&empty);
            let cid = cobj
                .get("choice_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| char::from(b'A' + (i as u8)).to_string());
            choices.push(ChoiceView {
                id: cid,
                label: str_field(cobj, "label"),
                action: str_field(cobj, "intent_action"),
                effects: effects_text(cobj.get("engine_effect_profile")),
                cost_hint: str_field(cobj, "cost_hint"),
                risk_hint: str_field(cobj, "risk_hint"),
                sentence: cobj
                    .get("outcome_sentence")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                raw: c.clone(),
            });
        }
        out.push(EventView {
            id: id.to_string(),
            title: str_field(obj, "title"),
            band: obj.get("weirdness_band").and_then(Value::as_i64),
            tags: obj.get("tags").map(value_text).unwrap_or_default(),
            narration: str_field(obj, "narration_seed"),
            choices,
            raw: raw.clone(),
        });
    }
    out
}

/// Copy of `v` with every text-only field removed, so two packs that differ only in text
/// compare equal. Text-only fields are title, narration, labels, hints and outcome sentences.
fn strip_text_fields(v: &Value) -> Value {
    let mut v = v.clone();
    if let Value::Object(map) = &mut v {
        map.remove("title");
        map.remove("narration_seed");
        if let Some(Value::Array(choices)) = map.get_mut("choices") {
            for c in choices.iter_mut() {
                if let Value::Object(cm) = c {
                    for k in ["label", "risk_hint", "cost_hint", "outcome_sentence"] {
                        cm.remove(k);
                    }
                }
            }
        }
    }
    v
}

fn strip_root(root: &Value) -> Value {
    match root {
        Value::Array(items) => Value::Array(items.iter().map(strip_text_fields).collect()),
        Value::Object(map) => {
            let mut map = map.clone();
            if let Some(Value::Array(items)) = map.get("events") {
                let stripped: Vec<Value> = items.iter().map(strip_text_fields).collect();
                map.insert("events".to_string(), Value::Array(stripped));
            }
            Value::Object(map)
        }
        other => other.clone(),
    }
}

/// Number of words in `s` (whitespace-separated).
pub fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Number of sentences in `s`: segments ended by `.`, `!` or `?` that hold a letter or digit,
/// plus a trailing unterminated segment that does.
pub fn sentence_count(s: &str) -> usize {
    let mut n = 0;
    let mut has_text = false;
    for c in s.chars() {
        if matches!(c, '.' | '!' | '?') {
            if has_text {
                n += 1;
                has_text = false;
            }
        } else if c.is_alphanumeric() {
            has_text = true;
        }
    }
    if has_text {
        n += 1;
    }
    n
}

fn hint_has_double_punct(hint: &str) -> bool {
    let mut it = hint.trim_end().chars().rev();
    matches!((it.next(), it.next()), (Some(a), Some(b)) if a.is_ascii_punctuation() && b.is_ascii_punctuation())
}

/// Mechanical flags for one choice, in the fixed order
/// `EMPTY, DIGIT, LEN, ECHO, DUP, PUNCT, HINT_DOUBLE_PUNCT`.
///
/// `duplicate` is true when the same outcome sentence appears on another choice in the pack.
pub fn mechanical_flags(
    label: &str,
    sentence: Option<&str>,
    cost_hint: &str,
    risk_hint: &str,
    duplicate: bool,
) -> Vec<&'static str> {
    let mut flags = Vec::new();
    let text = sentence.map(str::trim).unwrap_or("");
    if text.is_empty() {
        flags.push(FLAG_EMPTY);
    } else {
        if text.chars().any(|c| c.is_ascii_digit()) {
            flags.push(FLAG_DIGIT);
        }
        let n = sentence_count(text);
        if !(2..=3).contains(&n) {
            flags.push(FLAG_LEN);
        }
        let label_lc = label.trim().to_lowercase();
        if !label_lc.is_empty() && text.to_lowercase().contains(&label_lc) {
            flags.push(FLAG_ECHO);
        }
        if duplicate {
            flags.push(FLAG_DUP);
        }
        if text
            .chars()
            .any(|c| !(c.is_alphabetic() || matches!(c, ' ' | '\'' | '.' | ',')))
        {
            flags.push(FLAG_PUNCT);
        }
    }
    if hint_has_double_punct(cost_hint) || hint_has_double_punct(risk_hint) {
        flags.push(FLAG_HINT_DOUBLE_PUNCT);
    }
    flags
}

fn row_for(
    ev: &EventView,
    old: Option<&ChoiceView>,
    new: Option<&ChoiceView>,
    dups: &BTreeMap<String, usize>,
) -> ChoiceRow {
    let id = new.or(old).map(|c| c.id.clone()).unwrap_or_default();
    let pair = |f: fn(&ChoiceView) -> String| Pair {
        old: old.map(f),
        new: new.map(f),
    };
    let sentence = Pair {
        old: old.and_then(|c| c.sentence.clone()),
        new: new.and_then(|c| c.sentence.clone()),
    };
    let new_text = new.and_then(|c| c.sentence.as_deref()).unwrap_or("");
    let duplicate = dups.get(new_text.trim()).copied().unwrap_or(0) > 1;
    let flags = match new {
        Some(c) => mechanical_flags(
            &c.label,
            c.sentence.as_deref(),
            &c.cost_hint,
            &c.risk_hint,
            duplicate,
        ),
        None => Vec::new(),
    };
    ChoiceRow {
        event_id: ev.id.clone(),
        title: ev.title.clone(),
        band: ev.band,
        tags: ev.tags.clone(),
        narration: ev.narration.clone(),
        choice_id: id,
        label: pair(|c| c.label.clone()),
        action: pair(|c| c.action.clone()),
        effects: pair(|c| c.effects.clone()),
        cost_hint: pair(|c| c.cost_hint.clone()),
        risk_hint: pair(|c| c.risk_hint.clone()),
        words: word_count(new_text),
        sentences: sentence_count(new_text),
        sentence,
        flags,
        changed: match (old, new) {
            (Some(o), Some(n)) => o.raw != n.raw,
            _ => true,
        },
    }
}

fn summary(ev: &EventView) -> EventSummary {
    EventSummary {
        event_id: ev.id.clone(),
        title: ev.title.clone(),
        band: ev.band,
        choices: ev.choices.len(),
    }
}

/// Compare two content packs.
pub fn diff_packs(old: &ContentPack, new: &ContentPack, opts: DiffOptions) -> ContentDiff {
    let old_events = event_views(&old.raw_events_value);
    let new_events = event_views(&new.raw_events_value);

    let mut old_index: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, e) in old_events.iter().enumerate() {
        old_index.entry(e.id.as_str()).or_insert(i);
    }
    let mut new_index: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, e) in new_events.iter().enumerate() {
        new_index.entry(e.id.as_str()).or_insert(i);
    }

    let mut dups: BTreeMap<String, usize> = BTreeMap::new();
    for e in &new_events {
        for c in &e.choices {
            if let Some(s) = &c.sentence {
                let t = s.trim();
                if !t.is_empty() {
                    *dups.entry(t.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut rows = Vec::new();
    let mut field_changes = Vec::new();
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for ev in &new_events {
        let Some(&oi) = old_index.get(ev.id.as_str()) else {
            added.push(summary(ev));
            continue;
        };
        let oev = &old_events[oi];

        // Non-choice fields, one entry per changed field.
        let empty = serde_json::Map::new();
        let (oo, no) = (
            oev.raw.as_object().unwrap_or(&empty),
            ev.raw.as_object().unwrap_or(&empty),
        );
        let mut keys: Vec<&String> = oo.keys().chain(no.keys()).collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            if k == "id" || k == "choices" {
                continue;
            }
            if oo.get(k) != no.get(k) {
                let show = |v: Option<&Value>| v.map(value_text).unwrap_or_else(|| "(none)".into());
                field_changes.push(FieldChange {
                    event_id: ev.id.clone(),
                    field: k.clone(),
                    old: show(oo.get(k)),
                    new: show(no.get(k)),
                });
            }
        }

        for c in &ev.choices {
            let o = oev.choices.iter().find(|x| x.id == c.id);
            let row = row_for(ev, o, Some(c), &dups);
            if row.changed || opts.all {
                rows.push(row);
            }
        }
        for o in &oev.choices {
            if !ev.choices.iter().any(|x| x.id == o.id) {
                rows.push(row_for(ev, Some(o), None, &dups));
            }
        }
    }
    for ev in &old_events {
        if !new_index.contains_key(ev.id.as_str()) {
            removed.push(summary(ev));
        }
    }

    let mechanics_changed = old.raw_manifest_value != new.raw_manifest_value
        || strip_root(&old.raw_events_value) != strip_root(&new.raw_events_value);
    let changed_choices = rows.iter().filter(|r| r.changed).count();
    ContentDiff {
        old_hash: old.content_hash_hex.clone(),
        new_hash: new.content_hash_hex.clone(),
        old_events: old_events.len(),
        new_events: new_events.len(),
        old_choices: old_events.iter().map(|e| e.choices.len()).sum(),
        new_choices: new_events.iter().map(|e| e.choices.len()).sum(),
        changed_choices,
        all: opts.all,
        rows,
        added,
        removed,
        field_changes,
        mechanics_changed,
    }
}

// ── Markdown ────────────────────────────────────────────────────────────────

/// Escape text for one GitHub-flavoured Markdown table cell: pipes escaped, newlines `<br>`.
pub fn md_cell(s: &str) -> String {
    s.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace(['\r', '\n'], "<br>")
}

fn pair_text(p: &Pair) -> String {
    if !p.changed() {
        return p.new.clone().unwrap_or_default();
    }
    let show = |v: &Option<String>| match v {
        None => "(none)".to_string(),
        Some(s) if s.is_empty() => "(empty)".to_string(),
        Some(s) => s.clone(),
    };
    format!("{} -> {}", show(&p.old), show(&p.new))
}

fn band_text(b: Option<i64>) -> String {
    b.map(|n| n.to_string()).unwrap_or_default()
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn md_row(cells: &[String]) -> String {
    let mut s = String::from("|");
    for c in cells {
        s.push(' ');
        s.push_str(&md_cell(c));
        s.push_str(" |");
    }
    s.push('\n');
    s
}

fn md_header(cols: &[&str]) -> String {
    let mut s = md_row(&cols.iter().map(|c| c.to_string()).collect::<Vec<_>>());
    s.push('|');
    for _ in cols {
        s.push_str(" --- |");
    }
    s.push('\n');
    s
}

/// Render the diff as GitHub-flavoured Markdown (LF line endings).
pub fn render_markdown(d: &ContentDiff) -> String {
    let mut o = String::new();
    o.push_str("# Content diff\n\n");
    o.push_str(&format!("- old content hash: `{}`\n", d.old_hash));
    o.push_str(&format!("- new content hash: `{}`\n", d.new_hash));
    o.push_str(&format!(
        "- events: old {}, new {}\n",
        d.old_events, d.new_events
    ));
    o.push_str(&format!(
        "- choices: old {}, new {}\n",
        d.old_choices, d.new_choices
    ));
    o.push_str(&format!("- changed choices: {}\n", d.changed_choices));
    o.push_str(&format!("- added events: {}\n", d.added.len()));
    o.push_str(&format!("- removed events: {}\n", d.removed.len()));
    o.push_str(&format!(
        "- changed event fields: {}\n",
        d.field_changes.len()
    ));
    o.push_str(&format!(
        "- mechanics changed: {}\n\n",
        yes_no(d.mechanics_changed)
    ));

    o.push_str(if d.all {
        "## All choices\n\n"
    } else {
        "## Changed choices\n\n"
    });
    if d.rows.is_empty() {
        o.push_str("None.\n\n");
    } else {
        o.push_str(&md_header(&[
            "Event",
            "Title",
            "Band",
            "Tags",
            "Narration",
            "Choice",
            "Label",
            "Action",
            "Effects",
            "Cost hint",
            "Risk hint",
            "Outcome sentence",
            "Words",
            "Sentences",
            "Flags",
        ]));
        for r in &d.rows {
            o.push_str(&md_row(&[
                r.event_id.clone(),
                r.title.clone(),
                band_text(r.band),
                r.tags.clone(),
                r.narration.clone(),
                r.choice_id.clone(),
                pair_text(&r.label),
                pair_text(&r.action),
                pair_text(&r.effects),
                pair_text(&r.cost_hint),
                pair_text(&r.risk_hint),
                pair_text(&r.sentence),
                r.words.to_string(),
                r.sentences.to_string(),
                r.flags.join(" "),
            ]));
        }
        o.push('\n');
    }

    for (title, list) in [("Added events", &d.added), ("Removed events", &d.removed)] {
        o.push_str(&format!("## {title}\n\n"));
        if list.is_empty() {
            o.push_str("None.\n\n");
            continue;
        }
        o.push_str(&md_header(&["Event", "Title", "Band", "Choices"]));
        for e in list {
            o.push_str(&md_row(&[
                e.event_id.clone(),
                e.title.clone(),
                band_text(e.band),
                e.choices.to_string(),
            ]));
        }
        o.push('\n');
    }

    o.push_str("## Changed event fields\n\n");
    if d.field_changes.is_empty() {
        o.push_str("None.\n");
    } else {
        o.push_str(&md_header(&["Event", "Field", "Old", "New"]));
        for f in &d.field_changes {
            o.push_str(&md_row(&[
                f.event_id.clone(),
                f.field.clone(),
                f.old.clone(),
                f.new.clone(),
            ]));
        }
    }
    o
}

// ── HTML ────────────────────────────────────────────────────────────────────

/// Escape `& < > " '` for HTML text.
pub fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            other => o.push(other),
        }
    }
    o
}

fn td(class: &str, s: &str) -> String {
    if class.is_empty() {
        format!("<td>{}</td>", html_escape(s))
    } else {
        format!("<td class=\"{class}\">{}</td>", html_escape(s))
    }
}

const HTML_CSS: &str = "body{font-family:sans-serif;margin:1.5em;background:#fff;color:#111}\
table{border-collapse:collapse;margin-bottom:1.5em}\
th,td{border:1px solid #999;padding:3px 6px;text-align:left;vertical-align:top;font-size:13px}\
th{background:#eee}\
td.old{background:#fdeaea}td.new{background:#eafdea}\
span.flag{display:inline-block;border:1px solid #a60;background:#ffe9c4;border-radius:3px;padding:0 4px;margin:1px;font-size:11px}\
code{font-size:12px}";

fn html_table(head: &[&str], rows: Vec<String>) -> String {
    let mut o = String::from("<table>\n<tr>");
    for h in head {
        o.push_str(&format!("<th>{}</th>", html_escape(h)));
    }
    o.push_str("</tr>\n");
    for r in rows {
        o.push_str(&r);
        o.push('\n');
    }
    o.push_str("</table>\n");
    o
}

/// Render the diff as one self-contained HTML page: inline CSS only, no scripts, no external
/// URLs, every text value escaped, old and new text in separate cells (LF line endings).
pub fn render_html(d: &ContentDiff) -> String {
    let mut o = String::new();
    o.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    o.push_str("<title>Content diff</title>\n");
    o.push_str(&format!("<style>{HTML_CSS}</style>\n</head>\n<body>\n"));
    o.push_str("<h1>Content diff</h1>\n<ul>\n");
    let items = [
        format!(
            "old content hash: <code>{}</code>",
            html_escape(&d.old_hash)
        ),
        format!(
            "new content hash: <code>{}</code>",
            html_escape(&d.new_hash)
        ),
        format!("events: old {}, new {}", d.old_events, d.new_events),
        format!("choices: old {}, new {}", d.old_choices, d.new_choices),
        format!("changed choices: {}", d.changed_choices),
        format!("added events: {}", d.added.len()),
        format!("removed events: {}", d.removed.len()),
        format!("changed event fields: {}", d.field_changes.len()),
        format!("mechanics changed: {}", yes_no(d.mechanics_changed)),
    ];
    for i in items {
        o.push_str(&format!("<li>{i}</li>\n"));
    }
    o.push_str("</ul>\n");

    o.push_str(if d.all {
        "<h2>All choices</h2>\n"
    } else {
        "<h2>Changed choices</h2>\n"
    });
    if d.rows.is_empty() {
        o.push_str("<p>None.</p>\n");
    } else {
        let rows = d
            .rows
            .iter()
            .map(|r| {
                let flags: String = r
                    .flags
                    .iter()
                    .map(|f| format!("<span class=\"flag\">{}</span>", html_escape(f)))
                    .collect();
                let mut s = String::from("<tr>");
                s.push_str(&td("", &r.event_id));
                s.push_str(&td("", &r.title));
                s.push_str(&td("", &band_text(r.band)));
                s.push_str(&td("", &r.tags));
                s.push_str(&td("", &r.narration));
                s.push_str(&td("", &r.choice_id));
                for p in [
                    &r.label,
                    &r.action,
                    &r.effects,
                    &r.cost_hint,
                    &r.risk_hint,
                    &r.sentence,
                ] {
                    s.push_str(&td("old", p.old.as_deref().unwrap_or("")));
                    s.push_str(&td("new", p.new.as_deref().unwrap_or("")));
                }
                s.push_str(&td("", &r.words.to_string()));
                s.push_str(&td("", &r.sentences.to_string()));
                s.push_str(&format!("<td>{flags}</td></tr>"));
                s
            })
            .collect();
        o.push_str(&html_table(
            &[
                "Event",
                "Title",
                "Band",
                "Tags",
                "Narration",
                "Choice",
                "Label (old)",
                "Label (new)",
                "Action (old)",
                "Action (new)",
                "Effects (old)",
                "Effects (new)",
                "Cost hint (old)",
                "Cost hint (new)",
                "Risk hint (old)",
                "Risk hint (new)",
                "Outcome sentence (old)",
                "Outcome sentence (new)",
                "Words",
                "Sentences",
                "Flags",
            ],
            rows,
        ));
    }

    for (title, list) in [("Added events", &d.added), ("Removed events", &d.removed)] {
        o.push_str(&format!("<h2>{title}</h2>\n"));
        if list.is_empty() {
            o.push_str("<p>None.</p>\n");
            continue;
        }
        let rows = list
            .iter()
            .map(|e| {
                format!(
                    "<tr>{}{}{}{}</tr>",
                    td("", &e.event_id),
                    td("", &e.title),
                    td("", &band_text(e.band)),
                    td("", &e.choices.to_string())
                )
            })
            .collect();
        o.push_str(&html_table(&["Event", "Title", "Band", "Choices"], rows));
    }

    o.push_str("<h2>Changed event fields</h2>\n");
    if d.field_changes.is_empty() {
        o.push_str("<p>None.</p>\n");
    } else {
        let rows = d
            .field_changes
            .iter()
            .map(|f| {
                format!(
                    "<tr>{}{}{}{}</tr>",
                    td("", &f.event_id),
                    td("", &f.field),
                    td("old", &f.old),
                    td("new", &f.new)
                )
            })
            .collect();
        o.push_str(&html_table(&["Event", "Field", "Old", "New"], rows));
    }
    o.push_str("</body>\n</html>\n");
    o
}

// ── Loading and command line ────────────────────────────────────────────────

/// Usage text printed by `--help`.
pub const USAGE: &str = "\
Usage: etv_content_diff <old_dir> <new_dir> [--md <out.md>] [--html <out.html>] [--all]

Compare two content directories (each holding content_manifest.json and
event_skeletons.json) and render the changes as a table.

  --md <file>    write the Markdown table to <file>
  --html <file>  write a self-contained HTML page to <file>
  --all          list every choice, not only the changed ones
  -h, --help     print this help

With neither --md nor --html, Markdown is written to stdout.
Exit codes: 0 ok, 1 unreadable or invalid content, 2 bad arguments.
";

/// Read and parse one content directory.
pub fn load_pack_from_dir(dir: &Path) -> Result<ContentPack, String> {
    let read = |name: &str| -> Result<(String, String), String> {
        let path = dir.join(name);
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        serde_json::from_str::<Value>(&text)
            .map_err(|e| format!("invalid JSON in {}: {e}", path.display()))?;
        Ok((path.display().to_string(), text))
    };
    let (_, manifest) = read("content_manifest.json")?;
    let (events_path, events) = read("event_skeletons.json")?;
    let pack = ContentPack::from_raw_strings(&manifest, &events)
        .map_err(|e| format!("invalid content in {}: {e}", dir.display()))?;
    if events_array(&pack.raw_events_value).is_none() {
        return Err(format!(
            "invalid content in {events_path}: expected a JSON array or an object with an 'events' array"
        ));
    }
    Ok(pack)
}

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub old_dir: String,
    pub new_dir: String,
    pub md: Option<String>,
    pub html: Option<String>,
    pub all: bool,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliRequest {
    Help,
    Run(CliArgs),
}

/// Parse arguments (without the program name). Errors are the stderr message (exit 2).
pub fn parse_args(args: &[String]) -> Result<CliRequest, String> {
    let mut positional = Vec::new();
    let (mut md, mut html, mut all) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(CliRequest::Help),
            "--all" => all = true,
            "--md" | "--html" => {
                let Some(v) = it.next() else {
                    return Err(format!("{a} needs a file path"));
                };
                if a == "--md" {
                    md = Some(v.clone());
                } else {
                    html = Some(v.clone());
                }
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown option {other}"));
            }
            _ => positional.push(a.clone()),
        }
    }
    if positional.len() != 2 {
        return Err(format!(
            "expected exactly two content directories (old and new), got {}",
            positional.len()
        ));
    }
    let new_dir = positional.pop().unwrap_or_default();
    let old_dir = positional.pop().unwrap_or_default();
    Ok(CliRequest::Run(CliArgs {
        old_dir,
        new_dir,
        md,
        html,
        all,
    }))
}

/// Result of running the command line: exit code plus the text for stdout and stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Run the whole command line (without the program name).
pub fn run_cli(args: &[String]) -> CliOutput {
    let fail = |code: i32, msg: String| CliOutput {
        code,
        stdout: String::new(),
        stderr: format!("etv_content_diff: {msg}\n"),
    };
    let a = match parse_args(args) {
        Err(m) => return fail(2, format!("{m}\n\n{USAGE}").trim_end().to_string()),
        Ok(CliRequest::Help) => {
            return CliOutput {
                code: 0,
                stdout: USAGE.to_string(),
                stderr: String::new(),
            }
        }
        Ok(CliRequest::Run(a)) => a,
    };
    let old = match load_pack_from_dir(Path::new(&a.old_dir)) {
        Ok(p) => p,
        Err(m) => return fail(1, m),
    };
    let new = match load_pack_from_dir(Path::new(&a.new_dir)) {
        Ok(p) => p,
        Err(m) => return fail(1, m),
    };
    let diff = diff_packs(&old, &new, DiffOptions { all: a.all });
    let mut stdout = String::new();
    if a.md.is_none() && a.html.is_none() {
        stdout = render_markdown(&diff);
    }
    if let Some(p) = &a.md {
        if let Err(e) = fs::write(p, render_markdown(&diff)) {
            return fail(1, format!("cannot write {p}: {e}"));
        }
    }
    if let Some(p) = &a.html {
        if let Err(e) = fs::write(p, render_html(&diff)) {
            return fail(1, format!("cannot write {p}: {e}"));
        }
    }
    CliOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}
