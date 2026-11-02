//! Phase 1 integration tests for Escape the Valley 2.0 (`etv-core`):
//! 1. Content loader (embedded vs `--features authoring` `--content-dir`, byte identity of `event_skeletons.json`)
//! 2. `content_hash` (hand-worked literal SHA-256 on small fixture + shipped pack, float rejection, version string, save & ledger headers)
//! 3. Content validator (shipped content passes with counted outcome warning; `--require-outcomes` turns warning into error; one negative fixture per rule)
//! 4. Save schema v4 & 1.3.x refusal (`LEGACY_SAVE_REFUSAL_MESSAGE` byte-for-byte, v0–v3 byte preservation + zero backups/sidecars/temp files, v4 `ContentVersionMismatch` naming both hashes)
//! 5. Tier 2 snapshot harness (text-only content edits across all 1,000 seeds change `presentation_record` while leaving every turn's `mechanics_state_hash` identical)

use etv_core::content::{
    canonicalize_json_value_no_floats, compute_content_hash_hex, embedded_content_hash_hex,
    embedded_content_pack, embedded_version_string, parse_cli_args, ContentPack, TrailLedgerHeader,
    CONTENT_MANIFEST_JSON, EMBEDDED_CONTENT_HASH_HEX, EVENT_SKELETONS_JSON, LOGIC_VERSION,
};
use etv_core::save::{
    load_game_checked, load_game_checked_with_content_hash, load_game_result, save_game,
    state_to_value, SaveLoadError, LEGACY_SAVE_REFUSAL_MESSAGE, SAVE_DIR, SAVE_FILE, SAVE_VERSION,
};
use etv_core::snapshot::{build_1000_seeds, run_tier2_seed_trace};
use etv_core::validator::{
    validate_content, validate_content_with_options, ValidationOptions, ERR_DUPLICATE_ID,
    ERR_EMPTY_GATE_HINT, ERR_MISSING_OUTCOME_SENTENCES, ERR_MISSING_SCHEMA_VERSION,
    ERR_NO_AFFORDABLE_FALLBACK_CHOICE, ERR_UNKNOWN_REFERENCE_ID, WARN_MISSING_OUTCOME_SENTENCES,
};
use etv_core::worldgen::create_new_run;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in &digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

// ── 1. Content Loader & Byte Identity ────────────────────────────────────────

#[test]
fn test_item1_rust_owned_content_dir_byte_identical_to_python_data() {
    let rust_events_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("content/event_skeletons.json");
    let legacy_events_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/content-legacy/event_skeletons.json");
    let legacy_manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oracle/content-legacy/content_manifest.json");
    let py_events_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/escape_the_valley/data/event_skeletons.json");
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("content/content_manifest.json");

    let legacy_bytes = fs::read(&legacy_events_path).expect("read legacy event_skeletons.json");
    let py_bytes = fs::read(&py_events_path).expect("read python event_skeletons.json");
    assert_eq!(
        legacy_bytes, py_bytes,
        "rust/oracle/content-legacy/event_skeletons.json must be byte-for-byte identical to Python event_skeletons.json"
    );
    assert_eq!(
        sha256_hex(&legacy_bytes),
        "3402c6e1a1508fd55ecc60e2bfe819f60c219638f10d10e2881ca2d237a44576"
    );
    let legacy_manifest_bytes =
        fs::read(&legacy_manifest_path).expect("read legacy content_manifest.json");
    assert_eq!(
        sha256_hex(&legacy_manifest_bytes),
        "f84f98ea845bbfc9a0e0198ba9a29011104a2ed410d40867bb38a82dab342b9f"
    );

    let rust_bytes = fs::read(&rust_events_path).expect("read rust event_skeletons.json");
    assert_eq!(rust_bytes, EVENT_SKELETONS_JSON.as_bytes());

    let manifest_bytes = fs::read(&manifest_path).expect("read content_manifest.json");
    assert_eq!(manifest_bytes, CONTENT_MANIFEST_JSON.as_bytes());
    let manifest_val: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest_val["schema_version"], json!(1));
    assert_eq!(manifest_val["files"], json!(["event_skeletons.json"]));
}

#[cfg(not(feature = "authoring"))]
#[test]
fn test_item1_release_build_rejects_content_dir_flag() {
    let args = vec![
        "validate-content".to_string(),
        "--content-dir".to_string(),
        "some/dir".to_string(),
    ];
    let err = parse_cli_args(&args).expect_err("--content-dir must be rejected without authoring");
    assert!(
        err.contains("--content-dir") && err.contains("authoring"),
        "unexpected error: {err}"
    );

    let args_eq = vec![
        "inspect-event".to_string(),
        "f1_001".to_string(),
        "--content-dir=some/dir".to_string(),
    ];
    let err_eq =
        parse_cli_args(&args_eq).expect_err("--content-dir= must be rejected without authoring");
    assert!(err_eq.contains("--content-dir"));
}

#[cfg(feature = "authoring")]
#[test]
fn test_item1_authoring_mode_loads_content_dir_under_1s_without_recompile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(
        tmp.path().join("content_manifest.json"),
        CONTENT_MANIFEST_JSON,
    )
    .unwrap();

    // Edit one narration sentence in event f1_017 (which fires on turn 1 of seed 42)
    let edited_sentence =
        "AUTHORING_OVERRIDE_SENTENCE: A pale lantern swings from the old milepost.";
    let mut events_val: Value = serde_json::from_str(EVENT_SKELETONS_JSON).unwrap();
    let arr = events_val.as_array_mut().unwrap();
    let target_ev = arr
        .iter_mut()
        .find(|e| e.get("id").and_then(Value::as_str) == Some("f1_017"))
        .expect("f1_017 in event_skeletons.json");
    target_ev["narration_seed"] = json!(edited_sentence);
    fs::write(
        tmp.path().join("event_skeletons.json"),
        serde_json::to_string_pretty(&events_val).unwrap(),
    )
    .unwrap();

    let args = vec![
        "inspect-event".to_string(),
        "f1_017".to_string(),
        "--content-dir".to_string(),
        tmp.path().to_string_lossy().to_string(),
    ];
    let parsed = parse_cli_args(&args).expect("--content-dir accepted with authoring feature");
    assert_eq!(parsed.content_dir.as_deref(), Some(tmp.path()));

    let start = std::time::Instant::now();
    let pack = ContentPack::load_from_dir(tmp.path()).expect("load_from_dir");
    let trace = run_tier2_seed_trace(42, 2, &pack, 5);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs_f64() < 1.0,
        "authoring load + run took {:?} (must be < 1.0 s)",
        elapsed
    );
    assert_ne!(pack.content_hash_hex, EMBEDDED_CONTENT_HASH_HEX);

    let f1_017 = pack
        .all_events
        .iter()
        .find(|e| e.event_id == "f1_017")
        .unwrap();
    assert_eq!(f1_017.fallback_narration, edited_sentence);
    assert!(
        trace
            .turns
            .iter()
            .any(|t| t.presentation_record.narration == edited_sentence),
        "edited narration must appear in seed 42 run without recompiling"
    );
}

// ── 2. Content Hash (Hand-Worked Literal & Shipped Pack) ─────────────────────

#[test]
fn test_item2_content_hash_hand_worked_literal_and_shipped_pack() {
    // Hand-worked small fixture:
    // LOGIC_VERSION = "etv-rust-2.0-logic-v1"
    // content_manifest.json canonical = "{\"files\":[\"event_skeletons.json\"],\"schema_version\":1}\n"
    // event_skeletons.json canonical = "[{\"id\":\"ev_1\",\"title\":\"Test\"}]\n"
    // Preimage:
    // b"etv-rust-2.0-logic-v1\x00content_manifest.json\x00{\"files\":[\"event_skeletons.json\"],\"schema_version\":1}\n\x00event_skeletons.json\x00[{\"id\":\"ev_1\",\"title\":\"Test\"}]\n\x00"
    let small_manifest = r#"{"schema_version": 1, "files": ["event_skeletons.json"]}"#;
    let small_events = r#"[{"id": "ev_1", "title": "Test"}]"#;
    let small_pack =
        ContentPack::from_raw_strings(small_manifest, small_events).expect("small fixture pack");
    assert_eq!(
        small_pack.content_hash_hex,
        "45a3faac602c1c0fdaa71a7469751ac44540d5a06ce474824a8a3654e6c68bfa"
    );

    // Key reordering and CRLF line endings must canonicalize to the exact same hash
    let reordered_manifest = "{\r\n  \"files\": [\r\n    \"event_skeletons.json\"\r\n  ],\r\n  \"schema_version\": 1\r\n}";
    let reordered_events =
        "[\r\n  {\r\n    \"title\": \"Test\",\r\n    \"id\": \"ev_1\"\r\n  }\r\n]";
    let reordered_pack =
        ContentPack::from_raw_strings(reordered_manifest, reordered_events).unwrap();
    assert_eq!(
        reordered_pack.content_hash_hex,
        "45a3faac602c1c0fdaa71a7469751ac44540d5a06ce474824a8a3654e6c68bfa"
    );

    // Floating-point numbers in content JSON files must be rejected
    let float_json: Value = serde_json::from_str(r#"{"weight": 1.5}"#).unwrap();
    let float_err = canonicalize_json_value_no_floats(&float_json)
        .expect_err("floats in content files must be rejected");
    assert!(float_err.contains("floating-point"));

    // Frozen legacy content snapshot pack hash (pinned to 7836724a... at LOGIC_VERSION)
    let legacy_manifest = include_str!("../../oracle/content-legacy/content_manifest.json");
    let legacy_events = include_str!("../../oracle/content-legacy/event_skeletons.json");
    let legacy_pack =
        ContentPack::from_raw_strings(legacy_manifest, legacy_events).expect("legacy content pack");
    assert_eq!(
        legacy_pack.content_hash_hex,
        "7836724a01c9db8e8e97b234a817b4f3be44a923677292af96f8426d6540fe74"
    );

    // Shipped live embedded content pack hash (matches EMBEDDED_CONTENT_HASH_HEX)
    let embedded = embedded_content_pack();
    assert_eq!(embedded.logic_version, LOGIC_VERSION);
    assert_eq!(embedded.content_hash_hex, EMBEDDED_CONTENT_HASH_HEX);
    assert_eq!(embedded_content_hash_hex(), EMBEDDED_CONTENT_HASH_HEX);
    assert_eq!(
        compute_content_hash_hex(embedded),
        EMBEDDED_CONTENT_HASH_HEX
    );

    let ver = embedded_version_string();
    assert!(ver.contains("logic_version=etv-rust-2.0-logic-v1"));
    assert!(ver.contains(&format!("content_hash={EMBEDDED_CONTENT_HASH_HEX}")));

    let state = create_new_run(42);
    assert_eq!(state.content_hash, EMBEDDED_CONTENT_HASH_HEX);
    let ledger_header = TrailLedgerHeader::from_state(&state);
    assert_eq!(ledger_header.content_hash, EMBEDDED_CONTENT_HASH_HEX);
    assert!(ledger_header
        .verify_content_hash(EMBEDDED_CONTENT_HASH_HEX)
        .is_ok());
    assert_eq!(
        ledger_header.verify_content_hash("0000"),
        Err(SaveLoadError::ContentVersionMismatch {
            save_content_hash: EMBEDDED_CONTENT_HASH_HEX.to_string(),
            running_content_hash: "0000".to_string(),
        })
    );
}

// ── 3. Content Validator (Shipped Pack + 1 Fixture per Rule + --require-outcomes) ──

#[test]
fn test_item3_validator_shipped_content_and_require_outcomes_flag() {
    let embedded = embedded_content_pack();

    // Default: passes with 0 errors and 1 counted warning (627 choices missing outcome_sentence: 464 JSON + 163 built-in)
    let report_default = validate_content(embedded);
    assert!(
        report_default.is_ok(),
        "shipped content must pass default validation: {:?}",
        report_default.errors
    );
    assert_eq!(report_default.schema_version, Some(1));
    assert_eq!(report_default.event_count, 260);
    assert_eq!(report_default.json_event_count, 200);
    assert_eq!(report_default.missing_json_outcome_choices, 464);
    assert_eq!(report_default.missing_builtin_outcome_choices, 163);
    assert_eq!(report_default.missing_outcome_choices, 627);
    assert_eq!(report_default.warnings.len(), 1);
    assert_eq!(
        report_default.warnings[0].code,
        WARN_MISSING_OUTCOME_SENTENCES
    );
    assert!(report_default.warnings[0]
        .message
        .contains("627 choices have no outcome sentence"));

    // With --require-outcomes: the missing outcome sentences become an error
    let report_strict = validate_content_with_options(
        embedded,
        ValidationOptions {
            require_outcomes: true,
        },
    );
    assert!(!report_strict.is_ok());
    assert!(report_strict.warnings.is_empty());
    assert_eq!(report_strict.errors.len(), 1);
    assert_eq!(report_strict.errors[0].code, ERR_MISSING_OUTCOME_SENTENCES);
    assert!(report_strict.errors[0]
        .message
        .contains("627 choices have no outcome sentence"));
    assert!(!report_strict.errors[0].hint.is_empty());

    // When all choices have non-empty outcome_sentence, --require-outcomes passes with 0 errors and 0 warnings
    let mut with_outcomes_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[{"id": "ev_with_outcomes", "title": "Complete", "choices": [{"label": "Wait", "intent_action": "WAIT", "outcome_sentence": "You wait out the squall.", "engine_effect_profile": {"time_days": 1}}]}]"#,
    )
    .unwrap();
    for ev in &mut with_outcomes_pack.all_events {
        for ch in &mut ev.fallback_choices {
            if ch.outcome_sentence.is_none() {
                ch.outcome_sentence = Some("You press on down the valley trail.".to_string());
            }
        }
    }
    let report_complete = validate_content_with_options(
        &with_outcomes_pack,
        ValidationOptions {
            require_outcomes: true,
        },
    );
    assert!(report_complete.is_ok());
    assert_eq!(report_complete.missing_outcome_choices, 0);
    assert!(report_complete.warnings.is_empty());
    assert!(report_complete.errors.is_empty());
}

#[test]
fn test_item3_validator_rejects_each_defect_rule() {
    // Rule 1 fixture: missing schema_version in content_manifest.json
    let bad_manifest_pack = ContentPack::from_raw_strings(
        r#"{"files": ["event_skeletons.json"]}"#,
        r#"[{"id": "ev_ok", "title": "OK", "choices": [{"label": "Wait", "intent_action": "WAIT", "engine_effect_profile": {"time_days": 1}}]}]"#,
    )
    .unwrap();
    let rep1 = validate_content(&bad_manifest_pack);
    assert!(!rep1.is_ok());
    assert!(rep1
        .errors
        .iter()
        .any(|e| e.code == ERR_MISSING_SCHEMA_VERSION && !e.hint.is_empty()));

    // Rule 2 fixture: duplicate event_id and duplicate choice_id
    let dup_id_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[
            {
                "id": "dup_ev",
                "title": "First",
                "choices": [
                    {"choice_id": "A", "label": "One", "intent_action": "WAIT", "engine_effect_profile": {"time_days": 1}},
                    {"choice_id": "A", "label": "Two", "intent_action": "REST", "engine_effect_profile": {"time_days": 1}}
                ]
            },
            {
                "id": "dup_ev",
                "title": "Second",
                "choices": [
                    {"label": "Wait", "intent_action": "WAIT", "engine_effect_profile": {"time_days": 1}}
                ]
            }
        ]"#,
    )
    .unwrap();
    let rep2 = validate_content(&dup_id_pack);
    assert!(!rep2.is_ok());
    let dup_errors: Vec<_> = rep2
        .errors
        .iter()
        .filter(|e| e.code == ERR_DUPLICATE_ID)
        .collect();
    assert!(
        dup_errors.len() >= 2,
        "must report both duplicate event_id and duplicate choice_id: {dup_errors:?}"
    );

    // Rule 3 fixture: gated choice with empty cost_hint or risk_hint
    let empty_gate_hint_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[
            {
                "id": "ev_gated_missing_hint",
                "title": "Gated Event",
                "choices": [
                    {
                        "label": "Use spare parts",
                        "intent_action": "REPAIR",
                        "cost_hint": "1 part",
                        "risk_hint": "",
                        "engine_effect_profile": {"parts": -1, "wagon_condition": 10}
                    },
                    {
                        "label": "Wait",
                        "intent_action": "WAIT",
                        "engine_effect_profile": {"time_days": 1}
                    }
                ]
            }
        ]"#,
    )
    .unwrap();
    let rep3 = validate_content(&empty_gate_hint_pack);
    assert!(!rep3.is_ok());
    assert!(rep3
        .errors
        .iter()
        .any(|e| e.code == ERR_EMPTY_GATE_HINT && e.message.contains("ev_gated_missing_hint")));

    // Rule 4 fixture: event with no affordable fallback choice (every choice deducts supplies)
    let no_fallback_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[
            {
                "id": "ev_no_affordable",
                "title": "All Costly",
                "choices": [
                    {
                        "label": "Spend food",
                        "intent_action": "WAIT",
                        "engine_effect_profile": {"food": -3, "time_days": 1}
                    },
                    {
                        "label": "Spend parts",
                        "intent_action": "REPAIR",
                        "engine_effect_profile": {"parts": -1, "wagon_condition": 5}
                    }
                ]
            }
        ]"#,
    )
    .unwrap();
    let rep4 = validate_content(&no_fallback_pack);
    assert!(!rep4.is_ok());
    assert!(rep4.errors.iter().any(|e| {
        e.code == ERR_NO_AFFORDABLE_FALLBACK_CHOICE && e.message.contains("ev_no_affordable")
    }));

    // Rule 5 fixture: unknown trait, unknown place, and unknown event_id referenced
    let bad_refs_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[
            {
                "id": "ev_bad_refs",
                "title": "Bad References",
                "preconditions": ["trait:nonexistent_wizard", "place:Atlantis", "event:missing_ev_999"],
                "choices": [
                    {
                        "label": "Wait",
                        "intent_action": "WAIT",
                        "required_trait": "ghost_whisperer",
                        "cost_hint": "None",
                        "risk_hint": "Safe",
                        "engine_effect_profile": {
                            "time_days": 1,
                            "follow_up_event_id": "no_such_followup"
                        }
                    }
                ]
            }
        ]"#,
    )
    .unwrap();
    let rep5 = validate_content(&bad_refs_pack);
    assert!(!rep5.is_ok());
    let ref_messages: Vec<&str> = rep5
        .errors
        .iter()
        .filter(|e| e.code == ERR_UNKNOWN_REFERENCE_ID)
        .map(|e| e.message.as_str())
        .collect();
    assert_eq!(
        ref_messages,
        vec![
            "event 'ev_bad_refs': unknown trait 'nonexistent_wizard' referenced in precondition 'trait:nonexistent_wizard'",
            "event 'ev_bad_refs': unknown place 'Atlantis' referenced in precondition 'place:Atlantis'",
            "event 'ev_bad_refs': unknown event_id 'missing_ev_999' referenced in precondition 'event:missing_ev_999'",
            "event 'ev_bad_refs' choice 'A' references unknown trait 'ghost_whisperer'",
            "event 'ev_bad_refs' choice 'A' outcome references unknown event_id 'no_such_followup'",
        ]
    );
}

// ── 4. Save Schema v4, 1.3.x Refusal & Byte Preservation ─────────────────────

#[test]
fn test_item4_legacy_refusal_message_exact_bytes_and_file_unchanged() {
    let expected_message = "This save comes from the earlier version of Escape the Valley (1.3). This version can't open it, but your trail isn't lost: the earlier game still runs and will carry on from where you left off. To open it, install the earlier version with `pip install \"escape-the-valley<2\"`, then run `trail play` (or `trail tui --continue` for the full-screen game). Your save file has not been changed.";
    assert_eq!(
        LEGACY_SAVE_REFUSAL_MESSAGE.as_bytes(),
        expected_message.as_bytes(),
        "LEGACY_SAVE_REFUSAL_MESSAGE must match the card amendment byte-for-byte"
    );

    // Test refusing v0 (omitted save_version and explicit 0), v1, v2, and v3 saves on disk:
    // verify the file's bytes are identical before and after (sha256) and zero backup, sidecar, or temp files are written.
    let base_state = create_new_run(42);
    let base_v4_val = state_to_value(&base_state);

    let legacy_cases: Vec<(&str, Value, i64)> = vec![
        (
            "v0_omitted",
            {
                let mut v = base_v4_val.clone();
                let obj = v.as_object_mut().unwrap();
                obj.remove("save_version");
                obj.remove("content_hash");
                v
            },
            0,
        ),
        (
            "v0_explicit",
            {
                let mut v = base_v4_val.clone();
                let obj = v.as_object_mut().unwrap();
                obj.insert("save_version".to_string(), json!(0));
                obj.remove("content_hash");
                v
            },
            0,
        ),
        (
            "v1",
            {
                let mut v = base_v4_val.clone();
                let obj = v.as_object_mut().unwrap();
                obj.insert("save_version".to_string(), json!(1));
                obj.remove("content_hash");
                v
            },
            1,
        ),
        (
            "v2",
            {
                let mut v = base_v4_val.clone();
                let obj = v.as_object_mut().unwrap();
                obj.insert("save_version".to_string(), json!(2));
                obj.remove("content_hash");
                v
            },
            2,
        ),
        (
            "v3",
            {
                let mut v = base_v4_val.clone();
                let obj = v.as_object_mut().unwrap();
                obj.insert("save_version".to_string(), json!(3));
                obj.remove("content_hash");
                v
            },
            3,
        ),
    ];

    for (label, legacy_val, expected_ver) in legacy_cases {
        let tmp = tempfile::tempdir().expect("tempdir");
        let save_dir = tmp.path().join(SAVE_DIR);
        fs::create_dir_all(&save_dir).unwrap();
        let save_path = save_dir.join(SAVE_FILE);

        let payload_bytes = format!(
            "{}   \n\t  \n",
            serde_json::to_string_pretty(&legacy_val).unwrap()
        )
        .into_bytes();
        fs::write(&save_path, &payload_bytes).unwrap();
        let sha_before = sha256_hex(&fs::read(&save_path).unwrap());

        let res = load_game_result(tmp.path());
        assert!(!res.ok, "{label} save must be refused");
        assert_eq!(
            res.reason, expected_message,
            "wrong refusal message on {label}"
        );
        assert_eq!(
            res.error,
            Some(SaveLoadError::LegacySaveRefused {
                save_version: expected_ver
            })
        );

        let checked_err =
            load_game_checked(tmp.path()).expect_err("load_game_checked must refuse legacy save");
        assert_eq!(
            checked_err,
            SaveLoadError::LegacySaveRefused {
                save_version: expected_ver
            }
        );
        assert_eq!(checked_err.to_string(), expected_message);

        // Verify run.json bytes are 100% identical before and after
        let bytes_after = fs::read(&save_path).expect("run.json must still exist");
        let sha_after = sha256_hex(&bytes_after);
        assert_eq!(
            sha_before, sha_after,
            "refusing {label} save must leave run.json bytes identical"
        );
        assert_eq!(bytes_after, payload_bytes);

        // Verify no backup, sidecar, or temp file was written in .trail/ or parent dir
        let trail_entries: Vec<String> = fs::read_dir(&save_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            trail_entries,
            vec![SAVE_FILE.to_string()],
            "no backup, sidecar, or temp file may be written next to run.json on {label}"
        );
        let root_entries: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(root_entries, vec![SAVE_DIR.to_string()]);
    }
}

#[test]
fn test_item4_v4_save_roundtrip_and_content_version_mismatch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = create_new_run(42);
    let save_path = save_game(&state, tmp.path()).expect("save v4 game");

    let raw_saved: Value = serde_json::from_str(&fs::read_to_string(&save_path).unwrap()).unwrap();
    assert_eq!(raw_saved["save_version"], json!(SAVE_VERSION));
    assert_eq!(raw_saved["save_version"], json!(4));
    assert_eq!(raw_saved["content_hash"], json!(EMBEDDED_CONTENT_HASH_HEX));

    let loaded = load_game_checked(tmp.path()).expect("load matching v4 save");
    assert_eq!(loaded, state);
    assert_eq!(loaded.content_hash, EMBEDDED_CONTENT_HASH_HEX);

    // Loading a v4 save under a different running content_hash returns ContentVersionMismatch naming both hashes
    let other_hash = "45a3faac602c1c0fdaa71a7469751ac44540d5a06ce474824a8a3654e6c68bfa";
    let sha_before = sha256_hex(&fs::read(&save_path).unwrap());
    let mismatch_err = load_game_checked_with_content_hash(tmp.path(), other_hash)
        .expect_err("mismatched content_hash must fail with ContentVersionMismatch");
    assert_eq!(
        mismatch_err,
        SaveLoadError::ContentVersionMismatch {
            save_content_hash: EMBEDDED_CONTENT_HASH_HEX.to_string(),
            running_content_hash: other_hash.to_string(),
        }
    );
    let msg = mismatch_err.to_string();
    assert!(msg.contains(EMBEDDED_CONTENT_HASH_HEX));
    assert!(msg.contains(other_hash));

    let sha_after = sha256_hex(&fs::read(&save_path).unwrap());
    assert_eq!(
        sha_before, sha_after,
        "ContentVersionMismatch must not quarantine or modify run.json"
    );
}

// ── 5. Tier 2 Snapshot Harness (1,000 Seeds Text-Only Edit Invariance) ───────

#[test]
fn test_item5_tier2_snapshot_1000_seeds_text_edit_preserves_all_mechanics_hashes() {
    let embedded = embedded_content_pack();

    // Edit text fields (title, narration_seed, choice label, outcome_sentence) across multiple JSON events
    let mut edited_events_val: Value = serde_json::from_str(EVENT_SKELETONS_JSON).unwrap();
    for ev in edited_events_val.as_array_mut().unwrap().iter_mut() {
        let ev_id = ev
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(title) = ev.get("title").and_then(Value::as_str) {
            ev["title"] = json!(format!("{title} [Edited Title]"));
        }
        if let Some(narr) = ev.get("narration_seed").and_then(Value::as_str) {
            ev["narration_seed"] = json!(format!("{narr} The wind carries an unfamiliar echo."));
        }
        if let Some(choices) = ev.get_mut("choices").and_then(Value::as_array_mut) {
            for (idx, ch) in choices.iter_mut().enumerate() {
                if let Some(lbl) = ch.get("label").and_then(Value::as_str) {
                    ch["label"] = json!(format!("{lbl} (variant {idx})"));
                }
                ch["outcome_sentence"] =
                    json!(format!("Custom outcome prose for {ev_id} choice {idx}."));
            }
        }
    }
    let edited_events_json = serde_json::to_string_pretty(&edited_events_val).unwrap();
    let edited_pack =
        ContentPack::from_raw_strings(CONTENT_MANIFEST_JSON, &edited_events_json).unwrap();
    assert_ne!(edited_pack.content_hash_hex, embedded.content_hash_hex);

    let seeds = build_1000_seeds();
    assert_eq!(seeds.len(), 1000);

    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 16))
        .unwrap_or(4);
    let indexed_seeds: Vec<(usize, i128)> = seeds.into_iter().enumerate().collect();
    let chunk_size = indexed_seeds.len().div_ceil(worker_count);

    let results: Vec<(usize, usize)> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for chunk in indexed_seeds.chunks(chunk_size) {
            let emb_ref = embedded;
            let edt_ref = &edited_pack;
            handles.push(s.spawn(move || {
                let mut turns_checked = 0usize;
                let mut presentation_diffs = 0usize;
                for &(idx, seed) in chunk {
                    let base_trace = run_tier2_seed_trace(seed, idx, emb_ref, 60);
                    let edit_trace = run_tier2_seed_trace(seed, idx, edt_ref, 60);
                    assert_eq!(
                        base_trace.turns.len(),
                        edit_trace.turns.len(),
                        "turn count mismatch on seed {seed}"
                    );
                    for (bt, et) in base_trace.turns.iter().zip(edit_trace.turns.iter()) {
                        assert_eq!(bt.turn, et.turn);
                        assert_eq!(
                            bt.mechanics_state_hash, et.mechanics_state_hash,
                            "mechanics_state_hash diverged on seed {seed} turn {}",
                            bt.turn
                        );
                        if bt.presentation_record != et.presentation_record {
                            presentation_diffs += 1;
                        }
                        turns_checked += 1;
                    }
                }
                (turns_checked, presentation_diffs)
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let total_turns: usize = results.iter().map(|(t, _)| *t).sum();
    let total_presentation_diffs: usize = results.iter().map(|(_, p)| *p).sum();
    assert!(
        total_turns > 50_000,
        "expected > 50,000 turns across 1,000 seeds, got {total_turns}"
    );
    assert!(
        total_presentation_diffs > 10_000,
        "expected > 10,000 presentation record changes across 1,000 seeds, got {total_presentation_diffs}"
    );
}

#[test]
fn test_etv_cli_subcommands_and_validator_edge_cases() {
    use std::process::Command;

    let etv_bin = env!("CARGO_BIN_EXE_etv");

    // 1. etv --version
    let out_ver = Command::new(etv_bin)
        .arg("--version")
        .output()
        .expect("run etv --version");
    assert!(out_ver.status.success());
    let ver_stdout = String::from_utf8_lossy(&out_ver.stdout);
    assert!(ver_stdout.contains(EMBEDDED_CONTENT_HASH_HEX));
    assert!(ver_stdout.contains(LOGIC_VERSION));

    // 2. etv validate-content (default: exits 0 with warning)
    let out_val = Command::new(etv_bin)
        .arg("validate-content")
        .output()
        .expect("run etv validate-content");
    assert!(out_val.status.success());
    let val_stdout = String::from_utf8_lossy(&out_val.stdout);
    assert!(val_stdout.contains("WARNING [MISSING_OUTCOME_SENTENCES]"));
    assert!(val_stdout.contains("errors=0"));

    // 3. etv validate-content --require-outcomes (exits 1 with structured error)
    let out_strict = Command::new(etv_bin)
        .args(["validate-content", "--require-outcomes"])
        .output()
        .expect("run etv validate-content --require-outcomes");
    assert!(!out_strict.status.success());
    assert_eq!(out_strict.status.code(), Some(1));
    let strict_stderr = String::from_utf8_lossy(&out_strict.stderr);
    assert!(strict_stderr.contains("ERROR [MISSING_OUTCOME_SENTENCES]"));

    // 4. etv inspect-event f1_017 & unknown event
    let out_insp = Command::new(etv_bin)
        .args(["inspect-event", "f1_017"])
        .output()
        .expect("run etv inspect-event f1_017");
    assert!(out_insp.status.success());
    let insp_stdout = String::from_utf8_lossy(&out_insp.stdout);
    assert!(insp_stdout.contains("event_id: f1_017"));
    assert!(insp_stdout.contains(EMBEDDED_CONTENT_HASH_HEX));

    let out_insp_missing = Command::new(etv_bin)
        .args(["inspect-event", "no_such_event_xyz"])
        .output()
        .expect("run etv inspect-event missing");
    assert!(!out_insp_missing.status.success());

    // 5. etv run-seed 42 --turns 5
    let out_run = Command::new(etv_bin)
        .args(["run-seed", "42", "--turns", "5"])
        .output()
        .expect("run etv run-seed 42 --turns 5");
    assert!(out_run.status.success());
    let run_stdout = String::from_utf8_lossy(&out_run.stdout);
    assert!(run_stdout.contains("seed=42 turns=5"));

    // 6. CLI argument error paths
    assert!(parse_cli_args(&[]).is_err());
    assert!(parse_cli_args(&["unknown-cmd".to_string()]).is_err());
    assert!(parse_cli_args(&["inspect-event".to_string()]).is_err());
    assert!(parse_cli_args(&["run-seed".to_string()]).is_err());
    assert!(parse_cli_args(&["run-seed".to_string(), "not_an_int".to_string()]).is_err());
    assert!(parse_cli_args(&[
        "run-seed".to_string(),
        "42".to_string(),
        "--turns".to_string()
    ])
    .is_err());
    assert!(parse_cli_args(&[
        "run-seed".to_string(),
        "42".to_string(),
        "--turns".to_string(),
        "bad".to_string()
    ])
    .is_err());

    // 7. Additional validator branches (schema_version != 1, object wrapper, valid preconditions, place_id, trait/place in outcome)
    let wrong_ver_pack = ContentPack::from_raw_strings(
        r#"{"schema_version": 2, "files": ["event_skeletons.json"]}"#,
        r#"{"schema_version": 2, "events": [{"id": "ev_obj", "title": "Obj", "place_id": "UnknownTown", "preconditions": ["trait:healer", "place:Millford", "event:storm_sudden", "day>=3", "unknown_var>=1", "day>=not_int", "unparseable_cond"], "choices": [{"label": "Go", "intent_action": "WAIT", "engine_effect_profile": {"time_days": 1, "trait": "not_a_trait", "place": "NotAPlace"}}]}]}"#,
    )
    .unwrap();
    let rep_edge = validate_content(&wrong_ver_pack);
    assert!(!rep_edge.is_ok());
    assert!(rep_edge
        .errors
        .iter()
        .any(|e| e.code == ERR_MISSING_SCHEMA_VERSION));
    assert!(rep_edge
        .errors
        .iter()
        .any(|e| e.code == ERR_UNKNOWN_REFERENCE_ID && e.message.contains("UnknownTown")));
    assert!(rep_edge
        .errors
        .iter()
        .any(|e| e.code == ERR_UNKNOWN_REFERENCE_ID && e.message.contains("not_a_trait")));
    assert!(rep_edge
        .errors
        .iter()
        .any(|e| e.code == ERR_UNKNOWN_REFERENCE_ID && e.message.contains("NotAPlace")));
}

#[test]
fn test_fix1_mechanics_hash_covers_all_play_affecting_runstate_fields() {
    use etv_core::model::{GamePhase, MemoryCard, Trait, TransientEventIdCooldown, TwistModifier};
    use etv_core::snapshot::compute_mechanics_state_hash;
    use std::collections::BTreeMap;

    let base = create_new_run(42);
    let deltas = BTreeMap::new();
    let base_hash = compute_mechanics_state_hash(&base, GamePhase::Camp, "", "", &deltas);

    let mut s_weird = base.clone();
    s_weird.weirdness_level = 1;
    assert_ne!(
        compute_mechanics_state_hash(&s_weird, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "weirdness_level mutation must change mechanics_state_hash"
    );

    let mut s_tokens = base.clone();
    s_tokens.uncanny_tokens = 0;
    assert_ne!(
        compute_mechanics_state_hash(&s_tokens, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "uncanny_tokens mutation must change mechanics_state_hash"
    );

    let mut s_recent_ids = base.clone();
    s_recent_ids.recent_event_ids = TransientEventIdCooldown(Some(vec!["f1_017".to_string()]));
    assert_ne!(
        compute_mechanics_state_hash(&s_recent_ids, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "recent_event_ids mutation must change mechanics_state_hash"
    );

    let mut s_recent_tags = base.clone();
    s_recent_tags.recent_event_tags.push("folklore".to_string());
    assert_ne!(
        compute_mechanics_state_hash(&s_recent_tags, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "recent_event_tags mutation must change mechanics_state_hash"
    );

    let mut s_twists = base.clone();
    s_twists.twists = vec![
        TwistModifier::BanditYear,
        TwistModifier::SickSeason,
        TwistModifier::EarlyWinter,
    ];
    assert_ne!(
        compute_mechanics_state_hash(&s_twists, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "twists mutation must change mechanics_state_hash"
    );

    let mut s_cards = base.clone();
    s_cards.memory_cards.push(MemoryCard {
        id: "eng_event_f1_017_d1".to_string(),
        kind: "omen".to_string(),
        title: "Lantern Signals".to_string(),
        text: "Strange lanterns blink on the ridge.".to_string(),
        tags: vec!["folklore".to_string()],
        day_created: 1,
        day_last_seen: 1,
        entities: Vec::new(),
        salience: 0.7,
        cooldown_until: 0,
        source: "engine".to_string(),
    });
    let card_hash = compute_mechanics_state_hash(&s_cards, GamePhase::Camp, "", "", &deltas);
    assert_ne!(
        card_hash, base_hash,
        "memory_cards addition must change mechanics_state_hash"
    );

    // Text-only edits on a MemoryCard (title/text) must NOT change mechanics_state_hash
    let mut s_cards_text = s_cards.clone();
    s_cards_text.memory_cards[0].title = "Edited Card Title".to_string();
    s_cards_text.memory_cards[0].text = "Edited card prose.".to_string();
    assert_eq!(
        compute_mechanics_state_hash(&s_cards_text, GamePhase::Camp, "", "", &deltas),
        card_hash,
        "MemoryCard title/text prose edits must not change mechanics_state_hash"
    );

    let mut s_crises = base.clone();
    s_crises.resource_crises_seen.push("food".to_string());
    assert_ne!(
        compute_mechanics_state_hash(&s_crises, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "resource_crises_seen mutation must change mechanics_state_hash"
    );

    let mut s_doctrine = base.clone();
    s_doctrine.doctrine = "careful_hands_override".to_string();
    assert_ne!(
        compute_mechanics_state_hash(&s_doctrine, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "doctrine mutation must change mechanics_state_hash"
    );

    let mut s_taboo = base.clone();
    s_taboo.taboo = "never_night_override".to_string();
    assert_ne!(
        compute_mechanics_state_hash(&s_taboo, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "taboo mutation must change mechanics_state_hash"
    );

    let mut s_last_action = base.clone();
    s_last_action.last_action = "REPAIR".to_string();
    assert_ne!(
        compute_mechanics_state_hash(&s_last_action, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "last_action mutation must change mechanics_state_hash"
    );

    let mut s_traits = base.clone();
    s_traits.party.members[0].traits = vec![
        Trait::Tough,
        Trait::Healer,
        Trait::Mechanic,
        Trait::SharpEye,
    ];
    assert_ne!(
        compute_mechanics_state_hash(&s_traits, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "member traits mutation must change mechanics_state_hash"
    );

    // Non-mechanical RunState fields (run_id salt, callout_level, content_hash) must NOT change mechanics_state_hash
    let mut s_non_mech = base.clone();
    s_non_mech.run_id = "00000000-ffffffff".to_string();
    s_non_mech.callout_level = "minimal".to_string();
    s_non_mech.content_hash = "0123456789abcdef".to_string();
    assert_eq!(
        compute_mechanics_state_hash(&s_non_mech, GamePhase::Camp, "", "", &deltas),
        base_hash,
        "run_id, callout_level, and content_hash must not change mechanics_state_hash"
    );
}

#[test]
fn test_fix1_negative_control_mechanics_edit_changes_mechanics_hash() {
    let embedded = embedded_content_pack();
    let base_trace = run_tier2_seed_trace(42, 2, embedded, 5);

    // 1. Text-only edit on f1_017 (drawn on turn 1 of seed 42, resolved on turn 2)
    let mut text_only_val: Value = serde_json::from_str(EVENT_SKELETONS_JSON).unwrap();
    let f1_017_text = text_only_val
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e.get("id").and_then(Value::as_str) == Some("f1_017"))
        .unwrap();
    f1_017_text["title"] = json!("Edited Lantern Signals");
    f1_017_text["narration_seed"] = json!("Edited narration prose for negative control.");
    f1_017_text["choices"][0]["label"] = json!("Edited choice A label");
    f1_017_text["choices"][0]["outcome_sentence"] = json!("Edited outcome sentence for choice A.");
    let text_only_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        &serde_json::to_string(&text_only_val).unwrap(),
    )
    .unwrap();
    let text_only_trace = run_tier2_seed_trace(42, 2, &text_only_pack, 5);
    for (bt, tt) in base_trace.turns.iter().zip(text_only_trace.turns.iter()) {
        assert_eq!(
            bt.mechanics_state_hash, tt.mechanics_state_hash,
            "text-only edit must not change mechanics_state_hash on turn {}",
            bt.turn
        );
    }
    assert_ne!(
        base_trace.turns[0].presentation_record, text_only_trace.turns[0].presentation_record,
        "text-only edit must change presentation_record on turn 1"
    );

    // 2. Mechanics edit A: changing f1_017's tags from ["folklore", ...] to ["river"] changes
    // recent_event_tags on turn 1 (when f1_017 is selected) and memory_cards on turn 2.
    let mut tag_edit_val: Value = serde_json::from_str(EVENT_SKELETONS_JSON).unwrap();
    let f1_017_tag = tag_edit_val
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e.get("id").and_then(Value::as_str) == Some("f1_017"))
        .unwrap();
    f1_017_tag["tags"] = json!(["river"]);
    let tag_edit_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        &serde_json::to_string(&tag_edit_val).unwrap(),
    )
    .unwrap();
    let tag_edit_trace = run_tier2_seed_trace(42, 2, &tag_edit_pack, 5);
    assert_ne!(
        tag_edit_trace.turns[0].mechanics_state_hash, base_trace.turns[0].mechanics_state_hash,
        "changing event tags in content file must change mechanics_state_hash on turn 1"
    );

    // 3. Mechanics edit B: changing one choice's effect value on f1_017 changes mechanics_state_hash on turn 2
    let mut effect_edit_val: Value = serde_json::from_str(EVENT_SKELETONS_JSON).unwrap();
    let f1_017_eff = effect_edit_val
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e.get("id").and_then(Value::as_str) == Some("f1_017"))
        .unwrap();
    for ch in f1_017_eff["choices"].as_array_mut().unwrap() {
        ch["engine_effect_profile"]["food"] = json!(5);
    }
    let effect_edit_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        &serde_json::to_string(&effect_edit_val).unwrap(),
    )
    .unwrap();
    let effect_edit_trace = run_tier2_seed_trace(42, 2, &effect_edit_pack, 5);
    assert_ne!(
        effect_edit_trace.turns[1].mechanics_state_hash, base_trace.turns[1].mechanics_state_hash,
        "changing a choice effect value in content file must change mechanics_state_hash on turn 2"
    );
}

#[test]
fn test_fix1_validator_unknown_reference_fixtures_by_kind() {
    use etv_core::validator::ValidationDiagnostic;

    let canonical_trait_hint =
        "Use a canonical trait ID: tough, healer, tracker, mechanic, lucky, anxious, steady, sharp_eye.";

    // 1. Unknown trait ID fixture (precondition, choice required_trait, outcome grant_trait)
    let unknown_trait_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[
            {
                "id": "ev_unknown_trait",
                "title": "Unknown Trait Fixture",
                "preconditions": ["trait:nonexistent_wizard"],
                "choices": [
                    {
                        "choice_id": "A",
                        "label": "Wait",
                        "intent_action": "WAIT",
                        "required_trait": "ghost_whisperer",
                        "cost_hint": "None",
                        "risk_hint": "Safe",
                        "engine_effect_profile": {
                            "time_days": 1,
                            "grant_trait": "not_a_trait"
                        }
                    }
                ]
            }
        ]"#,
    )
    .unwrap();
    let rep_trait = validate_content(&unknown_trait_pack);
    assert_eq!(
        rep_trait.errors,
        vec![
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_trait': unknown trait 'nonexistent_wizard' referenced in precondition 'trait:nonexistent_wizard'".to_string(),
                hint: canonical_trait_hint.to_string(),
            },
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_trait' choice 'A' references unknown trait 'ghost_whisperer'".to_string(),
                hint: canonical_trait_hint.to_string(),
            },
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_trait' choice 'A' outcome references unknown trait 'not_a_trait'".to_string(),
                hint: canonical_trait_hint.to_string(),
            },
        ]
    );

    // 2. Unknown place ID fixture (precondition, event place_id, outcome place_id)
    let unknown_place_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[
            {
                "id": "ev_unknown_place",
                "title": "Unknown Place Fixture",
                "place_id": "UnknownTown",
                "preconditions": ["place:Atlantis"],
                "choices": [
                    {
                        "choice_id": "A",
                        "label": "Wait",
                        "intent_action": "WAIT",
                        "engine_effect_profile": {
                            "time_days": 1,
                            "place_id": "NotAPlace"
                        }
                    }
                ]
            }
        ]"#,
    )
    .unwrap();
    let rep_place = validate_content(&unknown_place_pack);
    assert_eq!(
        rep_place.errors,
        vec![
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_place': unknown place 'Atlantis' referenced in precondition 'place:Atlantis'".to_string(),
                hint: "Use a canonical place name from TOWN_NAMES or LANDMARK_NAMES.".to_string(),
            },
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_place' references unknown place_id 'UnknownTown'".to_string(),
                hint: "Use a canonical place name from TOWN_NAMES or LANDMARK_NAMES.".to_string(),
            },
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_place' choice 'A' outcome references unknown place 'NotAPlace'".to_string(),
                hint: "Use a canonical place name from TOWN_NAMES or LANDMARK_NAMES.".to_string(),
            },
        ]
    );

    // 3. Unknown event ID fixture (precondition, outcome follow_up_event_id)
    let unknown_event_pack = ContentPack::from_raw_strings(
        CONTENT_MANIFEST_JSON,
        r#"[
            {
                "id": "ev_unknown_event",
                "title": "Unknown Event Fixture",
                "preconditions": ["event:missing_ev_999"],
                "choices": [
                    {
                        "choice_id": "A",
                        "label": "Wait",
                        "intent_action": "WAIT",
                        "engine_effect_profile": {
                            "time_days": 1,
                            "follow_up_event_id": "no_such_followup"
                        }
                    }
                ]
            }
        ]"#,
    )
    .unwrap();
    let rep_event = validate_content(&unknown_event_pack);
    assert_eq!(
        rep_event.errors,
        vec![
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_event': unknown event_id 'missing_ev_999' referenced in precondition 'event:missing_ev_999'".to_string(),
                hint: "Reference an existing event_id present in the event library.".to_string(),
            },
            ValidationDiagnostic {
                code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                message: "event 'ev_unknown_event' choice 'A' outcome references unknown event_id 'no_such_followup'".to_string(),
                hint: "Reference an existing event_id present in the event library.".to_string(),
            },
        ]
    );
}
