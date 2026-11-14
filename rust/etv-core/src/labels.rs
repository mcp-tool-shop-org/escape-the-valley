//! The one table of player-facing labels for the engine's keys.
//!
//! The engine names supplies and the changes a turn shows by keys (`lantern_oil`, `time_days`,
//! `wagon_condition`). Keys stay internal: saves, turn facts, phrase tables and the Trail
//! Ledger's frozen form all keep them. Every line the player reads names them through this
//! table instead, so a key never reaches the screen.

/// `(key, label)` for every key the engine names: the twelve supplies, the other changes a turn
/// can show, and `oil`, the phrase table's short name for lantern oil.
pub const LABELS: [(&str, &str); 19] = [
    ("food", "Food"),
    ("water", "Water"),
    ("firewood", "Firewood"),
    ("meds", "Medicine"),
    ("ammo", "Ammunition"),
    ("cloth", "Cloth"),
    ("rope", "Rope"),
    ("lantern_oil", "Lantern oil"),
    ("oil", "Lantern oil"),
    ("tools", "Tools"),
    ("parts", "Parts"),
    ("boots", "Boots"),
    ("salt", "Salt"),
    ("health", "Health"),
    ("animals_health", "Animals' health"),
    ("wagon_condition", "Wagon condition"),
    ("morale", "Morale"),
    ("distance", "Distance"),
    ("time_days", "Days"),
];

/// The label for `key`, as it starts a line or a list item: `"Lantern oil"`.
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

/// The label for `key` inside a sentence: `"lantern oil"` (`"The lantern oil ran out first."`).
/// Only the first letter is lowered, so `"Animals' health"` reads `"animals' health"`.
pub fn label_in_sentence(key: &str) -> String {
    let l = label(key);
    let mut chars = l.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `text` with every key of the table that has an underscore in it (`lantern_oil`) replaced by
/// its label, as a whole word, in the case it was written in: `"ran out of lantern_oil"` reads
/// `"ran out of lantern oil"`, and `"No Lantern_oil Left"` reads `"No Lantern oil Left"`.
///
/// For prose stored with keys in it, such as a remembered card's text, which keeps its saved
/// form and is relabelled only when shown.
pub fn relabel_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.contains('_') {
            let lower = word.to_lowercase();
            if LABELS.iter().any(|(k, _)| *k == lower) {
                let capital = word.chars().next().is_some_and(char::is_uppercase);
                out.push_str(&if capital {
                    label(&lower)
                } else {
                    label_in_sentence(&lower)
                });
                word.clear();
                return;
            }
        }
        out.push_str(word);
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}
