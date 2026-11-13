//! Phase 2, Part A integration tests:
//! 1. Trail Ledger (`trail_ledger(&RunState) -> Vec<String>`) line-exact parity against
//!    `rust/oracle/vectors/ledger_reference.json.gz` and end-of-run `presentation_record` wiring.
//! 2. `etv --notices` LF-only output, `Cargo.lock`-derived compiled crate set equality (plus
//!    negative fixture tests), reproducible generator byte-for-byte currency, and SPDX license
//!    allow-list enforcement (no missing license text, no unallowed license, no copyleft license).
//! 3. Narrator seam (`Narrator`, `NullNarrator`, `NarrationContext`, 5 "turn matters" flag rules +
//!    negative test, tightened `night_at_camp` real engine step test, and 1,000-seed Tier 2 sweep
//!    proving `mechanics_state_hash` invariance).

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
use etv_core::memory::{
    build_gm_brief, build_gm_brief_readonly, compute_pressures, compute_themes,
};
use etv_core::model::{
    Condition, GMProfile, GamePhase, IntentAction, MemoryCard, PlayerIntent, RunState, TimeOfDay,
};
use etv_core::narrator::{
    build_narration_context, build_narration_context_with_phase, evaluate_turn_matters,
    journey_band, NarrationContext, Narrator, NullNarrator, PartyStatusFacts, PreTurnObservation,
};
use etv_core::notices::{
    compute_compiled_crates_from_cargo_lock, embedded_license_allowlist, embedded_notices_manifest,
    generate_notices_from_workspace, normalize_lf_and_sha256, render_third_party_notices,
    third_party_notices_text, validate_notices_against_allowlist,
    verify_manifest_coverage_against_cargo_lock, CrateLicenseFile, CrateNotice,
    EMBEDDED_NOTICES_MANIFEST_JSON, EMBEDDED_THIRD_PARTY_NOTICES_TXT,
};
use etv_core::physics::{current_node, HealthEffect};
use etv_core::snapshot::{
    build_1000_seeds, run_tier2_seed_trace, run_tier2_seed_trace_with_narrator,
    run_tier2_snapshot_matrix, run_tier2_snapshot_matrix_with_narrator,
};
use etv_core::worldgen::{create_new_run, create_new_run_with_content};

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

    let root = repo_root();
    let cargo_lock_raw =
        std::fs::read_to_string(root.join("rust/Cargo.lock")).expect("read rust/Cargo.lock");
    let cargo_toml_raw = std::fs::read_to_string(root.join("rust/etv-core/Cargo.toml"))
        .expect("read rust/etv-core/Cargo.toml");
    let compiled_crates = compute_compiled_crates_from_cargo_lock(&cargo_lock_raw, &cargo_toml_raw);
    assert!(
        !compiled_crates.is_empty(),
        "compiled crate set from Cargo.lock must not be empty"
    );

    for (name, ver) in &compiled_crates {
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
fn phase2a_notices_cargo_lock_compiled_set_equals_manifest_and_excludes_workspace_and_dev() {
    let root = repo_root();
    let cargo_lock_raw =
        std::fs::read_to_string(root.join("rust/Cargo.lock")).expect("read rust/Cargo.lock");
    let cargo_toml_raw = std::fs::read_to_string(root.join("rust/etv-core/Cargo.toml"))
        .expect("read rust/etv-core/Cargo.toml");
    let manifest = embedded_notices_manifest();

    let compiled_set =
        verify_manifest_coverage_against_cargo_lock(&manifest, &cargo_lock_raw, &cargo_toml_raw)
            .unwrap_or_else(|errs| {
                panic!(
                    "Cargo.lock compiled crate set did not match notices_manifest.json:\n{}",
                    errs.join("\n")
                )
            });

    let manifest_set: Vec<(String, String)> = manifest
        .crates
        .iter()
        .map(|c| (c.name.clone(), c.version.clone()))
        .collect();
    assert_eq!(
        compiled_set, manifest_set,
        "Cargo.lock compiled (name, version) set must equal notices_manifest.json"
    );

    // Workspace crates (`source == None` in Cargo.lock) and dev-only packages (`[dev-dependencies]`
    // in etv-core/Cargo.toml and their transitive closure) must be excluded.
    // (`rustix` left this list when `crossterm` made it a compiled crate on unix targets.)
    for excluded in ["etv-core", "flate2", "tempfile", "miniz_oxide", "fastrand"] {
        assert!(
            !compiled_set.iter().any(|(name, _)| name == excluded),
            "workspace or dev-only package '{excluded}' must be excluded from compiled crate set"
        );
    }
}

#[test]
fn phase2a_notices_cargo_lock_negative_fixture_with_extra_crate_or_mismatch_fails() {
    let root = repo_root();
    let cargo_lock_raw =
        std::fs::read_to_string(root.join("rust/Cargo.lock")).expect("read rust/Cargo.lock");
    let cargo_toml_raw = std::fs::read_to_string(root.join("rust/etv-core/Cargo.toml"))
        .expect("read rust/etv-core/Cargo.toml");
    let manifest = embedded_notices_manifest();

    // 1. Fixture lock with one extra compiled crate (`extra-audit-crate 0.4.2`) must fail and name it.
    let fixture_lock_with_extra = cargo_lock_raw.replace(
        "name = \"etv-core\"\nversion = \"2.0.0-alpha.1\"\ndependencies = [\n",
        "name = \"etv-core\"\nversion = \"2.0.0-alpha.1\"\ndependencies = [\n \"extra-audit-crate\",\n",
    ) + "\n[[package]]\nname = \"extra-audit-crate\"\nversion = \"0.4.2\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"1111111111111111111111111111111111111111111111111111111111111111\"\n";
    let fixture_toml_with_extra = cargo_toml_raw.replace(
        "[dependencies]\n",
        "[dependencies]\nextra-audit-crate = \"0.4\"\n",
    );

    let errs_extra = verify_manifest_coverage_against_cargo_lock(
        &manifest,
        &fixture_lock_with_extra,
        &fixture_toml_with_extra,
    )
    .expect_err("fixture Cargo.lock with one extra crate must fail");
    assert!(
        errs_extra
            .iter()
            .any(|e| e.contains("extra-audit-crate@0.4.2")),
        "failure must name the missing crate 'extra-audit-crate@0.4.2', got: {errs_extra:?}"
    );

    // 2. Fixture lock with a version mismatch on `serde` (`1.0.999` vs `1.0.229`) must fail and name `serde`.
    let fixture_lock_version_mismatch = cargo_lock_raw.replace(
        "name = \"serde\"\nversion = \"1.0.229\"",
        "name = \"serde\"\nversion = \"1.0.999\"",
    );
    let errs_ver = verify_manifest_coverage_against_cargo_lock(
        &manifest,
        &fixture_lock_version_mismatch,
        &cargo_toml_raw,
    )
    .expect_err("fixture Cargo.lock with version mismatch must fail");
    assert!(
        errs_ver
            .iter()
            .any(|e| e.contains("version mismatch for crate 'serde'") && e.contains("1.0.999")),
        "failure must name 'serde' version mismatch, got: {errs_ver:?}"
    );

    // 3. Fixture manifest with an extra crate not in Cargo.lock must fail and name it.
    let mut extra_manifest = manifest.clone();
    let (norm_txt, norm_sha) = normalize_lf_and_sha256(b"MIT License\n");
    extra_manifest.crates.push(CrateNotice {
        name: "zzz-rogue-crate".to_string(),
        version: "0.9.0".to_string(),
        license: "MIT".to_string(),
        repository: "https://example.invalid/zzz".to_string(),
        cargo_lock_checksum: "22".repeat(32),
        targets: vec!["x86_64-unknown-linux-gnu".to_string()],
        license_files: vec![CrateLicenseFile {
            filename: "LICENSE".to_string(),
            sha256: norm_sha,
            text: norm_txt,
        }],
    });
    let errs_manifest_extra = verify_manifest_coverage_against_cargo_lock(
        &extra_manifest,
        &cargo_lock_raw,
        &cargo_toml_raw,
    )
    .expect_err("manifest with extra crate not in Cargo.lock must fail");
    assert!(
        errs_manifest_extra
            .iter()
            .any(|e| e.contains("zzz-rogue-crate@0.9.0")),
        "failure must name the extra crate 'zzz-rogue-crate@0.9.0', got: {errs_manifest_extra:?}"
    );
}

#[test]
fn phase2a_notices_checked_in_file_is_current_with_cargo_lock_and_bundled_files() {
    let root = repo_root();
    let generator_script = root.join("rust/tools/gen_notices.py");
    assert!(
        generator_script.is_file(),
        "checked-in notices generator script must exist at {}",
        generator_script.display()
    );

    let (gen_manifest, gen_manifest_json, gen_notices_txt) = generate_notices_from_workspace(&root)
        .expect("generate_notices_from_workspace must succeed");

    let embedded_manifest = embedded_notices_manifest();
    assert_eq!(
        gen_manifest, embedded_manifest,
        "generated NoticesManifest does not match checked-in notices_manifest.json"
    );

    let clean_embedded_manifest_json = EMBEDDED_NOTICES_MANIFEST_JSON
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    assert_eq!(
        gen_manifest_json.as_bytes(),
        clean_embedded_manifest_json.as_bytes(),
        "checked-in notices_manifest.json does not match generator output byte-for-byte"
    );

    let clean_embedded_notices_txt = EMBEDDED_THIRD_PARTY_NOTICES_TXT
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    assert_eq!(
        gen_notices_txt.as_bytes(),
        clean_embedded_notices_txt.as_bytes(),
        "checked-in THIRD_PARTY_NOTICES.txt does not match generator output byte-for-byte"
    );
    assert_eq!(
        render_third_party_notices(&embedded_manifest),
        third_party_notices_text(),
        "render_third_party_notices does not match third_party_notices_text()"
    );
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
            "[narrator #{idx} at {} ({}) fallback={:?}]",
            ctx.place(),
            ctx.band(),
            ctx.fallback_line()
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
    // Uses real engine steps starting from an unmutated `create_new_run(42)` state (which begins in
    // `GamePhase::Camp` at `TimeOfDay::Morning`).
    let mut engine = StepEngine::new(create_new_run(42));
    assert_eq!(engine.phase, GamePhase::Camp);
    assert_eq!(engine.state.time_of_day, TimeOfDay::Morning);

    let rest_intent = PlayerIntent::new(IntentAction::Rest);

    // Step 1: Rest from Morning -> Afternoon at camp. Turn ends at Afternoon, NOT Night.
    let pre1 = PreTurnObservation::capture(engine.phase, &engine.state);
    let msgs1 = engine.step(&rest_intent);
    let flags1 = evaluate_turn_matters(&pre1, &rest_intent, &msgs1, &engine.state);
    assert_eq!(engine.phase, GamePhase::Camp);
    assert_eq!(engine.state.time_of_day, TimeOfDay::Afternoon);
    assert!(
        !flags1.night_at_camp,
        "resting from Morning to Afternoon must NOT flag night_at_camp"
    );
    assert!(!flags1.any());

    // Step 2: Rest from Afternoon -> Evening at camp. Turn ends at Evening, NOT Night.
    let pre2 = PreTurnObservation::capture(engine.phase, &engine.state);
    let msgs2 = engine.step(&rest_intent);
    let flags2 = evaluate_turn_matters(&pre2, &rest_intent, &msgs2, &engine.state);
    assert_eq!(engine.phase, GamePhase::Camp);
    assert_eq!(engine.state.time_of_day, TimeOfDay::Evening);
    assert!(
        !flags2.night_at_camp,
        "resting from Afternoon to Evening must NOT flag night_at_camp"
    );
    assert!(!flags2.any());

    // Step 3: Rest from Evening -> Night at camp. Turn ends at Night while camped -> MUST flag!
    let pre3 = PreTurnObservation::capture(engine.phase, &engine.state);
    let msgs3 = engine.step(&rest_intent);
    let flags3 = evaluate_turn_matters(&pre3, &rest_intent, &msgs3, &engine.state);
    assert_eq!(engine.phase, GamePhase::Camp);
    assert_eq!(engine.state.time_of_day, TimeOfDay::Night);
    assert!(
        flags3.night_at_camp,
        "turn ending at TimeOfDay::Night while in GamePhase::Camp must flag night_at_camp"
    );
    assert!(!flags3.death);
    assert!(!flags3.injury);
    assert!(!flags3.supply_zero);
    assert!(!flags3.landmark_arrival);
    assert!(flags3.any());

    // Step 4: Rest from Night -> Morning (Day 2) at camp. Turn ends at Morning -> must NOT flag!
    let pre4 = PreTurnObservation::capture(engine.phase, &engine.state);
    let msgs4 = engine.step(&rest_intent);
    let flags4 = evaluate_turn_matters(&pre4, &rest_intent, &msgs4, &engine.state);
    assert_eq!(engine.phase, GamePhase::Camp);
    assert_eq!(engine.state.time_of_day, TimeOfDay::Morning);
    assert!(
        !flags4.night_at_camp,
        "turn starting at Night but ending at Morning must NOT flag night_at_camp"
    );
}

#[test]
fn phase2a_narrator_flag_negative_unflagged_turn_still_calls_narrator_with_false_flags() {
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
    let ctx = build_narration_context(&state, "", "", &msgs, flags);
    assert!(!ctx.flags().any());
    let narrator_text = narrator.narrate(&ctx);
    assert_eq!(narrator.count(), 1);
    assert!(narrator_text.is_some());
    assert_eq!(NullNarrator.narrate(&ctx), None);
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

            expected_calls_in_seed += 1;
            if rt.presentation_record.turn_matters {
                total_flagged_turns += 1;
            } else {
                total_unflagged_turns += 1;
            }
            assert_eq!(
                nt.presentation_record.narrator_text, None,
                "NullNarrator must return None on every turn"
            );
            assert!(
                rt.presentation_record.narrator_text.is_some(),
                "test narrator must populate presentation_record.narrator_text on every turn"
            );
            assert_ne!(
                nt.presentation_record, rt.presentation_record,
                "presentation_record must differ on every turn when narrator emits text"
            );
            assert_eq!(
                nt.presentation_record.narrator_calls, expected_calls_in_seed,
                "NullNarrator narrator_calls count mismatch on seed {} turn {}",
                null_trace.seed, nt.turn
            );
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
        null_matrix.total_turns,
        "narrator must be invoked once on every shown turn"
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

struct PlaceholderNarrator;

impl Narrator for PlaceholderNarrator {
    fn narrate(&self, _ctx: &NarrationContext) -> Option<String> {
        Some("PLACEHOLDER".to_string())
    }
}

#[test]
fn narrator_every_turn_called_once_per_shown_turn_200_seeds() {
    let pack = embedded_content_pack();
    let seeds = &build_1000_seeds()[..200];
    assert_eq!(seeds.len(), 200);

    let counting = CountingNarrator::new();
    let mut total_shown_turns = 0usize;
    let mut unflagged_turns = 0usize;

    for (idx, &seed) in seeds.iter().enumerate() {
        let before_seed = counting.count();
        let trace = run_tier2_seed_trace_with_narrator(seed, idx, pack, 40, &counting);
        let seed_calls = counting.count() - before_seed;
        assert_eq!(
            seed_calls,
            trace.turns.len(),
            "counting fake narrator must be called once per shown turn on seed {seed}"
        );
        for (turn_idx, t) in trace.turns.iter().enumerate() {
            let expected_turn_calls = turn_idx + 1;
            assert_eq!(
                t.presentation_record.narrator_calls, expected_turn_calls,
                "narrator_calls must equal shown turn index on seed {seed} turn {}",
                t.turn
            );
            if !t.presentation_record.turn_matters {
                unflagged_turns += 1;
            }
        }
        total_shown_turns += trace.turns.len();
    }

    assert!(total_shown_turns > 0);
    assert!(
        unflagged_turns > 0,
        "200-seed sweep must include unflagged turns that still invoke the narrator"
    );
    assert_eq!(counting.count(), total_shown_turns);
}

#[test]
fn narrator_every_turn_mechanics_untouched_across_null_counting_and_placeholder_200_seeds() {
    let pack = embedded_content_pack();
    let seeds = &build_1000_seeds()[..200];
    assert_eq!(seeds.len(), 200);

    let counting = CountingNarrator::new();
    let placeholder = PlaceholderNarrator;

    for (idx, &seed) in seeds.iter().enumerate() {
        let null_trace = run_tier2_seed_trace_with_narrator(seed, idx, pack, 40, &NullNarrator);
        let counting_trace = run_tier2_seed_trace_with_narrator(seed, idx, pack, 40, &counting);
        let placeholder_trace =
            run_tier2_seed_trace_with_narrator(seed, idx, pack, 40, &placeholder);

        assert_eq!(null_trace.turns.len(), counting_trace.turns.len());
        assert_eq!(null_trace.turns.len(), placeholder_trace.turns.len());

        for i in 0..null_trace.turns.len() {
            let nt = &null_trace.turns[i];
            let ct = &counting_trace.turns[i];
            let pt = &placeholder_trace.turns[i];

            assert_eq!(nt.turn, ct.turn);
            assert_eq!(nt.turn, pt.turn);
            assert_eq!(
                nt.mechanics_state_hash, ct.mechanics_state_hash,
                "CountingNarrator mechanics_state_hash diverged from NullNarrator on seed {seed} turn {}",
                nt.turn
            );
            assert_eq!(
                nt.mechanics_state_hash, pt.mechanics_state_hash,
                "PlaceholderNarrator mechanics_state_hash diverged from NullNarrator on seed {seed} turn {}",
                nt.turn
            );
            assert_eq!(nt.presentation_record.narrator_calls, i + 1);
            assert_eq!(ct.presentation_record.narrator_calls, i + 1);
            assert_eq!(pt.presentation_record.narrator_calls, i + 1);
            assert_eq!(nt.presentation_record.narrator_text, None);
            assert_eq!(
                pt.presentation_record.narrator_text.as_deref(),
                Some("PLACEHOLDER")
            );
        }
    }
}

#[test]
fn narrator_every_turn_context_build_is_pure_on_run_state_rng_and_memory_marks() {
    let pack = embedded_content_pack();
    for &seed in &[0_i128, 1, 42, -42, 7] {
        let mut state = create_new_run_with_content(seed, pack);
        // Seed memory cards with expired cooldown (`cooldown_until == 0`) and matching tags so a
        // non-pure `retrieve_memories(..., true)` call would mutate `day_last_seen` and
        // `cooldown_until`.
        state.memory_cards.push(MemoryCard {
            id: "test_mem_river".to_string(),
            kind: "omen".to_string(),
            title: "PLACEHOLDER_TITLE".to_string(),
            text: "PLACEHOLDER_TEXT".to_string(),
            tags: vec![
                "river".to_string(),
                "food_low".to_string(),
                "folklore".to_string(),
            ],
            day_created: 1,
            day_last_seen: 1,
            entities: Vec::new(),
            salience: 0.9,
            cooldown_until: 0,
            source: "engine".to_string(),
        });
        state.recent_event_tags = vec!["river".to_string(), "folklore".to_string()];

        let mut engine = StepEngine::with_content(state, pack);
        let counting = CountingNarrator::new();

        for turn in 1..=30 {
            if engine.phase == GamePhase::GameOver || engine.state.game_over {
                break;
            }
            let pre_obs = PreTurnObservation::capture(engine.phase, &engine.state);
            let intent = match engine.phase {
                GamePhase::Event => PlayerIntent::new(IntentAction::Choose).with_choice("A"),
                GamePhase::Route => PlayerIntent::new(IntentAction::Choose).with_choice("A"),
                GamePhase::Camp => {
                    if turn % 4 == 0 {
                        PlayerIntent::new(IntentAction::Rest)
                    } else {
                        PlayerIntent::new(IntentAction::Travel)
                    }
                }
                GamePhase::GameOver => break,
            };
            let msgs = engine.step(&intent);
            engine.state.rng_counter = engine.rng.counter;
            engine.state.rng_state = Some(engine.rng.getstate());

            let state_before = engine.state.clone();
            let rng_state_before = engine.rng.getstate();
            let rng_counter_before = engine.rng.counter;
            let cards_before = engine.state.memory_cards.clone();

            let flags = evaluate_turn_matters(&pre_obs, &intent, &msgs, &engine.state);
            let ctx = build_narration_context_with_phase(
                &engine.state,
                engine.phase,
                "",
                "",
                &msgs,
                flags,
            );
            let _ = counting.narrate(&ctx);

            assert_eq!(
                engine.state, state_before,
                "RunState mutated during context build on seed {seed} turn {turn}"
            );
            assert_eq!(
                engine.rng.getstate(),
                rng_state_before,
                "RNG state mutated during context build on seed {seed} turn {turn}"
            );
            assert_eq!(
                engine.rng.counter, rng_counter_before,
                "RNG counter mutated during context build on seed {seed} turn {turn}"
            );
            assert_eq!(
                engine.state.memory_cards, cards_before,
                "memory_cards mutated during context build on seed {seed} turn {turn}"
            );
            for (before_card, after_card) in
                cards_before.iter().zip(engine.state.memory_cards.iter())
            {
                assert_eq!(before_card.day_last_seen, after_card.day_last_seen);
                assert_eq!(before_card.cooldown_until, after_card.cooldown_until);
            }
        }
    }
}

#[test]
fn narrator_every_turn_context_facts_match_engine_values() {
    let pack = embedded_content_pack();
    let mut state = create_new_run_with_content(42, pack);
    state.gm_profile = GMProfile::Lantern;
    state.weirdness_level = 3;
    state.uncanny_tokens = 2;
    state.supplies.set_food(4);
    state.supplies.set_water(6);
    state.recent_event_tags = vec!["river".to_string(), "folklore".to_string()];

    let mut engine = StepEngine::with_content(state, pack);
    let pre_obs = PreTurnObservation::capture(engine.phase, &engine.state);
    let intent = PlayerIntent::new(IntentAction::Rest);
    let msgs = engine.step(&intent);
    let flags = evaluate_turn_matters(&pre_obs, &intent, &msgs, &engine.state);

    let ctx = build_narration_context_with_phase(
        &engine.state,
        engine.phase,
        "ev_test",
        "A",
        &msgs,
        flags,
    );

    let expected_brief = build_gm_brief_readonly(&engine.state);
    let mut state_for_mut_brief = engine.state.clone();
    let expected_mut_brief = build_gm_brief(&mut state_for_mut_brief, false);
    assert_eq!(expected_brief.situation, expected_mut_brief.situation);
    assert_eq!(expected_brief.pressures, expected_mut_brief.pressures);
    assert_eq!(expected_brief.themes, expected_mut_brief.themes);
    assert_eq!(expected_brief.tone_profile, expected_mut_brief.tone_profile);
    assert_eq!(
        expected_brief.weirdness_allowance,
        expected_mut_brief.weirdness_allowance
    );

    let expected_node = current_node(&engine.state).expect("current node");
    let expected_fallback = if !msgs.outcome_narration.is_empty() {
        msgs.outcome_narration.clone()
    } else {
        msgs.lines.join(" ")
    };
    let expected_pressures: Vec<String> = compute_pressures(&engine.state)
        .into_iter()
        .take(3)
        .collect();
    let expected_themes = compute_themes(&engine.state);

    assert_eq!(ctx.fallback_line(), expected_fallback);
    assert_eq!(ctx.time_of_day(), engine.state.time_of_day);
    assert_eq!(ctx.phase(), engine.phase);
    assert_eq!(ctx.biome(), expected_node.biome.as_str());
    assert_eq!(ctx.place(), expected_node.name);
    assert_eq!(ctx.day(), engine.state.day);
    assert_eq!(ctx.band(), journey_band(&engine.state));
    assert_eq!(ctx.pressures(), expected_pressures.as_slice());
    assert_eq!(ctx.themes(), expected_themes.as_slice());
    assert_eq!(
        ctx.weirdness_allowance(),
        expected_brief.weirdness_allowance
    );
    assert_eq!(ctx.gm_profile(), engine.state.gm_profile);
    assert_eq!(ctx.situation(), expected_brief.situation);
    assert_eq!(
        ctx.party_status(),
        &PartyStatusFacts::from_state(&engine.state)
    );
    assert_eq!(
        ctx.party_status().alive_count(),
        engine.state.party.alive_count()
    );
    assert_eq!(
        ctx.party_status().total_count(),
        engine.state.party.members.len()
    );
    assert_eq!(ctx.flags(), &flags);
}

#[test]
fn narrator_every_turn_party_status_contains_no_member_names() {
    let pack = embedded_content_pack();
    for &seed in &[0_i128, 1, 2, 42, -42, 99] {
        let mut state = create_new_run_with_content(seed, pack);
        // Exercise injured, sick, exhausted, critical-health, and dead member states.
        if state.party.members.len() >= 4 {
            state.party.members[0].condition = Condition::Sick;
            state.party.members[0].health = 22;
            state.party.members[1].condition = Condition::Injured;
            state.party.members[1].health = 55;
            state.party.members[2].condition = Condition::Exhausted;
            state.party.members[2].health = 70;
            state.party.members[3].health = 0;
            state.party.members[3].death_cause = "Exposure".to_string();
        }
        let member_names: Vec<String> =
            state.party.members.iter().map(|m| m.name.clone()).collect();
        assert!(!member_names.is_empty());

        let msgs = StepMessages::default();
        let flags = evaluate_turn_matters(
            &PreTurnObservation::capture(GamePhase::Camp, &state),
            &PlayerIntent::new(IntentAction::Rest),
            &msgs,
            &state,
        );
        let ctx = build_narration_context_with_phase(&state, GamePhase::Camp, "", "", &msgs, flags);

        let party_json =
            serde_json::to_string(ctx.party_status()).expect("serialize PartyStatusFacts");
        let party_debug = format!("{:?}", ctx.party_status());
        let situation_str = ctx.situation();

        for name in &member_names {
            assert!(
                !name.is_empty(),
                "generated party member name must not be empty"
            );
            assert!(
                !party_json.contains(name),
                "party_status JSON must not contain member name '{name}': {party_json}"
            );
            assert!(
                !party_debug.contains(name),
                "party_status Debug output must not contain member name '{name}': {party_debug}"
            );
            assert!(
                !ctx.party_status().health_band().contains(name),
                "health_band must not contain member name '{name}'"
            );
            assert!(
                !ctx.party_status().morale_band().contains(name),
                "morale_band must not contain member name '{name}'"
            );
            assert!(
                !situation_str.contains(name),
                "situation must not contain member name '{name}': {situation_str}"
            );
        }
    }
}
