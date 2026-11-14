//! Phase 3: the live narrator connected to the play loop.
//!
//! Every test drives the narrator through a scripted transport. Nothing here opens a socket or
//! calls a model, and the scripted text is fixture text, not model text.

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::model::GMProfile;
use etv_core::narrator::{
    FallbackPhraseTable, NarratorLength, NarratorSettings, NarratorStats, NarratorTransport,
    NullNarrator, TransportError,
};
use etv_core::play::{
    connect_narrator, narrator_summary_line, run_play, PlayExit, NARRATOR_OFF_LINE,
};
use etv_core::save::state_to_value;
use etv_core::worldgen::create_new_run_with_content;

const SCENE_LINE: &str = "The fixture scene holds steady.";
const OUTCOME_LINE: &str = "The fixture outcome holds steady.";

/// What the scripted server does.
#[derive(Clone, Copy, PartialEq)]
enum Script {
    /// Answers every call with lines that pass the hard rules.
    Good,
    /// The service is not there.
    NoService,
    /// The service answers, but the model is not in its list.
    ModelMissing,
    /// The model never finishes loading.
    WarmupTimeout,
    /// Outcome lines carry a digit, which the hard rules reject; scene lines are fine.
    OutcomeBreaksRule,
    /// Every generation call times out after warm-up.
    CallsTimeOut,
    /// Outcome answers are not JSON.
    OutcomeNotJson,
}

struct ScriptedTransport {
    script: Script,
    tags_calls: AtomicUsize,
    generate_calls: AtomicUsize,
}

impl ScriptedTransport {
    fn new(script: Script) -> Arc<Self> {
        Arc::new(Self {
            script,
            tags_calls: AtomicUsize::new(0),
            generate_calls: AtomicUsize::new(0),
        })
    }
}

fn envelope(inner: serde_json::Value) -> String {
    serde_json::json!({ "response": inner.to_string() }).to_string()
}

impl NarratorTransport for ScriptedTransport {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        self.tags_calls.fetch_add(1, Ordering::SeqCst);
        match self.script {
            Script::NoService => Err(TransportError::Timeout),
            Script::ModelMissing => {
                Ok((200, r#"{"models":[{"name":"other:latest"}]}"#.to_string()))
            }
            _ => Ok((
                200,
                r#"{"models":[{"name":"fixture-model:latest"}]}"#.to_string(),
            )),
        }
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        self.generate_calls.fetch_add(1, Ordering::SeqCst);
        if body_json.contains("Warm up.") {
            return match self.script {
                Script::WarmupTimeout => Err(TransportError::Timeout),
                _ => Ok((200, r#"{"response":""}"#.to_string())),
            };
        }
        if self.script == Script::CallsTimeOut {
            return Err(TransportError::Timeout);
        }
        if body_json.contains("SCENE_SCHEMA") {
            return Ok((
                200,
                envelope(serde_json::json!({
                    "scene_id": "fixture",
                    "title": "Fixture",
                    "narration": SCENE_LINE,
                    "profile": "fireside",
                    "uncanny_intensity": "none",
                    "choices": [],
                    "tags": [],
                    "gm_aside": "",
                    "memory_proposals": []
                })),
            ));
        }
        let narration = match self.script {
            Script::OutcomeBreaksRule => "The fixture outcome costs 3 days.",
            Script::OutcomeNotJson => {
                return Ok((200, r#"{"response":"not json at all"}"#.to_string()))
            }
            _ => OUTCOME_LINE,
        };
        Ok((
            200,
            envelope(serde_json::json!({
                "scene_id": "fixture",
                "outcome_title": "Fixture",
                "outcome_narration": narration,
                "callout": "",
                "oregon_nod": "",
                "memory_proposals": []
            })),
        ))
    }
}

fn settings() -> NarratorSettings {
    NarratorSettings {
        model: "fixture-model".to_string(),
        length: NarratorLength::Brief,
        profile: GMProfile::Chronicler,
        ..NarratorSettings::default()
    }
}

fn new_engine(seed: i128) -> StepEngine {
    let content = embedded_content_pack();
    StepEngine::with_content(create_new_run_with_content(seed, content), content)
}

/// On seed 42 the first travel step brings up Persistent Rain; `a` takes "Continue despite the
/// wet"; the closing `a`, `q` always ends at camp.
const SCRIPT: &str = "t\na\nt\nt\na\nq\n";

fn play(
    script: Script,
) -> (
    String,
    StepEngine,
    Option<NarratorStats>,
    Vec<String>,
    usize,
) {
    let transport = ScriptedTransport::new(script);
    let connection = connect_narrator(&settings(), Some(transport.clone()));
    let table = FallbackPhraseTable::embedded();
    let mut engine = new_engine(42);
    let mut out = Vec::new();
    let exit = match &connection.narrator {
        Some(live) => run_play(
            &mut engine,
            live,
            Some(&table),
            "TEST",
            Cursor::new(SCRIPT),
            &mut out,
        ),
        None => run_play(
            &mut engine,
            &NullNarrator,
            Some(&table),
            "TEST",
            Cursor::new(SCRIPT),
            &mut out,
        ),
    }
    .unwrap();
    assert_eq!(exit, PlayExit::Quit);
    let stats = connection.narrator.as_ref().map(|n| n.stats());
    (
        String::from_utf8(out).unwrap(),
        engine,
        stats,
        connection.lines,
        transport.generate_calls.load(Ordering::SeqCst),
    )
}

const BRACKET_LINE: &str = "[Continue despite the wet] The party damages the wagon, loses heart, falls back on the trail and loses time.";
const NUMBERS_LINE: &str = "  Distance -4, Morale -2, Days +1, Wagon condition -2";

#[test]
fn a_connected_narrator_tells_every_turn_and_the_numbers_stay() {
    let (text, _, stats, lines, generate_calls) = play(Script::Good);
    assert_eq!(
        lines,
        vec!["Narrator: Chronicler, Brief, model fixture-model.".to_string()]
    );
    // The outcome of the first choice is the narrator's line, not the bracketed fallback.
    assert!(
        text.contains(&format!("{OUTCOME_LINE}\n{NUMBERS_LINE}\n")),
        "{text}"
    );
    assert!(!text.contains(BRACKET_LINE));
    assert!(text.contains(SCENE_LINE));
    let stats = stats.unwrap();
    // The script plays four turns on seed 42 (travel into the event, the choice, two more
    // travels; the second `a` is refused at camp and plays no turn). Two calls a turn, scene
    // and outcome, plus the one warm-up call.
    assert_eq!(stats.calls, 8);
    assert_eq!(generate_calls, stats.calls + 1);
    assert_eq!(stats.successes, stats.calls);
    assert_eq!(stats.fallbacks, 0);
}

#[test]
fn a_line_that_breaks_a_hard_rule_is_replaced_by_the_fallback_for_that_turn() {
    let (text, _, stats, _, _) = play(Script::OutcomeBreaksRule);
    assert!(
        text.contains(&format!("{BRACKET_LINE}\n{NUMBERS_LINE}\n")),
        "{text}"
    );
    assert!(!text.contains("costs 3 days"));
    // Scene lines still pass and are still shown.
    assert!(text.contains(SCENE_LINE));
    let stats = stats.unwrap();
    assert_eq!(stats.rule_rejects * 2, stats.calls);
    assert_eq!(stats.fallbacks, stats.rule_rejects);
    assert_eq!(stats.successes, stats.calls - stats.rule_rejects);
}

#[test]
fn timeouts_and_bad_answers_fall_back_and_play_goes_on() {
    let (text, _, stats, _, _) = play(Script::CallsTimeOut);
    assert!(text.contains(BRACKET_LINE));
    assert!(!text.contains(SCENE_LINE));
    let stats = stats.unwrap();
    // Two timeouts in a row pause the narrator for the session: the first turn's two calls are
    // made, and the other three turns ask for nothing. All eight lines fall back.
    assert_eq!(stats.calls, 2);
    assert_eq!(stats.timeouts, 2);
    assert_eq!(stats.fallbacks, 8);
    assert_eq!(stats.successes, 0);

    let (text, _, stats, _, _) = play(Script::OutcomeNotJson);
    assert!(text.contains(BRACKET_LINE));
    let stats = stats.unwrap();
    assert_eq!(stats.json_errors * 2, stats.calls);
    assert_eq!(stats.fallbacks, stats.json_errors);
}

#[test]
fn a_narrator_that_cannot_start_says_why_and_the_game_plays_without_it() {
    let cases = [
        (
            Script::NoService,
            "The narrator service did not answer at http://127.0.0.1:11434. Start it with: ollama serve",
            0usize,
        ),
        (
            Script::ModelMissing,
            "The narrator model 'fixture-model' is not installed in Ollama. Install it with: ollama pull fixture-model",
            0,
        ),
        (
            Script::WarmupTimeout,
            "The narrator model 'fixture-model' did not load in time.",
            1,
        ),
    ];
    for (script, reason, expected_generate_calls) in cases {
        let (text, _, stats, lines, generate_calls) = play(script);
        assert_eq!(
            lines,
            vec![reason.to_string(), NARRATOR_OFF_LINE.to_string()]
        );
        assert!(stats.is_none());
        // No call is made during play once the narrator failed to start.
        assert_eq!(generate_calls, expected_generate_calls);
        assert!(text.contains(BRACKET_LINE));
    }
}

#[test]
fn switched_off_or_no_connection_makes_no_call() {
    let transport = ScriptedTransport::new(Script::Good);
    let off = NarratorSettings {
        enabled: false,
        ..settings()
    };
    let connection = connect_narrator(&off, Some(transport.clone()));
    assert!(connection.narrator.is_none());
    assert_eq!(
        connection.lines,
        vec![
            "The narrator is switched off in the settings.".to_string(),
            NARRATOR_OFF_LINE.to_string()
        ]
    );
    assert_eq!(transport.tags_calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.generate_calls.load(Ordering::SeqCst), 0);

    let connection = connect_narrator(&settings(), None);
    assert!(connection.narrator.is_none());
    assert_eq!(
        connection.lines,
        vec![
            "This build has no narrator connection.".to_string(),
            NARRATOR_OFF_LINE.to_string()
        ]
    );
}

/// Whatever the narrator does, the run is the same run.
#[test]
fn the_narrator_never_changes_the_run() {
    let comparable = |engine: &StepEngine| {
        let mut value = state_to_value(&engine.state);
        value.as_object_mut().unwrap().remove("run_id");
        (value, engine.rng.counter)
    };
    let (_, baseline, _, _, _) = play(Script::NoService);
    for script in [
        Script::Good,
        Script::OutcomeBreaksRule,
        Script::CallsTimeOut,
        Script::OutcomeNotJson,
    ] {
        let (_, engine, _, _, _) = play(script);
        assert!(comparable(&engine) == comparable(&baseline));
    }
}

#[test]
fn summary_line_reports_the_counts() {
    let stats = NarratorStats {
        calls: 12,
        successes: 9,
        timeouts: 1,
        json_errors: 0,
        rule_rejects: 2,
        fallbacks: 3,
    };
    assert_eq!(
        narrator_summary_line(&stats),
        "Narrator: 12 lines asked for, 9 shown, 3 fell back to plain text (1 timed out, 2 broke a rule)."
    );
}

/// The default build has no narrator connection, so the binary says so and plays on.
#[cfg(not(feature = "narrator-ollama"))]
#[test]
fn default_build_binary_says_it_has_no_narrator_connection() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_etv"))
        .args(["new", "--seed", "42"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"t\na\nq\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(stdout.starts_with(&format!(
        "New run. Seed 42.\nThis build has no narrator connection.\n{NARRATOR_OFF_LINE}\nESCAPE THE VALLEY\n"
    )));
    assert!(stdout.contains(BRACKET_LINE));
    assert!(!stdout.contains("Narrator:"));
}
