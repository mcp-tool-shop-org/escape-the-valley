//! "Says what happened": checks that a narrator line does not contradict its turn, and that an
//! outcome line carries the turn's main fact.
//!
//! The hard-rule checker tests what a line must not do. A line can pass every one of those and
//! still say "No water." on a turn that gained water, or describe weather on the turn the journey
//! ended. These checks test the line against the turn's facts.
//!
//! They are off in the game. The measurement harness switches them on as an experiment arm (see
//! `narrator_experiment`), and can apply them to lines already written, so an earlier run can be
//! re-scored without calling a model.
//!
//! Everything here is a pure function of the line and the facts. The word lists are short and
//! written out below so they can be read and changed in one place.

use serde::{Deserialize, Serialize};

use crate::narrator::{
    death_may_be_stated, lookup_resource_key_for_noun, CheckFailure, CheckReasonCode,
    NarrationContext,
};

/// What the truth checks need to know about a turn and a line's place in it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TruthFacts {
    /// `true` for the outcome line, `false` for the scene line. The main-fact rules (ending,
    /// death, arrival) and the repeated-opening rule apply to the outcome line only.
    pub is_outcome: bool,
    /// Supply keys that stand at zero after the turn.
    pub supplies_at_zero: Vec<String>,
    /// Supply keys that rose on the turn.
    pub supplies_gained: Vec<String>,
    /// The ending grade when this turn ended the run, else empty.
    pub ending: String,
    /// A member died on the turn.
    pub death: bool,
    /// The party arrived at a landmark on the turn.
    pub arrival: bool,
    /// The place the party is at after the turn.
    pub place: String,
    /// None of the party is left alive.
    pub none_alive: bool,
    /// Run rules 1a to 1e (the "says what happened" rules proper).
    pub check_says_what_happened: bool,
    /// The event's own text, for the copying rule (empty when the turn has no event).
    pub event_text: String,
    /// Check that the line does not open with the event's text.
    pub check_event_copy: bool,
    /// The opening of the previous accepted line in the same run (its first three words; see
    /// [`opening_words`]), when the repeated-opening rule is on and there is one.
    pub previous_opening: Option<String>,
    /// How many opening words the repeated-opening rule compares: two (the rule as first
    /// measured; also what 0 means) or three (the `opening-three-words` arm).
    #[serde(default)]
    pub opening_words: usize,
    /// Openings the repeated-opening rule never rejects: the lead's own subjects, as lowercase
    /// words (see [`LEAD_SUBJECTS`]). A line whose first words are one of these is exempt.
    #[serde(default)]
    pub opening_exempt: Vec<String>,
    /// The `scene-rules` arm: the repeated-opening rule also checks the scene line.
    #[serde(default)]
    pub scene_repeat_opening: bool,
    /// The `pyrrhic-loss-word` arm: a pyrrhic ending's outcome line must also carry a word
    /// from [`PYRRHIC_LOSS_WORDS`].
    #[serde(default)]
    pub pyrrhic_loss_word: bool,
    /// The `choice-conveyed` arm, on a turn with a choice: the outcome line carries one of
    /// these crude stems (see [`stem`]; the choice label's first content word), or names a
    /// supply in `choice_supplies`. Both empty = the rule is off.
    #[serde(default)]
    pub choice_stems: Vec<String>,
    /// The supply keys the turn changed (spent or gained), for the `choice-conveyed` rule.
    #[serde(default)]
    pub choice_supplies: Vec<String>,
    /// The `lead-guard` arm: the lead the outcome prompt was given (empty = the guard is off).
    #[serde(default)]
    pub lead: String,
    /// The `lead-guard` arm: how many words of its own the outcome line must add to the lead.
    #[serde(default)]
    pub lead_guard_min: usize,
    /// The `lead-guard` arm: the turn's time of day (`morning`, `afternoon`, `evening`,
    /// `night`); empty = the time-of-day rule is off.
    #[serde(default)]
    pub time_of_day: String,
    /// The `no-anachronism` arm: reject a word from [`ANACHRONISM_WORDS`].
    #[serde(default)]
    pub check_anachronism: bool,
    /// The `taboo-rules` arm: the party's taboo and doctrine as the situation states them
    /// (lowercase; empty = none stated), and the words of the turn's own event and choice, which
    /// may make the taboo part of the turn.
    #[serde(default)]
    pub taboo: String,
    #[serde(default)]
    pub doctrine: String,
    #[serde(default)]
    pub check_taboo: bool,
    /// The turn's action (`travel`, `rest`, ... or empty) and whether it is night.
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub is_night: bool,
    /// The `no-water-source` arm: the water-source words the turn provides (from the place's
    /// name, the biome, the event and the choice). `None` = the rule is off.
    #[serde(default)]
    pub water_allowed: Option<Vec<String>>,
    /// Words of the turn's event title, event text and choice label, lowercase.
    #[serde(default)]
    pub turn_words: Vec<String>,
}

/// Keys of `shown_deltas` that are conditions or measures, not supplies.
const NOT_SUPPLIES: [&str; 6] = [
    "health",
    "morale",
    "wagon_condition",
    "animals_health",
    "time_days",
    "distance",
];

impl TruthFacts {
    /// The facts of `ctx` for its scene or outcome line, with rules 1a to 1e on. The
    /// event-copying and repeated-opening rules are left off; set `check_event_copy` and
    /// `previous_opening` to switch them on.
    pub fn from_context(ctx: &NarrationContext, is_outcome: bool) -> Self {
        let supplies_gained = ctx
            .shown_deltas()
            .iter()
            .filter(|(k, v)| **v > 0 && !NOT_SUPPLIES.contains(&k.as_str()))
            .map(|(k, _)| k.clone())
            .collect();
        Self {
            is_outcome,
            supplies_at_zero: ctx.supplies_at_zero().to_vec(),
            supplies_gained,
            ending: ctx.ending().to_string(),
            death: ctx.flags().death,
            arrival: ctx.flags().landmark_arrival,
            place: ctx.place().to_string(),
            none_alive: ctx.party_status().alive_count() == 0,
            check_says_what_happened: true,
            event_text: ctx.check_event_seed().to_string(),
            check_event_copy: false,
            previous_opening: None,
            opening_words: 2,
            opening_exempt: Vec::new(),
            scene_repeat_opening: false,
            pyrrhic_loss_word: false,
            choice_stems: Vec::new(),
            choice_supplies: Vec::new(),
            lead: String::new(),
            lead_guard_min: 0,
            time_of_day: String::new(),
            check_anachronism: false,
            taboo: String::new(),
            doctrine: String::new(),
            check_taboo: false,
            action: String::new(),
            is_night: false,
            water_allowed: None,
            turn_words: Vec::new(),
        }
    }
}

// ---- word lists --------------------------------------------------------------------------------

/// After a supply noun, these say the supply was lost or used up.
pub const LOSS_WORDS: [&str; 8] = [
    "spent",
    "lost",
    "gone",
    "wasted",
    "drained",
    "empty",
    "dry",
    "exhausted",
];

/// A triumphant ending's line carries one of these.
pub const TRIUMPHANT_WORDS: [&str; 20] = [
    "arrive",
    "arrives",
    "arrived",
    "arrival",
    "reach",
    "reaches",
    "reached",
    "journey ends",
    "journey's end",
    "end of the trail",
    "end of the road",
    "trail ends",
    "road ends",
    "made it",
    "out of the valley",
    "valley lies behind",
    "at last",
    "safe",
    "safety",
    "home",
];

/// A pyrrhic ending's line carries one of these: the arrival, or what it cost.
pub const PYRRHIC_WORDS: [&str; 22] = [
    "arrive",
    "arrives",
    "arrived",
    "arrival",
    "reach",
    "reaches",
    "reached",
    "journey ends",
    "journey's end",
    "end of the trail",
    "end of the road",
    "trail ends",
    "road ends",
    "out of the valley",
    "at last",
    "survivors",
    "those left",
    "what is left",
    "few remain",
    "fewer",
    "graves behind",
    "cost",
];

/// With the `pyrrhic-loss-word` arm a pyrrhic ending's line also carries one of these: what
/// the journey cost. The arrival words of [`PYRRHIC_WORDS`] alone no longer do.
pub const PYRRHIC_LOSS_WORDS: [&str; 26] = [
    "lost",
    "loss",
    "losses",
    "fallen",
    "dead",
    "died",
    "death",
    "deaths",
    "grave",
    "graves",
    "buried",
    "mourn",
    "mourning",
    "mourned",
    "missing",
    "absent",
    "empty places",
    "fewer",
    "survivors",
    "those left",
    "what is left",
    "few remain",
    "not all",
    "cost",
    "costly",
    "behind them",
];

/// Things the trail's world does not have. With the `no-anachronism` arm a line that names one
/// is rejected, in either slot. Whole words only.
pub const ANACHRONISM_WORDS: [&str; 44] = [
    "tire",
    "tires",
    "tyre",
    "tyres",
    "engine",
    "engines",
    "motor",
    "motors",
    "motorway",
    "car",
    "cars",
    "truck",
    "trucks",
    "phone",
    "phones",
    "telephone",
    "radio",
    "radios",
    "electric",
    "electricity",
    "plastic",
    "battery",
    "batteries",
    "headlight",
    "headlights",
    "gasoline",
    "petrol",
    "diesel",
    "asphalt",
    "highway",
    "freeway",
    "pavement",
    "flashlight",
    "computer",
    "screen",
    "screens",
    "internet",
    "airplane",
    "helicopter",
    "siren",
    "sirens",
    "chainsaw",
    "nylon",
    "smartphone",
];

/// Words that name a time of day, by the turn time they belong to. With the `lead-guard` arm a
/// line may name its own turn's time or a neighbouring one (morning and afternoon, afternoon
/// and evening, evening and night), never an opposite one.
pub const TIME_WORDS: [(&str, &[&str]); 4] = [
    (
        "morning",
        &["morning", "dawn", "sunrise", "daybreak", "first light"],
    ),
    ("afternoon", &["afternoon", "noon", "midday"]),
    (
        "evening",
        &[
            "evening",
            "dusk",
            "sunset",
            "twilight",
            "nightfall",
            "sundown",
        ],
    ),
    (
        "night",
        &["night", "midnight", "moonlight", "starlight", "moonlit"],
    ),
];

/// Words that count as adding nothing to the lead: articles, pronouns, linking words and the
/// commonest verbs. Every other word of three letters or more is a word of the line's own.
pub const LEAD_GUARD_STOP_WORDS: [&str; 40] = [
    "the", "and", "its", "his", "her", "their", "they", "them", "this", "that", "with", "from",
    "into", "onto", "upon", "for", "but", "not", "now", "all", "are", "was", "were", "has", "had",
    "have", "been", "being", "who", "which", "where", "there", "here", "then", "than", "out",
    "off", "yet", "too", "very",
];

/// A lost ending's line carries one of these.
pub const LOST_WORDS: [&str; 24] = [
    "no one",
    "none",
    "nobody",
    "the last",
    "journey ends",
    "journey is over",
    "trail ends",
    "road ends",
    "ends here",
    "goes no further",
    "no further",
    "lost",
    "over",
    "abandoned",
    "empty",
    "silent",
    "silence",
    "dead",
    "died",
    "dies",
    "death",
    "perished",
    "grave",
    "graves",
];

/// A death turn's line carries one of these.
pub const DEATH_WORDS: [&str; 22] = [
    "dead",
    "died",
    "dies",
    "dying",
    "death",
    "deaths",
    "grave",
    "graves",
    "buried",
    "burial",
    "perish",
    "perished",
    "perishes",
    "does not rise",
    "does not wake",
    "last breath",
    "breath stops",
    "no longer breathes",
    "laid to rest",
    "one fewer",
    "fewer now",
    "is lost",
];

/// An arrival turn's line carries one of these, or the place's name.
pub const ARRIVAL_WORDS: [&str; 16] = [
    "arrive",
    "arrives",
    "arrived",
    "arrival",
    "reach",
    "reaches",
    "reached",
    "comes to",
    "come to",
    "enters",
    "enter",
    "rolls into",
    "roll into",
    "pulls into",
    "pull into",
    "gates",
];

/// Words for the party or its team as doers. With no one left alive, a sentence that has one of
/// these must also have a word from [`STILLNESS_WORDS`].
pub const ACTOR_WORDS: [&str; 20] = [
    "party",
    "traveller",
    "travellers",
    "traveler",
    "travelers",
    "men",
    "man",
    "women",
    "woman",
    "driver",
    "drivers",
    "they",
    "hands",
    "team",
    "oxen",
    "ox",
    "mules",
    "mule",
    "horses",
    "horse",
];

/// Words that make a sentence about the party one of absence or stillness, not of action.
pub const STILLNESS_WORDS: [&str; 22] = [
    "no",
    "none",
    "nobody",
    "not",
    "without",
    "last",
    "lie",
    "lies",
    "lay",
    "still",
    "dead",
    "died",
    "silent",
    "empty",
    "abandoned",
    "left",
    "remains",
    "gone",
    "stop",
    "stops",
    "stopped",
    "unattended",
];

/// Words in a row from the event's text that a line may not open with.
pub const EVENT_COPY_WORDS: usize = 5;

// ---- helpers -----------------------------------------------------------------------------------

/// Lowercased words of `text`, each trimmed of everything but letters, digits and inner
/// apostrophes or hyphens.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// `true` when `phrase` (one or more words) appears in `tokens` as whole words in a row.
fn has_phrase(tokens: &[String], phrase: &str) -> bool {
    let want = words(phrase);
    if want.is_empty() || want.len() > tokens.len() {
        return false;
    }
    tokens
        .windows(want.len())
        .any(|w| w.iter().zip(&want).all(|(a, b)| a == b))
}

fn has_any(tokens: &[String], list: &[&str]) -> bool {
    list.iter().any(|phrase| has_phrase(tokens, phrase))
}

/// Supply words the hard-rule checker's noun table does not list, because the game's own lines
/// use them freely: the supply keys themselves, and rations.
const PLAIN_SUPPLY_WORDS: [(&str, &str); 14] = [
    ("food", "food"),
    ("rations", "food"),
    ("ration", "food"),
    ("water", "water"),
    ("firewood", "firewood"),
    ("ammo", "ammo"),
    ("ammunition", "ammo"),
    ("medicine", "meds"),
    ("meds", "meds"),
    ("rope", "rope"),
    ("cloth", "cloth"),
    ("tools", "tools"),
    ("parts", "parts"),
    ("oil", "oil"),
];

/// The supply key a word names, if it names one: the checker's noun table first, then the plain
/// supply words.
fn supply_key_of(word: &str) -> Option<&'static str> {
    lookup_resource_key_for_noun(word).or_else(|| {
        PLAIN_SUPPLY_WORDS
            .iter()
            .find(|(w, _)| *w == word)
            .map(|(_, key)| *key)
    })
}

fn supply_key_matches(list: &[String], res_key: &str) -> bool {
    list.iter().any(|k| {
        k == res_key
            || (res_key == "lantern_oil" && k == "oil")
            || (res_key == "oil" && k == "lantern_oil")
    })
}

/// The first two words of a line, lowercased and trimmed of punctuation, joined by a space.
pub fn opening_two_words(line: &str) -> String {
    opening_words(line, 2)
}

/// The first `n` words of a line, lowercased and trimmed of punctuation, joined by a space.
/// The narrator remembers the first three words of each accepted line; the two-word rule
/// compares the first two of those.
pub fn opening_words(line: &str, n: usize) -> String {
    words(line)
        .into_iter()
        .take(n)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The subjects the outcome lead ([`what_happened`]) opens its sentences with, and "the
/// wagon", which the repeated-opening ruling also names. A line that opens with one of these
/// is telling the turn's fact in the lead's own words, so the `opening-three-words` arm does
/// not count it as a repeated opening.
pub const LEAD_SUBJECTS: [&str; 5] = [
    "the party",
    "a traveller",
    "the last",
    "the turn",
    "the wagon",
];

/// `true` when `line` opens with [`EVENT_COPY_WORDS`] or more words that appear in a row in
/// `event_text`.
pub fn opens_with_event_text(line: &str, event_text: &str) -> bool {
    let line_words = words(line);
    let event_words = words(event_text);
    if line_words.len() < EVENT_COPY_WORDS || event_words.len() < EVENT_COPY_WORDS {
        return false;
    }
    let opening = &line_words[..EVENT_COPY_WORDS];
    event_words.windows(EVENT_COPY_WORDS).any(|w| w == opening)
}

fn fail(code: CheckReasonCode, detail: String) -> CheckFailure {
    CheckFailure { code, detail }
}

// ---- the rules ---------------------------------------------------------------------------------

/// Rules 1a and 1b: what the line says about supplies must match the turn.
///
/// - 1a: "no <supply>", "out of <supply>", "<supply> gone", "<supply> is gone" (and "are", "has",
///   "have") only when that supply stands at zero after the turn.
/// - 1b: a supply that rose on the turn is not followed, within two words, by a word from
///   [`LOSS_WORDS`].
pub fn supply_state_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    let tokens = words(line);
    for (i, word) in tokens.iter().enumerate() {
        let Some(key) = supply_key_of(word) else {
            continue;
        };
        let prev = |n: usize| i.checked_sub(n).map(|j| tokens[j].as_str());
        let next = |n: usize| tokens.get(i + n).map(String::as_str);
        let says_none = prev(1) == Some("no")
            || (prev(2) == Some("out") && prev(1) == Some("of"))
            || next(1) == Some("gone")
            || (matches!(next(1), Some("is" | "are" | "has" | "have")) && next(2) == Some("gone"));
        if says_none && !supply_key_matches(&facts.supplies_at_zero, key) {
            return Some(fail(
                CheckReasonCode::SupplyStateContradiction,
                format!("says there is no '{word}', but {key} is not at zero"),
            ));
        }
        if supply_key_matches(&facts.supplies_gained, key) {
            for n in 1..=2 {
                if let Some(after) = next(n) {
                    if LOSS_WORDS.contains(&after) {
                        return Some(fail(
                            CheckReasonCode::SupplyStateContradiction,
                            format!("says '{word} ... {after}', but {key} was gained on this turn"),
                        ));
                    }
                }
            }
        }
    }
    None
}

/// Rules 1c and 1d: the outcome line carries the turn's main fact. An ending comes before a
/// death, which comes before an arrival; only the first that applies is required.
pub fn main_fact_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.is_outcome {
        return None;
    }
    let tokens = words(line);
    match facts.ending.as_str() {
        "triumphant" | "weathered" => {
            return (!has_any(&tokens, &TRIUMPHANT_WORDS)).then(|| {
                fail(
                    CheckReasonCode::EndingNotConveyed,
                    format!(
                        "the journey ended ({}) and the line does not say so",
                        facts.ending
                    ),
                )
            });
        }
        "pyrrhic" => {
            return (!has_any(&tokens, &PYRRHIC_WORDS)).then(|| {
                fail(
                    CheckReasonCode::EndingNotConveyed,
                    "the journey ended (pyrrhic) and the line does not say so".to_string(),
                )
            });
        }
        "lost" => {
            return (!has_any(&tokens, &LOST_WORDS)).then(|| {
                fail(
                    CheckReasonCode::EndingNotConveyed,
                    "the journey ended (lost) and the line does not say so".to_string(),
                )
            });
        }
        _ => {}
    }
    if facts.death {
        return (!has_any(&tokens, &DEATH_WORDS)).then(|| {
            fail(
                CheckReasonCode::DeathNotConveyed,
                "a traveller died on this turn and the line does not say so".to_string(),
            )
        });
    }
    if facts.arrival {
        let names_place = !facts.place.trim().is_empty() && has_phrase(&tokens, &facts.place);
        return (!names_place && !has_any(&tokens, &ARRIVAL_WORDS)).then(|| {
            fail(
                CheckReasonCode::ArrivalNotConveyed,
                format!(
                    "the party arrived at {} and the line does not say so",
                    facts.place
                ),
            )
        });
    }
    None
}

/// Rule 1e: with no one left alive, no sentence shows the party or its team doing anything. A
/// sentence with a word from [`ACTOR_WORDS`] must also have one from [`STILLNESS_WORDS`].
pub fn no_one_left_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.none_alive {
        return None;
    }
    for sentence in line.split('.') {
        let tokens = words(sentence);
        let actor = tokens.iter().find(|t| ACTOR_WORDS.contains(&t.as_str()));
        if let Some(actor) = actor {
            if !tokens.iter().any(|t| STILLNESS_WORDS.contains(&t.as_str())) {
                return Some(fail(
                    CheckReasonCode::ActionWithNoOneLeft,
                    format!("no one is left alive, and the line has '{actor}' acting"),
                ));
            }
        }
    }
    None
}

/// The "says what happened" rules, 1a to 1e, in order. At most one failure per rule.
pub fn says_what_happened_failures(line: &str, facts: &TruthFacts) -> Vec<CheckFailure> {
    [
        supply_state_failure(line, facts),
        main_fact_failure(line, facts),
        no_one_left_failure(line, facts),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The copying rule: the line may not open with the event's own text.
pub fn event_copy_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    (facts.check_event_copy && opens_with_event_text(line, &facts.event_text)).then(|| {
        fail(
            CheckReasonCode::EventTextCopied,
            format!("opens with {EVENT_COPY_WORDS} or more words of the event's own text"),
        )
    })
}

/// The repetition rule: an outcome line may not open with the same words as the previous
/// accepted line of the run (two words, or three with the `opening-three-words` arm, which
/// also exempts a line that opens with one of the lead's subjects).
pub fn repeated_opening_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.is_outcome && !facts.scene_repeat_opening {
        return None;
    }
    let previous = facts.previous_opening.as_deref()?;
    let n = if facts.opening_words == 0 {
        2
    } else {
        facts.opening_words
    };
    let opening = opening_words(line, n);
    let previous = opening_words(previous, n);
    let exempt = facts.opening_exempt.iter().any(|subject| {
        let subject = words(subject);
        let head = words(line);
        !subject.is_empty() && head.len() >= subject.len() && head[..subject.len()] == subject[..]
    });
    (!exempt && !previous.is_empty() && opening == previous).then(|| {
        fail(
            CheckReasonCode::RepeatedOpening,
            format!("opens with '{opening}', as the previous line did"),
        )
    })
}

/// A crude stem for comparing words of a line with words of the lead or a label: lowercase,
/// with a final "ing", "ed", "es", "s", "d" or "e" taken off when three letters or more
/// remain, and a doubled last letter then made single ("travelled" and "travels" both give
/// "travel"; "died" and "dies" both give "die").
pub fn stem(word: &str) -> String {
    let w = word.to_ascii_lowercase();
    for suffix in ["ing", "ed", "es", "s", "d", "e"] {
        if let Some(head) = w.strip_suffix(suffix) {
            if head.len() >= 3 {
                let b = head.as_bytes();
                let n = b.len();
                if n >= 4 && b[n - 1] == b[n - 2] && suffix != "s" && suffix != "es" {
                    return head[..n - 1].to_string();
                }
                return head.to_string();
            }
        }
    }
    w
}

/// The words of a text that count for the lead guard: three letters or more, not a stop word.
fn own_words(text: &str) -> Vec<String> {
    words(text)
        .into_iter()
        .filter(|w| w.len() >= 3 && !LEAD_GUARD_STOP_WORDS.contains(&w.as_str()))
        .collect()
}

/// The words of `line` the lead guard counts as the line's own: by crude stem, not in the
/// lead, each once, in order of first use.
pub fn words_beyond_lead(line: &str, lead: &str) -> Vec<String> {
    let lead: Vec<String> = own_words(lead).iter().map(|w| stem(w)).collect();
    let mut added: Vec<String> = Vec::new();
    for w in own_words(line).iter().map(|w| stem(w)) {
        if !lead.contains(&w) && !added.contains(&w) {
            added.push(w);
        }
    }
    added
}

/// The `pyrrhic-loss-word` rule: a pyrrhic ending's outcome line says what it cost.
pub fn pyrrhic_loss_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.is_outcome || !facts.pyrrhic_loss_word || facts.ending != "pyrrhic" {
        return None;
    }
    (!has_any(&words(line), &PYRRHIC_LOSS_WORDS)).then(|| {
        fail(
            CheckReasonCode::PyrrhicLossNotConveyed,
            "the journey ended at a cost (pyrrhic) and the line does not say what was lost"
                .to_string(),
        )
    })
}

/// The `choice-conveyed` rule: on a choice turn the outcome line carries the choice's verb
/// (compared by crude stem) or names a supply the turn changed.
pub fn choice_conveyed_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.is_outcome || (facts.choice_stems.is_empty() && facts.choice_supplies.is_empty()) {
        return None;
    }
    let tokens = words(line);
    let line_stems: Vec<String> = tokens.iter().map(|w| stem(w)).collect();
    let verb = facts.choice_stems.iter().any(|s| line_stems.contains(s));
    let supply = tokens.iter().any(|w| {
        supply_key_of(w).is_some_and(|key| supply_key_matches(&facts.choice_supplies, key))
    });
    (!verb && !supply).then(|| {
        fail(
            CheckReasonCode::ChoiceNotConveyed,
            format!(
                "the line carries none of the choice's words ({})",
                facts.choice_stems.join(", ")
            ),
        )
    })
}

/// The `lead-guard` rule: the outcome line adds at least `lead_guard_min` words of its own to
/// the lead (see [`words_beyond_lead`]).
pub fn lead_only_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.is_outcome || facts.lead.is_empty() || facts.lead_guard_min == 0 {
        return None;
    }
    let added = words_beyond_lead(line, &facts.lead);
    (added.len() < facts.lead_guard_min).then(|| {
        fail(
            CheckReasonCode::LeadOnly,
            format!(
                "the line adds {} word(s) of its own to the lead; at least {} wanted",
                added.len(),
                facts.lead_guard_min
            ),
        )
    })
}

/// The `lead-guard` time rule: a line names no time of day opposite to the turn's.
pub fn time_of_day_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if facts.time_of_day.is_empty() {
        return None;
    }
    let order = ["morning", "afternoon", "evening", "night"];
    let own = order.iter().position(|t| *t == facts.time_of_day)?;
    let tokens = words(line);
    for (i, (time, list)) in TIME_WORDS.iter().enumerate() {
        if i.abs_diff(own) >= 2 && has_any(&tokens, list) {
            return Some(fail(
                CheckReasonCode::TimeOfDayContradiction,
                format!("names the {time} on a turn in the {}", facts.time_of_day),
            ));
        }
    }
    None
}

/// The `no-anachronism` rule: a line names nothing from [`ANACHRONISM_WORDS`].
pub fn anachronism_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.check_anachronism {
        return None;
    }
    words(line)
        .into_iter()
        .find(|w| ANACHRONISM_WORDS.contains(&w.as_str()))
        .map(|w| fail(CheckReasonCode::Anachronism, format!("names '{w}'")))
}

/// Water sources a line may name only when the turn provides one.
pub const WATER_SOURCE_WORDS: [&str; 18] = [
    "river",
    "rivers",
    "riverbank",
    "riverbed",
    "stream",
    "streams",
    "creek",
    "creeks",
    "brook",
    "brooks",
    "lake",
    "lakes",
    "pond",
    "ponds",
    "ford",
    "fords",
    "waterfall",
    "rapids",
];

/// Water sources a biome provides of itself: a swamp has standing water.
pub fn biome_water_words(biome: &str) -> &'static [&'static str] {
    match biome {
        "swamp" => &["pond", "ponds"],
        _ => &[],
    }
}

/// The water-source words a place's name provides ("Ironbrook" gives brook and brooks;
/// "Stillwater Crossing" and "Broken Bridge" give river and ford, as a crossing or a bridge
/// is over water).
pub fn place_water_words(place: &str) -> Vec<String> {
    let p = place.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    for w in WATER_SOURCE_WORDS {
        let stem = w.trim_end_matches('s');
        if p.contains(stem) {
            out.push(w.to_string());
        }
    }
    if p.contains("crossing")
        || p.contains("bridge")
        || p.contains("water")
        || p.contains("landing")
    {
        for w in ["river", "rivers", "riverbank", "ford", "fords", "stream"] {
            out.push(w.to_string());
        }
    }
    out
}

/// Drinking or drawing water: with a river word in the same sentence, it breaks "no river
/// water".
pub const DRINK_WORDS: [&str; 14] = [
    "drink", "drinks", "drank", "drinking", "sip", "sips", "sipped", "fill", "fills", "filled",
    "scoop", "scoops", "scooped", "cupped",
];
/// A river's water, for the "no river water" taboo.
pub const RIVER_WORDS: [&str; 8] = [
    "river",
    "rivers",
    "riverbank",
    "stream",
    "streams",
    "creek",
    "brook",
    "ford",
];
/// Travel words, for the "no travel at night" taboo.
pub const TRAVEL_WORDS: [&str; 14] = [
    "travel",
    "travels",
    "travelling",
    "rolls on",
    "roll on",
    "push on",
    "pushes on",
    "presses on",
    "press on",
    "moves on",
    "move on",
    "onward",
    "drive on",
    "drives on",
];
/// Leaving things, for the "leave nothing behind" taboo.
pub const LEAVE_WORDS: [&str; 10] = [
    "left behind",
    "leave behind",
    "leaves behind",
    "abandon",
    "abandons",
    "abandoned",
    "discard",
    "discards",
    "discarded",
    "cast off",
];
/// Owing, for the "no debts" doctrine.
pub const DEBT_WORDS: [&str; 10] = [
    "debt", "debts", "owe", "owes", "owed", "borrow", "borrows", "borrowed", "loan", "credit",
];
/// Carelessness, for the "careful hands" doctrine.
pub const CARELESS_WORDS: [&str; 8] = [
    "careless",
    "carelessly",
    "reckless",
    "recklessly",
    "clumsy",
    "clumsily",
    "fumble",
    "fumbles",
];

/// The `taboo-rules` rule (a): no sentence shows the party doing what its taboo forbids, or
/// going against its doctrine. The game's own action is allowed: leaving cargo on an
/// `abandon_cargo` turn, travelling on a night `travel` turn.
pub fn taboo_broken_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.check_taboo {
        return None;
    }
    for sentence in line.split('.') {
        let t = words(sentence);
        let broken = match facts.taboo.as_str() {
            // Read over the whole line: the river may be named in one sentence and the
            // drinking in the next.
            "no river water" => {
                let all = words(line);
                has_any(&all, &RIVER_WORDS) && has_any(&all, &DRINK_WORDS)
            }
            "no travel at night" => {
                facts.is_night && facts.action != "travel" && has_any(&t, &TRAVEL_WORDS)
            }
            "leave nothing behind" => facts.action != "abandon_cargo" && has_any(&t, &LEAVE_WORDS),
            _ => false,
        };
        if broken {
            return Some(fail(
                CheckReasonCode::TabooBroken,
                format!("shows the party breaking its taboo ({})", facts.taboo),
            ));
        }
        let against = match facts.doctrine.as_str() {
            "no debts" => {
                has_any(&t, &DEBT_WORDS)
                    && !facts
                        .turn_words
                        .iter()
                        .any(|w| DEBT_WORDS.contains(&w.as_str()))
            }
            "careful hands" => has_any(&t, &CARELESS_WORDS),
            _ => false,
        };
        if against {
            return Some(fail(
                CheckReasonCode::TabooBroken,
                format!(
                    "shows the party going against its doctrine ({})",
                    facts.doctrine
                ),
            ));
        }
    }
    None
}

/// The `taboo-rules` rule (c): the taboo is not stated as filler. A line may name it only when
/// the turn's event or choice touches it (a river, the night, something left).
pub fn taboo_filler_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.check_taboo || facts.taboo.is_empty() {
        return None;
    }
    let (phrases, touches): (&[&str], &[&str]) = match facts.taboo.as_str() {
        "no river water" => (
            &["river water"],
            &["river", "water", "stream", "ford", "drink", "well"],
        ),
        "no travel at night" => (
            &["travel at night", "night travel", "travel by night"],
            &["night", "dark"],
        ),
        "leave nothing behind" => (
            &["nothing behind", "leave nothing"],
            &["leave", "left", "behind", "abandon"],
        ),
        _ => return None,
    };
    let t = words(line);
    if !has_any(&t, phrases) {
        return None;
    }
    let involved = facts
        .turn_words
        .iter()
        .any(|w| touches.contains(&w.as_str()));
    (!involved).then(|| {
        fail(
            CheckReasonCode::TabooAsFiller,
            format!(
                "states the taboo ({}) on a turn that does not involve it",
                facts.taboo
            ),
        )
    })
}

/// The `no-water-source` rule (b): a line names no river, stream, lake or ford the turn does not
/// provide.
pub fn water_source_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    let allowed = facts.water_allowed.as_ref()?;
    words(line)
        .into_iter()
        .find(|w| WATER_SOURCE_WORDS.contains(&w.as_str()) && !allowed.contains(w))
        .map(|w| {
            fail(
                CheckReasonCode::InventedWaterSource,
                format!("names '{w}', which the place and the turn do not provide"),
            )
        })
}

/// Every truth failure for a line, in order: rules 1a to 1e, then event copying, then the
/// repeated opening, then the narrator-fixes rules (each off unless its arm set it).
pub fn truth_failures(line: &str, facts: &TruthFacts) -> Vec<CheckFailure> {
    let mut out = if facts.check_says_what_happened {
        says_what_happened_failures(line, facts)
    } else {
        Vec::new()
    };
    out.extend(event_copy_failure(line, facts));
    out.extend(repeated_opening_failure(line, facts));
    out.extend(pyrrhic_loss_failure(line, facts));
    out.extend(choice_conveyed_failure(line, facts));
    out.extend(lead_only_failure(line, facts));
    out.extend(time_of_day_failure(line, facts));
    out.extend(anachronism_failure(line, facts));
    out.extend(taboo_broken_failure(line, facts));
    out.extend(taboo_filler_failure(line, facts));
    out.extend(water_source_failure(line, facts));
    out
}

// ---- the outcome line's lead -------------------------------------------------------------------

/// How the outcome lead is worded. The default is the lead as first measured (past tense, the
/// choice given by its label).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LeadStyle {
    /// The `present-lead` arm: the lead in the present tense ("A traveller dies.").
    pub present: bool,
    /// The `choice-result-lead` arm: on a choice turn the lead says what the choice did (the
    /// supplies it cost or brought, and any hurt), not the choice's label.
    pub choice_result: bool,
}

/// Supply keys a turn changed, spent first, then gained, each in key order.
pub fn changed_supplies(ctx: &NarrationContext) -> (Vec<String>, Vec<String>) {
    let mut spent = Vec::new();
    let mut gained = Vec::new();
    for (key, delta) in ctx.shown_deltas() {
        if NOT_SUPPLIES.contains(&key.as_str()) {
            continue;
        }
        if *delta < 0 {
            spent.push(key.clone());
        } else if *delta > 0 {
            gained.push(key.clone());
        }
    }
    (spent, gained)
}

/// Supply keys as words for the lead: "lantern_oil" becomes "lantern oil", "ammo" becomes
/// "ammunition"; joined "a", "a and b", "a, b and c".
fn supply_list(keys: &[String]) -> String {
    let names: Vec<String> = keys
        .iter()
        .map(|k| match k.as_str() {
            "ammo" => "ammunition".to_string(),
            other => other.replace('_', " "),
        })
        .collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        n => format!("{} and {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

/// The lead of a choice turn in the `choice-result-lead` arm: what the choice did.
fn choice_result_lead(ctx: &NarrationContext, present: bool) -> String {
    let (spent, gained) = changed_supplies(ctx);
    let (cost, brought, nothing, hurt) = if present {
        (
            "costs",
            "brings",
            "The choice costs the party nothing it carries.",
            "A traveller is hurt.",
        )
    } else {
        (
            "cost",
            "brought",
            "The choice cost the party nothing it carried.",
            "A traveller was hurt.",
        )
    };
    let mut out = match (spent.is_empty(), gained.is_empty()) {
        (true, true) => nothing.to_string(),
        (false, true) => format!("The choice {cost} the party {}.", supply_list(&spent)),
        (true, false) => format!("The choice {brought} the party {}.", supply_list(&gained)),
        (false, false) => format!(
            "The choice {cost} the party {} and {brought} it {}.",
            supply_list(&spent),
            supply_list(&gained)
        ),
    };
    if ctx.flags().injury {
        out.push(' ');
        out.push_str(hurt);
    }
    out
}

/// The one main fact of a turn, stated plainly, for the head of the outcome prompt. An ending
/// comes before a death, which comes before an arrival, which comes before the choice taken,
/// which comes before the camp action.
pub fn what_happened(ctx: &NarrationContext) -> String {
    what_happened_in(ctx, LeadStyle::default())
}

/// [`what_happened`] worded as `style` asks.
pub fn what_happened_in(ctx: &NarrationContext, style: LeadStyle) -> String {
    if !style.present {
        if style.choice_result
            && ctx.ending().is_empty()
            && !(ctx.flags().death && death_may_be_stated(ctx))
            && !ctx.flags().landmark_arrival
            && !ctx.check_choice_label().is_empty()
        {
            return choice_result_lead(ctx, false);
        }
        return what_happened_past(ctx);
    }
    let place = ctx.place();
    match ctx.ending() {
        "triumphant" | "weathered" => {
            return format!(
                "The party reaches {place} and the end of the trail. The journey is over."
            );
        }
        "pyrrhic" => {
            return format!(
                "The party reaches {place} and the end of the trail, with some of its number lost on the way. The journey is over."
            );
        }
        "lost" => return "The last of the party dies. The journey ends here.".to_string(),
        _ => {}
    }
    if ctx.flags().death && death_may_be_stated(ctx) {
        return "A traveller dies.".to_string();
    }
    if ctx.flags().landmark_arrival {
        return format!("The party arrives at {place}.");
    }
    if !ctx.check_choice_label().is_empty() {
        if style.choice_result {
            return choice_result_lead(ctx, true);
        }
        return format!(
            "The party chooses: {}.",
            ctx.check_choice_label().trim_end_matches('.')
        );
    }
    let gained = |key: &str| ctx.shown_deltas().get(key).is_some_and(|d| *d > 0);
    match ctx.action() {
        "travel" => "The party travels on.".to_string(),
        "rest" => "The party rests.".to_string(),
        "hunt" => {
            if gained("food") {
                "The party hunts and brings back food.".to_string()
            } else {
                "The party hunts and brings back nothing.".to_string()
            }
        }
        "repair" => "The party repairs the wagon.".to_string(),
        "desperate_repair" => "The party makes a desperate repair to the wagon.".to_string(),
        "abandon_cargo" => "The party leaves cargo behind to lighten the wagon.".to_string(),
        "hard_ration" => "The party cuts its rations.".to_string(),
        "" => "The turn passes.".to_string(),
        other => format!("The party chooses to {other}."),
    }
}

/// The lead as first measured: past tense, the choice given by its label.
fn what_happened_past(ctx: &NarrationContext) -> String {
    match ctx.ending() {
        "triumphant" | "weathered" => {
            return format!(
                "The party reached {} and the end of the trail. The journey is over.",
                ctx.place()
            );
        }
        "pyrrhic" => {
            return format!(
                "The party reached {} and the end of the trail, with some of its number lost on the way. The journey is over.",
                ctx.place()
            );
        }
        "lost" => {
            return "The last of the party has died. The journey ends here.".to_string();
        }
        _ => {}
    }
    if ctx.flags().death && death_may_be_stated(ctx) {
        return "A traveller died.".to_string();
    }
    if ctx.flags().landmark_arrival {
        return format!("The party arrived at {}.", ctx.place());
    }
    if !ctx.check_choice_label().is_empty() {
        return format!(
            "The party chose: {}.",
            ctx.check_choice_label().trim_end_matches('.')
        );
    }
    let gained = |key: &str| ctx.shown_deltas().get(key).is_some_and(|d| *d > 0);
    match ctx.action() {
        "travel" => "The party travelled on.".to_string(),
        "rest" => "The party rested.".to_string(),
        "hunt" => {
            if gained("food") {
                "The party hunted and brought back food.".to_string()
            } else {
                "The party hunted and brought back nothing.".to_string()
            }
        }
        "repair" => "The party repaired the wagon.".to_string(),
        "desperate_repair" => "The party made a desperate repair to the wagon.".to_string(),
        "abandon_cargo" => "The party left cargo behind to lighten the wagon.".to_string(),
        "hard_ration" => "The party cut its rations.".to_string(),
        "" => "The turn passed.".to_string(),
        other => format!("The party chose to {other}."),
    }
}
