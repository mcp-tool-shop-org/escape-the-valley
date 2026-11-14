//! What the narrator client sends, and how it reads the answer.
//!
//! Every test uses a transport that records the request and replies from a script. Nothing here
//! opens a socket or calls a model.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::content::embedded_content_pack;
use etv_core::engine::StepMessages;
use etv_core::measure::{run_cli, run_measurement, sha256_hex, MeasureConfig, TurnSet, TurnSpec};
use etv_core::model::{GMProfile, GamePhase};
use etv_core::narrator::{
    build_narration_context_with_phase, build_scene_prompts, build_scene_prompts_for,
    build_system_prompt, build_system_prompt_with_rules, check_generated_line, narration_call_seed,
    narration_format_schema, narration_format_schema_cited, parse_slot_prose, CheckReasonCode,
    CheckerContext, FormatSchema, LiveNarrator, NarrationContext, Narrator, NarratorLength,
    NarratorSettings, NarratorTransport, PromptRules, TransportError, TurnMattersFlags,
    DEFAULT_OLLAMA_ENDPOINT, NARRATOR_NUM_CTX, NARRATOR_NUM_PREDICT,
    NARRATOR_NUM_PREDICT_CITED_EXTRA, NARRATOR_NUM_PREDICT_RICH, NARRATOR_TEMPERATURE,
    TIMEOUT_BREAKER_STRIKES,
};
use etv_core::worldgen::{create_new_run_with_content, FIRST_NAMES};
use serde_json::{json, Value};

/// How the scripted server answers a generation call.
#[derive(Clone, Copy)]
enum Reply {
    Sentences,
    Timeout,
    /// Times out on the first `n` generation calls after warm-up, then answers.
    TimeoutThenOk(usize),
    /// Answers with bare text, no JSON, as a model does when no schema is asked for.
    Text(&'static str),
}

struct Recorder {
    reply: Reply,
    bodies: Mutex<Vec<Value>>,
    endpoints: Mutex<Vec<String>>,
    generate_calls: AtomicUsize,
}

impl Recorder {
    fn new(reply: Reply) -> Arc<Self> {
        Arc::new(Self {
            reply,
            bodies: Mutex::new(Vec::new()),
            endpoints: Mutex::new(Vec::new()),
            generate_calls: AtomicUsize::new(0),
        })
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().unwrap().clone()
    }
}

impl NarratorTransport for Recorder {
    fn get_tags(&self, endpoint: &str, _t: Duration) -> Result<(u16, String), TransportError> {
        self.endpoints.lock().unwrap().push(endpoint.to_string());
        Ok((
            200,
            r#"{"models":[{"name":"fixture-model:latest"}]}"#.to_string(),
        ))
    }

    fn post_generate(
        &self,
        endpoint: &str,
        body_json: &str,
        _t: Duration,
    ) -> Result<(u16, String), TransportError> {
        self.endpoints.lock().unwrap().push(endpoint.to_string());
        let body: Value = serde_json::from_str(body_json).expect("request body is JSON");
        let is_warmup = body["prompt"] == "Warm up.";
        self.bodies.lock().unwrap().push(body);
        if is_warmup {
            return Ok((200, r#"{"response":""}"#.to_string()));
        }
        let n = self.generate_calls.fetch_add(1, Ordering::SeqCst);
        let timed_out = match self.reply {
            Reply::Sentences | Reply::Text(_) => false,
            Reply::Timeout => true,
            Reply::TimeoutThenOk(first) => n < first,
        };
        if timed_out {
            return Err(TransportError::Timeout);
        }
        if let Reply::Text(text) = self.reply {
            return Ok((200, json!({ "response": text }).to_string()));
        }
        let inner = json!({"sentences": ["The fixture line holds steady."]}).to_string();
        Ok((200, json!({ "response": inner }).to_string()))
    }
}

fn settings(length: NarratorLength) -> NarratorSettings {
    NarratorSettings {
        model: "fixture-model".to_string(),
        profile: GMProfile::Chronicler,
        length,
        ..NarratorSettings::default()
    }
}

fn context(seed: i128, day: i32) -> NarrationContext {
    let mut state = create_new_run_with_content(seed, embedded_content_pack());
    state.day = day;
    let msgs = StepMessages {
        outcome_narration: "Fixture fallback.".to_string(),
        ..StepMessages::default()
    };
    build_narration_context_with_phase(
        &state,
        GamePhase::Camp,
        "f1_001",
        "A",
        &msgs,
        TurnMattersFlags::default(),
    )
}

/// The two generation requests (scene, outcome) of one narrated turn.
fn turn_requests(length: NarratorLength, ctx: &NarrationContext) -> (Value, Value) {
    let rec = Recorder::new(Reply::Sentences);
    let narrator = LiveNarrator::with_transport(settings(length), rec.clone());
    let told = narrator.narrate_turn(ctx);
    if length == NarratorLength::Brief {
        // The scripted answer is one sentence, which is what Brief allows.
        let line = Some("The fixture line holds steady.");
        assert_eq!(told.scene.as_deref(), line);
        assert_eq!(told.outcome.as_deref(), line);
    }
    let bodies = rec.bodies();
    assert_eq!(bodies.len(), 2);
    (bodies[0].clone(), bodies[1].clone())
}

#[test]
fn format_is_a_full_json_schema_with_a_bounded_sentence_array() {
    // The length setting caps words, which a schema cannot count; the array is only bounded.
    for (length, min, max) in [
        (NarratorLength::Brief, 1, 8),
        (NarratorLength::Standard, 1, 8),
        (NarratorLength::Rich, 1, 8),
    ] {
        let expected = json!({
            "type": "object",
            "properties": {
                "sentences": {
                    "type": "array",
                    "items": {"type": "string"},
                    "minItems": min,
                    "maxItems": max
                }
            },
            "required": ["sentences"],
            "additionalProperties": false
        });
        assert_eq!(narration_format_schema(length), expected);
        let (scene, outcome) = turn_requests(length, &context(42, 1));
        assert_eq!(scene["format"], expected);
        assert_eq!(outcome["format"], expected);
        assert_ne!(scene["format"], json!("json"));
    }
}

#[test]
fn every_call_turns_thinking_off_and_caps_context_and_length() {
    assert_eq!(NARRATOR_TEMPERATURE, 0.15);
    assert_eq!(NARRATOR_NUM_CTX, 2048);
    assert_eq!(NARRATOR_NUM_PREDICT, 110);
    let (scene, outcome) = turn_requests(NarratorLength::Standard, &context(42, 1));
    for body in [&scene, &outcome] {
        assert_eq!(body["think"], json!(false));
        assert_eq!(body["stream"], json!(false));
        assert_eq!(body["model"], json!("fixture-model"));
        assert_eq!(body["options"]["temperature"], json!(0.15));
        assert_eq!(body["options"]["num_ctx"], json!(2048));
        assert_eq!(body["options"]["num_predict"], json!(110));
        // One stateless request: no carried conversation.
        assert!(body.get("context").is_none());
        assert!(body.get("messages").is_none());
        let keys: Vec<&str> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["format", "model", "options", "prompt", "stream", "system", "think"]
        );
    }

    // The warm-up call turns thinking off too.
    let rec = Recorder::new(Reply::Sentences);
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec.clone());
    narrator.warmup().unwrap();
    let bodies = rec.bodies();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["think"], json!(false));
}

#[test]
fn the_default_endpoint_is_the_ipv4_loopback_address() {
    assert_eq!(DEFAULT_OLLAMA_ENDPOINT, "http://127.0.0.1:11434");
    assert_eq!(
        NarratorSettings::default().endpoint,
        "http://127.0.0.1:11434"
    );
    let rec = Recorder::new(Reply::Sentences);
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec.clone());
    assert!(narrator.check_startup().available);
    narrator.narrate_turn(&context(42, 1));
    let endpoints = rec.endpoints.lock().unwrap().clone();
    assert_eq!(endpoints.len(), 3);
    assert!(endpoints.iter().all(|e| e == "http://127.0.0.1:11434"));
}

#[test]
fn the_sampling_seed_repeats_for_the_same_run_and_turn_and_differs_otherwise() {
    let ctx = context(42, 3);
    let (scene_a, outcome_a) = turn_requests(NarratorLength::Standard, &ctx);
    let (scene_b, outcome_b) = turn_requests(NarratorLength::Standard, &context(42, 3));
    // Same run seed and same turn: the same seeds again.
    assert_eq!(scene_a["options"]["seed"], scene_b["options"]["seed"]);
    assert_eq!(outcome_a["options"]["seed"], outcome_b["options"]["seed"]);
    // The seed sent is the derived one, and it fits in 31 bits.
    let scene_seed =
        narration_call_seed(&ctx, GMProfile::Chronicler, NarratorLength::Standard, true);
    let outcome_seed =
        narration_call_seed(&ctx, GMProfile::Chronicler, NarratorLength::Standard, false);
    assert_eq!(scene_a["options"]["seed"], json!(scene_seed));
    assert_eq!(outcome_a["options"]["seed"], json!(outcome_seed));
    assert!(scene_seed <= 0x7fff_ffff && outcome_seed <= 0x7fff_ffff);
    // Scene and outcome, another turn, another run, another profile or length: different seeds.
    let mut seen = std::collections::BTreeSet::new();
    for seed in [
        scene_seed,
        outcome_seed,
        narration_call_seed(
            &context(42, 4),
            GMProfile::Chronicler,
            NarratorLength::Standard,
            true,
        ),
        narration_call_seed(
            &context(43, 3),
            GMProfile::Chronicler,
            NarratorLength::Standard,
            true,
        ),
        narration_call_seed(&ctx, GMProfile::Fireside, NarratorLength::Standard, true),
        narration_call_seed(&ctx, GMProfile::Chronicler, NarratorLength::Rich, true),
    ] {
        assert!(seen.insert(seed), "seed {seed} repeated");
    }
    assert_eq!(ctx.run_seed(), 42);
}

#[test]
fn the_run_seed_never_reaches_a_prompt() {
    // 987654321 appears nowhere else in the turn facts, so finding it would mean a leak.
    let ctx = context(987_654_321, 2);
    let (scene, outcome) = turn_requests(NarratorLength::Rich, &ctx);
    for body in [&scene, &outcome] {
        assert!(!body["system"].as_str().unwrap().contains("987654321"));
        assert!(!body["prompt"].as_str().unwrap().contains("987654321"));
    }
}

#[test]
fn answers_are_read_as_a_sentence_array_and_the_older_shape_still_parses() {
    assert_eq!(
        parse_slot_prose(r#"{"sentences": ["One holds.", "  Two follows. "]}"#, true),
        Ok("One holds. Two follows.".to_string())
    );
    assert_eq!(
        parse_slot_prose(
            r#"{"cited_fact_keys": ["place"], "sentences": ["One holds."]}"#,
            false
        ),
        Ok("One holds.".to_string())
    );
    for bad in [
        r#"{"sentences": []}"#,
        r#"{"sentences": "One holds."}"#,
        r#"{"sentences": ["One holds.", 2]}"#,
        r#"{"sentences": ["One holds.", "  "]}"#,
        r#"["One holds."]"#,
        "One holds.",
        r#"{"other": "x"}"#,
    ] {
        assert!(parse_slot_prose(bad, true).is_err(), "{bad}");
        assert!(parse_slot_prose(bad, false).is_err(), "{bad}");
    }
    // The older shapes, each only in its own slot.
    let scene = r#"{"scene_id":"s","title":"T","narration":"One holds.","profile":"fireside","uncanny_intensity":"none","choices":[],"tags":[],"gm_aside":"","memory_proposals":[]}"#;
    let outcome = r#"{"scene_id":"s","outcome_title":"T","outcome_narration":"Two follows.","callout":"","oregon_nod":"","memory_proposals":[]}"#;
    assert_eq!(parse_slot_prose(scene, true), Ok("One holds.".to_string()));
    assert_eq!(
        parse_slot_prose(outcome, false),
        Ok("Two follows.".to_string())
    );
    assert!(parse_slot_prose(outcome, true).is_err());
}

#[test]
fn the_shipped_rules_wording_is_unchanged_and_the_positive_wording_names_no_banned_word() {
    let ctx = context(42, 1);
    let shipped = build_system_prompt(GMProfile::Fireside, NarratorLength::Standard, &ctx);
    assert_eq!(
        shipped,
        build_system_prompt_with_rules(
            GMProfile::Fireside,
            NarratorLength::Standard,
            &ctx,
            PromptRules::Shipped
        )
    );
    // The shipped wording names words and marks it bans.
    for named in ["Never use half", "death", "magic", "monsters", "semicolons"] {
        assert!(shipped.contains(named), "{named}");
    }

    let positive = build_system_prompt_with_rules(
        GMProfile::Fireside,
        NarratorLength::Standard,
        &ctx,
        PromptRules::Positive,
    );
    let rules_start = positive
        .find("SHARED VOICE RULES:")
        .expect("positive rules block");
    let rules_end = positive.find("TURN FACTS:").unwrap();
    let rules = positive[rules_start..rules_end].to_ascii_lowercase();
    for banned in [
        "never",
        "half",
        "death",
        "ruin",
        "magic",
        "monster",
        "horror",
        "semicolon",
        "exclamation",
        "question",
        "colon",
        "quote",
        "ellips",
        "digit",
        " no ",
        "not ",
    ] {
        assert!(!rules.contains(banned), "positive rules name {banned:?}");
    }
    assert!(positive.contains("LENGTH CAP: Standard (at most 45 words in all)."));
    // Same profile header and same turn facts either way.
    assert_eq!(
        &shipped[shipped.find("TURN FACTS:").unwrap()..],
        &positive[rules_end..]
    );
    assert_eq!(
        &shipped[..shipped.find("SHARED VOICE HARD RULES:").unwrap()],
        &positive[..rules_start]
    );
}

#[test]
fn the_user_prompt_shows_the_sentence_array_shape() {
    let ctx = context(42, 1);
    let (_, user) = build_scene_prompts(GMProfile::Fireside, NarratorLength::Rich, &ctx);
    assert_eq!(
        user,
        "Generate a scene response strictly as valid JSON matching SCENE_SCHEMA:\n{\"sentences\": [\"...\"]}\nThe sentences array holds complete sentences, one per item, at most 70 words in all, each following all the rules."
    );
    let (_, cited) = build_scene_prompts_for(
        GMProfile::Fireside,
        NarratorLength::Brief,
        &ctx,
        PromptRules::Shipped,
        FormatSchema::CitedFacts,
    );
    assert_eq!(
        cited,
        "Generate a scene response strictly as valid JSON matching SCENE_SCHEMA:\n{\"cited_fact_keys\": [\"...\"], \"sentences\": [\"...\"]}\nFirst list in cited_fact_keys the names of the turn facts the sentences rest on. Then the sentences array holds complete sentences, one per item, at most 25 words in all, each following all the rules."
    );
}

fn two_turn_set() -> (TurnSet, String) {
    let spec = |id: &str| TurnSpec {
        id: id.to_string(),
        seed: 42,
        event_id: "f1_001".to_string(),
        choice_id: "A".to_string(),
        weirdness_level: 0,
        uncanny_tokens: 2,
        deltas: Default::default(),
        effects: Default::default(),
        event_seed: "Fixture seed.".to_string(),
        choice_label: "Fixture label".to_string(),
        fallback_line: "Fixture fallback.".to_string(),
    };
    let set = TurnSet {
        schema: "etv-turn-set-1".to_string(),
        turns: vec![spec("a"), spec("b")],
    };
    let sha = sha256_hex(serde_json::to_string(&set).unwrap().as_bytes());
    (set, sha)
}

fn one_cell(model: &str) -> MeasureConfig {
    let mut cfg = MeasureConfig::new(model);
    cfg.profiles = vec![GMProfile::Chronicler];
    cfg.lengths = vec![NarratorLength::Rich];
    cfg
}

#[test]
fn the_harness_can_run_either_rules_wording_and_either_schema_and_records_which() {
    let (set, sha) = two_turn_set();

    // Defaults: the shipped wording and the plain schema.
    let rec = Recorder::new(Reply::Sentences);
    let (report, _) = run_measurement(&set, &sha, &one_cell("fixture-model"), rec.clone()).unwrap();
    assert_eq!(report.prompt_rules, PromptRules::Shipped);
    assert_eq!(report.format_schema, FormatSchema::Plain);
    let generation: Vec<Value> = rec.bodies().into_iter().skip(1).collect();
    assert_eq!(generation.len(), 4);
    for body in &generation {
        assert!(body["system"]
            .as_str()
            .unwrap()
            .contains("SHARED VOICE HARD RULES:"));
        assert_eq!(
            body["format"],
            narration_format_schema(NarratorLength::Rich)
        );
        assert_eq!(
            body["options"]["num_predict"],
            json!(NARRATOR_NUM_PREDICT_RICH)
        );
    }

    // The comparison run.
    let mut cfg = one_cell("fixture-model");
    cfg.prompt_rules = PromptRules::Positive;
    cfg.format_schema = FormatSchema::CitedFacts;
    let rec = Recorder::new(Reply::Sentences);
    let (report, _) = run_measurement(&set, &sha, &cfg, rec.clone()).unwrap();
    assert_eq!(report.prompt_rules, PromptRules::Positive);
    assert_eq!(report.format_schema, FormatSchema::CitedFacts);
    let text = serde_json::to_string(&report).unwrap();
    assert!(text.contains(r#""prompt_rules":"positive""#));
    assert!(text.contains(r#""format_schema":"cited""#));
    for body in rec.bodies().into_iter().skip(1) {
        let system = body["system"].as_str().unwrap();
        assert!(system.contains("SHARED VOICE RULES:"));
        assert!(!system.contains("SHARED VOICE HARD RULES:"));
        assert_eq!(
            body["format"],
            narration_format_schema_cited(NarratorLength::Rich)
        );
        assert_eq!(
            body["format"]["required"],
            json!(["cited_fact_keys", "sentences"])
        );
        assert_eq!(
            body["options"]["num_predict"],
            json!(NARRATOR_NUM_PREDICT_RICH + NARRATOR_NUM_PREDICT_CITED_EXTRA)
        );
        assert!(body["prompt"].as_str().unwrap().contains("cited_fact_keys"));
    }
}

#[test]
fn harness_flags_for_the_two_variants() {
    let args = |extra: &[&str]| {
        let mut v: Vec<String> = [
            "--turn-set",
            "does-not-matter.json",
            "--turn-set-sha256",
            "00",
            "--model",
            "fixture-model",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    };
    let mut out = Vec::new();
    let err = run_cli(
        &args(&["--prompt-rules", "loud"]),
        Recorder::new(Reply::Sentences),
        &mut out,
    )
    .unwrap_err();
    assert_eq!(
        err,
        "--prompt-rules must be shipped or positive, not \"loud\""
    );
    let err = run_cli(
        &args(&["--format-schema", "xml"]),
        Recorder::new(Reply::Sentences),
        &mut out,
    )
    .unwrap_err();
    assert_eq!(
        err,
        "--format-schema must be plain, cited or text, not \"xml\""
    );
}

#[test]
fn two_timeouts_in_a_row_pause_the_narrator_for_the_session() {
    assert_eq!(TIMEOUT_BREAKER_STRIKES, 2);
    let ctx = context(42, 1);

    // Every call times out: the first turn's two calls trip the breaker, and no later turn calls.
    let rec = Recorder::new(Reply::Timeout);
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec.clone());
    assert!(!narrator.is_paused());
    for _ in 0..5 {
        let told = narrator.narrate_turn(&ctx);
        assert_eq!((told.scene, told.outcome), (None, None));
    }
    assert!(narrator.is_paused());
    assert_eq!(rec.generate_calls.load(Ordering::SeqCst), 2);
    let stats = narrator.stats();
    assert_eq!(stats.calls, 2);
    assert_eq!(stats.timeouts, 2);
    // Five turns, two slots each, all fell back.
    assert_eq!(stats.fallbacks, 10);

    // One timeout followed by an answer does not trip it.
    let rec = Recorder::new(Reply::TimeoutThenOk(1));
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec.clone());
    for _ in 0..3 {
        narrator.narrate_turn(&ctx);
    }
    assert!(!narrator.is_paused());
    assert_eq!(rec.generate_calls.load(Ordering::SeqCst), 6);
    assert_eq!(narrator.stats().timeouts, 1);

    // With the breaker off (as the harness runs), every call is still made.
    let rec = Recorder::new(Reply::Timeout);
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec.clone());
    narrator.set_timeout_breaker(false);
    for _ in 0..5 {
        narrator.narrate_turn(&ctx);
    }
    assert!(!narrator.is_paused());
    assert_eq!(rec.generate_calls.load(Ordering::SeqCst), 10);
}

fn checker_with_names(names: &[&str]) -> CheckerContext {
    CheckerContext {
        party_names: names.iter().map(|s| s.to_string()).collect(),
        length: NarratorLength::Brief,
        ..CheckerContext::default()
    }
}

/// A party member's name can only cause a rejection when the line uses that word with a capital
/// letter. The same word in lower case passes, so a name that is also an ordinary word does not
/// block ordinary prose.
#[test]
fn party_names_that_are_ordinary_words_do_not_block_ordinary_prose() {
    let ctx = checker_with_names(&["Will", "May", "Mercy", "Patience"]);
    for line in [
        "The party will rest by the water.",
        "The rain may ease by dusk.",
        "The trail shows no mercy.",
        "The crossing takes patience.",
    ] {
        assert_eq!(check_generated_line(line, &ctx), Ok(()), "{line}");
    }
    // The name itself, capitalised, is still caught: at the start of a sentence by the name
    // rule, and inside one by the capital-letter rule, which runs first.
    let failure = check_generated_line("Mercy waits by the wagon.", &ctx).unwrap_err();
    assert_eq!(failure.code, CheckReasonCode::NamedPartyMember);
    let failure = check_generated_line("The party waits on Mercy.", &ctx).unwrap_err();
    assert_eq!(failure.code, CheckReasonCode::MidSentenceCapital);
    // Without those names in the party, the same sentence-initial words pass.
    let nobody = checker_with_names(&["Ruth"]);
    assert_eq!(
        check_generated_line("Patience wears thin on the trail.", &nobody),
        Ok(())
    );
    // Known cost, pinned on purpose: when a member is called Patience, a sentence that opens
    // with the ordinary word is rejected too, and that turn shows the fallback line.
    let failure = check_generated_line("Patience wears thin on the trail.", &ctx).unwrap_err();
    assert_eq!(failure.code, CheckReasonCode::NamedPartyMember);
}

/// A name cannot collide with the pronoun rule or the band vocabulary in a way that changes the
/// verdict: those rules give the same answer whether or not the word is also a name.
#[test]
fn a_name_never_changes_the_pronoun_or_vocabulary_verdict() {
    for line in [
        "The party rests while you wait.",
        "The party rests by the water.",
    ] {
        let with = check_generated_line(line, &checker_with_names(&["You", "Ghost"]));
        let without = check_generated_line(line, &checker_with_names(&[]));
        assert_eq!(
            with.clone().map_err(|f| f.code),
            without.map_err(|f| f.code),
            "{line}"
        );
    }
    let failure = check_generated_line(
        "The party rests while you wait.",
        &checker_with_names(&["You"]),
    )
    .unwrap_err();
    assert_eq!(failure.code, CheckReasonCode::PersonalPronoun);
}

/// The names the game itself gives party members: the two that are also ordinary English words
/// are listed here, so a new name of that kind is a decision, not an accident.
#[test]
fn the_name_pool_has_two_names_that_are_ordinary_words() {
    assert_eq!(FIRST_NAMES.len(), 18);
    let ordinary: Vec<&str> = FIRST_NAMES
        .iter()
        .copied()
        .filter(|n| {
            [
                "Mercy", "Patience", "Will", "May", "Grace", "Hope", "Faith", "Joy",
            ]
            .contains(n)
        })
        .collect();
    assert_eq!(ordinary, vec!["Mercy", "Patience"]);
}

/// The third harness arm: no schema at all. The request carries no `format` field, the prompt
/// asks for plain sentences, and the answer is checked as it stands.
#[test]
fn the_plain_text_arm_sends_no_format_and_checks_the_answer_as_it_stands() {
    let ctx = context(42, 1);

    let rec = Recorder::new(Reply::Text("The fixture line holds steady."));
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec.clone());
    narrator.set_format_schema(FormatSchema::PlainText);
    assert_eq!(narrator.format_schema(), FormatSchema::PlainText);
    let told = narrator.narrate_turn(&ctx);
    let line = Some("The fixture line holds steady.");
    assert_eq!(told.scene.as_deref(), line);
    assert_eq!(told.outcome.as_deref(), line);
    let bodies = rec.bodies();
    assert_eq!(bodies.len(), 2);
    for body in &bodies {
        assert!(body.get("format").is_none());
        let keys: Vec<&str> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["model", "options", "prompt", "stream", "system", "think"]
        );
        // Everything else is as in the other arms.
        assert_eq!(body["think"], json!(false));
        assert_eq!(body["options"]["temperature"], json!(0.15));
        assert_eq!(body["options"]["num_ctx"], json!(2048));
        assert_eq!(body["options"]["num_predict"], json!(110));
        assert!(body["options"]["seed"].is_u64());
    }
    assert_eq!(
        bodies[0]["prompt"],
        json!("Generate a scene response as plain text: complete sentences, at most 25 words in all, each following all the rules. Answer with the sentences only.")
    );
    assert_eq!(
        bodies[1]["prompt"],
        json!("Generate an outcome response as plain text: complete sentences, at most 25 words in all, each following all the rules. Answer with the sentences only.")
    );
    // The system prompt is the same as in the schema arm.
    let (shipped_sys, _) = build_scene_prompts(GMProfile::Chronicler, NarratorLength::Brief, &ctx);
    assert_eq!(bodies[0]["system"], json!(shipped_sys));

    // A plain-text answer that breaks a hard rule is rejected like any other, and falls back.
    let rec = Recorder::new(Reply::Text("The fixture line costs 3 days."));
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec);
    narrator.set_format_schema(FormatSchema::PlainText);
    let told = narrator.narrate_turn(&ctx);
    assert_eq!((told.scene, told.outcome), (None, None));
    assert_eq!(narrator.stats().rule_rejects, 2);

    // An answer wrapped in JSON is not unwrapped in this arm: it is checked as text, and the
    // braces and quotes fail the punctuation rule.
    let rec = Recorder::new(Reply::Text(
        r#"{"sentences": ["The fixture line holds steady."]}"#,
    ));
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec);
    narrator.set_format_schema(FormatSchema::PlainText);
    let told = narrator.narrate_turn(&ctx);
    assert_eq!((told.scene, told.outcome), (None, None));
    assert_eq!(narrator.stats().rule_rejects, 2);
    assert_eq!(narrator.stats().json_errors, 0);

    // An empty answer is a bad answer, not a rule reject.
    let rec = Recorder::new(Reply::Text("   "));
    let narrator = LiveNarrator::with_transport(settings(NarratorLength::Brief), rec);
    narrator.set_format_schema(FormatSchema::PlainText);
    narrator.narrate_turn(&ctx);
    assert_eq!(narrator.stats().json_errors, 2);
}

#[test]
fn the_harness_runs_and_records_the_plain_text_arm() {
    let (set, sha) = two_turn_set();
    let mut cfg = one_cell("fixture-model");
    cfg.lengths = vec![NarratorLength::Brief];
    cfg.format_schema = FormatSchema::PlainText;
    let rec = Recorder::new(Reply::Text("The fixture line holds steady."));
    let (report, _) = run_measurement(&set, &sha, &cfg, rec.clone()).unwrap();
    assert_eq!(report.format_schema, FormatSchema::PlainText);
    assert!(serde_json::to_string(&report)
        .unwrap()
        .contains(r#""format_schema":"text""#));
    let generation: Vec<Value> = rec.bodies().into_iter().skip(1).collect();
    assert_eq!(generation.len(), 4);
    assert!(generation.iter().all(|b| b.get("format").is_none()));
    assert_eq!(report.cells[0].accepted_slots, 4);
    assert_eq!(report.cells[0].fallback_slots, 0);
}

/// The length setting is the only authority on sentence count: no built prompt states a second
/// limit, for any profile, length, rules wording or answer shape.
#[test]
fn no_prompt_states_a_second_sentence_limit() {
    use etv_core::narrator::{build_outcome_prompts_for, profile_header};
    let ctx = context(42, 1);
    for profile in [
        GMProfile::Chronicler,
        GMProfile::Fireside,
        GMProfile::Lantern,
    ] {
        // The profile text itself states no sentence count.
        let header = profile_header(profile);
        for n in 1..=9 {
            assert!(!header.contains(&format!("{n} sentence")), "{header}");
        }
        assert!(!header.contains("Maximum"));
        assert!(!header.to_ascii_lowercase().contains("sentences for"));

        for (length, cap) in [
            (NarratorLength::Brief, 25),
            (NarratorLength::Standard, 45),
            (NarratorLength::Rich, 70),
        ] {
            assert_eq!(length.word_cap(), cap);
            for rules in [PromptRules::Shipped, PromptRules::Positive] {
                for schema in [
                    FormatSchema::Plain,
                    FormatSchema::CitedFacts,
                    FormatSchema::PlainText,
                ] {
                    for (sys, user) in [
                        build_scene_prompts_for(profile, length, &ctx, rules, schema),
                        build_outcome_prompts_for(profile, length, &ctx, rules, schema),
                    ] {
                        for text in [&sys, &user] {
                            assert!(!text.contains("Maximum"), "{text}");
                            // No prompt states a number of sentences any more.
                            for (at, _) in text.match_indices("sentences") {
                                let lead = text[..at].trim_end_matches("complete ");
                                let after_number = lead.ends_with(' ')
                                    && lead
                                        .trim_end()
                                        .chars()
                                        .last()
                                        .is_some_and(|c| c.is_ascii_digit());
                                assert!(!after_number, "a sentence limit in: {text}");
                            }
                            // Wherever a word cap is stated, it is this length's.
                            for (at, _) in text.match_indices(" words in all") {
                                assert!(
                                    text[..at].ends_with(&format!("at most {cap}")),
                                    "another word cap in: {text}"
                                );
                            }
                        }
                        // The limit is stated once in the rules, as the length cap.
                        assert_eq!(
                            sys.matches(&format!("(at most {cap} words in all)"))
                                .count(),
                            1
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn two_turns_with_the_same_facts_get_different_seeds_and_a_replay_the_same() {
    // The same run, day and facts at two places in the run: only the turn index differs.
    let ctx = context(42, 3);
    let first = ctx.clone().with_turn_index(7);
    let again = ctx.clone().with_turn_index(8);
    let seed = |c: &NarrationContext, scene: bool| {
        narration_call_seed(c, GMProfile::Fireside, NarratorLength::Brief, scene)
    };
    assert_ne!(seed(&first, true), seed(&again, true));
    assert_ne!(seed(&first, false), seed(&again, false));
    // A replay of the same turn (the same index) asks for the same samples, byte for byte.
    let replay = ctx.with_turn_index(7);
    assert_eq!(seed(&first, true), seed(&replay, true));
    assert_eq!(seed(&first, false), seed(&replay, false));
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&replay).unwrap()
    );
}

#[test]
fn the_turn_index_is_the_runs_step_counter_after_the_turn() {
    // A fresh run has stepped no turns; the context built from it carries that count. Every
    // step advancing it is proved in `narrator_turn_counter.rs`.
    let ctx = context(42, 1);
    let mut state = create_new_run_with_content(42, embedded_content_pack());
    state.day = 1;
    assert_eq!(state.turns_stepped, 0);
    assert_eq!(ctx.turn_index(), state.turns_stepped);
}
