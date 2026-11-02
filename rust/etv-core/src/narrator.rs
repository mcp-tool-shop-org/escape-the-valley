//! Narrator seam (`Narrator`, `NullNarrator`, `NarrationContext`, and `TurnMattersFlags`).
//!
//! Provides a strictly read-only presentation hook for a future AI narrator:
//! - [`NarrationContext`] is an owned, read-only snapshot of what a turn shows (the fixed outcome
//!   sentence, event and choice IDs, shown effects/deltas/lines, place, and band). It holds **no**
//!   reference or handle to [`crate::rng::CpythonRng`], [`crate::model::RunState`], the save path,
//!   or any mutable engine state.
//! - [`TurnMattersFlags`] is derived by a pure read of effects and state fields the engine has
//!   already computed on that turn, with **zero RNG draws and zero state mutation**.
//! - [`Narrator::narrate`] is called only when [`TurnMattersFlags::any`] is `true`.
//! - [`NullNarrator`] is the default implementation and always returns `None`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::StepMessages;
use crate::model::{GamePhase, IntentAction, PlayerIntent, RunState, TimeOfDay};
use crate::physics::{current_node, journey_pressure, HealthEffect};

/// Read-only trait for an optional turn narrator.
///
/// Implementations receive a frozen [`NarrationContext`] and may return additional presentation
/// prose (`Some(String)`) or `None` to keep the fixed outcome sentence unchanged.
pub trait Narrator {
    fn narrate(&self, ctx: &NarrationContext) -> Option<String>;
}

/// Default no-op narrator that always returns `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NullNarrator;

impl Narrator for NullNarrator {
    fn narrate(&self, _ctx: &NarrationContext) -> Option<String> {
        None
    }
}

/// Breakdown of the five "turn matters" rules derived by a pure read of post-step engine effects
/// and state fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnMattersFlags {
    /// A party member died on this turn (`StepMessages.effects` contains `HealthEffect::Died` or
    /// a member's `health` transitioned from `> 0` to `<= 0`).
    pub death: bool,
    /// A party member was injured on this turn (`PartyMember.health` fell below its pre-turn value).
    pub injury: bool,
    /// A supply key reached zero on this turn (`SuppliesState.items[k]` transitioned from `> 0`
    /// to `<= 0`).
    pub supply_zero: bool,
    /// The party arrived at a map node / landmark on this turn (`RunState.location_id` advanced to
    /// the destination node in `StepEngine::arrive_at_next_node`).
    pub landmark_arrival: bool,
    /// The party ended the turn camped at night (`pre.phase == GamePhase::Camp`,
    /// `!state.game_over`, `intent.action != IntentAction::Travel`, and post-step
    /// `state.time_of_day == TimeOfDay::Night`).
    pub night_at_camp: bool,
}

impl TurnMattersFlags {
    /// Returns `true` if any of the five "turn matters" rules fired.
    pub fn any(&self) -> bool {
        self.death || self.injury || self.supply_zero || self.landmark_arrival || self.night_at_camp
    }
}

/// Owned, read-only snapshot of what a single turn shows to the player.
///
/// `NarrationContext` intentionally contains no reference or field for [`crate::model::RunState`],
/// [`crate::rng::CpythonRng`], or any save file handle.
///
/// # Compile-level isolation checks
///
/// `NarrationContext` cannot reach `&mut RunState` or `rng`:
///
/// ```compile_fail
/// use etv_core::narrator::{NarrationContext, Narrator};
///
/// struct BadNarrator;
/// impl Narrator for BadNarrator {
///     fn narrate(&self, ctx: &NarrationContext) -> Option<String> {
///         // ERROR: no field `state` or `rng` on `&NarrationContext`
///         let _ = &mut ctx.state;
///         let _ = ctx.rng.random();
///         None
///     }
/// }
/// ```
///
/// Even a narrator attempting to mutate fields on `NarrationContext` fails to compile because all
/// fields are private with shared-reference getters only:
///
/// ```compile_fail
/// use etv_core::narrator::NarrationContext;
///
/// fn try_mutate(ctx: &NarrationContext) {
///     ctx.outcome_sentence = String::from("tampered");
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NarrationContext {
    outcome_sentence: String,
    event_id: String,
    choice_id: String,
    shown_effects: Vec<HealthEffect>,
    shown_deltas: BTreeMap<String, i32>,
    shown_lines: Vec<String>,
    place: String,
    band: String,
    flags: TurnMattersFlags,
}

impl NarrationContext {
    /// Construct an owned, read-only [`NarrationContext`] snapshot for a completed turn.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        outcome_sentence: String,
        event_id: String,
        choice_id: String,
        shown_effects: Vec<HealthEffect>,
        shown_deltas: BTreeMap<String, i32>,
        shown_lines: Vec<String>,
        place: String,
        band: String,
        flags: TurnMattersFlags,
    ) -> Self {
        Self {
            outcome_sentence,
            event_id,
            choice_id,
            shown_effects,
            shown_deltas,
            shown_lines,
            place,
            band,
            flags,
        }
    }

    /// Fixed outcome sentence produced by the deterministic engine for this turn.
    pub fn outcome_sentence(&self) -> &str {
        &self.outcome_sentence
    }

    /// Event ID active or resolved on this turn (empty if none).
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    /// Choice ID selected on this turn (empty if none).
    pub fn choice_id(&self) -> &str {
        &self.choice_id
    }

    /// Health effects shown to the player on this turn.
    pub fn shown_effects(&self) -> &[HealthEffect] {
        &self.shown_effects
    }

    /// Numerical resource/stat deltas shown to the player on this turn.
    pub fn shown_deltas(&self) -> &BTreeMap<String, i32> {
        &self.shown_deltas
    }

    /// Output lines emitted by the engine on this turn.
    pub fn shown_lines(&self) -> &[String] {
        &self.shown_lines
    }

    /// Human-readable name (or ID) of the current map node / place.
    pub fn place(&self) -> &str {
        &self.place
    }

    /// Journey band label (`"early"`, `"mid"`, or `"late"`) derived from journey progress.
    pub fn band(&self) -> &str {
        &self.band
    }

    /// Which of the five "turn matters" rules triggered on this turn.
    pub fn flags(&self) -> &TurnMattersFlags {
        &self.flags
    }
}

/// Read-only snapshot of pre-step values captured before `StepEngine::step` so post-step changes
/// (health drop, supply reaching zero, landmark arrival, night at camp) can be detected purely by
/// reading existing `RunState` fields and `StepMessages` without any new RNG draw or state change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreTurnObservation {
    pub phase: GamePhase,
    pub time_of_day: TimeOfDay,
    pub location_id: String,
    pub member_health: Vec<i32>,
    pub supplies: BTreeMap<String, i32>,
}

impl PreTurnObservation {
    /// Capture pre-turn values from `phase` and `state` prior to `StepEngine::step`.
    pub fn capture(phase: GamePhase, state: &RunState) -> Self {
        Self {
            phase,
            time_of_day: state.time_of_day,
            location_id: state.location_id.clone(),
            member_health: state.party.members.iter().map(|m| m.health).collect(),
            supplies: state.supplies.items.clone(),
        }
    }
}

/// Evaluate the five "turn matters" rules by a pure read of `pre`, `intent`, `msgs`, and
/// post-step `state` (zero RNG draws, zero state mutation):
///
/// 1. `death`: reads `msgs.effects` (`StepMessages::effects` in `rust/etv-core/src/engine.rs:67`,
///    populated with `HealthEffect::Died` from `rust/etv-core/src/physics.rs:192,265,652` and
///    `rust/etv-core/src/events.rs:681`) and `state.party.members[i].health`
///    (`rust/etv-core/src/model.rs:580`).
/// 2. `injury`: reads `state.party.members[i].health` (`rust/etv-core/src/model.rs:580`, updated
///    at `rust/etv-core/src/physics.rs:208,218,247,339,591,615` and
///    `rust/etv-core/src/events.rs:674-675`) and checks `post_health < pre_health`.
/// 3. `supply_zero`: reads `state.supplies.items` (`rust/etv-core/src/model.rs:676`, updated at
///    `rust/etv-core/src/model.rs:701-710`) and checks if any key transitioned from `> 0` before
///    the turn to `<= 0` after the turn.
/// 4. `landmark_arrival`: reads `state.location_id` (`rust/etv-core/src/model.rs:1083`), updated
///    exclusively by `StepEngine::arrive_at_next_node` (`rust/etv-core/src/engine.rs:310-311,961`),
///    checking `state.location_id != pre.location_id`.
/// 5. `night_at_camp`: flags only when the turn ends at night while the party is camped — reads
///    `pre.phase` (`StepEngine::phase` at `rust/etv-core/src/engine.rs:146,234` / `GamePhase::Camp`
///    at `rust/etv-core/src/model.rs:501`), `state.game_over` (`RunState::game_over` at
///    `rust/etv-core/src/model.rs:1109`), `intent.action` (`PlayerIntent::action` at
///    `rust/etv-core/src/model.rs:956`, requiring `intent.action != IntentAction::Travel` so the
///    party remains camped in `GamePhase::Camp`), and post-step `state.time_of_day`
///    (`RunState::time_of_day` at `rust/etv-core/src/model.rs:1081`, compared against
///    `TimeOfDay::Night` at `rust/etv-core/src/model.rs:130`, advanced in
///    `StepEngine::advance_time` at `rust/etv-core/src/engine.rs:1035-1044`).
pub fn evaluate_turn_matters(
    pre: &PreTurnObservation,
    intent: &PlayerIntent,
    msgs: &StepMessages,
    state: &RunState,
) -> TurnMattersFlags {
    let effect_death = msgs
        .effects
        .iter()
        .any(|eff| matches!(eff, HealthEffect::Died { .. }));
    let transition_death = pre
        .member_health
        .iter()
        .zip(state.party.members.iter())
        .any(|(&before, after)| before > 0 && after.health <= 0);
    let death = effect_death || transition_death;

    let injury = pre
        .member_health
        .iter()
        .zip(state.party.members.iter())
        .any(|(&before, after)| after.health < before);

    let supply_zero = pre.supplies.iter().any(|(key, &before_val)| {
        let after_val = state.supplies.items.get(key).copied().unwrap_or(0);
        before_val > 0 && after_val <= 0
    });

    let landmark_arrival = state.location_id != pre.location_id;

    let night_at_camp = pre.phase == GamePhase::Camp
        && !state.game_over
        && intent.action != IntentAction::Travel
        && state.time_of_day == TimeOfDay::Night;

    TurnMattersFlags {
        death,
        injury,
        supply_zero,
        landmark_arrival,
        night_at_camp,
    }
}

/// Compute the journey band label (`"early"`, `"mid"`, or `"late"`) from `journey_pressure(state)`
/// (`rust/etv-core/src/physics.rs:43-48`).
pub fn journey_band(state: &RunState) -> String {
    let p = journey_pressure(state);
    if p < 0.33 {
        "early".to_string()
    } else if p < 0.66 {
        "mid".to_string()
    } else {
        "late".to_string()
    }
}

/// Build an owned, read-only [`NarrationContext`] from a completed step.
pub fn build_narration_context(
    state: &RunState,
    event_id: &str,
    choice_id: &str,
    msgs: &StepMessages,
    flags: TurnMattersFlags,
) -> NarrationContext {
    let outcome_sentence = if !msgs.outcome_narration.is_empty() {
        msgs.outcome_narration.clone()
    } else {
        msgs.lines.join(" ")
    };
    let place = current_node(state)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| state.location_id.clone());
    let band = journey_band(state);

    NarrationContext::new(
        outcome_sentence,
        event_id.to_string(),
        choice_id.to_string(),
        msgs.effects.clone(),
        msgs.outcome_deltas.clone(),
        msgs.lines.clone(),
        place,
        band,
        flags,
    )
}
