//! Step 4 and step 5 tests: the finished hard-rule checker and the fallback-rate harness.
//!
//! Every expected value below is worked by hand from the voice guide rules and the redesign, not
//! copied from a run of the code. No test touches the network or a model: transports are scripted.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::content::embedded_content_pack;
use etv_core::engine::{EventChoiceInfo, StepMessages};
use etv_core::measure::{
    build_turn_context, parse_length, parse_profile, parse_turn_set, percentile_nearest_rank,
    run_cli, run_measurement, sha256_hex, LatencySummary, MeasureConfig, TURN_SET_SCHEMA,
};
use etv_core::model::{GMProfile, GamePhase};
use etv_core::narrator::{
    build_narration_context_with_phase, build_system_prompt, check_generated_line,
    compose_all_464_choice_fallbacks, CheckReasonCode, CheckerContext, FallbackPhraseTable,
    LiveNarrator, NarrationContext, Narrator, NarratorLength, NarratorSettings, NarratorTransport,
    SlotOutcome, TransportError, TurnMattersFlags,
};
use etv_core::worldgen::create_new_run_with_content;

const FIXTURE_SHA256: &str = "3b94b999eb21a22af6fba1eb264933ed43e771aace404c8d897afac4095ee99f";

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("narrator_turn_set_sample.json")
}

fn ctx_with(f: impl FnOnce(&mut CheckerContext)) -> CheckerContext {
    let mut c = CheckerContext::default();
    f(&mut c);
    c
}

fn code_of(line: &str, ctx: &CheckerContext) -> CheckReasonCode {
    check_generated_line(line, ctx)
        .expect_err(&format!("expected {line:?} to fail"))
        .code
}

// ---------------------------------------------------------------------------
// Reason codes
// ---------------------------------------------------------------------------

#[test]
fn reason_code_names_are_the_twelve_stable_strings() {
    let names: Vec<&str> = CheckReasonCode::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "numbers_digits_or_units",
            "quantifier_before_resource_or_half",
            "death_words",
            "mid_sentence_capital",
            "unauthorised_resource_noun",
            "band_banned_vocabulary",
            "invalid_punctuation",
            "length_cap_exceeded",
            "personal_pronoun",
            "named_party_member",
            "draft_animal_species",
            "choice_parroting",
        ]
    );
}

// ---------------------------------------------------------------------------
// New rules
// ---------------------------------------------------------------------------

#[test]
fn first_and_second_person_is_rejected_third_person_passes() {
    let ctx = CheckerContext::default();
    assert_eq!(
        check_generated_line("The team pulls the wagon through the ford.", &ctx),
        Ok(())
    );
    assert_eq!(
        code_of("The team pulls you through the ford.", &ctx),
        CheckReasonCode::PersonalPronoun
    );
    assert_eq!(
        code_of("We cross the ford.", &ctx),
        CheckReasonCode::PersonalPronoun
    );
    assert_eq!(
        code_of("Mud says you'll slip.", &ctx),
        CheckReasonCode::PersonalPronoun
    );
    assert_eq!(
        code_of("The wheel grips my hand.", &ctx),
        CheckReasonCode::PersonalPronoun
    );
    // "mine" is also a noun, so it is not treated as a pronoun.
    assert_eq!(
        check_generated_line("The trail passes an old mine.", &ctx),
        Ok(())
    );
}

#[test]
fn party_member_names_are_rejected_at_sentence_start_and_elsewhere() {
    let ctx = ctx_with(|c| c.party_names = vec!["Elias Thorne".to_string(), "Rosa".to_string()]);
    assert_eq!(
        check_generated_line("The drivers grip the reins.", &ctx),
        Ok(())
    );
    // First word: the mid-sentence capital rule cannot see this one.
    assert_eq!(
        code_of("Elias drives the team.", &ctx),
        CheckReasonCode::NamedPartyMember
    );
    assert_eq!(
        code_of("Thorne grips the reins.", &ctx),
        CheckReasonCode::NamedPartyMember
    );
    // Mid-sentence, the older capital rule fires first.
    assert_eq!(
        code_of("The team follows Rosa.", &ctx),
        CheckReasonCode::MidSentenceCapital
    );
    // Without names the rule is inactive: a capitalised first word is just a first word.
    assert_eq!(
        check_generated_line("Elias drives the team.", &CheckerContext::default()),
        Ok(())
    );
}

#[test]
fn draft_animal_species_need_the_event_to_name_them() {
    let none = CheckerContext::default();
    assert_eq!(
        check_generated_line("The team leans into the collars.", &none),
        Ok(())
    );
    assert_eq!(
        code_of("The mules lean into the collars.", &none),
        CheckReasonCode::DraftAnimalSpecies
    );
    assert_eq!(
        code_of("An ox stands in the mud.", &none),
        CheckReasonCode::DraftAnimalSpecies
    );
    let named = ctx_with(|c| c.event_seed = "A trader leads two mules down the road.".to_string());
    assert_eq!(
        check_generated_line("The mules lean into the collars.", &named),
        Ok(())
    );
    // Only the named species is allowed.
    assert_eq!(
        code_of("The oxen lean into the collars.", &named),
        CheckReasonCode::DraftAnimalSpecies
    );
}

#[test]
fn a_resource_the_event_seed_or_label_names_is_not_unspent() {
    let bare = CheckerContext::default();
    assert_eq!(
        code_of("The rope bites into the timber.", &bare),
        CheckReasonCode::UnauthorisedResourceNoun
    );
    let seed = ctx_with(|c| c.event_seed = "A coil of rope hangs from the pole.".to_string());
    assert_eq!(
        check_generated_line("The rope bites into the timber.", &seed),
        Ok(())
    );
    let label = ctx_with(|c| c.choice_label = "Cut the rope loose".to_string());
    assert_eq!(
        check_generated_line("The rope falls into the mud.", &label),
        Ok(())
    );
    // The turn's own effects still authorise it, as before.
    let effect = ctx_with(|c| c.allowed_resources = vec!["rope".to_string()]);
    assert_eq!(
        check_generated_line("The rope bites into the timber.", &effect),
        Ok(())
    );
    // Naming rope does not open the door to flour.
    assert_eq!(
        code_of("The flour spills across the bed.", &seed),
        CheckReasonCode::UnauthorisedResourceNoun
    );
}

#[test]
fn choice_parroting_needs_the_label_verb_and_a_label_noun() {
    let ctx = ctx_with(|c| c.choice_label = "Repair the bridge planks".to_string());
    // Label content words: repair, bridge, planks.
    assert_eq!(
        code_of("The party repairs the bridge planks.", &ctx),
        CheckReasonCode::ChoiceParroting
    );
    // Inflections count: "repairing" and "bridges" share stems with "repair" and "bridge".
    assert_eq!(
        code_of("The party is repairing old bridges.", &ctx),
        CheckReasonCode::ChoiceParroting
    );
    // The verb alone is not parroting.
    assert_eq!(
        check_generated_line("Hammers repair the damage, and the wheels cross.", &ctx),
        Ok(())
    );
    // A noun alone is not parroting.
    assert_eq!(
        check_generated_line("Hammer strikes the bridge, and the wheels cross.", &ctx),
        Ok(())
    );
    // A one-content-word label has no verb and noun to repeat.
    let rest = ctx_with(|c| c.choice_label = "Rest".to_string());
    assert_eq!(
        check_generated_line("The team rests by the fire.", &rest),
        Ok(())
    );
    // An empty label switches the rule off.
    assert_eq!(
        check_generated_line(
            "The party repairs the bridge planks.",
            &CheckerContext::default()
        ),
        Ok(())
    );
}

#[test]
fn older_rules_still_fire_first_where_both_apply() {
    // "you" and a digit: digits are checked before pronouns.
    let ctx = CheckerContext::default();
    assert_eq!(
        code_of("You find 3 wheels.", &ctx),
        CheckReasonCode::NumbersDigitsOrUnits
    );
    // "you" and a semicolon: punctuation is checked first.
    assert_eq!(
        code_of("You wait; the rain falls.", &ctx),
        CheckReasonCode::InvalidPunctuation
    );
}

// ---------------------------------------------------------------------------
// Context plumbing: names, seed and label reach the checker, never the prompt
// ---------------------------------------------------------------------------

fn context_for(
    seed: i128,
    seed_text: &str,
    label: &str,
    deltas: &[(&str, i32)],
) -> (NarrationContext, Vec<String>) {
    let mut state = create_new_run_with_content(seed, embedded_content_pack());
    state.weirdness_level = 0;
    let names: Vec<String> = state.party.members.iter().map(|m| m.name.clone()).collect();
    let mut msgs = StepMessages {
        outcome_narration: "The wagon waits.".to_string(),
        event_narration: seed_text.to_string(),
        event_choices: vec![EventChoiceInfo {
            id: "A".to_string(),
            label: label.to_string(),
            risk_hint: String::new(),
            cost_hint: String::new(),
        }],
        ..StepMessages::default()
    };
    for (k, v) in deltas {
        msgs.outcome_deltas.insert((*k).to_string(), *v);
    }
    let ctx = build_narration_context_with_phase(
        &state,
        GamePhase::Camp,
        "f1_001",
        "A",
        &msgs,
        TurnMattersFlags::default(),
    );
    (ctx, names)
}

#[test]
fn narration_context_carries_check_facts_and_the_prompt_omits_them() {
    let (ctx, names) = context_for(
        42,
        "A shallow ford runs brown.",
        "Drive the team through",
        &[("water", -1)],
    );
    assert_eq!(names.len(), 4, "a new run starts with a party of four");
    assert_eq!(ctx.check_party_names(), names.as_slice());
    assert_eq!(ctx.check_event_seed(), "A shallow ford runs brown.");
    assert_eq!(ctx.check_choice_label(), "Drive the team through");

    let checker = CheckerContext::from_narration_context(&ctx, NarratorLength::Standard);
    assert_eq!(checker.party_names, names);
    assert_eq!(checker.event_seed, "A shallow ford runs brown.");
    assert_eq!(checker.choice_label, "Drive the team through");
    assert_eq!(checker.allowed_resources, vec!["water".to_string()]);

    let prompt = build_system_prompt(GMProfile::Fireside, NarratorLength::Standard, &ctx);
    for name in &names {
        assert!(
            !prompt.contains(name.as_str()),
            "prompt must not contain party name {name:?}"
        );
    }
    assert!(!prompt.contains("A shallow ford runs brown."));
    assert!(!prompt.contains("Drive the team through"));
}

#[test]
fn a_label_that_is_missing_from_the_choice_list_gives_an_empty_label() {
    let mut state = create_new_run_with_content(42, embedded_content_pack());
    state.weirdness_level = 0;
    let msgs = StepMessages {
        outcome_narration: "The wagon waits.".to_string(),
        ..StepMessages::default()
    };
    let ctx = build_narration_context_with_phase(
        &state,
        GamePhase::Camp,
        "f1_001",
        "Z",
        &msgs,
        TurnMattersFlags::default(),
    );
    assert_eq!(ctx.check_choice_label(), "");
    assert_eq!(ctx.check_event_seed(), "");
}

// ---------------------------------------------------------------------------
// Fallback lines against the checker
// ---------------------------------------------------------------------------

#[test]
fn every_composed_fallback_line_passes_the_checker() {
    let table = FallbackPhraseTable::embedded();
    let lines = compose_all_464_choice_fallbacks(embedded_content_pack(), &table);
    assert_eq!(lines.len(), 464);
    let all_keys = table.key_order.clone();
    let mut failures: Vec<String> = Vec::new();
    for ((event, choice), line) in &lines {
        // Every resource key allowed, a generous length: this asks only whether the plain line
        // itself obeys the shared hard rules, as the redesign requires of the fallback lines.
        let ctx = ctx_with(|c| {
            c.allowed_resources = all_keys.clone();
            c.length = NarratorLength::Brief;
        });
        if let Err(f) = check_generated_line(line, &ctx) {
            failures.push(format!(
                "{event}:{choice} {line:?} -> {:?} {}",
                f.code, f.detail
            ));
        }
    }
    assert_eq!(failures, Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// LiveNarrator: reject tally and slot records
// ---------------------------------------------------------------------------

struct Script {
    tags: String,
    queue: Mutex<VecDeque<Result<(u16, String), TransportError>>>,
}

impl Script {
    fn new(model_tag: &str, responses: Vec<Result<(u16, String), TransportError>>) -> Arc<Self> {
        Arc::new(Self {
            tags: format!(r#"{{"models":[{{"name":"{model_tag}"}}]}}"#),
            queue: Mutex::new(responses.into()),
        })
    }
}

impl NarratorTransport for Script {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Ok((200, self.tags.clone()))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        _body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        self.queue
            .lock()
            .expect("script lock")
            .pop_front()
            .unwrap_or(Err(TransportError::ConnectionFailed(
                "script exhausted".to_string(),
            )))
    }
}

fn scene_ok(line: &str) -> Result<(u16, String), TransportError> {
    let inner = serde_json::json!({"scene_id": "s", "title": "t", "narration": line}).to_string();
    Ok((200, serde_json::json!({"response": inner}).to_string()))
}

fn outcome_ok(line: &str) -> Result<(u16, String), TransportError> {
    let inner =
        serde_json::json!({"scene_id": "s", "outcome_title": "t", "outcome_narration": line})
            .to_string();
    Ok((200, serde_json::json!({"response": inner}).to_string()))
}

fn warm() -> Result<(u16, String), TransportError> {
    Ok((200, r#"{"response":"ok"}"#.to_string()))
}

#[test]
fn live_narrator_tallies_rejects_by_code_and_records_every_call() {
    let (ctx, _) = context_for(
        42,
        "A shallow ford runs brown.",
        "Drive the team through",
        &[("water", -1)],
    );
    let transport = Script::new(
        "m:latest",
        vec![
            scene_ok("Mud clings to the spokes, and the team leans into the collars."),
            outcome_ok("The team pulls you through the ford."),
        ],
    );
    let settings = NarratorSettings {
        model: "m".to_string(),
        ..NarratorSettings::default()
    };
    let narrator = LiveNarrator::with_transport(settings, transport);
    narrator.enable_slot_recording();
    let turn = narrator.narrate_turn(&ctx);

    assert_eq!(
        turn.scene.as_deref(),
        Some("Mud clings to the spokes, and the team leans into the collars.")
    );
    assert_eq!(turn.outcome, None);
    let stats = narrator.stats();
    assert_eq!(
        (
            stats.calls,
            stats.successes,
            stats.rule_rejects,
            stats.fallbacks
        ),
        (2, 1, 1, 1)
    );
    assert_eq!(
        narrator.reject_code_counts(),
        BTreeMap::from([("personal_pronoun", 1usize)])
    );

    let recs = narrator.take_slot_records();
    assert_eq!(recs.len(), 2);
    assert!(recs[0].is_scene);
    assert_eq!(recs[0].outcome, SlotOutcome::Accepted);
    assert!(!recs[1].is_scene);
    assert_eq!(
        recs[1].outcome,
        SlotOutcome::RuleReject {
            code: CheckReasonCode::PersonalPronoun,
            detail: "first or second person word 'you'".to_string(),
        }
    );
    assert_eq!(
        recs[1].candidate.as_deref(),
        Some("The team pulls you through the ford.")
    );
    assert!(recs[1]
        .raw_response
        .as_deref()
        .is_some_and(|r| r.contains("outcome_narration")));
    assert!(
        narrator.take_slot_records().is_empty(),
        "taking the records clears them"
    );
}

#[test]
fn slot_recording_is_off_by_default() {
    let (ctx, _) = context_for(
        42,
        "A shallow ford runs brown.",
        "Drive the team through",
        &[("water", -1)],
    );
    let transport = Script::new(
        "m:latest",
        vec![
            scene_ok("The team holds."),
            outcome_ok("The wagon rolls on."),
        ],
    );
    let settings = NarratorSettings {
        model: "m".to_string(),
        ..NarratorSettings::default()
    };
    let narrator = LiveNarrator::with_transport(settings, transport);
    narrator.narrate_turn(&ctx);
    assert!(narrator.take_slot_records().is_empty());
    assert!(narrator.reject_code_counts().is_empty());
}

// ---------------------------------------------------------------------------
// Step 5 harness: pure helpers
// ---------------------------------------------------------------------------

#[test]
fn sha256_of_abc_matches_the_published_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn nearest_rank_percentiles_by_hand() {
    // n = 4: p50 rank ceil(200/100) = 2, p95 rank ceil(380/100) = 4.
    assert_eq!(percentile_nearest_rank(&[10, 20, 30, 40], 50), Some(20));
    assert_eq!(percentile_nearest_rank(&[10, 20, 30, 40], 95), Some(40));
    // n = 1: every percentile is the only value.
    assert_eq!(percentile_nearest_rank(&[7], 50), Some(7));
    assert_eq!(percentile_nearest_rank(&[7], 95), Some(7));
    // n = 20: p95 rank ceil(1900/100) = 19, the 19th value.
    let twenty: Vec<u64> = (1..=20).map(|n| n * 10).collect();
    assert_eq!(percentile_nearest_rank(&twenty, 95), Some(190));
    assert_eq!(percentile_nearest_rank(&twenty, 50), Some(100));
    assert_eq!(percentile_nearest_rank(&[], 50), None);

    // Unsorted input is sorted first.
    let s = LatencySummary::from_samples(&[40, 10, 30, 20]);
    assert_eq!(
        (s.calls, s.min_ms, s.p50_ms, s.p95_ms, s.max_ms),
        (4, Some(10), Some(20), Some(40), Some(40))
    );
    let empty = LatencySummary::from_samples(&[]);
    assert_eq!((empty.calls, empty.min_ms, empty.max_ms), (0, None, None));
}

#[test]
fn profile_and_length_names_parse() {
    assert_eq!(parse_profile("Chronicler"), Ok(GMProfile::Chronicler));
    assert_eq!(parse_profile("fireside"), Ok(GMProfile::Fireside));
    assert_eq!(parse_profile("Lantern-Bearer"), Ok(GMProfile::Lantern));
    assert_eq!(parse_profile("lantern_bearer"), Ok(GMProfile::Lantern));
    assert!(parse_profile("bard").is_err());
    assert_eq!(parse_length("Rich"), Ok(NarratorLength::Rich));
    assert!(parse_length("huge").is_err());
}

// ---------------------------------------------------------------------------
// Step 5 harness: the frozen turn set
// ---------------------------------------------------------------------------

#[test]
fn the_sample_turn_set_is_frozen_by_sha256() {
    let bytes = std::fs::read(fixture_path()).expect("fixture");
    assert_eq!(sha256_hex(&bytes), FIXTURE_SHA256);
    let set = parse_turn_set(&bytes, FIXTURE_SHA256).expect("parse");
    assert_eq!(set.schema, TURN_SET_SCHEMA);
    let ids: Vec<&str> = set.turns.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["s001", "s002", "s003", "s004"]);
    // Upper-case digest is accepted; a different digest is refused.
    assert!(parse_turn_set(&bytes, &FIXTURE_SHA256.to_ascii_uppercase()).is_ok());
    let wrong = "0".repeat(64);
    let err = parse_turn_set(&bytes, &wrong).expect_err("must refuse");
    assert_eq!(
        err,
        format!("turn set sha256 mismatch: file is {FIXTURE_SHA256}, frozen digest is {wrong}")
    );
}

#[test]
fn turn_set_validation_refuses_bad_sets() {
    let ok_turn = |id: &str, band: u32, fb: &str| {
        serde_json::json!({
            "id": id, "seed": 1, "event_id": "e", "choice_id": "A", "weirdness_level": band,
            "event_seed": "s", "choice_label": "l", "fallback_line": fb
        })
    };
    let refuse = |doc: serde_json::Value| {
        let bytes = doc.to_string().into_bytes();
        parse_turn_set(&bytes, &sha256_hex(&bytes)).expect_err("must refuse")
    };
    assert_eq!(
        refuse(serde_json::json!({"schema": "other", "turns": [ok_turn("a", 0, "x")]})),
        "turn set schema is \"other\", expected \"etv-turn-set-1\""
    );
    assert_eq!(
        refuse(serde_json::json!({"schema": TURN_SET_SCHEMA, "turns": []})),
        "turn set has no turns"
    );
    assert_eq!(
        refuse(
            serde_json::json!({"schema": TURN_SET_SCHEMA, "turns": [ok_turn("a", 0, "x"), ok_turn("a", 1, "x")]})
        ),
        "duplicate turn id \"a\""
    );
    assert_eq!(
        refuse(serde_json::json!({"schema": TURN_SET_SCHEMA, "turns": [ok_turn("a", 4, "x")]})),
        "turn \"a\": weirdness_level 4 is outside 0..=3"
    );
    assert_eq!(
        refuse(serde_json::json!({"schema": TURN_SET_SCHEMA, "turns": [ok_turn("a", 0, "  ")]})),
        "turn \"a\": fallback_line is empty"
    );
    let unknown = serde_json::json!({"schema": TURN_SET_SCHEMA, "turns": [], "extra": 1})
        .to_string()
        .into_bytes();
    assert!(parse_turn_set(&unknown, &sha256_hex(&unknown)).is_err());
}

#[test]
fn turn_contexts_are_rebuilt_from_the_frozen_facts() {
    let bytes = std::fs::read(fixture_path()).expect("fixture");
    let set = parse_turn_set(&bytes, FIXTURE_SHA256).expect("parse");
    let s003 = &set.turns[2];
    let ctx = build_turn_context(s003, GMProfile::Lantern);
    assert_eq!(ctx.event_id(), "f1_004");
    assert_eq!(ctx.choice_id(), "A");
    assert_eq!(ctx.fallback_line(), "The food runs low.");
    assert_eq!(ctx.check_event_seed(), "Mist hangs over the river.");
    assert_eq!(ctx.check_choice_label(), "Investigate the strange lights");
    assert_eq!(ctx.gm_profile(), GMProfile::Lantern);
    assert_eq!(ctx.shown_deltas().get("food"), Some(&-1));
    // The checker band follows the engine allowance: level 3 with tokens is "strong" (band 2),
    // level 2 with tokens is "hint" (band 1), level 0 is band 0, and level 2 with no tokens left
    // is "none" (band 0).
    let band_of = |i: usize| {
        let c = build_turn_context(&set.turns[i], GMProfile::Fireside);
        CheckerContext::from_narration_context(&c, NarratorLength::Brief).weirdness_band
    };
    assert_eq!(
        (band_of(0), band_of(1), band_of(2), band_of(3)),
        (0, 1, 2, 0)
    );
    // Rebuilding twice gives the same facts.
    assert_eq!(build_turn_context(s003, GMProfile::Lantern), ctx);
}

// ---------------------------------------------------------------------------
// Step 5 harness: a whole measurement on a scripted transport
// ---------------------------------------------------------------------------

fn one_cell_config() -> MeasureConfig {
    let mut cfg = MeasureConfig::new("testmodel");
    cfg.profiles = vec![GMProfile::Fireside];
    cfg.lengths = vec![NarratorLength::Standard];
    cfg
}

fn scripted_run_responses() -> Vec<Result<(u16, String), TransportError>> {
    let good_scene = "Mud clings to the spokes, and the team leans into the collars.";
    let good_outcome = "The wagon rolls on.";
    vec![
        warm(),
        // s001: both accepted.
        scene_ok(good_scene),
        outcome_ok(good_outcome),
        // s002: scene times out, outcome accepted.
        Err(TransportError::Timeout),
        outcome_ok(good_outcome),
        // s003: scene is not JSON in the slot, outcome fails the pronoun rule.
        Ok((200, r#"{"response":"not json"}"#.to_string())),
        outcome_ok("The team pulls you through the ford."),
        // s004: scene accepted, outcome is an HTTP 500.
        scene_ok(good_scene),
        Ok((500, "boom".to_string())),
    ]
}

#[test]
fn a_scripted_measurement_counts_every_kind_of_fallback() {
    let bytes = std::fs::read(fixture_path()).expect("fixture");
    let set = parse_turn_set(&bytes, FIXTURE_SHA256).expect("parse");
    let transport = Script::new("testmodel:latest", scripted_run_responses());
    let (report, rows) =
        run_measurement(&set, FIXTURE_SHA256, &one_cell_config(), transport).expect("run");

    assert_eq!(report.schema, "etv-fallback-measure-1");
    assert_eq!(report.turn_set_sha256, FIXTURE_SHA256);
    assert_eq!((report.turns_in_set, report.turns_run), (4, 4));
    assert_eq!(report.model, "testmodel");
    assert_eq!(report.cells.len(), 1);
    let cell = &report.cells[0];
    assert_eq!(
        (cell.profile.as_str(), cell.length.as_str()),
        ("Fireside", "standard")
    );
    // 4 turns x 2 slots = 8. Accepted: s001 x2, s002 outcome, s004 scene = 4.
    assert_eq!(
        (
            cell.turns,
            cell.slots,
            cell.accepted_slots,
            cell.fallback_slots
        ),
        (4, 8, 4, 4)
    );
    // Fallbacks: one timeout, one bad slot JSON, one rule reject, one HTTP error.
    assert_eq!(
        (
            cell.timeouts,
            cell.bad_json,
            cell.rule_rejects,
            cell.transport_or_http_errors,
            cell.not_called
        ),
        (1, 1, 1, 1, 0)
    );
    assert_eq!(
        cell.rejects_by_code,
        BTreeMap::from([("personal_pronoun".to_string(), 1usize)])
    );
    // Turns with a fallback: s002, s003, s004.
    assert_eq!(cell.turns_with_fallback, 3);
    assert_eq!(cell.fallback_rate_slots, 0.5);
    assert_eq!(cell.fallback_rate_turns, 0.75);
    // Every one of the 8 calls has a latency (a timeout is a call too).
    assert_eq!(cell.latency.calls, 8);

    assert_eq!(rows.len(), 4);
    let ids: Vec<&str> = rows.iter().map(|r| r.turn_id.as_str()).collect();
    assert_eq!(ids, vec!["s001", "s002", "s003", "s004"]);
    assert_eq!(rows[0].scene.status, "accepted");
    assert_eq!(
        rows[0].scene.candidate.as_deref(),
        Some("Mud clings to the spokes, and the team leans into the collars.")
    );
    assert_eq!(rows[1].scene.status, "timeout");
    assert_eq!(rows[1].scene.candidate, None);
    assert_eq!(rows[2].scene.status, "bad_slot_json");
    assert_eq!(rows[2].scene.raw_response.as_deref(), Some("not json"));
    assert_eq!(rows[2].outcome.status, "rule_reject");
    assert_eq!(
        rows[2].outcome.reject_code.as_deref(),
        Some("personal_pronoun")
    );
    // Everything the model wrote is kept, including the rejected line.
    assert_eq!(
        rows[2].outcome.candidate.as_deref(),
        Some("The team pulls you through the ford.")
    );
    assert_eq!(rows[3].outcome.status, "http_status");
}

#[test]
fn max_turns_limits_the_run() {
    let bytes = std::fs::read(fixture_path()).expect("fixture");
    let set = parse_turn_set(&bytes, FIXTURE_SHA256).expect("parse");
    let transport = Script::new(
        "testmodel:latest",
        vec![
            warm(),
            scene_ok("The team holds."),
            outcome_ok("The wagon rolls on."),
        ],
    );
    let mut cfg = one_cell_config();
    cfg.max_turns = Some(1);
    let (report, rows) = run_measurement(&set, FIXTURE_SHA256, &cfg, transport).expect("run");
    assert_eq!((report.turns_in_set, report.turns_run), (4, 1));
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (report.cells[0].slots, report.cells[0].accepted_slots),
        (2, 2)
    );
    assert_eq!(report.cells[0].fallback_rate_slots, 0.0);
}

#[test]
fn nine_cells_cover_every_profile_and_length() {
    let bytes = std::fs::read(fixture_path()).expect("fixture");
    let set = parse_turn_set(&bytes, FIXTURE_SHA256).expect("parse");
    // One turn per cell. Brief needs one sentence: both lines are one sentence, and Rich needs
    // two to three, so the Rich cells fall back on length. 9 warm-ups + 18 calls.
    let mut responses = Vec::new();
    for _ in 0..9 {
        responses.push(warm());
        responses.push(scene_ok("The team holds."));
        responses.push(outcome_ok("The wagon rolls on."));
    }
    let transport = Script::new("testmodel:latest", responses);
    let mut cfg = MeasureConfig::new("testmodel");
    cfg.max_turns = Some(1);
    let (report, rows) = run_measurement(&set, FIXTURE_SHA256, &cfg, transport).expect("run");
    let cells: Vec<(String, String)> = report
        .cells
        .iter()
        .map(|c| (c.profile.clone(), c.length.clone()))
        .collect();
    let mut expected = Vec::new();
    for p in ["Chronicler", "Fireside", "Lantern-Bearer"] {
        for l in ["brief", "standard", "rich"] {
            expected.push((p.to_string(), l.to_string()));
        }
    }
    assert_eq!(cells, expected);
    assert_eq!(rows.len(), 9);
    for c in &report.cells {
        if c.length == "rich" {
            assert_eq!((c.accepted_slots, c.rule_rejects), (0, 2), "{}", c.profile);
            assert_eq!(
                c.rejects_by_code,
                BTreeMap::from([("length_cap_exceeded".to_string(), 2usize)])
            );
            assert_eq!(c.fallback_rate_slots, 1.0);
        } else {
            assert_eq!(
                (c.accepted_slots, c.rule_rejects),
                (2, 0),
                "{} {}",
                c.profile,
                c.length
            );
        }
    }
}

#[test]
fn a_missing_model_aborts_instead_of_measuring_nothing() {
    let bytes = std::fs::read(fixture_path()).expect("fixture");
    let set = parse_turn_set(&bytes, FIXTURE_SHA256).expect("parse");
    let transport = Script::new("othermodel:latest", vec![]);
    let err = run_measurement(&set, FIXTURE_SHA256, &one_cell_config(), transport)
        .expect_err("model is not installed");
    assert!(
        err.contains("testmodel"),
        "message should name the model: {err}"
    );
}

// ---------------------------------------------------------------------------
// Step 5 harness: the command line
// ---------------------------------------------------------------------------

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn cli_check_only_verifies_the_digest_and_makes_no_model_call() {
    let path = fixture_path();
    let path = path.to_str().expect("utf8 path");
    // The script is empty: any model call would fail the run.
    let transport = Script::new("none:latest", vec![]);
    let mut out = Vec::new();
    run_cli(
        &args(&[
            "--turn-set",
            path,
            "--turn-set-sha256",
            FIXTURE_SHA256,
            "--check-only",
        ]),
        transport.clone(),
        &mut out,
    )
    .expect("check only");
    assert_eq!(
        String::from_utf8(out).expect("utf8"),
        format!("turn set ok: 4 turns, sha256 {FIXTURE_SHA256}\n")
    );
    assert_eq!(transport.queue.lock().expect("lock").len(), 0);

    let wrong = "f".repeat(64);
    let err = run_cli(
        &args(&[
            "--turn-set",
            path,
            "--turn-set-sha256",
            &wrong,
            "--check-only",
        ]),
        transport,
        &mut Vec::new(),
    )
    .expect_err("refused");
    assert!(err.starts_with("turn set sha256 mismatch"), "{err}");
}

#[test]
fn cli_argument_errors_are_plain() {
    let t = Script::new("m:latest", vec![]);
    let path = fixture_path();
    let path = path.to_str().expect("utf8 path");
    let mut sink = Vec::new();
    assert_eq!(
        run_cli(&args(&[]), t.clone(), &mut sink).expect_err("e"),
        "--turn-set is required"
    );
    assert_eq!(
        run_cli(&args(&["--turn-set", path]), t.clone(), &mut sink).expect_err("e"),
        "--turn-set-sha256 is required"
    );
    assert_eq!(
        run_cli(
            &args(&["--turn-set", path, "--turn-set-sha256", FIXTURE_SHA256]),
            t.clone(),
            &mut sink
        )
        .expect_err("e"),
        "--model is required (there is no default model)"
    );
    assert_eq!(
        run_cli(&args(&["--model"]), t.clone(), &mut sink).expect_err("e"),
        "--model needs a value"
    );
    assert_eq!(
        run_cli(&args(&["--max-turns", "many"]), t.clone(), &mut sink).expect_err("e"),
        "--max-turns needs a whole number"
    );
    assert!(run_cli(&args(&["--bogus"]), t.clone(), &mut sink)
        .expect_err("e")
        .starts_with("unknown argument \"--bogus\""));
    let mut help = Vec::new();
    run_cli(&args(&["--help"]), t, &mut help).expect("help");
    assert!(String::from_utf8(help)
        .expect("utf8")
        .starts_with("narrator_measure:"));
}

#[test]
fn cli_full_run_writes_report_and_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let report_path = dir.path().join("report.json");
    let rows_path = dir.path().join("rows.jsonl");
    let path = fixture_path();
    let transport = Script::new("testmodel:latest", scripted_run_responses());
    let mut out = Vec::new();
    run_cli(
        &args(&[
            "--turn-set",
            path.to_str().expect("utf8 path"),
            "--turn-set-sha256",
            FIXTURE_SHA256,
            "--model",
            "testmodel",
            "--profiles",
            "fireside",
            "--lengths",
            "standard",
            "--report",
            report_path.to_str().expect("utf8 path"),
            "--rows",
            rows_path.to_str().expect("utf8 path"),
        ]),
        transport,
        &mut out,
    )
    .expect("run");

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).expect("report"))
            .expect("json");
    assert_eq!(report["schema"], "etv-fallback-measure-1");
    assert_eq!(report["turn_set_sha256"], FIXTURE_SHA256);
    assert_eq!(report["cells"][0]["fallback_slots"], 4);
    assert_eq!(report["cells"][0]["fallback_rate_slots"], 0.5);
    assert_eq!(
        String::from_utf8(out).expect("utf8").trim_end(),
        std::fs::read_to_string(&report_path).expect("report")
    );

    let rows = std::fs::read_to_string(&rows_path).expect("rows");
    let lines: Vec<&str> = rows.lines().collect();
    assert_eq!(lines.len(), 4);
    let third: serde_json::Value = serde_json::from_str(lines[2]).expect("row json");
    assert_eq!(third["turn_id"], "s003");
    assert_eq!(third["outcome"]["reject_code"], "personal_pronoun");
}
