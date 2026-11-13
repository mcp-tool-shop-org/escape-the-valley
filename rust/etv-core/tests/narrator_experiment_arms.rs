//! The narrator's experiment arms: all off by default, switched on only by the measurement
//! harness. Each test names what an arm sends and what it accepts.
//!
//! No model is called anywhere in this file.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::measure::{
    parse_any_turn_set, parse_turn_set_v2, run_cli, run_measurement_any, MeasureConfig,
    MeasureReport, TurnRow,
};
use etv_core::model::GMProfile;
use etv_core::narrator::{
    build_outcome_prompts, check_generated_line, narration_call_seed, CheckReasonCode,
    CheckerContext, NarratorLength, NarratorTransport, TransportError,
};
use etv_core::narrator_experiment::{
    candidate_seed, length_target, restore_stopped_period, rulings_system_prompt,
    rulings_team_species, rulings_user_prompt, schema_item_limit, trim_to_word_cap, ExperimentArms,
    CANDIDATE_SEED_OFFSETS, FIRESIDE_OXEN_LINES, MAX_CANDIDATES, QUANTIFIER_LINE,
};
use serde_json::{json, Value};

const V2_SHA256: &str = "095455fb593311ecdb76b139aa1ad20220807bdf7bcd4d236ec648e276c13769";
const V2_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v2.json");

/// Records every generation body and answers from a script (repeating its last answer).
struct Script {
    answers: Vec<&'static str>,
    bodies: Mutex<Vec<String>>,
}

impl Script {
    fn new(answers: &[&'static str]) -> Arc<Self> {
        Arc::new(Self {
            answers: answers.to_vec(),
            bodies: Mutex::new(Vec::new()),
        })
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|b| serde_json::from_str(b).unwrap())
            .collect()
    }
}

impl NarratorTransport for Script {
    fn get_tags(&self, _e: &str, _t: Duration) -> Result<(u16, String), TransportError> {
        Ok((
            200,
            r#"{"models":[{"name":"fixture-model:latest","digest":"abc123def456","details":{"quantization_level":"Q4_K_M"}}]}"#
                .to_string(),
        ))
    }

    fn post_generate(
        &self,
        _e: &str,
        body_json: &str,
        _t: Duration,
    ) -> Result<(u16, String), TransportError> {
        if body_json.contains("\"Warm up.\"") {
            return Ok((200, r#"{"response":""}"#.to_string()));
        }
        let mut bodies = self.bodies.lock().unwrap();
        let answer = self.answers[bodies.len().min(self.answers.len() - 1)];
        bodies.push(body_json.to_string());
        Ok((200, json!({ "response": answer }).to_string()))
    }
}

fn cell(profile: GMProfile, length: NarratorLength, turns: usize) -> MeasureConfig {
    let mut cfg = MeasureConfig::new("fixture-model");
    cfg.profiles = vec![profile];
    cfg.lengths = vec![length];
    cfg.max_turns = Some(turns);
    cfg
}

fn run(cfg: &MeasureConfig, answers: &[&'static str]) -> (MeasureReport, Vec<TurnRow>, Vec<Value>) {
    let set = parse_any_turn_set(V2_BYTES, V2_SHA256).unwrap();
    let transport = Script::new(answers);
    let (report, rows) = run_measurement_any(&set, V2_SHA256, cfg, transport.clone()).unwrap();
    (report, rows, transport.bodies())
}

const OK: &str = r#"{"sentences":["The team holds."]}"#;

// ---- off by default ----------------------------------------------------------------------------

#[test]
fn every_arm_is_off_by_default_and_the_default_request_is_the_shipped_one() {
    let arms = ExperimentArms::default();
    assert!(arms.is_shipped());
    assert_eq!(arms.label(), "shipped");
    assert_eq!(arms.candidates, 1);
    assert_eq!(MeasureConfig::new("m").experiment, arms);

    let cfg = cell(GMProfile::Fireside, NarratorLength::Standard, 1);
    let (report, rows, bodies) = run(&cfg, &[OK]);
    assert_eq!(report.experiment, "shipped");
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        assert_eq!(body["options"]["temperature"], json!(0.15));
        assert!(body["options"].get("top_p").is_none());
        assert!(body["options"].get("top_k").is_none());
        assert!(body["options"].get("stop").is_none());
        assert_eq!(
            body["format"]["properties"]["sentences"]["maxItems"],
            json!(8)
        );
        let system = body["system"].as_str().unwrap();
        assert!(system.contains("- LENGTH CAP: Standard (at most 45 words in all)."));
        assert!(system.contains(QUANTIFIER_LINE));
        assert!(!system.contains("oxen"));
    }
    assert_eq!((rows[0].scene.attempts, rows[0].outcome.attempts), (1, 1));
    assert_eq!(report.cells[0].calls_made, 2);
    // The run record names the model as the server lists it.
    assert_eq!(report.model_digest.as_deref(), Some("abc123def456"));
    assert_eq!(report.model_quantization.as_deref(), Some("Q4_K_M"));
}

#[test]
fn labels_name_the_arms_that_are_on() {
    let mut arms = ExperimentArms::default();
    arms.rulings_prompt = true;
    assert_eq!(arms.label(), "rulings-prompt");
    arms.model_card_sampling = true;
    arms.candidates = 3;
    arms.trim_to_cap = true;
    assert_eq!(
        arms.label(),
        "rulings-prompt+model-card-sampling+candidates-3+trim-to-cap"
    );
    let mut stop = ExperimentArms::default();
    stop.brief_by_stop = true;
    assert_eq!(stop.label(), "brief-by-stop");
}

// ---- the rulings prompt ------------------------------------------------------------------------

#[test]
fn the_rulings_arm_gives_a_target_and_drops_the_cap_and_the_quantifier_line() {
    assert_eq!(
        length_target(NarratorLength::Brief),
        "one sentence, about 12 words"
    );
    assert_eq!(
        length_target(NarratorLength::Standard),
        "one or two sentences, about 20 words"
    );
    assert_eq!(
        length_target(NarratorLength::Rich),
        "two or three sentences, about 40 words"
    );

    for (length, target, cap) in [
        (
            NarratorLength::Brief,
            "- LENGTH: Brief. Write one sentence, about 12 words.",
            25,
        ),
        (
            NarratorLength::Standard,
            "- LENGTH: Standard. Write one or two sentences, about 20 words.",
            45,
        ),
        (
            NarratorLength::Rich,
            "- LENGTH: Rich. Write two or three sentences, about 40 words.",
            70,
        ),
    ] {
        for profile in [
            GMProfile::Chronicler,
            GMProfile::Fireside,
            GMProfile::Lantern,
        ] {
            let mut cfg = cell(profile, length, 1);
            cfg.experiment.rulings_prompt = true;
            let (report, _, bodies) = run(&cfg, &[OK]);
            assert_eq!(report.experiment, "rulings-prompt");
            for body in &bodies {
                let system = body["system"].as_str().unwrap();
                let user = body["prompt"].as_str().unwrap();
                assert_eq!(system.matches(target).count(), 1, "{system}");
                assert!(!system.contains("LENGTH CAP"));
                assert!(!system.contains("words in all"));
                assert!(!system.contains("Allowed quantifiers"));
                assert!(!system.contains("one, both, pair"));
                // The cap is nowhere in the prompt; it is the checker's.
                assert!(!user.contains("words in all"), "{user}");
                assert!(!user.contains(&format!("{cap} words")));
                // Fireside alone is told about the oxen and the party.
                assert_eq!(
                    system.contains("- The wagon is drawn by oxen."),
                    profile == GMProfile::Fireside
                );
                assert_eq!(
                    system.contains("- Speak of the party and the travellers. Address no one."),
                    profile == GMProfile::Fireside
                );
                assert!(!system.contains("vivid, personal"));
                // The answer shape is unchanged.
                assert_eq!(
                    body["format"]["properties"]["sentences"]["maxItems"],
                    json!(8)
                );
                assert_eq!(body["options"]["temperature"], json!(0.15));
            }
        }
    }
}

/// v2-w0-001 has lost one traveller and v2-w0-003 has one left.
#[test]
fn the_rulings_arm_words_the_partys_losses_without_the_word_one() {
    let set = parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap();
    for (id, shipped, reworded) in [
        (
            "v2-w0-001",
            "One of the party is lost.",
            "A traveller is lost.",
        ),
        (
            "v2-w0-003",
            "Only one of the party is left.",
            "A lone traveller remains.",
        ),
    ] {
        let turn = set.turns.iter().find(|t| t.id == id).unwrap();
        let ctx = turn.context.clone();
        let (system, user) =
            build_outcome_prompts(GMProfile::Chronicler, NarratorLength::Standard, &ctx);
        assert!(system.contains(shipped));
        let arm = rulings_system_prompt(&system, GMProfile::Chronicler, NarratorLength::Standard);
        assert!(!arm.contains(shipped));
        assert_eq!(arm.matches(reworded).count(), 1);
        // The stored turn is untouched: only the prompt text of the arm differs.
        assert!(turn.context.situation().contains(shipped));
        assert_eq!(
            rulings_user_prompt(&user, NarratorLength::Standard),
            "Generate an outcome response strictly as valid JSON matching OUTCOME_SCHEMA:\n{\"sentences\": [\"...\"]}\nThe sentences array holds complete sentences, one per item, each following all the rules."
        );
    }
    // No turn's situation, as the arm sends it, holds the word "one".
    for turn in &set.turns {
        let (system, _) = build_outcome_prompts(
            GMProfile::Chronicler,
            NarratorLength::Standard,
            &turn.context,
        );
        let arm = rulings_system_prompt(&system, GMProfile::Chronicler, NarratorLength::Standard);
        let situation = arm
            .lines()
            .find(|l| l.starts_with("- situation: "))
            .unwrap()
            .to_ascii_lowercase();
        assert!(
            !situation
                .split(|c: char| !c.is_ascii_alphabetic())
                .any(|w| w == "one"),
            "{} {situation}",
            turn.id
        );
    }
}

#[test]
fn in_the_rulings_arm_fireside_may_say_oxen_and_no_other_voice_may() {
    assert_eq!(
        rulings_team_species(GMProfile::Fireside),
        vec!["ox".to_string(), "oxen".to_string()]
    );
    assert!(rulings_team_species(GMProfile::Chronicler).is_empty());
    assert!(rulings_team_species(GMProfile::Lantern).is_empty());
    assert!(FIRESIDE_OXEN_LINES.contains("oxen"));

    let line = "The oxen lean into the yoke.";
    let shipped = CheckerContext::default();
    assert_eq!(
        check_generated_line(line, &shipped).unwrap_err().code,
        CheckReasonCode::DraftAnimalSpecies
    );
    let fireside = CheckerContext {
        team_species: rulings_team_species(GMProfile::Fireside),
        ..CheckerContext::default()
    };
    assert_eq!(check_generated_line(line, &fireside), Ok(()));
    // A horse is still not the team.
    assert_eq!(
        check_generated_line("The horse leans into the yoke.", &fireside)
            .unwrap_err()
            .code,
        CheckReasonCode::DraftAnimalSpecies
    );

    // Through the harness: the same answer is accepted for Fireside and rejected for the
    // Chronicler in the arm, and rejected for Fireside with the arm off.
    let oxen: &[&'static str] = &[r#"{"sentences":["The oxen lean into the yoke."]}"#];
    for (profile, arm_on, accepted) in [
        (GMProfile::Fireside, true, 2),
        (GMProfile::Chronicler, true, 0),
        (GMProfile::Fireside, false, 0),
    ] {
        let mut cfg = cell(profile, NarratorLength::Standard, 1);
        cfg.experiment.rulings_prompt = arm_on;
        let (report, _, _) = run(&cfg, oxen);
        assert_eq!(
            report.cells[0].accepted_slots, accepted,
            "{profile:?} {arm_on}"
        );
    }
}

// ---- brief by stop -----------------------------------------------------------------------------

#[test]
fn a_brief_line_is_plain_text_stopped_at_the_first_full_stop() {
    assert_eq!(
        restore_stopped_period("Mud grips the wheels"),
        "Mud grips the wheels."
    );
    assert_eq!(
        restore_stopped_period("  Mud grips the wheels.  "),
        "Mud grips the wheels."
    );
    assert_eq!(restore_stopped_period("   "), "");

    let mut cfg = cell(GMProfile::Chronicler, NarratorLength::Brief, 1);
    cfg.experiment.brief_by_stop = true;
    // The server stops before the period, so the answer has none.
    let (report, rows, bodies) = run(&cfg, &["Mud grips the wheels"]);
    assert_eq!(report.experiment, "brief-by-stop");
    for body in &bodies {
        assert!(body.get("format").is_none());
        assert_eq!(body["options"]["stop"], json!(["."]));
        assert!(body["prompt"]
            .as_str()
            .unwrap()
            .contains("as plain text: complete sentences, at most 25 words in all"));
    }
    assert_eq!(report.cells[0].accepted_slots, 2);
    assert_eq!(
        rows[0].scene.candidate.as_deref(),
        Some("Mud grips the wheels.")
    );
    assert_eq!(
        rows[0].scene.raw_response.as_deref(),
        Some("Mud grips the wheels")
    );
}

#[test]
fn in_that_arm_standard_and_rich_keep_the_schema_with_a_sentence_item_limit() {
    assert_eq!(schema_item_limit(NarratorLength::Brief), 1);
    assert_eq!(schema_item_limit(NarratorLength::Standard), 2);
    assert_eq!(schema_item_limit(NarratorLength::Rich), 3);
    for (length, limit) in [(NarratorLength::Standard, 2), (NarratorLength::Rich, 3)] {
        let mut cfg = cell(GMProfile::Chronicler, length, 1);
        cfg.experiment.brief_by_stop = true;
        let (_, _, bodies) = run(&cfg, &[OK]);
        for body in &bodies {
            let sentences = &body["format"]["properties"]["sentences"];
            assert_eq!(sentences["maxItems"], json!(limit));
            assert_eq!(sentences["minItems"], json!(1));
            assert_eq!(body["format"]["additionalProperties"], json!(false));
            assert!(body["options"].get("stop").is_none());
        }
    }
}

// ---- model-card sampling -----------------------------------------------------------------------

#[test]
fn the_model_card_arm_sends_the_cards_sampling_settings_and_the_same_seed() {
    let base = cell(GMProfile::Lantern, NarratorLength::Standard, 1);
    let (_, _, shipped) = run(&base, &[OK]);
    let mut cfg = base.clone();
    cfg.experiment.model_card_sampling = true;
    let (report, _, bodies) = run(&cfg, &[OK]);
    assert_eq!(report.experiment, "model-card-sampling");
    for (body, was) in bodies.iter().zip(&shipped) {
        assert_eq!(body["options"]["temperature"], json!(0.7));
        assert_eq!(body["options"]["top_p"], json!(0.8));
        assert_eq!(body["options"]["top_k"], json!(20));
        // Everything else is the shipped request.
        assert_eq!(body["options"]["seed"], was["options"]["seed"]);
        assert_eq!(body["options"]["num_ctx"], was["options"]["num_ctx"]);
        assert_eq!(
            body["options"]["num_predict"],
            was["options"]["num_predict"]
        );
        assert_eq!(body["system"], was["system"]);
        assert_eq!(body["prompt"], was["prompt"]);
        assert_eq!(body["format"], was["format"]);
    }
}

// ---- candidates --------------------------------------------------------------------------------

#[test]
fn candidate_seeds_are_the_turns_seed_plus_fixed_offsets() {
    assert_eq!(MAX_CANDIDATES, 3);
    assert_eq!(CANDIDATE_SEED_OFFSETS, [0, 7_919, 104_729]);
    assert_eq!(candidate_seed(1_000, 0), 1_000);
    assert_eq!(candidate_seed(1_000, 1), 8_919);
    assert_eq!(candidate_seed(1_000, 2), 105_729);
    // Kept to 31 bits: 2147483647 + 7919 wraps to 7918.
    assert_eq!(candidate_seed(0x7FFF_FFFF, 1), 7_918);
}

const BAD: &str = r#"{"sentences":["We saw two men; the wind rose."]}"#;

#[test]
fn candidates_run_one_after_another_and_the_first_accepted_line_is_used() {
    let set = parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap();
    let ctx = set.turns[0]
        .context
        .clone()
        .with_gm_profile(GMProfile::Chronicler);
    let scene_seed =
        narration_call_seed(&ctx, GMProfile::Chronicler, NarratorLength::Standard, true);
    let outcome_seed =
        narration_call_seed(&ctx, GMProfile::Chronicler, NarratorLength::Standard, false);

    let mut cfg = cell(GMProfile::Chronicler, NarratorLength::Standard, 1);
    cfg.experiment.candidates = 3;
    // Scene: rejected, rejected, accepted (three calls). Outcome: accepted at once (one call).
    let (report, rows, bodies) = run(&cfg, &[BAD, BAD, OK, OK]);
    assert_eq!(report.experiment, "candidates-3");
    assert_eq!(bodies.len(), 4);
    let seeds: Vec<u64> = bodies
        .iter()
        .map(|b| b["options"]["seed"].as_u64().unwrap())
        .collect();
    assert_eq!(
        seeds,
        vec![
            u64::from(candidate_seed(scene_seed, 0)),
            u64::from(candidate_seed(scene_seed, 1)),
            u64::from(candidate_seed(scene_seed, 2)),
            u64::from(outcome_seed),
        ]
    );
    // The candidates differ in their seed only.
    assert_eq!(bodies[0]["system"], bodies[1]["system"]);
    assert_eq!(bodies[0]["prompt"], bodies[2]["prompt"]);

    let row = &rows[0];
    assert_eq!((row.scene.attempts, row.outcome.attempts), (3, 1));
    assert_eq!(
        row.scene.attempt_outcomes,
        [
            "rule_reject:invalid_punctuation",
            "rule_reject:invalid_punctuation",
            "accepted"
        ]
    );
    assert_eq!(row.scene.status, "accepted");
    assert_eq!(row.scene.candidate.as_deref(), Some("The team holds."));
    assert_eq!(row.scene.seed, Some(candidate_seed(scene_seed, 2)));
    let cell = &report.cells[0];
    assert_eq!(
        (cell.slots, cell.accepted_slots, cell.calls_made),
        (2, 2, 4)
    );
    assert_eq!(cell.latency.calls, 2);
}

#[test]
fn when_every_candidate_is_rejected_the_line_falls_back_after_three_calls() {
    let mut cfg = cell(GMProfile::Chronicler, NarratorLength::Standard, 1);
    cfg.experiment.candidates = 3;
    let (report, rows, bodies) = run(&cfg, &[BAD]);
    assert_eq!(bodies.len(), 6);
    assert_eq!(rows[0].scene.attempts, 3);
    assert_eq!(rows[0].scene.status, "rule_reject");
    let cell = &report.cells[0];
    assert_eq!(
        (cell.accepted_slots, cell.fallback_slots, cell.calls_made),
        (0, 2, 6)
    );
    // One rejected line per slot, not one per call.
    assert_eq!(cell.rule_rejects, 2);
}

/// Two plays of the same turns with the same answers give byte-identical text, requests and
/// seeds. Only the clock differs.
#[test]
fn two_plays_of_the_same_turn_give_byte_identical_text() {
    let mut cfg = cell(GMProfile::Fireside, NarratorLength::Rich, 3);
    cfg.experiment.candidates = 3;
    cfg.experiment.trim_to_cap = true;
    cfg.experiment.model_card_sampling = true;
    cfg.experiment.rulings_prompt = true;
    let script: &[&'static str] = &[BAD, OK, BAD, BAD, OK, OK, BAD, BAD, BAD, OK];
    let strip = |rows: &[TurnRow]| -> String {
        rows.iter()
            .map(|row| {
                let mut v = serde_json::to_value(row).unwrap();
                v.as_object_mut().unwrap().remove("latency_ms_turn");
                for slot in ["scene", "outcome"] {
                    v[slot].as_object_mut().unwrap().remove("latency_ms");
                }
                v.to_string()
            })
            .collect::<Vec<String>>()
            .join("\n")
    };
    let (_, first, bodies_a) = run(&cfg, script);
    let (_, second, bodies_b) = run(&cfg, script);
    assert_eq!(strip(&first), strip(&second));
    assert_eq!(bodies_a, bodies_b);
    // And the text itself, line by line.
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(a.scene.candidate, b.scene.candidate);
        assert_eq!(a.outcome.candidate, b.outcome.candidate);
        assert_eq!(a.scene.seed, b.scene.seed);
    }
}

// ---- trim to cap -------------------------------------------------------------------------------

#[test]
fn an_over_long_line_is_cut_back_to_whole_sentences_within_the_cap() {
    // Within the cap: unchanged.
    assert_eq!(
        trim_to_word_cap("Mud holds. Wind rises.", 25),
        "Mud holds. Wind rises."
    );
    // 4 + 4 + 4 words, cap 9: the first two sentences fit, the third does not.
    assert_eq!(
        trim_to_word_cap(
            "Mud grips the wheels. Wind crosses the grass. Rain falls at dusk.",
            9
        ),
        "Mud grips the wheels. Wind crosses the grass."
    );
    // Cap 4: only the first sentence.
    assert_eq!(
        trim_to_word_cap(
            "Mud grips the wheels. Wind crosses the grass. Rain falls at dusk.",
            4
        ),
        "Mud grips the wheels."
    );
    // The first sentence alone is over the cap: there is nowhere to cut, so it is left.
    assert_eq!(
        trim_to_word_cap("Mud grips the wheels of the wagon. Wind rises.", 3),
        "Mud grips the wheels of the wagon. Wind rises."
    );
    // Trailing text with no period is dropped with the sentences that do not fit.
    assert_eq!(
        trim_to_word_cap("Mud holds. Wind rises and the team", 4),
        "Mud holds."
    );
}

#[test]
fn with_the_trim_arm_an_over_long_answer_is_trimmed_then_accepted() {
    // 28 words in seven sentences; Brief allows 25.
    let long: &[&'static str] = &[
        r#"{"sentences":["Mud grips the wheels. Wind crosses the grass. Rain falls at dusk. The team holds firm. Smoke drifts low here. The trail runs on. Night comes down slow."]}"#,
    ];
    let cfg = cell(GMProfile::Chronicler, NarratorLength::Brief, 1);
    let (shipped, _, _) = run(&cfg, long);
    assert_eq!(shipped.cells[0].accepted_slots, 0);
    assert_eq!(shipped.cells[0].rejects_by_code["length_cap_exceeded"], 2);

    let mut arm = cfg.clone();
    arm.experiment.trim_to_cap = true;
    let (report, rows, _) = run(&arm, long);
    assert_eq!(report.experiment, "trim-to-cap");
    assert_eq!(report.cells[0].accepted_slots, 2);
    // The first six sentences are 24 words; the seventh would make 28.
    assert_eq!(
        rows[0].scene.candidate.as_deref(),
        Some("Mud grips the wheels. Wind crosses the grass. Rain falls at dusk. The team holds firm. Smoke drifts low here. The trail runs on.")
    );
    // The model's own text is kept whole in the row.
    assert!(rows[0]
        .scene
        .raw_response
        .as_deref()
        .unwrap()
        .contains("Night comes down slow."));
}

// ---- the command line --------------------------------------------------------------------------

#[test]
fn the_command_line_switches_arms_on_and_refuses_bad_values() {
    let set_path = format!(
        "{}/tests/fixtures/narrator_turn_set_v2.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let base = |extra: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = [
            "--turn-set",
            &set_path,
            "--turn-set-sha256",
            V2_SHA256,
            "--model",
            "fixture-model",
            "--profiles",
            "fireside",
            "--lengths",
            "brief",
            "--max-turns",
            "1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    };
    let transport = Script::new(&["Smoke drifts over the camp"]);
    let mut out = Vec::new();
    run_cli(
        &base(&[
            "--arm",
            "rulings-prompt",
            "--arm",
            "brief-by-stop",
            "--arm",
            "model-card-sampling",
            "--arm",
            "trim-to-cap",
            "--candidates",
            "2",
        ]),
        transport.clone(),
        &mut out,
    )
    .unwrap();
    let report: MeasureReport = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        report.experiment,
        "rulings-prompt+brief-by-stop+model-card-sampling+candidates-2+trim-to-cap"
    );
    assert_eq!(report.cells[0].accepted_slots, 2);
    let body = &transport.bodies()[0];
    assert_eq!(body["options"]["stop"], json!(["."]));
    assert_eq!(body["options"]["temperature"], json!(0.7));
    assert!(body["system"]
        .as_str()
        .unwrap()
        .contains("- The wagon is drawn by oxen."));

    for (extra, message) in [
        (
            vec!["--arm", "faster"],
            "--arm must be rulings-prompt, brief-by-stop, model-card-sampling or trim-to-cap, not \"faster\"",
        ),
        (vec!["--candidates", "4"], "--candidates must be 1 to 3"),
        (vec!["--candidates", "0"], "--candidates must be 1 to 3"),
        (vec!["--candidates", "two"], "--candidates needs a whole number"),
    ] {
        let mut out = Vec::new();
        let err = run_cli(&base(&extra), Script::new(&[OK]), &mut out).unwrap_err();
        assert_eq!(err, message);
    }
}
