//! Narrator turn set v1: the frozen set of turns the fallback-rate measurement runs on.
//!
//! The committed file is frozen by its SHA-256. If the generator, the engine or the content
//! changes what it would produce, this test fails; that is the point. A new set is a new file
//! with a new name and digest, never an edit of this one.

use std::collections::{BTreeMap, BTreeSet};

use etv_core::content::embedded_content_pack;
use etv_core::measure::{build_turn_context, parse_turn_set, sha256_hex, TurnSpec};
use etv_core::model::GMProfile;
use etv_core::turn_set::{generate_turn_set, turn_set_to_json, TURN_SET_V1_QUOTAS};

/// The frozen digest of `tests/fixtures/narrator_turn_set_v1.json`.
const TURN_SET_V1_SHA256: &str = "838e4346263507a112b6a161a4107a3647c253a39646b2427236afdd4e526f17";

const TURN_SET_V1_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v1.json");

#[test]
fn committed_file_matches_its_frozen_digest_and_parses() {
    assert!(!TURN_SET_V1_BYTES.contains(&b'\r'));
    assert_eq!(sha256_hex(TURN_SET_V1_BYTES), TURN_SET_V1_SHA256);
    let set = parse_turn_set(TURN_SET_V1_BYTES, TURN_SET_V1_SHA256).unwrap();
    assert_eq!(set.turns.len(), 240);
    assert!(parse_turn_set(TURN_SET_V1_BYTES, &"0".repeat(64)).is_err());
}

#[test]
fn generator_reproduces_the_committed_bytes() {
    assert_eq!(TURN_SET_V1_QUOTAS, [(0, 40), (1, 40), (2, 120), (3, 40)]);
    let set = generate_turn_set(&TURN_SET_V1_QUOTAS, embedded_content_pack());
    let text = turn_set_to_json(&set);
    assert_eq!(sha256_hex(text.as_bytes()), TURN_SET_V1_SHA256);
    assert!(text.as_bytes() == TURN_SET_V1_BYTES);
}

#[test]
fn makeup_of_the_set() {
    let set = parse_turn_set(TURN_SET_V1_BYTES, TURN_SET_V1_SHA256).unwrap();
    let mut per_level: BTreeMap<u8, usize> = BTreeMap::new();
    let mut pairs_per_level: BTreeSet<(u8, &str, &str)> = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let content_ids: BTreeSet<&str> = embedded_content_pack()
        .all_events
        .iter()
        .map(|e| e.event_id.as_str())
        .collect();
    let mut per_seed_level: BTreeMap<(u8, i64), usize> = BTreeMap::new();
    for t in &set.turns {
        *per_level.entry(t.weirdness_level).or_default() += 1;
        // No (event, choice) pair repeats inside a level.
        assert!(pairs_per_level.insert((t.weirdness_level, &t.event_id, &t.choice_id)));
        assert!(ids.insert(t.id.as_str()));
        assert!(content_ids.contains(t.event_id.as_str()), "{}", t.event_id);
        assert!(!t.choice_label.is_empty());
        assert!(!t.event_seed.is_empty());
        assert!(t.fallback_line.ends_with('.'));
        assert!((0..=2).contains(&t.uncanny_tokens));
        *per_seed_level
            .entry((t.weirdness_level, t.seed))
            .or_default() += 1;
    }
    assert_eq!(
        per_level,
        BTreeMap::from([(0, 40), (1, 40), (2, 120), (3, 40)])
    );
    // At most four turns come from one run.
    assert_eq!(per_seed_level.values().copied().max(), Some(4));
    assert_eq!(set.turns[0].id, "w0-001");
    assert_eq!(set.turns[239].id, "w3-040");

    // Counted from the file by hand-run script when it was frozen.
    let distinct_events: BTreeSet<&str> = set.turns.iter().map(|t| t.event_id.as_str()).collect();
    assert_eq!(distinct_events.len(), 120);
    let without_effects = set.turns.iter().filter(|t| t.effects.is_empty()).count();
    assert_eq!(without_effects, 6);
}

#[test]
fn every_turn_rebuilds_with_the_effects_the_game_showed() {
    let set = parse_turn_set(TURN_SET_V1_BYTES, TURN_SET_V1_SHA256).unwrap();
    for t in &set.turns {
        for profile in [
            GMProfile::Chronicler,
            GMProfile::Fireside,
            GMProfile::Lantern,
        ] {
            let ctx = build_turn_context(t, profile);
            assert_eq!(ctx.event_id(), t.event_id);
            assert_eq!(ctx.choice_id(), t.choice_id);
            assert_eq!(ctx.fallback_line(), t.fallback_line);
            assert_eq!(ctx.gm_profile(), profile);
            if !t.effects.is_empty() {
                assert_eq!(ctx.shown_deltas(), &t.effects, "turn {}", t.id);
            }
        }
    }
}

/// One turn checked by hand against the file: a morale and time outcome that carries no supply
/// change. Before `effects` existed, a rebuilt turn like this showed the narrator no effects.
#[test]
fn effects_carry_non_supply_changes() {
    let set = parse_turn_set(TURN_SET_V1_BYTES, TURN_SET_V1_SHA256).unwrap();
    let t = set.turns.iter().find(|t| t.id == "w2-021").unwrap();
    assert_eq!(t.event_id, "f3_005");
    assert_eq!(t.choice_id, "A");
    assert_eq!(t.seed, 6);
    assert!(t.deltas.is_empty());
    assert_eq!(
        t.effects,
        BTreeMap::from([
            ("animals_health".to_string(), -2),
            ("health".to_string(), -36),
            ("time_days".to_string(), 1),
            ("wagon_condition".to_string(), -4),
        ])
    );
    assert_eq!(
        t.fallback_line,
        "The party loses strength, strains the animals, damages the wagon and loses time."
    );
    let ctx = build_turn_context(t, GMProfile::Fireside);
    assert_eq!(ctx.shown_deltas(), &t.effects);
}

/// A turn written before `effects` existed still parses and takes the old path.
#[test]
fn a_turn_without_effects_uses_its_supply_deltas() {
    let old: TurnSpec = serde_json::from_str(
        r#"{"id":"x1","seed":42,"event_id":"f1_001","choice_id":"A","weirdness_level":0,
            "deltas":{"water":-1},"event_seed":"Fixture seed.","choice_label":"Fixture label",
            "fallback_line":"Fixture line."}"#,
    )
    .unwrap();
    assert!(old.effects.is_empty());
    let ctx = build_turn_context(&old, GMProfile::Chronicler);
    assert_eq!(
        ctx.shown_deltas(),
        &BTreeMap::from([("water".to_string(), -1)])
    );
    // And it serialises without an `effects` key, as before.
    assert!(!serde_json::to_string(&old).unwrap().contains("effects"));
}
