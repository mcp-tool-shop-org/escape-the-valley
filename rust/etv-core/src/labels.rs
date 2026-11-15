//! The one table of player-facing labels for the engine's keys.
//!
//! The engine names supplies and the changes a turn shows by keys (`lantern_oil`, `time_days`,
//! `wagon_condition`). Keys stay internal: saves, turn facts, phrase tables and the Trail
//! Ledger's frozen form all keep them. Every line the player reads names them through this
//! table instead, so a key never reaches the screen.

/// `(key, label)` for every key the engine names: the twelve supplies, the other changes a turn
/// can show, and `oil`, the phrase table's short name for lantern oil.
///
/// A supply's label uses the approved fallback phrase table's word for it, so the panel and
/// the narrator's fallback line agree ("Lamp oil" beside "uses lamp oil"); a test holds them
/// together.
///
/// **Adding or changing an entry rewrites prose, narrator text included.** `play_turn` runs
/// [`relabel_text`] over every line it shows, the live narrator's scene and outcome among them,
/// so any key here whose label is a different word (other than [`PHRASE_ONLY_KEYS`]) is
/// replaced wherever it stands alone as a word in that text. An entry like `("rope", "Cord")`
/// would turn a narrator's "the rope frays" into "the cord frays". Choose a key's label with
/// that in mind, and keep a new key that is an ordinary English word out of the prose pass by
/// adding it to [`PHRASE_ONLY_KEYS`].
pub const LABELS: [(&str, &str); 19] = [
    ("food", "Food"),
    ("water", "Water"),
    ("firewood", "Firewood"),
    ("meds", "Medicine"),
    ("ammo", "Ammunition"),
    ("cloth", "Cloth"),
    ("rope", "Rope"),
    ("lantern_oil", "Lamp oil"),
    ("oil", "Lamp oil"),
    ("tools", "Tools"),
    ("parts", "Spare parts"),
    ("boots", "Boots"),
    ("salt", "Salt"),
    ("health", "Health"),
    ("animals_health", "Animals' health"),
    ("wagon_condition", "Wagon condition"),
    ("morale", "Morale"),
    ("distance", "Distance"),
    ("time_days", "Days"),
];

/// The label for `key`, as it starts a line or a list item: `"Lamp oil"`.
///
/// A key the table does not hold still reads as words, never as a key: its underscores become
/// spaces and its first letter is capitalised (`"some_new_key"` reads `"Some new key"`).
pub fn label(key: &str) -> String {
    if let Some((_, l)) = LABELS.iter().find(|(k, _)| *k == key) {
        return (*l).to_string();
    }
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The label for `key` inside a sentence: `"lamp oil"` (`"The lamp oil ran out first."`).
/// Only the first letter is lowered, so `"Animals' health"` reads `"animals' health"`.
pub fn label_in_sentence(key: &str) -> String {
    let l = label(key);
    let mut chars = l.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Keys that [`relabel_text`] never rewrites in prose, because each is also an ordinary English
/// word: `oil`, the phrase table's short name for lantern oil ("uses lamp oil"), and `parts`
/// ("in these parts", "the fog parts at dawn"). Wherever a supply is named (the supplies panel,
/// the supply line, a turn's changes, the cannot-afford tag, the player's ledger) its label is
/// used directly, "Lamp oil" and "Spare parts" included; free text is left as written, apart
/// from a signed change such as "-1 parts" (see [`relabel_text`]).
pub const PHRASE_ONLY_KEYS: [&str; 2] = ["oil", "parts"];

/// Whether [`relabel_text`] renames `word` (written in lowercase) when it stands alone: a key of
/// the table whose label is a different word (`ammo`, `meds`, `lantern_oil`, `time_days`, ...).
/// A key that is its own label (`food`) needs no renaming.
fn renamed_in_prose(word: &str) -> bool {
    !PHRASE_ONLY_KEYS.contains(&word)
        && LABELS.iter().any(|(k, _)| *k == word)
        && label(word).to_lowercase() != word
}

/// Whether a single quote opened before position `at` is still open there, on the same line.
///
/// One small state machine, scanning the line up to `at`. For each `'`:
/// - between two word characters it is an apostrophe, and nothing changes (`don't`, `AMMO'S`);
/// - while a quote is open, it closes the quote when the next character is not a word
///   character, whatever came before it (`'wait!'`, `'-'`, `''`, `'No;'`); otherwise nothing
///   changes;
/// - while no quote is open, it opens one when the character before it is not a word character
///   (or it starts the line) and the character after it exists and is not whitespace (`'meds'`,
///   `'-1 meds'`, `Wait...'meds'`); otherwise nothing changes (a trailing possessive such as
///   `drivers'`). Two exceptions open nothing: a `'s` with no word character after it, a
///   possessive (`Jr.'s`, `Dr.'s`, `(the scout)'s`, `"Go."'s`); and a `'` after `.` `,` `!` or
///   `?` with no word character after it (`the end.')`), a closing quote.
///
/// Known limits, left as they are: a word-initial elision (`'tis`, `'em`) reads as an opening
/// quote; curly quotes are not quotes here; nested quotes are not tracked; a plural possessive
/// inside a quoted span reads as the span's closing quote; a quote opened on an earlier line is
/// not seen; and an open quote is not closed by a `'` that a word character follows
/// (`'no meds'x`).
fn inside_single_quotes(chars: &[char], at: usize) -> bool {
    let line_start = chars[..at]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |p| p + 1);
    let word_char = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    let mut open = false;
    for k in line_start..at {
        if chars[k] != '\'' {
            continue;
        }
        let before = k.checked_sub(1).and_then(|b| chars.get(b));
        let after = chars.get(k + 1);
        if word_char(before) && word_char(after) {
            continue;
        }
        if open {
            if !word_char(after) {
                open = false;
            }
        } else if !word_char(before) && after.is_some_and(|c| !c.is_whitespace()) {
            let clause_mark = before.is_some_and(|c| matches!(c, ',' | '.' | '!' | '?'));
            let possessive_s = matches!(after, Some('s' | 'S')) && !word_char(chars.get(k + 2));
            if !possessive_s && !(clause_mark && !word_char(after)) {
                open = true;
            }
        }
    }
    open
}

/// `text` with every key that [`renamed_in_prose`] names replaced by its label, as a whole word,
/// in the case it was written in: `"-1 ammo"` reads `"-1 ammunition"`, `"ran out of
/// lantern_oil"` reads `"ran out of lamp oil"`, and `"No Lantern_oil Left"` reads `"No Lamp oil
/// Left"`. Any case of the key is caught (`"AMMO"`, `"Meds"`); the label keeps a capital first
/// letter if the key had one, and is written in capitals if the key was (`"AMMO'S GONE"` reads
/// `"AMMUNITION'S GONE"`). The words in [`PHRASE_ONLY_KEYS`] are left as written, except right
/// after a signed number, where they name a supply change: `"-1 parts"` reads `"-1 spare
/// parts"`, while `"3 parts water"` and `"in these parts"` stay.
///
/// A plural key's possessive follows the label: `"the meds' cost"` reads `"the medicine's
/// cost"`, since "medicine" is not a plural. A key that is not plural keeps its apostrophe as
/// it was, and so does a key inside a single-quoted span on its line: `"'the meds'"` reads
/// `"'the medicine'"`, the span's closing quote and not a possessive.
///
/// For text the player reads that was written with keys in it: the engine's own lines, event
/// text and choice hints from the content pack, a remembered card's saved text, and the
/// narrator's own lines (see [`LABELS`]). Each keeps its stored form and is relabelled only when
/// shown.
pub fn relabel_text(text: &str) -> String {
    relabel_chars(&text.chars().collect::<Vec<char>>())
}

/// Whether the word starting at `start` comes right after a signed number and one space:
/// `+2 parts`, `-1 parts`, at the start of the text or after a character that is not part of a
/// word and is not itself `+` or `-` (`--3 parts water` stays).
fn after_signed_number(chars: &[char], start: usize) -> bool {
    let Some(space) = start.checked_sub(1) else {
        return false;
    };
    if chars[space] != ' ' {
        return false;
    }
    let mut k = space;
    while k > 0 && chars[k - 1].is_ascii_digit() {
        k -= 1;
    }
    if k == space || k == 0 || !matches!(chars[k - 1], '+' | '-') {
        return false;
    }
    let sign = k - 1;
    // The sign starts the text or follows a character that is neither part of a word nor
    // another sign (`--3 parts water` is a dash, not a change).
    sign == 0
        || !(chars[sign - 1].is_alphanumeric()
            || chars[sign - 1] == '_'
            || matches!(chars[sign - 1], '+' | '-'))
}

fn relabel_chars(chars: &[char]) -> String {
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if !(chars[i].is_alphanumeric() || chars[i] == '_') {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let lower = word.to_lowercase();
        // A word in PHRASE_ONLY_KEYS is prose, except right after a signed number, where it is a
        // supply change ("-1 parts" reads "-1 spare parts"; "3 parts water" stays).
        let change = PHRASE_ONLY_KEYS.contains(&lower.as_str())
            && label(&lower).to_lowercase() != lower
            && after_signed_number(chars, start);
        if !renamed_in_prose(&lower) && !change {
            out.push_str(&word);
            continue;
        }
        let capital = word.chars().next().is_some_and(char::is_uppercase);
        // All capitals (two letters or more, none lower case): the label in capitals too.
        let all_caps = word.chars().filter(|c| c.is_alphabetic()).count() > 1
            && !word.chars().any(char::is_lowercase);
        let shown = if all_caps {
            label(&lower).to_uppercase()
        } else if capital {
            label(&lower)
        } else {
            label_in_sentence(&lower)
        };
        out.push_str(&shown);
        // "meds'" (the plural possessive, an apostrophe not followed by a letter) reads
        // "medicine's" when the label is not a plural. Inside a single-quoted span ("'the
        // meds'"), the apostrophe closes the span and stays a quote.
        let plural_possessive = !inside_single_quotes(chars, start)
            && chars.get(i) == Some(&'\'')
            && !chars.get(i + 1).is_some_and(|c| c.is_alphanumeric())
            && lower.ends_with('s')
            && !shown.to_lowercase().ends_with('s');
        if plural_possessive {
            out.push_str(if all_caps { "'S" } else { "'s" });
            i += 1;
        }
    }
    out
}
