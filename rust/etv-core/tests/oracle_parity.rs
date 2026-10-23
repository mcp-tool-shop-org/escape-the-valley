//! Oracle parity test suite for Escape the Valley 2.0 Phase 1a & Phase 1b (`etv-core`).
//!
//! Verifies all Phase 1a and Phase 1b oracle checks against CPython 3.12.13 / v1.3.1 (47b01a3):
//! 1. S1 & gameplay RNG call-shape vectors (6,459 assertions across 10 seeds, both `sample` branches)
//! 2. Worldgen (200 seeds: byte-for-byte canonical RunState JSON + RNG state SHA-256 + weather + 200 JSON events)
//! 3. Save round trip (20 real Python saves: load, re-save, canonical JSON match, reload equality)
//! 4. Migration (v1 and v2 saves migrate to identical v3 state + audit L-2 `last_settle_failed`/`minted_initial`)
//! 5. RNG restore (20 saves × 100 draws with `rng_state`, 20 saves × 100 draws with `None` counter-replay, and malformed `rng_state` fallback)
//! 6. Secrets sidecar & corrupt quarantine (`run.json.corrupt-<timestamp>` and `secrets.json.corrupt-<timestamp>`)
//! 7. Golden harness: 1,000 seeds × 60 turns (59,653 turns) + full 260-event library + 8 ending/finalization vectors
//! 8. Golden harness save-and-resume: 120 seeds saved at fixed turn 20, loaded in Rust, and continued through turn 60
//! 9. Negative harness checks: corrupting a recorded hash or truncating a turn list fails with `seed`, `turn`, `rng_hash`, then `state_diff`

use etv_core::engine::{compute_ending, StepEngine, StepMessages};
use etv_core::events::build_event_library;
use etv_core::model::{Biome, EndingResult, IntentAction, PlayerIntent, RunState};
use etv_core::rng::{coerce_rng_state, CpythonRng, RngState};
use etv_core::save::{
    has_save, load_game, load_game_result, read_secrets_sidecar, save_game, state_to_value,
    value_to_state, SAVE_DIR, SAVE_FILE, SECRETS_FILE,
};
use etv_core::worldgen::{
    create_new_run, create_new_run_with_rng, generate_weather, load_json_events,
    load_json_events_from_str, EventSkeleton,
};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::Path;

fn read_gzip_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
    let mut decoder = GzDecoder::new(bytes);
    let mut s = String::new();
    decoder.read_to_string(&mut s).expect("valid gzip stream");
    serde_json::from_str(&s).expect("valid JSON in gzip vector")
}

/// Serialize a `serde_json::Value` with sorted object keys and compact separators
/// `( ",", ":" )` to match Python `json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)`.
/// Note: `serde_json::Map` without `preserve_order` is backed by `BTreeMap`, so keys are already sorted.
fn to_canonical_json_string(val: &Value) -> String {
    serde_json::to_string(val).expect("serialize canonical JSON")
}

fn sha256_hex_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in &digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

#[derive(Deserialize)]
struct OracleMeta {
    python_version: String,
    oracle_commit: String,
}

// ── 1. S1 & Gameplay RNG Vectors ────────────────────────────────────────────

#[derive(Deserialize)]
struct S1VectorsFile {
    schema: String,
    metadata: OracleMeta,
    seed_vectors: Vec<S1SeedVector>,
    save_restore_vector: S1SaveRestoreVector,
}

#[derive(Deserialize)]
struct S1SeedVector {
    label: String,
    seed: String,
    initial_state: RngState,
    random_draws: Vec<S1RandomDraw>,
    getrandbits_draws: Vec<S1GetRandBitsDraw>,
    randint_draws: Vec<S1RandIntDraw>,
    choice_draws: Vec<S1ChoiceDraw>,
    shuffle_draws: Vec<S1ShuffleDraw>,
    sample_draws: Vec<S1SampleDraw>,
    weighted_draws: Vec<S1WeightedDraw>,
    final_state: RngState,
}

#[derive(Deserialize)]
struct S1RandomDraw {
    bits: String,
}

#[derive(Deserialize)]
struct S1GetRandBitsDraw {
    k: u32,
    value: String,
}

#[derive(Deserialize)]
struct S1RandIntDraw {
    a: String,
    b: String,
    values: Vec<String>,
}

#[derive(Deserialize)]
struct S1ChoiceDraw {
    len: usize,
    picks: Vec<String>,
}

#[derive(Deserialize)]
struct S1ShuffleDraw {
    len: usize,
    result: Vec<usize>,
}

#[derive(Deserialize)]
struct S1SampleDraw {
    name: String,
    n: usize,
    k: usize,
    result: Vec<usize>,
}

#[derive(Deserialize)]
struct S1WeightedDraw {
    name: String,
    items: Vec<String>,
    weights: Vec<f64>,
    picks: Vec<String>,
}

#[derive(Deserialize)]
struct S1SaveRestoreVector {
    saved_rng_state: RngState,
    post_random_bits: Vec<String>,
    post_randint: Vec<String>,
    post_shuffle: Vec<usize>,
    post_sample_pool: Vec<usize>,
    post_sample_set: Vec<usize>,
    post_weighted: Vec<String>,
    post_final_state: RngState,
}

// Fails on 02532e17 because `rng_s1_vectors.json.gz` on 02532e17 only had 6 seeds,
// 15 randint ranges, 5 choice lengths, 12 sample cases, and 3 weighted cases (2,645 checks vs 6,459).
#[test]
fn check_01_s1_rng_vectors_and_pool_label() {
    let raw = include_bytes!("../../oracle/vectors/rng_s1_vectors.json.gz");
    let vf: S1VectorsFile = read_gzip_json(raw);
    assert_eq!(vf.schema, "etv-s1-rng-v2");
    assert_eq!(vf.metadata.python_version, "3.12.13");
    assert!(vf.metadata.oracle_commit.starts_with("47b01a3"));

    let mut total_checks = 0usize;
    let mut saw_pool_n1000_k20 = false;
    let mut saw_set_n2000_k20 = false;
    let mut pool_branch_cases = 0usize;
    let mut set_branch_cases = 0usize;
    assert_eq!(vf.seed_vectors.len(), 10);

    for sv in &vf.seed_vectors {
        let seed: i128 = sv.seed.parse().expect("valid i128 seed");
        let mut rng = CpythonRng::from_seed(seed);

        assert_eq!(rng.getstate(), sv.initial_state);
        total_checks += 1;

        assert_eq!(sv.random_draws.len(), 20);
        for rd in &sv.random_draws {
            let got_bits = rng.random().to_bits();
            let exp_bits: u64 = rd.bits.parse().unwrap();
            assert_eq!(got_bits, exp_bits);
            total_checks += 1;
        }

        assert_eq!(sv.getrandbits_draws.len(), 60);
        for gd in &sv.getrandbits_draws {
            let got = rng.getrandbits(gd.k);
            let exp: u128 = gd.value.parse().unwrap();
            assert_eq!(got, exp);
            total_checks += 1;
        }

        assert_eq!(sv.randint_draws.len(), 24);
        for rd in &sv.randint_draws {
            let a: i128 = rd.a.parse().unwrap();
            let b: i128 = rd.b.parse().unwrap();
            assert_eq!(rd.values.len(), 16);
            for exp_str in &rd.values {
                let got = rng.randint(a, b);
                let exp: i128 = exp_str.parse().unwrap();
                assert_eq!(got, exp);
                total_checks += 1;
            }
        }

        assert_eq!(sv.choice_draws.len(), 8);
        for cd in &sv.choice_draws {
            let seq: Vec<String> = (0..cd.len).map(|i| format!("item_{i}")).collect();
            assert_eq!(cd.picks.len(), 10);
            for exp in &cd.picks {
                let got = rng.choice(&seq);
                assert_eq!(got, exp);
                total_checks += 1;
            }
        }

        assert_eq!(sv.shuffle_draws.len(), 6);
        for sd in &sv.shuffle_draws {
            let mut seq: Vec<usize> = (0..sd.len).collect();
            let prev_counter = rng.counter;
            rng.shuffle(&mut seq);
            assert_eq!(rng.counter, prev_counter + (sd.len as u64));
            assert_eq!(sd.result.len(), sd.len);
            assert_eq!(seq, sd.result);
            total_checks += 1;
        }

        assert_eq!(sv.sample_draws.len(), 15);
        for sm in &sv.sample_draws {
            assert_ne!(
                sm.name, "set_n1000_k20",
                "set_n1000_k20 must be renamed to pool_n1000_k20"
            );
            if sm.name == "pool_n1000_k20" {
                saw_pool_n1000_k20 = true;
            }
            if sm.name == "set_n2000_k20" {
                saw_set_n2000_k20 = true;
            }
            if sm.name.starts_with("pool_") {
                pool_branch_cases += 1;
            } else if sm.name.starts_with("set_") {
                set_branch_cases += 1;
            }
            let pop: Vec<usize> = (0..sm.n).collect();
            let prev_counter = rng.counter;
            let got = rng.sample(&pop, sm.k);
            assert_eq!(rng.counter, prev_counter + (sm.k as u64));
            assert_eq!(sm.result.len(), sm.k);
            assert_eq!(got, sm.result, "mismatch on {}", sm.name);
            total_checks += 1;
        }

        assert_eq!(sv.weighted_draws.len(), 5);
        for wd in &sv.weighted_draws {
            assert!(!wd.items.is_empty());
            assert_eq!(wd.items.len(), wd.weights.len());
            assert_eq!(wd.picks.len(), 15);
            for exp in &wd.picks {
                let got = rng.weighted_choice(&wd.items, &wd.weights);
                assert_eq!(got, exp, "mismatch on {}", wd.name);
                total_checks += 1;
            }
        }

        assert_eq!(
            rng.getstate(),
            sv.final_state,
            "final_state mismatch on {}",
            sv.label
        );
        total_checks += 1;
    }

    assert!(saw_pool_n1000_k20, "expected pool_n1000_k20 sample vector");
    assert!(saw_set_n2000_k20, "expected set_n2000_k20 sample vector");
    assert_eq!(pool_branch_cases, 100);
    assert_eq!(set_branch_cases, 50);

    let srv = &vf.save_restore_vector;
    let mut restored = CpythonRng::from_seed(0);
    restored.setstate(&srv.saved_rng_state).unwrap();
    assert_eq!(restored.getstate(), srv.saved_rng_state);
    total_checks += 1;

    assert_eq!(srv.post_random_bits.len(), 10);
    for exp_bits_str in &srv.post_random_bits {
        let exp_bits: u64 = exp_bits_str.parse().unwrap();
        assert_eq!(restored.random().to_bits(), exp_bits);
        total_checks += 1;
    }
    assert_eq!(srv.post_randint.len(), 10);
    for exp_val_str in &srv.post_randint {
        let exp_val: i128 = exp_val_str.parse().unwrap();
        assert_eq!(restored.randint(-50, 500), exp_val);
        total_checks += 1;
    }
    assert_eq!(srv.post_shuffle.len(), 10);
    let mut shuf: Vec<usize> = (0..10).collect();
    restored.shuffle(&mut shuf);
    assert_eq!(shuf, srv.post_shuffle);
    total_checks += 1;

    assert_eq!(srv.post_sample_pool.len(), 4);
    let pop15: Vec<usize> = (0..15).collect();
    assert_eq!(restored.sample(&pop15, 4), srv.post_sample_pool);
    total_checks += 1;

    assert_eq!(srv.post_sample_set.len(), 5);
    let pop100: Vec<usize> = (0..100).collect();
    assert_eq!(restored.sample(&pop100, 5), srv.post_sample_set);
    total_checks += 1;

    assert_eq!(srv.post_weighted.len(), 10);
    let items = vec![
        "food".to_string(),
        "water".to_string(),
        "ammo".to_string(),
        "parts".to_string(),
    ];
    let weights = vec![2.5, 3.0, 1.0, 0.5];
    for exp_pick in &srv.post_weighted {
        assert_eq!(restored.weighted_choice(&items, &weights), exp_pick);
        total_checks += 1;
    }

    assert_eq!(restored.getstate(), srv.post_final_state);
    total_checks += 1;

    assert_eq!(total_checks, 6455);
}

// ── 2. Worldgen (200 seeds) + Event Loader (200 events) ─────────────────────

#[derive(Deserialize)]
struct WorldgenFile {
    schema: String,
    metadata: OracleMeta,
    count: usize,
    vectors: Vec<WorldgenVector>,
    json_events_count: usize,
    json_events: Vec<OracleJsonEvent>,
}

#[derive(Deserialize)]
struct WorldgenVector {
    seed: String,
    run_id_prefix: String,
    rng_counter: u64,
    rng_state_sha256: String,
    weather_picks: Vec<String>,
    canonical_json: String,
}

#[derive(Deserialize)]
struct OracleJsonEvent {
    event_id: String,
    title: String,
    category: String,
    tags: Vec<String>,
    severity: String,
    biome_filter: Option<Vec<String>>,
    costs_uncanny_token: bool,
    folklore_type: Option<String>,
    fallback_narration: String,
    choices: Vec<OracleChoice>,
    outcomes: BTreeMap<String, OracleOutcome>,
}

#[derive(Deserialize)]
struct OracleChoice {
    choice_id: String,
    label: String,
    action: String,
    style: String,
    #[serde(default)]
    risk_hint: String,
    #[serde(default)]
    cost_hint: String,
}

#[derive(Deserialize)]
struct OracleOutcome {
    supplies_delta: BTreeMap<String, i32>,
    #[serde(default)]
    supplies_delta_order: Vec<String>,
    health_delta: i32,
    wagon_delta: i32,
    animals_health_delta: i32,
    morale_delta: i32,
    time_cost: i32,
    distance_delta: i32,
    special_flags: Vec<String>,
}

// preservation: Phase 1a worldgen 200 seeds + 200 JSON event skeletons
#[test]
fn check_02_worldgen_200_seeds_and_event_skeletons() {
    let raw = include_bytes!("../../oracle/vectors/worldgen_200.json.gz");
    let wf: WorldgenFile = read_gzip_json(raw);
    assert_eq!(wf.schema, "etv-phase1a-worldgen-v1");
    assert_eq!(wf.metadata.python_version, "3.12.13");
    assert!(wf.metadata.oracle_commit.starts_with("47b01a3"));
    assert_eq!(wf.count, 200);
    assert_eq!(wf.vectors.len(), 200);

    for (idx, wv) in wf.vectors.iter().enumerate() {
        let seed: i128 = wv.seed.parse().expect("valid seed");
        let (mut state, mut rng) =
            create_new_run_with_rng(seed, etv_core::model::GMProfile::Fireside, 2);

        let parts: Vec<&str> = state.run_id.split('-').collect();
        assert_eq!(parts.len(), 2, "salted run_id must have two parts");
        assert_eq!(parts[0], wv.run_id_prefix);
        assert_eq!(parts[1].len(), 8);
        assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));

        assert_eq!(
            state.rng_counter, wv.rng_counter,
            "rng_counter mismatch on seed {seed}"
        );
        assert_eq!(
            rng.state_sha256_hex(),
            wv.rng_state_sha256,
            "rng_state_sha256 mismatch on seed {seed} (idx {idx})"
        );

        let mut got_weather = Vec::with_capacity(10);
        for day in [1, 25] {
            for biome in Biome::ALL {
                got_weather.push(generate_weather(&mut rng, biome, day).as_str().to_string());
            }
        }
        assert_eq!(wv.weather_picks.len(), 10);
        assert_eq!(got_weather.len(), 10);
        assert_eq!(
            got_weather, wv.weather_picks,
            "weather_picks mismatch on seed {seed}"
        );

        state.run_id = parts[0].to_string();
        let val = state_to_value(&state);
        let got_canonical = to_canonical_json_string(&val);
        assert_eq!(
            got_canonical, wv.canonical_json,
            "canonical RunState JSON mismatch on seed {seed} (idx {idx})"
        );
    }

    verify_worldgen_json_events(&wf);

    assert!(load_json_events_from_str("{not json").is_empty());
    assert!(load_json_events_from_str("{\"not\": \"a list\"}").is_empty());
}

fn verify_event_slice(rust_events: &[EventSkeleton], oracle_events: &[OracleJsonEvent]) {
    assert_eq!(rust_events.len(), oracle_events.len());
    for (re, oe) in rust_events.iter().zip(oracle_events.iter()) {
        assert_eq!(re.event_id, oe.event_id);
        assert_eq!(re.title, oe.title);
        assert_eq!(
            serde_json::to_value(re.category).unwrap().as_str().unwrap(),
            oe.category
        );
        assert_eq!(re.tags, oe.tags);
        assert_eq!(re.severity, oe.severity);
        let re_biomes: Option<Vec<String>> = re
            .biome_filter
            .as_ref()
            .map(|bf| bf.iter().map(|b| b.as_str().to_string()).collect());
        assert_eq!(re_biomes, oe.biome_filter);
        assert_eq!(re.costs_uncanny_token, oe.costs_uncanny_token);
        let re_ft: Option<String> = re.folklore_type.map(|ft| {
            serde_json::to_value(ft)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        });
        assert_eq!(re_ft, oe.folklore_type);
        assert_eq!(re.fallback_narration, oe.fallback_narration);
        assert_eq!(re.fallback_choices.len(), oe.choices.len());
        for (rc, oc) in re.fallback_choices.iter().zip(oe.choices.iter()) {
            assert_eq!(rc.choice_id, oc.choice_id);
            assert_eq!(rc.label, oc.label);
            assert_eq!(rc.action, oc.action);
            assert_eq!(rc.style, oc.style);
            if !oc.risk_hint.is_empty() || !rc.risk_hint.is_empty() {
                assert_eq!(rc.risk_hint, oc.risk_hint);
            }
            if !oc.cost_hint.is_empty() || !rc.cost_hint.is_empty() {
                assert_eq!(rc.cost_hint, oc.cost_hint);
            }
        }
        assert_eq!(re.outcome_templates.len(), oe.outcomes.len());
        for (cid, oo) in &oe.outcomes {
            let ro = re.outcome_templates.get(cid).expect("outcome key");
            assert_eq!(ro.supplies_delta, oo.supplies_delta);
            if !oo.supplies_delta_order.is_empty() {
                let got_order: Vec<String> = ro
                    .ordered_supplies_delta()
                    .into_iter()
                    .map(|(k, _)| k.to_string())
                    .collect();
                assert_eq!(
                    got_order, oo.supplies_delta_order,
                    "supplies_delta_order mismatch on {}:{}",
                    re.event_id, cid
                );
            }
            assert_eq!(ro.health_delta, oo.health_delta);
            assert_eq!(ro.wagon_delta, oo.wagon_delta);
            assert_eq!(ro.animals_health_delta, oo.animals_health_delta);
            assert_eq!(ro.morale_delta, oo.morale_delta);
            assert_eq!(ro.time_cost, oo.time_cost);
            assert_eq!(ro.distance_delta, oo.distance_delta);
            assert_eq!(ro.special_flags, oo.special_flags);
        }
    }
}

fn verify_worldgen_json_events(wf: &WorldgenFile) {
    let rust_events = load_json_events();
    assert_eq!(rust_events.len(), 200);
    assert_eq!(wf.json_events_count, 200);
    assert_eq!(wf.json_events.len(), 200);
    verify_event_slice(&rust_events, &wf.json_events);
}

// preservation: Phase 1a negative check for truncated json_events
#[test]
#[should_panic(expected = "assertion `left == right` failed")]
fn check_02_negative_truncated_json_events_must_fail() {
    let raw = include_bytes!("../../oracle/vectors/worldgen_200.json.gz");
    let mut wf: WorldgenFile = read_gzip_json(raw);
    wf.json_events.truncate(5);
    verify_worldgen_json_events(&wf);
}

// ── 3, 4, 5. Save Round Trip (20 saves), Migration (v1/v2 -> v3), RNG Restore ──

#[derive(Deserialize)]
struct SavesFile {
    schema: String,
    metadata: OracleMeta,
    saves_20: Vec<SaveVector>,
    migrations: Vec<MigrationVector>,
}

#[derive(Deserialize)]
struct SaveVector {
    label: String,
    raw_save_json: String,
    canonical_save_json: String,
    #[serde(default)]
    minted_snapshot: BTreeMap<String, i32>,
    next_100_draws_with_state: Vec<RngDrawRecord>,
    next_100_draws_counter_only: Vec<RngDrawRecord>,
}

#[derive(Deserialize)]
struct MigrationVector {
    label: String,
    input_json: String,
    canonical_migrated_json: String,
    #[serde(default)]
    minted_snapshot: BTreeMap<String, i32>,
    next_100_draws: Vec<RngDrawRecord>,
}

#[derive(Deserialize, Debug, PartialEq)]
struct RngDrawRecord {
    op: String,
    #[serde(default)]
    bits: Option<String>,
    #[serde(default)]
    val: Option<i128>,
    #[serde(default)]
    pick: Option<String>,
    #[serde(default)]
    samp: Option<Vec<i32>>,
    counter: u64,
}

fn run_100_draws(rng: &mut CpythonRng) -> Vec<RngDrawRecord> {
    let mut out = Vec::with_capacity(100);
    for i in 0..100 {
        match i % 5 {
            0 => {
                let v = rng.random();
                out.push(RngDrawRecord {
                    op: "random".to_string(),
                    bits: Some(v.to_bits().to_string()),
                    val: None,
                    pick: None,
                    samp: None,
                    counter: rng.counter,
                });
            }
            1 => {
                let v = rng.randint(-20, 120);
                out.push(RngDrawRecord {
                    op: "randint".to_string(),
                    bits: None,
                    val: Some(v),
                    pick: None,
                    samp: None,
                    counter: rng.counter,
                });
            }
            2 => {
                let seq = ["a", "b", "c", "d", "e", "f"];
                let pick = (*rng.choice(&seq)).to_string();
                out.push(RngDrawRecord {
                    op: "choice".to_string(),
                    bits: None,
                    val: None,
                    pick: Some(pick),
                    samp: None,
                    counter: rng.counter,
                });
            }
            3 => {
                let pop = [0, 1, 2, 3, 4, 5, 6, 7];
                let samp = rng.sample(&pop, 3);
                out.push(RngDrawRecord {
                    op: "sample".to_string(),
                    bits: None,
                    val: None,
                    pick: None,
                    samp: Some(samp),
                    counter: rng.counter,
                });
            }
            _ => {
                let items = ["clear", "rain", "storm", "fog"];
                let weights = [3.0, 1.5, 0.5, 1.0];
                let pick = (*rng.weighted_choice(&items, &weights)).to_string();
                out.push(RngDrawRecord {
                    op: "weighted".to_string(),
                    bits: None,
                    val: None,
                    pick: Some(pick),
                    samp: None,
                    counter: rng.counter,
                });
            }
        }
    }
    out
}

// preservation: Phase 1a save round-trip across 20 real Python saves
#[test]
fn check_03_save_round_trip_20_saves() {
    let raw = include_bytes!("../../oracle/vectors/saves_20.json.gz");
    let sf: SavesFile = read_gzip_json(raw);
    assert_eq!(sf.schema, "etv-phase1a-saves-v1");
    assert_eq!(sf.metadata.python_version, "3.12.13");
    assert!(sf.metadata.oracle_commit.starts_with("47b01a3"));
    assert_eq!(sf.saves_20.len(), 20);

    for sv in &sf.saves_20 {
        let tmp = tempfile::tempdir().expect("tempdir");
        let save_dir = tmp.path().join(SAVE_DIR);
        std::fs::create_dir_all(&save_dir).unwrap();
        let save_path = save_dir.join(SAVE_FILE);
        std::fs::write(&save_path, &sv.raw_save_json).unwrap();

        let res = load_game_result(tmp.path());
        assert!(res.ok, "failed to load {}: {}", sv.label, res.reason);
        let loaded_state = res.state.expect("loaded state");

        let direct_serde: RunState =
            serde_json::from_str(&sv.raw_save_json).expect("direct serde deserialize");
        assert_eq!(
            direct_serde, loaded_state,
            "direct serde mismatch on {}",
            sv.label
        );

        let tmp2 = tempfile::tempdir().expect("tempdir 2");
        let resaved_path = save_game(&loaded_state, tmp2.path()).expect("save_game");
        let resaved_text = std::fs::read_to_string(&resaved_path).unwrap();
        let resaved_val: Value = serde_json::from_str(&resaved_text).unwrap();
        let resaved_canonical = to_canonical_json_string(&resaved_val);

        assert_eq!(
            resaved_canonical, sv.canonical_save_json,
            "canonical JSON mismatch after round-trip on {}",
            sv.label
        );

        let mut reloaded = load_game(tmp2.path()).expect("reload own save");
        assert_eq!(
            reloaded, loaded_state,
            "reload state mismatch on {}",
            sv.label
        );
        assert!(
            reloaded.backpack.minted_initial.is_empty(),
            "minted_initial must be empty right after load on {}",
            sv.label
        );
        let got_snap = reloaded.backpack.minted_snapshot_of();
        assert_eq!(
            got_snap, sv.minted_snapshot,
            "minted_snapshot mismatch on {}",
            sv.label
        );
        assert_eq!(
            reloaded.backpack.minted_initial, sv.minted_snapshot,
            "minted_initial field after minted_snapshot_of mismatch on {}",
            sv.label
        );
    }
}

// preservation: Phase 1a schema migration v1/v2 -> v3 and seed overflow checks
#[test]
fn check_04_migration_v1_v2_to_v3_and_audit_l2() {
    let raw = include_bytes!("../../oracle/vectors/saves_20.json.gz");
    let sf: SavesFile = read_gzip_json(raw);
    assert_eq!(sf.migrations.len(), 5);

    for mv in &sf.migrations {
        let tmp = tempfile::tempdir().expect("tempdir");
        let save_dir = tmp.path().join(SAVE_DIR);
        std::fs::create_dir_all(&save_dir).unwrap();
        std::fs::write(save_dir.join(SAVE_FILE), &mv.input_json).unwrap();

        let res = load_game_result(tmp.path());
        assert!(
            res.ok,
            "migration load failed on {}: {}",
            mv.label, res.reason
        );
        let state = res.state.unwrap();

        assert!(
            state.backpack.minted_initial.is_empty(),
            "minted_initial must be empty right after load on {}",
            mv.label
        );

        let migrated_val = state_to_value(&state);
        let canonical = to_canonical_json_string(&migrated_val);
        assert_eq!(
            canonical, mv.canonical_migrated_json,
            "migrated v3 state mismatch on {}",
            mv.label
        );

        let mut bp_for_snap = state.backpack.clone();
        let got_snap = bp_for_snap.minted_snapshot_of();
        assert_eq!(
            got_snap, mv.minted_snapshot,
            "minted_snapshot mismatch on {}",
            mv.label
        );
        assert_eq!(
            bp_for_snap.minted_initial, mv.minted_snapshot,
            "minted_initial field after minted_snapshot_of mismatch on {}",
            mv.label
        );

        assert_eq!(mv.next_100_draws.len(), 100);
        let mut rng = CpythonRng::from_seed_and_counter(
            state.seed,
            state.rng_counter,
            state.rng_state.as_ref(),
        );
        let draws = run_100_draws(&mut rng);
        assert_eq!(draws.len(), 100);
        assert_eq!(
            draws, mv.next_100_draws,
            "RNG draws mismatch on migrated {}",
            mv.label
        );
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let mut run = create_new_run(42);
    run.backpack.enabled = true;
    run.backpack.last_settle_failed = true;
    let mut minted = BTreeMap::new();
    minted.insert("food".to_string(), 50);
    minted.insert("water".to_string(), 50);
    minted.insert("meds".to_string(), 5);
    minted.insert("ammo".to_string(), 20);
    minted.insert("parts".to_string(), 4);
    run.backpack.minted_initial = minted.clone();

    save_game(&run, tmp.path()).unwrap();
    let loaded = load_game(tmp.path()).expect("load L-2 save");
    assert!(loaded.backpack.last_settle_failed);
    assert_eq!(loaded.backpack.minted_initial, minted);

    for v in [0, 1, 2, 3] {
        let mut val = state_to_value(&run);
        val.as_object_mut()
            .unwrap()
            .insert("save_version".to_string(), json!(v));
        assert!(value_to_state(&val).is_ok(), "version {v} must load");
    }
    let tmp_incompat = tempfile::tempdir().expect("tempdir");
    let save_dir = tmp_incompat.path().join(SAVE_DIR);
    std::fs::create_dir_all(&save_dir).unwrap();
    let mut future_val = state_to_value(&run);
    future_val
        .as_object_mut()
        .unwrap()
        .insert("save_version".to_string(), json!(99));
    std::fs::write(
        save_dir.join(SAVE_FILE),
        serde_json::to_string_pretty(&future_val).unwrap(),
    )
    .unwrap();
    let incompat_res = load_game_result(tmp_incompat.path());
    assert!(!incompat_res.ok);
    assert_eq!(incompat_res.reason, "incompatible");
    assert!(save_dir.join(SAVE_FILE).exists());

    let tmp_big_seed = tempfile::tempdir().expect("tempdir big seed");
    let big_seed: i128 = (u64::MAX as i128) + 42;
    let run_big = create_new_run(big_seed);
    let mut val_big = state_to_value(&run_big);
    let mut raw_big = serde_json::to_string_pretty(&val_big).unwrap();
    raw_big = raw_big.replace(
        &format!("\"seed\": \"{}\"", big_seed),
        &format!("\"seed\": {}", big_seed),
    );
    let save_dir_big = tmp_big_seed.path().join(SAVE_DIR);
    std::fs::create_dir_all(&save_dir_big).unwrap();
    std::fs::write(save_dir_big.join(SAVE_FILE), &raw_big).unwrap();
    let res_big = load_game_result(tmp_big_seed.path());
    assert!(
        res_big.ok,
        "bare JSON seed > u64::MAX within i128 must load: {}",
        res_big.reason
    );
    assert_eq!(res_big.state.unwrap().seed, big_seed);

    let tmp_overflow = tempfile::tempdir().expect("tempdir overflow seed");
    let save_dir_ov = tmp_overflow.path().join(SAVE_DIR);
    std::fs::create_dir_all(&save_dir_ov).unwrap();
    val_big
        .as_object_mut()
        .unwrap()
        .insert("seed".to_string(), json!(0));
    let raw_ov = serde_json::to_string_pretty(&val_big).unwrap().replace(
        "\"seed\": 0",
        "\"seed\": 340282366920938463463374607431768211456",
    );
    std::fs::write(save_dir_ov.join(SAVE_FILE), &raw_ov).unwrap();
    let res_ov = load_game_result(tmp_overflow.path());
    assert!(!res_ov.ok);
    assert_eq!(res_ov.reason, "seed_out_of_range");
    assert!(
        save_dir_ov.join(SAVE_FILE).exists(),
        "seed_out_of_range save must not be quarantined as corrupt"
    );
}

// preservation: Phase 1a RNG restore across 20 saves and malformed rng_state fallback
#[test]
fn check_05_rng_restore_20_saves_plus_missing_and_malformed() {
    let raw = include_bytes!("../../oracle/vectors/saves_20.json.gz");
    let sf: SavesFile = read_gzip_json(raw);
    assert_eq!(sf.saves_20.len(), 20);

    for sv in &sf.saves_20 {
        let val: Value = serde_json::from_str(&sv.raw_save_json).unwrap();
        let state = value_to_state(&val).expect("parse save");

        assert_eq!(sv.next_100_draws_with_state.len(), 100);
        let mut rng_full = CpythonRng::from_seed_and_counter(
            state.seed,
            state.rng_counter,
            state.rng_state.as_ref(),
        );
        let draws_full = run_100_draws(&mut rng_full);
        assert_eq!(draws_full.len(), 100);
        assert_eq!(
            draws_full, sv.next_100_draws_with_state,
            "100 draws (with rng_state) mismatch on {}",
            sv.label
        );

        assert_eq!(sv.next_100_draws_counter_only.len(), 100);
        let mut rng_counter =
            CpythonRng::from_seed_and_counter(state.seed, state.rng_counter, None);
        let draws_counter = run_100_draws(&mut rng_counter);
        assert_eq!(draws_counter.len(), 100);
        assert_eq!(
            draws_counter, sv.next_100_draws_counter_only,
            "100 draws (counter-only) mismatch on {}",
            sv.label
        );

        for bad_json in [
            json!([1, 2, 3]),
            json!("nope"),
            json!({"version": 3}),
            json!([2, vec![0u32; 625], null]),
            json!([3, vec![0u32; 624], null]),
        ] {
            assert!(coerce_rng_state(&bad_json).is_none());
            let mut modified = val.clone();
            modified
                .as_object_mut()
                .unwrap()
                .insert("rng_state".to_string(), bad_json);
            let st_bad = value_to_state(&modified).expect("malformed rng_state must not fail load");
            assert!(st_bad.rng_state.is_none());
            let mut rng_fallback = CpythonRng::from_seed_and_counter(
                st_bad.seed,
                st_bad.rng_counter,
                st_bad.rng_state.as_ref(),
            );
            assert_eq!(
                run_100_draws(&mut rng_fallback),
                sv.next_100_draws_counter_only
            );
        }

        let invalid_struct = RngState(99, vec![1, 2, 3], None);
        let mut rng_invalid_direct =
            CpythonRng::from_seed_and_counter(state.seed, state.rng_counter, Some(&invalid_struct));
        assert_eq!(
            run_100_draws(&mut rng_invalid_direct),
            sv.next_100_draws_counter_only
        );
    }
}

// ── 6. Secrets Sidecar (Audit M-5) & Corrupt Save + Corrupt Secrets Quarantine ──

// Fails on 02532e17 because corrupt `secrets.json` was silently overwritten instead of
// being quarantined as `secrets.json.corrupt-<timestamp>`.
#[test]
fn check_06_secrets_sidecar_and_corrupt_quarantine() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base: &Path = tmp.path();

    let mut run_a = create_new_run(42);
    let mut run_b = create_new_run(42);
    assert_ne!(run_a.run_id, run_b.run_id);
    assert_eq!(
        run_a.run_id.split('-').next().unwrap(),
        run_b.run_id.split('-').next().unwrap()
    );

    run_a.backpack.enabled = true;
    run_a.backpack.wallet_address = "rWalletAddrA".to_string();
    run_a.backpack.wallet_secret = "sTestnetDummySecretA00000000".to_string();
    run_a.backpack.issuer_address = "rIssuerAddrA".to_string();
    run_a.backpack.issuer_secret = "sTestnetDummyIssuerA00000000".to_string();

    save_game(&run_a, base).expect("save run_a");
    assert!(has_save(base));

    let run_json_raw = std::fs::read_to_string(base.join(SAVE_DIR).join(SAVE_FILE)).unwrap();
    assert!(!run_json_raw.contains("sTestnetDummySecretA00000000"));
    assert!(!run_json_raw.contains("sTestnetDummyIssuerA00000000"));
    assert!(!run_json_raw.contains("wallet_secret"));
    assert!(!run_json_raw.contains("issuer_secret"));

    save_game(&run_b, base).expect("save run_b without secrets");
    let save_dir = base.join(SAVE_DIR);
    let sec_a =
        read_secrets_sidecar(&save_dir, &run_a.run_id).expect("run_a secrets still present");
    assert_eq!(sec_a.wallet_secret, "sTestnetDummySecretA00000000");
    assert_eq!(sec_a.issuer_secret, "sTestnetDummyIssuerA00000000");
    assert!(read_secrets_sidecar(&save_dir, &run_b.run_id).is_none());

    run_b.backpack.enabled = true;
    run_b.backpack.wallet_address = "rWalletAddrB".to_string();
    run_b.backpack.wallet_secret = "sTestnetDummySecretB11111111".to_string();
    run_b.backpack.issuer_address = "rIssuerAddrB".to_string();
    run_b.backpack.issuer_secret = "sTestnetDummyIssuerB11111111".to_string();
    save_game(&run_b, base).expect("save run_b with secrets");

    let sec_a_after = read_secrets_sidecar(&save_dir, &run_a.run_id).expect("run_a secrets");
    let sec_b_after = read_secrets_sidecar(&save_dir, &run_b.run_id).expect("run_b secrets");
    assert_eq!(sec_a_after.wallet_secret, "sTestnetDummySecretA00000000");
    assert_eq!(sec_b_after.wallet_secret, "sTestnetDummySecretB11111111");

    let loaded_b = load_game(base).expect("load run_b");
    assert_eq!(loaded_b.run_id, run_b.run_id);
    assert_eq!(
        loaded_b.backpack.wallet_secret,
        "sTestnetDummySecretB11111111"
    );
    assert_eq!(
        loaded_b.backpack.issuer_secret,
        "sTestnetDummyIssuerB11111111"
    );

    // Corrupt secrets.json -> quarantined as secrets.json.corrupt-<timestamp> before starting fresh
    let secrets_path = save_dir.join(SECRETS_FILE);
    std::fs::write(&secrets_path, "{corrupt secrets json!!!").unwrap();
    save_game(&run_b, base).expect("save run_b after corrupt secrets.json");
    let secrets_backups: Vec<_> = std::fs::read_dir(&save_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("secrets.json.corrupt-")
        })
        .collect();
    assert_eq!(secrets_backups.len(), 1);
    assert_eq!(
        std::fs::read_to_string(secrets_backups[0].path()).unwrap(),
        "{corrupt secrets json!!!"
    );
    let sec_b_fresh = read_secrets_sidecar(&save_dir, &run_b.run_id).expect("fresh run_b secrets");
    assert_eq!(sec_b_fresh.wallet_secret, "sTestnetDummySecretB11111111");

    // Corrupt run.json -> quarantined as run.json.corrupt-<timestamp>
    let save_path = save_dir.join(SAVE_FILE);
    std::fs::write(&save_path, "{corrupt json!!!").unwrap();
    let bad_res = load_game_result(base);
    assert!(!bad_res.ok);
    assert_eq!(bad_res.reason, "corrupt");
    assert!(
        !save_path.exists(),
        "corrupt run.json must be moved out of the way"
    );

    let corrupt_backups: Vec<_> = std::fs::read_dir(&save_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("run.json.corrupt-")
        })
        .collect();
    assert_eq!(corrupt_backups.len(), 1);
    assert_eq!(
        std::fs::read_to_string(corrupt_backups[0].path()).unwrap(),
        "{corrupt json!!!"
    );
}

// ── 7, 8, 9. Phase 1b Golden Harness: 1,000 Seeds × 60 Turns + 120 Save/Resume ──

#[derive(Deserialize)]
struct TurnsHarnessFile {
    schema: String,
    metadata: OracleMeta,
    seed_count: usize,
    max_turns_per_seed: usize,
    total_turns: usize,
    event_library_count: usize,
    event_library: Vec<OracleJsonEvent>,
    ending_vectors: Vec<EndingVector>,
    runs: Vec<SeedRunVector>,
}

#[derive(Deserialize)]
struct EndingVector {
    label: String,
    use_finalize: bool,
    input_state_json: String,
    expected_cause_of_death: String,
    expected_game_over: bool,
    expected_ending: EndingResult,
}

#[derive(Deserialize, Clone)]
struct SeedRunVector {
    seed: String,
    turns: Vec<TurnRecord>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
struct TurnRecord {
    t: usize,
    act: String,
    cid: String,
    pace: String,
    cnt: u64,
    rng_h: String,
    st_h: String,
    st_d: StateDigest,
    ev: EmittedEventsRecord,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
struct StateDigest {
    day: i32,
    time: String,
    loc: String,
    dest: String,
    rem: i32,
    dist: i32,
    wagon: i32,
    pace: String,
    morale: i32,
    hp: Vec<i32>,
    sup: BTreeMap<String, i32>,
    j_len: usize,
    mc_len: usize,
    over: bool,
    vic: bool,
    cod: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
struct EmittedEventsRecord {
    phase: String,
    lines: Vec<String>,
    event_id: String,
    event_title: String,
    event_choices: Vec<EmittedChoice>,
    route_options: Vec<EmittedRoute>,
    outcome_title: String,
    outcome_narration: String,
    outcome_deltas: BTreeMap<String, i32>,
    ending: Option<EndingResult>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
struct EmittedChoice {
    id: String,
    label: String,
    risk_hint: String,
    cost_hint: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
struct EmittedRoute {
    node_id: String,
    name: String,
    distance: i32,
}

#[derive(Deserialize)]
struct ResumeHarnessFile {
    schema: String,
    metadata: OracleMeta,
    count: usize,
    save_turn: usize,
    resume_runs: Vec<ResumeRunVector>,
}

#[derive(Deserialize)]
struct ResumeRunVector {
    seed: String,
    seed_index: usize,
    save_turn: usize,
    raw_save_json: String,
}

fn compute_state_hash_and_digest(state: &RunState) -> (String, StateDigest) {
    let mut val = state_to_value(state);
    if let Some(obj) = val.as_object_mut() {
        if let Some(rid) = obj.get("run_id").and_then(|v| v.as_str()) {
            if let Some((prefix, _)) = rid.split_once('-') {
                obj.insert("run_id".to_string(), json!(format!("{prefix}-00000000")));
            }
        }
    }
    let raw = serde_json::to_vec(&val).expect("serialize RunState JSON");
    let st_h = sha256_hex_bytes(&raw);

    let digest = StateDigest {
        day: state.day,
        time: serde_json::to_value(state.time_of_day)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string(),
        loc: state.location_id.clone(),
        dest: state.destination_id.clone(),
        rem: state.distance_remaining,
        dist: state.distance_traveled,
        wagon: state.wagon.condition,
        pace: state.wagon.pace.as_str().to_string(),
        morale: state.party.morale,
        hp: state.party.members.iter().map(|m| m.health).collect(),
        sup: state.supplies.to_dict(),
        j_len: state.journal.len(),
        mc_len: state.memory_cards.len(),
        over: state.game_over,
        vic: state.victory,
        cod: state.cause_of_death.clone(),
    };
    (st_h, digest)
}

fn capture_emitted_events(engine: &StepEngine, msgs: &StepMessages) -> EmittedEventsRecord {
    let phase_str = serde_json::to_value(engine.phase)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    EmittedEventsRecord {
        phase: phase_str,
        lines: msgs.lines.clone(),
        event_id: engine
            .pending_event()
            .map(|e| e.event_id.clone())
            .unwrap_or_default(),
        event_title: msgs.event_title.clone(),
        event_choices: msgs
            .event_choices
            .iter()
            .map(|c| EmittedChoice {
                id: c.id.clone(),
                label: c.label.clone(),
                risk_hint: c.risk_hint.clone(),
                cost_hint: c.cost_hint.clone(),
            })
            .collect(),
        route_options: msgs
            .route_options
            .iter()
            .map(|r| EmittedRoute {
                node_id: r.node_id.clone(),
                name: r.name.clone(),
                distance: r.distance,
            })
            .collect(),
        outcome_title: msgs.outcome_title.clone(),
        outcome_narration: msgs.outcome_narration.clone(),
        outcome_deltas: msgs.outcome_deltas.clone(),
        ending: msgs.ending.clone(),
    }
}

fn format_state_diff(
    exp_st_h: &str,
    got_st_h: &str,
    exp_d: &StateDigest,
    got_d: &StateDigest,
    exp_ev: &EmittedEventsRecord,
    got_ev: &EmittedEventsRecord,
) -> String {
    let mut diffs = Vec::new();
    diffs.push(format!(
        "expected_state_hash={exp_st_h} actual_state_hash={got_st_h}"
    ));
    if exp_d.day != got_d.day {
        diffs.push(format!("day: expected={} actual={}", exp_d.day, got_d.day));
    }
    if exp_d.time != got_d.time {
        diffs.push(format!(
            "time_of_day: expected={} actual={}",
            exp_d.time, got_d.time
        ));
    }
    if exp_d.loc != got_d.loc || exp_d.dest != got_d.dest {
        diffs.push(format!(
            "loc/dest: expected=({},{}) actual=({},{})",
            exp_d.loc, exp_d.dest, got_d.loc, got_d.dest
        ));
    }
    if exp_d.rem != got_d.rem || exp_d.dist != got_d.dist {
        diffs.push(format!(
            "rem/dist: expected=({},{}) actual=({},{})",
            exp_d.rem, exp_d.dist, got_d.rem, got_d.dist
        ));
    }
    if exp_d.wagon != got_d.wagon || exp_d.pace != got_d.pace {
        diffs.push(format!(
            "wagon/pace: expected=({},{}) actual=({},{})",
            exp_d.wagon, exp_d.pace, got_d.wagon, got_d.pace
        ));
    }
    if exp_d.morale != got_d.morale {
        diffs.push(format!(
            "morale: expected={} actual={}",
            exp_d.morale, got_d.morale
        ));
    }
    if exp_d.hp != got_d.hp {
        diffs.push(format!(
            "party_hp: expected={:?} actual={:?}",
            exp_d.hp, got_d.hp
        ));
    }
    if exp_d.sup != got_d.sup {
        diffs.push(format!(
            "supplies: expected={:?} actual={:?}",
            exp_d.sup, got_d.sup
        ));
    }
    if exp_d.j_len != got_d.j_len || exp_d.mc_len != got_d.mc_len {
        diffs.push(format!(
            "journal/memory_cards len: expected=({},{}) actual=({},{})",
            exp_d.j_len, exp_d.mc_len, got_d.j_len, got_d.mc_len
        ));
    }
    if exp_d.over != got_d.over || exp_d.vic != got_d.vic || exp_d.cod != got_d.cod {
        diffs.push(format!(
            "over/vic/cod: expected=({},{},{:?}) actual=({},{},{:?})",
            exp_d.over, exp_d.vic, exp_d.cod, got_d.over, got_d.vic, got_d.cod
        ));
    }
    if exp_ev != got_ev {
        diffs.push(format!(
            "events_emitted: expected={} actual={}",
            serde_json::to_string(exp_ev).unwrap(),
            serde_json::to_string(got_ev).unwrap()
        ));
    }
    diffs.join("; ")
}

/// Execute and verify a single turn step on `engine`.
/// On failure, stops immediately and formats the diagnostic in the required order:
/// `seed`, `turn`, `rng_hash`, then `state_diff`.
fn verify_turn_step(
    seed_str: &str,
    engine: &mut StepEngine,
    tr: &TurnRecord,
) -> Result<(), String> {
    let action: IntentAction =
        serde_json::from_value(json!(tr.act)).map_err(|e| format!("bad action {}: {e}", tr.act))?;
    let intent = PlayerIntent {
        action,
        choice_id: tr.cid.clone(),
        pace: tr.pace.clone(),
    };
    let msgs = engine.step(&intent);

    let got_cnt = engine.state.rng_counter;
    let got_rng_h = engine.rng.state_sha256_hex();
    let (got_st_h, got_st_d) = compute_state_hash_and_digest(&engine.state);
    let got_ev = capture_emitted_events(engine, &msgs);

    if got_cnt != tr.cnt
        || got_rng_h != tr.rng_h
        || got_st_h != tr.st_h
        || got_st_d != tr.st_d
        || got_ev != tr.ev
    {
        let state_diff =
            format_state_diff(&tr.st_h, &got_st_h, &tr.st_d, &got_st_d, &tr.ev, &got_ev);
        return Err(format!(
            "golden harness mismatch:\n  seed: {seed_str}\n  turn: {}\n  rng_hash: expected={} actual={} (counter: expected={} actual={})\n  state_diff: {state_diff}",
            tr.t, tr.rng_h, got_rng_h, tr.cnt, got_cnt
        ));
    }
    Ok(())
}

/// Replay and verify an entire seed run (`60` turns, or until game over).
/// Stops at the first mismatch and reports `seed`, `turn`, `rng_hash`, then `state_diff`.
fn verify_seed_run(run: &SeedRunVector) -> Result<usize, String> {
    if run.turns.is_empty() {
        return Err(format!(
            "golden harness mismatch:\n  seed: {}\n  turn: 0\n  rng_hash: none\n  state_diff: empty turn list",
            run.seed
        ));
    }
    let last_turn = run.turns.last().unwrap();
    if run.turns.len() != 60 && !(run.turns.len() < 60 && last_turn.ev.phase == "over") {
        return Err(format!(
            "golden harness mismatch:\n  seed: {}\n  turn: {}\n  rng_hash: {}\n  state_diff: expected 60 turns or game_over on final turn, got {} turns with final phase {:?}",
            run.seed,
            last_turn.t,
            last_turn.rng_h,
            run.turns.len(),
            last_turn.ev.phase
        ));
    }

    let seed: i128 = run
        .seed
        .parse()
        .map_err(|e| format!("invalid seed {}: {e}", run.seed))?;
    let state = create_new_run(seed);
    let mut engine = StepEngine::new(state);

    for (idx, tr) in run.turns.iter().enumerate() {
        if tr.t != idx + 1 {
            return Err(format!(
                "golden harness mismatch:\n  seed: {}\n  turn: {}\n  rng_hash: {}\n  state_diff: non-sequential turn index (expected {})",
                run.seed,
                tr.t,
                tr.rng_h,
                idx + 1
            ));
        }
        verify_turn_step(&run.seed, &mut engine, tr)?;
    }

    Ok(run.turns.len())
}

// Fails on 02532e17 because the Phase 1b turn engine (`StepEngine`, `physics`, `memory`, `events`)
// and `turns_1000_seeds.json.gz` did not exist on 02532e17.
#[test]
fn check_07_golden_harness_1000_seeds_60_turns_and_260_events() {
    let raw = include_bytes!("../../oracle/vectors/turns_1000_seeds.json.gz");
    let tf: TurnsHarnessFile = read_gzip_json(raw);
    assert_eq!(tf.schema, "etv-phase1b-turns-v1");
    assert_eq!(tf.metadata.python_version, "3.12.13");
    assert!(tf.metadata.oracle_commit.starts_with("47b01a3"));

    // Non-vacuous length assertions
    assert_eq!(tf.seed_count, 1000);
    assert_eq!(tf.max_turns_per_seed, 60);
    assert_eq!(tf.runs.len(), 1000);
    assert_eq!(tf.event_library_count, 260);
    assert_eq!(tf.event_library.len(), 260);
    assert_eq!(tf.ending_vectors.len(), 8);

    // 1. Verify full 260-event library (`60` hand-authored + `200` JSON)
    let rust_lib = build_event_library();
    assert_eq!(rust_lib.len(), 260);
    verify_event_slice(&rust_lib, &tf.event_library);

    // 2. Verify all 8 ending and `finalize_run` vectors
    for ev in &tf.ending_vectors {
        let val: Value = serde_json::from_str(&ev.input_state_json).unwrap();
        let st = value_to_state(&val).expect("parse ending vector state");
        let (got_ending, got_cod, got_over) = if ev.use_finalize {
            let mut eng = StepEngine::new(st);
            let end = eng.finalize_run("stalled");
            (end, eng.state.cause_of_death.clone(), eng.state.game_over)
        } else {
            let end = compute_ending(&st);
            (end, st.cause_of_death.clone(), st.game_over)
        };
        assert_eq!(
            got_cod, ev.expected_cause_of_death,
            "cause_of_death mismatch on {}",
            ev.label
        );
        assert_eq!(
            got_over, ev.expected_game_over,
            "game_over mismatch on {}",
            ev.label
        );
        assert_eq!(
            got_ending, ev.expected_ending,
            "ending mismatch on {}",
            ev.label
        );
    }

    // 3. Verify required special seeds and coverage across the 1,000 seeds
    let mut seeds_seen = HashSet::with_capacity(1000);
    let mut has_seed_0 = false;
    let mut has_seed_1 = false;
    let mut has_seed_42 = false;
    let mut has_negative_seed = false;
    let mut has_seed_above_u64_max = false;
    let mut actions_seen = HashSet::new();
    let mut phases_seen = HashSet::new();
    let mut full_60_runs = 0usize;
    let mut game_over_runs = 0usize;

    for run in &tf.runs {
        assert!(
            seeds_seen.insert(run.seed.clone()),
            "duplicate seed {}",
            run.seed
        );
        let seed_val: i128 = run.seed.parse().expect("valid i128 seed");
        if seed_val == 0 {
            has_seed_0 = true;
        }
        if seed_val == 1 {
            has_seed_1 = true;
        }
        if seed_val == 42 {
            has_seed_42 = true;
        }
        if seed_val < 0 {
            has_negative_seed = true;
        }
        if seed_val > (u64::MAX as i128) {
            has_seed_above_u64_max = true;
        }
        if run.turns.len() == 60 {
            full_60_runs += 1;
        } else {
            assert!(run.turns.len() < 60 && !run.turns.is_empty());
            assert_eq!(run.turns.last().unwrap().ev.phase, "over");
            game_over_runs += 1;
        }
        for tr in &run.turns {
            actions_seen.insert(tr.act.clone());
            phases_seen.insert(tr.ev.phase.clone());
        }
    }
    assert_eq!(seeds_seen.len(), 1000);
    assert!(has_seed_0, "must include seed 0");
    assert!(has_seed_1, "must include seed 1");
    assert!(has_seed_42, "must include seed 42");
    assert!(has_negative_seed, "must include a negative seed");
    assert!(has_seed_above_u64_max, "must include a seed > u64::MAX");
    assert!(full_60_runs > 900);
    assert!(game_over_runs >= 50);
    for expected_action in [
        "TRAVEL",
        "REST",
        "HUNT",
        "REPAIR",
        "CHANGE_PACE",
        "ABANDON_CARGO",
        "DESPERATE_REPAIR",
        "HARD_RATION",
        "CHOOSE",
    ] {
        assert!(
            actions_seen.contains(expected_action),
            "policy must exercise {expected_action}"
        );
    }
    for expected_phase in ["camp", "event", "route", "over"] {
        assert!(
            phases_seen.contains(expected_phase),
            "policy must reach phase {expected_phase}"
        );
    }

    // 4. Replay all 1,000 seeds in parallel worker chunks, checking results in deterministic
    // seed-index order (0..1000) so the first mismatch is always reported first.
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 16))
        .unwrap_or(4);
    let chunk_size = tf.runs.len().div_ceil(worker_count);

    let chunk_results: Vec<Vec<Result<usize, String>>> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for chunk in tf.runs.chunks(chunk_size) {
            handles.push(s.spawn(move || {
                let mut out = Vec::with_capacity(chunk.len());
                for run in chunk {
                    out.push(verify_seed_run(run));
                }
                out
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut verified_turns = 0usize;
    for res in chunk_results.into_iter().flatten() {
        match res {
            Ok(n) => verified_turns += n,
            Err(diag) => panic!("{diag}"),
        }
    }
    assert_eq!(verified_turns, tf.total_turns);
    assert_eq!(verified_turns, 59653);
}

// Fails on 02532e17 because `StepEngine` and `resume_120_seeds.json.gz` did not exist on 02532e17.
#[test]
fn check_08_golden_harness_120_save_and_resume_runs() {
    let raw_turns = include_bytes!("../../oracle/vectors/turns_1000_seeds.json.gz");
    let tf: TurnsHarnessFile = read_gzip_json(raw_turns);
    let raw_resume = include_bytes!("../../oracle/vectors/resume_120_seeds.json.gz");
    let rf: ResumeHarnessFile = read_gzip_json(raw_resume);

    assert_eq!(rf.schema, "etv-phase1b-resume-v1");
    assert_eq!(rf.metadata.python_version, "3.12.13");
    assert!(rf.metadata.oracle_commit.starts_with("47b01a3"));
    assert_eq!(rf.count, 120);
    assert_eq!(rf.save_turn, 20);
    assert_eq!(rf.resume_runs.len(), 120);

    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 16))
        .unwrap_or(4);
    let chunk_size = rf.resume_runs.len().div_ceil(worker_count);
    let all_runs = &tf.runs;

    let chunk_results: Vec<Vec<Result<usize, String>>> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for chunk in rf.resume_runs.chunks(chunk_size) {
            handles.push(s.spawn(move || {
                let mut out = Vec::with_capacity(chunk.len());
                for rr in chunk {
                    let uninterrupted = &all_runs[rr.seed_index];
                    assert_eq!(uninterrupted.seed, rr.seed);
                    assert_eq!(rr.save_turn, 20);
                    assert!(uninterrupted.turns.len() > rr.save_turn);

                    // Path A: Load Python's turn-20 save in Rust and continue turns 21..=60
                    let tmp_py = tempfile::tempdir().expect("tempdir py save");
                    let save_dir_py = tmp_py.path().join(SAVE_DIR);
                    std::fs::create_dir_all(&save_dir_py).unwrap();
                    std::fs::write(save_dir_py.join(SAVE_FILE), &rr.raw_save_json).unwrap();

                    let loaded_py = load_game(tmp_py.path()).expect("load Python turn-20 save");
                    let mut eng_from_py = StepEngine::new(loaded_py);

                    let mut count_turns = 0usize;
                    let mut err_opt: Option<String> = None;
                    for tr in &uninterrupted.turns[rr.save_turn..] {
                        if let Err(diag) = verify_turn_step(&rr.seed, &mut eng_from_py, tr) {
                            err_opt = Some(format!("[resume-from-python-save] {diag}"));
                            break;
                        }
                        count_turns += 1;
                    }
                    if let Some(e) = err_opt {
                        out.push(Err(e));
                        continue;
                    }

                    // Path B: Run turns 1..=20 in Rust, save at turn 20, load in Rust, and continue turns 21..=60
                    let seed_val: i128 = rr.seed.parse().unwrap();
                    let mut eng_rust = StepEngine::new(create_new_run(seed_val));
                    for tr in &uninterrupted.turns[..rr.save_turn] {
                        if let Err(diag) = verify_turn_step(&rr.seed, &mut eng_rust, tr) {
                            err_opt = Some(format!("[pre-save-rust] {diag}"));
                            break;
                        }
                    }
                    if let Some(e) = err_opt {
                        out.push(Err(e));
                        continue;
                    }

                    let tmp_rs = tempfile::tempdir().expect("tempdir rust save");
                    save_game(&eng_rust.state, tmp_rs.path()).expect("save at turn 20 in Rust");
                    let loaded_rs = load_game(tmp_rs.path()).expect("load Rust turn-20 save");
                    let mut eng_from_rs = StepEngine::new(loaded_rs);

                    for tr in &uninterrupted.turns[rr.save_turn..] {
                        if let Err(diag) = verify_turn_step(&rr.seed, &mut eng_from_rs, tr) {
                            err_opt = Some(format!("[resume-from-rust-save] {diag}"));
                            break;
                        }
                    }
                    if let Some(e) = err_opt {
                        out.push(Err(e));
                    } else {
                        out.push(Ok(count_turns));
                    }
                }
                out
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut resumed_turns_verified = 0usize;
    for res in chunk_results.into_iter().flatten() {
        match res {
            Ok(n) => resumed_turns_verified += n,
            Err(diag) => panic!("{diag}"),
        }
    }
    assert!(resumed_turns_verified >= 120 * 30);
}

// Fails on 02532e17 because `verify_seed_run` and the Phase 1b golden harness did not exist on 02532e17.
#[test]
fn check_09_negative_corrupted_hash_and_truncated_lists_must_fail() {
    let raw = include_bytes!("../../oracle/vectors/turns_1000_seeds.json.gz");
    let tf: TurnsHarnessFile = read_gzip_json(raw);

    let seed42_run = tf
        .runs
        .iter()
        .find(|r| r.seed == "42")
        .expect("seed 42 run present")
        .clone();

    // 1. Corrupt `st_h` on turn 7 AND turn 12 of seed 42; check must stop at the FIRST mismatch (turn 7)
    // and report `seed`, `turn`, `rng_hash`, then `state_diff` in that exact order.
    let mut corrupted_state = seed42_run.clone();
    corrupted_state.turns[6].st_h =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    corrupted_state.turns[11].st_h =
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();

    let err_state =
        verify_seed_run(&corrupted_state).expect_err("corrupted state_hash must fail check");
    let pos_seed = err_state.find("seed: 42").expect("reports seed: 42");
    let pos_turn = err_state
        .find("turn: 7")
        .expect("stops at first mismatch turn: 7");
    let pos_rng = err_state.find("rng_hash:").expect("reports rng_hash:");
    let pos_diff = err_state.find("state_diff:").expect("reports state_diff:");
    assert!(pos_seed < pos_turn && pos_turn < pos_rng && pos_rng < pos_diff);
    assert!(!err_state.contains("turn: 12"));

    // 2. Corrupt `rng_h` on turn 3 of seed 0; check must stop at turn 3 and report in order.
    let mut corrupted_rng = tf.runs[0].clone();
    assert_eq!(corrupted_rng.seed, "0");
    corrupted_rng.turns[2].rng_h =
        "1111111111111111111111111111111111111111111111111111111111111111".to_string();
    let err_rng = verify_seed_run(&corrupted_rng).expect_err("corrupted rng_hash must fail check");
    let r_seed = err_rng.find("seed: 0").expect("reports seed: 0");
    let r_turn = err_rng.find("turn: 3").expect("stops at turn: 3");
    let r_rng = err_rng.find("rng_hash:").expect("reports rng_hash:");
    let r_diff = err_rng.find("state_diff:").expect("reports state_diff:");
    assert!(r_seed < r_turn && r_turn < r_rng && r_rng < r_diff);

    // 3. Truncate a 60-turn non-game-over run to 10 turns; check must reject the truncated turn list.
    let mut truncated_turns = seed42_run;
    truncated_turns.turns.truncate(10);
    assert_ne!(truncated_turns.turns.last().unwrap().ev.phase, "over");
    let err_trunc =
        verify_seed_run(&truncated_turns).expect_err("truncated turn list must fail check");
    assert!(err_trunc.contains("expected 60 turns or game_over on final turn"));
}
