//! Tests for the content-diff renderer (`etv_core::content_diff`) and its binary
//! `etv_content_diff`. Every expected value is a literal.

use std::fs;
use std::path::Path;
use std::process::Command;

use etv_core::content::{embedded_content_pack, ContentPack, EMBEDDED_CONTENT_HASH_HEX};
use etv_core::content_diff::{
    diff_packs, html_escape, md_cell, mechanical_flags, render_html, render_markdown, run_cli,
    sentence_count, word_count, DiffOptions,
};
use serde_json::{json, Value};

const MANIFEST: &str = r#"{"schema_version":1,"files":["event_skeletons.json"]}"#;

fn pack(events: Value) -> ContentPack {
    ContentPack::from_raw_strings(MANIFEST, &events.to_string()).expect("fixture pack loads")
}

fn event(id: &str, title: &str, choices: Vec<Value>) -> Value {
    json!({
        "id": id,
        "title": title,
        "tags": ["river", "survival"],
        "preconditions": [],
        "weirdness_band": 1,
        "narration_seed": format!("Fixture narration {id}."),
        "choices": choices,
    })
}

#[allow(clippy::too_many_arguments)]
fn choice(
    label: &str,
    action: &str,
    effects: Value,
    cost: &str,
    risk: &str,
    sentence: &str,
) -> Value {
    json!({
        "label": label,
        "intent_action": action,
        "engine_effect_profile": effects,
        "cost_hint": cost,
        "risk_hint": risk,
        "outcome_sentence": sentence,
    })
}

/// The two-event base pack.
fn base_events() -> Value {
    let e1 = event(
        "e1",
        "Fixture Event One",
        vec![
            choice(
                "Fixture label A",
                "WAIT",
                json!({"time_days": 1, "food": -1}),
                "Time",
                "Safe",
                "Fixture sentence one. Fixture sentence two.",
            ),
            choice(
                "Fixture label B",
                "FORD",
                json!({"morale": 2}),
                "Mud",
                "Wet",
                "Fixture sentence five. Fixture sentence six.",
            ),
        ],
    );
    let mut e2 = event(
        "e2",
        "Fixture Event Two",
        vec![choice(
            "Fixture label C",
            "REST",
            json!({"health": 3}),
            "Rest",
            "Calm",
            "Fixture sentence seven. Fixture sentence eight.",
        )],
    );
    e2["weirdness_band"] = json!(0);
    e2["tags"] = json!(["human"]);
    json!([e1, e2])
}

const HEADER_RULE: &str =
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |";
const HEADER_ROW: &str = "| Event | Title | Band | Tags | Narration | Choice | Label | Action | Effects | Cost hint | Risk hint | Outcome sentence | Words | Sentences | Flags |";

// ── 1. embedded against itself ──────────────────────────────────────────────

#[test]
fn embedded_against_itself_reports_zero_changes() {
    let p = embedded_content_pack();
    let d = diff_packs(p, p, DiffOptions::default());
    assert_eq!(d.new_events, 200);
    assert_eq!(d.new_choices, 464);
    assert_eq!(d.changed_choices, 0);
    assert!(d.rows.is_empty());
    assert!(!d.mechanics_changed);
    let md = render_markdown(&d);
    assert!(md.contains(&format!(
        "- old content hash: `{EMBEDDED_CONTENT_HASH_HEX}`\n"
    )));
    assert!(md.contains(&format!(
        "- new content hash: `{EMBEDDED_CONTENT_HASH_HEX}`\n"
    )));
    assert!(md.contains("- events: old 200, new 200\n"));
    assert!(md.contains("- choices: old 464, new 464\n"));
    assert!(md.contains("- changed choices: 0\n"));
    assert!(md.contains("- added events: 0\n"));
    assert!(md.contains("- removed events: 0\n"));
    assert!(md.contains("- mechanics changed: no\n"));
}

#[test]
fn all_flag_lists_every_embedded_choice() {
    let p = embedded_content_pack();
    let d = diff_packs(p, p, DiffOptions { all: true });
    assert_eq!(d.rows.len(), 464);
    assert_eq!(d.changed_choices, 0);
    let md = render_markdown(&d);
    assert!(md.contains("## All choices\n"));
    // First embedded choice, as shipped (no outcome sentence yet, so EMPTY).
    assert!(md.contains(
        "| f1_001 | Shallow Ford | 0 | river, ford, survival | The river runs low but the mud pulls at the wheels with sullen persistence. | A | Drive the team through the ford | FORD | animals_health -2, time_days +1, wagon_condition -1 |  |  |  | 0 | 0 | EMPTY |\n"
    ));
}

// ── 2 and 3. hand-built pairs ───────────────────────────────────────────────

#[test]
fn sentence_hint_and_effect_change_renders_exact_row_and_mechanics_yes() {
    let old = pack(base_events());
    let mut ev = base_events();
    ev[0]["choices"][0]["outcome_sentence"] =
        json!("Fixture sentence three. Fixture sentence four.");
    ev[0]["choices"][0]["cost_hint"] = json!("Time and food");
    ev[0]["choices"][0]["engine_effect_profile"]["food"] = json!(-2);
    let new = pack(ev);

    let d = diff_packs(&old, &new, DiffOptions::default());
    assert_eq!(d.changed_choices, 1);
    let md = render_markdown(&d);
    let expected_row = "| e1 | Fixture Event One | 1 | river, survival | Fixture narration e1. | A | Fixture label A | WAIT | food -1, time_days +1 -> food -2, time_days +1 | Time -> Time and food | Safe | Fixture sentence one. Fixture sentence two. -> Fixture sentence three. Fixture sentence four. | 6 | 2 |  |\n";
    let expected_table = format!(
        "## Changed choices\n\n{HEADER_ROW}\n{HEADER_RULE}\n{expected_row}\n## Added events\n"
    );
    assert!(md.contains(&expected_table), "got:\n{md}");
    assert!(md.contains("- changed choices: 1\n"));
    assert!(md.contains("- mechanics changed: yes\n"));
    assert!(!md.contains("\r"));
}

#[test]
fn sentence_only_change_says_mechanics_no() {
    let old = pack(base_events());
    let mut ev = base_events();
    ev[0]["choices"][0]["outcome_sentence"] =
        json!("Fixture sentence three. Fixture sentence four.");
    let new = pack(ev);

    let d = diff_packs(&old, &new, DiffOptions::default());
    let md = render_markdown(&d);
    let expected_row = "| e1 | Fixture Event One | 1 | river, survival | Fixture narration e1. | A | Fixture label A | WAIT | food -1, time_days +1 | Time | Safe | Fixture sentence one. Fixture sentence two. -> Fixture sentence three. Fixture sentence four. | 6 | 2 |  |\n";
    assert!(md.contains(expected_row), "got:\n{md}");
    assert!(md.contains("- changed choices: 1\n"));
    assert!(md.contains("- mechanics changed: no\n"));
}

#[test]
fn text_fields_title_narration_label_hints_are_text_only() {
    let old = pack(base_events());
    let mut ev = base_events();
    ev[0]["title"] = json!("Fixture Event One Renamed");
    ev[0]["narration_seed"] = json!("Fixture narration changed.");
    ev[0]["choices"][1]["label"] = json!("Fixture label B2");
    ev[0]["choices"][1]["risk_hint"] = json!("Wet and cold");
    let new = pack(ev);
    let d = diff_packs(&old, &new, DiffOptions::default());
    assert!(!d.mechanics_changed);
    let md = render_markdown(&d);
    assert!(md.contains("- changed event fields: 2\n"));
    assert!(md.contains("| e1 | title | Fixture Event One | Fixture Event One Renamed |\n"));
    assert!(md.contains(
        "| e1 | narration_seed | Fixture narration e1. | Fixture narration changed. |\n"
    ));
    assert!(md.contains(
        "| Fixture label B -> Fixture label B2 | FORD | morale +2 | Mud | Wet -> Wet and cold |"
    ));
}

#[test]
fn tags_action_and_precondition_changes_are_mechanics() {
    let old = pack(base_events());
    for (path, val) in [
        ("tags", json!(["river"])),
        ("preconditions", json!(["has_food"])),
        ("weirdness_band", json!(3)),
    ] {
        let mut ev = base_events();
        ev[0][path] = val;
        let d = diff_packs(&old, &pack(ev), DiffOptions::default());
        assert!(d.mechanics_changed, "{path} must count as mechanics");
    }
    let mut ev = base_events();
    ev[0]["choices"][0]["intent_action"] = json!("REST");
    let d = diff_packs(&old, &pack(ev), DiffOptions::default());
    assert!(d.mechanics_changed);
    assert_eq!(d.changed_choices, 1);
}

// ── 4. flags ────────────────────────────────────────────────────────────────

const OK_SENTENCE: &str = "Fixture sentence one. Fixture sentence two.";

fn flags_of(sentence: Option<&str>) -> Vec<&'static str> {
    mechanical_flags("Fixture label", sentence, "Time", "Safe", false)
}

#[test]
fn flag_digit() {
    assert_eq!(
        flags_of(Some("Fixture sentence 1. Fixture sentence two.")),
        vec!["DIGIT", "PUNCT"]
    );
    assert!(flags_of(Some(OK_SENTENCE)).is_empty());
}

#[test]
fn flag_len() {
    assert_eq!(flags_of(Some("Fixture sentence one.")), vec!["LEN"]);
    assert_eq!(flags_of(Some("One. Two. Three. Four.")), vec!["LEN"]);
    assert!(flags_of(Some("One. Two. Three.")).is_empty());
}

#[test]
fn flag_empty() {
    assert_eq!(flags_of(None), vec!["EMPTY"]);
    assert_eq!(flags_of(Some("   ")), vec!["EMPTY"]);
    assert!(flags_of(Some(OK_SENTENCE)).is_empty());
}

#[test]
fn flag_echo() {
    let echo = mechanical_flags(
        "Fixture label",
        Some("We chose the FIXTURE LABEL path. Fixture sentence two."),
        "Time",
        "Safe",
        false,
    );
    assert_eq!(echo, vec!["ECHO"]);
    let no_echo = mechanical_flags("Fixture label", Some(OK_SENTENCE), "Time", "Safe", false);
    assert!(no_echo.is_empty());
}

#[test]
fn flag_dup() {
    assert_eq!(
        mechanical_flags("Fixture label", Some(OK_SENTENCE), "Time", "Safe", true),
        vec!["DUP"]
    );
    assert!(mechanical_flags("Fixture label", Some(OK_SENTENCE), "Time", "Safe", false).is_empty());

    // Computed over the whole new pack.
    let mut ev = base_events();
    ev[0]["choices"][1]["outcome_sentence"] = json!(OK_SENTENCE);
    let p = pack(ev);
    let d = diff_packs(&p, &p, DiffOptions { all: true });
    let by_choice: Vec<(String, Vec<&str>)> = d
        .rows
        .iter()
        .map(|r| (format!("{}{}", r.event_id, r.choice_id), r.flags.clone()))
        .collect();
    assert_eq!(
        by_choice,
        vec![
            ("e1A".to_string(), vec!["DUP"]),
            ("e1B".to_string(), vec!["DUP"]),
            ("e2A".to_string(), vec![]),
        ]
    );
}

#[test]
fn flag_punct() {
    assert_eq!(
        flags_of(Some("Fixture sentence one! Fixture sentence two.")),
        vec!["PUNCT"]
    );
    assert_eq!(
        flags_of(Some("Fixture sentence one; fine. Fixture sentence two.")),
        vec!["PUNCT"]
    );
    assert!(flags_of(Some(
        "It's fixture sentence one, really. Fixture sentence two."
    ))
    .is_empty());
}

#[test]
fn flag_hint_double_punct() {
    let hit = |cost: &str, risk: &str| mechanical_flags("L", Some(OK_SENTENCE), cost, risk, false);
    assert_eq!(hit("Time..", "Safe"), vec!["HINT_DOUBLE_PUNCT"]);
    assert_eq!(hit("Time", "Safe (maybe.)"), vec!["HINT_DOUBLE_PUNCT"]);
    assert!(hit("Time.", "Safe").is_empty());
    assert!(hit("Time", "Safe)").is_empty());
}

#[test]
fn counters() {
    assert_eq!(word_count("  Fixture   sentence one. "), 3);
    assert_eq!(word_count(""), 0);
    assert_eq!(sentence_count("One. Two! Three?"), 3);
    assert_eq!(sentence_count("One. Two"), 2);
    assert_eq!(sentence_count("..."), 0);
}

// ── 5. escaping ─────────────────────────────────────────────────────────────

#[test]
fn html_escapes_every_text_value_and_has_no_raw_markup() {
    let old = pack(base_events());
    let mut ev = base_events();
    ev[0]["choices"][0]["label"] = json!("<b>&\"'");
    ev[0]["title"] = json!("<i>T</i>");
    let new = pack(ev);
    let html = render_html(&diff_packs(&old, &new, DiffOptions::default()));
    assert!(html.contains("<td class=\"old\">Fixture label A</td>"));
    assert!(html.contains("<td class=\"new\">&lt;b&gt;&amp;&quot;&#39;</td>"));
    assert!(html.contains("<td>&lt;i&gt;T&lt;/i&gt;</td>"));
    assert!(!html.contains("<b>"));
    assert!(!html.contains("<i>"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("http"));
    assert!(!html.contains('\r'));
    assert!(html.starts_with("<!DOCTYPE html>\n"));
    assert_eq!(html_escape("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
}

#[test]
fn html_flags_are_small_labels() {
    let p = pack(base_events());
    let mut ev = base_events();
    ev[0]["choices"][0]["outcome_sentence"] = json!("One.");
    let html = render_html(&diff_packs(&p, &pack(ev), DiffOptions::default()));
    assert!(html.contains("<td><span class=\"flag\">LEN</span></td></tr>"));
}

#[test]
fn markdown_cell_escapes_pipes_and_newlines() {
    assert_eq!(md_cell("a|b\nc\r\nd"), "a\\|b<br>c<br>d");
    let old = pack(base_events());
    let mut ev = base_events();
    ev[0]["choices"][0]["label"] = json!("a|b\nc");
    let md = render_markdown(&diff_packs(&old, &pack(ev), DiffOptions::default()));
    assert!(md.contains("| Fixture label A -> a\\|b<br>c | WAIT |"));
    // Every table line is one physical line: header, rule, one row.
    let rows = md.lines().filter(|l| l.starts_with("| e1 ")).count();
    assert_eq!(rows, 1);
}

// ── 6. added and removed events ─────────────────────────────────────────────

#[test]
fn added_and_removed_events_are_listed() {
    let old = pack(base_events());
    let mut ev = base_events();
    ev[1] = event(
        "e3",
        "Fixture Event Three",
        vec![choice(
            "Fixture label D",
            "WAIT",
            json!({}),
            "Time",
            "Safe",
            OK_SENTENCE,
        )],
    );
    let new = pack(ev);
    let d = diff_packs(&old, &new, DiffOptions::default());
    assert_eq!(d.added.len(), 1);
    assert_eq!(d.removed.len(), 1);
    assert!(d.mechanics_changed);
    let md = render_markdown(&d);
    assert!(md.contains("- added events: 1\n"));
    assert!(md.contains("- removed events: 1\n"));
    assert!(md.contains(
        "## Added events\n\n| Event | Title | Band | Choices |\n| --- | --- | --- | --- |\n| e3 | Fixture Event Three | 1 | 1 |\n"
    ));
    assert!(md.contains(
        "## Removed events\n\n| Event | Title | Band | Choices |\n| --- | --- | --- | --- |\n| e2 | Fixture Event Two | 0 | 1 |\n"
    ));
}

#[test]
fn added_and_removed_choices_are_rows() {
    let old = pack(base_events());
    let mut ev = base_events();
    ev[0]["choices"].as_array_mut().unwrap().remove(1);
    ev[1]["choices"].as_array_mut().unwrap().push(choice(
        "Fixture label E",
        "WAIT",
        json!({"food": 1}),
        "Time",
        "Safe",
        OK_SENTENCE,
    ));
    let d = diff_packs(&old, &pack(ev), DiffOptions::default());
    assert_eq!(d.changed_choices, 2);
    let md = render_markdown(&d);
    assert!(md.contains("| B | Fixture label B -> (none) | FORD -> (none) | morale +2 -> (none) |"));
    assert!(md.contains("| B | (none) -> Fixture label E | (none) -> WAIT | (none) -> food +1 |"));
}

// ── 7. the binary ───────────────────────────────────────────────────────────

fn write_dir(dir: &Path, events: &Value) {
    fs::write(dir.join("content_manifest.json"), MANIFEST).unwrap();
    fs::write(dir.join("event_skeletons.json"), events.to_string()).unwrap();
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_etv_content_diff"))
}

fn s(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

#[test]
fn binary_bad_arguments_exit_2() {
    for args in [
        vec![],
        vec!["only_one_dir"],
        vec!["a", "b", "c"],
        vec!["a", "b", "--bogus"],
        vec!["a", "b", "--md"],
        vec!["a", "b", "--html"],
    ] {
        let out = bin().args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "args {args:?}");
        assert!(out.stdout.is_empty());
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.starts_with("etv_content_diff: "), "got {err}");
        assert!(err.contains("Usage: etv_content_diff"));
    }
    let out = bin().args(["a", "b", "--bogus"]).output().unwrap();
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("unknown option --bogus"));
    let out = bin().args(["a", "b", "--md"]).output().unwrap();
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("--md needs a file path"));
    let out = bin().output().unwrap();
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("expected exactly two content directories (old and new), got 0"));
}

#[test]
fn binary_help_prints_usage_and_exits_0() {
    let out = bin().arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with(
        "Usage: etv_content_diff <old_dir> <new_dir> [--md <out.md>] [--html <out.html>] [--all]\n"
    ));
    assert!(out.stderr.is_empty());
}

#[test]
fn binary_unreadable_or_invalid_content_exits_1_naming_the_path() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nope");
    let missing_s = missing.to_str().unwrap().to_string();
    let good = tmp.path().join("good");
    fs::create_dir(&good).unwrap();
    write_dir(&good, &base_events());
    let good_s = good.to_str().unwrap().to_string();

    let out = bin().args([&good_s, &missing_s]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("cannot read"), "got {err}");
    assert!(err.contains(&missing_s), "got {err}");

    let bad = tmp.path().join("bad");
    fs::create_dir(&bad).unwrap();
    write_dir(&bad, &base_events());
    fs::write(bad.join("event_skeletons.json"), "{ not json").unwrap();
    let bad_s = bad.to_str().unwrap().to_string();
    let out = bin().args([&bad_s, &good_s]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("invalid JSON in"), "got {err}");
    assert!(err.contains(&bad_s), "got {err}");

    // Valid JSON of the wrong shape.
    let odd = tmp.path().join("odd");
    fs::create_dir(&odd).unwrap();
    write_dir(&odd, &json!("a string"));
    let odd_s = odd.to_str().unwrap().to_string();
    let out = bin().args([&good_s, &odd_s]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("invalid content in"), "got {err}");
    assert!(err.contains(&odd_s), "got {err}");
}

#[test]
fn binary_writes_markdown_to_stdout_by_default_and_files_on_request() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    write_dir(&a, &base_events());
    let mut ev = base_events();
    ev[0]["choices"][0]["outcome_sentence"] =
        json!("Fixture sentence three. Fixture sentence four.");
    write_dir(&b, &ev);
    let (a_s, b_s) = (a.to_str().unwrap(), b.to_str().unwrap());

    let out = bin().args([a_s, b_s]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("# Content diff\n\n"));
    assert!(stdout.contains("- mechanics changed: no\n"));
    assert!(!stdout.contains('\r'));

    let md_path = tmp.path().join("out.md");
    let html_path = tmp.path().join("out.html");
    let out = bin()
        .args([
            a_s,
            b_s,
            "--md",
            md_path.to_str().unwrap(),
            "--html",
            html_path.to_str().unwrap(),
            "--all",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let md = fs::read(&md_path).unwrap();
    let html = fs::read(&html_path).unwrap();
    assert!(String::from_utf8(md.clone())
        .unwrap()
        .contains("## All choices\n"));
    assert!(String::from_utf8(html.clone())
        .unwrap()
        .contains("<h2>All choices</h2>\n"));
    assert!(!md.contains(&b'\r'));
    assert!(!html.contains(&b'\r'));

    // Output is deterministic.
    let again = bin().args([a_s, b_s, "--all"]).output().unwrap();
    assert_eq!(again.stdout, md);

    // Unwritable output path: exit 1, path named.
    let bad_out = tmp.path().join("no_such_dir").join("x.md");
    let out = bin()
        .args([a_s, b_s, "--md", bad_out.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("cannot write"));
}

#[test]
fn run_cli_in_process_matches_binary_contract() {
    assert_eq!(run_cli(&s(&["--help"])).code, 0);
    assert_eq!(run_cli(&s(&[])).code, 2);
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("content");
    let c = content.to_str().unwrap();
    let out = run_cli(&s(&[c, c]));
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("- events: old 200, new 200\n"));
    assert!(out.stdout.contains("- mechanics changed: no\n"));
    assert!(out.stderr.is_empty());
}
