//! Measurement summaries: every rule a line breaks (beside the first), the coin count under the
//! unit rule, repeated openings, latency per turn, and the paired per-turn comparison of two
//! runs. Play-time behaviour is unchanged: the game's checker still reports the first rule only.
//!
//! No model is called anywhere in this file.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::measure::{
    parse_any_turn_set, run_cli, run_measurement_any, MeasureConfig, MeasureReport, SlotRow,
    TurnRow,
};
use etv_core::measure_stats::{
    cell_reports_from_rows, mean_turn_pass_rate, opening_key, paired_difference, turn_pass_counts,
    OpeningCount, RowsSummary, RuleBreak, ROWS_SUMMARY_SCHEMA,
};
use etv_core::model::GMProfile;
use etv_core::narrator::{
    check_generated_line, check_generated_line_all, CheckReasonCode, CheckerContext,
    NarratorLength, NarratorTransport, TransportError,
};

const V2_SHA256: &str = "095455fb593311ecdb76b139aa1ad20220807bdf7bcd4d236ec648e276c13769";
const V2_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v2.json");

fn ctx() -> CheckerContext {
    CheckerContext {
        length: NarratorLength::Standard,
        ..CheckerContext::default()
    }
}

fn codes(line: &str, c: &CheckerContext) -> Vec<CheckReasonCode> {
    check_generated_line_all(line, c)
        .into_iter()
        .map(|f| f.code)
        .collect()
}

// ---- every rule a line breaks ------------------------------------------------------------------

#[test]
fn a_clean_line_breaks_nothing_and_a_bad_line_lists_every_rule_once_in_order() {
    let c = ctx();
    assert_eq!(codes("The team holds in the mud.", &c), vec![]);

    // One rule.
    assert_eq!(
        codes("The trail climbs for three ridges.", &c),
        vec![CheckReasonCode::NumbersDigitsOrUnits]
    );
    // "one coin": the quantifier rule fires first and hides two more.
    let line = "The wagon holds one coin.";
    assert_eq!(
        check_generated_line(line, &c).unwrap_err().code,
        CheckReasonCode::QuantifierBeforeResourceOrHalf
    );
    let all = check_generated_line_all(line, &c);
    assert_eq!(
        all.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec![
            CheckReasonCode::QuantifierBeforeResourceOrHalf,
            CheckReasonCode::NumbersDigitsOrUnits,
            CheckReasonCode::UnauthorisedResourceNoun,
        ]
    );
    assert_eq!(all[1].detail, "unit word 'coin' after quantity word 'one'");
    // A rule broken several times in one line is listed once.
    assert_eq!(
        codes("Two men walk three ridges and four fords.", &c),
        vec![CheckReasonCode::NumbersDigitsOrUnits]
    );
    // Punctuation, a number, a death word and a pronoun in one line.
    assert_eq!(
        codes("We saw two dead men; the wind rose.", &c),
        vec![
            CheckReasonCode::InvalidPunctuation,
            CheckReasonCode::NumbersDigitsOrUnits,
            CheckReasonCode::DeathWords,
            CheckReasonCode::PersonalPronoun,
        ]
    );
    // An empty line is one failure.
    assert_eq!(codes("   ", &c), vec![CheckReasonCode::InvalidPunctuation]);
}

#[test]
fn the_first_entry_is_always_what_the_games_checker_reports() {
    let lengths = [
        NarratorLength::Brief,
        NarratorLength::Standard,
        NarratorLength::Rich,
    ];
    let lines = [
        "The team holds in the mud.",
        "The wagon holds one coin.",
        "We saw two dead men; the wind rose.",
        "Mud grips the wheels. Wind stirs. One boot sinks. Both men watch the river.",
        "The horse pulls a pair of boots through your camp!",
        "No period here",
        "",
        "Thomas walks three miles to Ravenmoor with gold and a monster.",
        "Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. \
         Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud. Mud.",
    ];
    for length in lengths {
        let mut c = ctx();
        c.length = length;
        c.party_names = vec!["Thomas".to_string()];
        for line in lines {
            let first = check_generated_line(line, &c).err();
            let all = check_generated_line_all(line, &c);
            assert_eq!(first.as_ref(), all.first(), "{length:?} {line:?}");
            // No rule is listed twice.
            for (i, f) in all.iter().enumerate() {
                assert!(all[..i].iter().all(|g| g.code != f.code), "{line:?}");
            }
        }
    }
}

// ---- a measured cell ---------------------------------------------------------------------------

/// Answers the model list and the warm-up, then gives the scripted answers in order.
struct Script {
    answers: Vec<&'static str>,
    next: Mutex<usize>,
}

impl NarratorTransport for Script {
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
        if body_json.contains("\"Warm up.\"") {
            return Ok((200, r#"{"response":""}"#.to_string()));
        }
        let mut next = self.next.lock().unwrap();
        let answer = self.answers[*next % self.answers.len()];
        *next += 1;
        Ok((200, serde_json::json!({ "response": answer }).to_string()))
    }
}

fn one_cell(turns: usize) -> MeasureConfig {
    let mut cfg = MeasureConfig::new("fixture-model");
    cfg.profiles = vec![GMProfile::Fireside];
    cfg.lengths = vec![NarratorLength::Standard];
    cfg.max_turns = Some(turns);
    cfg
}

/// Three turns, six lines: four accepted (two sharing an opening), two rejected.
const ANSWERS: [&str; 6] = [
    r#"{"sentences":["Mud grips the wheels."]}"#,
    r#"{"sentences":["Mud grips the wheels again."]}"#,
    r#"{"sentences":["The wagon holds one coin."]}"#,
    r#"{"sentences":["Wind crosses the grass."]}"#,
    r#"{"sentences":["We saw two men; the wind rose."]}"#,
    r#"{"sentences":["The team holds."]}"#,
];

fn run_three_turns() -> (MeasureReport, Vec<TurnRow>) {
    let set = parse_any_turn_set(V2_BYTES, V2_SHA256).unwrap();
    let transport = Arc::new(Script {
        answers: ANSWERS.to_vec(),
        next: Mutex::new(0),
    });
    run_measurement_any(&set, V2_SHA256, &one_cell(3), transport).unwrap()
}

fn break_codes(slot: &SlotRow) -> Vec<&str> {
    slot.all_rule_breaks
        .iter()
        .map(|b| b.code.as_str())
        .collect()
}

#[test]
fn rows_and_the_cell_report_carry_the_all_rules_count_beside_the_first_rule() {
    let (report, rows) = run_three_turns();
    let cell = &report.cells[0];
    assert_eq!(
        (cell.slots, cell.accepted_slots, cell.rule_rejects),
        (6, 4, 2)
    );

    // The first-rule count is as before: one code per rejected line.
    assert_eq!(
        cell.rejects_by_code,
        [
            ("invalid_punctuation".to_string(), 1),
            ("quantifier_before_resource_or_half".to_string(), 1),
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        rows[1].scene.reject_code.as_deref(),
        Some("quantifier_before_resource_or_half")
    );
    assert_eq!(
        rows[2].scene.reject_code.as_deref(),
        Some("invalid_punctuation")
    );

    // The all-rules list: the first entry is the recorded first rule, the rest were hidden.
    assert_eq!(
        break_codes(&rows[1].scene),
        [
            "quantifier_before_resource_or_half",
            "numbers_digits_or_units",
            "unauthorised_resource_noun"
        ]
    );
    assert_eq!(
        break_codes(&rows[2].scene),
        [
            "invalid_punctuation",
            "numbers_digits_or_units",
            "personal_pronoun"
        ]
    );
    for row in &rows {
        for slot in [&row.scene, &row.outcome] {
            if slot.status == "accepted" {
                assert!(slot.all_rule_breaks.is_empty());
            } else {
                assert_eq!(
                    slot.all_rule_breaks.first().map(|b| b.code.as_str()),
                    slot.reject_code.as_deref()
                );
            }
        }
    }

    assert_eq!(cell.lines_checked, 6);
    assert_eq!(cell.lines_breaking_more_than_one_rule, 2);
    assert_eq!(
        cell.rejects_all_rules,
        [
            ("invalid_punctuation".to_string(), 1),
            ("numbers_digits_or_units".to_string(), 2),
            ("personal_pronoun".to_string(), 1),
            ("quantifier_before_resource_or_half".to_string(), 1),
            ("unauthorised_resource_noun".to_string(), 1),
        ]
        .into_iter()
        .collect()
    );
    // "one coin" is the only line where the unit rule fires on coin.
    assert_eq!(cell.unit_rule_coin_rejects, 1);
}

#[test]
fn openings_and_turn_latency_are_reported_per_cell() {
    assert_eq!(opening_key("Mud grips the wheels."), "mud grips the");
    assert_eq!(opening_key("Mud grips, the wheels."), "mud grips the");
    assert_eq!(opening_key("The team holds."), "the team holds");
    assert_eq!(opening_key("Rain."), "rain");

    let (report, rows) = run_three_turns();
    let cell = &report.cells[0];
    // Accepted: "Mud grips the wheels." / "Mud grips the wheels again." / "Wind crosses the
    // grass." / "The team holds.": three openings, two of the four lines unique.
    assert_eq!(cell.accepted_lines, 4);
    assert_eq!(cell.distinct_openings, 3);
    assert_eq!(cell.unique_opening_share, 0.5);
    assert_eq!(
        cell.top_openings,
        vec![OpeningCount {
            opening: "mud grips the".to_string(),
            count: 2
        }]
    );

    // One latency per turn: the two calls together.
    assert_eq!(cell.latency.calls, 6);
    assert_eq!(cell.latency_turn.calls, 3);
    for row in &rows {
        assert_eq!(
            row.latency_ms_turn,
            Some(row.scene.latency_ms.unwrap() + row.outcome.latency_ms.unwrap())
        );
    }
}

#[test]
fn a_report_rebuilt_from_the_rows_equals_the_report_the_run_wrote() {
    let (report, rows) = run_three_turns();
    assert_eq!(cell_reports_from_rows(&rows), report.cells);
}

// ---- paired comparison -------------------------------------------------------------------------

fn slot(accepted: bool) -> SlotRow {
    SlotRow {
        status: if accepted { "accepted" } else { "rule_reject" }.to_string(),
        reject_code: None,
        reject_detail: None,
        latency_ms: None,
        raw_response: None,
        candidate: None,
        seed: None,
        request: None,
        all_rule_breaks: Vec::<RuleBreak>::new(),
    }
}

fn row(turn: &str, scene: bool, outcome: bool) -> TurnRow {
    TurnRow {
        profile: "Fireside".to_string(),
        length: "standard".to_string(),
        turn_id: turn.to_string(),
        scene: slot(scene),
        outcome: slot(outcome),
        latency_ms_turn: None,
    }
}

/// Four turns, two lines each.
/// Run a: pass rates 0, 0.5, 1, 1. Run b: 0.5, 0.5, 1, 0.
/// Differences b - a: +0.5, 0, 0, -1. Mean -0.125.
/// Squared deviations from the mean: 0.390625, 0.015625, 0.015625, 0.765625; sum 1.1875.
/// Sample variance 1.1875 / 3 = 0.3958333; standard deviation 0.6291529.
/// Half-width 1.96 x 0.6291529 / sqrt(4) = 0.6165698.
#[test]
fn the_paired_difference_is_per_turn_with_a_95_percent_interval() {
    let a = vec![
        row("t1", false, false),
        row("t2", true, false),
        row("t3", true, true),
        row("t4", true, true),
    ];
    let b = vec![
        row("t1", true, false),
        row("t2", false, true),
        row("t3", true, true),
        row("t4", false, false),
        // A turn only run b has is left out of the comparison and counted.
        row("t5", true, true),
    ];
    assert_eq!(turn_pass_counts(&a)["t2"], (1, 2));
    assert_eq!(mean_turn_pass_rate(&a), 0.625);
    assert_eq!(mean_turn_pass_rate(&b[..4]), 0.5);

    let p = paired_difference(&a, &b);
    assert_eq!((p.turns, p.turns_only_in_a, p.turns_only_in_b), (4, 0, 1));
    assert_eq!(p.mean_pass_rate_a, 0.625);
    assert_eq!(p.mean_pass_rate_b, 0.5);
    assert_eq!(p.mean_difference, -0.125);
    let close = |x: f64, y: f64| (x - y).abs() < 1e-6;
    assert!(close(p.sd_difference, 0.6291529), "{}", p.sd_difference);
    assert!(close(p.ci95_low, -0.125 - 0.6165698), "{}", p.ci95_low);
    assert!(close(p.ci95_high, -0.125 + 0.6165698), "{}", p.ci95_high);

    // A run compared with itself differs by nothing.
    let same = paired_difference(&a, &a);
    assert_eq!(
        (
            same.mean_difference,
            same.sd_difference,
            same.ci95_low,
            same.ci95_high
        ),
        (0.0, 0.0, 0.0, 0.0)
    );
}

/// The turn is the unit: more lines on a turn do not make more evidence. Two rows for one turn
/// (two cells) pool into that turn's one pass rate.
#[test]
fn several_rows_for_a_turn_pool_into_one_pass_rate() {
    let a = vec![
        row("t1", true, true),
        row("t1", false, false),
        row("t2", true, true),
    ];
    assert_eq!(turn_pass_counts(&a)["t1"], (2, 4));
    assert_eq!(mean_turn_pass_rate(&a), 0.75);
    let b = vec![row("t1", true, true), row("t2", true, true)];
    let p = paired_difference(&a, &b);
    assert_eq!(p.turns, 2);
    assert_eq!(p.mean_difference, 0.25);
}

// ---- the command line --------------------------------------------------------------------------

#[test]
fn summarise_rows_reads_files_calls_no_model_and_can_pair_two_runs() {
    let dir = std::env::temp_dir().join(format!("etv-measure-a-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rows_path = dir.join("rows.jsonl");
    let report_path = dir.join("report.json");
    let set_path = format!(
        "{}/tests/fixtures/narrator_turn_set_v2.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();

    // A run writes its rows and report.
    let run_args = s(&[
        "--turn-set",
        &set_path,
        "--turn-set-sha256",
        V2_SHA256,
        "--model",
        "fixture-model",
        "--profiles",
        "fireside",
        "--lengths",
        "standard",
        "--max-turns",
        "3",
        "--rows",
        rows_path.to_str().unwrap(),
        "--report",
        report_path.to_str().unwrap(),
    ]);
    let transport = Arc::new(Script {
        answers: ANSWERS.to_vec(),
        next: Mutex::new(0),
    });
    let mut out = Vec::new();
    run_cli(&run_args, transport, &mut out).unwrap();
    let report: MeasureReport =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();

    // Strip the new fields from the rows file, as a file written before they existed.
    let old_style: String = std::fs::read_to_string(&rows_path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
            v.as_object_mut().unwrap().remove("latency_ms_turn");
            for slot in ["scene", "outcome"] {
                v[slot].as_object_mut().unwrap().remove("all_rule_breaks");
            }
            format!("{v}\n")
        })
        .collect();
    let old_path = dir.join("rows-old.jsonl");
    std::fs::write(&old_path, old_style).unwrap();

    // Summarising calls no model: this transport fails every call.
    struct NoCalls;
    impl NarratorTransport for NoCalls {
        fn get_tags(&self, _e: &str, _t: Duration) -> Result<(u16, String), TransportError> {
            panic!("summarise-rows asked for the model list");
        }
        fn post_generate(
            &self,
            _e: &str,
            _b: &str,
            _t: Duration,
        ) -> Result<(u16, String), TransportError> {
            panic!("summarise-rows called the model");
        }
    }
    let sum_args = s(&[
        "--turn-set",
        &set_path,
        "--turn-set-sha256",
        V2_SHA256,
        "--summarise-rows",
        old_path.to_str().unwrap(),
        "--paired-with",
        rows_path.to_str().unwrap(),
    ]);
    let mut out = Vec::new();
    run_cli(&sum_args, Arc::new(NoCalls), &mut out).unwrap();
    let summary: RowsSummary = serde_json::from_slice(&out).unwrap();
    assert_eq!(summary.schema, ROWS_SUMMARY_SCHEMA);
    assert_eq!(summary.turn_set_sha256, V2_SHA256);
    assert_eq!((summary.rows, summary.turns), (3, 3));
    // The old-style rows get their all-rules lists back from the turn set and the checker, and
    // the rebuilt cell equals the one the run reported.
    assert_eq!(summary.cells, report.cells);
    // Turn pass rates: 1, 0.5, 0.5.
    assert!((summary.mean_turn_pass_rate - 2.0 / 3.0).abs() < 1e-12);
    let paired = summary.paired.unwrap();
    assert_eq!(paired.turns, 3);
    assert_eq!(
        (paired.mean_difference, paired.ci95_low, paired.ci95_high),
        (0.0, 0.0, 0.0)
    );

    // --paired-with alone is refused, and a row for a turn outside the set is an error.
    let mut out = Vec::new();
    let err = run_cli(
        &s(&[
            "--turn-set",
            &set_path,
            "--turn-set-sha256",
            V2_SHA256,
            "--paired-with",
            rows_path.to_str().unwrap(),
        ]),
        Arc::new(NoCalls),
        &mut out,
    )
    .unwrap_err();
    assert_eq!(err, "--paired-with needs --summarise-rows");

    let stray = dir.join("stray.jsonl");
    std::fs::write(
        &stray,
        format!(
            "{}\n",
            serde_json::to_string(&row("not-a-turn", true, true)).unwrap()
        ),
    )
    .unwrap();
    let mut out = Vec::new();
    let err = run_cli(
        &s(&[
            "--turn-set",
            &set_path,
            "--turn-set-sha256",
            V2_SHA256,
            "--summarise-rows",
            stray.to_str().unwrap(),
        ]),
        Arc::new(NoCalls),
        &mut out,
    )
    .unwrap_err();
    assert_eq!(err, "turn \"not-a-turn\" is not in the turn set");

    for f in [&rows_path, &report_path, &old_path, &stray] {
        std::fs::remove_file(f).unwrap();
    }
    std::fs::remove_dir(&dir).unwrap();
}
