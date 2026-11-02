//! Step 3 integration tests: Port the live narrator to Rust.
//!
//! Covers:
//! 1. Narrator off: a full game plays to an ending with every turn showing the fallback line.
//! 2. Fallback phrase table & composer: 464 choice fallbacks composed from effects, shipped table
//!    holds only `"PLACEHOLDER-<key>-<dir>"` entries (test fails if any non-placeholder phrase is
//!    present), and release builds (`cfg(not(debug_assertions))`) refuse to start the
//!    composed-fallback path while placeholders are present and show the engine's own plain line.
//! 3. Three profiles (`Chronicler`, `Fireside`, `Lantern-Bearer`), prompt construction from shared
//!    hard rules + `PROFILE_HEADERS` + `NarrationContext` facts, never containing `fallback_line`
//!    and never leaking literal `"lantern"` into the system prompt.
//! 4. Mechanics untouched: with a fake narrator in CI, `mechanics_state_hash` sequences are
//!    identical across all 3 profiles, all 3 lengths, and 2 fake "models" over 200 seeds; only
//!    `presentation_record` differs; Tier 1 is unaffected.
//! 5. Hard-rule checker: table of lines that must pass and lines that must fail (one per reason
//!    code), with failing lines producing the fallback line.
//! 6. Endpoint safety & settings stub: non-loopback endpoints refused unless set explicitly in
//!    settings; default build has no `ureq` in `cargo tree`; `ureq` 3.4.2 (`MIT OR Apache-2.0`)
//!    pinned and listed in the `--notices` allow-list.
//! 7. Timeouts & startup messages: fake slow server triggers per-call timeout and fallback while
//!    the separate warm-up allowance succeeds; missing-model and connection-error messages at
//!    startup match legacy plain-words guidance and offer to play with the narrator off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use etv_core::content::{embedded_content_pack, EMBEDDED_CONTENT_HASH_HEX};
use etv_core::engine::{StepEngine, StepMessages};
use etv_core::model::{GMProfile, GamePhase, IntentAction, PlayerIntent};
use etv_core::narrator::{
    build_narration_context_with_phase, build_outcome_prompts, build_scene_prompts,
    build_system_prompt, check_generated_line, compose_all_464_choice_fallbacks,
    connect_error_message, evaluate_turn_matters, is_loopback_endpoint, model_missing_error,
    parse_outcome_response_strict, parse_scene_response_strict, profile_header,
    profile_interface_name, resolve_fallback_line_with_mode, validate_endpoint, CheckReasonCode,
    CheckerContext, ComposedFallbackComposer, EndpointError, FallbackComposerError,
    FallbackPhraseTable, LiveNarrator, Narrator, NarratorLength, NarratorSettings,
    NarratorTransport, NullNarrator, PreTurnObservation, TransportError, DEFAULT_OLLAMA_ENDPOINT,
    RESOURCE_NOUN_TABLE,
};
use etv_core::notices::embedded_license_allowlist;
use etv_core::save::state_to_value;
use etv_core::snapshot::{
    build_1000_seeds, run_tier2_seed_trace, run_tier2_seed_trace_with_narrator,
};
use etv_core::worldgen::create_new_run_with_content;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

// ============================================================================
// 1. Narrator off: full game plays to an ending with every turn showing fallback
// ============================================================================

#[test]
fn step3_narrator_off_full_game_shows_fallback_line_every_turn() {
    let pack = embedded_content_pack();
    let trace = run_tier2_seed_trace(0, 0, pack, 200);
    assert_eq!(
        trace.turns.len(),
        93,
        "seed 0 should play to a full game ending at turn 93"
    );

    // Also run step-by-step with both NullNarrator and a disabled LiveNarrator to verify
    // every turn resolves to its non-empty fallback_line and reaches an EndingResult.
    let state = create_new_run_with_content(0, pack);
    let mut engine = StepEngine::with_content(state, pack);
    let disabled_settings = NarratorSettings {
        enabled: false,
        ..Default::default()
    };
    let disabled_narrator = LiveNarrator::new_offline(disabled_settings);

    for (idx, snap) in trace.turns.iter().enumerate() {
        let turn_num = idx + 1;
        assert_eq!(snap.turn, turn_num);
        assert_eq!(
            snap.presentation_record.narrator_text, None,
            "NullNarrator must emit None on turn {turn_num}"
        );
        assert_eq!(
            snap.presentation_record.narrator_scene_text, None,
            "NullNarrator must emit None scene text on turn {turn_num}"
        );
        assert!(
            !snap.presentation_record.displayed_line.is_empty(),
            "displayed fallback line must be non-empty on turn {turn_num}"
        );

        let pre_obs = PreTurnObservation::capture(engine.phase, &engine.state);
        let pre_ev = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or_default();
        let intent = match engine.phase {
            GamePhase::Event => {
                let choices = engine.pending_event_choices();
                let cidx = turn_num % choices.len();
                let cid = choices[cidx].id.clone();
                PlayerIntent::new(IntentAction::Choose).with_choice(&cid)
            }
            GamePhase::Route => {
                let routes = engine.pending_routes();
                let count = routes.len().clamp(1, 4);
                let cid = ["A", "B", "C", "D"][turn_num % count];
                PlayerIntent::new(IntentAction::Choose).with_choice(cid)
            }
            GamePhase::Camp => {
                let selector = turn_num % 12;
                match selector {
                    0 if turn_num == 1 => {
                        PlayerIntent::new(IntentAction::ChangePace).with_pace("steady")
                    }
                    3 if engine.state.supplies.ammo() > 0 && engine.state.supplies.food() < 35 => {
                        PlayerIntent::new(IntentAction::Hunt)
                    }
                    6 if engine.state.wagon.condition < 75 && engine.state.supplies.parts() > 0 => {
                        PlayerIntent::new(IntentAction::Repair)
                    }
                    9 if engine.state.party.morale < 65 => PlayerIntent::new(IntentAction::Rest),
                    _ => PlayerIntent::new(IntentAction::Travel),
                }
            }
            GamePhase::GameOver => PlayerIntent::new(IntentAction::Travel),
        };
        let chosen_cid = intent.choice_id.clone();
        let msgs = engine.step(&intent);
        let post_ev = engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or(pre_ev);
        let flags = evaluate_turn_matters(&pre_obs, &intent, &msgs, &engine.state);
        let ctx = build_narration_context_with_phase(
            &engine.state,
            engine.phase,
            &post_ev,
            &chosen_cid,
            &msgs,
            flags,
        );

        assert_eq!(NullNarrator.narrate(&ctx), None);
        assert_eq!(disabled_narrator.narrate(&ctx), None);
        assert_eq!(ctx.resolve_display_line(&NullNarrator), ctx.fallback_line());
        assert_eq!(
            ctx.resolve_display_line(&disabled_narrator),
            ctx.fallback_line()
        );
        assert_eq!(snap.presentation_record.displayed_line, ctx.fallback_line());
    }

    assert!(engine.state.game_over, "full game must reach game_over");
    assert!(
        engine.state.ending.is_some(),
        "full game must produce an EndingResult"
    );
    assert!(
        !trace
            .turns
            .last()
            .unwrap()
            .presentation_record
            .trail_ledger_lines
            .is_empty(),
        "terminal turn must include Trail Ledger lines"
    );
}

// ============================================================================
// 2. Fallback phrase table & 464 choice composer + release-build refusal
// ============================================================================

#[test]
fn step3_fallback_table_ships_only_placeholder_entries() {
    let table = FallbackPhraseTable::embedded();
    assert!(
        table.all_placeholders(),
        "shipped fallback_phrases.json must contain only PLACEHOLDER-<key>-<dir> entries, found non-placeholders: {:?}",
        table.non_placeholder_entries()
    );
    assert!(table.has_placeholders());
    assert!(table.non_placeholder_entries().is_empty());

    // Verify negative check: a table containing any non-placeholder phrase must be caught.
    let non_placeholder_json = r#"{
        "schema_version": 1,
        "phrases": {
            "food": {
                "neg": "NOT_A_PLACEHOLDER",
                "pos": "PLACEHOLDER-food-pos"
            }
        }
    }"#;
    let tampered =
        FallbackPhraseTable::from_json_str(non_placeholder_json).expect("parse tampered table");
    assert!(
        !tampered.all_placeholders(),
        "table with non-placeholder entry must fail all_placeholders()"
    );
    assert_eq!(
        tampered.non_placeholder_entries(),
        vec![(
            "food".to_string(),
            "neg".to_string(),
            "NOT_A_PLACEHOLDER".to_string()
        )]
    );
}

#[test]
fn step3_fallback_composer_464_choices_and_release_build_refusal() {
    let pack = embedded_content_pack();
    let table = FallbackPhraseTable::embedded();

    let composed = compose_all_464_choice_fallbacks(pack, &table);
    assert_eq!(
        composed.len(),
        464,
        "must compose one fallback sentence for each of the 464 JSON event choices"
    );
    for ((event_id, choice_id), sentence) in &composed {
        assert!(
            sentence.starts_with("PLACEHOLDER-") && sentence.ends_with('.'),
            "composed fallback for {event_id}:{choice_id} must use placeholder phrases and end with a period, got: {sentence:?}"
        );
    }

    // Release build (`is_release_build = true` / `cfg(not(debug_assertions))`) must refuse to
    // start the composed-fallback path while the table holds placeholders or is missing, and
    // must show the engine's own plain line instead.
    assert_eq!(
        ComposedFallbackComposer::try_start_with_mode(Some(&table), true).unwrap_err(),
        FallbackComposerError::PlaceholdersInReleaseBuild
    );
    assert_eq!(
        ComposedFallbackComposer::try_start_with_mode(None, true).unwrap_err(),
        FallbackComposerError::TableMissing
    );

    let state = create_new_run_with_content(42, pack);
    let mut msgs = StepMessages {
        outcome_narration: "-2 food, +1 morale".to_string(),
        ..Default::default()
    };
    msgs.outcome_deltas.insert("food".to_string(), -2);
    let flags = evaluate_turn_matters(
        &PreTurnObservation::capture(GamePhase::Camp, &state),
        &PlayerIntent::new(IntentAction::Choose).with_choice("A"),
        &msgs,
        &state,
    );
    let ctx =
        build_narration_context_with_phase(&state, GamePhase::Camp, "f1_001", "A", &msgs, flags);

    let release_line = resolve_fallback_line_with_mode(&ctx, Some(&table), true);
    assert_eq!(
        release_line,
        ctx.fallback_line(),
        "release build must refuse placeholder table and return engine's plain fallback_line"
    );
    assert!(
        !release_line.contains("PLACEHOLDER-"),
        "placeholders must never reach a player in a release build"
    );

    let missing_table_line = resolve_fallback_line_with_mode(&ctx, None, false);
    assert_eq!(missing_table_line, ctx.fallback_line());

    let debug_composed_line = resolve_fallback_line_with_mode(&ctx, Some(&table), false);
    assert!(
        debug_composed_line.contains("PLACEHOLDER-food-neg"),
        "debug mode with placeholder table composes from effects, got: {debug_composed_line}"
    );
}

// ============================================================================
// 3. Profiles & Prompt Hygiene (no fallback_line, no literal "lantern" leak)
// ============================================================================

#[test]
fn step3_profile_prompts_contain_rules_voice_and_facts_never_fallback_or_lantern_leak() {
    let pack = embedded_content_pack();
    let mut state = create_new_run_with_content(42, pack);
    state.supplies.set_food(12);
    state.recent_event_tags = vec!["river".to_string(), "folklore".to_string()];

    let mut msgs = StepMessages {
        outcome_narration: "SENTINEL_FALLBACK_LINE_NEVER_IN_PROMPT_98765".to_string(),
        lines: vec!["SENTINEL_FALLBACK_LINE_NEVER_IN_PROMPT_98765".to_string()],
        ..Default::default()
    };
    msgs.outcome_deltas.insert("rope".to_string(), -1);

    let flags = evaluate_turn_matters(
        &PreTurnObservation::capture(GamePhase::Event, &state),
        &PlayerIntent::new(IntentAction::Choose).with_choice("A"),
        &msgs,
        &state,
    );
    let ctx =
        build_narration_context_with_phase(&state, GamePhase::Event, "f1_003", "A", &msgs, flags);
    assert_eq!(
        ctx.fallback_line(),
        "SENTINEL_FALLBACK_LINE_NEVER_IN_PROMPT_98765"
    );

    assert_eq!(profile_interface_name(GMProfile::Chronicler), "Chronicler");
    assert_eq!(profile_interface_name(GMProfile::Fireside), "Fireside");
    assert_eq!(profile_interface_name(GMProfile::Lantern), "Lantern-Bearer");

    for profile in [
        GMProfile::Chronicler,
        GMProfile::Fireside,
        GMProfile::Lantern,
    ] {
        for length in [
            NarratorLength::Brief,
            NarratorLength::Standard,
            NarratorLength::Rich,
        ] {
            let sys = build_system_prompt(profile, length, &ctx);
            let (scene_sys, scene_user) = build_scene_prompts(profile, length, &ctx);
            let (out_sys, out_user) = build_outcome_prompts(profile, length, &ctx);

            // (a) Shared hard rules present
            assert!(
                sys.contains("SHARED VOICE HARD RULES"),
                "system prompt must include shared hard rules section"
            );
            // (b) Per-profile voice header present
            assert!(
                sys.contains(profile_header(profile)),
                "system prompt must include profile header for {profile:?}"
            );
            // (c) Turn facts from NarrationContext present
            assert!(sys.contains("f1_003"), "prompt must include event_id");
            assert!(sys.contains(ctx.place()), "prompt must include place");
            assert!(sys.contains(ctx.band()), "prompt must include journey band");
            assert!(sys.contains(ctx.biome()), "prompt must include biome");
            assert!(
                sys.contains(ctx.party_status().health_band()),
                "prompt must include party health_band"
            );
            assert!(
                sys.contains(ctx.party_status().morale_band()),
                "prompt must include party morale_band"
            );

            // Never fallback_line
            for text in [&sys, &scene_sys, &scene_user, &out_sys, &out_user] {
                assert!(
                    !text.contains(ctx.fallback_line()),
                    "prompt must NEVER contain fallback_line ({profile:?}, {length:?})"
                );
            }

            // No party member names
            for member in &state.party.members {
                assert!(
                    !sys.contains(&member.name),
                    "prompt must not contain party member name '{}'",
                    member.name
                );
            }

            // Prompt hygiene: literal word "lantern" must not appear in the system prompt
            // when lantern_oil is not in the turn's effects.
            assert!(
                !sys.to_lowercase().contains("lantern"),
                "system prompt for {profile:?} must not contain the literal word 'lantern': {sys}"
            );
        }
    }
}

// ============================================================================
// 4. Mechanics untouched: 3 profiles × 3 lengths × 2 fake models over 200 seeds
// ============================================================================

struct FakePlaceholderModelTransport {
    model_tag: &'static str,
    calls: AtomicUsize,
}

impl FakePlaceholderModelTransport {
    fn new(model_tag: &'static str) -> Self {
        Self {
            model_tag,
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn build_placeholder_prose(
        &self,
        profile: GMProfile,
        length: NarratorLength,
        slot: &str,
    ) -> String {
        let prof_word = match profile {
            GMProfile::Chronicler => "chronicler",
            GMProfile::Fireside => "fireside",
            GMProfile::Lantern => "uncanny",
        };
        let model_word = self.model_tag;
        let s1 = format!("Placeholder {slot} {prof_word} {model_word} first clause holds steady.");
        let s2 = format!("Placeholder {slot} {prof_word} {model_word} second clause adds texture.");
        let s3 =
            format!("Placeholder {slot} {prof_word} {model_word} third clause closes the beat.");
        match length {
            NarratorLength::Brief => s1,
            NarratorLength::Standard => format!("{s1} {s2}"),
            NarratorLength::Rich => format!("{s1} {s2} {s3}"),
        }
    }
}

impl NarratorTransport for FakePlaceholderModelTransport {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Ok((
            200,
            r#"{"models":[{"name":"fake-alpha:latest"},{"name":"fake-beta:latest"}]}"#.to_string(),
        ))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let profile = if body_json.contains("The Chronicler") {
            GMProfile::Chronicler
        } else if body_json.contains("The Uncanny Bearer") {
            GMProfile::Lantern
        } else {
            GMProfile::Fireside
        };
        let length = if body_json.contains("LENGTH CAP: Brief") {
            NarratorLength::Brief
        } else if body_json.contains("LENGTH CAP: Rich") {
            NarratorLength::Rich
        } else {
            NarratorLength::Standard
        };

        let is_scene = body_json.contains("SCENE_SCHEMA") || body_json.contains("scene response");
        let inner_json = if is_scene {
            let prose = self.build_placeholder_prose(profile, length, "scene");
            serde_json::json!({
                "scene_id": "placeholder_scene",
                "title": "Placeholder",
                "narration": prose,
                "profile": profile.as_str(),
                "uncanny_intensity": "none",
                "choices": [
                    {"id": "A", "label": "Placeholder A", "intent": {"action": "TRAVEL", "style": "NEUTRAL", "notes": ""}, "risk_hint": "", "cost_hint": ""},
                    {"id": "B", "label": "Placeholder B", "intent": {"action": "REST", "style": "CAUTIOUS", "notes": ""}, "risk_hint": "", "cost_hint": ""}
                ],
                "tags": [],
                "gm_aside": "",
                "memory_proposals": []
            })
        } else {
            let prose = self.build_placeholder_prose(profile, length, "outcome");
            serde_json::json!({
                "scene_id": "placeholder_scene",
                "outcome_title": "Placeholder",
                "outcome_narration": prose,
                "callout": "Placeholder callout.",
                "oregon_nod": "",
                "memory_proposals": []
            })
        };

        let outer = serde_json::json!({
            "response": inner_json.to_string()
        });
        Ok((200, outer.to_string()))
    }
}

#[test]
fn step3_mechanics_untouched_3_profiles_3_lengths_2_fake_models_200_seeds() {
    let pack = embedded_content_pack();
    assert_eq!(pack.content_hash_hex, EMBEDDED_CONTENT_HASH_HEX);

    let seeds = &build_1000_seeds()[..200];
    assert_eq!(seeds.len(), 200);
    let max_turns = 15;

    let baseline_traces: Vec<_> = seeds
        .iter()
        .enumerate()
        .map(|(idx, &seed)| {
            run_tier2_seed_trace_with_narrator(seed, idx, pack, max_turns, &NullNarrator)
        })
        .collect();

    let profiles = [
        GMProfile::Chronicler,
        GMProfile::Fireside,
        GMProfile::Lantern,
    ];
    let lengths = [
        NarratorLength::Brief,
        NarratorLength::Standard,
        NarratorLength::Rich,
    ];
    let models: [(&str, &'static str); 2] = [("fake-alpha", "alpha"), ("fake-beta", "beta")];

    let mut first_combo_presentation = Vec::new();
    let mut distinct_presentation_combos = 0usize;

    for &profile in &profiles {
        for &length in &lengths {
            for &(model_name, model_tag) in &models {
                let transport = Arc::new(FakePlaceholderModelTransport::new(model_tag));
                let settings = NarratorSettings {
                    profile,
                    length,
                    model: model_name.to_string(),
                    ..Default::default()
                };
                let narrator = LiveNarrator::with_transport(settings, transport.clone());

                let mut total_turns_for_combo = 0usize;
                let mut combo_sample_texts = Vec::new();

                for (idx, &seed) in seeds.iter().enumerate() {
                    let trace =
                        run_tier2_seed_trace_with_narrator(seed, idx, pack, max_turns, &narrator);
                    let base = &baseline_traces[idx];
                    assert_eq!(trace.seed, base.seed);
                    assert_eq!(trace.turns.len(), base.turns.len());

                    for (b_turn, n_turn) in base.turns.iter().zip(trace.turns.iter()) {
                        assert_eq!(b_turn.turn, n_turn.turn);
                        assert_eq!(
                            b_turn.mechanics_state_hash,
                            n_turn.mechanics_state_hash,
                            "mechanics_state_hash diverged on seed {seed} turn {} for ({profile:?}, {length:?}, {model_name})",
                            b_turn.turn
                        );
                        assert!(
                            n_turn.presentation_record.narrator_scene_text.is_some(),
                            "scene narration must be populated on seed {seed} turn {}",
                            b_turn.turn
                        );
                        assert!(
                            n_turn.presentation_record.narrator_text.is_some(),
                            "outcome narration must be populated on seed {seed} turn {}",
                            b_turn.turn
                        );
                        assert_ne!(
                            b_turn.presentation_record, n_turn.presentation_record,
                            "presentation_record must differ from NullNarrator"
                        );
                        if idx == 0 && b_turn.turn == 1 {
                            combo_sample_texts.push(
                                n_turn
                                    .presentation_record
                                    .narrator_text
                                    .clone()
                                    .unwrap_or_default(),
                            );
                        }
                    }
                    total_turns_for_combo += trace.turns.len();
                }

                // Two calls per turn (scene + outcome)
                assert_eq!(
                    transport.call_count(),
                    total_turns_for_combo * 2,
                    "LiveNarrator must make exactly 2 calls per shown turn (scene and outcome)"
                );

                if first_combo_presentation.is_empty() {
                    first_combo_presentation = combo_sample_texts;
                } else if combo_sample_texts != first_combo_presentation {
                    distinct_presentation_combos += 1;
                }
            }
        }
    }

    assert_eq!(
        distinct_presentation_combos,
        17,
        "all 18 (profile, length, model) combinations must produce distinct presentation_record text"
    );
}

// ============================================================================
// 5. Hard-rule checker: pass table, fail table (1+ per reason code), fallback
// ============================================================================

struct FixedSingleLineTransport {
    scene_json: String,
    outcome_json: String,
}

impl NarratorTransport for FixedSingleLineTransport {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Ok((
            200,
            r#"{"models":[{"name":"llama3.2:latest"}]}"#.to_string(),
        ))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        let is_scene = body_json.contains("SCENE_SCHEMA") || body_json.contains("scene response");
        let resp = if is_scene {
            &self.scene_json
        } else {
            &self.outcome_json
        };
        let outer = serde_json::json!({ "response": resp });
        Ok((200, outer.to_string()))
    }
}

#[test]
fn step3_hard_rule_checker_pass_and_fail_table_and_fallback_on_failure() {
    assert!(
        RESOURCE_NOUN_TABLE.len() >= 10,
        "reviewed resource noun table must cover the engine supply keys"
    );

    let mut base_ctx = CheckerContext {
        allowed_resources: vec!["rope".to_string()],
        allowed_landmarks: vec!["Millford".to_string(), "Widow's Pass".to_string()],
        weirdness_band: 0,
        length: NarratorLength::Standard,
    };

    // Table of lines that MUST PASS
    let pass_cases: Vec<(&str, NarratorLength, u8)> = vec![
        (
            "Placeholder wagon rolls over wet gravel, and the team holds steady.",
            NarratorLength::Brief,
            0,
        ),
        (
            "Placeholder one wheel drops into the rut, and both axles groan behind the lead pair.",
            NarratorLength::Standard,
            0,
        ),
        (
            "Placeholder rope holds the timber tight. Placeholder team leans into the traces.",
            NarratorLength::Standard,
            0,
        ),
        (
            "Placeholder wagon reaches Millford before dusk. Placeholder canvas stays dry at Widow's Pass.",
            NarratorLength::Standard,
            0,
        ),
        (
            "Placeholder cold reeds part around dry stones. Placeholder water stays still. Placeholder wind drops before dawn.",
            NarratorLength::Rich,
            2,
        ),
    ];

    for (line, length, band) in pass_cases {
        base_ctx.length = length;
        base_ctx.weirdness_band = band;
        assert_eq!(
            check_generated_line(line, &base_ctx),
            Ok(()),
            "expected line to PASS checker: {line:?}"
        );
    }

    // Table of lines that MUST FAIL, covering every CheckReasonCode
    let fail_cases: Vec<(&str, NarratorLength, u8, CheckReasonCode)> = vec![
        // 1. NumbersDigitsOrUnits (digit, number word, unit)
        (
            "Placeholder wagon rolls 3 steps.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::NumbersDigitsOrUnits,
        ),
        (
            "Placeholder three wheels drop into the mud.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::NumbersDigitsOrUnits,
        ),
        (
            "Placeholder wagon rolls for miles across the mud.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::NumbersDigitsOrUnits,
        ),
        // 2. QuantifierBeforeResourceOrHalf ("one", "both", "pair" before resource, and "half")
        (
            "Placeholder one rope holds the timber.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::QuantifierBeforeResourceOrHalf,
        ),
        (
            "Placeholder one sack of flour sits in the bed.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::QuantifierBeforeResourceOrHalf,
        ),
        (
            "Placeholder both ropes hold the timber.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::QuantifierBeforeResourceOrHalf,
        ),
        (
            "Placeholder pair of boots sits in the bed.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::QuantifierBeforeResourceOrHalf,
        ),
        (
            "Placeholder half the canvas tears in the wind.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::QuantifierBeforeResourceOrHalf,
        ),
        // 3. DeathWords
        (
            "Placeholder beast died in the mud.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::DeathWords,
        ),
        // 4. MidSentenceCapital
        (
            "Placeholder wagon passes Elias in the mud.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::MidSentenceCapital,
        ),
        // 5. UnauthorisedResourceNoun (flour/lantern/boots not in allowed_resources=["rope"])
        (
            "Placeholder flour spills across the bed.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::UnauthorisedResourceNoun,
        ),
        (
            "Placeholder lantern swings from the pole.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::UnauthorisedResourceNoun,
        ),
        // 6. BandBannedVocabulary
        (
            "Placeholder river whispers with malice.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::BandBannedVocabulary,
        ),
        (
            "Placeholder disembodied voice calls from the trees.",
            NarratorLength::Standard,
            1,
            CheckReasonCode::BandBannedVocabulary,
        ),
        (
            "Placeholder eldritch ghost stands on the bank.",
            NarratorLength::Standard,
            2,
            CheckReasonCode::BandBannedVocabulary,
        ),
        (
            "Placeholder monster casts a magic spell.",
            NarratorLength::Standard,
            3,
            CheckReasonCode::BandBannedVocabulary,
        ),
        // 7. InvalidPunctuation
        (
            "Placeholder wagon rolls; the team holds.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::InvalidPunctuation,
        ),
        (
            "Placeholder wagon rolls!",
            NarratorLength::Standard,
            0,
            CheckReasonCode::InvalidPunctuation,
        ),
        (
            "Placeholder wagon rolls...",
            NarratorLength::Standard,
            0,
            CheckReasonCode::InvalidPunctuation,
        ),
        // 8. LengthCapExceeded
        (
            "Placeholder first sentence. Placeholder second sentence.",
            NarratorLength::Brief,
            0,
            CheckReasonCode::LengthCapExceeded,
        ),
        (
            "Placeholder first. Placeholder second. Placeholder third.",
            NarratorLength::Standard,
            0,
            CheckReasonCode::LengthCapExceeded,
        ),
        (
            "Placeholder single sentence only.",
            NarratorLength::Rich,
            0,
            CheckReasonCode::LengthCapExceeded,
        ),
    ];

    let pack = embedded_content_pack();
    let mut state = create_new_run_with_content(42, pack);
    let mut msgs = StepMessages {
        outcome_narration: "PLAIN_ENGINE_FALLBACK_LINE".to_string(),
        ..Default::default()
    };
    msgs.outcome_deltas.insert("rope".to_string(), -1);
    let flags = evaluate_turn_matters(
        &PreTurnObservation::capture(GamePhase::Camp, &state),
        &PlayerIntent::new(IntentAction::Travel),
        &msgs,
        &state,
    );

    for (bad_line, length, band, expected_code) in fail_cases {
        base_ctx.length = length;
        base_ctx.weirdness_band = band;
        let err = check_generated_line(bad_line, &base_ctx)
            .expect_err(&format!("expected line to FAIL checker: {bad_line:?}"));
        assert_eq!(
            err.code, expected_code,
            "wrong reason code for failing line {bad_line:?}"
        );

        state.weirdness_level = i32::from(band);
        let turn_ctx = build_narration_context_with_phase(
            &state,
            GamePhase::Camp,
            "f1_001",
            "A",
            &msgs,
            flags,
        );

        // Verify that a failing line in LiveNarrator produces the fallback line.
        let bad_scene = serde_json::json!({
            "scene_id": "s1",
            "title": "Placeholder",
            "narration": bad_line,
            "profile": "fireside",
            "uncanny_intensity": "none",
            "choices": [],
            "tags": [],
            "gm_aside": "",
            "memory_proposals": []
        })
        .to_string();
        let bad_outcome = serde_json::json!({
            "scene_id": "s1",
            "outcome_title": "Placeholder",
            "outcome_narration": bad_line,
            "callout": "",
            "oregon_nod": "",
            "memory_proposals": []
        })
        .to_string();

        let settings = NarratorSettings {
            length,
            ..Default::default()
        };
        let narrator = LiveNarrator::with_transport(
            settings,
            Arc::new(FixedSingleLineTransport {
                scene_json: bad_scene,
                outcome_json: bad_outcome,
            }),
        );
        let turn_res = narrator.narrate_turn(&turn_ctx);
        assert_eq!(
            turn_res.scene, None,
            "failing scene line must be rejected: {bad_line:?}"
        );
        assert_eq!(
            turn_res.outcome, None,
            "failing outcome line must be rejected: {bad_line:?}"
        );
        assert_eq!(
            turn_ctx.resolve_display_line(&narrator),
            "PLAIN_ENGINE_FALLBACK_LINE",
            "failing line must produce the fallback line"
        );
        assert!(narrator.stats().rule_rejects >= 2);
    }

    // Strict JSON parsing check: malformed JSON counts as a failure and produces fallback.
    assert!(parse_scene_response_strict("not json at all").is_err());
    assert!(parse_scene_response_strict("```json\n{\"narration\":\"Placeholder.\"}\n```").is_err());
    assert!(parse_outcome_response_strict("{\"outcome_narration\": 123}").is_err());
    assert!(parse_outcome_response_strict("{}").is_err());
}

// ============================================================================
// 6. Endpoint safety, settings stub, and no ureq in default cargo tree
// ============================================================================

#[test]
fn step3_endpoint_safety_refuses_non_loopback_and_default_build_has_no_ureq() {
    assert_eq!(DEFAULT_OLLAMA_ENDPOINT, "http://127.0.0.1:11434");

    // Loopback endpoints accepted by default
    for valid_loopback in [
        "http://127.0.0.1:11434",
        "http://127.0.0.1:8080/",
        "http://localhost:11434",
        "http://[::1]:11434",
    ] {
        assert!(
            is_loopback_endpoint(valid_loopback),
            "{valid_loopback} must be recognized as loopback"
        );
        assert!(validate_endpoint(valid_loopback, false).is_ok());
    }

    // Non-loopback endpoints refused unless set explicitly
    for non_loopback in [
        "http://192.168.1.50:11434",
        "http://10.0.0.5:11434",
        "https://api.example.com/v1",
        "http://0.0.0.0:11434",
        "http://localhost.evil.example:11434",
        "http://127.0.0.1@evil.example:11434",
    ] {
        assert!(
            !is_loopback_endpoint(non_loopback),
            "{non_loopback} must NOT be treated as loopback"
        );
        assert_eq!(
            validate_endpoint(non_loopback, false),
            Err(EndpointError::NonLoopbackRefused(non_loopback.to_string()))
        );
    }

    // Explicit opt-in in settings permits a valid non-loopback endpoint (still rejecting @ userinfo)
    assert!(validate_endpoint("http://192.168.1.50:11434", true).is_ok());
    assert!(validate_endpoint("http://127.0.0.1@evil.example:11434", true).is_err());

    // Settings stub loads from player's settings file, never from save or content_hash
    let tmp = tempfile::tempdir().expect("tempdir");
    let settings_path = tmp.path().join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{
            "profile": "lantern",
            "endpoint": "http://127.0.0.1:11434",
            "model": "qwen3:8b",
            "length": "rich"
        }"#,
    )
    .unwrap();
    let loaded = NarratorSettings::load_from_file(&settings_path).expect("load settings");
    assert_eq!(loaded.profile, GMProfile::Lantern);
    assert_eq!(loaded.endpoint, "http://127.0.0.1:11434");
    assert_eq!(loaded.model, "qwen3:8b");
    assert_eq!(loaded.length, NarratorLength::Rich);

    // Non-loopback in settings file without explicit allow_non_loopback is refused
    let bad_settings_path = tmp.path().join("bad_settings.json");
    std::fs::write(
        &bad_settings_path,
        r#"{"endpoint": "http://192.168.1.99:11434"}"#,
    )
    .unwrap();
    assert!(NarratorSettings::load_from_file(&bad_settings_path).is_err());

    // Save JSON and content_hash never include settings fields (endpoint, model, length)
    let state = create_new_run_with_content(42, embedded_content_pack());
    let save_val = state_to_value(&state);
    let save_obj = save_val.as_object().unwrap();
    assert!(!save_obj.contains_key("endpoint"));
    assert!(!save_obj.contains_key("model"));
    assert!(!save_obj.contains_key("length"));
    assert_eq!(
        embedded_content_pack().content_hash_hex,
        EMBEDDED_CONTENT_HASH_HEX
    );

    // Verify ureq 3.4.2 is pinned and listed in license_allowlist.json
    let allowlist = embedded_license_allowlist();
    assert!(
        allowlist.reviewed_crates.iter().any(|c| {
            c.name == "ureq" && c.version == "3.4.2" && c.license == "MIT OR Apache-2.0"
        }),
        "ureq 3.4.2 (MIT OR Apache-2.0) must be listed in license_allowlist.json reviewed_crates"
    );

    // Default build has no `ureq` in its dependency tree (`cargo tree` with default features)
    let manifest_path = repo_root().join("rust/Cargo.toml");
    let tree_out = Command::new("cargo")
        .args([
            "tree",
            "--manifest-path",
            &manifest_path.to_string_lossy(),
            "--offline",
            "-e",
            "normal",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run cargo tree with default features");
    assert!(
        tree_out.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&tree_out.stderr)
    );
    let tree_stdout = String::from_utf8(tree_out.stdout).expect("utf8 cargo tree");
    assert!(
        !tree_stdout.contains("ureq"),
        "default build must have NO ureq in its dependency tree, got:\n{tree_stdout}"
    );
}

// ============================================================================
// 7. Timeouts (warm-up vs per-call) & startup missing-model/connection messages
// ============================================================================

struct FakeSlowServerTransport {
    simulated_delay: Duration,
}

impl NarratorTransport for FakeSlowServerTransport {
    fn get_tags(
        &self,
        _endpoint: &str,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        if self.simulated_delay > timeout {
            return Err(TransportError::Timeout);
        }
        Ok((
            200,
            r#"{"models":[{"name":"llama3.2:latest"}]}"#.to_string(),
        ))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        _body_json: &str,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        if self.simulated_delay > timeout {
            return Err(TransportError::Timeout);
        }
        let ok_resp = serde_json::json!({
            "response": serde_json::json!({
                "scene_id": "s1",
                "title": "Placeholder",
                "narration": "Placeholder wagon rolls steady.",
                "outcome_title": "Placeholder",
                "outcome_narration": "Placeholder wagon rolls steady.",
                "profile": "fireside",
                "uncanny_intensity": "none",
                "choices": [],
                "tags": [],
                "gm_aside": "",
                "callout": "",
                "oregon_nod": "",
                "memory_proposals": []
            }).to_string()
        });
        Ok((200, ok_resp.to_string()))
    }
}

struct FakeConnectionRefusedTransport;

impl NarratorTransport for FakeConnectionRefusedTransport {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Err(TransportError::ConnectionFailed(
            "connection refused".to_string(),
        ))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        _body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Err(TransportError::ConnectionFailed(
            "connection refused".to_string(),
        ))
    }
}

#[test]
fn step3_timeouts_fake_slow_server_triggers_per_call_fallback_and_separate_warmup_and_startup_messages(
) {
    // Verify Director's default timeout settings: 30s warm-up, 10s per call
    let default_settings = NarratorSettings::default();
    assert_eq!(default_settings.warmup_timeout_ms, 30_000);
    assert_eq!(default_settings.per_call_timeout_ms, 10_000);

    // Fake slow server with 120ms delay:
    // - warm-up allowance is 300ms (so warm-up succeeds!)
    // - per-call timeout is 40ms (so scene and outcome calls time out and trigger fallback!)
    let settings = NarratorSettings {
        warmup_timeout_ms: 300,
        per_call_timeout_ms: 40,
        ..Default::default()
    };
    let slow_transport = Arc::new(FakeSlowServerTransport {
        simulated_delay: Duration::from_millis(120),
    });
    let narrator = LiveNarrator::with_transport(settings, slow_transport);

    assert!(
        narrator.warmup().is_ok(),
        "warm-up must succeed within its separate 300ms warm-up allowance"
    );
    assert!(narrator.is_warmed_up());

    let pack = embedded_content_pack();
    let state = create_new_run_with_content(42, pack);
    let msgs = StepMessages {
        outcome_narration: "FALLBACK_AFTER_PER_CALL_TIMEOUT".to_string(),
        ..Default::default()
    };
    let flags = evaluate_turn_matters(
        &PreTurnObservation::capture(GamePhase::Camp, &state),
        &PlayerIntent::new(IntentAction::Travel),
        &msgs,
        &state,
    );
    let ctx = build_narration_context_with_phase(&state, GamePhase::Camp, "", "", &msgs, flags);

    let turn_res = narrator.narrate_turn(&ctx);
    assert_eq!(turn_res.scene, None, "slow scene call must time out");
    assert_eq!(turn_res.outcome, None, "slow outcome call must time out");
    assert_eq!(
        narrator.stats().timeouts,
        2,
        "both scene and outcome timeouts must be recorded on the turn"
    );
    assert_eq!(
        ctx.resolve_display_line(&narrator),
        "FALLBACK_AFTER_PER_CALL_TIMEOUT",
        "turn must show fallback line after per-call timeout"
    );
    let _ = BTreeMap::<String, i32>::new();

    // Missing model at start: plain words + offer to play with narrator off
    let missing_model_settings = NarratorSettings {
        model: "qwen3:8b".to_string(),
        ..Default::default()
    };
    let missing_model_narrator = LiveNarrator::with_transport(
        missing_model_settings,
        Arc::new(FakeSlowServerTransport {
            simulated_delay: Duration::from_millis(1),
        }),
    );
    let status_missing = missing_model_narrator.check_startup();
    assert!(status_missing.installed);
    assert!(!status_missing.available);
    assert!(status_missing.offer_narrator_off);
    assert_eq!(
        status_missing.message.as_deref(),
        Some(model_missing_error("qwen3:8b", "").as_str())
    );
    assert!(status_missing
        .message
        .as_deref()
        .unwrap()
        .contains("ollama pull qwen3:8b, or trail tui --gm-off"));

    // Missing connection at start: plain words + offer to play with narrator off
    let down_narrator = LiveNarrator::with_transport(
        NarratorSettings::default(),
        Arc::new(FakeConnectionRefusedTransport),
    );
    let status_down = down_narrator.check_startup();
    assert!(!status_down.installed);
    assert!(!status_down.available);
    assert!(status_down.offer_narrator_off);
    assert_eq!(
        status_down.message.as_deref(),
        Some(connect_error_message().as_str())
    );
    assert!(status_down
        .message
        .as_deref()
        .unwrap()
        .contains("The storyteller is not running."));
    assert!(status_down
        .message
        .as_deref()
        .unwrap()
        .contains("trail tui --gm-off"));
}
