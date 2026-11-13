//! Experiment arms for the narrator, all off by default.
//!
//! The game never turns any of these on. The measurement harness does, one arm at a time, to
//! compare each against a baseline run on the same turns. An arm that earns its place is then
//! made the shipped behaviour in a change of its own; until then this file is the whole of it.
//!
//! The arms:
//! - **rulings prompt**: the prompt gives a sentence count and a word target per length where
//!   it gave a word cap, drops the "allowed quantifiers" line, tells the Fireside voice that the
//!   wagon is drawn by oxen and that it speaks of the party, and words the party's losses
//!   without the word "one". The checker's caps and rules are unchanged, except that the
//!   Fireside voice may then say "oxen".
//! - **brief by stop**: a Brief line is asked for as plain text and generation stops at the
//!   first full stop; the period is put back in code. Standard and Rich keep the schema, with
//!   the sentence array limited to two and three items.
//! - **model-card sampling**: temperature 0.7, top-p 0.8, top-k 20, as the model's card
//!   recommends, where the shipped call sends temperature 0.15.
//! - **candidates**: up to three calls per line, one after another, each with its own seed
//!   derived from the turn's seed; the first accepted line is used.
//! - **trim to cap**: a line over its word cap is cut back to its leading whole sentences that
//!   fit, then checked.
//!
//! Everything here is a pure function of its inputs. The same turn, arm and seed give the same
//! request, and the same answers give the same line.

use serde::{Deserialize, Serialize};

use crate::model::GMProfile;
use crate::narrator::NarratorLength;

/// Most candidates one line may be given.
pub const MAX_CANDIDATES: u8 = 3;

/// Added to the turn's seed for the first, second and third candidate. The first candidate
/// uses the turn's own seed, so with one candidate the request is the shipped one.
pub const CANDIDATE_SEED_OFFSETS: [u32; MAX_CANDIDATES as usize] = [0, 7_919, 104_729];

/// Sampling settings recommended on the model card of Qwen3-4B-Instruct-2507, which Ollama's
/// package of the model also ships as its defaults.
pub const MODEL_CARD_TEMPERATURE: f64 = 0.7;
pub const MODEL_CARD_TOP_P: f64 = 0.8;
pub const MODEL_CARD_TOP_K: u32 = 20;

/// Which experiment arms are on. The default is all off: the shipped narrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentArms {
    pub rulings_prompt: bool,
    pub brief_by_stop: bool,
    pub model_card_sampling: bool,
    /// Calls a line may be given, `1..=MAX_CANDIDATES`. One is the shipped behaviour.
    pub candidates: u8,
    pub trim_to_cap: bool,
}

impl Default for ExperimentArms {
    fn default() -> Self {
        Self {
            rulings_prompt: false,
            brief_by_stop: false,
            model_card_sampling: false,
            candidates: 1,
            trim_to_cap: false,
        }
    }
}

impl ExperimentArms {
    /// `true` when nothing is switched on.
    pub fn is_shipped(&self) -> bool {
        *self == Self::default()
    }

    /// A short name for run records: the arms that are on, joined by `+`, or `shipped`.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.rulings_prompt {
            parts.push("rulings-prompt".to_string());
        }
        if self.brief_by_stop {
            parts.push("brief-by-stop".to_string());
        }
        if self.model_card_sampling {
            parts.push("model-card-sampling".to_string());
        }
        if self.candidates > 1 {
            parts.push(format!("candidates-{}", self.candidates));
        }
        if self.trim_to_cap {
            parts.push("trim-to-cap".to_string());
        }
        if parts.is_empty() {
            "shipped".to_string()
        } else {
            parts.join("+")
        }
    }
}

/// The seed for candidate `k` (0-based) of a line whose own seed is `base`. Kept to 31 bits,
/// as the turn's seed is.
pub fn candidate_seed(base: u32, k: usize) -> u32 {
    base.wrapping_add(CANDIDATE_SEED_OFFSETS[k]) & 0x7FFF_FFFF
}

/// The sentence count and word target the rulings arm gives for a length.
pub fn length_target(length: NarratorLength) -> &'static str {
    match length {
        NarratorLength::Brief => "one sentence, about 12 words",
        NarratorLength::Standard => "one or two sentences, about 20 words",
        NarratorLength::Rich => "two or three sentences, about 40 words",
    }
}

/// Most sentence items the schema allows in the brief-by-stop arm (Standard and Rich; Brief
/// is plain text there).
pub fn schema_item_limit(length: NarratorLength) -> usize {
    match length {
        NarratorLength::Brief => 1,
        NarratorLength::Standard => 2,
        NarratorLength::Rich => 3,
    }
}

/// The shipped rule lines the rulings arm removes or replaces.
pub const QUANTIFIER_LINE: &str = "- Allowed quantifiers for wagon, team, or body only: one, both, pair (never before a supply noun). Never use half.\n";

/// What the rulings arm adds to the Fireside voice.
pub const FIRESIDE_OXEN_LINES: &str =
    "\n- The wagon is drawn by oxen.\n- Speak of the party and the travellers. Address no one.";

/// The party-loss phrases as shipped, and as the rulings arm words them (no "one").
pub const PARTY_LOSS_REWORDS: [(&str, &str); 2] = [
    ("One of the party is lost.", "A traveller is lost."),
    (
        "Only one of the party is left.",
        "A lone traveller remains.",
    ),
];

/// The system prompt as the rulings arm sends it, made from the shipped one.
///
/// - The length cap line becomes a target: a sentence count and a word count to aim for.
/// - The "allowed quantifiers" line is dropped.
/// - Fireside is told the team is oxen and to speak of the party; its "personal" becomes
///   "close".
/// - The party-loss phrases in the situation lose the word "one".
pub fn rulings_system_prompt(shipped: &str, profile: GMProfile, length: NarratorLength) -> String {
    let cap_line = format!(
        "- LENGTH CAP: {} (at most {} words in all).",
        length.as_str(),
        length.word_cap()
    );
    let target_line = format!(
        "- LENGTH: {}. Write {}.",
        length.as_str(),
        length_target(length)
    );
    let mut out = shipped.replace(&cap_line, &target_line);
    out = out.replace(QUANTIFIER_LINE, "");
    if profile == GMProfile::Fireside {
        out = out.replace(
            "vivid, personal, slightly haunted",
            "vivid, close, slightly haunted",
        );
        let tone = "- Tone: empathetic, atmospheric, quietly foreboding.";
        out = out.replace(tone, &format!("{tone}{FIRESIDE_OXEN_LINES}"));
    }
    for (from, to) in PARTY_LOSS_REWORDS {
        out = out.replace(from, to);
    }
    out
}

/// The user prompt as the rulings arm sends it: the word cap is not stated.
pub fn rulings_user_prompt(shipped: &str, length: NarratorLength) -> String {
    shipped.replace(&format!(", at most {} words in all", length.word_cap()), "")
}

/// The species words the checker lets a voice use in the rulings arm: Fireside is told the team
/// is oxen, so it may say so. Every other voice is as shipped.
pub fn rulings_team_species(profile: GMProfile) -> Vec<String> {
    if profile == GMProfile::Fireside {
        vec!["ox".to_string(), "oxen".to_string()]
    } else {
        Vec::new()
    }
}

/// A Brief answer that was stopped at its first full stop, with the period put back. The
/// model's text up to the stop is kept as it is; an answer that already ends in a period (a
/// server that returns the stop text) is not given a second one.
pub fn restore_stopped_period(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.ends_with('.') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

/// Cut `line` back to its leading whole sentences that fit in `cap` words. A line already
/// within the cap is returned as it is, and so is a line whose first sentence alone is over
/// the cap (there is no sentence end to cut at; the checker then rejects it for length).
pub fn trim_to_word_cap(line: &str, cap: usize) -> String {
    let trimmed = line.trim();
    if trimmed.split_whitespace().count() <= cap {
        return trimmed.to_string();
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut words = 0;
    for sentence in trimmed.split_inclusive('.') {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        let n = sentence.split_whitespace().count();
        if !sentence.ends_with('.') || words + n > cap {
            break;
        }
        words += n;
        kept.push(sentence);
    }
    if kept.is_empty() {
        trimmed.to_string()
    } else {
        kept.join(" ")
    }
}
