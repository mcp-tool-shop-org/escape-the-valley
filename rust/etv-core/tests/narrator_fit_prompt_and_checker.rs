//! Changes made after the first fallback-rate run, where about six lines in ten were rejected
//! and most rejections were the prompt and the checker disagreeing:
//!
//! - length is a word cap (Brief 25, Standard 45, Rich 70), not a sentence count;
//! - a unit word is rejected only after a number or quantity word;
//! - a resource noun may appear anywhere in the line, and a supply the situation names may be
//!   mentioned; invented nouns are still rejected;
//! - the situation carries bands, not numbers;
//! - the typographic apostrophe is read as a plain one;
//! - Rich gets an answer budget that covers its cap;
//! - the settings screen says short, medium, long;
//! - the harness keeps the request and seed with each row and hands rows over as it goes.
//!
//! No model is called anywhere in this file.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::measure::{
    parse_any_turn_set, parse_turn_set_v2, run_cli, run_measurement_any_with_rows, MeasureConfig,
    TurnSpecV2,
};
use etv_core::model::GMProfile;
use etv_core::narrator::{
    check_generated_line, format_turn_facts_for_slot, party_loss_phrase, plain_apostrophes,
    situation_for_narrator, situation_supply_key, CheckReasonCode, CheckerContext, FormatSchema,
    NarratorLength, NarratorTransport, PromptSlot, TransportError, NARRATOR_NUM_PREDICT,
    NARRATOR_NUM_PREDICT_RICH,
};
use etv_core::screens::length_description;

const V2_SHA256: &str = "458d444aae17a37b4247870481a1533585b58e3233670558a9436208182e0016";
const V2_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v2.json");

fn turns() -> Vec<TurnSpecV2> {
    parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap().turns
}

fn ctx(length: NarratorLength) -> CheckerContext {
    CheckerContext {
        length,
        ..CheckerContext::default()
    }
}

fn code(line: &str, c: &CheckerContext) -> Option<CheckReasonCode> {
    check_generated_line(line, c).err().map(|e| e.code)
}

/// A line of exactly `words` words in short sentences that breaks no other rule.
fn line_of(words: usize) -> String {
    let mut out = Vec::new();
    for i in 0..words {
        out.push(match i % 4 {
            0 => "Mud",
            1 => "holds",
            2 => "the",
            _ => "wheels.",
        });
    }
    let mut text = out.join(" ");
    if !text.ends_with('.') {
        text.push('.');
    }
    text
}

// ---- length ------------------------------------------------------------------------------------

#[test]
fn the_caps_are_25_45_and_70_words() {
    assert_eq!(NarratorLength::Brief.word_cap(), 25);
    assert_eq!(NarratorLength::Standard.word_cap(), 45);
    assert_eq!(NarratorLength::Rich.word_cap(), 70);
    for (length, cap) in [
        (NarratorLength::Brief, 25),
        (NarratorLength::Standard, 45),
        (NarratorLength::Rich, 70),
    ] {
        let at = line_of(cap);
        assert_eq!(at.split_whitespace().count(), cap);
        assert_eq!(code(&at, &ctx(length)), None, "{length:?} at the cap");
        let over = line_of(cap + 1);
        let err = check_generated_line(&over, &ctx(length)).unwrap_err();
        assert_eq!(err.code, CheckReasonCode::LengthCapExceeded);
        assert_eq!(
            err.detail,
            format!(
                "expected at most {cap} words for {length:?}, found {}",
                cap + 1
            )
        );
    }
}

/// An answer of the kind the first run rejected as "expected 2..=3 sentences": six short sentences from the
/// Chronicler, well under the Rich cap. A single sentence passes at Rich too.
#[test]
fn short_sentences_are_not_counted_against_the_line() {
    let six = "Mist hangs low today. Mud swells under the wheels. Sun dips low. \
               The team leans into the traces. Wind crosses the grass. The trail runs on.";
    assert_eq!(six.matches('.').count(), 6);
    assert_eq!(six.split_whitespace().count(), 26);
    assert_eq!(code(six, &ctx(NarratorLength::Rich)), None);
    assert_eq!(code(six, &ctx(NarratorLength::Standard)), None);
    // 26 words is one over Brief.
    assert_eq!(
        code(six, &ctx(NarratorLength::Brief)),
        Some(CheckReasonCode::LengthCapExceeded)
    );
    assert_eq!(code("The team holds.", &ctx(NarratorLength::Rich)), None);
}

#[test]
fn rich_gets_an_answer_budget_that_covers_its_cap() {
    assert_eq!(NARRATOR_NUM_PREDICT, 110);
    assert_eq!(NARRATOR_NUM_PREDICT_RICH, 150);
    for (schema, brief, standard, rich) in [
        (FormatSchema::Plain, 110, 110, 150),
        (FormatSchema::PlainText, 110, 110, 150),
        (FormatSchema::CitedFacts, 160, 160, 200),
    ] {
        assert_eq!(schema.num_predict(NarratorLength::Brief), brief);
        assert_eq!(schema.num_predict(NarratorLength::Standard), standard);
        assert_eq!(schema.num_predict(NarratorLength::Rich), rich);
    }
}

#[test]
fn the_settings_screen_says_short_medium_long() {
    assert_eq!(length_description(NarratorLength::Brief), "short");
    assert_eq!(length_description(NarratorLength::Standard), "medium");
    assert_eq!(length_description(NarratorLength::Rich), "long");
}

// ---- unit words --------------------------------------------------------------------------------

#[test]
fn a_unit_word_alone_is_an_ordinary_word() {
    let c = ctx(NarratorLength::Standard);
    for line in [
        "Figures step from the brush.",
        "Each step sinks into the mud.",
        "The trail takes a turn at the ridge.",
        "The wagon rests at the foot of the hill.",
        "The rain goes on for days.",
        "The road runs for miles.",
        "A second gust lifts the canvas.",
        "Hooves pound the wet ground.",
    ] {
        assert_eq!(code(line, &c), None, "{line}");
    }
}

#[test]
fn a_unit_word_after_a_number_or_quantity_word_is_rejected() {
    let c = ctx(NarratorLength::Standard);
    for (line, detail) in [
        ("The party walks two steps.", "banned number word 'two'"),
        (
            "The wagon moves one step.",
            "unit word 'step' after quantity word 'one'",
        ),
        (
            "The rain lasts several days.",
            "unit word 'days' after quantity word 'several'",
        ),
        (
            "The ford lies a mile on.",
            "unit word 'mile' after quantity word 'a'",
        ),
        (
            "The team rests an hour.",
            "unit word 'hour' after quantity word 'an'",
        ),
        (
            "The ridge stands many miles off.",
            "unit word 'miles' after quantity word 'many'",
        ),
        (
            "The trail climbs for three days.",
            "banned number word 'three'",
        ),
    ] {
        let err = check_generated_line(line, &c).unwrap_err();
        assert_eq!(err.code, CheckReasonCode::NumbersDigitsOrUnits, "{line}");
        assert_eq!(err.detail, detail, "{line}");
    }
    // Digits are rejected as before.
    assert_eq!(
        code("The camp lies 3 ridges off.", &c),
        Some(CheckReasonCode::NumbersDigitsOrUnits)
    );
}

// ---- resource nouns ----------------------------------------------------------------------------

#[test]
fn a_changed_resource_may_be_named_in_any_sentence() {
    let mut c = ctx(NarratorLength::Standard);
    c.allowed_resources = vec!["rope".to_string()];
    assert_eq!(code("The rope frays at the knot.", &c), None);
    assert_eq!(
        code("The wagon slides. The rope frays at the knot.", &c),
        None
    );
    assert_eq!(
        code("Wind rises. Mud holds. The rope frays at the knot.", &c),
        None
    );
}

#[test]
fn invented_resource_nouns_are_still_rejected() {
    let mut c = ctx(NarratorLength::Standard);
    c.allowed_resources = vec!["rope".to_string()];
    for line in [
        "The boots sink in the mud.",
        "Gold glints in the dust.",
        "The wagon slides. Flour spills across the bed.",
    ] {
        assert_eq!(
            code(line, &c),
            Some(CheckReasonCode::UnauthorisedResourceNoun),
            "{line}"
        );
    }
}

#[test]
fn a_supply_the_situation_names_may_be_mentioned() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    assert_eq!(situation_supply_key(&s(&["FOOD_GONE"])), Some("food"));
    assert_eq!(situation_supply_key(&s(&["FOOD_CRITICAL"])), Some("food"));
    assert_eq!(situation_supply_key(&s(&["WATER_GONE"])), Some("water"));
    assert_eq!(situation_supply_key(&s(&["WATER_CRITICAL"])), Some("water"));
    // Only the top pressure is in the situation text.
    assert_eq!(situation_supply_key(&s(&["PARTY_SICK", "FOOD_GONE"])), None);
    assert_eq!(situation_supply_key(&s(&[])), None);

    let mut with_food = 0;
    let mut without = 0;
    for t in turns() {
        let c = &t.context;
        let checker = CheckerContext::from_narration_context(c, NarratorLength::Standard);
        let named = situation_supply_key(c.pressures());
        let changed = |key: &str| c.shown_deltas().get(key).is_some_and(|d| *d != 0);
        for key in ["food", "water"] {
            let allowed = checker.allowed_resources.iter().any(|k| k == key);
            assert_eq!(
                allowed,
                named == Some(key) || changed(key),
                "{} {key}",
                t.id
            );
        }
        // A turn whose situation says there is no food, on which food did not change.
        if named == Some("food") && !changed("food") {
            assert!(c.situation().contains("food") || c.situation().contains("Rations"));
            assert_eq!(
                check_generated_line("No food is left in the wagon.", &checker)
                    .err()
                    .map(|e| e.code)
                    .filter(|code| *code == CheckReasonCode::UnauthorisedResourceNoun),
                None,
                "{}",
                t.id
            );
            with_food += 1;
        }
        if named.is_none() && !changed("food") {
            without += 1;
        }
    }
    assert!(with_food > 0);
    assert!(without > 0);
}

// ---- the situation carries bands, not numbers --------------------------------------------------

#[test]
fn party_loss_is_told_without_a_number() {
    assert_eq!(party_loss_phrase(4, 4), None);
    assert_eq!(party_loss_phrase(3, 4), Some("One of the party is lost."));
    assert_eq!(
        party_loss_phrase(2, 4),
        Some("Several of the party are lost.")
    );
    assert_eq!(
        party_loss_phrase(1, 4),
        Some("Only one of the party is left.")
    );
    assert_eq!(party_loss_phrase(0, 4), Some("None of the party is left."));
}

#[test]
fn the_day_and_the_head_count_are_not_handed_to_the_narrator() {
    assert_eq!(
        situation_for_narrator(
            "Day 23, afternoon. Swamp terrain. Nearing the halfway mark. \
             3 of 4 party members alive. They have no food. \
             They carry no debts. Morale holds, but the wagon is lean. \
             They refuse to drink river water."
        ),
        "Swamp terrain. Nearing the halfway mark. One of the party is lost. They have no food. \
         Their doctrine: no debts. Their taboo: no river water."
    );
    assert_eq!(
        situation_for_narrator(
            "Day 1, morning. Plains terrain. Early in the journey. \
             Every repair is deliberate. The caravan moves carefully. They leave nothing behind."
        ),
        "Plains terrain. Early in the journey. Their doctrine: careful hands. \
         Their taboo: leave nothing behind."
    );
}

#[test]
fn no_situation_in_the_set_holds_a_digit_and_the_counts_match_the_party() {
    let mut phrases = [0usize; 4];
    for t in turns() {
        let c = &t.context;
        let situation = c.situation();
        assert!(
            !situation.chars().any(|ch| ch.is_ascii_digit()),
            "{} {situation}",
            t.id
        );
        assert!(!situation.contains("Day "), "{}", t.id);
        let facts = format_turn_facts_for_slot(c, PromptSlot::Outcome);
        let line = facts
            .lines()
            .find(|l| l.starts_with("- situation: "))
            .unwrap();
        assert!(!line.chars().any(|ch| ch.is_ascii_digit()), "{}", t.id);

        let expected = party_loss_phrase(
            c.party_status().alive_count(),
            c.party_status().total_count(),
        );
        for (i, phrase) in [
            "One of the party is lost.",
            "Several of the party are lost.",
            "Only one of the party is left.",
            "None of the party is left.",
        ]
        .iter()
        .enumerate()
        {
            let present = situation.contains(phrase);
            assert_eq!(present, expected == Some(*phrase), "{} {phrase}", t.id);
            if present {
                phrases[i] += 1;
            }
        }
    }
    assert_eq!(phrases, [46, 23, 21, 8]);
}

// ---- typographic apostrophes -------------------------------------------------------------------

#[test]
fn the_typographic_apostrophe_is_read_as_a_plain_one() {
    let raw = "The wagon\u{2019}s tongue dips toward the mud.";
    let c = ctx(NarratorLength::Standard);
    // As the model wrote it, the checker has no such character.
    assert_eq!(code(raw, &c), Some(CheckReasonCode::InvalidPunctuation));
    let plain = plain_apostrophes(raw);
    assert_eq!(plain, "The wagon's tongue dips toward the mud.");
    assert_eq!(code(&plain, &c), None);
    assert_eq!(plain_apostrophes("\u{2018}tis"), "'tis");
    // Other typographic marks are left alone, and still rejected.
    let dash = plain_apostrophes("The wagon rolls \u{2014} the team holds.");
    assert_eq!(code(&dash, &c), Some(CheckReasonCode::InvalidPunctuation));
}

// ---- the harness keeps the request and seed, and hands rows over as it goes ---------------------

/// Answers the model list and the warm-up, records every body, and gives a fixed answer.
struct Fixed {
    bodies: Mutex<Vec<String>>,
    answer: &'static str,
}

impl NarratorTransport for Fixed {
    fn get_tags(&self, _e: &str, _t: Duration) -> Result<(u16, String), TransportError> {
        Ok((
            200,
            r#"{"models":[{"name":"fixture-model:latest"}]}"#.to_string(),
        ))
    }

    fn post_generate(
        &self,
        _e: &str,
        body_json: &str,
        _t: Duration,
    ) -> Result<(u16, String), TransportError> {
        self.bodies.lock().unwrap().push(body_json.to_string());
        if body_json.contains("\"Warm up.\"") {
            return Ok((200, r#"{"response":""}"#.to_string()));
        }
        Ok((
            200,
            serde_json::json!({ "response": self.answer }).to_string(),
        ))
    }
}

fn one_cell() -> MeasureConfig {
    let mut cfg = MeasureConfig::new("fixture-model");
    cfg.profiles = vec![GMProfile::Fireside];
    cfg.lengths = vec![NarratorLength::Standard];
    cfg.max_turns = Some(3);
    cfg
}

#[test]
fn each_row_carries_the_request_and_seed_and_arrives_as_its_turn_ends() {
    let set = parse_any_turn_set(V2_BYTES, V2_SHA256).unwrap();
    let transport = Arc::new(Fixed {
        bodies: Mutex::new(Vec::new()),
        answer: r#"{"sentences":["The wagon’s wheels hold in the mud."]}"#,
    });
    // (rows seen so far, generation bodies sent when the row arrived)
    let mut seen: Vec<(usize, usize)> = Vec::new();
    let probe = transport.clone();
    let mut on_row = |_row: &etv_core::measure::TurnRow| -> Result<(), String> {
        let sent = probe.bodies.lock().unwrap().len();
        seen.push((seen.len() + 1, sent));
        Ok(())
    };
    let (report, rows) =
        run_measurement_any_with_rows(&set, V2_SHA256, &one_cell(), transport.clone(), &mut on_row)
            .unwrap();
    // Row n arrives after the warm-up and 2n calls, not at the end.
    assert_eq!(seen, vec![(1, 3), (2, 5), (3, 7)]);
    assert_eq!(rows.len(), 3);
    assert_eq!(report.cells[0].accepted_slots, 6);

    let bodies = transport.bodies.lock().unwrap().clone();
    for (i, row) in rows.iter().enumerate() {
        for (slot, body) in [
            (&row.scene, &bodies[1 + 2 * i]),
            (&row.outcome, &bodies[2 + 2 * i]),
        ] {
            let sent: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(slot.request.as_ref(), Some(&sent));
            assert_eq!(
                slot.seed.map(u64::from),
                sent["options"]["seed"].as_u64(),
                "{}",
                row.turn_id
            );
            assert!(slot.seed.is_some());
            // The typographic apostrophe was read as a plain one and the line accepted.
            assert_eq!(slot.status, "accepted");
            assert_eq!(
                slot.candidate.as_deref(),
                Some("The wagon's wheels hold in the mud.")
            );
        }
    }
}

#[test]
fn an_error_from_the_row_writer_stops_the_run() {
    let set = parse_any_turn_set(V2_BYTES, V2_SHA256).unwrap();
    let transport = Arc::new(Fixed {
        bodies: Mutex::new(Vec::new()),
        answer: r#"{"sentences":["The team holds."]}"#,
    });
    let mut n = 0;
    let mut on_row = |_row: &etv_core::measure::TurnRow| -> Result<(), String> {
        n += 1;
        if n == 2 {
            Err("disk full".to_string())
        } else {
            Ok(())
        }
    };
    let err =
        run_measurement_any_with_rows(&set, V2_SHA256, &one_cell(), transport.clone(), &mut on_row)
            .unwrap_err();
    assert_eq!(err, "disk full");
    // Warm-up, two calls for the first turn, two for the second; the third turn was never asked.
    assert_eq!(transport.bodies.lock().unwrap().len(), 5);
}

#[test]
fn the_command_line_writes_one_row_per_turn_with_request_and_seed() {
    let dir = std::env::temp_dir().join(format!("etv-fit-rows-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rows_path = dir.join("rows.jsonl");
    let set_path = format!(
        "{}/tests/fixtures/narrator_turn_set_v2.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let args: Vec<String> = [
        "--turn-set",
        &set_path,
        "--turn-set-sha256",
        V2_SHA256,
        "--model",
        "fixture-model",
        "--profiles",
        "chronicler",
        "--lengths",
        "rich",
        "--max-turns",
        "2",
        "--rows",
        rows_path.to_str().unwrap(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let transport = Arc::new(Fixed {
        bodies: Mutex::new(Vec::new()),
        answer: r#"{"sentences":["The team holds."]}"#,
    });
    let mut out = Vec::new();
    run_cli(&args, transport, &mut out).unwrap();
    let text = std::fs::read_to_string(&rows_path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        for slot in ["scene", "outcome"] {
            assert!(row[slot]["seed"].is_u64());
            assert_eq!(row[slot]["request"]["options"]["seed"], row[slot]["seed"]);
            // Rich: the larger answer budget.
            assert_eq!(row[slot]["request"]["options"]["num_predict"], 150);
            assert_eq!(row[slot]["request"]["model"], "fixture-model");
            assert_eq!(
                row[slot]["raw_response"],
                r#"{"sentences":["The team holds."]}"#
            );
        }
    }
    std::fs::remove_file(&rows_path).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}
