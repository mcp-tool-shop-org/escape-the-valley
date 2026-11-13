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
    /// The first two words of the previous accepted line in the same run, when the
    /// repeated-opening rule is on and there is one.
    pub previous_opening: Option<String>,
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
    words(line)
        .into_iter()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

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

/// The repetition rule: an outcome line may not open with the same two words as the previous
/// accepted line of the run.
pub fn repeated_opening_failure(line: &str, facts: &TruthFacts) -> Option<CheckFailure> {
    if !facts.is_outcome {
        return None;
    }
    let previous = facts.previous_opening.as_deref()?;
    let opening = opening_two_words(line);
    (!previous.is_empty() && opening == previous).then(|| {
        fail(
            CheckReasonCode::RepeatedOpening,
            format!("opens with '{opening}', as the previous line did"),
        )
    })
}

/// Every truth failure for a line, in order: rules 1a to 1e, then event copying, then the
/// repeated opening.
pub fn truth_failures(line: &str, facts: &TruthFacts) -> Vec<CheckFailure> {
    let mut out = if facts.check_says_what_happened {
        says_what_happened_failures(line, facts)
    } else {
        Vec::new()
    };
    out.extend(event_copy_failure(line, facts));
    out.extend(repeated_opening_failure(line, facts));
    out
}

// ---- the outcome line's lead -------------------------------------------------------------------

/// The one main fact of a turn, stated plainly, for the head of the outcome prompt. An ending
/// comes before a death, which comes before an arrival, which comes before the choice taken,
/// which comes before the camp action.
pub fn what_happened(ctx: &NarrationContext) -> String {
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
