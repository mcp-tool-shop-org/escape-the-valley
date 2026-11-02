//! Phase 2, Part A integration tests:
//! 1. Trail Ledger (`trail_ledger(&RunState) -> Vec<String>`) line-exact parity against
//!    `rust/oracle/vectors/ledger_reference.json.gz` and end-of-run `presentation_record` wiring.
//! 2. `etv --notices` LF-only output, crate + bundled data file coverage, `Cargo.lock` currency,
//!    and SPDX license allow-list enforcement (no missing license text, no unallowed license, no
//!    copyleft license).
//! 3. Narrator seam (`Narrator`, `NullNarrator`, `NarrationContext`, 5 "turn matters" flag rules +
//!    negative test, and 1,000-seed Tier 2 sweep proving `mechanics_state_hash` invariance).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use flate2::read::GzDecoder;
use serde_json::Value;

use etv_core::content::embedded_content_pack;
use etv_core::engine::{StepEngine, StepMessages};
use etv_core::ledger::trail_ledger;
use etv_core::model::{GamePhase, IntentAction, PlayerIntent, RunState, TimeOfDay};
use etv_core::narrator::{
    build_narration_context, evaluate_turn_matters, NarrationContext, Narrator, NullNarrator,
    PreTurnObservation,
};
use etv_core::notices::{
    embedded_license_allowlist, embedded_notices_manifest, normalize_lf_and_sha256,
    parse_cargo_lock_packages, render_third_party_notices, third_party_notices_text,
    validate_notices_against_allowlist, CrateLicenseFile, CrateNotice,
};
use etv_core::physics::HealthEffect;
use etv_core::snapshot::{
    run_tier2_seed_trace, run_tier2_snapshot_matrix, run_tier2_snapshot_matrix_with_narrator,
};
use etv_core::worldgen::create_new_run;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn load_gzip_json(rel_path: &str) -> Value {
    let full = repo_root().join(rel_path);
    let file =
        File::open(&full).unwrap_or_else(|e| panic!("failed to open {}: {e}", full.display()));
    let mut decoder = GzDecoder::new(file);
    let mut json_str = String::new();
    decoder
        .read_to_string(&mut json_str)
        .unwrap_or_else(|e| panic!("failed to gunzip {}: {e}", full.display()));
    serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("failed to parse JSON {}: {e}", full.display()))
}

// ============================================================================
// 1. Trail Ledger tests
// ============================================================================

#[test]
fn phase2a_trail_ledger_matches_all_reference_vectors_line_exact() {
    let doc = load_gzip_json("rust/oracle/vectors/ledger_reference.json.gz");
    assert_eq!(doc["schema"].as_str(), Some("etv-phase2-ledger-v1"));

    let cases = doc["ledger_cases"]
        .as_array()
        .expect("ledger_cases must be an array");
    assert_eq!(cases.len(), 12, "expected all 12 ledger reference vectors");

    for case in cases {
        let case_id = case["case_id"].as_str().expect("case_id");
        let state: RunState = serde_json::from_value(case["state"].clone())
            .unwrap_or_else(|e| panic!("ledger case {case_id} state failed to parse: {e}"));
        let expected_lines: Vec<String> = case["trail_ledger_lines"]
            .as_array()
            .expect("trail_ledger_lines array")
            .iter()
            .map(|v| v.as_str().expect("string line").to_string())
            .collect();
        let expected_text = case["trail_ledger_text"]
            .as_str()
            .expect("trail_ledger_text");

        let actual_lines = trail_ledger(&state);
        assert_eq!(
            actual_lines, expected_lines,
            "trail_ledger mismatch on reference vector '{case_id}'"
        );
        assert_eq!(
            actual_lines.join("\n"),
            expected_text,
            "trail_ledger joined text mismatch on reference vector '{case_id}'"
        );
    }
}

#[test]
fn phase2a_trail_ledger_end_of_run_presentation_record_and_mechanics_hash_isolation() {
    let pack = embedded_content_pack();
    let trace = run_tier2_seed_trace(0, 0, pack, 200);
    assert_eq!(
        trace.turns.len(),
        93,
        "seed 0 should reach GameOver at turn 93"
    );

    for turn_snap in &trace.turns[..trace.turns.len() - 1] {
        assert!(
            turn_snap.presentation_record.trail_ledger_lines.is_empty(),
            "non-terminal turn {} must have empty trail_ledger_lines",
            turn_snap.turn
        );
    }

    let final_turn = trace.turns.last().expect("final turn");
    assert_eq!(final_turn.turn, 93);
    assert_eq!(
        final_turn.mechanics_state_hash,
        "008d0402081435e4009a986113fe680ba96bbff78f187526e76c43449b1ded19",
        "adding Trail Ledger to presentation_record must not alter mechanics_state_hash"
    );
    assert!(
        !final_turn.presentation_record.trail_ledger_lines.is_empty(),
        "terminal turn must populate presentation_record.trail_ledger_lines"
    );
    assert_eq!(
        final_turn.presentation_record.trail_ledger_lines[0],
        "========================================"
    );
    assert_eq!(
        final_turn.presentation_record.trail_ledger_lines[1],
        "TRAIL LEDGER"
    );
    assert_eq!(
        final_turn.presentation_record.trail_ledger_lines[2],
        "========================================"
    );

    // Also verify CLI `etv run-seed 0 --turns 200` emits TRAIL LEDGER at end of run.
    let bin = env!("CARGO_BIN_EXE_etv");
    let output = Command::new(bin)
        .args(["run-seed", "0", "--turns", "200"])
        .stdin(Stdio::null())
        .output()
        .expect("run etv run-seed 0 --turns 200");
    assert!(output.status.success(), "etv run-seed 0 failed");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(
        stdout.contains("TRAIL LEDGER\n========================================\n"),
        "etv run-seed must print TRAIL LEDGER at end of run"
    );
}

// ============================================================================
// 2. `etv --notices` tests
// ============================================================================

#[test]
fn phase2a_notices_cli_flag_outputs_lf_only_and_covers_all_compiled_crates_and_bundled_files() {
    let bin = env!("CARGO_BIN_EXE_etv");
    let output = Command::new(bin)
        .arg("--notices")
        .stdin(Stdio::null())
        .output()
        .expect("run etv --notices");
    assert!(
        output.status.success(),
        "etv --notices failed with status {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let raw_bytes = &output.stdout;
    assert!(
        !raw_bytes.is_empty(),
        "etv --notices output must not be empty"
    );
    assert!(
        !raw_bytes.contains(&b'\r'),
        "etv --notices output must be strictly LF-only (found CR byte)"
    );
    assert_eq!(
        raw_bytes.last().copied(),
        Some(b'\n'),
        "etv --notices output must end with LF"
    );

    let text = std::str::from_utf8(raw_bytes).expect("valid UTF-8 notices");
    assert_eq!(text, third_party_notices_text());

    let expected_compiled_crates = [
        ("block-buffer", "0.10.4"),
        ("cfg-if", "1.0.5"),
        ("cpufeatures", "0.2.17"),
        ("crypto-common", "0.1.7"),
        ("digest", "0.10.7"),
        ("generic-array", "0.14.7"),
        ("getrandom", "0.3.4"),
        ("itoa", "1.0.18"),
        ("libc", "0.2.191"),
        ("memchr", "2.8.3"),
        ("proc-macro2", "1.0.107"),
        ("quote", "1.0.47"),
        ("serde", "1.0.229"),
        ("serde_core", "1.0.229"),
        ("serde_derive", "1.0.229"),
        ("serde_json", "1.0.154"),
        ("sha2", "0.10.9"),
        ("syn", "3.0.9"),
        ("typenum", "1.20.1"),
        ("unicode-ident", "1.0.27"),
        ("version_check", "0.9.5"),
        ("zmij", "1.0.23"),
    ];

    for (name, ver) in expected_compiled_crates {
        let header = format!("Name: {name}\nVersion: {ver}\n");
        assert!(
            text.contains(&header),
            "etv --notices missing compiled crate section for {name} {ver}"
        );
    }

    for bundled in [
        "Bundled Data File: rust/etv-core/content/content_manifest.json",
        "Bundled Data File: rust/etv-core/content/event_skeletons.json",
    ] {
        assert!(
            text.contains(bundled),
            "etv --notices missing bundled data file section '{bundled}'"
        );
    }
}

#[test]
fn phase2a_notices_checked_in_file_is_current_with_cargo_lock_and_bundled_files() {
    let manifest = embedded_notices_manifest();
    let rendered = render_third_party_notices(&manifest);
    let embedded_txt = third_party_notices_text();
    assert_eq!(
        rendered, embedded_txt,
        "checked-in THIRD_PARTY_NOTICES.txt is out of sync with notices_manifest.json"
    );

    let root = repo_root();
    let (_, repo_lic_sha) =
        normalize_lf_and_sha256(&std::fs::read(root.join("LICENSE")).expect("read repo LICENSE"));
    assert_eq!(
        manifest.package.license_sha256, repo_lic_sha,
        "manifest package license SHA-256 does not match repository LICENSE"
    );

    for bf in &manifest.bundled_data_files {
        let disk_bytes = std::fs::read(root.join(&bf.path))
            .unwrap_or_else(|e| panic!("failed to read bundled file {}: {e}", bf.path));
        let (_, disk_sha) = normalize_lf_and_sha256(&disk_bytes);
        assert_eq!(
            bf.sha256, disk_sha,
            "bundled data file {} SHA-256 in notices_manifest.json is stale",
            bf.path
        );
        assert_eq!(
            bf.license_sha256, repo_lic_sha,
            "bundled data file {} license SHA-256 is stale",
            bf.path
        );
    }

    let cargo_lock_raw =
        std::fs::read_to_string(root.join("rust/Cargo.lock")).expect("read rust/Cargo.lock");
    let lock_pkgs = parse_cargo_lock_packages(&cargo_lock_raw);

    let cargo_toml_raw = std::fs::read_to_string(root.join("rust/etv-core/Cargo.toml"))
        .expect("read rust/etv-core/Cargo.toml");
    // Verify direct runtime dependencies from rust/etv-core/Cargo.toml are present in manifest.
    for direct_dep in ["serde", "serde_json", "sha2"] {
        assert!(
            cargo_toml_raw.contains(direct_dep),
            "expected direct dependency {direct_dep} in rust/etv-core/Cargo.toml"
        );
        assert!(
            manifest.crates.iter().any(|c| c.name == direct_dep),
            "direct dependency {direct_dep} missing from notices_manifest.json"
        );
    }

    for crate_notice in &manifest.crates {
        let key = (crate_notice.name.clone(), crate_notice.version.clone());
        let lock_chk = lock_pkgs
            .get(&key)
            .unwrap_or_else(|| {
                panic!(
                    "crate {} {} in notices_manifest.json not found in rust/Cargo.lock",
                    crate_notice.name, crate_notice.version
                )
            })
            .as_deref()
            .expect("third-party crate in Cargo.lock must have checksum");
        assert_eq!(
            crate_notice.cargo_lock_checksum, lock_chk,
            "Cargo.lock checksum mismatch for {} {}",
            crate_notice.name, crate_notice.version
        );
    }
}

#[test]
fn phase2a_notices_allowlist_passes_current_build_and_rejects_missing_unallowed_or_copyleft() {
    let manifest = embedded_notices_manifest();
    let allowlist = embedded_license_allowlist();

    // 1. Current build passes cleanly.
    validate_notices_against_allowlist(&manifest, &allowlist)
        .expect("current build must pass license allow-list validation");

    // 2. Missing license text on a crate fails.
    let mut missing_text_manifest = manifest.clone();
    missing_text_manifest.crates[0].license_files.clear();
    let err_missing =
        validate_notices_against_allowlist(&missing_text_manifest, &allowlist).unwrap_err();
    assert!(
        err_missing.iter().any(|e| e.code == "MISSING_LICENSE_TEXT"),
        "expected MISSING_LICENSE_TEXT error, got {err_missing:?}"
    );

    // 3. Unallowed license expression fails.
    let mut unallowed_manifest = manifest.clone();
    unallowed_manifest.crates[0].license = "BSD-4-Clause".to_string();
    let err_unallowed =
        validate_notices_against_allowlist(&unallowed_manifest, &allowlist).unwrap_err();
    assert!(
        err_unallowed
            .iter()
            .any(|e| e.code == "UNALLOWED_LICENSE_EXPRESSION"
                || e.code == "UNALLOWED_SPDX_IDENTIFIER"),
        "expected unallowed license failure, got {err_unallowed:?}"
    );

    // 4. Copyleft license on a crate fails with COPYLEFT_LICENSE_PROHIBITED.
    for copyleft_expr in [
        "GPL-3.0-only",
        "LGPL-2.1-or-later",
        "AGPL-3.0-only",
        "MPL-2.0",
    ] {
        let mut copyleft_manifest = manifest.clone();
        let (norm_txt, norm_sha) = normalize_lf_and_sha256(b"Copyleft sample text\n");
        copyleft_manifest.crates.push(CrateNotice {
            name: "zzz-copyleft-crate".to_string(),
            version: "0.1.0".to_string(),
            license: copyleft_expr.to_string(),
            repository: "https://example.invalid/zzz".to_string(),
            cargo_lock_checksum: "00".repeat(32),
            targets: vec!["x86_64-unknown-linux-gnu".to_string()],
            license_files: vec![CrateLicenseFile {
                filename: "LICENSE".to_string(),
                sha256: norm_sha,
                text: norm_txt,
            }],
        });
        let err_copyleft =
            validate_notices_against_allowlist(&copyleft_manifest, &allowlist).unwrap_err();
        assert!(
            err_copyleft
                .iter()
                .any(|e| e.code == "COPYLEFT_LICENSE_PROHIBITED"),
            "expected COPYLEFT_LICENSE_PROHIBITED for {copyleft_expr}, got {err_copyleft:?}"
        );
    }

    // 5. Even adding a copyleft identifier to the allow-list itself is rejected.
    let mut poisoned_allowlist = allowlist.clone();
    poisoned_allowlist
        .allowed_spdx_identifiers
        .push("GPL-3.0-only".to_string());
    let err_poisoned =
        validate_notices_against_allowlist(&manifest, &poisoned_allowlist).unwrap_err();
    assert!(
        err_poisoned
            .iter()
            .any(|e| e.code == "ALLOWLIST_CONTAINS_COPYLEFT"),
        "expected ALLOWLIST_CONTAINS_COPYLEFT, got {err_poisoned:?}"
    );
}

// ============================================================================
// 3. Narrator seam tests
// ============================================================================

struct CountingNarrator {
    calls: AtomicUsize,
}

impl CountingNarrator {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Narrator for CountingNarrator {
    fn narrate(&self, ctx: &NarrationContext) -> Option<String> {
        let idx = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        Some(format!(
            "[narrator #{idx} at {} ({}) outcome={:?}]",
            ctx.place(),
            ctx.band(),
            ctx.outcome_sentence()
        ))
    }
}

#[test]
fn phase2a_narrator_flag_rule_death() {
    let mut state = create_new_run(42);
    let pre = PreTurnObservation::capture(GamePhase::Camp, &state);
    let dead_name = state.party.members[0].name.clone();
    state.party.members[0].health = 0;
    state.party.members[0].death_cause = "starvation".to_string();

    let mut msgs = StepMessages::default();
    msgs.effects.push(HealthEffect::Died {
        member: dead_name.clone(),
        cause: "starvation".to_string(),
    });
    msgs.lines
        .push(format!("{dead_name} has died (starvation)."));

    let intent = PlayerIntent::new(IntentAction::Travel);
    let flags = evaluate_turn_matters(&pre, &intent, &msgs, &state);
    assert!(flags.death, "death rule must fire when a party member dies");
    assert!(flags.any(), "turn_matters must be true on death");

    let narrator = CountingNarrator::new();
    let ctx = build_narration_context(&state, "", "", &msgs, flags);
    let text = narrator.narrate(&ctx);
    assert_eq!(narrator.count(), 1);
    assert!(ctx.flags().death);
    assert!(text.is_some());
}

#[test]
fn phase2a_narrator_flag_rule_injury() {
    let mut state = create_new_run(42);
    let pre = PreTurnObservation::capture(GamePhase::Camp, &state);
    state.party.members[0].health -= 15;
    assert!(state.party.members[0].is_alive());

    let mut msgs = StepMessages::default();
    msgs.lines
        .push("Sprained ankle on loose shale.".to_string());

    let intent = PlayerIntent::new(IntentAction::Travel);
    let flags = evaluate_turn_matters(&pre, &intent, &msgs, &state);
    assert!(
        flags.injury,
        "injury rule must fire when member health falls below pre-turn health"
    );
    assert!(!flags.death, "non-fatal injury must not set death flag");
    assert!(!flags.supply_zero);
    assert!(!flags.landmark_arrival);
    assert!(!flags.night_at_camp);
    assert!(flags.any());

    let narrator = CountingNarrator::new();
    let ctx = build_narration_context(&state, "trail_shale", "A", &msgs, flags);
    assert!(narrator.narrate(&ctx).is_some());
    assert_eq!(narrator.count(), 1);
}

#[test]
fn phase2a_narrator_flag_rule_supply_reaching_zero() {
    let mut state = create_new_run(42);
    state.supplies.set_water(4);
    let pre = PreTurnObservation::capture(GamePhase::Camp, &state);

    // Water drops from 4 to 0 on this turn.
    state.supplies.set_water(0);
    let mut msgs = StepMessages::default();
    msgs.lines.push("The last canteen runs dry.".to_string());

    let intent = PlayerIntent::new(IntentAction::Travel);
    let flags = evaluate_turn_matters(&pre, &intent, &msgs, &state);
    assert!(
        flags.supply_zero,
        "supply_zero rule must fire when a supply transitions from > 0 to <= 0"
    );
    assert!(!flags.death);
    assert!(!flags.injury);
    assert!(!flags.landmark_arrival);
    assert!(!flags.night_at_camp);
    assert!(flags.any());

    let narrator = CountingNarrator::new();
    let ctx = build_narration_context(&state, "", "", &msgs, flags);
    assert!(narrator.narrate(&ctx).is_some());
    assert_eq!(narrator.count(), 1);
}

#[test]
fn phase2a_narrator_flag_rule_landmark_arrival() {
    let mut state = create_new_run(42);
    state.distance_remaining = 3;
    // Ensure plenty of supplies and full health so only landmark_arrival fires.
    state.supplies.set_food(80);
    state.supplies.set_water(80);
    state.supplies.set("salt", 20);
    let mut engine = StepEngine::new(state);
    let pre = PreTurnObservation::capture(engine.phase, &engine.state);
    let intent = PlayerIntent::new(IntentAction::Travel);
    let msgs = engine.step(&intent);

    let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
    assert!(
        flags.landmark_arrival,
        "landmark_arrival rule must fire when arriving at the next map node"
    );
    assert_ne!(engine.state.location_id, pre.location_id);
    assert!(flags.any());
}

#[test]
fn phase2a_narrator_flag_rule_night_at_camp() {
    let mut state = create_new_run(42);
    state.time_of_day = TimeOfDay::Evening;
    state.supplies.set_food(80);
    state.supplies.set_water(80);
    let mut engine = StepEngine::new(state);

    let pre = PreTurnObservation::capture(engine.phase, &engine.state);
    let intent = PlayerIntent::new(IntentAction::Rest);
    let msgs = engine.step(&intent);

    let flags = evaluate_turn_matters(&pre, &intent, &msgs, &engine.state);
    assert!(
        flags.night_at_camp,
        "night_at_camp rule must fire when resting at camp into the night"
    );
    assert_eq!(engine.state.time_of_day, TimeOfDay::Night);
    assert!(!flags.death);
    assert!(!flags.injury);
    assert!(!flags.supply_zero);
    assert!(!flags.landmark_arrival);
    assert!(flags.any());
}

#[test]
fn phase2a_narrator_flag_negative_unflagged_turn_does_not_call_narrator() {
    let mut state = create_new_run(42);
    state.time_of_day = TimeOfDay::Morning;
    state.distance_remaining = 40;
    state.supplies.set_food(90);
    state.supplies.set_water(90);
    state.supplies.set("salt", 20);

    let pre = PreTurnObservation::capture(GamePhase::Camp, &state);
    // Simulate a routine daytime travel step with no death, no injury, no supply hitting zero,
    // no landmark arrival, and no night at camp.
    state.distance_remaining = 28;
    state.distance_traveled += 12;
    state.supplies.set_food(86);
    state.supplies.set_water(86);
    state.time_of_day = TimeOfDay::Afternoon;
    state.last_action = "TRAVEL".to_string();

    let mut msgs = StepMessages::default();
    msgs.lines
        .push("Traveled 12 miles. 28 miles remaining.".to_string());

    let intent = PlayerIntent::new(IntentAction::Travel);
    let flags = evaluate_turn_matters(&pre, &intent, &msgs, &state);
    assert!(!flags.death);
    assert!(!flags.injury);
    assert!(!flags.supply_zero);
    assert!(!flags.landmark_arrival);
    assert!(!flags.night_at_camp);
    assert!(
        !flags.any(),
        "turn matching none of the 5 rules must not be flagged"
    );

    let narrator = CountingNarrator::new();
    let narrator_text = if flags.any() {
        let ctx = build_narration_context(&state, "", "", &msgs, flags);
        narrator.narrate(&ctx)
    } else {
        None
    };
    assert_eq!(
        narrator.count(),
        0,
        "narrator must not be called on unflagged turns"
    );
    assert_eq!(narrator_text, None);
    assert_eq!(
        NullNarrator.narrate(&build_narration_context(&state, "", "", &msgs, flags)),
        None
    );
}

struct PseudoRandomTestNarrator {
    counter: AtomicUsize,
}

impl PseudoRandomTestNarrator {
    fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }
}

impl Narrator for PseudoRandomTestNarrator {
    fn narrate(&self, ctx: &NarrationContext) -> Option<String> {
        // SplitMix64 stream generating arbitrary varied strings per call without touching game RNG.
        let n = self.counter.fetch_add(1, Ordering::Relaxed) as u64 + 1;
        let mut z = n.wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        let rand_word = z ^ (z >> 31);
        let _ = (
            ctx.event_id(),
            ctx.choice_id(),
            ctx.shown_effects(),
            ctx.shown_deltas(),
            ctx.shown_lines(),
        );
        Some(format!(
            "NARRATOR_RANDOM_{rand_word:016x}_place={}_band={}_flags={}{}{}{}{}",
            ctx.place(),
            ctx.band(),
            usize::from(ctx.flags().death),
            usize::from(ctx.flags().injury),
            usize::from(ctx.flags().supply_zero),
            usize::from(ctx.flags().landmark_arrival),
            usize::from(ctx.flags().night_at_camp),
        ))
    }
}

#[test]
fn phase2a_narrator_1000_tier2_seeds_random_text_preserves_mechanics_hashes() {
    let pack = embedded_content_pack();
    let max_turns_per_seed = 25;

    let null_matrix = run_tier2_snapshot_matrix(pack, max_turns_per_seed);
    let random_narrator = PseudoRandomTestNarrator::new();
    let narrated_matrix =
        run_tier2_snapshot_matrix_with_narrator(pack, max_turns_per_seed, &random_narrator);

    assert_eq!(null_matrix.seed_count, 1000);
    assert_eq!(narrated_matrix.seed_count, 1000);
    assert_eq!(null_matrix.total_turns, narrated_matrix.total_turns);

    let mut total_flagged_turns = 0usize;
    let mut total_unflagged_turns = 0usize;

    for (null_trace, narr_trace) in null_matrix.traces.iter().zip(narrated_matrix.traces.iter()) {
        assert_eq!(null_trace.seed, narr_trace.seed);
        assert_eq!(null_trace.turns.len(), narr_trace.turns.len());

        let mut expected_calls_in_seed = 0usize;
        for (nt, rt) in null_trace.turns.iter().zip(narr_trace.turns.iter()) {
            assert_eq!(nt.turn, rt.turn);
            assert_eq!(
                nt.mechanics_state_hash, rt.mechanics_state_hash,
                "mechanics_state_hash diverged on seed {} turn {}",
                null_trace.seed, nt.turn
            );
            assert_eq!(
                nt.presentation_record.turn_matters,
                rt.presentation_record.turn_matters
            );
            assert_eq!(
                nt.presentation_record.trail_ledger_lines,
                rt.presentation_record.trail_ledger_lines,
                "narrator output must never feed back into trail_ledger_lines"
            );

            if rt.presentation_record.turn_matters {
                expected_calls_in_seed += 1;
                total_flagged_turns += 1;
                assert_eq!(
                    nt.presentation_record.narrator_text, None,
                    "NullNarrator must return None on flagged turns"
                );
                assert!(
                    rt.presentation_record.narrator_text.is_some(),
                    "test narrator must populate presentation_record.narrator_text on flagged turns"
                );
                assert_ne!(
                    nt.presentation_record, rt.presentation_record,
                    "presentation_record must differ on flagged turns when narrator emits text"
                );
            } else {
                total_unflagged_turns += 1;
                assert_eq!(
                    rt.presentation_record.narrator_text, None,
                    "narrator must not be called on unflagged turns"
                );
                assert_eq!(nt.presentation_record, rt.presentation_record);
            }

            assert_eq!(
                rt.presentation_record.narrator_calls, expected_calls_in_seed,
                "narrator_calls count mismatch on seed {} turn {}",
                null_trace.seed, nt.turn
            );
        }
    }

    assert!(
        total_flagged_turns > 0,
        "expected flagged turns across the 1,000-seed matrix"
    );
    assert!(
        total_unflagged_turns > 0,
        "expected unflagged turns across the 1,000-seed matrix"
    );
    assert_eq!(
        random_narrator.counter.load(Ordering::SeqCst),
        total_flagged_turns,
        "narrator must be invoked exclusively on flagged turns"
    );

    // Ensure no HashMap or HashSet iteration was introduced in the new Phase 2A modules.
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for rel in ["ledger.rs", "notices.rs", "narrator.rs"] {
        let code = std::fs::read_to_string(src_dir.join(rel)).expect("read module source");
        for line in code.lines() {
            let code_part = line.split("//").next().unwrap_or("");
            assert!(
                !code_part.contains("HashMap") && !code_part.contains("HashSet"),
                "src/{rel} must use BTreeMap/Vec only, found HashMap/HashSet in: {line}"
            );
        }
    }
    let _ = BTreeMap::<String, i32>::new();
}
