//! Narrator turn set v3: the v2 generator's output after the blockers fix (2026-11-14). Session 7
//! runs on it. Versions 1 and 2 stay frozen beside it, unedited, as the record of sessions 1 to 6.
//!
//! It keeps the v2 schema (`etv-turn-set-2`) and the v2 turn ids: the turns are the same 360,
//! drawn the same way. What changed is recorded below and pinned by a test, from the two stored
//! files:
//!
//! - every stored context carries `turn_index`, the run's step counter after the turn, which is
//!   the turn's own `turn` number on every turn (never 0, and written even when 0);
//! - eleven turns carry the meds they spend in `shown_deltas` (the hand-written key list left
//!   meds out before the fix);
//! - nothing else: no fallback line, tag, id, order or other fact changed. No turn in the set
//!   spends or gains salt.
//!
//! The committed file is frozen by its SHA-256. If the generator, the engine or the content
//! changes what it would produce, the reproduce test fails on purpose. A new set is a new file
//! with a new name and digest, never an edit of this one.

use std::collections::BTreeMap;

use serde_json::Value;

use etv_core::content::embedded_content_pack;
use etv_core::measure::{parse_any_turn_set, parse_turn_set_v2, sha256_hex, AnyTurnSet};
use etv_core::ruleset::Ruleset;
use etv_core::turn_set::{generate_turn_set_v2, turn_set_v2_to_json, TURN_SET_RULESET};

const V3_SHA256: &str = "508e2a496084c7f69b548813c00938c704d38e64c30d747e2daaa61acfd89dc0";
const V3_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v3.json");
const V2_SHA256: &str = "59a3e4f8870474d01b775a52071fae9a12629cec0f225606271e9d77f5f8ca46";
const V2_BYTES: &[u8] = include_bytes!("fixtures/narrator_turn_set_v2.json");

/// The eleven turns whose stored context gained a meds entry in `shown_deltas`, with the value.
const MEDS_TURNS: [(&str, i64); 11] = [
    ("v2-w0-009", -2),
    ("v2-w1-009", -2),
    ("v2-w1-033", -1),
    ("v2-w1-084", -1),
    ("v2-w1-085", -2),
    ("v2-w1-088", -1),
    ("v2-w1-089", -1),
    ("v2-w2-015", -1),
    ("v2-w2-089", -1),
    ("v2-w3-017", -1),
    ("v2-w3-069", -1),
];

#[test]
fn the_file_matches_its_digest_and_the_harness_reads_it() {
    assert!(!V3_BYTES.contains(&b'\r'));
    assert_eq!(sha256_hex(V3_BYTES), V3_SHA256);
    let set = parse_any_turn_set(V3_BYTES, V3_SHA256).unwrap();
    assert!(matches!(set, AnyTurnSet::V2(_)));
    assert_eq!(set.len(), 360);
    // Refused under the v2 digest, and v2 under this one.
    assert!(parse_any_turn_set(V3_BYTES, V2_SHA256).is_err());
    assert!(parse_any_turn_set(V2_BYTES, V3_SHA256).is_err());
}

#[test]
fn generator_reproduces_the_committed_bytes() {
    // Drawn under the Legacy rules, pinned, so V2 rule changes cannot move the file.
    assert_eq!(TURN_SET_RULESET, Ruleset::Legacy);
    let set = generate_turn_set_v2(embedded_content_pack());
    let text = turn_set_v2_to_json(&set);
    assert_eq!(sha256_hex(text.as_bytes()), V3_SHA256);
    assert!(text.as_bytes() == V3_BYTES);
}

#[test]
fn every_context_carries_its_turn_index_and_it_is_the_turns_step_number() {
    let set = parse_turn_set_v2(V3_BYTES, V3_SHA256).unwrap();
    for t in &set.turns {
        assert_eq!(t.context.turn_index(), u64::from(t.turn), "{}", t.id);
        assert!(t.context.turn_index() > 0, "{}", t.id);
    }
    // Written on every turn, so a stored 0 could never be told from a missing field.
    let raw: Value = serde_json::from_slice(V3_BYTES).unwrap();
    for t in raw["turns"].as_array().unwrap() {
        assert!(t["context"].get("turn_index").is_some(), "{}", t["id"]);
    }
}

/// The whole difference from v2, field by field, read from the two stored files.
#[test]
fn the_difference_from_v2_is_the_turn_index_and_eleven_meds_entries() {
    let v2: Value = serde_json::from_slice(V2_BYTES).unwrap();
    let v3: Value = serde_json::from_slice(V3_BYTES).unwrap();
    assert_eq!(v2["schema"], v3["schema"]);
    let (a, b) = (
        v2["turns"].as_array().unwrap(),
        v3["turns"].as_array().unwrap(),
    );
    assert_eq!(a.len(), 360);
    assert_eq!(b.len(), 360);
    let meds: BTreeMap<&str, i64> = MEDS_TURNS.iter().copied().collect();
    let mut seen_meds = BTreeMap::new();
    for (old, new) in a.iter().zip(b) {
        let id = new["id"].as_str().unwrap();
        assert_eq!(old["id"], new["id"]);
        assert!(old["context"].get("turn_index").is_none(), "{id}");
        let mut stripped = new.clone();
        let ctx = stripped["context"].as_object_mut().unwrap();
        assert_eq!(ctx.remove("turn_index"), Some(new["turn"].clone()), "{id}");
        let deltas = ctx["shown_deltas"].as_object_mut().unwrap();
        if let Some(v) = deltas.remove("meds") {
            assert!(old["context"]["shown_deltas"].get("meds").is_none(), "{id}");
            seen_meds.insert(id, v.as_i64().unwrap());
        }
        assert!(deltas.get("salt").is_none(), "{id}");
        // With those two taken away, the turn is exactly as v2 stored it.
        assert_eq!(&stripped, old, "{id}");
    }
    assert_eq!(seen_meds, meds);
}
