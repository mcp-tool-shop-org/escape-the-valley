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

/// The phrase table's short name for lantern oil. It names no supply of its own and the engine
/// never writes it in a line, while the word "oil" is ordinary prose ("uses lamp oil"), so
/// [`relabel_text`] leaves it alone.
pub const PHRASE_ONLY_KEYS: [&str; 1] = ["oil"];

/// Whether [`relabel_text`] renames `word` (written in lowercase) when it stands alone: a key of
/// the table whose label is a different word (`ammo`, `meds`, `lantern_oil`, `time_days`, ...).
/// A key that is its own label (`food`) needs no renaming.
fn renamed_in_prose(word: &str) -> bool {
    !PHRASE_ONLY_KEYS.contains(&word)
        && LABELS.iter().any(|(k, _)| *k == word)
        && label(word).to_lowercase() != word
}

/// Whether the text before a word (`before`) already ends with the words that open `key`'s
/// label, so the word completes the label: "spare " before "parts". Case is ignored.
fn label_already_written(before: &[char], key: &str) -> bool {
    let shown = label(key).to_lowercase();
    let Some(opening) = shown.strip_suffix(key).filter(|o| !o.is_empty()) else {
        return false;
    };
    let opening: Vec<char> = opening.chars().collect();
    let n = opening.len();
    if before.len() < n {
        return false;
    }
    let tail = &before[before.len() - n..];
    let same = tail
        .iter()
        .zip(&opening)
        .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()));
    // The opening must start a word of its own ("spare", not "unspare").
    let starts_a_word = before.len() == n || {
        let c = before[before.len() - n - 1];
        !(c.is_alphanumeric() || c == '_')
    };
    same && starts_a_word
}

/// `text` with every key that [`renamed_in_prose`] names replaced by its label, as a whole word,
/// in the case it was written in: `"-1 ammo"` reads `"-1 ammunition"`, `"ran out of
/// lantern_oil"` reads `"ran out of lamp oil"`, and `"No Lantern_oil Left"` reads `"No Lamp oil
/// Left"`. Any case of the key is caught (`"AMMO"`, `"Meds"`); the label keeps a capital first
/// letter if the key had one, and is written in capitals if the key was (`"AMMO'S GONE"` reads
/// `"AMMUNITION'S GONE"`).
///
/// A plural key's possessive follows the label: `"the meds' cost"` reads `"the medicine's
/// cost"`, since "medicine" is not a plural. A key that is not plural keeps its apostrophe as
/// it was, and so does a key opened by a quote mark: `"'meds'"` reads `"'medicine'"`, a quoted
/// word and not a possessive.
///
/// A key whose label ends with the key itself ("Spare parts" for `parts`) is left alone where
/// the text already writes the whole label: "uses spare parts" stays as it is, while "No parts
/// for repairs" reads "No spare parts for repairs".
///
/// For text the player reads that was written with keys in it: the engine's own lines, event
/// text and choice hints from the content pack, a remembered card's saved text, and the
/// narrator's own lines (see [`LABELS`]). Each keeps its stored form and is relabelled only when
/// shown.
pub fn relabel_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
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
        if !renamed_in_prose(&lower) || label_already_written(&chars[..start], &lower) {
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
        // "medicine's" when the label is not a plural. A word opened by a quote mark ("'meds'")
        // is quoted, and its closing quote stays a quote.
        let opened_by_quote = start > 0 && chars[start - 1] == '\'';
        let plural_possessive = !opened_by_quote
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
