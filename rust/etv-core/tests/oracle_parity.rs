//! Oracle parity test suite for Escape the Valley 2.0 Phase 1a (`etv-core`).
//!
//! Verifies all five Phase 1a oracle checks against CPython 3.12.13 / v1.3.1 (47b01a3):
//! 1. S1 RNG call-shape vectors (2,645 assertions + `pool_n1000_k20` label check)
//! 2. Worldgen (200 seeds: byte-for-byte canonical RunState JSON + RNG state SHA-256 + weather + 200 JSON events)
//! 3. Save round trip (20 real Python saves: load, re-save, canonical JSON match, reload equality)
//! 4. Migration (v1 and v2 saves migrate to identical v3 state + audit L-2 `last_settle_failed`/`minted_initial`)
//! 5. RNG restore (20 saves × 100 draws with `rng_state`, 20 saves × 100 draws with `None` counter-replay, and malformed `rng_state` fallback)
//! 6. Secrets sidecar (same-seed runs keep separate secrets, saving a run without secrets preserves other runs' entries, atomic write & permissions, corrupt save quarantine)

use etv_core::model::{Biome, RunState};
use etv_core::rng::{coerce_rng_state, CpythonRng, RngState};
use etv_core::save::{
    has_save, load_game, load_game_result, read_secrets_sidecar, save_game, state_to_value,
    value_to_state, SAVE_DIR, SAVE_FILE, SECRETS_FILE,
};
use etv_core::worldgen::{
    create_new_run, create_new_run_with_rng, generate_weather, load_json_events,
    load_json_events_from_str,
};
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

fn read_gzip_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> T {
    let mut decoder = GzDecoder::new(bytes);
    let mut s = String::new();
    decoder.read_to_string(&mut s).expect("valid gzip stream");
    serde_json::from_str(&s).expect("valid JSON in gzip vector")
}

/// Serialize a `serde_json::Value` with recursively sorted object keys and compact separators
/// `( ",", ":" )` to match Python `json.dumps(obj, sort_keys=True, separators=(",", ":"))`.
fn to_canonical_json_string(val: &Value) -> String {
    let sorted = sort_json_keys(val);
    serde_json::to_string(&sorted).expect("serialize canonical JSON")
}

fn sort_json_keys(val: &Value) -> Value {
    match val {
        Value::Object(map) => {
            let mut sorted_keys: Vec<(&String, &Value)> = map.iter().collect();
            sorted_keys.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in sorted_keys {
                out.insert(k.clone(), sort_json_keys(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_json_keys).collect()),
        other => other.clone(),
    }
}

#[derive(Deserialize)]
struct OracleMeta {
    python_version: String,
    oracle_commit: String,
}

// ── 1. S1 RNG Vectors ───────────────────────────────────────────────────────

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

#[test]
fn check_01_s1_rng_vectors_and_pool_label() {
    let raw = include_bytes!("../../oracle/vectors/rng_s1_vectors.json.gz");
    let vf: S1VectorsFile = read_gzip_json(raw);
    assert_eq!(vf.schema, "etv-s1-rng-v2");
    assert_eq!(vf.metadata.python_version, "3.12.13");
    assert!(vf.metadata.oracle_commit.starts_with("47b01a3"));

    let mut total_checks = 0usize;
    let mut saw_pool_n1000_k20 = false;

    for sv in &vf.seed_vectors {
        let seed: i128 = sv.seed.parse().expect("valid i128 seed");
        let mut rng = CpythonRng::from_seed(seed);

        assert_eq!(rng.getstate(), sv.initial_state);
        total_checks += 1;

        for rd in &sv.random_draws {
            let got_bits = rng.random().to_bits();
            let exp_bits: u64 = rd.bits.parse().unwrap();
            assert_eq!(got_bits, exp_bits);
            total_checks += 1;
        }

        for gd in &sv.getrandbits_draws {
            let got = rng.getrandbits(gd.k);
            let exp: u128 = gd.value.parse().unwrap();
            assert_eq!(got, exp);
            total_checks += 1;
        }

        for rd in &sv.randint_draws {
            let a: i128 = rd.a.parse().unwrap();
            let b: i128 = rd.b.parse().unwrap();
            for exp_str in &rd.values {
                let got = rng.randint(a, b);
                let exp: i128 = exp_str.parse().unwrap();
                assert_eq!(got, exp);
                total_checks += 1;
            }
        }

        for cd in &sv.choice_draws {
            let seq: Vec<String> = (0..cd.len).map(|i| format!("item_{i}")).collect();
            for exp in &cd.picks {
                let got = rng.choice(&seq);
                assert_eq!(got, exp);
                total_checks += 1;
            }
        }

        for sd in &sv.shuffle_draws {
            let mut seq: Vec<usize> = (0..sd.len).collect();
            let prev_counter = rng.counter;
            rng.shuffle(&mut seq);
            assert_eq!(rng.counter, prev_counter + (sd.len as u64));
            assert_eq!(seq, sd.result);
            total_checks += 1;
        }

        for sm in &sv.sample_draws {
            assert_ne!(
                sm.name, "set_n1000_k20",
                "set_n1000_k20 must be renamed to pool_n1000_k20"
            );
            if sm.name == "pool_n1000_k20" {
                saw_pool_n1000_k20 = true;
            }
            let pop: Vec<usize> = (0..sm.n).collect();
            let prev_counter = rng.counter;
            let got = rng.sample(&pop, sm.k);
            assert_eq!(rng.counter, prev_counter + (sm.k as u64));
            assert_eq!(got, sm.result, "mismatch on {}", sm.name);
            total_checks += 1;
        }

        for wd in &sv.weighted_draws {
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

    let srv = &vf.save_restore_vector;
    let mut restored = CpythonRng::from_seed(0);
    restored.setstate(&srv.saved_rng_state).unwrap();
    assert_eq!(restored.getstate(), srv.saved_rng_state);
    total_checks += 1;

    for exp_bits_str in &srv.post_random_bits {
        let exp_bits: u64 = exp_bits_str.parse().unwrap();
        assert_eq!(restored.random().to_bits(), exp_bits);
        total_checks += 1;
    }
    for exp_val_str in &srv.post_randint {
        let exp_val: i128 = exp_val_str.parse().unwrap();
        assert_eq!(restored.randint(-50, 500), exp_val);
        total_checks += 1;
    }
    let mut shuf: Vec<usize> = (0..10).collect();
    restored.shuffle(&mut shuf);
    assert_eq!(shuf, srv.post_shuffle);
    total_checks += 1;

    let pop15: Vec<usize> = (0..15).collect();
    assert_eq!(restored.sample(&pop15, 4), srv.post_sample_pool);
    total_checks += 1;

    let pop100: Vec<usize> = (0..100).collect();
    assert_eq!(restored.sample(&pop100, 5), srv.post_sample_set);
    total_checks += 1;

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

    assert_eq!(total_checks, 2645);
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
}

#[derive(Deserialize)]
struct OracleOutcome {
    supplies_delta: BTreeMap<String, i32>,
    health_delta: i32,
    wagon_delta: i32,
    animals_health_delta: i32,
    morale_delta: i32,
    time_cost: i32,
    distance_delta: i32,
    special_flags: Vec<String>,
}

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

        // Verify run_id shape: <8-hex prefix>-<8-hex random>
        let parts: Vec<&str> = state.run_id.split('-').collect();
        assert_eq!(parts.len(), 2, "salted run_id must have two parts");
        assert_eq!(parts[0], wv.run_id_prefix);
        assert_eq!(parts[1].len(), 8);
        assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));

        // Verify RNG counter and post-worldgen RNG state SHA-256 hash
        assert_eq!(
            state.rng_counter, wv.rng_counter,
            "rng_counter mismatch on seed {seed}"
        );
        assert_eq!(
            rng.state_sha256_hex(),
            wv.rng_state_sha256,
            "rng_state_sha256 mismatch on seed {seed} (idx {idx})"
        );

        // Verify weather generation across days and biomes
        let mut got_weather = Vec::with_capacity(10);
        for day in [1, 25] {
            for biome in Biome::ALL {
                got_weather.push(generate_weather(&mut rng, biome, day).as_str().to_string());
            }
        }
        assert_eq!(
            got_weather, wv.weather_picks,
            "weather_picks mismatch on seed {seed}"
        );

        // Normalize run_id to prefix and compare canonical JSON byte for byte
        state.run_id = parts[0].to_string();
        let val = state_to_value(&state);
        let got_canonical = to_canonical_json_string(&val);
        assert_eq!(
            got_canonical, wv.canonical_json,
            "canonical RunState JSON mismatch on seed {seed} (idx {idx})"
        );
    }

    // Verify compiled-in event_skeletons.json loader matches all 200 Python events
    let rust_events = load_json_events();
    assert_eq!(rust_events.len(), 200);
    assert_eq!(wf.json_events_count, 200);
    for (re, oe) in rust_events.iter().zip(wf.json_events.iter()) {
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
        }
        assert_eq!(re.outcome_templates.len(), oe.outcomes.len());
        for (cid, oo) in &oe.outcomes {
            let ro = re.outcome_templates.get(cid).expect("outcome key");
            assert_eq!(ro.supplies_delta, oo.supplies_delta);
            assert_eq!(ro.health_delta, oo.health_delta);
            assert_eq!(ro.wagon_delta, oo.wagon_delta);
            assert_eq!(ro.animals_health_delta, oo.animals_health_delta);
            assert_eq!(ro.morale_delta, oo.morale_delta);
            assert_eq!(ro.time_cost, oo.time_cost);
            assert_eq!(ro.distance_delta, oo.distance_delta);
            assert_eq!(ro.special_flags, oo.special_flags);
        }
    }

    // Also check graceful degradation on corrupt/bad event JSON
    assert!(load_json_events_from_str("{not json").is_empty());
    assert!(load_json_events_from_str("{\"not\": \"a list\"}").is_empty());
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
    next_100_draws_with_state: Vec<RngDrawRecord>,
    next_100_draws_counter_only: Vec<RngDrawRecord>,
}

#[derive(Deserialize)]
struct MigrationVector {
    label: String,
    input_json: String,
    canonical_migrated_json: String,
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

        // 1. Rust loads Python's save
        let res = load_game_result(tmp.path());
        assert!(res.ok, "failed to load {}: {}", sv.label, res.reason);
        let loaded_state = res.state.expect("loaded state");

        // Also test direct serde_json deserialization of RunState (without rename table)
        let direct_serde: RunState =
            serde_json::from_str(&sv.raw_save_json).expect("direct serde deserialize");
        assert_eq!(
            direct_serde, loaded_state,
            "direct serde mismatch on {}",
            sv.label
        );

        // 2. Rust re-saves it to a fresh directory
        let tmp2 = tempfile::tempdir().expect("tempdir 2");
        let resaved_path = save_game(&loaded_state, tmp2.path()).expect("save_game");
        let resaved_text = std::fs::read_to_string(&resaved_path).unwrap();
        let resaved_val: Value = serde_json::from_str(&resaved_text).unwrap();
        let resaved_canonical = to_canonical_json_string(&resaved_val);

        // Result is semantically equal to the original (same JSON after sorting keys)
        assert_eq!(
            resaved_canonical, sv.canonical_save_json,
            "canonical JSON mismatch after round-trip on {}",
            sv.label
        );

        // 3. Rust reads its own save back and gets the exact same state
        let reloaded = load_game(tmp2.path()).expect("reload own save");
        assert_eq!(
            reloaded, loaded_state,
            "reload state mismatch on {}",
            sv.label
        );
    }
}

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

        let migrated_val = state_to_value(&state);
        let canonical = to_canonical_json_string(&migrated_val);
        assert_eq!(
            canonical, mv.canonical_migrated_json,
            "migrated v3 state mismatch on {}",
            mv.label
        );

        // Also verify RNG restore on the migrated save
        let mut rng = CpythonRng::from_seed_and_counter(
            state.seed,
            state.rng_counter,
            state.rng_state.as_ref(),
        );
        let draws = run_100_draws(&mut rng);
        assert_eq!(
            draws, mv.next_100_draws,
            "RNG draws mismatch on migrated {}",
            mv.label
        );
    }

    // Verify Audit L-2: `last_settle_failed` and `minted_initial` persist directly across save/load
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

    // Also verify schema version handling: versions 0, 1, 2, 3 load; version 4+ returns "incompatible"
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
    // Incompatible save is left in place, never quarantined as corrupt
    assert!(save_dir.join(SAVE_FILE).exists());
}

#[test]
fn check_05_rng_restore_20_saves_plus_missing_and_malformed() {
    let raw = include_bytes!("../../oracle/vectors/saves_20.json.gz");
    let sf: SavesFile = read_gzip_json(raw);
    assert_eq!(sf.saves_20.len(), 20);

    for sv in &sf.saves_20 {
        let val: Value = serde_json::from_str(&sv.raw_save_json).unwrap();
        let state = value_to_state(&val).expect("parse save");

        // 1. With full rng_state: next 100 draws match Python
        let mut rng_full = CpythonRng::from_seed_and_counter(
            state.seed,
            state.rng_counter,
            state.rng_state.as_ref(),
        );
        let draws_full = run_100_draws(&mut rng_full);
        assert_eq!(
            draws_full, sv.next_100_draws_with_state,
            "100 draws (with rng_state) mismatch on {}",
            sv.label
        );

        // 2. With no rng_state (None): counter-replay gives the exact same next 100 draws as Python
        let mut rng_counter =
            CpythonRng::from_seed_and_counter(state.seed, state.rng_counter, None);
        let draws_counter = run_100_draws(&mut rng_counter);
        assert_eq!(
            draws_counter, sv.next_100_draws_counter_only,
            "100 draws (counter-only) mismatch on {}",
            sv.label
        );

        // 3. With malformed rng_states in JSON ([1, 2, 3], "nope", {"bad": 1}, wrong version, bad index):
        // coerce_rng_state returns None and load falls back to counter-replay
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

        // Also test passing an invalid RngState struct directly into from_seed_and_counter
        let invalid_struct = RngState(99, vec![1, 2, 3], None);
        let mut rng_invalid_direct =
            CpythonRng::from_seed_and_counter(state.seed, state.rng_counter, Some(&invalid_struct));
        assert_eq!(
            run_100_draws(&mut rng_invalid_direct),
            sv.next_100_draws_counter_only
        );
    }
}

// ── 6. Secrets Sidecar (Audit M-5) & Corrupt Save Quarantine ────────────────

#[test]
fn check_06_secrets_sidecar_and_corrupt_quarantine() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base: &Path = tmp.path();

    // Two runs with the same seed (42) get distinct salted run_ids
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

    // Verify secrets never appear in run.json
    let run_json_raw = std::fs::read_to_string(base.join(SAVE_DIR).join(SAVE_FILE)).unwrap();
    assert!(!run_json_raw.contains("sTestnetDummySecretA00000000"));
    assert!(!run_json_raw.contains("sTestnetDummyIssuerA00000000"));
    assert!(!run_json_raw.contains("wallet_secret"));
    assert!(!run_json_raw.contains("issuer_secret"));

    // Saving run_b WITHOUT secrets must NOT drop run_a's secrets entry (audit M-5)
    save_game(&run_b, base).expect("save run_b without secrets");
    let save_dir = base.join(SAVE_DIR);
    let sec_a =
        read_secrets_sidecar(&save_dir, &run_a.run_id).expect("run_a secrets still present");
    assert_eq!(sec_a.wallet_secret, "sTestnetDummySecretA00000000");
    assert_eq!(sec_a.issuer_secret, "sTestnetDummyIssuerA00000000");
    assert!(read_secrets_sidecar(&save_dir, &run_b.run_id).is_none());

    // Now enable backpack on run_b and save; both run_a and run_b keep separate entries in secrets.json
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

    // Loading restores the active run's secrets into BackpackState
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

    // Verify no leftover .tmp files exist in .trail/
    let tmp_files: Vec<_> = std::fs::read_dir(&save_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(tmp_files.is_empty());
    assert!(save_dir.join(SECRETS_FILE).exists());

    // Verify corrupt-save quarantine (`run.json.corrupt-<timestamp>`)
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
