//! Phase 2, Part B integration tests: the Ledger Backpack's pure settlement logic and its
//! reconciliation proof.
//!
//! 1. Memo grammar, hex, IOU balance parsing and the mint snapshot, against hand-worked literals
//!    (the memo strings were also checked against the frozen Python `_settlement_memo_text`).
//! 2. Settlement planning and the double-settlement guards, each test named after its legacy
//!    comment (ledger-CRIT-2, ledger-003, ENG-A-08, F-86a4c19c).
//! 3. `RecordedTransport` fixture-shape tests: every field the Rust code reads must exist in the
//!    fixture, and a read of an absent field fails.
//! 4. The reconciliation proof against the Python reference vectors
//!    (`rust/oracle/vectors/ledger_reference.json.gz`, `proof_cases`), one test per failure kind,
//!    and a 200-mutation tamper sweep.
//! 5. A deterministic engine run (narrator off, seed 7) with the backpack on, settled through
//!    `RecordedTransport`, that reconciles cleanly; backpack off identical to today; save schema v4
//!    carries the settlement records unchanged.
//!
//! No test opens a socket. Fixture addresses are obviously fake.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use serde_json::{json, Value};

use etv_core::backpack::{
    balance_to_int, build_memo, compute_deltas, encode_minted_txid, fetch_balances,
    fetch_onchain_memos, hex_decode, hex_encode, missing_reads, parse_account_lines,
    plan_settlement, read_required, retry_pending, settle_checkpoint, settlement_memo_text,
    stamp_minted_snapshot, strip_unknown_pending_keys, FetchError, LedgerTransport, PaymentAttempt,
    PaymentDirection, PlanOutcome, ReadLog, RecordedTransport, SettleStatus, SettlementBatch,
    ShapeError, TransportError, MAX_ACCOUNT_TX_PAGES,
};
use etv_core::engine::StepEngine;
use etv_core::ledger_proof::{
    is_inconclusive, proof_verdict, prove_player_save, read_ledger_view, reconcile,
    report_to_value, FailureCode, LedgerView, ProofVerdict, ReconcileInput,
};
use etv_core::model::{
    BackpackState, GamePhase, IntentAction, PlayerIntent, RunState, SettlementRecord, SuppliesState,
};
use etv_core::save::{state_to_value, value_to_state};
use etv_core::worldgen::create_new_run;

const PLAYER: &str = "rTestPlayerWallet111111111111";
const ISSUER: &str = "rTestIssuerWallet222222222222";
const RUN_ID: &str = "run-2b-seed7";

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
    let mut s = String::new();
    GzDecoder::new(file)
        .read_to_string(&mut s)
        .unwrap_or_else(|e| panic!("failed to gunzip {}: {e}", full.display()));
    serde_json::from_str(&s).expect("vector JSON parses")
}

fn load_fixture() -> Value {
    let p =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xrpl_recorded_run_seed7.json");
    let txt = std::fs::read_to_string(&p).expect("fixture readable");
    serde_json::from_str(&txt).expect("fixture parses")
}

fn map(pairs: &[(&str, i32)]) -> BTreeMap<String, i32> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn supplies(pairs: &[(&str, i32)]) -> SuppliesState {
    let mut s = SuppliesState::default();
    for (k, v) in pairs {
        s.set(k, *v);
    }
    s
}

fn enabled_backpack(last_settled: &[(&str, i32)]) -> BackpackState {
    BackpackState {
        enabled: true,
        wallet_address: PLAYER.to_string(),
        issuer_address: ISSUER.to_string(),
        trust_lines_ready: true,
        last_settled_supplies: map(last_settled),
        ..BackpackState::default()
    }
}

// ============================================================================
// 1. Memo grammar, hex, balances, mint snapshot
// ============================================================================

#[test]
fn memo_grammar_matches_legacy_build_memo() {
    // Literals worked by hand from backpack.py:143-168 and confirmed against the frozen Python
    // `_settlement_memo_text`: sorted by game key, CODE then signed number, explicit '+', then |V:v1.
    let d = map(&[("food", -10), ("water", -12), ("ammo", -3)]);
    assert_eq!(
        settlement_memo_text("r1", 6, &d),
        "TRAIL|RUN:r1|DAY:6|DELTA:AMO-3,FOD-10,WTR-12|V:v1"
    );
    // A positive delta carries '+', a zero delta none, an unknown key is skipped.
    let d = map(&[("food", 4), ("gold", 9), ("meds", 0)]);
    assert_eq!(
        settlement_memo_text("r1", 7, &d),
        "TRAIL|RUN:r1|DAY:7|DELTA:FOD+4,MED0|V:v1"
    );
    assert_eq!(
        settlement_memo_text("r", 1, &BTreeMap::new()),
        "TRAIL|RUN:r|DAY:1|DELTA:|V:v1"
    );
    // The on-chain Memo: MemoType is hex("text/plain"); MemoData is the hex of the text.
    let m = build_memo("r1", 6, &map(&[("food", -10)]));
    assert_eq!(m.memo_type, "746578742F706C61696E");
    assert_eq!(
        hex_decode(&m.memo_data).unwrap(),
        "TRAIL|RUN:r1|DAY:6|DELTA:FOD-10|V:v1"
    );
    assert!(m
        .memo_data
        .starts_with("545241494C7C52554E3A72317C4441593A36"));
}

#[test]
fn hex_round_trips_and_rejects_bad_input() {
    assert_eq!(hex_encode("A|z"), "417C7A");
    assert_eq!(hex_decode("417C7A").unwrap(), "A|z");
    assert_eq!(hex_decode("417c7a").unwrap(), "A|z");
    assert!(hex_decode("417C7").is_err(), "odd length");
    assert!(hex_decode("41ZZ").is_err(), "non-hex digit");
    assert!(hex_decode("FF").is_err(), "invalid utf-8");
    assert_eq!(hex_decode("").unwrap(), "");
}

#[test]
fn iou_balance_parses_exactly_and_refuses_a_fraction() {
    for (s, want) in [
        ("34", 34),
        ("34.0", 34),
        ("-5", -5),
        ("0", 0),
        ("0.00", 0),
        ("1e2", 100),
    ] {
        assert_eq!(balance_to_int(s), Ok(want), "{s}");
    }
    for bad in ["34.5", "abc", "", "3e-1", "2147483648", "1.2.3", "--1"] {
        assert!(balance_to_int(bad).is_err(), "{bad:?} must not parse");
    }
}

#[test]
fn mint_snapshot_stamps_a_reserved_permit_and_only_when_complete() {
    let mut bp = BackpackState::default();
    let snap = map(&[
        ("food", 50),
        ("water", 50),
        ("meds", 5),
        ("ammo", 20),
        ("parts", 3),
    ]);
    stamp_minted_snapshot(&mut bp, Some(&snap));
    assert_eq!(bp.minted_initial, snap);
    assert_eq!(bp.permits.len(), 1);
    assert_eq!(bp.permits[0].permit_id, "__minted_initial__");
    assert!(bp.permits[0].used);
    assert_eq!(
        bp.permits[0].txid,
        "ammo=20,food=50,meds=5,parts=3,water=50"
    );
    assert_eq!(encode_minted_txid(&snap), bp.permits[0].txid);
    // Re-stamping replaces, never duplicates.
    stamp_minted_snapshot(&mut bp, Some(&snap));
    assert_eq!(bp.permits.len(), 1);
    // The model's reader hydrates from the shim alone.
    let mut legacy = BackpackState {
        permits: bp.permits.clone(),
        ..BackpackState::default()
    };
    assert_eq!(legacy.minted_snapshot_of(), snap);
    // A half-minted pack stamps nothing.
    let mut half = BackpackState::default();
    stamp_minted_snapshot(&mut half, Some(&map(&[("food", 1)])));
    assert!(half.permits.is_empty() && half.minted_initial.is_empty());
}

// ============================================================================
// 2. Planning and the double-settlement guards
// ============================================================================

#[test]
fn plan_computes_deltas_and_directions_and_is_pure() {
    let bp = enabled_backpack(&[
        ("food", 50),
        ("water", 50),
        ("meds", 5),
        ("ammo", 20),
        ("parts", 4),
    ]);
    let before = bp.clone();
    let sup = supplies(&[
        ("food", 45),
        ("water", 50),
        ("meds", 11),
        ("ammo", 15),
        ("parts", 1),
    ]);
    let PlanOutcome::Batch(b) = plan_settlement(&bp, &sup, "run-x", 11, "Dunhollow") else {
        panic!("expected a batch");
    };
    assert_eq!(bp, before, "planning must not mutate the backpack");
    assert_eq!(
        b.deltas,
        map(&[("ammo", -5), ("food", -5), ("meds", 6), ("parts", -3)])
    );
    assert_eq!(
        b.memo_text,
        "TRAIL|RUN:run-x|DAY:11|DELTA:AMO-5,FOD-5,MED+6,PRT-3|V:v1"
    );
    let keys: Vec<&str> = b.payments.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["ammo", "food", "meds", "parts"]);
    let meds = &b.payments[2];
    assert_eq!(meds.direction, PaymentDirection::IssuerToPlayer);
    assert_eq!(
        (meds.account.as_str(), meds.destination.as_str()),
        (ISSUER, PLAYER)
    );
    assert_eq!((meds.currency.as_str(), meds.value), ("MED", 6));
    let ammo = &b.payments[0];
    assert_eq!(ammo.direction, PaymentDirection::PlayerToIssuer);
    assert_eq!(
        (ammo.account.as_str(), ammo.destination.as_str()),
        (PLAYER, ISSUER)
    );
    assert_eq!(ammo.value, 5);
    assert!(b
        .payments
        .iter()
        .all(|p| p.memo == b.memo && p.issuer == ISSUER));
}

#[test]
fn plan_not_enabled_and_no_changes() {
    let sup = supplies(&[("food", 5)]);
    let off = BackpackState::default();
    assert_eq!(
        plan_settlement(&off, &sup, "r", 1, "x"),
        PlanOutcome::NotEnabled
    );
    let mut no_wallet = enabled_backpack(&[]);
    no_wallet.wallet_address.clear();
    assert_eq!(
        plan_settlement(&no_wallet, &sup, "r", 1, "x"),
        PlanOutcome::NotEnabled
    );
    let on = enabled_backpack(&[("food", 5)]);
    assert_eq!(
        plan_settlement(&on, &sup, "r", 1, "x"),
        PlanOutcome::NoChanges
    );
    assert_eq!(compute_deltas(&sup, &map(&[("food", 5)])), BTreeMap::new());
    // A missing baseline counts as 0.
    assert_eq!(compute_deltas(&sup, &BTreeMap::new()), map(&[("food", 5)]));
}

/// Scripted transport for guard tests: pops one result per payment; `Err` stops the batch.
struct Scripted {
    results: VecDeque<Result<String, String>>,
    submitted: Vec<SettlementBatch>,
}

impl Scripted {
    fn new(results: Vec<Result<&str, &str>>) -> Self {
        Self {
            results: results
                .into_iter()
                .map(|r| r.map(str::to_string).map_err(str::to_string))
                .collect(),
            submitted: Vec::new(),
        }
    }
}

impl LedgerTransport for Scripted {
    fn balances(&mut self, a: &str) -> Result<Value, TransportError> {
        Err(TransportError::NoRecording(a.to_string()))
    }
    fn submit_batch(&mut self, batch: &SettlementBatch) -> Vec<PaymentAttempt> {
        self.submitted.push(batch.clone());
        let mut out = Vec::new();
        for p in &batch.payments {
            let r = self
                .results
                .pop_front()
                .unwrap_or(Err("script exhausted".into()));
            let failed = r.is_err();
            out.push(PaymentAttempt {
                key: p.key.clone(),
                result: r
                    .map(|h| json!({ "hash": h }))
                    .map_err(TransportError::Unreachable),
            });
            if failed {
                break;
            }
        }
        out
    }
    fn tx_memos(&mut self, a: &str, _m: Option<&Value>) -> Result<Value, TransportError> {
        Err(TransportError::NoRecording(a.to_string()))
    }
}

fn settle(
    bp: &mut BackpackState,
    sup: &SuppliesState,
    day: i32,
    t: &mut Scripted,
) -> etv_core::backpack::SettleReport {
    settle_checkpoint(
        bp,
        sup,
        "run-x",
        day,
        "Town",
        "T",
        t,
        &mut ReadLog::default(),
    )
}

#[test]
fn ledger_crit_2_partial_batch_folds_confirmed_keys_and_queues_only_the_rest() {
    // ammo, food, meds all change; ammo clears, food then fails, meds is never attempted.
    let mut bp = enabled_backpack(&[("ammo", 20), ("food", 50), ("meds", 5)]);
    let sup = supplies(&[("ammo", 15), ("food", 45), ("meds", 11)]);
    let mut t = Scripted::new(vec![Ok("TX_AMMO"), Err("blip")]);
    let r = settle(&mut bp, &sup, 11, &mut t);
    assert!(matches!(r.status, SettleStatus::Pending { .. }));
    // The confirmed key is folded into the baseline with its real txid, never discarded.
    assert_eq!(bp.last_settled_supplies["ammo"], 15);
    assert_eq!(
        bp.last_settled_supplies["food"], 50,
        "unconfirmed key must not advance"
    );
    assert_eq!(bp.last_settled_supplies["meds"], 5);
    assert_eq!(bp.settlements.len(), 1);
    assert_eq!(bp.settlements[0].deltas, map(&[("ammo", -5)]));
    assert_eq!(bp.settlements[0].txids, ["TX_AMMO"]);
    assert_eq!(bp.settlements[0].status, "settled");
    // Only the keys that did NOT clear are queued pending.
    assert_eq!(bp.pending_settlements.len(), 1);
    assert_eq!(
        bp.pending_settlements[0].deltas,
        map(&[("food", -5), ("meds", 6)])
    );
    assert_eq!(bp.pending_settlements[0].status, "pending");
    assert!(bp.last_settle_failed);
    assert_eq!(
        bp.last_settlement_day, 0,
        "day only advances on full success"
    );
}

#[test]
fn ledger_crit_2_retry_never_resubmits_a_confirmed_key() {
    let mut bp = enabled_backpack(&[("ammo", 20), ("food", 50), ("meds", 5)]);
    let sup = supplies(&[("ammo", 15), ("food", 45), ("meds", 11)]);
    let mut t = Scripted::new(vec![Ok("TX_AMMO"), Err("blip")]);
    settle(&mut bp, &sup, 11, &mut t);
    // Retry: food clears, meds fails. Only meds may stay pending.
    let mut t2 = Scripted::new(vec![Ok("TX_FOOD"), Err("blip again")]);
    let moved = retry_pending(&mut bp, "run-x", "T", &mut t2, &mut ReadLog::default());
    assert_eq!(moved, 1);
    let retried: Vec<&str> = t2.submitted[0]
        .payments
        .iter()
        .map(|p| p.key.as_str())
        .collect();
    assert_eq!(
        retried,
        ["food", "meds"],
        "ammo already cleared and must not be retried"
    );
    assert_eq!(bp.pending_settlements.len(), 1);
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("meds", 6)]));
    assert_eq!(bp.last_settled_supplies["food"], 45);
    // A third pass resubmits meds only.
    let mut t3 = Scripted::new(vec![Ok("TX_MEDS")]);
    retry_pending(&mut bp, "run-x", "T", &mut t3, &mut ReadLog::default());
    let again: Vec<&str> = t3.submitted[0]
        .payments
        .iter()
        .map(|p| p.key.as_str())
        .collect();
    assert_eq!(again, ["meds"]);
    assert!(bp.pending_settlements.is_empty());
    assert!(!bp.last_settle_failed);
    assert_eq!(
        bp.last_settled_supplies,
        map(&[("ammo", 15), ("food", 45), ("meds", 11)])
    );
}

#[test]
fn ledger_003_stored_memo_equals_the_onchain_memo_bytes_for_the_full_batch() {
    let mut bp = enabled_backpack(&[("ammo", 20), ("food", 50), ("meds", 5)]);
    let sup = supplies(&[("ammo", 15), ("food", 45), ("meds", 11)]);
    // Partial failure: the receipt still names the FULL batch, as every Payment carried it.
    let mut t = Scripted::new(vec![Ok("TX_AMMO"), Err("blip")]);
    settle(&mut bp, &sup, 11, &mut t);
    let full = "TRAIL|RUN:run-x|DAY:11|DELTA:AMO-5,FOD-5,MED+6|V:v1";
    assert_eq!(bp.settlements[0].memo, full);
    assert_eq!(
        hex_decode(&t.submitted[0].payments[0].memo.memo_data).unwrap(),
        full
    );
    // The pending record's stored memo names only what is still pending.
    assert_eq!(
        bp.pending_settlements[0].memo,
        "TRAIL|RUN:run-x|DAY:11|DELTA:FOD-5,MED+6|V:v1"
    );
    // On retry the on-chain memo is the record's pre-narrowing set, and the receipt matches it.
    let mut t2 = Scripted::new(vec![Ok("TX_FOOD"), Err("blip")]);
    retry_pending(&mut bp, "run-x", "T", &mut t2, &mut ReadLog::default());
    let onchain = hex_decode(&t2.submitted[0].payments[0].memo.memo_data).unwrap();
    assert_eq!(onchain, "TRAIL|RUN:run-x|DAY:11|DELTA:FOD-5,MED+6|V:v1");
    assert_eq!(bp.settlements[1].memo, onchain);
    assert_eq!(
        bp.pending_settlements[0].memo,
        "TRAIL|RUN:run-x|DAY:11|DELTA:MED+6|V:v1"
    );
}

#[test]
fn eng_a_08_retried_delta_folds_into_baseline_so_the_next_settle_is_empty() {
    let mut bp = enabled_backpack(&[("food", 50)]);
    let sup = supplies(&[("food", 45)]);
    let mut t = Scripted::new(vec![Err("down")]);
    let r = settle(&mut bp, &sup, 11, &mut t);
    assert!(matches!(r.status, SettleStatus::Pending { .. }));
    assert_eq!(bp.last_settled_supplies["food"], 50);
    // Back online: the retry pays the pending delta and folds it into the baseline, so the fresh
    // settle measures nothing new and does not pay it a second time.
    let mut t2 = Scripted::new(vec![Ok("TX_RETRY")]);
    let r2 = settle(&mut bp, &sup, 12, &mut t2);
    assert_eq!(r2.retried_moved, 1);
    assert_eq!(r2.status, SettleStatus::NoChanges);
    assert_eq!(
        t2.submitted.len(),
        1,
        "exactly one batch (the retry) was submitted"
    );
    assert_eq!(bp.last_settled_supplies["food"], 45);
    assert_eq!(bp.settlements.len(), 1);
}

#[test]
fn double_settlement_replan_after_confirm_is_empty() {
    let mut bp = enabled_backpack(&[("food", 50), ("water", 50)]);
    let sup = supplies(&[("food", 45), ("water", 40)]);
    let mut t = Scripted::new(vec![Ok("TX_F"), Ok("TX_W")]);
    let r = settle(&mut bp, &sup, 11, &mut t);
    assert_eq!(r.status, SettleStatus::Settled);
    assert_eq!(bp.last_settlement_day, 11);
    assert!(!bp.last_settle_failed);
    // Settling the same supplies again submits nothing.
    let mut t2 = Scripted::new(vec![]);
    let r2 = settle(&mut bp, &sup, 11, &mut t2);
    assert_eq!(r2.status, SettleStatus::NoChanges);
    assert!(t2.submitted.is_empty());
    assert_eq!(bp.settlements.len(), 1);
}

#[test]
fn double_settlement_replayed_txid_is_refused_and_not_folded() {
    let mut bp = enabled_backpack(&[("food", 50), ("water", 50)]);
    let sup = supplies(&[("food", 45), ("water", 40)]);
    // A stale transport answers both payments with the same hash.
    let mut t = Scripted::new(vec![Ok("TX_SAME"), Ok("TX_SAME")]);
    let r = settle(&mut bp, &sup, 11, &mut t);
    assert!(
        matches!(&r.status, SettleStatus::Pending { reason } if reason.contains("duplicate txid"))
    );
    assert_eq!(bp.last_settled_supplies["food"], 45, "first payment folded");
    assert_eq!(
        bp.last_settled_supplies["water"], 50,
        "replayed hash must not fold a second key"
    );
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("water", -10)]));
    // A txid already in the log is refused on a later batch too.
    let mut t2 = Scripted::new(vec![Ok("TX_SAME")]);
    retry_pending(&mut bp, "run-x", "T", &mut t2, &mut ReadLog::default());
    assert_eq!(bp.last_settled_supplies["water"], 50);
    assert_eq!(bp.pending_settlements.len(), 1);
}

#[test]
fn f_86a4c19c_unknown_pending_keys_are_stripped_and_empty_records_dropped() {
    let mut bp = enabled_backpack(&[]);
    bp.pending_settlements.push(SettlementRecord {
        day: 3,
        deltas: map(&[("gold", 9), ("food", -2)]),
        ..SettlementRecord::default()
    });
    bp.pending_settlements.push(SettlementRecord {
        day: 4,
        deltas: map(&[("gold", 1)]),
        ..SettlementRecord::default()
    });
    assert!(strip_unknown_pending_keys(&mut bp));
    assert_eq!(bp.pending_settlements.len(), 1);
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("food", -2)]));
    assert!(!strip_unknown_pending_keys(&mut bp));
}

#[test]
fn a_transport_that_returns_too_few_or_misordered_results_is_a_failure_not_a_fold() {
    struct Bad(bool);
    impl LedgerTransport for Bad {
        fn balances(&mut self, a: &str) -> Result<Value, TransportError> {
            Err(TransportError::NoRecording(a.to_string()))
        }
        fn submit_batch(&mut self, b: &SettlementBatch) -> Vec<PaymentAttempt> {
            if self.0 {
                return Vec::new();
            }
            // Right results, wrong order.
            b.payments
                .iter()
                .rev()
                .map(|p| PaymentAttempt {
                    key: p.key.clone(),
                    result: Ok(json!({"hash": format!("TX_{}", p.key)})),
                })
                .collect()
        }
        fn tx_memos(&mut self, a: &str, _m: Option<&Value>) -> Result<Value, TransportError> {
            Err(TransportError::NoRecording(a.to_string()))
        }
    }
    for short in [true, false] {
        let mut bp = enabled_backpack(&[("food", 50), ("water", 50)]);
        let sup = supplies(&[("food", 45), ("water", 40)]);
        let r = settle_checkpoint(
            &mut bp,
            &sup,
            "run-x",
            1,
            "T",
            "",
            &mut Bad(short),
            &mut ReadLog::default(),
        );
        assert!(matches!(r.status, SettleStatus::Pending { .. }));
        assert_eq!(
            bp.last_settled_supplies,
            map(&[("food", 50), ("water", 50)])
        );
        assert_eq!(bp.pending_settlements[0].deltas.len(), 2);
    }
}

// ============================================================================
// 3. RecordedTransport and response shapes
// ============================================================================

#[test]
fn recorded_fixture_reads_only_fields_that_exist_and_the_read_set_is_pinned() {
    let doc = load_fixture();
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let mut log = ReadLog::default();
    let bal = fetch_balances(&mut t, PLAYER, ISSUER, &mut log).unwrap();
    // The extra non-issuer FOD line (999) is filtered out.
    assert_eq!(
        bal,
        map(&[
            ("AMO", 10),
            ("FOD", 31),
            ("MED", 9),
            ("PRT", 0),
            ("WTR", 39)
        ])
    );
    let memos = fetch_onchain_memos(&mut t, PLAYER, ISSUER, &mut log).unwrap();
    assert_eq!(
        memos.len(),
        14,
        "14 settlement memos; 5 memo-less mint payments are skipped"
    );
    // A submit read too, so `submit.hash` is in the log.
    let sub = doc["submit"][0]["result"].clone();
    etv_core::backpack::parse_submit_result(&sub, &mut log);

    let (req_missing, opt_missing) = missing_reads(&log);
    assert!(
        req_missing.is_empty(),
        "required field absent from fixture: {req_missing:?}"
    );
    // Optional reads that are legitimately absent somewhere (the other shape of the entry, a
    // transaction without a memo, the last page's marker).
    let allowed: BTreeSet<String> = [
        "account_tx.transactions[].tx",
        "account_tx.transactions[].hash",
        "account_tx.transactions[].tx.Memos",
        "account_tx.marker",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert!(
        opt_missing.is_subset(&allowed),
        "unexpected absent optional read: {opt_missing:?}"
    );

    // Every optional read must be found at least once, so a mistyped field name cannot hide.
    for r in log.reads.iter().filter(|r| r.optional) {
        assert!(
            log.reads.iter().any(|o| o.path == r.path && o.found),
            "optional field {} never found in the fixture",
            r.path
        );
    }
    // The complete set of paths the code reads. Adding a read to a parser changes this set and
    // fails here until the fixture is updated and the new path is listed on purpose.
    let paths: BTreeSet<&str> = log.reads.iter().map(|r| r.path.as_str()).collect();
    let expected: BTreeSet<&str> = [
        "account_lines.lines",
        "account_lines.lines[].account",
        "account_lines.lines[].balance",
        "account_lines.lines[].currency",
        "account_tx.marker",
        "account_tx.transactions",
        "account_tx.transactions[].hash",
        "account_tx.transactions[].tx",
        "account_tx.transactions[].tx.Memos",
        "account_tx.transactions[].tx.Memos[0].Memo.MemoData",
        "account_tx.transactions[].tx.hash",
        "account_tx.transactions[].tx_json",
        "submit.hash",
    ]
    .into_iter()
    .collect();
    assert_eq!(paths, expected);
}

#[test]
fn reading_a_field_the_fixture_lacks_fails() {
    let doc = load_fixture();
    // A parser that reads a field the real client never reads (`limit`) fails on the fixture.
    let result = &doc["account_lines"][PLAYER];
    let mut log = ReadLog::default();
    let line = &result["lines"][0];
    assert_eq!(
        read_required(line, "limit", "account_lines.lines[].limit", &mut log).map(|_| ()),
        Err(ShapeError::MissingField(
            "account_lines.lines[].limit".to_string()
        ))
    );
    // And removing a field the code does read makes the read fail, one field at a time.
    for field in ["balance", "currency", "account"] {
        let mut d = doc.clone();
        for l in d["account_lines"][PLAYER]["lines"].as_array_mut().unwrap() {
            l.as_object_mut().unwrap().remove(field);
        }
        let mut t = RecordedTransport::from_fixture(&d).unwrap();
        let err = fetch_balances(&mut t, PLAYER, ISSUER, &mut ReadLog::default()).unwrap_err();
        assert_eq!(
            err,
            FetchError::Shape(ShapeError::MissingField(format!(
                "account_lines.lines[].{field}"
            )))
        );
    }
    let mut d = doc.clone();
    d["account_lines"][PLAYER]
        .as_object_mut()
        .unwrap()
        .remove("lines");
    assert!(parse_account_lines(&d["account_lines"][PLAYER], &mut ReadLog::default()).is_err());
    // A transaction with neither `tx` nor `tx_json` is a shape error.
    let mut d = doc.clone();
    let first = &mut d["account_tx"][2]["result"]["transactions"][0];
    first.as_object_mut().unwrap().remove("tx");
    let mut t = RecordedTransport::from_fixture(&d).unwrap();
    assert!(matches!(
        fetch_onchain_memos(&mut t, "", ISSUER, &mut ReadLog::default()),
        Err(FetchError::Shape(ShapeError::MissingField(_)))
    ));
}

#[test]
fn recorded_transport_refuses_calls_it_has_no_recording_for() {
    let doc = load_fixture();
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    assert!(matches!(
        t.balances("rNobody"),
        Err(TransportError::NoRecording(_))
    ));
    assert!(matches!(
        t.tx_memos(PLAYER, Some(&json!({"ledger": 1}))),
        Err(TransportError::NoRecording(_))
    ));
    // A payment that differs in any field (here: value) is not replayed.
    let bp = enabled_backpack(&[("food", 50)]);
    let PlanOutcome::Batch(mut b) =
        plan_settlement(&bp, &supplies(&[("food", 45)]), RUN_ID, 11, "x")
    else {
        panic!()
    };
    b.payments[0].value = 4;
    let attempts = t.submit_batch(&b);
    assert!(matches!(
        attempts[0].result,
        Err(TransportError::NoRecording(_))
    ));
    assert_eq!(t.unconsumed_submits(), 14, "nothing was consumed");
}

#[test]
fn account_tx_pagination_follows_the_marker_and_is_capped() {
    let doc = load_fixture();
    // The fixture pages the player account over two pages; the issuer page is not needed here.
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let memos = fetch_onchain_memos(&mut t, PLAYER, "", &mut ReadLog::default()).unwrap();
    assert_eq!(
        t.calls.len(),
        2,
        "two pages: first with no marker, then with the marker"
    );
    assert_eq!(memos.len(), 14);
    // A server that echoes a marker forever is stopped at the page cap.
    let echo = json!({"ledger": 1});
    let loop_doc = json!({"account_tx": [
        {"account": PLAYER, "marker": null, "result": {"transactions": [], "marker": echo}},
        {"account": PLAYER, "marker": echo, "result": {"transactions": [], "marker": echo}}
    ]});
    let mut t = RecordedTransport::from_fixture(&loop_doc).unwrap();
    let memos = fetch_onchain_memos(&mut t, PLAYER, "", &mut ReadLog::default()).unwrap();
    assert!(memos.is_empty());
    assert_eq!(t.calls.len(), MAX_ACCOUNT_TX_PAGES);
    assert_eq!(MAX_ACCOUNT_TX_PAGES, 100);
}

#[test]
fn a_non_integer_onledger_balance_is_left_out_so_the_proof_reports_it_missing() {
    let doc = json!({"account_lines": {PLAYER: {"lines": [
        {"account": ISSUER, "currency": "FOD", "balance": "31.5"},
        {"account": ISSUER, "currency": "WTR", "balance": "39"}
    ]}}});
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let bal = fetch_balances(&mut t, PLAYER, ISSUER, &mut ReadLog::default()).unwrap();
    assert_eq!(bal, map(&[("WTR", 39)]));
}

// ============================================================================
// 4. The proof: Python reference vectors, failure kinds, tamper sweep
// ============================================================================

/// Base state of the Python generator's proof cases (`generate_vectors.py:2098-2147`), with the
/// run id taken from the vector so the memo strings line up.
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

fn vector_memos(run_id: &str) -> BTreeMap<String, String> {
    [
        (
            "TX_PROOF_D6_1",
            format!("TRAIL|RUN:{run_id}|DAY:6|DELTA:FOD-10"),
        ),
        (
            "TX_PROOF_D6_2",
            format!("TRAIL|RUN:{run_id}|DAY:6|DELTA:WTR-12,AMO-3"),
        ),
        (
            "TX_PROOF_D12_1",
            format!("TRAIL|RUN:{run_id}|DAY:12|DELTA:FOD-6,WTR-4,MED-2,AMO-2,PRT-1"),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[test]
fn proof_matches_every_python_reference_proof_case() {
    let doc = load_gzip_json("rust/oracle/vectors/ledger_reference.json.gz");
    let cases = doc["proof_cases"].as_array().expect("proof_cases");
    assert_eq!(cases.len(), 9);
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

    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let seed = case["seed"].as_i64().unwrap() as i128;
        let run_id = case["report_dict"]["run_id"]
            .as_str()
            .unwrap_or("none")
            .to_string();
        let mut bp = vector_base(&run_id);
        let mut view = LedgerView {
            balances: Some(matching.clone()),
            onchain_memos: Some(vector_memos(&run_id)),
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
        match id {
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
        assert_eq!(
            res.verdict.as_str(),
            case["verdict"].as_str().unwrap(),
            "{id}: verdict"
        );
        let want_notes: Vec<String> = case["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap().to_string())
            .collect();
        assert_eq!(res.notes, want_notes, "{id}: notes");
        if case["report_dict"].is_null() {
            assert!(res.report.is_none(), "{id}: no report");
            continue;
        }
        let report = res.report.as_ref().expect("report");
        assert_eq!(
            report_to_value(report),
            case["report_dict"],
            "{id}: report_dict"
        );
        // The vector's `passed` is the player-save result (verdict == PASS), not report.passed.
        assert_eq!(
            res.verdict == ProofVerdict::Pass,
            case["passed"].as_bool().unwrap(),
            "{id}: passed"
        );
    }
}

/// A clean reconciliation input and the engine state it came from.
struct Clean {
    bp: BackpackState,
    input: ReconcileInput,
    state: RunState,
}

#[test]
fn failure_codes_are_distinct_and_stable() {
    let all = [
        (FailureCode::LedgerBalanceMissing, "LEDGER_BALANCE_MISSING"),
        (
            FailureCode::LedgerBalanceMismatch,
            "LEDGER_BALANCE_MISMATCH",
        ),
        (FailureCode::ConservationBroken, "CONSERVATION_BROKEN"),
        (
            FailureCode::MemoLocalPrefixMismatch,
            "MEMO_LOCAL_PREFIX_MISMATCH",
        ),
        (FailureCode::SettlementNoTxids, "SETTLEMENT_NO_TXIDS"),
        (
            FailureCode::SettlementMissingOnLedger,
            "SETTLEMENT_MISSING_ON_LEDGER",
        ),
        (
            FailureCode::OnchainMemoPrefixMismatch,
            "ONCHAIN_MEMO_PREFIX_MISMATCH",
        ),
        (FailureCode::DoubleSettlementTxid, "DOUBLE_SETTLEMENT_TXID"),
        (FailureCode::SettlementsPending, "SETTLEMENTS_PENDING"),
    ];
    let strings: BTreeSet<&str> = all
        .iter()
        .map(|(c, s)| {
            assert_eq!(c.as_str(), *s);
            *s
        })
        .collect();
    assert_eq!(strings.len(), 9);
}

fn codes(input: &ReconcileInput) -> BTreeSet<FailureCode> {
    reconcile(input).codes()
}

fn set(cs: &[FailureCode]) -> BTreeSet<FailureCode> {
    cs.iter().copied().collect()
}

#[test]
fn proof_clean_input_reports_no_failure() {
    let c = clean_run();
    let r = reconcile(&c.input);
    assert!(
        r.passed && r.failures.is_empty() && r.notes.is_empty(),
        "{:?}",
        r.notes
    );
    assert_eq!(proof_verdict(&r, false, false), ProofVerdict::Pass);
}

#[test]
fn proof_failure_tampered_balance_gives_its_own_code() {
    let mut i = clean_run().input;
    *i.ledger_balances.get_mut("FOD").unwrap() -= 1;
    assert_eq!(codes(&i), set(&[FailureCode::LedgerBalanceMismatch]));
    let r = reconcile(&i);
    assert_eq!(r.notes, ["food: ledger 30 != engine settled 31 (FOD)"]);
    assert_eq!(proof_verdict(&r, false, false), ProofVerdict::Fail);
    // A balance that is absent altogether is a different code.
    let mut i = clean_run().input;
    i.ledger_balances.remove("WTR");
    assert_eq!(codes(&i), set(&[FailureCode::LedgerBalanceMissing]));
}

#[test]
fn proof_failure_missing_settlement_gives_its_own_code() {
    // The ledger holds no memo for a txid the engine claims it settled.
    let mut i = clean_run().input;
    i.onchain_memos.as_mut().unwrap().remove("TX2B_D14_FOD");
    assert_eq!(codes(&i), set(&[FailureCode::SettlementMissingOnLedger]));
    // A settled record that carries no txid at all cannot be backed by anything on the ledger.
    let mut i = clean_run().input;
    i.settlements[1].txids.clear();
    assert_eq!(codes(&i), set(&[FailureCode::SettlementNoTxids]));
    // Engine log lost a settlement the ledger kept: conservation breaks.
    let mut i = clean_run().input;
    i.settlements.remove(0);
    assert_eq!(codes(&i), set(&[FailureCode::ConservationBroken]));
}

#[test]
fn proof_failure_wrong_memo_prefix_gives_its_own_code() {
    let mut i = clean_run().input;
    i.onchain_memos.as_mut().unwrap().insert(
        "TX2B_D11_AMO".into(),
        "TRAIL|RUN:other-run|DAY:11|DELTA:AMO-5|V:v1".into(),
    );
    assert_eq!(codes(&i), set(&[FailureCode::OnchainMemoPrefixMismatch]));
    // The engine-stored memo is checked separately, as local consistency only.
    let mut i = clean_run().input;
    i.settlements[0].memo = "TRAIL|RUN:other-run|DAY:11|DELTA:".into();
    assert_eq!(codes(&i), set(&[FailureCode::MemoLocalPrefixMismatch]));
    assert!(!reconcile(&i).passed, "a corrupt stored memo must not pass");
}

#[test]
fn proof_memo_prefix_requires_a_field_boundary() {
    // Python's bare startswith would accept an on-chain day-11 memo for a record that says day 1:
    // "...|DAY:1" is a string prefix of "...|DAY:11|...". Here the next character must be '|'.
    let mut i = clean_run().input;
    assert_eq!(i.settlements[0].day, 11);
    i.settlements[0].day = 1;
    i.settlements[0].memo = i.settlements[0].memo.replacen("DAY:11", "DAY:1", 1);
    assert_eq!(codes(&i), set(&[FailureCode::OnchainMemoPrefixMismatch]));
}

#[test]
fn proof_failure_double_settlement_gives_its_own_code() {
    let mut i = clean_run().input;
    let last = i.settlements.last().unwrap().clone();
    i.settlements.push(last); // the same settlement recorded twice
    assert_eq!(
        codes(&i),
        set(&[
            FailureCode::DoubleSettlementTxid,
            FailureCode::ConservationBroken
        ])
    );
    let r = reconcile(&i);
    assert!(r.double_settlement && !r.passed);
    assert!(!is_inconclusive(&r));
}

#[test]
fn proof_pending_alone_is_inconclusive_not_drift() {
    let mut i = clean_run().input;
    i.pending.push(SettlementRecord {
        day: 26,
        deltas: map(&[("water", -3)]),
        ..SettlementRecord::default()
    });
    assert_eq!(codes(&i), set(&[FailureCode::SettlementsPending]));
    let r = reconcile(&i);
    assert!(is_inconclusive(&r));
    assert_eq!(proof_verdict(&r, false, false), ProofVerdict::Inconclusive);
}

/// Tiny counter-driven generator (no RNG crate).
fn lcg(n: &mut u64) -> u64 {
    *n = n
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *n >> 33
}

#[test]
fn proof_tamper_sweep_200_single_field_mutations_never_report_clean() {
    let base = clean_run().input;
    let r0 = reconcile(&base);
    assert!(r0.passed, "the base input must be clean: {:?}", r0.notes);
    let codes_ = ["AMO", "FOD", "MED", "PRT", "WTR"];
    let keys = ["ammo", "food", "meds", "parts", "water"];
    let mut state = 0x5EED_u64;
    let mut seen: BTreeSet<FailureCode> = BTreeSet::new();
    for n in 0..200usize {
        let mut i = base.clone();
        let p = 1 + (lcg(&mut state) % 9) as i32;
        let ki = (lcg(&mut state) % 5) as usize;
        let ri = (lcg(&mut state) as usize) % i.settlements.len();
        let kind = n % 17;
        match kind {
            0 => *i.ledger_balances.get_mut(codes_[ki]).unwrap() += p,
            1 => *i.ledger_balances.get_mut(codes_[ki]).unwrap() -= p,
            2 => {
                i.ledger_balances.remove(codes_[ki]);
            }
            3 => *i.minted_initial.get_mut(keys[ki]).unwrap() += p,
            4 => *i.last_settled_supplies.get_mut(keys[ki]).unwrap() += p,
            5 => {
                let k = i.settlements[ri].deltas.keys().next().unwrap().clone();
                *i.settlements[ri].deltas.get_mut(&k).unwrap() += p;
            }
            6 => i.settlements[ri].txids.clear(),
            7 => {
                let t = i.settlements[ri].txids[0].clone();
                i.onchain_memos.as_mut().unwrap().remove(&t);
            }
            8 => {
                let t = i.settlements[ri].txids[0].clone();
                let m = i.onchain_memos.as_mut().unwrap();
                let v = m[&t].replacen("RUN:", "RUN:x", 1);
                m.insert(t, v);
            }
            9 => {
                let t = i.settlements[ri].txids[0].clone();
                let m = i.onchain_memos.as_mut().unwrap();
                let v = m[&t].replacen("TRAIL", "TRAIM", 1);
                m.insert(t, v);
            }
            10 => {
                let rec = &mut i.settlements[ri];
                rec.memo = rec.memo.replacen("TRAIL", "TRAIM", 1);
            }
            11 => {
                let dup = i.settlements[ri].clone();
                i.settlements.push(dup);
            }
            12 => {
                i.settlements.remove(ri);
            }
            13 => i.pending.push(SettlementRecord {
                day: 26,
                deltas: map(&[(keys[ki], -p)]),
                ..SettlementRecord::default()
            }),
            14 => i.settlements[ri].day += p,
            15 => i.run_id.push('x'),
            16 => i.onchain_memos = Some(BTreeMap::new()),
            _ => unreachable!(),
        }
        assert_ne!(i, base, "mutation {n} (kind {kind}) changed nothing");
        let r = reconcile(&i);
        assert!(
            !r.passed,
            "mutation {n} (kind {kind}, p {p}, key {ki}) reported clean"
        );
        assert_ne!(
            proof_verdict(&r, false, false),
            ProofVerdict::Pass,
            "mutation {n}"
        );
        assert!(
            !r.failures.is_empty(),
            "mutation {n} gave no structured failure"
        );
        seen.extend(r.codes());
    }
    // The sweep exercised every failure kind. The tenth is ONCHAIN_MEMO_DELTA_MISMATCH: a single
    // edited saved delta (kind 5) now also disagrees with the on-chain memo body.
    assert_eq!(seen.len(), 10, "{seen:?}");
}

// ============================================================================
// 5. The deterministic run, settled through RecordedTransport
// ============================================================================

fn pick_intent(engine: &StepEngine) -> PlayerIntent {
    let s = &engine.state;
    match engine.phase {
        GamePhase::Event | GamePhase::Route => {
            PlayerIntent::new(IntentAction::Choose).with_choice("A")
        }
        _ => {
            if s.supplies.food() < 30 && s.supplies.ammo() > 0 {
                PlayerIntent::new(IntentAction::Hunt)
            } else if s.wagon.condition < 60 && s.supplies.parts() > 0 {
                PlayerIntent::new(IntentAction::Repair)
            } else {
                PlayerIntent::new(IntentAction::Travel)
            }
        }
    }
}

fn supplies_five(s: &SuppliesState) -> BTreeMap<String, i32> {
    ["ammo", "food", "meds", "parts", "water"]
        .iter()
        .map(|k| (k.to_string(), s.get(k)))
        .collect()
}

fn json_five(v: &Value) -> BTreeMap<String, i32> {
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, x)| (k.clone(), x.as_i64().unwrap() as i32))
        .collect()
}

/// Drive seed 7 to its end (narrator off, no GM), with the backpack on and settling at each
/// town arrival and once at the end. With `settle` false the backpack stays off.
fn drive(settle: bool, transport: &mut RecordedTransport, log: &mut ReadLog) -> (RunState, usize) {
    let mut state = create_new_run(7);
    state.run_id = RUN_ID.to_string(); // the generated run id has a random suffix
    if settle {
        let mut bp = enabled_backpack(&[]);
        let initial = supplies_five(&state.supplies);
        bp.last_settled_supplies = initial.clone();
        stamp_minted_snapshot(&mut bp, Some(&initial));
        bp.last_settlement_day = state.day;
        state.backpack = bp;
    }
    let mut engine = StepEngine::new(state);
    let mut prev = engine.state.location_id.clone();
    let mut settlements = 0usize;
    for _ in 0..400 {
        if engine.phase == GamePhase::GameOver || engine.state.game_over {
            break;
        }
        let intent = pick_intent(&engine);
        engine.step(&intent);
        if engine.state.location_id != prev {
            prev = engine.state.location_id.clone();
            let node = engine
                .state
                .map_nodes
                .iter()
                .find(|n| n.node_id == prev)
                .unwrap()
                .clone();
            if node.is_town && settle {
                let st = &mut engine.state;
                let sup = st.supplies.clone();
                let r = settle_checkpoint(
                    &mut st.backpack,
                    &sup,
                    RUN_ID,
                    st.day,
                    &node.name,
                    "",
                    transport,
                    log,
                );
                assert_eq!(r.status, SettleStatus::Settled, "{}", node.name);
                settlements += 1;
            }
        }
    }
    assert!(engine.state.game_over);
    if settle {
        let st = &mut engine.state;
        let sup = st.supplies.clone();
        let r = settle_checkpoint(
            &mut st.backpack,
            &sup,
            RUN_ID,
            st.day,
            "final-reconcile",
            "",
            transport,
            log,
        );
        assert_eq!(r.status, SettleStatus::Settled);
        settlements += 1;
    }
    (engine.state, settlements)
}

fn clean_run() -> Clean {
    let doc = load_fixture();
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let mut log = ReadLog::default();
    let (state, _) = drive(true, &mut t, &mut log);
    let bp = state.backpack.clone();
    let view = read_ledger_view(&mut t, &bp, &mut log);
    let mut bp2 = bp.clone();
    let input = ReconcileInput {
        run_id: RUN_ID.to_string(),
        seed: 7,
        minted_initial: bp2.minted_snapshot_of(),
        ledger_balances: view.balances.expect("balances"),
        last_settled_supplies: bp.last_settled_supplies.clone(),
        settlements: bp.settlements.clone(),
        pending: bp.pending_settlements.clone(),
        player_address: bp.wallet_address.clone(),
        issuer_address: bp.issuer_address.clone(),
        onchain_memos: view.onchain_memos,
    };
    Clean { bp, input, state }
}

#[test]
fn deterministic_run_with_backpack_on_settles_through_recorded_transport_and_reconciles_cleanly() {
    let doc = load_fixture();
    let truth = &doc["engine_truth"];
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let mut log = ReadLog::default();
    let (state, settled) = drive(true, &mut t, &mut log);
    assert_eq!(settled, 5, "four town checkpoints plus the final reconcile");
    assert_eq!(state.day, 26, "engine truth: seed 7 ends on day 26");
    let bp = &state.backpack;

    // Hand-recorded engine truth: what each checkpoint settled.
    let cps = truth["checkpoints"].as_array().unwrap();
    assert_eq!(bp.settlements.len(), cps.len());
    let mut last = json_five(&truth["initial"]);
    for (rec, cp) in bp.settlements.iter().zip(cps) {
        let now = json_five(&cp["supplies"]);
        let want: BTreeMap<String, i32> = now
            .iter()
            .filter(|(k, v)| last[*k] != **v)
            .map(|(k, v)| (k.clone(), v - last[k]))
            .collect();
        assert_eq!(rec.day, cp["day"].as_i64().unwrap() as i32);
        assert_eq!(rec.location, cp["location"].as_str().unwrap());
        assert_eq!(rec.deltas, want, "day {}", rec.day);
        assert_eq!(rec.status, "settled");
        last = now;
    }
    // Hand-worked literals for the first and last checkpoints.
    assert_eq!(
        bp.settlements[0].memo,
        "TRAIL|RUN:run-2b-seed7|DAY:11|DELTA:AMO-5,FOD-5,MED+6,PRT-3|V:v1"
    );
    assert_eq!(
        bp.settlements[0].deltas,
        map(&[("ammo", -5), ("food", -5), ("meds", 6), ("parts", -3)])
    );
    assert_eq!(
        bp.settlements[4].memo,
        "TRAIL|RUN:run-2b-seed7|DAY:26|DELTA:AMO-1,FOD-3,WTR-5|V:v1"
    );
    assert_eq!(
        bp.last_settled_supplies,
        map(&[
            ("ammo", 10),
            ("food", 31),
            ("meds", 9),
            ("parts", 0),
            ("water", 39)
        ])
    );
    assert_eq!(bp.last_settled_supplies, supplies_five(&state.supplies));
    assert_eq!(
        bp.settlements.iter().map(|s| s.txids.len()).sum::<usize>(),
        14
    );
    assert!(bp.pending_settlements.is_empty() && !bp.last_settle_failed);
    assert_eq!(
        t.unconsumed_submits(),
        0,
        "every recorded payment was matched by a call"
    );

    // Reconcile through the same transport.
    let view = read_ledger_view(&mut t, bp, &mut log);
    let res = prove_player_save(RUN_ID, 7, bp, &view);
    assert_eq!(res.verdict, ProofVerdict::Pass, "{:?}", res.notes);
    let report = res.report.unwrap();
    assert!(report.passed && report.failures.is_empty() && report.notes.is_empty());
    assert_eq!(report.settlements_count, 5);
    assert_eq!(report.txids.len(), 14);
    for r in &report.resources {
        assert!(r.balance_ok && r.conservation_ok, "{r:?}");
    }
    let (req_missing, _) = missing_reads(&log);
    assert!(req_missing.is_empty(), "{req_missing:?}");
}

#[test]
fn backpack_off_leaves_the_game_identical_to_today() {
    // Off by default.
    let fresh = create_new_run(7);
    assert_eq!(fresh.backpack, BackpackState::default());
    assert!(!fresh.backpack.enabled);
    // Settling with the backpack off touches nothing and never calls the transport.
    let mut off = BackpackState::default();
    let mut t = RecordedTransport::from_fixture(&load_fixture()).unwrap();
    let r = settle_checkpoint(
        &mut off,
        &fresh.supplies,
        RUN_ID,
        5,
        "Town",
        "",
        &mut t,
        &mut ReadLog::default(),
    );
    assert_eq!(r.status, SettleStatus::NotEnabled);
    assert_eq!(off, BackpackState::default());
    assert!(t.calls.is_empty());

    // The same seed played with the backpack off and on gives the same engine state, backpack aside:
    // settlement reads the supplies and writes only the backpack.
    let doc = load_fixture();
    let mut t_off = RecordedTransport::from_fixture(&doc).unwrap();
    let (off_state, _) = drive(false, &mut t_off, &mut ReadLog::default());
    let mut t_on = RecordedTransport::from_fixture(&doc).unwrap();
    let (on_state, _) = drive(true, &mut t_on, &mut ReadLog::default());
    assert!(t_off.calls.is_empty());
    let mut a = state_to_value(&off_state);
    let mut b = state_to_value(&on_state);
    a.as_object_mut().unwrap().remove("backpack");
    b.as_object_mut().unwrap().remove("backpack");
    assert_eq!(a, b);
    assert_eq!(off_state.backpack, BackpackState::default());
}

#[test]
fn settlement_records_round_trip_through_save_schema_v4_unchanged() {
    let c = clean_run();
    let v = state_to_value(&c.state);
    assert_eq!(v["save_version"], json!(4), "save schema stays v4");
    // The record fields the settlement logic writes are all already in the v4 backpack object.
    let bp = &v["backpack"];
    for k in [
        "settlements",
        "pending_settlements",
        "last_settled_supplies",
        "last_settlement_day",
        "minted_initial",
    ] {
        assert!(bp.get(k).is_some(), "v4 backpack lacks {k}");
    }
    for k in [
        "day",
        "location",
        "deltas",
        "txids",
        "status",
        "memo",
        "timestamp",
    ] {
        assert!(
            bp["settlements"][0].get(k).is_some(),
            "v4 settlement record lacks {k}"
        );
    }
    let back = value_to_state(&v).expect("v4 save loads");
    assert_eq!(back.backpack, c.bp);
}

#[test]
fn nothing_in_the_backpack_or_proof_modules_can_open_a_socket() {
    // The crate's dependency list is unchanged: no HTTP or XRPL client crate was added.
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let deps: Vec<&str> = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap()
        .split("[dev-dependencies]")
        .next()
        .unwrap()
        .lines()
        .filter_map(|l| l.split('=').next().map(str::trim))
        .filter(|n| !n.is_empty())
        .collect();
    assert_eq!(deps, ["getrandom", "serde", "serde_json", "sha2", "ureq"]);
    // And the two modules mention no network type (needles are split so this file does not match).
    let needles = [
        concat!("std::", "net"),
        concat!("Tcp", "Stream"),
        concat!("ur", "eq"),
        concat!("req", "west"),
        concat!("hy", "per"),
        concat!("xr", "pl::"),
    ];
    for (name, src) in [
        ("backpack.rs", include_str!("../src/backpack.rs")),
        ("ledger_proof.rs", include_str!("../src/ledger_proof.rs")),
    ] {
        for n in needles {
            assert!(!src.contains(n), "{name} mentions {n}");
        }
    }
}
