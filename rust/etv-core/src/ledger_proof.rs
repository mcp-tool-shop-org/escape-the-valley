//! Ledger reconciliation proof (Phase 2B): prove the on-ledger backpack matches the engine.
//!
//! Ported from the frozen Python `src/escape_the_valley/ledger_proof.py` (`reconcile`,
//! `proof_verdict`, `_is_inconclusive`, `report_to_dict`, `proof_player_save`). It is a pure
//! function over three inputs: engine truth (`last_settled_supplies`, the settlement log, the
//! mint snapshot), ledger balances, and settlement memos read back off the chain. No I/O.
//!
//! Three checks, as in Python:
//! 1. equal balances per token (on-ledger balance == engine's settled supply),
//! 2. conservation (`minted + sum of settlement deltas == final settled supply`),
//! 3. the memo prefix `TRAIL|RUN:<id>|DAY:<n>`, once on the engine-stored memo (local consistency
//!    only) and once on the memo decoded from the chain (the external check).
//!
//! Every failure kind has its own [`FailureCode`], so a consumer never has to parse a note.
//!
//! Additions beyond the Python (named so a reviewer can find them):
//! - [`FailureCode::DoubleSettlementTxid`]: the same txid recorded twice in the settlement log;
//! - [`FailureCode::SettlementNoTxids`] and [`FailureCode::SettlementMissingOnLedger`] are
//!   separate codes where Python only wrote a note;
//! - a corrupt engine-stored memo (`memo_local_ok` false) blocks a pass even when the external
//!   memo check passes; Python's `passed` ignored it in that case;
//! - a memo-fetch failure is treated as an unreachable ledger (INCONCLUSIVE) by
//!   [`read_ledger_view`], where Python's `fetch_onchain_memos` returned `{}` and the proof then
//!   failed every settlement as "no on-chain memo".
//!
//! Not ported: the Markdown renderer (`report_to_markdown`) and the faucet harness (`run_proof`).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::backpack::{fetch_balances, fetch_onchain_memos, token_code, LedgerTransport, ReadLog};
use crate::model::{BackpackState, SettlementRecord, XRPL_RESOURCES};

/// A distinct, stable code per failure kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FailureCode {
    /// A token has no on-ledger balance at all.
    LedgerBalanceMissing,
    /// The on-ledger balance differs from the engine's settled supply (tampered or drifted).
    LedgerBalanceMismatch,
    /// `minted + sum of deltas != final settled supply`.
    ConservationBroken,
    /// An engine-stored settlement memo does not start with `TRAIL|RUN:<id>|DAY:<n>`.
    MemoLocalPrefixMismatch,
    /// A settled record carries no txid, so nothing on the ledger can back it.
    SettlementNoTxids,
    /// A recorded txid has no memo on the ledger: the settlement is not there.
    SettlementMissingOnLedger,
    /// A decoded on-chain memo does not start with `TRAIL|RUN:<id>|DAY:<n>`.
    OnchainMemoPrefixMismatch,
    /// The same txid appears more than once in the settlement log.
    DoubleSettlementTxid,
    /// Checkpoints are still pending (unsettled); not drift on its own.
    SettlementsPending,
}

impl FailureCode {
    /// The stable string for the code.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LedgerBalanceMissing => "LEDGER_BALANCE_MISSING",
            Self::LedgerBalanceMismatch => "LEDGER_BALANCE_MISMATCH",
            Self::ConservationBroken => "CONSERVATION_BROKEN",
            Self::MemoLocalPrefixMismatch => "MEMO_LOCAL_PREFIX_MISMATCH",
            Self::SettlementNoTxids => "SETTLEMENT_NO_TXIDS",
            Self::SettlementMissingOnLedger => "SETTLEMENT_MISSING_ON_LEDGER",
            Self::OnchainMemoPrefixMismatch => "ONCHAIN_MEMO_PREFIX_MISMATCH",
            Self::DoubleSettlementTxid => "DOUBLE_SETTLEMENT_TXID",
            Self::SettlementsPending => "SETTLEMENTS_PENDING",
        }
    }
}

/// One failure: its code and the same text Python wrote as a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofFailure {
    pub code: FailureCode,
    pub detail: String,
}

/// Per-resource reconciliation (`ResourceCheck`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCheck {
    pub resource: String,
    pub code: String,
    pub minted: i32,
    pub sum_deltas: i32,
    pub engine_settled: i32,
    pub ledger: Option<i32>,
    pub balance_ok: bool,
    pub conservation_ok: bool,
}

impl ResourceCheck {
    pub fn ok(&self) -> bool {
        self.balance_ok && self.conservation_ok
    }
}

/// The three inputs plus identifiers (`reconcile`'s keyword arguments).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileInput {
    pub run_id: String,
    pub seed: i128,
    /// Enable-time mint snapshot, keyed by game key.
    pub minted_initial: BTreeMap<String, i32>,
    /// On-ledger balances keyed by XRPL currency code (`FOD`, `WTR`, ...).
    pub ledger_balances: BTreeMap<String, i32>,
    /// Engine's last-settled snapshot, keyed by game key.
    pub last_settled_supplies: BTreeMap<String, i32>,
    pub settlements: Vec<SettlementRecord>,
    pub pending: Vec<SettlementRecord>,
    pub player_address: String,
    pub issuer_address: String,
    /// txid -> decoded on-chain memo; `None` when no memos were fetched (offline reconcile).
    pub onchain_memos: Option<BTreeMap<String, String>>,
}

/// Full reconciliation verdict (`ReconcileReport`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    pub run_id: String,
    pub seed: i128,
    pub player_address: String,
    pub issuer_address: String,
    pub settlements_count: usize,
    pub pending_count: usize,
    pub txids: Vec<String>,
    pub resources: Vec<ResourceCheck>,
    /// Authoritative memo verdict: external when on-chain memos were supplied, else local.
    pub memo_ok: bool,
    pub passed: bool,
    pub notes: Vec<String>,
    pub memo_local_ok: bool,
    pub onchain_memo_ok: Option<bool>,
    /// Structured failures, one per failing check, each with its own code.
    pub failures: Vec<ProofFailure>,
    /// True when the same txid is recorded more than once.
    pub double_settlement: bool,
}

impl ReconcileReport {
    /// The set of failure codes in this report.
    pub fn codes(&self) -> BTreeSet<FailureCode> {
        self.failures.iter().map(|f| f.code).collect()
    }
}

/// Python's `repr()` of a string, so the notes stay text-identical to the legacy ones: single
/// quotes, unless the text holds a single quote and no double quote.
fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// True when `memo` starts with `prefix` at a field boundary: the next character is `|` or the
/// memo ends. Python used a bare `startswith`, under which the day-1 prefix `...|DAY:1` would also
/// accept a day-11 memo; a real memo always has `|` after the day.
fn has_prefix(memo: &str, prefix: &str) -> bool {
    memo.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('|'))
}

/// Reconcile on-ledger balances against the engine's settled snapshot (`reconcile`).
pub fn reconcile(input: &ReconcileInput) -> ReconcileReport {
    let mut notes: Vec<String> = Vec::new();
    let mut failures: Vec<ProofFailure> = Vec::new();
    let mut fail = |notes: &mut Vec<String>, code: FailureCode, text: String| {
        notes.push(text.clone());
        failures.push(ProofFailure { code, detail: text });
    };

    let mut sum_deltas: BTreeMap<&str, i32> = XRPL_RESOURCES.iter().map(|k| (*k, 0)).collect();
    for rec in &input.settlements {
        for (key, val) in &rec.deltas {
            if let Some(s) = sum_deltas.get_mut(key.as_str()) {
                *s = s.saturating_add(*val);
            }
        }
    }

    let mut resources = Vec::new();
    for key in XRPL_RESOURCES {
        let code = token_code(key).expect("resource has a token code");
        let minted = input.minted_initial.get(key).copied().unwrap_or(0);
        let settled = input.last_settled_supplies.get(key).copied().unwrap_or(0);
        let sdelta = sum_deltas[key];
        let ledger = input.ledger_balances.get(code).copied();
        let balance_ok = ledger == Some(settled);
        let conservation_ok = minted.saturating_add(sdelta) == settled;
        match ledger {
            None => fail(
                &mut notes,
                FailureCode::LedgerBalanceMissing,
                format!("{key}: no on-ledger balance for {code}"),
            ),
            Some(l) if l != settled => fail(
                &mut notes,
                FailureCode::LedgerBalanceMismatch,
                format!("{key}: ledger {l} != engine settled {settled} ({code})"),
            ),
            _ => {}
        }
        if !conservation_ok {
            fail(
                &mut notes,
                FailureCode::ConservationBroken,
                format!(
                    "{key}: minted {minted} + deltas {sdelta} ({}) != settled {settled}",
                    minted.saturating_add(sdelta)
                ),
            );
        }
        resources.push(ResourceCheck {
            resource: key.to_string(),
            code: code.to_string(),
            minted,
            sum_deltas: sdelta,
            engine_settled: settled,
            ledger,
            balance_ok,
            conservation_ok,
        });
    }

    // Local consistency: the engine-stored memo begins with run + its own day. The engine writes
    // this string, so it is consistency, not integrity (ledger-003).
    let mut memo_local_ok = true;
    for rec in &input.settlements {
        let prefix = format!("TRAIL|RUN:{}|DAY:{}", input.run_id, rec.day);
        if !has_prefix(&rec.memo, &prefix) {
            memo_local_ok = false;
            fail(
                &mut notes,
                FailureCode::MemoLocalPrefixMismatch,
                format!(
                    "settlement day {}: stored memo {} does not start with {} (local consistency)",
                    rec.day,
                    py_repr(&rec.memo),
                    py_repr(&prefix)
                ),
            );
        }
    }

    // External integrity: the decoded on-chain memo, keyed by txid.
    let onchain_memo_ok = match &input.onchain_memos {
        None => {
            notes.push(
                "memo: no on-chain memos supplied \u{2014} external integrity NOT verified; \
                 memo_ok reflects local consistency only"
                    .to_string(),
            );
            None
        }
        Some(memos) => {
            let mut ok = true;
            for rec in &input.settlements {
                let prefix = format!("TRAIL|RUN:{}|DAY:{}", input.run_id, rec.day);
                if rec.txids.is_empty() {
                    // A settled record with no txid would pass vacuously (ledger-A02).
                    ok = false;
                    fail(
                        &mut notes,
                        FailureCode::SettlementNoTxids,
                        format!(
                            "settlement day {}: no txids to verify on-chain (external integrity)",
                            rec.day
                        ),
                    );
                    continue;
                }
                for txid in &rec.txids {
                    match memos.get(txid) {
                        None => {
                            ok = false;
                            fail(
                                &mut notes,
                                FailureCode::SettlementMissingOnLedger,
                                format!(
                                    "settlement day {}: no on-chain memo for txid {} (external integrity)",
                                    rec.day, py_repr(txid)
                                ),
                            );
                        }
                        Some(actual) if !has_prefix(actual, &prefix) => {
                            ok = false;
                            fail(
                                &mut notes,
                                FailureCode::OnchainMemoPrefixMismatch,
                                format!(
                                    "settlement day {}: on-chain memo {} does not start with {} (external integrity)",
                                    rec.day, py_repr(actual), py_repr(&prefix)
                                ),
                            );
                        }
                        Some(_) => {}
                    }
                }
            }
            Some(ok)
        }
    };
    let memo_ok = onchain_memo_ok.unwrap_or(memo_local_ok);

    // Addition beyond Python: the same txid recorded twice is a double settlement.
    let mut txids: Vec<String> = Vec::new();
    for rec in &input.settlements {
        txids.extend(rec.txids.iter().cloned());
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &txids {
        *counts.entry(t.as_str()).or_insert(0) += 1;
    }
    let mut double_settlement = false;
    for (txid, n) in counts {
        if n > 1 {
            double_settlement = true;
            fail(
                &mut notes,
                FailureCode::DoubleSettlementTxid,
                format!(
                    "txid {} recorded {n} times (double settlement)",
                    py_repr(txid)
                ),
            );
        }
    }

    if !input.pending.is_empty() {
        fail(
            &mut notes,
            FailureCode::SettlementsPending,
            format!(
                "{} settlement(s) still pending (unsettled on ledger)",
                input.pending.len()
            ),
        );
    }

    // Beyond Python: a corrupt engine-stored memo also blocks a pass. Python let an external memo
    // match override it (`passed` ignored `memo_local_ok`).
    let passed = resources.iter().all(ResourceCheck::ok)
        && memo_ok
        && memo_local_ok
        && !double_settlement
        && input.pending.is_empty();

    ReconcileReport {
        run_id: input.run_id.clone(),
        seed: input.seed,
        player_address: input.player_address.clone(),
        issuer_address: input.issuer_address.clone(),
        settlements_count: input.settlements.len(),
        pending_count: input.pending.len(),
        txids,
        resources,
        memo_ok,
        passed,
        notes,
        memo_local_ok,
        onchain_memo_ok,
        failures,
        double_settlement,
    }
}

/// PASS, FAIL or INCONCLUSIVE (`ProofVerdict`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofVerdict {
    Pass,
    Fail,
    Inconclusive,
}

impl ProofVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Inconclusive => "INCONCLUSIVE",
        }
    }
}

/// True when the ONLY reason the proof did not pass is unsettled checkpoints (`_is_inconclusive`,
/// ledger-B06). Drift is never inconclusive.
pub fn is_inconclusive(report: &ReconcileReport) -> bool {
    if report.passed {
        return false;
    }
    report.resources.iter().all(ResourceCheck::ok)
        && report.memo_ok
        && report.memo_local_ok
        && !report.double_settlement
        && report.pending_count > 0
}

/// Classify a report (`proof_verdict`). ANDON: drift is always FAIL.
pub fn proof_verdict(
    report: &ReconcileReport,
    network_error: bool,
    minted_missing: bool,
) -> ProofVerdict {
    if network_error {
        return ProofVerdict::Inconclusive;
    }
    if report.passed {
        return if minted_missing {
            ProofVerdict::Inconclusive
        } else {
            ProofVerdict::Pass
        };
    }
    if is_inconclusive(report) {
        return ProofVerdict::Inconclusive;
    }
    ProofVerdict::Fail
}

/// Serialize a report exactly as Python's `report_to_dict` does (no secrets). The structured
/// failures are not in this dict; read [`ReconcileReport::failures`].
pub fn report_to_value(report: &ReconcileReport) -> Value {
    json!({
        "run_id": report.run_id,
        "seed": report.seed.to_string().parse::<i64>().unwrap_or(0),
        "player_address": report.player_address,
        "issuer_address": report.issuer_address,
        "settlements_count": report.settlements_count,
        "pending_count": report.pending_count,
        "txids": report.txids,
        "memo_ok": report.memo_ok,
        "memo_local_ok": report.memo_local_ok,
        "onchain_memo_ok": report.onchain_memo_ok,
        "passed": report.passed,
        "inconclusive": is_inconclusive(report),
        "notes": report.notes,
        "resources": report.resources.iter().map(|r| json!({
            "resource": r.resource,
            "code": r.code,
            "minted": r.minted,
            "sum_deltas": r.sum_deltas,
            "engine_settled": r.engine_settled,
            "ledger": r.ledger,
            "balance_ok": r.balance_ok,
            "conservation_ok": r.conservation_ok,
        })).collect::<Vec<_>>(),
    })
}

/// What the ledger side returned for a loaded save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerView {
    /// Balances by currency code; `None` when AccountLines (or the memo read) could not be done.
    pub balances: Option<BTreeMap<String, i32>>,
    pub onchain_memos: Option<BTreeMap<String, String>>,
}

/// Read the ledger side through a transport (`wallet_info` + `fetch_onchain_memos`). Any failure
/// is an unreachable ledger, which the proof reports as INCONCLUSIVE.
pub fn read_ledger_view<T: LedgerTransport + ?Sized>(
    transport: &mut T,
    bp: &BackpackState,
    log: &mut ReadLog,
) -> LedgerView {
    let balances = fetch_balances(transport, &bp.wallet_address, &bp.issuer_address, log).ok();
    let onchain_memos = if balances.is_some() {
        fetch_onchain_memos(transport, &bp.wallet_address, &bp.issuer_address, log).ok()
    } else {
        None
    };
    if balances.is_some() && onchain_memos.is_none() {
        return LedgerView {
            balances: None,
            onchain_memos: None,
        };
    }
    LedgerView {
        balances,
        onchain_memos,
    }
}

/// Result of proving a loaded save (`PlayerProofResult`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerProofResult {
    pub verdict: ProofVerdict,
    pub report: Option<ReconcileReport>,
    pub notes: Vec<String>,
}

/// Reconcile a loaded save against what the ledger returned (`proof_player_save`, with the
/// ledger reads already done). Network misses and a missing mint snapshot are INCONCLUSIVE;
/// real drift is FAIL.
pub fn prove_player_save(
    run_id: &str,
    seed: i128,
    bp: &BackpackState,
    view: &LedgerView,
) -> PlayerProofResult {
    if !bp.enabled || bp.wallet_address.is_empty() {
        return PlayerProofResult {
            verdict: ProofVerdict::Inconclusive,
            report: None,
            notes: vec!["backpack not enabled on this save".to_string()],
        };
    }
    let mut bp = bp.clone();
    let mut notes: Vec<String> = Vec::new();

    let mut minted = bp.minted_snapshot_of();
    let minted_missing = !XRPL_RESOURCES.iter().all(|k| minted.contains_key(*k));
    if minted_missing {
        // Reconstruct minted = last_settled - sum(deltas): conservation becomes tautological, so a
        // clean match can only be INCONCLUSIVE.
        let mut sum: BTreeMap<&str, i32> = XRPL_RESOURCES.iter().map(|k| (*k, 0)).collect();
        for rec in &bp.settlements {
            for (key, val) in &rec.deltas {
                if let Some(s) = sum.get_mut(key.as_str()) {
                    *s = s.saturating_add(*val);
                }
            }
        }
        minted = XRPL_RESOURCES
            .iter()
            .map(|k| {
                let settled = bp.last_settled_supplies.get(*k).copied().unwrap_or(0);
                (k.to_string(), settled.saturating_sub(sum[k]))
            })
            .collect();
        notes.push(
            "mint snapshot missing from this save \u{2014} conservation is reconstructed \
             (tautological); live balance + on-chain memo checks still apply"
                .to_string(),
        );
    }

    let network_error = view.balances.is_none();
    if network_error {
        notes.push("could not reach the ledger (AccountLines); not a drift FAIL".to_string());
    }

    let last_settled: BTreeMap<String, i32> = XRPL_RESOURCES
        .iter()
        .map(|k| {
            (
                k.to_string(),
                bp.last_settled_supplies.get(*k).copied().unwrap_or(0),
            )
        })
        .collect();
    let input = ReconcileInput {
        run_id: run_id.to_string(),
        seed,
        minted_initial: minted,
        ledger_balances: view.balances.clone().unwrap_or_default(),
        last_settled_supplies: last_settled,
        settlements: bp.settlements.clone(),
        pending: bp.pending_settlements.clone(),
        player_address: bp.wallet_address.clone(),
        issuer_address: bp.issuer_address.clone(),
        onchain_memos: if network_error {
            None
        } else {
            view.onchain_memos.clone()
        },
    };
    let mut report = reconcile(&input);
    let mut all_notes = notes;
    all_notes.append(&mut report.notes);
    report.notes = all_notes.clone();
    let verdict = proof_verdict(&report, network_error, minted_missing);
    PlayerProofResult {
        verdict,
        report: Some(report),
        notes: all_notes,
    }
}
