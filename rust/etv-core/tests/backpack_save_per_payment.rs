//! The ledger backpack saves its state after EACH confirmed payment, so a crash part way through a
//! batch cannot make the next run pay the same keys again.
//!
//! Every expected value below is a hand-worked literal. The sweep's invariant is the same one
//! `backpack_pending_overlap.rs` uses: for every key, the payments on the ledger are exactly the
//! nonzero per-checkpoint supply changes, each once. There is no second implementation of this
//! behaviour to compare against (the shipped Python persists per key through an optional hook but
//! has no test of a crash between two keys), so nothing here is compared to Python.
//!
//! "The process dies" is modelled without killing anything: the transport refuses every payment
//! from that point on, and the test reloads the state from the last snapshot the persist hook
//! was handed, exactly what a real restart would find on disk.
//!
//! No test opens a socket: the transport is an in-memory simulation of a ledger.

use std::collections::{BTreeMap, BTreeSet};

use etv_core::backpack::{
    fetch_onchain_memos, plan_settlement, settle_checkpoint, settle_checkpoint_with_persist,
    FetchError, LedgerTransport, PaymentAttempt, PaymentDirection, PersistFailed, PlanOutcome,
    ReadLog, RecordedTransport, SettleReport, SettleStatus, SettlementBatch, TransportError,
    MAX_ACCOUNT_TX_PAGES,
};
use etv_core::ledger_proof::{
    prove_player_save, read_ledger_view, reconcile, FailureCode, ProofVerdict, ReconcileInput,
    ReconcileReport,
};
use etv_core::model::{BackpackState, SuppliesState};
use serde_json::{json, Value};

const PLAYER: &str = "rTestPlayerWallet111111111111";
const ISSUER: &str = "rTestIssuerWallet222222222222";
const RUN_ID: &str = "run-save";

const KEYS: [&str; 5] = ["ammo", "food", "meds", "parts", "water"];
const CODES: [&str; 5] = ["AMO", "FOD", "MED", "PRT", "WTR"];

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

/// A start where every one of the five keys is nonzero in the first checkpoint below.
const START: [(&str, i32); 5] = [
    ("ammo", 20),
    ("food", 50),
    ("meds", 5),
    ("parts", 5),
    ("water", 20),
];
/// The checkpoint's supplies: ammo -2, food -3, meds -1, parts -2, water -5.
const AFTER: [(&str, i32); 5] = [
    ("ammo", 18),
    ("food", 47),
    ("meds", 4),
    ("parts", 3),
    ("water", 15),
];

/// An in-memory ledger that settles one payment per call.
#[derive(Default)]
struct Ledger {
    /// Every payment that landed, in order: (key, signed delta in the supply convention, txid).
    paid: Vec<(String, i32, String)>,
    /// Simulated on-ledger balance per game key.
    balances: BTreeMap<String, i32>,
    /// txid -> on-chain memo text.
    memos: BTreeMap<String, String>,
    /// Calls made to `submit_payment`.
    submit_calls: usize,
    /// "The process died": every payment is refused from now on.
    dead: bool,
    /// After this many payments have landed in total, the process dies.
    die_after_landed: Option<usize>,
    /// Misbehave: answer every call with the result for the batch's LAST payment's key, paying
    /// nothing. `Some(n)`: only from the n-th call of this ledger on (1-based).
    wrong_key_from_call: Option<usize>,
}

impl Ledger {
    fn new(initial: &BTreeMap<String, i32>) -> Self {
        Ledger {
            balances: initial.clone(),
            ..Ledger::default()
        }
    }

    fn keys_paid(&self) -> Vec<&str> {
        self.paid.iter().map(|(k, _, _)| k.as_str()).collect()
    }

    fn paid_for(&self, key: &str) -> Vec<i32> {
        self.paid
            .iter()
            .filter(|(k, _, _)| k == key)
            .map(|(_, v, _)| *v)
            .collect()
    }
}

impl LedgerTransport for Ledger {
    fn balances(&mut self, a: &str) -> Result<Value, TransportError> {
        Err(TransportError::NoRecording(a.to_string()))
    }

    fn submit_payment(&mut self, batch: &SettlementBatch, index: usize) -> PaymentAttempt {
        self.submit_calls += 1;
        let p = &batch.payments[index];
        if let Some(from) = self.wrong_key_from_call {
            if self.submit_calls >= from {
                let wrong = &batch.payments[batch.payments.len() - 1];
                return PaymentAttempt {
                    key: wrong.key.clone(),
                    result: Ok(json!({ "hash": "TX_WRONG" })),
                };
            }
        }
        if self.dead {
            return PaymentAttempt {
                key: p.key.clone(),
                result: Err(TransportError::Unreachable("process died".into())),
            };
        }
        let signed = match p.direction {
            PaymentDirection::PlayerToIssuer => -(p.value as i32),
            PaymentDirection::IssuerToPlayer => p.value as i32,
        };
        *self.balances.entry(p.key.clone()).or_insert(0) += signed;
        let txid = format!("TX{:04}", self.paid.len() + 1);
        self.paid.push((p.key.clone(), signed, txid.clone()));
        self.memos.insert(txid.clone(), batch.memo_text.clone());
        if self.die_after_landed == Some(self.paid.len()) {
            self.dead = true;
        }
        PaymentAttempt {
            key: p.key.clone(),
            result: Ok(json!({ "hash": txid })),
        }
    }

    fn tx_memos(&mut self, a: &str, _m: Option<&Value>) -> Result<Value, TransportError> {
        Err(TransportError::NoRecording(a.to_string()))
    }
}

/// A stand-in for the caller's save file: keeps the last snapshot it was handed and what each
/// call saw. `fail_on_call` (1-based) makes that call fail WITHOUT saving.
#[derive(Default)]
struct Disk {
    last: Option<BackpackState>,
    /// (key whose baseline moved since the previous snapshot, newest txid in the receipts).
    seen: Vec<(String, String)>,
    calls: usize,
    fail_on_call: Option<usize>,
}

impl Disk {
    fn save(&mut self, bp: &BackpackState, base: &BTreeMap<String, i32>) -> Result<(), String> {
        self.calls += 1;
        if self.fail_on_call == Some(self.calls) {
            return Err("disk full".to_string());
        }
        let prev = self
            .last
            .as_ref()
            .map(|s| s.last_settled_supplies.clone())
            .unwrap_or_else(|| base.clone());
        let moved: Vec<&String> = bp
            .last_settled_supplies
            .iter()
            .filter(|(k, v)| prev.get(*k) != Some(*v))
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            moved.len(),
            1,
            "each save follows exactly one folded payment"
        );
        let txid = bp
            .settlements
            .last()
            .and_then(|s| s.txids.last())
            .cloned()
            .unwrap_or_default();
        self.seen.push((moved[0].clone(), txid));
        self.last = Some(bp.clone());
        Ok(())
    }
}

fn settle_with_disk(
    bp: &mut BackpackState,
    sup: &SuppliesState,
    day: i32,
    t: &mut Ledger,
    disk: &mut Disk,
) -> Result<SettleReport, PersistFailed<String>> {
    // The baseline the state had before this call: what the first save is compared to.
    let base = bp.last_settled_supplies.clone();
    settle_checkpoint_with_persist(
        bp,
        sup,
        RUN_ID,
        day,
        "Town",
        "T",
        t,
        &mut |s| disk.save(s, &base),
        &mut ReadLog::default(),
    )
}

fn proof_report(bp: &BackpackState, minted: &BTreeMap<String, i32>, t: &Ledger) -> ReconcileReport {
    let ledger_balances: BTreeMap<String, i32> = KEYS
        .iter()
        .zip(CODES.iter())
        .map(|(k, c)| (c.to_string(), t.balances.get(*k).copied().unwrap_or(0)))
        .collect();
    reconcile(&ReconcileInput {
        run_id: RUN_ID.to_string(),
        seed: 7,
        minted_initial: minted.clone(),
        ledger_balances,
        last_settled_supplies: bp.last_settled_supplies.clone(),
        settlements: bp.settlements.clone(),
        pending: bp.pending_settlements.clone(),
        player_address: PLAYER.to_string(),
        issuer_address: ISSUER.to_string(),
        onchain_memos: Some(t.memos.clone()),
    })
}

// ── a. crash after payments 1 and 2 were saved: resume pays exactly three more ─────────────

#[test]
fn a_crash_after_two_saved_payments_resumes_with_exactly_three_more_never_seven() {
    let minted = map(&START);
    let mut bp = enabled_backpack(&START);
    let mut t = Ledger::new(&minted);
    t.die_after_landed = Some(2);
    let mut disk = Disk::default();
    let sup = supplies(&AFTER);

    // The process dies after payment 2 (the ledger refuses everything from then on).
    let r = settle_with_disk(&mut bp, &sup, 11, &mut t, &mut disk).unwrap();
    assert!(matches!(r.status, SettleStatus::Pending { .. }));
    assert_eq!(t.keys_paid(), vec!["ammo", "food"]);

    // Reload from what the hook last saw: exactly those two keys settled, with their txids.
    let mut reloaded = disk.last.clone().expect("a save was made");
    assert_eq!(reloaded.settlements.len(), 1);
    assert_eq!(
        reloaded.settlements[0].deltas,
        map(&[("ammo", -2), ("food", -3)])
    );
    assert_eq!(reloaded.settlements[0].txids, vec!["TX0001", "TX0002"]);
    assert_eq!(
        reloaded.last_settled_supplies,
        map(&[
            ("ammo", 18),
            ("food", 47),
            ("meds", 5),
            ("parts", 5),
            ("water", 20)
        ])
    );
    // Three keys are still unpaid, and they are exactly what the next plan asks for.
    match plan_settlement(&reloaded, &sup, RUN_ID, 11, "Town") {
        PlanOutcome::Batch(b) => {
            assert_eq!(b.deltas, map(&[("meds", -1), ("parts", -2), ("water", -5)]));
        }
        other => panic!("expected a three-key batch, got {other:?}"),
    }

    // The process comes back and the ledger is reachable: three more payments, five in total.
    t.dead = false;
    t.die_after_landed = None;
    let mut disk2 = Disk::default();
    let r2 = settle_with_disk(&mut reloaded, &sup, 11, &mut t, &mut disk2).unwrap();
    assert_eq!(r2.status, SettleStatus::Settled);
    assert_eq!(
        t.keys_paid(),
        vec!["ammo", "food", "meds", "parts", "water"]
    );
    assert_eq!(t.paid.len(), 5, "five payments in total, never seven");
    assert!(reloaded.pending_settlements.is_empty());
    let report = proof_report(&reloaded, &minted, &t);
    assert!(report.passed, "{:?}", report.notes);
}

// ── b. the hook is called once per confirmed payment, in order, state already folded ───────

#[test]
fn b_the_persist_hook_runs_once_per_confirmed_payment_in_order_with_the_state_folded() {
    let minted = map(&START);
    let mut bp = enabled_backpack(&START);
    let mut t = Ledger::new(&minted);
    let mut disk = Disk::default();
    let r = settle_with_disk(&mut bp, &supplies(&AFTER), 11, &mut t, &mut disk).unwrap();
    assert_eq!(r.status, SettleStatus::Settled);
    assert_eq!(disk.calls, 5);
    let want: Vec<(String, String)> = [
        ("ammo", "TX0001"),
        ("food", "TX0002"),
        ("meds", "TX0003"),
        ("parts", "TX0004"),
        ("water", "TX0005"),
    ]
    .iter()
    .map(|(k, x)| (k.to_string(), x.to_string()))
    .collect();
    assert_eq!(disk.seen, want);
    // The last snapshot is the finished batch.
    let last = disk.last.unwrap();
    assert_eq!(last.settlements[0].txids.len(), 5);
    assert_eq!(last.last_settled_supplies, map(&AFTER));
}

#[test]
fn b2_a_wrapper_with_no_hook_still_works_and_matches() {
    let minted = map(&START);
    let mut bp = enabled_backpack(&START);
    let mut t = Ledger::new(&minted);
    let r = settle_checkpoint(
        &mut bp,
        &supplies(&AFTER),
        RUN_ID,
        11,
        "Town",
        "T",
        &mut t,
        &mut ReadLog::default(),
    );
    assert_eq!(r.status, SettleStatus::Settled);
    assert_eq!(r.confirmed.len(), 5);
    assert_eq!(bp.last_settlement_day, 11);
}

// ── c. a failing persist stops the batch ───────────────────────────────────────────────────

#[test]
fn c_persist_failing_after_payment_two_stops_the_batch_and_says_a_payment_may_have_landed() {
    let minted = map(&START);
    let mut bp = enabled_backpack(&START);
    let mut t = Ledger::new(&minted);
    let mut disk = Disk {
        fail_on_call: Some(2),
        ..Disk::default()
    };
    let sup = supplies(&AFTER);

    let err = settle_with_disk(&mut bp, &sup, 11, &mut t, &mut disk).unwrap_err();
    assert_eq!(err.key, "food");
    assert_eq!(err.source, "disk full");
    assert_eq!(
        err.to_string(),
        "the payment for food was confirmed but the state could not be saved (disk full); \
         a payment may have landed without being saved"
    );
    // Payments 1 and 2 are on the ledger; payment 3 was never submitted.
    assert_eq!(t.keys_paid(), vec!["ammo", "food"]);
    assert_eq!(t.submit_calls, 2);
    // State as last successfully saved: payment 1 only.
    let saved = disk.last.clone().unwrap();
    assert_eq!(saved.settlements[0].deltas, map(&[("ammo", -2)]));
    assert_eq!(saved.settlements[0].txids, vec!["TX0001"]);
    assert_eq!(saved.last_settled_supplies["food"], 50);
    // In memory the rest are left pending.
    assert_eq!(
        bp.pending_settlements[0].deltas,
        map(&[("meds", -1), ("parts", -2), ("water", -5)])
    );
    assert!(bp.last_settle_failed);

    // A caller that survived the failure and settles again from the in-memory state pays the
    // three remaining keys and nothing twice: five payments in total.
    let mut disk_ok = Disk::default();
    settle_with_disk(&mut bp, &sup, 12, &mut t, &mut disk_ok).unwrap();
    assert_eq!(
        t.keys_paid(),
        vec!["ammo", "food", "meds", "parts", "water"]
    );
    assert!(bp.pending_settlements.is_empty());
}

#[test]
fn c2_after_a_persist_failure_the_next_settle_from_the_last_good_save_pays_the_key_again() {
    // The design does NOT avoid this by construction: the last good save lacks payment 2, so
    // planning from it asks for food again. This is the known window (see test d).
    let minted = map(&START);
    let mut bp = enabled_backpack(&START);
    let mut t = Ledger::new(&minted);
    let mut disk = Disk {
        fail_on_call: Some(2),
        ..Disk::default()
    };
    let sup = supplies(&AFTER);
    settle_with_disk(&mut bp, &sup, 11, &mut t, &mut disk).unwrap_err();

    let mut from_disk = disk.last.clone().unwrap();
    let mut disk2 = Disk::default();
    settle_with_disk(&mut from_disk, &sup, 11, &mut t, &mut disk2).unwrap();
    assert_eq!(
        t.keys_paid(),
        vec!["ammo", "food", "food", "meds", "parts", "water"]
    );
    assert_eq!(t.paid_for("food"), vec![-3, -3]);
}

// ── d. the known limit ─────────────────────────────────────────────────────────────────────

/// KNOWN LIMIT, not a goal: one payment confirms on the ledger and the process dies before its
/// save. The save lacks that payment, so the next settle submits it a second time. The shipped
/// Python has the same window. The reconciliation proof reports it afterwards
/// (LEDGER_BALANCE_MISMATCH). Closing the window needs amount and destination evidence from the
/// AccountTx response; that is a follow-up PR, after the real response shape (Amount vs
/// DeliverMax by api_version) has been read at source. This test pins today's behaviour so the
/// follow-up has something to change.
#[test]
fn known_limit_crash_between_confirmation_and_save_pays_that_key_twice_and_the_proof_reports_it() {
    let minted = map(&START);
    let mut bp = enabled_backpack(&START);
    let mut t = Ledger::new(&minted);
    // The save for payment 2 never happens (the process died first).
    let mut disk = Disk {
        fail_on_call: Some(2),
        ..Disk::default()
    };
    let sup = supplies(&AFTER);
    settle_with_disk(&mut bp, &sup, 11, &mut t, &mut disk).unwrap_err();
    assert_eq!(t.keys_paid(), vec!["ammo", "food"]);

    // Restart: state reloaded from before payment 2 was folded; the same ledger.
    let mut reloaded = disk.last.clone().unwrap();
    let mut disk2 = Disk::default();
    settle_with_disk(&mut reloaded, &sup, 11, &mut t, &mut disk2).unwrap();

    // Food was submitted a second time.
    assert_eq!(t.paid.len(), 6);
    assert_eq!(t.paid_for("food"), vec![-3, -3]);
    assert_eq!(t.balances["food"], 44);
    assert_eq!(reloaded.last_settled_supplies["food"], 47);

    // The proof reports it: the ledger holds 44 food, the save says 47.
    let report = proof_report(&reloaded, &minted, &t);
    assert!(!report.passed);
    assert_eq!(
        report.codes(),
        BTreeSet::from([FailureCode::LedgerBalanceMismatch])
    );
    assert_eq!(report.failures[0].code.as_str(), "LEDGER_BALANCE_MISMATCH");
}

// ── e. a buggy transport: results for the wrong key ────────────────────────────────────────

fn five_key_state() -> (BackpackState, SuppliesState, BTreeMap<String, i32>) {
    (enabled_backpack(&START), supplies(&AFTER), map(&START))
}

#[test]
fn e_a_result_for_the_wrong_key_stops_the_batch_at_once_with_no_second_payment() {
    let (mut bp, sup, minted) = five_key_state();
    let mut t = Ledger::new(&minted);
    t.wrong_key_from_call = Some(1);
    let mut disk = Disk::default();
    let r = settle_with_disk(&mut bp, &sup, 11, &mut t, &mut disk).unwrap();
    assert_eq!(t.submit_calls, 1, "one call, then the batch stops");
    assert!(t.paid.is_empty());
    assert_eq!(disk.calls, 0);
    match r.status {
        SettleStatus::Pending { reason } => {
            assert_eq!(reason, "result for water where ammo was expected");
        }
        other => panic!("expected Pending, got {other:?}"),
    }
    assert_eq!(bp.last_settled_supplies, map(&START));
    assert_eq!(
        bp.pending_settlements[0].deltas,
        map(&[
            ("ammo", -2),
            ("food", -3),
            ("meds", -1),
            ("parts", -2),
            ("water", -5)
        ])
    );
}

#[test]
fn e2_a_wrong_key_after_two_good_payments_stops_there() {
    let (mut bp, sup, minted) = five_key_state();
    let mut t = Ledger::new(&minted);
    t.wrong_key_from_call = Some(3);
    let mut disk = Disk::default();
    let r = settle_with_disk(&mut bp, &sup, 11, &mut t, &mut disk).unwrap();
    assert_eq!(t.submit_calls, 3);
    assert_eq!(t.keys_paid(), vec!["ammo", "food"]);
    assert_eq!(disk.calls, 2);
    match r.status {
        SettleStatus::Pending { reason } => {
            assert_eq!(reason, "result for water where meds was expected");
        }
        other => panic!("expected Pending, got {other:?}"),
    }
    assert_eq!(
        bp.pending_settlements[0].deltas,
        map(&[("meds", -1), ("parts", -2), ("water", -5)])
    );
    assert_eq!(bp.last_settled_supplies["meds"], 5);
}

// ── f. the AccountTx page cap is loud ──────────────────────────────────────────────────────

#[test]
fn f_hitting_the_account_tx_page_cap_is_an_error_and_the_proof_is_inconclusive() {
    let echo = json!({"ledger": 1});
    let doc = json!({
        "account_lines": {PLAYER: {"lines": []}},
        "account_tx": [
            {"account": PLAYER, "marker": null, "result": {"transactions": [], "marker": echo}},
            {"account": PLAYER, "marker": echo, "result": {"transactions": [], "marker": echo}}
        ]
    });
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let err = fetch_onchain_memos(&mut t, PLAYER, ISSUER, &mut ReadLog::default()).unwrap_err();
    assert_eq!(
        err,
        FetchError::PageCapReached {
            account: PLAYER.to_string(),
            pages: 100
        }
    );
    assert_eq!(MAX_ACCOUNT_TX_PAGES, 100);
    assert_eq!(t.calls.len(), 100);

    // The proof reads it as an unreachable ledger, never as a short list.
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let bp = enabled_backpack(&START);
    let view = read_ledger_view(&mut t, &bp, &mut ReadLog::default());
    assert_eq!(view.balances, None);
    assert_eq!(view.onchain_memos, None);
    let result = prove_player_save(RUN_ID, 7, &bp, &view);
    assert_eq!(result.verdict, ProofVerdict::Inconclusive);
}

#[test]
fn f2_a_page_list_that_ends_exactly_at_the_cap_is_complete() {
    // 100 pages where the 100th has no marker: not truncated, so no error.
    let mut pages = Vec::new();
    for i in 0..MAX_ACCOUNT_TX_PAGES {
        let marker = if i == 0 { Value::Null } else { json!({"p": i}) };
        let mut result = json!({"transactions": []});
        if i + 1 < MAX_ACCOUNT_TX_PAGES {
            result["marker"] = json!({"p": i + 1});
        }
        pages.push(json!({"account": PLAYER, "marker": marker, "result": result}));
    }
    let doc = json!({ "account_tx": pages });
    let mut t = RecordedTransport::from_fixture(&doc).unwrap();
    let memos = fetch_onchain_memos(&mut t, PLAYER, "", &mut ReadLog::default()).unwrap();
    assert!(memos.is_empty());
    assert_eq!(t.calls.len(), 100);
}

// ── g. sweep: crashes at save points ───────────────────────────────────────────────────────

/// Tiny counter-driven generator (no RNG crate).
fn lcg(n: &mut u64) -> u64 {
    *n = n
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *n >> 33
}

fn sup_of(cur: &BTreeMap<String, i32>) -> SuppliesState {
    supplies(
        &cur.iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect::<Vec<_>>(),
    )
}

/// 200 deterministic sequences. At each checkpoint the process may die right after the k-th save
/// (a crash AT a save point; a crash inside the window between a confirmation and its save is the
/// documented limit, test d, and is not injected here). The state is reloaded from the last save
/// and the same checkpoint runs again. Afterwards every key's payments are exactly its nonzero
/// per-checkpoint changes, each once; nothing is pending; the proof is clean.
#[test]
fn g_sweep_200_sequences_with_crashes_at_save_points_pay_each_change_exactly_once() {
    let mut rng = 0xBADC0DE_u64;
    let mut violations: Vec<String> = Vec::new();
    let mut crashes = 0usize;
    for seq in 0..200usize {
        let initial: Vec<(&str, i32)> = KEYS.iter().map(|k| (*k, 40)).collect();
        let mut bp = enabled_backpack(&initial);
        let minted = bp.last_settled_supplies.clone();
        let mut t = Ledger::new(&minted);
        let checkpoints = 2 + (lcg(&mut rng) % 5) as usize; // 2..=6
        let mut cur = minted.clone();
        let mut steps: BTreeMap<String, Vec<i32>> = BTreeMap::new();
        for day in 0..checkpoints {
            for key in KEYS {
                // Most keys change at most checkpoints, so batches are often three to five keys.
                let step = (lcg(&mut rng) % 9) as i32 - 4; // -4..=4
                *cur.get_mut(key).unwrap() += step;
                if step != 0 {
                    steps.entry(key.to_string()).or_default().push(step);
                }
            }
            let sup = sup_of(&cur);
            // 0: no crash; k >= 1: die right after the k-th save of this checkpoint.
            let crash_after = match lcg(&mut rng) % 3 {
                0 => 1 + (lcg(&mut rng) % 4) as usize,
                _ => 0,
            };
            let mut last_save = bp.clone();
            let mut saves = 0usize;
            {
                let dead = std::cell::Cell::new(false);
                let mut hook = |s: &BackpackState| -> Result<(), String> {
                    saves += 1;
                    last_save = s.clone();
                    if saves == crash_after {
                        dead.set(true);
                    }
                    Ok(())
                };
                // Run until the transport "dies": we model the death by refusing payments once
                // the hook has seen the k-th save, via the ledger's dead flag.
                t.dead = false;
                let mut guarded = Guarded {
                    inner: &mut t,
                    dead: &dead,
                };
                let _ = settle_checkpoint_with_persist(
                    &mut bp,
                    &sup,
                    RUN_ID,
                    day as i32 + 1,
                    "Town",
                    "T",
                    &mut guarded,
                    &mut hook,
                    &mut ReadLog::default(),
                );
            }
            if crash_after != 0 && saves >= crash_after {
                crashes += 1;
                // Restart from the last save; the same checkpoint runs again.
                bp = last_save;
                t.dead = false;
                let _ = settle_checkpoint(
                    &mut bp,
                    &sup,
                    RUN_ID,
                    day as i32 + 1,
                    "Town",
                    "T",
                    &mut t,
                    &mut ReadLog::default(),
                );
            }
        }
        // A last checkpoint with no change settles anything still pending.
        let sup = sup_of(&cur);
        t.dead = false;
        settle_checkpoint(
            &mut bp,
            &sup,
            RUN_ID,
            checkpoints as i32 + 1,
            "Town",
            "T",
            &mut t,
            &mut ReadLog::default(),
        );

        let mut bad: Vec<String> = Vec::new();
        for key in KEYS {
            let mut want = steps.get(key).cloned().unwrap_or_default();
            let mut got = t.paid_for(key);
            want.sort_unstable();
            got.sort_unstable();
            if got != want {
                bad.push(format!("{key}: payments {got:?} != changes {want:?}"));
            }
            let net = cur[key] - minted[key];
            if got.iter().sum::<i32>() != net {
                bad.push(format!("{key}: net != {net}"));
            }
        }
        if !bp.pending_settlements.is_empty() {
            bad.push("pending not empty".to_string());
        }
        if bp.last_settled_supplies != cur {
            bad.push("baseline != final supplies".to_string());
        }
        let report = proof_report(&bp, &minted, &t);
        if !report.passed {
            bad.push(format!("proof: {:?}", report.codes()));
        }
        if !bad.is_empty() {
            violations.push(format!("sequence {seq}: {}", bad.join("; ")));
        }
    }
    assert!(crashes > 20, "the sweep must actually crash ({crashes})");
    assert!(
        violations.is_empty(),
        "{} of 200 sequences violate the invariant; first: {:?}",
        violations.len(),
        violations.first()
    );
}

/// The ledger, refusing every payment once `dead` is set (the process died after a save).
struct Guarded<'a> {
    inner: &'a mut Ledger,
    dead: &'a std::cell::Cell<bool>,
}

impl LedgerTransport for Guarded<'_> {
    fn balances(&mut self, a: &str) -> Result<Value, TransportError> {
        self.inner.balances(a)
    }
    fn submit_payment(&mut self, batch: &SettlementBatch, index: usize) -> PaymentAttempt {
        if self.dead.get() {
            return PaymentAttempt {
                key: batch.payments[index].key.clone(),
                result: Err(TransportError::Unreachable("process died".into())),
            };
        }
        self.inner.submit_payment(batch, index)
    }
    fn tx_memos(&mut self, a: &str, m: Option<&Value>) -> Result<Value, TransportError> {
        self.inner.tx_memos(a, m)
    }
}
