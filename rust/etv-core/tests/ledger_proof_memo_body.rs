//! Finding 4 of the PR #67 review: a tampered save must not pass the proof.
//!
//! The proof used to check only each on-chain memo's prefix. It now also parses the memo's delta
//! body and compares it, key by key, with the saved settlement record for the same transaction.
//!
//! The clean input is built by hand from the recorded fixture (`xrpl_recorded_run_seed7.json`):
//! the five records and their 14 transaction ids are written out as literals, the ledger memos are
//! read back through `RecordedTransport` and `fetch_onchain_memos`, and the balances are the
//! fixture's AccountLines. No test opens a socket.
//!
//! Failure kinds are compared by their stable strings so the pre-change behaviour can be measured
//! with the same assertions.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use serde_json::Value;

use etv_core::backpack::{
    fetch_onchain_memos, settlement_memo_text, stamp_minted_snapshot, ReadLog, RecordedTransport,
};
use etv_core::ledger_proof::{
    is_inconclusive, proof_verdict, prove_player_save, reconcile, report_to_value, LedgerView,
    ProofVerdict, ReconcileInput, ReconcileReport,
};
use etv_core::model::{BackpackState, SettlementRecord};

const PLAYER: &str = "rTestPlayerWallet111111111111";
const ISSUER: &str = "rTestIssuerWallet222222222222";
const RUN_ID: &str = "run-2b-seed7";

fn map(pairs: &[(&str, i32)]) -> BTreeMap<String, i32> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn fixture() -> Value {
    let p =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xrpl_recorded_run_seed7.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("fixture readable"))
        .expect("fixture parses")
}

fn record(day: i32, location: &str, deltas: &[(&str, i32)], txids: &[&str]) -> SettlementRecord {
    let deltas = map(deltas);
    SettlementRecord {
        day,
        location: location.to_string(),
        memo: settlement_memo_text(RUN_ID, day, &deltas),
        deltas,
        txids: txids.iter().map(|t| t.to_string()).collect(),
        status: "settled".to_string(),
        timestamp: String::new(),
    }
}

/// The five settled records of the recorded run, as literals (same values the existing
/// deterministic-run test pins from the fixture's engine truth).
fn records() -> Vec<SettlementRecord> {
    vec![
        record(
            11,
            "Dunhollow",
            &[("ammo", -5), ("food", -5), ("meds", 6), ("parts", -3)],
            &[
                "TX2B_D11_AMO",
                "TX2B_D11_FOD",
                "TX2B_D11_MED",
                "TX2B_D11_PRT",
            ],
        ),
        record(
            14,
            "Coldspring",
            &[("food", -10), ("meds", -2)],
            &["TX2B_D14_FOD", "TX2B_D14_MED"],
        ),
        record(
            19,
            "Fellgate",
            &[("ammo", -3), ("parts", -1)],
            &["TX2B_D19_AMO", "TX2B_D19_PRT"],
        ),
        record(
            22,
            "Hazelwick",
            &[("ammo", -1), ("food", -1), ("water", -6)],
            &["TX2B_D22_AMO", "TX2B_D22_FOD", "TX2B_D22_WTR"],
        ),
        record(
            26,
            "final-reconcile",
            &[("ammo", -1), ("food", -3), ("water", -5)],
            &["TX2B_D26_AMO", "TX2B_D26_FOD", "TX2B_D26_WTR"],
        ),
    ]
}

fn clean() -> ReconcileInput {
    let doc = fixture();
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let mut log = ReadLog::default();
    let memos = fetch_onchain_memos(&mut t, PLAYER, ISSUER, &mut log).expect("memos");
    ReconcileInput {
        run_id: RUN_ID.to_string(),
        seed: 7,
        minted_initial: map(&[
            ("ammo", 20),
            ("food", 50),
            ("meds", 5),
            ("parts", 4),
            ("water", 50),
        ]),
        ledger_balances: map(&[
            ("AMO", 10),
            ("FOD", 31),
            ("MED", 9),
            ("PRT", 0),
            ("WTR", 39),
        ]),
        last_settled_supplies: map(&[
            ("ammo", 10),
            ("food", 31),
            ("meds", 9),
            ("parts", 0),
            ("water", 39),
        ]),
        settlements: records(),
        pending: vec![],
        player_address: PLAYER.to_string(),
        issuer_address: ISSUER.to_string(),
        onchain_memos: Some(memos),
    }
}

fn code_strings(r: &ReconcileReport) -> Vec<&'static str> {
    r.failures.iter().map(|f| f.code.as_str()).collect()
}

fn details(r: &ReconcileReport, code: &str) -> Vec<String> {
    r.failures
        .iter()
        .filter(|f| f.code.as_str() == code)
        .map(|f| f.detail.clone())
        .collect()
}

fn set_chain(i: &mut ReconcileInput, txid: &str, memo: &str) {
    i.onchain_memos
        .as_mut()
        .unwrap()
        .insert(txid.to_string(), memo.to_string());
}

// ============================================================================
// c. The clean recorded run is still clean
// ============================================================================

#[test]
fn the_clean_recorded_run_is_still_clean() {
    let i = clean();
    // 14 settlement transactions have a memo on the ledger (the five mint payments carry none).
    assert_eq!(i.onchain_memos.as_ref().unwrap().len(), 14);
    let r = reconcile(&i);
    assert!(r.passed, "{:?}", r.notes);
    assert!(r.failures.is_empty() && r.notes.is_empty());
    assert_eq!(r.txids.len(), 14);
    assert_eq!(proof_verdict(&r, false, false), ProofVerdict::Pass);
}

// ============================================================================
// a. The failing-before test: a consistent multi-field tamper
// ============================================================================

/// The day-26 record says food -1 instead of -3, and the save's own mint snapshot says 48 food
/// instead of 50, so `minted + deltas == settled` still balances (48 - 17 = 31). The settled
/// supply, the ledger balance and every memo prefix are untouched.
fn consistent_tamper() -> ReconcileInput {
    let mut i = clean();
    i.settlements[4].deltas.insert("food".to_string(), -1);
    i.minted_initial.insert("food".to_string(), 48);
    i
}

#[test]
fn a_consistent_multi_field_tamper_is_caught_at_the_exact_transaction() {
    let r = reconcile(&consistent_tamper());
    // Measured on the base commit before the change (no body check): passed=true, no failures.
    println!(
        "tampered save: passed={} codes={:?} notes={:?}",
        r.passed,
        code_strings(&r),
        r.notes
    );
    // Conservation and the balance still hold, as the tamperer intended.
    assert!(r.resources.iter().all(|x| x.ok()), "{:?}", r.resources);
    assert!(r.memo_local_ok);
    assert_eq!(
        code_strings(&r),
        [
            "ONCHAIN_MEMO_DELTA_MISMATCH",
            "ONCHAIN_MEMO_DELTA_MISMATCH",
            "ONCHAIN_MEMO_DELTA_MISMATCH"
        ],
        "{:?}",
        r.notes
    );
    assert_eq!(
        details(&r, "ONCHAIN_MEMO_DELTA_MISMATCH"),
        [
            "settlement day 26: txid 'TX2B_D26_AMO': food: saved record has -1, on-chain memo has -3 (external integrity)",
            "settlement day 26: txid 'TX2B_D26_FOD': food: saved record has -1, on-chain memo has -3 (external integrity)",
            "settlement day 26: txid 'TX2B_D26_WTR': food: saved record has -1, on-chain memo has -3 (external integrity)",
        ]
    );
    assert!(!r.passed && !r.memo_ok);
    assert_eq!(r.onchain_memo_ok, Some(false));
    assert_eq!(proof_verdict(&r, false, false), ProofVerdict::Fail);
    assert!(!is_inconclusive(&r));
}

// ============================================================================
// b. One test per new failure kind
// ============================================================================

#[test]
fn body_differs_gives_its_own_code_per_transaction() {
    let mut i = clean();
    set_chain(
        &mut i,
        "TX2B_D14_FOD",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-9,MED-2|V:v1",
    );
    let r = reconcile(&i);
    assert_eq!(code_strings(&r), ["ONCHAIN_MEMO_DELTA_MISMATCH"]);
    assert_eq!(
        r.notes,
        ["settlement day 14: txid 'TX2B_D14_FOD': food: saved record has -10, on-chain memo has -9 (external integrity)"]
    );
    assert!(!r.passed);
}

#[test]
fn a_sign_flip_is_a_body_difference() {
    let mut i = clean();
    set_chain(
        &mut i,
        "TX2B_D11_MED",
        "TRAIL|RUN:run-2b-seed7|DAY:11|DELTA:AMO-5,FOD-5,MED-6,PRT-3|V:v1",
    );
    let r = reconcile(&i);
    assert_eq!(code_strings(&r), ["ONCHAIN_MEMO_DELTA_MISMATCH"]);
    assert_eq!(
        r.notes,
        ["settlement day 11: txid 'TX2B_D11_MED': meds: saved record has 6, on-chain memo has -6 (external integrity)"]
    );
}

#[test]
fn body_unparseable_gives_its_own_code() {
    let mut i = clean();
    set_chain(
        &mut i,
        "TX2B_D14_FOD",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-10,XXX-2|V:v1",
    );
    // Same keys, but not the canonical order the builder writes.
    set_chain(
        &mut i,
        "TX2B_D14_MED",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:MED-2,FOD-10|V:v1",
    );
    // A schema version the proof does not know.
    set_chain(
        &mut i,
        "TX2B_D19_AMO",
        "TRAIL|RUN:run-2b-seed7|DAY:19|DELTA:AMO-3,PRT-1|V:v2",
    );
    // A versioned memo with no DELTA field.
    set_chain(&mut i, "TX2B_D19_PRT", "TRAIL|RUN:run-2b-seed7|DAY:19|V:v1");
    let r = reconcile(&i);
    assert_eq!(
        code_strings(&r),
        [
            "ONCHAIN_MEMO_BODY_UNPARSEABLE",
            "ONCHAIN_MEMO_BODY_UNPARSEABLE",
            "ONCHAIN_MEMO_BODY_UNPARSEABLE",
            "ONCHAIN_MEMO_BODY_UNPARSEABLE"
        ]
    );
    assert_eq!(
        r.notes,
        [
            "settlement day 14: txid 'TX2B_D14_FOD': on-chain memo 'TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-10,XXX-2|V:v1' has an unparseable delta body (unknown token code 'XXX') (external integrity)",
            "settlement day 14: txid 'TX2B_D14_MED': on-chain memo 'TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:MED-2,FOD-10|V:v1' has an unparseable delta body (body is not the canonical form the memo builder writes) (external integrity)",
            "settlement day 19: txid 'TX2B_D19_AMO': on-chain memo 'TRAIL|RUN:run-2b-seed7|DAY:19|DELTA:AMO-3,PRT-1|V:v2' has an unparseable delta body (body is not the canonical form the memo builder writes) (external integrity)",
            "settlement day 19: txid 'TX2B_D19_PRT': on-chain memo 'TRAIL|RUN:run-2b-seed7|DAY:19|V:v1' has an unparseable delta body (no DELTA field after the prefix) (external integrity)",
        ]
    );
    assert!(!r.passed);
}

#[test]
fn an_extra_key_on_chain_gives_its_own_code() {
    let mut i = clean();
    set_chain(
        &mut i,
        "TX2B_D14_MED",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-10,MED-2,WTR-4|V:v1",
    );
    let r = reconcile(&i);
    assert_eq!(code_strings(&r), ["ONCHAIN_MEMO_DELTA_EXTRA_KEY"]);
    assert_eq!(
        r.notes,
        ["settlement day 14: txid 'TX2B_D14_MED': water: on-chain memo has -4, no saved record accounts for it (external integrity)"]
    );
    assert!(!r.passed);
}

#[test]
fn an_extra_key_that_a_pending_record_of_the_same_day_carries_is_a_partial_batch_not_a_tamper() {
    // A batch that failed part way keeps its full delta set on-chain; the receipt keeps the
    // confirmed keys; the rest is pending with the same amount. Only the pending code remains.
    let mut i = clean();
    set_chain(
        &mut i,
        "TX2B_D14_MED",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-10,MED-2,WTR-4|V:v1",
    );
    set_chain(
        &mut i,
        "TX2B_D14_FOD",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-10,MED-2,WTR-4|V:v1",
    );
    let mut p = record(14, "Coldspring", &[("water", -4)], &[]);
    p.status = "pending".to_string();
    i.pending.push(p);
    let r = reconcile(&i);
    assert_eq!(code_strings(&r), ["SETTLEMENTS_PENDING"]);
    // The same extra key with a different amount in the pending record is not accounted for.
    i.pending[0].deltas.insert("water".to_string(), -3);
    let r = reconcile(&i);
    assert_eq!(
        code_strings(&r),
        [
            "ONCHAIN_MEMO_DELTA_EXTRA_KEY",
            "ONCHAIN_MEMO_DELTA_EXTRA_KEY",
            "SETTLEMENTS_PENDING"
        ]
    );
}

#[test]
fn a_key_missing_on_chain_gives_its_own_code() {
    let mut i = clean();
    set_chain(
        &mut i,
        "TX2B_D14_MED",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-10|V:v1",
    );
    let r = reconcile(&i);
    assert_eq!(code_strings(&r), ["ONCHAIN_MEMO_DELTA_MISSING_KEY"]);
    assert_eq!(
        r.notes,
        ["settlement day 14: txid 'TX2B_D14_MED': meds: saved record has -2, the on-chain memo has no such key (external integrity)"]
    );
    assert!(!r.passed);
}

#[test]
fn a_memo_without_a_version_field_keeps_the_prefix_only_check() {
    // Memos the builder did not write (the Python reference vectors use them) are not body-checked.
    let mut i = clean();
    set_chain(
        &mut i,
        "TX2B_D14_FOD",
        "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-1",
    );
    let r = reconcile(&i);
    assert!(r.passed, "{:?}", r.notes);
}

#[test]
fn a_fetch_failure_stays_inconclusive() {
    let c = clean();
    let mut bp = BackpackState {
        enabled: true,
        wallet_address: PLAYER.to_string(),
        issuer_address: ISSUER.to_string(),
        trust_lines_ready: true,
        last_settled_supplies: c.last_settled_supplies.clone(),
        settlements: c.settlements.clone(),
        ..BackpackState::default()
    };
    stamp_minted_snapshot(&mut bp, Some(&c.minted_initial));
    let view = LedgerView {
        balances: None,
        onchain_memos: None,
    };
    let res = prove_player_save(RUN_ID, 7, &bp, &view);
    assert_eq!(res.verdict, ProofVerdict::Inconclusive);
}

// ============================================================================
// Seed above i64::MAX (review note 8, second half)
// ============================================================================

#[test]
fn the_report_writes_a_seed_above_i64_max_exactly() {
    let seed_of = |seed: i128| {
        let mut i = clean();
        i.seed = seed;
        report_to_value(&reconcile(&i))["seed"].clone()
    };
    assert_eq!(seed_of(7), serde_json::json!(7));
    assert_eq!(seed_of(-7), serde_json::json!(-7));
    assert_eq!(
        seed_of(9_223_372_036_854_775_807),
        serde_json::json!(9223372036854775807i64)
    );
    // Above i64::MAX but within u64: still a JSON integer, written exactly (was 0).
    assert_eq!(
        seed_of(18_446_744_073_709_551_615),
        serde_json::json!(18446744073709551615u64)
    );
    // Beyond u64 no JSON integer can hold it here: its decimal digits as a string (was 0).
    assert_eq!(
        seed_of(170_141_183_460_469_231_731_687_303_715_884_105_727),
        serde_json::json!("170141183460469231731687303715884105727")
    );
}

// ============================================================================
// d. The multi-field mutation sweep
// ============================================================================

/// Counts of mutations by the exact set of failure codes that caught them.
fn run_sweep() -> (usize, BTreeMap<String, usize>) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    let mut check = |label: String, i: &ReconcileInput| {
        let r = reconcile(i);
        assert!(!r.passed, "{label}: tampered save reported clean");
        assert_eq!(
            proof_verdict(&r, false, false),
            ProofVerdict::Fail,
            "{label}"
        );
        let codes: BTreeSet<&str> = code_strings(&r).into_iter().collect();
        assert!(!codes.is_empty(), "{label}: no code");
        *counts
            .entry(codes.into_iter().collect::<Vec<_>>().join("+"))
            .or_insert(0) += 1;
        total += 1;
    };
    let base = clean();

    // Family 1 (120): one record delta moved by k, and the save's mint claim moved by the same k,
    // so conservation still balances. Two fields.
    for n in 0..120usize {
        let mut i = base.clone();
        let r = n % 5;
        let keys: Vec<String> = i.settlements[r].deltas.keys().cloned().collect();
        let key = keys[(n / 5) % keys.len()].clone();
        let k = (1 + (n / 25) % 4) as i32 * if n % 2 == 0 { 1 } else { -1 };
        *i.settlements[r].deltas.get_mut(&key).unwrap() += k;
        *i.minted_initial.get_mut(&key).unwrap() += k;
        check(format!("family1 n={n}"), &i);
    }

    // Family 2 (150): two record deltas for the same key move by +k and -k, so the sum, the
    // conservation and the ledger balance are all unchanged. Two fields.
    let key_records: [(&str, [usize; 4], usize); 5] = [
        ("food", [0, 1, 3, 4], 4),
        ("ammo", [0, 2, 3, 4], 4),
        ("water", [3, 4, 0, 0], 2),
        ("meds", [0, 1, 0, 0], 2),
        ("parts", [0, 2, 0, 0], 2),
    ];
    for (key, recs, len) in key_records {
        for a in 0..len {
            for b in 0..len {
                if a == b {
                    continue;
                }
                for k in 1..=5 {
                    let mut i = base.clone();
                    *i.settlements[recs[a]].deltas.get_mut(key).unwrap() += k;
                    *i.settlements[recs[b]].deltas.get_mut(key).unwrap() -= k;
                    check(format!("family2 {key} {a}->{b} k={k}"), &i);
                }
            }
        }
    }

    // Family 3 (120): three fields: a record delta, the mint claim, and the record's stored memo
    // rewritten to match its tampered deltas (so even the local text agrees with the edit).
    for n in 0..120usize {
        let mut i = base.clone();
        let r = n % 5;
        let keys: Vec<String> = i.settlements[r].deltas.keys().cloned().collect();
        let key = keys[(n / 5) % keys.len()].clone();
        let k = (1 + (n / 25) % 4) as i32;
        *i.settlements[r].deltas.get_mut(&key).unwrap() -= k;
        *i.minted_initial.get_mut(&key).unwrap() -= k;
        i.settlements[r].memo =
            settlement_memo_text(RUN_ID, i.settlements[r].day, &i.settlements[r].deltas);
        check(format!("family3 n={n}"), &i);
    }

    // Family 4 (30): three fields with the engine total moved too: a record delta, the mint claim
    // and the last-settled supply all shifted by k. The ledger balance and the memo disagree.
    for n in 0..30usize {
        let mut i = base.clone();
        let r = n % 5;
        let keys: Vec<String> = i.settlements[r].deltas.keys().cloned().collect();
        let key = keys[(n / 5) % keys.len()].clone();
        let k = (1 + n / 15) as i32;
        *i.settlements[r].deltas.get_mut(&key).unwrap() += k;
        *i.last_settled_supplies.get_mut(&key).unwrap() += k;
        check(format!("family4 n={n}"), &i);
    }
    (total, counts)
}

#[test]
fn multi_field_tamper_sweep_never_reports_clean() {
    let (total, counts) = run_sweep();
    println!("sweep: {total} mutations, by code set: {counts:?}");
    assert_eq!(total, 420);
    assert_eq!(counts.values().sum::<usize>(), total);
    assert_eq!(
        counts,
        [
            ("CONSERVATION_BROKEN+ONCHAIN_MEMO_DELTA_MISMATCH", 240usize),
            ("LEDGER_BALANCE_MISMATCH+ONCHAIN_MEMO_DELTA_MISMATCH", 30),
            ("ONCHAIN_MEMO_DELTA_MISMATCH", 150),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect::<BTreeMap<_, _>>()
    );
}

// ============================================================================
// e. The Python reference proof cases
// ============================================================================

fn gz_json(rel: &str) -> Value {
    let full: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(rel);
    let mut s = String::new();
    GzDecoder::new(File::open(full).expect("open vector"))
        .read_to_string(&mut s)
        .expect("gunzip");
    serde_json::from_str(&s).expect("vector JSON")
}

/// The reference cases' base save (same literals as the Python vector generator's inputs).
fn vector_base(run_id: &str) -> BackpackState {
    let mut bp = BackpackState {
        enabled: true,
        wallet_address: "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh".to_string(),
        issuer_address: "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe".to_string(),
        trust_lines_ready: true,
        ..BackpackState::default()
    };
    stamp_minted_snapshot(
        &mut bp,
        Some(&map(&[
            ("food", 50),
            ("water", 50),
            ("meds", 5),
            ("ammo", 20),
            ("parts", 3),
        ])),
    );
    bp.last_settled_supplies = map(&[
        ("food", 34),
        ("water", 34),
        ("meds", 3),
        ("ammo", 15),
        ("parts", 2),
    ]);
    bp.last_settlement_day = 12;
    bp.settlements = vec![
        SettlementRecord {
            day: 6,
            location: "Red Ford".into(),
            deltas: map(&[("food", -10), ("water", -12), ("ammo", -3)]),
            txids: vec!["TX_PROOF_D6_1".into(), "TX_PROOF_D6_2".into()],
            status: "confirmed".into(),
            memo: format!("TRAIL|RUN:{run_id}|DAY:6|DELTA:FOD-10,WTR-12,AMO-3"),
            timestamp: String::new(),
        },
        SettlementRecord {
            day: 12,
            location: "Pine Camp".into(),
            deltas: map(&[
                ("food", -6),
                ("water", -4),
                ("meds", -2),
                ("ammo", -2),
                ("parts", -1),
            ]),
            txids: vec!["TX_PROOF_D12_1".into()],
            status: "confirmed".into(),
            memo: format!("TRAIL|RUN:{run_id}|DAY:12|DELTA:FOD-6,WTR-4,MED-2,AMO-2,PRT-1"),
            timestamp: String::new(),
        },
    ];
    bp
}

fn vector_memos(run_id: &str, suffix: &str) -> BTreeMap<String, String> {
    [
        (
            "TX_PROOF_D6_1",
            format!("TRAIL|RUN:{run_id}|DAY:6|DELTA:FOD-10{suffix}"),
        ),
        (
            "TX_PROOF_D6_2",
            format!("TRAIL|RUN:{run_id}|DAY:6|DELTA:WTR-12,AMO-3{suffix}"),
        ),
        (
            "TX_PROOF_D12_1",
            format!("TRAIL|RUN:{run_id}|DAY:12|DELTA:FOD-6,WTR-4,MED-2,AMO-2,PRT-1{suffix}"),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Run every reference case; return (case id, Python verdict, Rust verdict, codes).
fn run_reference_cases(memo_suffix: &str) -> Vec<(String, String, String, Vec<&'static str>)> {
    let doc = gz_json("rust/oracle/vectors/ledger_reference.json.gz");
    let cases = doc["proof_cases"].as_array().expect("proof_cases");
    let matching = map(&[
        ("FOD", 34),
        ("WTR", 34),
        ("MED", 3),
        ("AMO", 15),
        ("PRT", 2),
    ]);
    let drifted = map(&[
        ("FOD", 31),
        ("WTR", 34),
        ("MED", 3),
        ("AMO", 19),
        ("PRT", 2),
    ]);
    let mut out = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap().to_string();
        let seed = case["seed"].as_i64().unwrap() as i128;
        let run_id = case["report_dict"]["run_id"]
            .as_str()
            .unwrap_or("none")
            .to_string();
        let mut bp = vector_base(&run_id);
        let mut view = LedgerView {
            balances: Some(matching.clone()),
            onchain_memos: Some(vector_memos(&run_id, memo_suffix)),
        };
        let pending = |deltas: &[(&str, i32)]| SettlementRecord {
            day: 12,
            location: "Pine Camp".into(),
            deltas: map(deltas),
            txids: vec![],
            status: "pending".into(),
            memo: format!("TRAIL|RUN:{run_id}|DAY:12"),
            timestamp: String::new(),
        };
        match id.as_str() {
            "pass_exact_match" => {}
            "pass_legacy_permit_shim" => bp.minted_initial.clear(),
            "fail_balance_drift" => view.balances = Some(drifted.clone()),
            "fail_pending_and_drift" => {
                bp.pending_settlements.push(pending(&[("food", -2)]));
                view.balances = Some(drifted.clone());
            }
            "inconclusive_pending_settlements" => {
                bp.pending_settlements.push(pending(&[("water", -3)]));
            }
            "inconclusive_network_unreachable" => {
                view = LedgerView {
                    balances: None,
                    onchain_memos: None,
                };
            }
            "inconclusive_missing_mint_snapshot" => {
                bp.minted_initial.clear();
                bp.permits.clear();
            }
            "fail_onchain_memo_mismatch" => {
                view.onchain_memos.as_mut().unwrap().insert(
                    "TX_PROOF_D6_1".into(),
                    "TRAIL|RUN:tampered-00000000|DAY:6|DELTA:FOD-10".into(),
                );
            }
            "inconclusive_backpack_disabled" => bp.enabled = false,
            other => panic!("unknown reference case {other}"),
        }
        let res = prove_player_save(&run_id, seed, &bp, &view);
        let codes = res
            .report
            .as_ref()
            .map(|r| code_strings(r))
            .unwrap_or_default();
        out.push((
            id,
            case["verdict"].as_str().unwrap().to_string(),
            res.verdict.as_str().to_string(),
            codes,
        ));
    }
    out
}

#[test]
fn no_python_reference_case_changes_verdict() {
    let rows = run_reference_cases("");
    assert_eq!(rows.len(), 9);
    let flipped: Vec<&str> = rows
        .iter()
        .filter(|(_, py, rs, _)| py != rs)
        .map(|(id, _, _, _)| id.as_str())
        .collect();
    assert_eq!(flipped, Vec::<&str>::new(), "{rows:?}");
}

/// What the same cases do if the reference memos carried the `|V:v1` marker the real builder
/// always writes. The vector memos split the day-6 settlement across two transactions, each
/// naming only part of the record, which the builder never does (it puts the FULL delta set in
/// every payment's memo), so the body check then rightly fails them. This is a measurement of the
/// stricter rule, not a change to the vectors.
#[test]
fn reference_cases_with_versioned_memos_that_flip_are_named_with_their_reason() {
    let rows = run_reference_cases("|V:v1");
    println!("versioned-memo reference run: {rows:?}");
    let flipped: Vec<(&str, &str, &str)> = rows
        .iter()
        .filter(|(_, py, rs, _)| py != rs)
        .map(|(id, py, rs, _)| (id.as_str(), py.as_str(), rs.as_str()))
        .collect();
    // The same reason for every id below, and the only one: the vector memos are not what the
    // builder writes. TX_PROOF_D6_1 names only FOD-10 while the saved day-6 record holds food -10,
    // water -12, ammo -3 (ONCHAIN_MEMO_DELTA_MISSING_KEY, twice); TX_PROOF_D6_2 and TX_PROOF_D12_1
    // list their keys in an order the builder never writes (ONCHAIN_MEMO_BODY_UNPARSEABLE). The
    // balances, conservation and pending checks of these cases are untouched.
    assert_eq!(
        flipped,
        [
            ("pass_exact_match", "PASS", "FAIL"),
            ("pass_legacy_permit_shim", "PASS", "FAIL"),
            ("inconclusive_pending_settlements", "INCONCLUSIVE", "FAIL"),
            ("inconclusive_missing_mint_snapshot", "INCONCLUSIVE", "FAIL"),
        ]
    );
    for (id, _, _, codes) in &rows {
        for c in codes {
            if c.starts_with("ONCHAIN_MEMO_DELTA") || c.starts_with("ONCHAIN_MEMO_BODY") {
                assert!(
                    [
                        "ONCHAIN_MEMO_DELTA_MISSING_KEY",
                        "ONCHAIN_MEMO_BODY_UNPARSEABLE"
                    ]
                    .contains(c),
                    "{id}: {c}"
                );
            }
        }
    }
}

// ============================================================================
// The memo parser round-trips every memo the recorded fixture holds
// ============================================================================

#[test]
fn the_parser_round_trips_every_fixture_memo_through_the_builder_format() {
    use etv_core::ledger_proof::{parse_memo_deltas, MemoBody};
    let i = clean();
    let memos = i.onchain_memos.as_ref().unwrap();
    let mut seen = 0;
    for rec in &i.settlements {
        for txid in &rec.txids {
            let memo = &memos[txid];
            match parse_memo_deltas(memo, RUN_ID, rec.day) {
                MemoBody::Deltas(d) => {
                    assert_eq!(settlement_memo_text(RUN_ID, rec.day, &d), *memo, "{txid}");
                    assert_eq!(d, rec.deltas, "{txid}");
                    seen += 1;
                }
                other => panic!("{txid}: {other:?}"),
            }
        }
    }
    assert_eq!(seen, 14);
    // An empty delta list is the builder's `DELTA:|V:v1`.
    assert_eq!(
        parse_memo_deltas("TRAIL|RUN:r|DAY:1|DELTA:|V:v1", "r", 1),
        MemoBody::Deltas(BTreeMap::new())
    );
    // A positive amount needs the explicit plus the builder writes; a bare one is refused.
    assert!(matches!(
        parse_memo_deltas("TRAIL|RUN:r|DAY:1|DELTA:FOD5|V:v1", "r", 1),
        MemoBody::Unparseable(_)
    ));
    // A duplicated code is refused rather than merged.
    assert!(matches!(
        parse_memo_deltas("TRAIL|RUN:r|DAY:1|DELTA:FOD-1,FOD-1|V:v1", "r", 1),
        MemoBody::Unparseable(_)
    ));
    // Non-ASCII where a code should be does not panic.
    assert!(matches!(
        parse_memo_deltas("TRAIL|RUN:r|DAY:1|DELTA:\u{e9}\u{e9}-1|V:v1", "r", 1),
        MemoBody::Unparseable(_)
    ));
}

// ============================================================================
// The version gate, the partial-batch rule, and the mint claim
// ============================================================================

#[test]
fn the_memo_builder_always_writes_the_version_field() {
    // The body check runs only on memos with a `|V:` field. The builder
    // (`backpack.rs::settlement_memo_text`) writes `|V:v1` on every memo, whatever the deltas.
    let cases: [&[(&str, i32)]; 4] = [
        &[],
        &[("food", -3)],
        &[("ammo", 5), ("meds", -1), ("water", 0)],
        &[("gold", 9)], // a key outside the token map is skipped but the memo is still versioned
    ];
    for deltas in cases {
        let memo = settlement_memo_text("r", 3, &map(deltas));
        assert!(memo.ends_with("|V:v1"), "{memo}");
        assert!(memo.starts_with("TRAIL|RUN:r|DAY:3|DELTA:"), "{memo}");
    }
    assert_eq!(
        settlement_memo_text("r", 3, &map(&[("gold", 9)])),
        "TRAIL|RUN:r|DAY:3|DELTA:|V:v1"
    );
}

#[test]
fn partial_batch_rule_pins_the_excused_and_the_not_excused_cases() {
    let full = "TRAIL|RUN:run-2b-seed7|DAY:14|DELTA:FOD-10,MED-2,WTR-4|V:v1";
    let with_pending = |day: i32, water: i32| {
        let mut i = clean();
        set_chain(&mut i, "TX2B_D14_FOD", full);
        set_chain(&mut i, "TX2B_D14_MED", full);
        let mut p = record(day, "Coldspring", &[("water", water)], &[]);
        p.status = "pending".to_string();
        i.pending.push(p);
        code_strings(&reconcile(&i))
    };
    // Excused: same day, same key, same amount.
    assert_eq!(with_pending(14, -4), ["SETTLEMENTS_PENDING"]);
    // Not excused: same day, different amount.
    assert_eq!(
        with_pending(14, -3),
        [
            "ONCHAIN_MEMO_DELTA_EXTRA_KEY",
            "ONCHAIN_MEMO_DELTA_EXTRA_KEY",
            "SETTLEMENTS_PENDING"
        ]
    );
    // Not excused: same amount, different day.
    assert_eq!(
        with_pending(15, -4),
        [
            "ONCHAIN_MEMO_DELTA_EXTRA_KEY",
            "ONCHAIN_MEMO_DELTA_EXTRA_KEY",
            "SETTLEMENTS_PENDING"
        ]
    );
}

#[test]
fn a_lone_mint_edit_still_breaks_conservation() {
    // The save's mint claim is not checked against the chain directly (no mint evidence is
    // available), but minted + chain-verified deltas must equal the ledger balance.
    let mut i = clean();
    i.minted_initial.insert("food".to_string(), 48);
    let r = reconcile(&i);
    assert_eq!(code_strings(&r), ["CONSERVATION_BROKEN"]);
    assert_eq!(r.notes, ["food: minted 48 + deltas -19 (29) != settled 31"]);
    assert!(!r.passed);
}
