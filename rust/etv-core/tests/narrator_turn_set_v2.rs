//! Narrator turn set v2: turns drawn mid-run, each stored with the whole narrator context the
//! game built for it. Version 1 stays frozen beside it and is not touched.
//!
//! The committed file is frozen by its SHA-256. If the generator, the engine or the content
//! changes what it would produce, the reproduce test fails on purpose. A new set is a new file
//! with a new name and digest, never an edit of this one.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::content::embedded_content_pack;
use etv_core::measure::{
    parse_any_turn_set, parse_turn_set_v2, run_cli, run_measurement_any, sha256_hex, AnyTurnSet,
    MeasureConfig, TURN_SET_SCHEMA_V2,
};
use etv_core::model::{GMProfile, GamePhase, TimeOfDay};
use etv_core::narrator::{
    build_outcome_prompts, build_scene_prompts, NarratorLength, NarratorTransport, TransportError,
};
use etv_core::ruleset::Ruleset;
use etv_core::turn_set::{
    generate_turn_set_v2, turn_set_v2_to_json, TURN_SET_RULESET, TURN_SET_V2_PER_LEVEL,
    TURN_SET_V2_QUOTAS,
};

const V2_SHA256: &str = "b446c8be27f036cf1447c9ec1f7706f872b957eb159846274bd315e9a5cf87bf";
const V2_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v2.json");
const V1_SHA256: &str = "838e4346263507a112b6a161a4107a3647c253a39646b2427236afdd4e526f17";
const V1_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v1.json");

#[test]
fn both_files_match_their_frozen_digests_and_the_harness_reads_either() {
    assert!(!V2_BYTES.contains(&b'\r'));
    assert_eq!(sha256_hex(V2_BYTES), V2_SHA256);
    // Version 1 is untouched.
    assert_eq!(sha256_hex(V1_BYTES), V1_SHA256);

    let v2 = parse_any_turn_set(V2_BYTES, V2_SHA256).unwrap();
    assert!(matches!(v2, AnyTurnSet::V2(_)));
    assert_eq!(v2.len(), 360);
    assert_eq!(v2.schema(), TURN_SET_SCHEMA_V2);
    let v1 = parse_any_turn_set(V1_BYTES, V1_SHA256).unwrap();
    assert!(matches!(v1, AnyTurnSet::V1(_)));
    assert_eq!(v1.len(), 240);

    // Each file is refused under the other's digest, and under a wrong one.
    assert!(parse_any_turn_set(V2_BYTES, V1_SHA256).is_err());
    assert!(parse_any_turn_set(V1_BYTES, V2_SHA256).is_err());
    assert!(parse_turn_set_v2(V2_BYTES, &"0".repeat(64)).is_err());
    // A version 1 file is not a version 2 file.
    assert!(parse_turn_set_v2(V1_BYTES, V1_SHA256).is_err());
}

#[test]
fn generator_reproduces_the_committed_bytes() {
    // Drawn under the Legacy rules, pinned, so V2 rule changes cannot move the file.
    assert_eq!(TURN_SET_RULESET, Ruleset::Legacy);
    let set = generate_turn_set_v2(embedded_content_pack());
    let text = turn_set_v2_to_json(&set);
    assert_eq!(sha256_hex(text.as_bytes()), V2_SHA256);
    assert!(text.as_bytes() == V2_BYTES);
}

fn tag_counts() -> BTreeMap<String, usize> {
    let set = parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap();
    let mut counts = BTreeMap::new();
    for t in &set.turns {
        for tag in &t.strata {
            *counts.entry(tag.clone()).or_default() += 1;
        }
    }
    counts
}

#[test]
fn every_quota_is_met_and_the_counts_are_the_measured_ones() {
    let counts = tag_counts();
    for (tag, quota) in TURN_SET_V2_QUOTAS {
        let have = counts.get(tag).copied().unwrap_or(0);
        assert!(have >= quota, "{tag}: {have} of {quota}");
    }
    // Counted from the file when it was frozen.
    let expected: [(&str, usize); 30] = [
        ("allowance:hint", 49),
        ("allowance:none", 254),
        ("allowance:strong", 57),
        ("biome:alpine", 38),
        ("biome:desert", 25),
        ("biome:forest", 107),
        ("biome:plains", 137),
        ("biome:swamp", 53),
        ("camp:hunt", 18),
        ("camp:rest", 38),
        ("camp:travel", 80),
        ("death", 33),
        ("early_journey", 189),
        ("ending", 14),
        ("event_choice", 108),
        ("event_presented", 115),
        ("fork_choice", 1),
        ("injury", 88),
        ("landmark_arrival", 56),
        ("late_journey", 69),
        ("low_health", 32),
        ("low_morale", 79),
        ("mid_journey", 102),
        ("night_at_camp", 37),
        ("supply_zero", 31),
        ("victory", 6),
        ("weirdness:0", 90),
        ("weirdness:1", 90),
        ("weirdness:2", 90),
        ("weirdness:3", 90),
    ];
    let expected: BTreeMap<String, usize> =
        expected.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    assert_eq!(counts, expected);
    assert_eq!(TURN_SET_V2_PER_LEVEL, 90);
}

/// What version 1 lacked: the stored contexts span the journey, the day, the clock and the
/// party's condition. Every value is counted from the stored context itself.
#[test]
fn the_stored_contexts_span_the_journey() {
    let set = parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap();
    let mut band: BTreeMap<String, usize> = BTreeMap::new();
    let mut phase: BTreeMap<&str, usize> = BTreeMap::new();
    let mut clock: BTreeMap<&str, usize> = BTreeMap::new();
    let mut health: BTreeMap<String, usize> = BTreeMap::new();
    let mut morale: BTreeMap<String, usize> = BTreeMap::new();
    let mut days = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut per_run: BTreeMap<(u8, String, i64), usize> = BTreeMap::new();
    let mut ordinary = 0usize;
    for t in &set.turns {
        let c = &t.context;
        *band.entry(c.band().to_string()).or_default() += 1;
        *phase.entry(c.phase().as_str()).or_default() += 1;
        *clock.entry(c.time_of_day().as_str()).or_default() += 1;
        *health
            .entry(c.party_status().health_band().to_string())
            .or_default() += 1;
        *morale
            .entry(c.party_status().morale_band().to_string())
            .or_default() += 1;
        days.insert(c.day());
        assert!(ids.insert(t.id.clone()));
        *per_run
            .entry((t.weirdness_level, t.policy.clone(), t.seed))
            .or_default() += 1;
        // The tags agree with the stored context.
        let f = c.flags();
        for (on, tag) in [
            (f.death, "death"),
            (f.injury, "injury"),
            (f.supply_zero, "supply_zero"),
            (f.landmark_arrival, "landmark_arrival"),
            (f.night_at_camp, "night_at_camp"),
        ] {
            assert_eq!(t.strata.iter().any(|s| s == tag), on, "{} {tag}", t.id);
        }
        assert!(t.strata.contains(&format!("{}_journey", c.band())));
        assert!(t.strata.contains(&format!("biome:{}", c.biome())));
        assert_eq!(
            t.strata.iter().any(|s| s == "ending"),
            c.phase() == GamePhase::GameOver
        );
        if t.strata.iter().any(|s| s == "night_at_camp") {
            assert_eq!(c.time_of_day(), TimeOfDay::Night);
        }
        if ![
            "death",
            "ending",
            "low_health",
            "low_morale",
            "supply_zero",
            "injury",
        ]
        .iter()
        .any(|r| t.strata.iter().any(|s| s == r))
        {
            ordinary += 1;
        }
        assert!(!c.fallback_line().trim().is_empty());
    }
    let map = |pairs: &[(&str, usize)]| -> BTreeMap<String, usize> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    };
    assert_eq!(band, map(&[("early", 189), ("late", 69), ("mid", 102)]));
    assert_eq!(
        phase,
        BTreeMap::from([("camp", 230), ("event", 115), ("over", 14), ("route", 1)])
    );
    assert_eq!(
        clock,
        BTreeMap::from([
            ("afternoon", 73),
            ("evening", 92),
            ("morning", 74),
            ("night", 121)
        ])
    );
    assert_eq!(
        health,
        map(&[
            ("critical", 32),
            ("dead", 8),
            ("steady", 180),
            ("worn", 140)
        ])
    );
    assert_eq!(
        morale,
        map(&[("broken", 47), ("high", 256), ("low", 32), ("steady", 25)])
    );
    assert_eq!(days.iter().next().copied(), Some(1));
    assert_eq!(days.iter().next_back().copied(), Some(48));
    assert_eq!(days.len(), 37);
    assert_eq!(per_run.len(), 167);
    // The fourteen ending turns carry the engine's grade; no other turn does.
    let mut endings: BTreeMap<String, usize> = BTreeMap::new();
    for t in &set.turns {
        *endings.entry(t.context.ending().to_string()).or_default() += 1;
        assert_eq!(
            t.context.ending().is_empty(),
            t.context.phase() != GamePhase::GameOver,
            "{}",
            t.id
        );
    }
    assert_eq!(
        endings,
        map(&[("", 346), ("lost", 8), ("pyrrhic", 2), ("triumphant", 4)])
    );
    assert_eq!(per_run.values().copied().max(), Some(5));
    assert_eq!(ordinary, 215);
}

/// One turn checked by hand against the file: a death late in a run.
#[test]
fn one_late_death_turn_as_stored() {
    let set = parse_turn_set_v2(V2_BYTES, V2_SHA256).unwrap();
    let t = set.turns.iter().find(|t| t.id == "v2-w2-072").unwrap();
    assert_eq!((t.seed, t.turn, t.policy.as_str()), (2, 131, "first"));
    assert_eq!(t.action, "travel");
    assert_eq!(
        t.strata,
        vec![
            "allowance:none",
            "biome:forest",
            "death",
            "event_presented",
            "injury",
            "late_journey",
            "low_health",
            "low_morale",
            "weirdness:2",
        ]
    );
    let c = &t.context;
    assert_eq!(c.day(), 27);
    assert_eq!(c.place(), "The Switchback");
    assert_eq!(c.time_of_day(), TimeOfDay::Morning);
    assert_eq!(c.band(), "late");
    assert_eq!(c.party_status().health_band(), "critical");
    assert_eq!(c.party_status().morale_band(), "broken");
    assert!(c.flags().death && c.flags().injury);
    assert_eq!(c.run_seed(), 2);
    // What the turn was: travelling brought up an event, with three choices on offer.
    assert_eq!(c.event_title(), "Solitary Hunter");
    assert_eq!(
        c.check_event_seed(),
        "A man with a long rifle nods once, eyes measuring the wagon."
    );
    assert_eq!(
        c.offered_labels(),
        [
            "Trade powder for meat",
            "Continue without parley",
            "Ask of the ridges ahead"
        ]
    );
    assert_eq!(c.action(), "travel");

    // The prompts built from it carry those facts, which a version 1 turn never did.
    let (scene, _) = build_scene_prompts(GMProfile::Chronicler, NarratorLength::Standard, c);
    for fact in [
        "- event_title: Solitary Hunter",
        "- event_text: A man with a long rifle nods once, eyes measuring the wagon.",
        "- choices_offered: Trade powder for meat | Continue without parley | Ask of the ridges ahead",
        "- action: travel",
    ] {
        assert!(scene.contains(fact), "{fact}");
    }
    assert!(!scene.contains("WHAT RESULTED"));
    let (system, _) = build_outcome_prompts(GMProfile::Chronicler, NarratorLength::Standard, c);
    for fact in [
        "- journey_band: late",
        "- place: The Switchback",
        "- time_of_day: morning",
        "- party_health_band: critical",
        "- party_morale_band: broken",
        "turn_matters: death=true, injury=true",
    ] {
        assert!(system.contains(fact), "{fact}");
    }
}

struct Recorder {
    systems: Mutex<Vec<String>>,
}

impl NarratorTransport for Recorder {
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
        let body: serde_json::Value = serde_json::from_str(body_json).unwrap();
        if body["prompt"] == "Warm up." {
            return Ok((200, r#"{"response":""}"#.to_string()));
        }
        self.systems
            .lock()
            .unwrap()
            .push(body["system"].as_str().unwrap_or("").to_string());
        Err(TransportError::Timeout)
    }
}

/// The harness replays a version 2 turn from its stored context, under each profile in turn.
#[test]
fn the_harness_replays_stored_contexts_under_each_profile() {
    let set = parse_any_turn_set(V2_BYTES, V2_SHA256).unwrap();
    let mut cfg = MeasureConfig::new("fixture-model");
    cfg.lengths = vec![NarratorLength::Brief];
    cfg.max_turns = Some(2);
    let rec = Arc::new(Recorder {
        systems: Mutex::new(Vec::new()),
    });
    let (report, rows) = run_measurement_any(&set, V2_SHA256, &cfg, rec.clone()).unwrap();
    assert_eq!(report.turn_set_sha256, V2_SHA256);
    assert_eq!(report.turns_in_set, 360);
    assert_eq!(report.turns_run, 2);
    assert_eq!(report.cells.len(), 3);
    // Three profiles x two turns, in file order.
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].turn_id, "v2-w0-001");
    assert_eq!(rows[1].turn_id, "v2-w0-002");
    let systems = rec.systems.lock().unwrap().clone();
    // Two calls a turn.
    assert_eq!(systems.len(), 12);
    assert!(systems[0].starts_with("You are \"The Chronicler,\""));
    assert!(systems[4].starts_with("You are \"The Fireside Storyteller,\""));
    assert!(systems[8].starts_with("You are \"The Uncanny Bearer,\""));
    // The same turn's facts under every profile: only the profile header differs.
    let facts = |s: &str| s[s.find("TURN FACTS:").unwrap()..].to_string();
    assert_eq!(facts(&systems[0]), facts(&systems[4]));
    assert_eq!(facts(&systems[0]), facts(&systems[8]));

    // The stored context is replayed with only its profile replaced.
    let AnyTurnSet::V2(v2) = &set else {
        panic!("version 2 expected");
    };
    let turn = set.measured_turns()[0];
    let replayed = turn.context(GMProfile::Lantern);
    assert_eq!(replayed.gm_profile(), GMProfile::Lantern);
    assert_eq!(
        replayed,
        v2.turns[0]
            .context
            .clone()
            .with_gm_profile(GMProfile::Lantern)
    );
}

#[test]
fn check_only_accepts_either_set_by_its_digest() {
    let dir = tempfile::tempdir().unwrap();
    let run = |bytes: &[u8], sha: &str| -> Result<String, String> {
        let path = dir.path().join("set.json");
        std::fs::write(&path, bytes).unwrap();
        let args: Vec<String> = [
            "--turn-set",
            path.to_str().unwrap(),
            "--turn-set-sha256",
            sha,
            "--check-only",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut out = Vec::new();
        run_cli(
            &args,
            Arc::new(Recorder {
                systems: Mutex::new(Vec::new()),
            }),
            &mut out,
        )?;
        Ok(String::from_utf8(out).unwrap())
    };
    assert_eq!(
        run(V2_BYTES, V2_SHA256).unwrap(),
        format!("turn set ok: 360 turns, sha256 {V2_SHA256}\n")
    );
    assert_eq!(
        run(V1_BYTES, V1_SHA256).unwrap(),
        format!("turn set ok: 240 turns, sha256 {V1_SHA256}\n")
    );
    assert!(run(V2_BYTES, V1_SHA256)
        .unwrap_err()
        .starts_with("turn set sha256 mismatch"));
}
