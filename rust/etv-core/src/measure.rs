//! Step 5 harness: fallback-rate and latency measurement for the live narrator.
//!
//! The redesign (`REDESIGN-narrator-every-turn`, step 5) measures, on a fixed set of turns and for
//! every personality, length and model: how often a generated line fails the hard rules (so the
//! plain fallback line shows), and how long the narrator takes per call.
//!
//! This module is the instrument only. It makes no model call by itself: the caller supplies a
//! [`NarratorTransport`], and the unit tests drive it with scripted transports. A run is refused
//! unless the turn-set file's SHA-256 matches the digest the caller froze in advance, so a result
//! always names the exact turn set it measured.
//!
//! ## Turn-set file (`etv-turn-set-1`)
//!
//! ```json
//! {
//!   "schema": "etv-turn-set-1",
//!   "turns": [
//!     {
//!       "id": "t001",
//!       "seed": 42,
//!       "event_id": "f1_001",
//!       "choice_id": "A",
//!       "weirdness_level": 0,
//!       "uncanny_tokens": 2,
//!       "deltas": {"rope": -1},
//!       "event_seed": "A shallow ford.",
//!       "choice_label": "Drive the team through",
//!       "fallback_line": "The rope runs out."
//!     }
//!   ]
//! }
//! ```
//!
//! Each turn is rebuilt into a [`NarrationContext`] through the same engine path the game uses
//! (`build_narration_context_with_phase`), so the prompt facts and the checker facts are the real
//! ones. `deltas` are the applied supply deltas shown on the turn, `fallback_line` is the plain
//! line the game would show.
//!
//! ## Reports
//!
//! [`run_measurement`] returns a [`MeasureReport`] (counts, rates, latency, rejects by reason
//! code; no model text) and one [`TurnRow`] per profile, length and turn that keeps everything
//! the model wrote (Director ruling: keep all model output).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::content::{embedded_content_pack, hex_encode_32};
use crate::engine::{EventChoiceInfo, StepMessages};
use crate::model::{GMProfile, GamePhase};
use crate::narrator::{
    build_narration_context_with_deltas, build_narration_context_with_phase,
    profile_interface_name, FormatSchema, LiveNarrator, NarrationContext, Narrator, NarratorLength,
    NarratorSettings, NarratorTransport, PromptRules, SlotOutcome, SlotRecord, TurnMattersFlags,
    DEFAULT_OLLAMA_ENDPOINT,
};
use crate::worldgen::create_new_run_with_content;

/// Schema tag of the turn-set file.
pub const TURN_SET_SCHEMA: &str = "etv-turn-set-1";
/// Schema tag of a version 2 turn-set file: each turn stores the whole narrator context the
/// game built for it.
pub const TURN_SET_SCHEMA_V2: &str = "etv-turn-set-2";
/// Schema tag of the summary report.
pub const REPORT_SCHEMA: &str = "etv-fallback-measure-1";

fn default_uncanny_tokens() -> i32 {
    2
}

/// One frozen turn: everything needed to rebuild its [`NarrationContext`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnSpec {
    pub id: String,
    pub seed: i64,
    pub event_id: String,
    pub choice_id: String,
    /// Engine weirdness level `0..=3` (`RunState::weirdness_level`). The checker band is derived
    /// from it exactly as in the game: level 3 with tokens left is "strong" (band 2), level 2 is
    /// "hint" (band 1), anything else is band 0.
    pub weirdness_level: u8,
    /// Uncanny tokens left (`RunState::uncanny_tokens`); none left means band 0. A new run has 2.
    #[serde(default = "default_uncanny_tokens")]
    pub uncanny_tokens: i32,
    /// Applied supply deltas shown on the turn (resource key to signed amount).
    #[serde(default)]
    pub deltas: BTreeMap<String, i32>,
    /// Every change the turn applied, by effect key (supplies and also morale, wagon, time and
    /// the rest), as the game's own context carries them. When present it is used as the
    /// turn's shown effects in place of `deltas`, so the prompt and the checker see what the
    /// game would show. Absent in turn sets written before it existed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub effects: BTreeMap<String, i32>,
    /// The event's `narration_seed` text.
    pub event_seed: String,
    /// The chosen option's label.
    pub choice_label: String,
    /// The plain fallback line shown when the narrator falls back.
    pub fallback_line: String,
}

/// A frozen set of turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnSet {
    pub schema: String,
    pub turns: Vec<TurnSpec>,
}

/// One frozen turn of a version 2 set: the narrator context exactly as the game built it at
/// that moment of a real run (day, journey band, phase, time of day, place, biome, party health
/// and morale bands, pressures, themes, the turn's effects and flags, and the checker's facts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnSpecV2 {
    pub id: String,
    /// The run's seed.
    pub seed: i64,
    /// Engine weirdness level the run was created at (`0..=3`).
    pub weirdness_level: u8,
    /// Simulator policy that played the run.
    pub policy: String,
    /// Turn of the run (1 is the first step).
    pub turn: u32,
    /// What the player did on the turn: a camp action name, or `choose`.
    pub action: String,
    /// The quota tags this turn was selected for or also satisfies, sorted.
    pub strata: Vec<String>,
    /// The context the game built for this turn. Its `gm_profile` is replaced by the profile
    /// under measurement when the turn is replayed.
    pub context: NarrationContext,
}

/// A frozen version 2 set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnSetV2 {
    pub schema: String,
    pub turns: Vec<TurnSpecV2>,
}

/// A turn set of either version, as read from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnyTurnSet {
    V1(TurnSet),
    V2(TurnSetV2),
}

impl AnyTurnSet {
    pub fn len(&self) -> usize {
        match self {
            AnyTurnSet::V1(s) => s.turns.len(),
            AnyTurnSet::V2(s) => s.turns.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn schema(&self) -> &str {
        match self {
            AnyTurnSet::V1(s) => &s.schema,
            AnyTurnSet::V2(s) => &s.schema,
        }
    }

    /// The turns in file order, each able to rebuild its context.
    pub fn measured_turns(&self) -> Vec<MeasuredTurn<'_>> {
        match self {
            AnyTurnSet::V1(s) => s.turns.iter().map(MeasuredTurn::V1).collect(),
            AnyTurnSet::V2(s) => s.turns.iter().map(MeasuredTurn::V2).collect(),
        }
    }
}

/// One turn of either set version.
#[derive(Debug, Clone, Copy)]
pub enum MeasuredTurn<'a> {
    V1(&'a TurnSpec),
    V2(&'a TurnSpecV2),
}

impl MeasuredTurn<'_> {
    pub fn id(&self) -> &str {
        match self {
            MeasuredTurn::V1(t) => &t.id,
            MeasuredTurn::V2(t) => &t.id,
        }
    }

    /// The narrator context of this turn for `profile`: rebuilt from a fresh run for a version 1
    /// turn, the stored context for a version 2 turn.
    pub fn context(&self, profile: GMProfile) -> NarrationContext {
        match self {
            MeasuredTurn::V1(t) => build_turn_context(t, profile),
            MeasuredTurn::V2(t) => t.context.clone().with_gm_profile(profile),
        }
    }
}

/// Parse a version 2 turn-set file, refusing it unless its SHA-256 equals `expected_sha256`.
pub fn parse_turn_set_v2(bytes: &[u8], expected_sha256: &str) -> Result<TurnSetV2, String> {
    let actual = sha256_hex(bytes);
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(format!(
            "turn set sha256 mismatch: file is {actual}, frozen digest is {expected}"
        ));
    }
    let set: TurnSetV2 =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid turn set JSON: {e}"))?;
    if set.schema != TURN_SET_SCHEMA_V2 {
        return Err(format!(
            "turn set schema is {:?}, expected {TURN_SET_SCHEMA_V2:?}",
            set.schema
        ));
    }
    if set.turns.is_empty() {
        return Err("turn set has no turns".to_string());
    }
    let mut seen = BTreeSet::new();
    for t in &set.turns {
        if !seen.insert(t.id.as_str()) {
            return Err(format!("duplicate turn id {:?}", t.id));
        }
        if t.context.fallback_line().trim().is_empty() {
            return Err(format!("turn {:?}: fallback line is empty", t.id));
        }
    }
    Ok(set)
}

/// Parse a turn-set file of either version by its `schema` tag, refusing it unless its SHA-256
/// equals `expected_sha256`.
pub fn parse_any_turn_set(bytes: &[u8], expected_sha256: &str) -> Result<AnyTurnSet, String> {
    #[derive(Deserialize)]
    struct Tag {
        schema: String,
    }
    let actual = sha256_hex(bytes);
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(format!(
            "turn set sha256 mismatch: file is {actual}, frozen digest is {expected}"
        ));
    }
    let tag: Tag =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid turn set JSON: {e}"))?;
    match tag.schema.as_str() {
        TURN_SET_SCHEMA => parse_turn_set(bytes, expected_sha256).map(AnyTurnSet::V1),
        TURN_SET_SCHEMA_V2 => parse_turn_set_v2(bytes, expected_sha256).map(AnyTurnSet::V2),
        other => Err(format!(
            "turn set schema is {other:?}, expected {TURN_SET_SCHEMA:?} or {TURN_SET_SCHEMA_V2:?}"
        )),
    }
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex_encode_32(&digest)
}

/// Parse a turn-set file, refusing it unless its SHA-256 equals `expected_sha256`
/// (case-insensitive) and its content is well formed.
pub fn parse_turn_set(bytes: &[u8], expected_sha256: &str) -> Result<TurnSet, String> {
    let actual = sha256_hex(bytes);
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(format!(
            "turn set sha256 mismatch: file is {actual}, frozen digest is {expected}"
        ));
    }
    let set: TurnSet =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid turn set JSON: {e}"))?;
    if set.schema != TURN_SET_SCHEMA {
        return Err(format!(
            "turn set schema is {:?}, expected {TURN_SET_SCHEMA:?}",
            set.schema
        ));
    }
    if set.turns.is_empty() {
        return Err("turn set has no turns".to_string());
    }
    let mut seen = BTreeSet::new();
    for t in &set.turns {
        if !seen.insert(t.id.as_str()) {
            return Err(format!("duplicate turn id {:?}", t.id));
        }
        if t.weirdness_level > 3 {
            return Err(format!(
                "turn {:?}: weirdness_level {} is outside 0..=3",
                t.id, t.weirdness_level
            ));
        }
        if t.fallback_line.trim().is_empty() {
            return Err(format!("turn {:?}: fallback_line is empty", t.id));
        }
    }
    Ok(set)
}

/// Rebuild the [`NarrationContext`] of one frozen turn for `profile`.
pub fn build_turn_context(spec: &TurnSpec, profile: GMProfile) -> NarrationContext {
    let mut state = create_new_run_with_content(i128::from(spec.seed), embedded_content_pack());
    state.weirdness_level = i32::from(spec.weirdness_level);
    state.uncanny_tokens = spec.uncanny_tokens;
    state.gm_profile = profile;
    let msgs = StepMessages {
        outcome_narration: spec.fallback_line.clone(),
        outcome_deltas: spec.deltas.clone(),
        event_narration: spec.event_seed.clone(),
        event_choices: vec![EventChoiceInfo {
            id: spec.choice_id.clone(),
            label: spec.choice_label.clone(),
            risk_hint: String::new(),
            cost_hint: String::new(),
            ..Default::default()
        }],
        ..StepMessages::default()
    };
    if spec.effects.is_empty() {
        return build_narration_context_with_phase(
            &state,
            GamePhase::Camp,
            &spec.event_id,
            &spec.choice_id,
            &msgs,
            TurnMattersFlags::default(),
        );
    }
    build_narration_context_with_deltas(
        &state,
        GamePhase::Camp,
        &spec.event_id,
        &spec.choice_id,
        &msgs,
        TurnMattersFlags::default(),
        spec.effects.clone(),
    )
}

/// Nearest-rank percentile of an ascending slice: the value at rank `ceil(pct * n / 100)`.
/// `None` for an empty slice.
pub fn percentile_nearest_rank(sorted: &[u64], pct: u32) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len() as u64;
    let rank = ((u64::from(pct) * n).div_ceil(100)).max(1);
    sorted.get((rank - 1) as usize).copied()
}

/// Parse a profile name: `chronicler`, `fireside`, `lantern` or `lantern-bearer`.
pub fn parse_profile(name: &str) -> Result<GMProfile, String> {
    match name.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "chronicler" => Ok(GMProfile::Chronicler),
        "fireside" => Ok(GMProfile::Fireside),
        "lantern" | "lantern-bearer" => Ok(GMProfile::Lantern),
        other => Err(format!("unknown profile {other:?}")),
    }
}

/// Parse a length name: `brief`, `standard` or `rich`.
pub fn parse_length(name: &str) -> Result<NarratorLength, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "brief" => Ok(NarratorLength::Brief),
        "standard" => Ok(NarratorLength::Standard),
        "rich" => Ok(NarratorLength::Rich),
        other => Err(format!("unknown length {other:?}")),
    }
}

/// What to measure and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasureConfig {
    /// Model name recorded in the report and sent to the endpoint.
    pub model: String,
    pub endpoint: String,
    pub profiles: Vec<GMProfile>,
    pub lengths: Vec<NarratorLength>,
    pub warmup_timeout_ms: u64,
    pub per_call_timeout_ms: u64,
    /// Run only the first `n` turns of the set (`None` = all).
    pub max_turns: Option<usize>,
    /// Wording of the shared rules in the system prompt (`Shipped` unless a comparison run
    /// asks for `Positive`, which names no banned word).
    pub prompt_rules: PromptRules,
    /// Answer shape asked for in `format` (`Plain` unless a comparison run asks for
    /// `CitedFacts`).
    pub format_schema: FormatSchema,
}

impl MeasureConfig {
    /// All three profiles and all three lengths, the default loopback endpoint, and the
    /// narrator's default timeouts.
    pub fn new(model: &str) -> Self {
        let d = NarratorSettings::default();
        Self {
            model: model.to_string(),
            endpoint: DEFAULT_OLLAMA_ENDPOINT.to_string(),
            profiles: vec![
                GMProfile::Chronicler,
                GMProfile::Fireside,
                GMProfile::Lantern,
            ],
            lengths: vec![
                NarratorLength::Brief,
                NarratorLength::Standard,
                NarratorLength::Rich,
            ],
            warmup_timeout_ms: d.warmup_timeout_ms,
            per_call_timeout_ms: d.per_call_timeout_ms,
            max_turns: None,
            prompt_rules: PromptRules::Shipped,
            format_schema: FormatSchema::Plain,
        }
    }
}

/// Latency of the calls in one cell, in milliseconds (nearest-rank percentiles).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencySummary {
    pub calls: usize,
    pub min_ms: Option<u64>,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub max_ms: Option<u64>,
}

impl LatencySummary {
    /// Summarise call latencies (any order).
    pub fn from_samples(samples: &[u64]) -> Self {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Self {
            calls: sorted.len(),
            min_ms: sorted.first().copied(),
            p50_ms: percentile_nearest_rank(&sorted, 50),
            p95_ms: percentile_nearest_rank(&sorted, 95),
            max_ms: sorted.last().copied(),
        }
    }
}

/// Counts for one profile and length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellReport {
    pub profile: String,
    pub length: String,
    pub turns: usize,
    /// Narrator slots shown (two per turn: scene and outcome).
    pub slots: usize,
    pub accepted_slots: usize,
    pub fallback_slots: usize,
    /// Turns on which at least one slot fell back.
    pub turns_with_fallback: usize,
    pub timeouts: usize,
    pub bad_json: usize,
    pub transport_or_http_errors: usize,
    pub not_called: usize,
    pub rule_rejects: usize,
    /// Rule rejects by [`crate::narrator::CheckReasonCode::as_str`] name.
    pub rejects_by_code: BTreeMap<String, usize>,
    /// `fallback_slots / slots`.
    pub fallback_rate_slots: f64,
    /// `turns_with_fallback / turns`.
    pub fallback_rate_turns: f64,
    pub latency: LatencySummary,
}

/// Summary of a whole run (no model text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasureReport {
    pub schema: String,
    pub turn_set_sha256: String,
    pub turns_in_set: usize,
    pub turns_run: usize,
    pub model: String,
    pub endpoint: String,
    pub per_call_timeout_ms: u64,
    pub warmup_timeout_ms: u64,
    /// Wording of the shared rules the run sent: `shipped` or `positive`.
    #[serde(default)]
    pub prompt_rules: PromptRules,
    /// Answer shape the run asked for: `plain` or `cited`.
    #[serde(default)]
    pub format_schema: FormatSchema,
    pub cells: Vec<CellReport>,
}

/// One slot of one turn, with everything the model wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRow {
    /// `accepted`, `timeout`, `transport_error`, `http_status`, `bad_envelope`, `bad_slot_json`,
    /// `rule_reject` or `not_called`.
    pub status: String,
    pub reject_code: Option<String>,
    pub reject_detail: Option<String>,
    pub latency_ms: Option<u64>,
    pub raw_response: Option<String>,
    pub candidate: Option<String>,
}

/// One turn for one profile and length.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnRow {
    pub profile: String,
    pub length: String,
    pub turn_id: String,
    pub scene: SlotRow,
    pub outcome: SlotRow,
}

fn slot_row(rec: Option<&SlotRecord>) -> SlotRow {
    let Some(rec) = rec else {
        return SlotRow {
            status: "not_called".to_string(),
            reject_code: None,
            reject_detail: None,
            latency_ms: None,
            raw_response: None,
            candidate: None,
        };
    };
    let (status, code, detail) = match &rec.outcome {
        SlotOutcome::Accepted => ("accepted", None, None),
        SlotOutcome::Timeout => ("timeout", None, None),
        SlotOutcome::TransportError => ("transport_error", None, None),
        SlotOutcome::HttpStatus { .. } => ("http_status", None, None),
        SlotOutcome::BadEnvelope => ("bad_envelope", None, None),
        SlotOutcome::BadSlotJson => ("bad_slot_json", None, None),
        SlotOutcome::RuleReject { code, detail } => (
            "rule_reject",
            Some(code.as_str().to_string()),
            Some(detail.clone()),
        ),
    };
    SlotRow {
        status: status.to_string(),
        reject_code: code,
        reject_detail: detail,
        latency_ms: Some(rec.latency_ms),
        raw_response: rec.raw_response.clone(),
        candidate: rec.candidate.clone(),
    }
}

fn ratio(num: usize, den: usize) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

/// Run the measurement: for every profile and length, narrate every turn of `set` through a
/// [`LiveNarrator`] on `transport`, and tally the outcome of every scene and outcome call.
///
/// `set_sha256` is recorded in the report; the caller has already checked it with
/// [`parse_turn_set`]. Each cell first checks that the model is available and warms it up; a
/// failure there aborts the run, since a measurement against a missing model means nothing.
pub fn run_measurement(
    set: &TurnSet,
    set_sha256: &str,
    cfg: &MeasureConfig,
    transport: Arc<dyn NarratorTransport>,
) -> Result<(MeasureReport, Vec<TurnRow>), String> {
    let all: Vec<MeasuredTurn<'_>> = set.turns.iter().map(MeasuredTurn::V1).collect();
    run_measurement_on(&all, set_sha256, cfg, transport)
}

/// [`run_measurement`] for a set of either version.
pub fn run_measurement_any(
    set: &AnyTurnSet,
    set_sha256: &str,
    cfg: &MeasureConfig,
    transport: Arc<dyn NarratorTransport>,
) -> Result<(MeasureReport, Vec<TurnRow>), String> {
    run_measurement_on(&set.measured_turns(), set_sha256, cfg, transport)
}

fn run_measurement_on(
    all: &[MeasuredTurn<'_>],
    set_sha256: &str,
    cfg: &MeasureConfig,
    transport: Arc<dyn NarratorTransport>,
) -> Result<(MeasureReport, Vec<TurnRow>), String> {
    let turns: Vec<&MeasuredTurn<'_>> = all
        .iter()
        .take(cfg.max_turns.unwrap_or(usize::MAX))
        .collect();
    let mut cells = Vec::new();
    let mut rows = Vec::new();

    for &profile in &cfg.profiles {
        for &length in &cfg.lengths {
            let settings = NarratorSettings {
                enabled: true,
                profile,
                endpoint: cfg.endpoint.clone(),
                allow_non_loopback: false,
                model: cfg.model.clone(),
                length,
                warmup_timeout_ms: cfg.warmup_timeout_ms,
                per_call_timeout_ms: cfg.per_call_timeout_ms,
            };
            let narrator = LiveNarrator::with_transport(settings, transport.clone());
            narrator.set_prompt_rules(cfg.prompt_rules);
            narrator.set_format_schema(cfg.format_schema);
            // Every turn of the set is asked for, so the breaker is off here.
            narrator.set_timeout_breaker(false);
            narrator.enable_slot_recording();
            let startup = narrator.check_startup();
            if !startup.available {
                return Err(startup
                    .message
                    .unwrap_or_else(|| "model is not available".to_string()));
            }
            narrator.warmup()?;

            let mut cell = CellReport {
                profile: profile_interface_name(profile).to_string(),
                length: length.as_str().to_ascii_lowercase(),
                turns: turns.len(),
                slots: turns.len() * 2,
                accepted_slots: 0,
                fallback_slots: 0,
                turns_with_fallback: 0,
                timeouts: 0,
                bad_json: 0,
                transport_or_http_errors: 0,
                not_called: 0,
                rule_rejects: 0,
                rejects_by_code: BTreeMap::new(),
                fallback_rate_slots: 0.0,
                fallback_rate_turns: 0.0,
                latency: LatencySummary::from_samples(&[]),
            };
            let mut latencies = Vec::new();

            for spec in &turns {
                let ctx = spec.context(profile);
                narrator.narrate_turn(&ctx);
                let recs = narrator.take_slot_records();
                let scene = recs.iter().find(|r| r.is_scene);
                let outcome = recs.iter().find(|r| !r.is_scene);
                let mut turn_fell_back = false;
                for slot in [scene, outcome] {
                    match slot {
                        None => {
                            cell.not_called += 1;
                            cell.fallback_slots += 1;
                            turn_fell_back = true;
                        }
                        Some(rec) => {
                            latencies.push(rec.latency_ms);
                            match &rec.outcome {
                                SlotOutcome::Accepted => cell.accepted_slots += 1,
                                other => {
                                    cell.fallback_slots += 1;
                                    turn_fell_back = true;
                                    match other {
                                        SlotOutcome::Timeout => cell.timeouts += 1,
                                        SlotOutcome::BadEnvelope | SlotOutcome::BadSlotJson => {
                                            cell.bad_json += 1
                                        }
                                        SlotOutcome::TransportError
                                        | SlotOutcome::HttpStatus { .. } => {
                                            cell.transport_or_http_errors += 1
                                        }
                                        SlotOutcome::RuleReject { code, .. } => {
                                            cell.rule_rejects += 1;
                                            *cell
                                                .rejects_by_code
                                                .entry(code.as_str().to_string())
                                                .or_insert(0) += 1;
                                        }
                                        SlotOutcome::Accepted => {}
                                    }
                                }
                            }
                        }
                    }
                }
                if turn_fell_back {
                    cell.turns_with_fallback += 1;
                }
                rows.push(TurnRow {
                    profile: cell.profile.clone(),
                    length: cell.length.clone(),
                    turn_id: spec.id().to_string(),
                    scene: slot_row(scene),
                    outcome: slot_row(outcome),
                });
            }

            cell.fallback_rate_slots = ratio(cell.fallback_slots, cell.slots);
            cell.fallback_rate_turns = ratio(cell.turns_with_fallback, cell.turns);
            cell.latency = LatencySummary::from_samples(&latencies);
            cells.push(cell);
        }
    }

    Ok((
        MeasureReport {
            schema: REPORT_SCHEMA.to_string(),
            turn_set_sha256: set_sha256.trim().to_ascii_lowercase(),
            turns_in_set: all.len(),
            turns_run: turns.len(),
            model: cfg.model.clone(),
            endpoint: cfg.endpoint.clone(),
            per_call_timeout_ms: cfg.per_call_timeout_ms,
            warmup_timeout_ms: cfg.warmup_timeout_ms,
            prompt_rules: cfg.prompt_rules,
            format_schema: cfg.format_schema,
            cells,
        },
        rows,
    ))
}

/// Usage text for the `narrator_measure` binary.
pub const CLI_USAGE: &str = "\
narrator_measure: fallback-rate and latency measurement for the live narrator (step 5)

USAGE:
  narrator_measure --turn-set PATH --turn-set-sha256 HEX --check-only
  narrator_measure --turn-set PATH --turn-set-sha256 HEX --model NAME
                   [--endpoint URL] [--profiles LIST] [--lengths LIST] [--max-turns N]
                   [--warmup-timeout-ms N] [--per-call-timeout-ms N]
                   [--prompt-rules shipped|positive] [--format-schema plain|cited|text]
                   [--report PATH] [--rows PATH]

  --turn-set-sha256   digest the turn set was frozen under; the run is refused on a mismatch
  --check-only        verify the digest and rebuild every turn; makes no model call
  --profiles          comma list of chronicler,fireside,lantern-bearer (default: all)
  --lengths           comma list of brief,standard,rich (default: all)
  --prompt-rules      wording of the shared rules in the prompt: shipped (default), or
                      positive (says what to do and names no banned word), to compare
  --format-schema     answer shape asked for: plain (default, a sentences array); cited
                      (a cited_fact_keys array before the sentences array); or text
                      (no schema: plain sentences, checked as they stand), to compare
  --report PATH       write the summary JSON here (counts, rates, latency; no model text)
  --rows PATH         write one JSON line per turn with everything the model wrote
";

fn take_value<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, String> {
    *i += 1;
    args.get(*i)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Command-line driver behind the `narrator_measure` binary. `args` excludes the program name.
/// The transport is injected so tests never touch the network.
pub fn run_cli(
    args: &[String],
    transport: Arc<dyn NarratorTransport>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let mut turn_set = None;
    let mut sha = None;
    let mut check_only = false;
    let mut model = None;
    let mut cfg = MeasureConfig::new("");
    let mut report_path = None;
    let mut rows_path = None;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "--help" | "-h" => {
                out.write_all(CLI_USAGE.as_bytes())
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
            "--check-only" => check_only = true,
            "--turn-set" => turn_set = Some(take_value(args, &mut i, flag)?.to_string()),
            "--turn-set-sha256" => sha = Some(take_value(args, &mut i, flag)?.to_string()),
            "--model" => model = Some(take_value(args, &mut i, flag)?.to_string()),
            "--endpoint" => cfg.endpoint = take_value(args, &mut i, flag)?.to_string(),
            "--report" => report_path = Some(take_value(args, &mut i, flag)?.to_string()),
            "--rows" => rows_path = Some(take_value(args, &mut i, flag)?.to_string()),
            "--profiles" => {
                cfg.profiles = take_value(args, &mut i, flag)?
                    .split(',')
                    .map(parse_profile)
                    .collect::<Result<_, _>>()?;
            }
            "--lengths" => {
                cfg.lengths = take_value(args, &mut i, flag)?
                    .split(',')
                    .map(parse_length)
                    .collect::<Result<_, _>>()?;
            }
            "--max-turns" => {
                cfg.max_turns = Some(
                    take_value(args, &mut i, flag)?
                        .parse()
                        .map_err(|_| "--max-turns needs a whole number".to_string())?,
                );
            }
            "--prompt-rules" => {
                cfg.prompt_rules = match take_value(args, &mut i, flag)? {
                    "shipped" => PromptRules::Shipped,
                    "positive" => PromptRules::Positive,
                    other => {
                        return Err(format!(
                            "--prompt-rules must be shipped or positive, not {other:?}"
                        ))
                    }
                };
            }
            "--format-schema" => {
                cfg.format_schema = match take_value(args, &mut i, flag)? {
                    "plain" => FormatSchema::Plain,
                    "cited" => FormatSchema::CitedFacts,
                    "text" => FormatSchema::PlainText,
                    other => {
                        return Err(format!(
                            "--format-schema must be plain, cited or text, not {other:?}"
                        ))
                    }
                };
            }
            "--warmup-timeout-ms" => {
                cfg.warmup_timeout_ms = take_value(args, &mut i, flag)?
                    .parse()
                    .map_err(|_| "--warmup-timeout-ms needs a whole number".to_string())?;
            }
            "--per-call-timeout-ms" => {
                cfg.per_call_timeout_ms = take_value(args, &mut i, flag)?
                    .parse()
                    .map_err(|_| "--per-call-timeout-ms needs a whole number".to_string())?;
            }
            other => return Err(format!("unknown argument {other:?}\n\n{CLI_USAGE}")),
        }
        i += 1;
    }

    let turn_set = turn_set.ok_or("--turn-set is required")?;
    let sha = sha.ok_or("--turn-set-sha256 is required")?;
    let bytes =
        std::fs::read(&turn_set).map_err(|e| format!("cannot read turn set {turn_set}: {e}"))?;
    let set = parse_any_turn_set(&bytes, &sha)?;

    if check_only {
        for spec in set.measured_turns() {
            for &profile in &cfg.profiles {
                spec.context(profile);
            }
        }
        writeln!(
            out,
            "turn set ok: {} turns, sha256 {}",
            set.len(),
            sha.trim().to_ascii_lowercase()
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }

    cfg.model = model.ok_or("--model is required (there is no default model)")?;
    let (report, rows) = run_measurement_any(&set, &sha, &cfg, transport)?;
    let report_json =
        serde_json::to_string_pretty(&report).map_err(|e| format!("report encode: {e}"))?;
    if let Some(path) = report_path {
        std::fs::write(&path, &report_json).map_err(|e| format!("cannot write {path}: {e}"))?;
    }
    if let Some(path) = rows_path {
        let mut text = String::new();
        for row in &rows {
            text.push_str(&serde_json::to_string(row).map_err(|e| format!("row encode: {e}"))?);
            text.push('\n');
        }
        std::fs::write(&path, text).map_err(|e| format!("cannot write {path}: {e}"))?;
    }
    writeln!(out, "{report_json}").map_err(|e| e.to_string())?;
    Ok(())
}
