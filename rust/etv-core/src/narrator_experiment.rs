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
use crate::narrator::{CheckerContext, NarrationContext, NarratorLength};
use crate::narrator_truth::{
    changed_supplies, stem, what_happened_in, LeadStyle, TruthFacts, LEAD_SUBJECTS,
};

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
    /// Rules 1a to 1e of `narrator_truth`: a line may not contradict its turn, and the
    /// outcome line carries the turn's main fact.
    pub says_what_happened: bool,
    /// The outcome prompt leads with the turn's one main fact and drops the event's text.
    pub outcome_lead: bool,
    /// An outcome line may not open with the same two words as the previous accepted line.
    pub no_repeat_opening: bool,
    /// A line may not open with five or more words of the event's own text.
    pub no_event_copy: bool,
    /// Checker change: choice parroting rejects only a line that copies the whole choice label
    /// (two or more words) in a row, or five or more of its words in a row. Naming what the
    /// party did passes.
    pub parrot_verbatim: bool,
    /// Checker change: the repeated-opening rule compares the first three words, not two, and
    /// a line that opens with one of the lead's own subjects ([`LEAD_SUBJECTS`]) is exempt.
    pub opening_three_words: bool,
    /// The narrator fixes. A pyrrhic ending's outcome line must say what it cost.
    pub pyrrhic_loss_word: bool,
    /// The outcome lead on a choice turn says what the choice did, not its label.
    pub choice_result_lead: bool,
    /// On a choice turn the outcome line must carry the choice's verb or a supply it changed.
    pub choice_conveyed: bool,
    /// The scene line also gets the repeated-opening rule (event copying already covers it).
    pub scene_rules: bool,
    /// Both lines: no word from the anachronism list.
    pub no_anachronism: bool,
    /// The outcome line adds words of its own to the lead (one at Brief, two at Standard and
    /// Rich), names no time of day opposite to the turn's, and the lead's heading asks for one
    /// concrete detail.
    pub lead_guard: bool,
    /// The outcome lead is worded in the present tense.
    pub present_lead: bool,
    /// Both lines: the party is not shown breaking its taboo or doctrine, and the taboo is not
    /// stated as filler.
    pub taboo_rules: bool,
    /// Both lines: no river, stream, lake or ford the place, the biome, the run's themes and
    /// the turn's facts do not provide.
    pub no_water_source: bool,
    /// Measurement only: how many of the model's layers to put on the GPU (`num_gpu`), to
    /// see latency on a weaker card. `None` leaves the server's own choice.
    pub num_gpu: Option<u32>,
}

impl Default for ExperimentArms {
    fn default() -> Self {
        Self {
            rulings_prompt: false,
            brief_by_stop: false,
            model_card_sampling: false,
            candidates: 1,
            trim_to_cap: false,
            says_what_happened: false,
            outcome_lead: false,
            no_repeat_opening: false,
            no_event_copy: false,
            parrot_verbatim: false,
            opening_three_words: false,
            pyrrhic_loss_word: false,
            choice_result_lead: false,
            choice_conveyed: false,
            scene_rules: false,
            no_anachronism: false,
            lead_guard: false,
            present_lead: false,
            taboo_rules: false,
            no_water_source: false,
            num_gpu: None,
        }
    }
}

impl ExperimentArms {
    /// `true` when nothing is switched on.
    pub fn is_shipped(&self) -> bool {
        *self == Self::default()
    }

    /// How the outcome lead is worded under these arms.
    pub fn lead_style(&self) -> LeadStyle {
        LeadStyle {
            present: self.present_lead,
            choice_result: self.choice_result_lead,
        }
    }

    /// `true` when some arm needs the turn's facts in the checker.
    fn needs_truth(&self) -> bool {
        self.says_what_happened
            || self.no_event_copy
            || self.no_repeat_opening
            || self.pyrrhic_loss_word
            || self.choice_conveyed
            || self.no_anachronism
            || self.lead_guard
            || self.taboo_rules
            || self.no_water_source
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
        if self.says_what_happened {
            parts.push("says-what-happened".to_string());
        }
        if self.outcome_lead {
            parts.push("outcome-lead".to_string());
        }
        if self.no_repeat_opening {
            parts.push("no-repeat-opening".to_string());
        }
        if self.no_event_copy {
            parts.push("no-event-copy".to_string());
        }
        if self.parrot_verbatim {
            parts.push("parrot-verbatim".to_string());
        }
        if self.opening_three_words {
            parts.push("opening-three-words".to_string());
        }
        for (on, name) in [
            (self.pyrrhic_loss_word, "pyrrhic-loss-word"),
            (self.choice_result_lead, "choice-result-lead"),
            (self.choice_conveyed, "choice-conveyed"),
            (self.scene_rules, "scene-rules"),
            (self.no_anachronism, "no-anachronism"),
            (self.lead_guard, "lead-guard"),
            (self.present_lead, "present-lead"),
            (self.taboo_rules, "taboo-rules"),
            (self.no_water_source, "no-water-source"),
        ] {
            if on {
                parts.push(name.to_string());
            }
        }
        if let Some(layers) = self.num_gpu {
            parts.push(format!("num-gpu-{layers}"));
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
    "\n- If the team is mentioned, it is oxen.\n- Speak of the party and the travellers. Address no one.";

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

/// The checker context for one line of a turn under `arms`: the shipped context, plus what the
/// arms add. The narrator and the measurement harness both build it here, so a line is judged
/// the same way when it is written and when it is re-read.
pub fn checker_context_for(
    ctx: &NarrationContext,
    length: NarratorLength,
    profile: GMProfile,
    arms: ExperimentArms,
    is_outcome: bool,
    previous_opening: Option<&str>,
) -> CheckerContext {
    let mut checker = CheckerContext::from_narration_context(ctx, length);
    checker.parrot_verbatim = arms.parrot_verbatim;
    if arms.rulings_prompt {
        checker.team_species = rulings_team_species(profile);
    }
    if arms.needs_truth() {
        let mut facts = TruthFacts::from_context(ctx, is_outcome);
        facts.check_says_what_happened = arms.says_what_happened;
        facts.check_event_copy = arms.no_event_copy;
        facts.previous_opening = if arms.no_repeat_opening {
            previous_opening.map(str::to_string)
        } else {
            None
        };
        if arms.opening_three_words {
            facts.opening_words = 3;
            facts.opening_exempt = LEAD_SUBJECTS.iter().map(|s| s.to_string()).collect();
        }
        facts.scene_repeat_opening = arms.scene_rules && arms.no_repeat_opening;
        facts.pyrrhic_loss_word = arms.pyrrhic_loss_word;
        facts.check_anachronism = arms.no_anachronism;
        let label = ctx.check_choice_label();
        if arms.choice_conveyed && is_outcome && !label.is_empty() {
            if let Some(verb) = label.split_whitespace().map(clean).find(|w| w.len() >= 3) {
                facts.choice_stems = vec![stem(&verb)];
            }
            let (mut spent, gained) = changed_supplies(ctx);
            spent.extend(gained);
            facts.choice_supplies = spent;
        }
        let turn_text = format!(
            "{} {} {}",
            ctx.event_title(),
            ctx.check_event_seed(),
            ctx.check_choice_label()
        )
        .to_ascii_lowercase();
        let turn_words: Vec<String> = turn_text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect();
        if arms.taboo_rules {
            let situation = ctx.situation().to_ascii_lowercase();
            let after = |label: &str| {
                situation
                    .split(label)
                    .nth(1)
                    .and_then(|rest| rest.split('.').next())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default()
            };
            facts.taboo = after("their taboo:");
            facts.doctrine = after("their doctrine:");
            facts.check_taboo = true;
            facts.action = ctx.action().to_string();
            facts.is_night = ctx.time_of_day().as_str() == "night";
            facts.turn_words = turn_words.clone();
        }
        if arms.no_water_source {
            let mut allowed = crate::narrator_truth::place_water_words(ctx.place());
            allowed.extend(
                crate::narrator_truth::biome_water_words(ctx.biome())
                    .iter()
                    .map(|w| w.to_string()),
            );
            for w in &turn_words {
                if crate::narrator_truth::WATER_SOURCE_WORDS.contains(&w.as_str()) {
                    allowed.push(w.clone());
                    // The plural and singular go together.
                    let other = if let Some(s) = w.strip_suffix('s') {
                        s.to_string()
                    } else {
                        format!("{w}s")
                    };
                    allowed.push(other);
                }
            }
            // Water gained on the turn came from somewhere: every source word is allowed.
            if ctx.shown_deltas().get("water").is_some_and(|d| *d > 0) {
                allowed.extend(
                    crate::narrator_truth::WATER_SOURCE_WORDS
                        .iter()
                        .map(|w| w.to_string()),
                );
            }
            // The run's "river" theme is the designer's flavour: it provides river scenery
            // (Publisher's ruling). The taboo rule still catches the party drinking from it.
            if ctx.themes().iter().any(|t| t == "river") {
                for w in [
                    "river",
                    "rivers",
                    "riverbank",
                    "riverbed",
                    "stream",
                    "streams",
                    "ford",
                    "fords",
                ] {
                    allowed.push(w.to_string());
                }
            }
            facts.water_allowed = Some(allowed);
        }
        if arms.lead_guard && is_outcome {
            facts.lead = if arms.outcome_lead {
                what_happened_in(ctx, arms.lead_style())
            } else {
                String::new()
            };
            facts.lead_guard_min = match length {
                NarratorLength::Brief => 1,
                _ => 2,
            };
            facts.time_of_day = ctx.time_of_day().as_str().to_string();
        }
        checker.truth = Some(facts);
    }
    checker
}

/// The head of the outcome prompt in the outcome-lead arm.
pub const OUTCOME_LEAD_HEADING: &str =
    "WHAT HAPPENED (say this first, plainly, then at most one detail of the place):";

/// The head of the outcome prompt with the `lead-guard` arm: the line tells the fact in its
/// own words and adds one concrete detail drawn from the turn facts.
pub const OUTCOME_LEAD_HEADING_GUARD: &str = "WHAT HAPPENED (tell this first, plainly, in your own words, and add one concrete detail from the turn facts: the place, the time of day, the weather, or a body or party detail):";

/// A label word as the choice rule reads it: lowercase, punctuation trimmed.
fn clean(raw: &str) -> String {
    raw.trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

/// The outcome call's system prompt in the outcome-lead arm, made from the one it would have
/// had: the turn's one main fact is put first, before the turn facts, and the event's own text
/// and the list of choices on offer are taken out (that is the scene call's material).
pub fn lead_outcome_system_prompt(outcome_system: &str, ctx: &NarrationContext) -> String {
    lead_outcome_system_prompt_for(outcome_system, ctx, ExperimentArms::default())
}

/// [`lead_outcome_system_prompt`] with the lead worded and headed as `arms` ask (the
/// `present-lead`, `choice-result-lead` and `lead-guard` arms).
pub fn lead_outcome_system_prompt_for(
    outcome_system: &str,
    ctx: &NarrationContext,
    arms: ExperimentArms,
) -> String {
    let heading = if arms.lead_guard {
        OUTCOME_LEAD_HEADING_GUARD
    } else {
        OUTCOME_LEAD_HEADING
    };
    let lead = format!(
        "{heading}\n- {}\n\n",
        what_happened_in(ctx, arms.lead_style())
    );
    let mut out = String::with_capacity(outcome_system.len() + lead.len());
    for line in outcome_system.split_inclusive('\n') {
        if line.starts_with("- event_text: ") || line.starts_with("- choices_offered: ") {
            continue;
        }
        if line.starts_with("TURN FACTS:") {
            out.push_str(&lead);
        }
        out.push_str(line);
    }
    out
}
