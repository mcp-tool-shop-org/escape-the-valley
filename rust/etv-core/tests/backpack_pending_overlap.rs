//! A supply change that is already queued in a pending settlement record must never be queued (or
//! paid) a second time, however many checkpoints pass while the transport is offline.
//!
//! The planning rule under test: the fresh delta is
//! `current supplies - (last_settled + sum of pending deltas)`, per key, in the supply sign
//! convention (negative = the player lost supplies). Every expected value below is a hand-worked
//! literal, except the sweep's invariant, which is the arithmetic identity
//! `sum of payments == final - initial` per key. There is no second implementation of this
//! behaviour (the frozen Python plans from the baseline alone), so there is nothing to compare to.
//!
//! No test opens a socket: the transport is an in-memory simulation of a ledger.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use etv_core::backpack::{
    plan_settlement, retry_pending, settle_checkpoint, LedgerTransport, PaymentAttempt,
    PaymentDirection, PlanOutcome, ReadLog, SettleReport, SettleStatus, SettlementBatch,
    TransportError,
};
use etv_core::ledger_proof::{proof_verdict, reconcile, ProofVerdict, ReconcileInput};
use etv_core::model::{BackpackState, SuppliesState};
use serde_json::{json, Value};

const PLAYER: &str = "rTestPlayerWallet111111111111";
const ISSUER: &str = "rTestIssuerWallet222222222222";
const RUN_ID: &str = "run-overlap";

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

/// An in-memory ledger. Each call to `submit_batch` consults, in order: `online`, the per-call
/// `script` (popped once per call; `Some(k)` fails the k-th payment of that call, `None` is
/// clean), and `fail_keys` (a key that always fails while listed). A confirmed payment moves the
/// simulated balance and records its memo under a fresh txid.
struct Sim {
    online: bool,
    script: VecDeque<Option<usize>>,
    fail_keys: BTreeSet<String>,
    /// Every confirmed payment in order: (game key, signed delta in the supply convention).
    paid: Vec<(String, i32)>,
    calls: usize,
    /// Simulated on-ledger balance per game key.
    ledger: BTreeMap<String, i32>,
    /// txid -> on-chain memo text.
    memos: BTreeMap<String, String>,
}

impl Sim {
    fn new(initial: &BTreeMap<String, i32>) -> Self {
        Sim {
            online: true,
            script: VecDeque::new(),
            fail_keys: BTreeSet::new(),
            paid: Vec::new(),
            calls: 0,
            ledger: initial.clone(),
            memos: BTreeMap::new(),
        }
    }

    fn paid_for(&self, key: &str) -> Vec<i32> {
        self.paid
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| *v)
            .collect()
    }

    fn net(&self, key: &str) -> i32 {
        self.paid_for(key).iter().sum()
    }
}

impl LedgerTransport for Sim {
    fn balances(&mut self, a: &str) -> Result<Value, TransportError> {
        Err(TransportError::NoRecording(a.to_string()))
    }

    fn submit_batch(&mut self, batch: &SettlementBatch) -> Vec<PaymentAttempt> {
        self.calls += 1;
        let scripted = self.script.pop_front().flatten();
        let mut out = Vec::new();
        for (i, p) in batch.payments.iter().enumerate() {
            let refuse = !self.online || scripted == Some(i) || self.fail_keys.contains(&p.key);
            if refuse {
                out.push(PaymentAttempt {
                    key: p.key.clone(),
                    result: Err(TransportError::Unreachable("scripted".into())),
                });
                break;
            }
            let signed = match p.direction {
                PaymentDirection::PlayerToIssuer => -(p.value as i32),
                PaymentDirection::IssuerToPlayer => p.value as i32,
            };
            *self.ledger.entry(p.key.clone()).or_insert(0) += signed;
            self.paid.push((p.key.clone(), signed));
            let txid = format!("TX{:04}", self.paid.len());
            self.memos.insert(txid.clone(), batch.memo_text.clone());
            out.push(PaymentAttempt {
                key: p.key.clone(),
                result: Ok(json!({ "hash": txid })),
            });
        }
        out
    }

    fn tx_memos(&mut self, a: &str, _m: Option<&Value>) -> Result<Value, TransportError> {
        Err(TransportError::NoRecording(a.to_string()))
    }
}

fn checkpoint(bp: &mut BackpackState, sup: &SuppliesState, day: i32, t: &mut Sim) -> SettleReport {
    settle_checkpoint(
        bp,
        sup,
        RUN_ID,
        day,
        "Town",
        "T",
        t,
        &mut ReadLog::default(),
    )
}

/// The reconciliation proof over the simulated ledger: balances and on-chain memos are what the
/// simulation holds, the mint snapshot is the supplies at enable time.
fn proof(bp: &BackpackState, minted: &BTreeMap<String, i32>, t: &Sim) -> ProofVerdict {
    let ledger_balances: BTreeMap<String, i32> = KEYS
        .iter()
        .zip(CODES.iter())
        .map(|(k, c)| (c.to_string(), t.ledger.get(*k).copied().unwrap_or(0)))
        .collect();
    let report = reconcile(&ReconcileInput {
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
    });
    assert!(report.passed, "proof must be clean: {:?}", report.notes);
    proof_verdict(&report, false, false)
}

fn all_five(food: i32, parts: i32, water: i32) -> Vec<(&'static str, i32)> {
    vec![
        ("ammo", 20),
        ("food", food),
        ("meds", 5),
        ("parts", parts),
        ("water", water),
    ]
}

// ── a. two offline checkpoints in a row ────────────────────────────────────────────────────

#[test]
fn a_two_offline_checkpoints_queue_the_same_change_once() {
    let start = all_five(50, 5, 20);
    let mut bp = enabled_backpack(&start);
    let mut t = Sim::new(&bp.last_settled_supplies);
    t.online = false;
    let sup = supplies(&all_five(47, 5, 20));

    let r1 = checkpoint(&mut bp, &sup, 11, &mut t);
    assert!(matches!(r1.status, SettleStatus::Pending { .. }));
    assert_eq!(bp.pending_settlements.len(), 1);
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("food", -3)]));

    // Checkpoint 2: nothing changed, still offline.
    checkpoint(&mut bp, &sup, 12, &mut t);
    assert_eq!(
        bp.pending_settlements.len(),
        1,
        "exactly one pending record for the one -3 change"
    );
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("food", -3)]));
    assert_eq!(bp.last_settled_supplies["food"], 50);
}

// ── b. recovery: one payment in total, a clean proof ───────────────────────────────────────

#[test]
fn b_recovery_pays_the_change_exactly_once_and_the_proof_is_clean() {
    let start = all_five(50, 5, 20);
    let mut bp = enabled_backpack(&start);
    let minted = bp.last_settled_supplies.clone();
    let mut t = Sim::new(&minted);
    let sup = supplies(&all_five(47, 5, 20));

    t.online = false;
    checkpoint(&mut bp, &sup, 11, &mut t);
    checkpoint(&mut bp, &sup, 12, &mut t);
    t.online = true;
    let r3 = checkpoint(&mut bp, &sup, 13, &mut t);

    assert_eq!(
        t.paid_for("food"),
        vec![-3],
        "exactly one food payment of 3 across the whole sequence"
    );
    assert_eq!(t.paid.len(), 1);
    assert_eq!(r3.status, SettleStatus::NoChanges);
    assert_eq!(bp.last_settled_supplies["food"], 47);
    assert!(bp.pending_settlements.is_empty());
    assert!(!bp.last_settle_failed);
    assert_eq!(bp.settlements.len(), 1);
    assert_eq!(bp.settlements[0].deltas, map(&[("food", -3)]));
    assert_eq!(proof(&bp, &minted, &t), ProofVerdict::Pass);
}

// ── c. moved back, and a mixed case ────────────────────────────────────────────────────────

#[test]
fn c_pending_minus_three_then_gaining_three_plans_plus_three() {
    let start = all_five(50, 5, 20);
    let mut bp = enabled_backpack(&start);
    let mut t = Sim::new(&bp.last_settled_supplies);
    t.online = false;
    checkpoint(&mut bp, &supplies(&all_five(47, 5, 20)), 11, &mut t);
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("food", -3)]));

    // Food is back to 50: effective baseline is 50 + (-3) = 47, so the plan is 50 - 47 = +3.
    // Not 0 (that would forget the queued -3 is still to be paid) and not -3 (the queued one).
    let back = supplies(&all_five(50, 5, 20));
    match plan_settlement(&bp, &back, RUN_ID, 12, "Town") {
        PlanOutcome::Batch(b) => assert_eq!(b.deltas, map(&[("food", 3)])),
        other => panic!("expected a +3 batch, got {other:?}"),
    }
    checkpoint(&mut bp, &back, 12, &mut t);
    assert_eq!(bp.pending_settlements.len(), 2);
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("food", -3)]));
    assert_eq!(bp.pending_settlements[1].deltas, map(&[("food", 3)]));

    // Recovery pays -3 then +3: net zero, baseline back at 50.
    t.online = true;
    checkpoint(&mut bp, &back, 13, &mut t);
    assert_eq!(t.paid_for("food"), vec![-3, 3]);
    assert_eq!(bp.last_settled_supplies["food"], 50);
    assert!(bp.pending_settlements.is_empty());
}

#[test]
fn c_mixed_pending_food_and_parts_then_more_food_and_water_plans_only_the_new_part() {
    let start = all_five(50, 5, 20);
    let mut bp = enabled_backpack(&start);
    let mut t = Sim::new(&bp.last_settled_supplies);
    t.online = false;
    checkpoint(&mut bp, &supplies(&all_five(47, 4, 20)), 11, &mut t);
    assert_eq!(
        bp.pending_settlements[0].deltas,
        map(&[("food", -3), ("parts", -1)])
    );

    // food 47 -> 45 (-2 more), water 20 -> 24 (+4), parts unchanged at 4 (already queued).
    let later = supplies(&all_five(45, 4, 24));
    match plan_settlement(&bp, &later, RUN_ID, 12, "Town") {
        PlanOutcome::Batch(b) => assert_eq!(b.deltas, map(&[("food", -2), ("water", 4)])),
        other => panic!("expected a food -2 / water +4 batch, got {other:?}"),
    }
    checkpoint(&mut bp, &later, 12, &mut t);
    assert_eq!(bp.pending_settlements.len(), 2);
    assert_eq!(
        bp.pending_settlements[1].deltas,
        map(&[("food", -2), ("water", 4)])
    );
}

// ── d. three offline checkpoints, a different change each ──────────────────────────────────

#[test]
fn d_three_offline_checkpoints_queue_each_change_once() {
    let start = all_five(50, 5, 20);
    let mut bp = enabled_backpack(&start);
    let mut t = Sim::new(&bp.last_settled_supplies);
    t.online = false;
    checkpoint(&mut bp, &supplies(&all_five(47, 5, 20)), 11, &mut t); // food -3
    checkpoint(&mut bp, &supplies(&all_five(47, 4, 20)), 12, &mut t); // parts -1
    checkpoint(&mut bp, &supplies(&all_five(45, 4, 20)), 13, &mut t); // food -2

    let got: Vec<BTreeMap<String, i32>> = bp
        .pending_settlements
        .iter()
        .map(|r| r.deltas.clone())
        .collect();
    assert_eq!(
        got,
        vec![
            map(&[("food", -3)]),
            map(&[("parts", -1)]),
            map(&[("food", -2)])
        ]
    );
    // Summed per key: food -5 (50 -> 45) and parts -1 (5 -> 4): each change appears once.
    let mut sum: BTreeMap<String, i32> = BTreeMap::new();
    for r in &bp.pending_settlements {
        for (k, v) in &r.deltas {
            *sum.entry(k.clone()).or_insert(0) += v;
        }
    }
    assert_eq!(sum, map(&[("food", -5), ("parts", -1)]));
    assert_eq!(bp.last_settled_supplies, map(&start));
}

// ── e. partial confirmation ────────────────────────────────────────────────────────────────

#[test]
fn e_partial_confirmation_moves_the_baseline_for_food_only_and_rope_stays_pending_once() {
    // "rope" in the brief is `parts` here: the five on-chain resources are ammo, food, meds,
    // parts and water.
    let start = all_five(50, 5, 20);
    let mut bp = enabled_backpack(&start);
    let mut t = Sim::new(&bp.last_settled_supplies);
    let sup = supplies(&all_five(47, 4, 20));

    t.online = false;
    checkpoint(&mut bp, &sup, 11, &mut t);
    assert_eq!(
        bp.pending_settlements[0].deltas,
        map(&[("food", -3), ("parts", -1)])
    );

    // Checkpoint 2: food confirms, parts keeps failing.
    t.online = true;
    t.fail_keys.insert("parts".to_string());
    let r2 = checkpoint(&mut bp, &sup, 12, &mut t);
    assert_eq!(t.paid_for("food"), vec![-3]);
    assert_eq!(bp.last_settled_supplies["food"], 47);
    assert_eq!(bp.last_settled_supplies["parts"], 5);
    assert_eq!(bp.pending_settlements.len(), 1);
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("parts", -1)]));
    assert_eq!(r2.retried_moved, 1);
    assert!(bp.last_settle_failed);

    // Checkpoint 3, nothing new: still exactly one pending record for parts, nothing new queued.
    let before = t.calls;
    checkpoint(&mut bp, &sup, 13, &mut t);
    assert_eq!(bp.pending_settlements.len(), 1);
    assert_eq!(bp.pending_settlements[0].deltas, map(&[("parts", -1)]));
    assert_eq!(
        t.calls - before,
        1,
        "only the retry of the one pending record is submitted; no fresh batch is planned"
    );

    // Parts recovers: one payment in total for parts.
    t.fail_keys.clear();
    checkpoint(&mut bp, &sup, 14, &mut t);
    assert_eq!(t.paid_for("parts"), vec![-1]);
    assert!(bp.pending_settlements.is_empty());
    assert_eq!(bp.last_settled_supplies["parts"], 4);
}

// ── f. sweep ───────────────────────────────────────────────────────────────────────────────

/// Tiny counter-driven generator (no RNG crate).
fn lcg(n: &mut u64) -> u64 {
    *n = n
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *n >> 33
}

#[test]
fn f_sweep_200_sequences_pay_each_supply_change_exactly_once() {
    let mut rng = 0xC0FFEE_u64;
    let mut violations: Vec<String> = Vec::new();
    for seq in 0..200usize {
        let initial = all_five(40, 40, 40);
        let mut bp = enabled_backpack(&initial);
        let minted = bp.last_settled_supplies.clone();
        let mut t = Sim::new(&minted);
        let checkpoints = 2 + (lcg(&mut rng) % 5) as usize; // 2..=6
                                                            // A scripted failure per transport call: offline, fail at the 2nd payment, or clean.
        for _ in 0..checkpoints * 2 {
            t.script.push_back(match lcg(&mut rng) % 4 {
                0 => Some(0),
                1 => Some(1),
                _ => None,
            });
        }
        let mut cur = minted.clone();
        // The nonzero per-checkpoint change of each key, in the order it happened.
        let mut steps: BTreeMap<String, Vec<i32>> = BTreeMap::new();
        for day in 0..checkpoints {
            for key in ["food", "water", "parts"] {
                let step = (lcg(&mut rng) % 9) as i32 - 4; // -4..=4
                *cur.get_mut(key).unwrap() += step;
                if step != 0 {
                    steps.entry(key.to_string()).or_default().push(step);
                }
            }
            let sup = supplies(
                &cur.iter()
                    .map(|(k, v)| (k.as_str(), *v))
                    .collect::<Vec<_>>(),
            );
            checkpoint(&mut bp, &sup, day as i32 + 1, &mut t);
        }
        // The transport finally recovers; one last checkpoint with no change.
        t.script.clear();
        let sup = supplies(
            &cur.iter()
                .map(|(k, v)| (k.as_str(), *v))
                .collect::<Vec<_>>(),
        );
        checkpoint(&mut bp, &sup, checkpoints as i32 + 1, &mut t);

        let mut bad: Vec<String> = Vec::new();
        for key in KEYS {
            let want = cur[key] - minted[key];
            if t.net(key) != want {
                bad.push(format!("{key}: paid {} want {want}", t.net(key)));
            }
        }
        // Nothing is paid twice: the payments for a key are exactly its nonzero per-checkpoint
        // changes, each once (compared as sorted lists; the retry order is not under test here).
        for key in KEYS {
            let mut want = steps.get(key).cloned().unwrap_or_default();
            let mut got = t.paid_for(key);
            want.sort_unstable();
            got.sort_unstable();
            if got != want {
                bad.push(format!("{key}: payments {got:?} != changes {want:?}"));
            }
        }
        if !bp.pending_settlements.is_empty() {
            bad.push("pending not empty".to_string());
        }
        if bp.last_settled_supplies != cur {
            bad.push("baseline != final supplies".to_string());
        }
        let txids: Vec<&String> = bp.settlements.iter().flat_map(|s| &s.txids).collect();
        let unique: BTreeSet<&String> = txids.iter().copied().collect();
        if txids.len() != unique.len() {
            bad.push("a txid was recorded twice".to_string());
        }
        if !bad.is_empty() {
            violations.push(format!("sequence {seq}: {}", bad.join("; ")));
        }
    }
    assert!(
        violations.is_empty(),
        "{} of 200 sequences violate the invariant; first: {:?}",
        violations.len(),
        violations.first()
    );
}

// ── note 6: last_settle_failed ─────────────────────────────────────────────────────────────

#[test]
fn last_settle_failed_is_cleared_when_nothing_is_pending_which_departs_from_python() {
    // Python's `_retry_pending` returns early (backpack.py:851) and leaves the flag when the queue
    // is already empty. Rust clears it: with nothing unsettled, a "last settlement failed" notice
    // would be false. This pins the departure.
    let mut bp = enabled_backpack(&all_five(50, 5, 20));
    bp.last_settle_failed = true;
    let mut t = Sim::new(&bp.last_settled_supplies);
    let moved = retry_pending(&mut bp, RUN_ID, "T", &mut t, &mut ReadLog::default());
    assert_eq!(moved, 0);
    assert!(!bp.last_settle_failed);
    assert_eq!(t.calls, 0);
}
