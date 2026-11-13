//! Ledger Backpack, pure settlement logic (Phase 2B).
//!
//! Ported from the frozen Python legacy (`src/escape_the_valley/backpack.py` and
//! `backpack_models.py`). Everything here is a pure function over a [`BackpackState`], the
//! engine's supplies and a settlement log. There is no I/O: the only door to a ledger is the
//! [`LedgerTransport`] trait, which has **no network implementation** in this crate. The
//! [`RecordedTransport`] replays fixtures shaped like the XRPL responses the Python code reads.
//!
//! XRPL is Testnet only. Nothing in this module opens a socket, and the default build has no
//! HTTP or XRPL client dependency.
//!
//! What is ported (Python function -> Rust):
//! - `_settlement_memo_text` / `_build_memo` / `_hex_encode` / `_hex_decode` ->
//!   [`settlement_memo_text`], [`build_memo`], [`hex_encode`], [`hex_decode`]
//! - `_balance_to_int` -> [`balance_to_int`]
//! - `stamp_minted_snapshot` -> [`stamp_minted_snapshot`]
//! - the delta and batch planning in `BackpackManager.settle` -> [`compute_deltas`],
//!   [`plan_settlement`]
//! - the per-payment fold in `settle` and `_retry_pending` (ledger-CRIT-2, ledger-003, ENG-A-08,
//!   F-86a4c19c) -> [`fold_fresh_settlement`], [`retry_pending`], [`strip_unknown_pending_keys`]
//! - `wallet_info`'s AccountLines read and `fetch_onchain_memos`' AccountTx read ->
//!   [`fetch_balances`], [`fetch_onchain_memos`]
//!
//! Not ported in this phase: wallet creation, faucet, trust lines and minting (`enable`), parcels
//! (`send_parcel`, `check_parcels`, `accept_parcel`), and the Testnet host allow-list, which belongs
//! with the future network implementation. Submission order inside one batch is the sorted game-key
//! order here; Python iterated a `set`, so its payment order was unspecified (the memo, which is
//! sorted, is identical).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::{
    BackpackState, PermitRecord, SettlementRecord, SuppliesState, MEMO_SCHEMA_VERSION,
    MINTED_SNAPSHOT_PERMIT_ID, XRPL_RESOURCES,
};

/// Game key -> (XRPL 3-char currency code, 4-char display label)
/// (`backpack_models.py::XRPL_TOKEN_MAP`, same order as Python).
pub const XRPL_TOKEN_MAP: [(&str, &str, &str); 5] = [
    ("food", "FOD", "FOOD"),
    ("water", "WTR", "WATR"),
    ("meds", "MED", "MEDS"),
    ("ammo", "AMO", "AMMO"),
    ("parts", "PRT", "PART"),
];

/// Page cap for AccountTx pagination (`backpack.py::fetch_onchain_memos`, `max_pages = 100`).
pub const MAX_ACCOUNT_TX_PAGES: usize = 100;

/// XRPL currency code for a game key, or `None` for a key that is not tracked on-chain.
pub fn token_code(key: &str) -> Option<&'static str> {
    XRPL_TOKEN_MAP
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, code, _)| *code)
}

// ── Memo grammar (backpack.py:138-179) ──────────────────────────────────────────────────────

/// Uppercase hex of the UTF-8 bytes (`_hex_encode`, backpack.py:138).
pub fn hex_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for b in text.as_bytes() {
        out.push_str(&format!("{b:02X}"));
    }
    out
}

/// Decode hex into UTF-8 text (`_hex_decode`). Errors on odd length, a non-hex digit or
/// invalid UTF-8, where Python raises `ValueError` / `UnicodeDecodeError`.
pub fn hex_decode(hex: &str) -> Result<String, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("odd-length hex".to_string());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let raw = hex.as_bytes();
    for pair in raw.chunks(2) {
        let s = std::str::from_utf8(pair).map_err(|_| "non-ascii hex".to_string())?;
        let b = u8::from_str_radix(s, 16).map_err(|_| format!("non-hex digit in {s:?}"))?;
        bytes.push(b);
    }
    String::from_utf8(bytes).map_err(|_| "invalid utf-8".to_string())
}

/// The exact bytes written on-chain as the settlement memo (`_settlement_memo_text`,
/// backpack.py:143-168): `TRAIL|RUN:<id>|DAY:<n>|DELTA:<CODE><signed>,...|V:v1`.
///
/// Deltas are sorted by game key; a key outside the token map is skipped; a positive delta is
/// written with an explicit `+`.
pub fn settlement_memo_text(run_id: &str, day: i32, deltas: &BTreeMap<String, i32>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (key, diff) in deltas {
        let Some(code) = token_code(key) else {
            continue;
        };
        let sign = if *diff > 0 { "+" } else { "" };
        parts.push(format!("{code}{sign}{diff}"));
    }
    format!(
        "TRAIL|RUN:{run_id}|DAY:{day}|DELTA:{}|V:{MEMO_SCHEMA_VERSION}",
        parts.join(",")
    )
}

/// One XRPL memo as it is signed: hex `MemoData` and hex `MemoType` (`_build_memo`,
/// backpack.py:171-178).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoField {
    pub memo_data: String,
    pub memo_type: String,
}

/// Build the memo for a settlement transaction (`_build_memo`).
pub fn build_memo(run_id: &str, day: i32, deltas: &BTreeMap<String, i32>) -> MemoField {
    MemoField {
        memo_data: hex_encode(&settlement_memo_text(run_id, day, deltas)),
        memo_type: hex_encode("text/plain"),
    }
}

// ── IOU balance parsing (backpack.py:201-213) ───────────────────────────────────────────────

/// Parse an IOU balance string to an exact integer without binary floating point
/// (`_balance_to_int`, ledger-004). A non-integer balance is an error, not a truncation: it is
/// itself a drift signal. Accepts an optional sign, a decimal point and an exponent.
pub fn balance_to_int(balance: &str) -> Result<i32, String> {
    let s = balance.trim();
    let bad = || format!("non-integer IOU balance: {balance:?}");
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (mant, exp) = match rest.find(['e', 'E']) {
        Some(i) => {
            let e: i32 = rest[i + 1..].parse().map_err(|_| bad())?;
            (&rest[..i], e)
        }
        None => (rest, 0),
    };
    let (int_part, frac_part) = match mant.split_once('.') {
        Some((a, b)) => (a, b),
        None => (mant, ""),
    };
    if (int_part.is_empty() && frac_part.is_empty())
        || !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    let mut digits = format!("{int_part}{frac_part}");
    let shift = exp - frac_part.len() as i32; // value = digits * 10^shift
    if shift >= 0 {
        if digits.trim_start_matches('0').is_empty() {
            digits = "0".to_string();
        } else {
            if shift > 12 {
                return Err(bad());
            }
            digits.push_str(&"0".repeat(shift as usize));
        }
    } else {
        let cut = (-shift) as usize;
        if cut > digits.len() {
            if digits.chars().all(|c| c == '0') {
                digits = "0".to_string();
            } else {
                return Err(bad());
            }
        } else {
            let (head, tail) = digits.split_at(digits.len() - cut);
            if !tail.chars().all(|c| c == '0') {
                return Err(bad());
            }
            digits = if head.is_empty() {
                "0".to_string()
            } else {
                head.to_string()
            };
        }
    }
    let magnitude: i64 = digits.parse().map_err(|_| bad())?;
    let signed = if neg { -magnitude } else { magnitude };
    i32::try_from(signed).map_err(|_| bad())
}

// ── Mint snapshot (backpack_models.py:112-170) ──────────────────────────────────────────────

/// Encode a mint snapshot as the reserved permit's `txid` (`_encode_minted_txid`):
/// `key=value` pairs over the five resources in sorted key order.
pub fn encode_minted_txid(snapshot: &BTreeMap<String, i32>) -> String {
    XRPL_RESOURCES
        .iter()
        .map(|k| format!("{k}={}", snapshot.get(*k).copied().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(",")
}

/// Freeze the enable-time mint snapshot and mirror it into the reserved permit
/// (`stamp_minted_snapshot`). Only a complete five-resource snapshot is stamped.
pub fn stamp_minted_snapshot(bp: &mut BackpackState, snapshot: Option<&BTreeMap<String, i32>>) {
    let data = snapshot
        .cloned()
        .unwrap_or_else(|| bp.minted_initial.clone());
    if !XRPL_RESOURCES.iter().all(|k| data.contains_key(*k)) {
        return;
    }
    let complete: BTreeMap<String, i32> = XRPL_RESOURCES
        .iter()
        .map(|k| (k.to_string(), data[*k]))
        .collect();
    bp.minted_initial = complete.clone();
    let encoded = encode_minted_txid(&complete);
    bp.permits
        .retain(|p| p.permit_id != MINTED_SNAPSHOT_PERMIT_ID);
    bp.permits.push(PermitRecord {
        permit_id: MINTED_SNAPSHOT_PERMIT_ID.to_string(),
        txid: encoded,
        used: true,
        day_earned: bp.last_settlement_day,
        day_used: 0,
    });
}

// ── Planning (pure) ─────────────────────────────────────────────────────────────────────────

/// Supply deltas since the last settlement (backpack.py:639-646). Keys are the five XRPL
/// resources; a missing baseline is 0; zero diffs are omitted.
pub fn compute_deltas(
    supplies: &SuppliesState,
    last_settled: &BTreeMap<String, i32>,
) -> BTreeMap<String, i32> {
    let mut out = BTreeMap::new();
    for key in XRPL_RESOURCES {
        let current = supplies.get(key);
        let previous = last_settled.get(key).copied().unwrap_or(0);
        let diff = current.saturating_sub(previous);
        if diff != 0 {
            out.insert(key.to_string(), diff);
        }
    }
    out
}

/// Which way a settlement payment moves tokens (backpack.py:680-705).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentDirection {
    /// The player lost supplies: tokens go back to the issuer.
    PlayerToIssuer,
    /// The player gained supplies: the issuer sends tokens to the player.
    IssuerToPlayer,
}

/// One issued-currency Payment to submit. Carries no secret: signing is the future network
/// implementation's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRequest {
    pub key: String,
    pub currency: String,
    pub issuer: String,
    /// Absolute amount (`str(abs(diff))` in Python).
    pub value: u32,
    pub direction: PaymentDirection,
    pub account: String,
    pub destination: String,
    pub memo: MemoField,
}

/// A settlement batch: every payment carries the same memo, which names the FULL delta set
/// (ledger-003, backpack.py:668-673).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementBatch {
    pub run_id: String,
    pub day: i32,
    pub location: String,
    pub deltas: BTreeMap<String, i32>,
    pub memo_text: String,
    pub memo: MemoField,
    pub payments: Vec<PaymentRequest>,
}

/// Build the batch for a delta set (shared by a fresh settle and a pending retry).
pub fn build_batch(
    bp: &BackpackState,
    run_id: &str,
    day: i32,
    location: &str,
    deltas: &BTreeMap<String, i32>,
) -> SettlementBatch {
    let memo = build_memo(run_id, day, deltas);
    let mut payments = Vec::new();
    for (key, diff) in deltas {
        let Some(code) = token_code(key) else {
            continue;
        };
        let (direction, account, destination) = if *diff < 0 {
            (
                PaymentDirection::PlayerToIssuer,
                bp.wallet_address.clone(),
                bp.issuer_address.clone(),
            )
        } else {
            (
                PaymentDirection::IssuerToPlayer,
                bp.issuer_address.clone(),
                bp.wallet_address.clone(),
            )
        };
        payments.push(PaymentRequest {
            key: key.clone(),
            currency: code.to_string(),
            issuer: bp.issuer_address.clone(),
            value: diff.unsigned_abs(),
            direction,
            account,
            destination,
            memo: memo.clone(),
        });
    }
    SettlementBatch {
        run_id: run_id.to_string(),
        day,
        location: location.to_string(),
        deltas: deltas.clone(),
        memo_text: settlement_memo_text(run_id, day, deltas),
        memo,
        payments,
    }
}

/// Result of planning a fresh checkpoint settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanOutcome {
    /// Backpack off or no wallet (`settle` returns "Backpack not enabled").
    NotEnabled,
    /// Nothing changed since the last settlement ("No changes to settle.").
    NoChanges,
    Batch(SettlementBatch),
}

/// Per-key sum of the deltas of every outstanding pending record, in the supply sign convention
/// (a negative number is supplies the player lost). Keys no record names are absent.
pub fn pending_deltas(bp: &BackpackState) -> BTreeMap<String, i32> {
    let mut out: BTreeMap<String, i32> = BTreeMap::new();
    for rec in &bp.pending_settlements {
        for (key, val) in &rec.deltas {
            let slot = out.entry(key.clone()).or_insert(0);
            *slot = slot.saturating_add(*val);
        }
    }
    out
}

/// The supplies the ledger will hold once every queued payment lands: `last_settled_supplies`
/// plus the pending deltas, per key (a key absent from either side counts as 0).
pub fn effective_baseline(bp: &BackpackState) -> BTreeMap<String, i32> {
    let mut out = bp.last_settled_supplies.clone();
    for (key, val) in pending_deltas(bp) {
        let slot = out.entry(key).or_insert(0);
        *slot = slot.saturating_add(val);
    }
    out
}

/// Plan a checkpoint settlement: the delta since the last settlement, as one batch
/// (`BackpackManager.settle`, backpack.py:630-675). Pure: it reads, never mutates.
///
/// Planning rule (a deliberate departure from Python, which re-plans a change that is already
/// queued): the delta is `current - (last_settled + sum of pending deltas)`, per key. A supply
/// change already covered by a pending record is therefore never queued a second time, however
/// many checkpoints pass while the transport is offline; if the supply moves back, the new
/// delta is the move back (pending -3, then +3 gained, plans +3).
///
/// Idempotency: once a batch's payments are folded into `last_settled_supplies`, planning again
/// over the same supplies returns [`PlanOutcome::NoChanges`]; so does planning again while the
/// same batch sits in the pending queue.
pub fn plan_settlement(
    bp: &BackpackState,
    supplies: &SuppliesState,
    run_id: &str,
    day: i32,
    location: &str,
) -> PlanOutcome {
    if !bp.enabled || bp.wallet_address.is_empty() {
        return PlanOutcome::NotEnabled;
    }
    let deltas = compute_deltas(supplies, &effective_baseline(bp));
    if deltas.is_empty() {
        return PlanOutcome::NoChanges;
    }
    PlanOutcome::Batch(build_batch(bp, run_id, day, location, &deltas))
}

// ── Transport trait and response shapes ─────────────────────────────────────────────────────

/// Why a transport call produced no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The ledger could not be reached (the offline path).
    Unreachable(String),
    /// A [`RecordedTransport`] was asked for a call it holds no recording for.
    NoRecording(String),
}

/// The outcome of one payment in a batch: the game key and the XRPL `result` object
/// (`resp.result` in Python) or an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentAttempt {
    pub key: String,
    pub result: Result<Value, TransportError>,
}

/// The only door to a ledger. **No network implementation exists in this crate.** Every method
/// returns the XRPL `result` object exactly as the Python client reads it, so the shape checks
/// live in the core ([`parse_account_lines`] and friends), not in the transport.
pub trait LedgerTransport {
    /// AccountLines `result` for `account` (`client.request(AccountLines(account=...))`).
    fn balances(&mut self, account: &str) -> Result<Value, TransportError>;

    /// Submit ONE payment of `batch` (the one at `index` in `batch.payments`) and return its
    /// outcome once the ledger has confirmed or refused it. The core calls this once per payment,
    /// saves state between calls, and stops at the first failure, so a transport never sees the
    /// payments after a failed one and need not track the batch. `batch` is context only (its
    /// full-batch memo, run and day); the payment itself carries everything that is signed.
    fn submit_payment(&mut self, batch: &SettlementBatch, index: usize) -> PaymentAttempt;

    /// One AccountTx page `result` for `account`, resuming at `marker` when given.
    fn tx_memos(&mut self, account: &str, marker: Option<&Value>) -> Result<Value, TransportError>;
}

/// One read of a response field, for the fixture-shape test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRead {
    pub path: String,
    pub found: bool,
    pub optional: bool,
}

/// Log of every field the parsers read. A test compares it with the fixture: a required read
/// that is absent is an error, and an optional read must be found at least once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadLog {
    pub reads: Vec<FieldRead>,
}

impl ReadLog {
    fn note(&mut self, path: &str, found: bool, optional: bool) {
        self.reads.push(FieldRead {
            path: path.to_string(),
            found,
            optional,
        });
    }
}

/// A response did not have the shape the Python client reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeError {
    MissingField(String),
    WrongType(String),
}

/// Read a field the Python client reads without a default. Logged; absent is an error.
pub fn read_required<'a>(
    v: &'a Value,
    name: &str,
    path: &str,
    log: &mut ReadLog,
) -> Result<&'a Value, ShapeError> {
    match v.get(name) {
        Some(x) => {
            log.note(path, true, false);
            Ok(x)
        }
        None => {
            log.note(path, false, false);
            Err(ShapeError::MissingField(path.to_string()))
        }
    }
}

/// Read a field the Python client reads with a default (`.get`). Logged either way.
pub fn read_optional<'a>(
    v: &'a Value,
    name: &str,
    path: &str,
    log: &mut ReadLog,
) -> Option<&'a Value> {
    let x = v.get(name).filter(|x| !x.is_null());
    log.note(path, x.is_some(), true);
    x
}

fn req_str<'a>(
    v: &'a Value,
    name: &str,
    path: &str,
    log: &mut ReadLog,
) -> Result<&'a str, ShapeError> {
    read_required(v, name, path, log)?
        .as_str()
        .ok_or_else(|| ShapeError::WrongType(path.to_string()))
}

/// One trust line (`wallet_info`, backpack.py:1402-1406). Python reads `account`, `currency`,
/// `balance`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustLine {
    pub account: String,
    pub currency: String,
    pub balance: String,
}

/// Parse an AccountLines `result` (`resp.result.get("lines", [])`, backpack.py:1402).
pub fn parse_account_lines(
    result: &Value,
    log: &mut ReadLog,
) -> Result<Vec<TrustLine>, ShapeError> {
    let lines = read_required(result, "lines", "account_lines.lines", log)?
        .as_array()
        .ok_or_else(|| ShapeError::WrongType("account_lines.lines".to_string()))?;
    let mut out = Vec::new();
    for line in lines {
        out.push(TrustLine {
            account: req_str(line, "account", "account_lines.lines[].account", log)?.to_string(),
            currency: req_str(line, "currency", "account_lines.lines[].currency", log)?.to_string(),
            balance: req_str(line, "balance", "account_lines.lines[].balance", log)?.to_string(),
        });
    }
    Ok(out)
}

/// One AccountTx transaction entry as `fetch_onchain_memos` reads it (backpack.py:1336-1346).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxEntry {
    pub hash: String,
    /// `Memos[0].Memo.MemoData` when the transaction has a memo.
    pub memo_data: Option<String>,
}

/// One AccountTx page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxPage {
    pub entries: Vec<TxEntry>,
    /// Opaque continuation marker; `None` means the account is fully paged (backpack.py:1351).
    pub marker: Option<Value>,
}

/// Parse an AccountTx `result` (backpack.py:1335-1352).
///
/// Python reads `tx = tx_entry.get("tx", tx_entry.get("tx_json", {}))` and
/// `tx_hash = tx_entry.get("hash") or tx.get("hash", "")` because the tx-hash location varies
/// with AccountTx `api_version` (ledger-007). Both shapes are accepted; a transaction with
/// neither `tx` nor `tx_json`, or with a hash in neither place, is a shape error.
pub fn parse_account_tx_page(result: &Value, log: &mut ReadLog) -> Result<TxPage, ShapeError> {
    let txs = read_required(result, "transactions", "account_tx.transactions", log)?
        .as_array()
        .ok_or_else(|| ShapeError::WrongType("account_tx.transactions".to_string()))?;
    let mut entries = Vec::new();
    for entry in txs {
        let tx = match read_optional(entry, "tx", "account_tx.transactions[].tx", log) {
            Some(t) => t,
            None => read_required(entry, "tx_json", "account_tx.transactions[].tx_json", log)
                .map_err(|_| ShapeError::MissingField("transactions[].tx|tx_json".to_string()))?,
        };
        let hash = match read_optional(entry, "hash", "account_tx.transactions[].hash", log) {
            Some(h) => h,
            None => read_required(tx, "hash", "account_tx.transactions[].tx.hash", log)
                .map_err(|_| ShapeError::MissingField("transactions[].hash".to_string()))?,
        }
        .as_str()
        .ok_or_else(|| ShapeError::WrongType("transactions[].hash".to_string()))?
        .to_string();
        let memo_data = match read_optional(tx, "Memos", "account_tx.transactions[].tx.Memos", log)
        {
            Some(memos) => memos
                .as_array()
                .and_then(|a| a.first())
                .and_then(|first| {
                    first.get("Memo").and_then(|m| {
                        read_optional(
                            m,
                            "MemoData",
                            "account_tx.transactions[].tx.Memos[0].Memo.MemoData",
                            log,
                        )
                    })
                })
                .and_then(|d| d.as_str())
                .map(str::to_string),
            None => None,
        };
        entries.push(TxEntry { hash, memo_data });
    }
    let marker = read_optional(result, "marker", "account_tx.marker", log).cloned();
    Ok(TxPage { entries, marker })
}

/// Parse a submit result: Python reads `resp.result.get("hash", "")` (backpack.py:710). An absent
/// hash is an empty txid, as in Python; the read is logged so a typo cannot hide.
pub fn parse_submit_result(result: &Value, log: &mut ReadLog) -> String {
    read_optional(result, "hash", "submit.hash", log)
        .and_then(|h| h.as_str())
        .unwrap_or("")
        .to_string()
}

/// Why a ledger read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    Transport(TransportError),
    Shape(ShapeError),
    /// AccountTx still offered another page after [`MAX_ACCOUNT_TX_PAGES`]: the memo list would
    /// be short, so no list is returned.
    PageCapReached {
        account: String,
        pages: usize,
    },
}

/// Read on-ledger balances keyed by XRPL currency code (`wallet_info`, backpack.py:1398-1415):
/// only lines whose counterparty is the issuer count. A non-integer balance is left out (so the
/// proof reports it missing), where Python logs a warning.
pub fn fetch_balances<T: LedgerTransport + ?Sized>(
    transport: &mut T,
    wallet_address: &str,
    issuer_address: &str,
    log: &mut ReadLog,
) -> Result<BTreeMap<String, i32>, FetchError> {
    let result = transport
        .balances(wallet_address)
        .map_err(FetchError::Transport)?;
    let lines = parse_account_lines(&result, log).map_err(FetchError::Shape)?;
    let mut out = BTreeMap::new();
    for line in lines {
        if line.account == issuer_address {
            if let Ok(v) = balance_to_int(&line.balance) {
                out.insert(line.currency, v);
            }
        }
    }
    Ok(out)
}

/// Read settlement memos back off the chain, keyed by txid (`fetch_onchain_memos`,
/// backpack.py:1296-1365): both accounts, paged by marker up to [`MAX_ACCOUNT_TX_PAGES`] (hitting the cap
/// is [`FetchError::PageCapReached`], never a silently short list), first memo per transaction,
/// hex-decoded; an undecodable memo is skipped.
pub fn fetch_onchain_memos<T: LedgerTransport + ?Sized>(
    transport: &mut T,
    wallet_address: &str,
    issuer_address: &str,
    log: &mut ReadLog,
) -> Result<BTreeMap<String, String>, FetchError> {
    let mut memos: BTreeMap<String, String> = BTreeMap::new();
    for account in [wallet_address, issuer_address] {
        if account.is_empty() {
            continue;
        }
        let mut marker: Option<Value> = None;
        let mut finished = false;
        for _ in 0..MAX_ACCOUNT_TX_PAGES {
            let result = transport
                .tx_memos(account, marker.as_ref())
                .map_err(FetchError::Transport)?;
            let page = parse_account_tx_page(&result, log).map_err(FetchError::Shape)?;
            for entry in page.entries {
                if entry.hash.is_empty() || memos.contains_key(&entry.hash) {
                    continue;
                }
                let Some(data) = entry.memo_data else {
                    continue;
                };
                if let Ok(decoded) = hex_decode(&data) {
                    memos.insert(entry.hash, decoded);
                }
            }
            match page.marker {
                Some(m) => marker = Some(m),
                None => {
                    finished = true;
                    break;
                }
            }
        }
        // Python stops quietly at the cap and returns a short list. Rust does not: a list that may
        // be missing memos must not look complete (the proof reads this as INCONCLUSIVE).
        if !finished {
            return Err(FetchError::PageCapReached {
                account: account.to_string(),
                pages: MAX_ACCOUNT_TX_PAGES,
            });
        }
    }
    Ok(memos)
}

// ── Folding payment results into the backpack (double-settlement guards) ───────────────────

/// A settle stopped because the caller's persist hook failed. **A payment may have landed on the
/// ledger without being saved**: `key`'s payment was confirmed and folded into the in-memory
/// state, but the save of that state failed, so a save made from the last good snapshot does not
/// show it. The batch was stopped and the keys not yet paid were left pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistFailed<E> {
    /// The game key whose confirmed payment the failed save was meant to record.
    pub key: String,
    pub source: E,
}

impl<E: std::fmt::Display> std::fmt::Display for PersistFailed<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the payment for {} was confirmed but the state could not be saved ({}); \
             a payment may have landed without being saved",
            self.key, self.source
        )
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for PersistFailed<E> {}

struct BatchRun<E> {
    confirmed: BTreeMap<String, i32>,
    txids: Vec<String>,
    failure: Option<String>,
    persist_failed: Option<PersistFailed<E>>,
}

/// Run one batch a payment at a time. For EACH payment, in this exact order:
///
/// 1. submit it (`next`), which blocks until the transport reports the outcome;
/// 2. receive the confirmation (a refused, unreachable, wrong-key or duplicate-txid result stops
///    the batch here and folds nothing);
/// 3. fold it into `bp`: the baseline (`last_settled_supplies`) moves by that key's delta, and
///    the batch's receipt in `bp.settlements` gains that key and its txid (ledger-CRIT-2);
/// 4. `on_confirmed` (a pending retry removes the key from its queued record here);
/// 5. call `persist` with the state as it now stands, so every snapshot the caller saves is
///    self-consistent (a key is never advanced in the baseline without its receipt);
/// 6. only then submit the next payment.
///
/// If `persist` fails, the batch STOPS: no further payment is submitted, and the failure is
/// returned in `persist_failed` (a payment may have landed without being saved). Unpaid keys stay
/// pending (the callers queue them). Stopping, not tolerating and continuing (as the Python
/// `_persist_state` does), because a save that fails once usually fails again: every payment
/// made after it would widen the gap between the ledger and the save.
///
/// The window that remains: the process dies after step 2 of a payment and before step 5 has
/// saved. The save then lacks that payment, and the next settle pays it again. Python has the
/// same window. See `tests/backpack_save_per_payment.rs`.
fn run_batch<E>(
    bp: &mut BackpackState,
    batch: &SettlementBatch,
    timestamp: &str,
    next: &mut dyn FnMut(&SettlementBatch, usize) -> Option<PaymentAttempt>,
    on_confirmed: &mut dyn FnMut(&mut BackpackState, &str),
    persist: &mut dyn FnMut(&BackpackState) -> Result<(), E>,
    log: &mut ReadLog,
) -> BatchRun<E> {
    let mut run = BatchRun {
        confirmed: BTreeMap::new(),
        txids: Vec::new(),
        failure: None,
        persist_failed: None,
    };
    let mut receipt_idx: Option<usize> = None;
    for (index, payment) in batch.payments.iter().enumerate() {
        let Some(attempt) = next(batch, index) else {
            run.failure = Some(format!("no result for payment {}", payment.key));
            break;
        };
        // The core asks for one payment at a time, so a result for another key means the
        // transport is broken: stop, and never go on to a second payment.
        if attempt.key != payment.key {
            run.failure = Some(format!(
                "result for {} where {} was expected",
                attempt.key, payment.key
            ));
            break;
        }
        let value = match attempt.result {
            Ok(v) => v,
            Err(e) => {
                run.failure = Some(format!("{e:?}"));
                break;
            }
        };
        let txid = parse_submit_result(&value, log);
        if !txid.is_empty() {
            let seen =
                run.txids.contains(&txid) || bp.settlements.iter().any(|s| s.txids.contains(&txid));
            if seen {
                run.failure = Some(format!("duplicate txid {txid}: refusing to fold twice"));
                break;
            }
            run.txids.push(txid.clone());
        }
        let diff = signed_delta(payment);
        run.confirmed.insert(payment.key.clone(), diff);
        let base = bp
            .last_settled_supplies
            .entry(payment.key.clone())
            .or_insert(0);
        *base = base.saturating_add(diff);
        let idx = *receipt_idx.get_or_insert_with(|| {
            bp.settlements.push(SettlementRecord {
                day: batch.day,
                location: batch.location.clone(),
                deltas: BTreeMap::new(),
                txids: Vec::new(),
                status: "settled".to_string(),
                // ledger-003: the stored memo is the full-batch text every Payment carried.
                memo: batch.memo_text.clone(),
                timestamp: timestamp.to_string(),
            });
            bp.settlements.len() - 1
        });
        bp.settlements[idx].deltas.insert(payment.key.clone(), diff);
        if !txid.is_empty() {
            bp.settlements[idx].txids.push(txid);
        }
        on_confirmed(bp, &payment.key);
        if let Err(source) = persist(bp) {
            run.persist_failed = Some(PersistFailed {
                key: payment.key.clone(),
                source,
            });
            break;
        }
    }
    run
}

fn signed_delta(payment: &PaymentRequest) -> i32 {
    let mag = i32::try_from(payment.value).unwrap_or(i32::MAX);
    match payment.direction {
        PaymentDirection::PlayerToIssuer => -mag,
        PaymentDirection::IssuerToPlayer => mag,
    }
}

/// How a fresh settlement ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettleStatus {
    NotEnabled,
    NoChanges,
    /// Every payment confirmed.
    Settled,
    /// A payment failed; only the keys that did NOT clear were queued pending.
    Pending {
        reason: String,
    },
}

/// Summary of one [`settle_checkpoint`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettleReport {
    pub status: SettleStatus,
    pub confirmed: BTreeMap<String, i32>,
    pub txids: Vec<String>,
    /// Pending records that cleared (at least partly) during the retry pass that runs first.
    pub retried_moved: usize,
}

/// The state change after a fresh batch ran: `last_settlement_day` / `last_settle_failed` exactly
/// as Python, plus a pending record for the keys that did not clear.
fn finish_fresh<E>(
    bp: &mut BackpackState,
    batch: &SettlementBatch,
    run: BatchRun<E>,
    timestamp: &str,
) -> Result<SettleReport, PersistFailed<E>> {
    let BatchRun {
        confirmed,
        txids,
        failure,
        persist_failed,
    } = run;
    let remaining: BTreeMap<String, i32> = batch
        .deltas
        .iter()
        .filter(|(k, _)| !confirmed.contains_key(*k))
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    let stopped = failure.is_some() || (persist_failed.is_some() && !remaining.is_empty());
    if !stopped {
        bp.last_settlement_day = batch.day;
        bp.last_settle_failed = false;
    } else {
        bp.pending_settlements.push(SettlementRecord {
            day: batch.day,
            location: batch.location.clone(),
            memo: settlement_memo_text(&batch.run_id, batch.day, &remaining),
            deltas: remaining,
            txids: Vec::new(),
            status: "pending".to_string(),
            timestamp: timestamp.to_string(),
        });
        bp.last_settle_failed = true;
    }
    if let Some(p) = persist_failed {
        return Err(p);
    }
    let status = match failure {
        None => SettleStatus::Settled,
        Some(reason) => SettleStatus::Pending { reason },
    };
    Ok(SettleReport {
        status,
        confirmed,
        txids,
        retried_moved: 0,
    })
}

/// Fold the attempts of a fresh batch into `bp` (backpack.py:676-815) from attempts the caller
/// already holds: per-key baseline advance, a receipt in `bp.settlements` for what cleared, a
/// pending record for what did not, and `last_settlement_day` / `last_settle_failed` updated
/// exactly as Python does. No persistence point (use [`settle_checkpoint_with_persist`] for
/// that); a missing attempt is a failure.
pub fn fold_fresh_settlement(
    bp: &mut BackpackState,
    batch: &SettlementBatch,
    attempts: Vec<PaymentAttempt>,
    timestamp: &str,
    log: &mut ReadLog,
) -> SettleReport {
    let mut attempts = attempts.into_iter();
    let run = run_batch(
        bp,
        batch,
        timestamp,
        &mut |_, _| attempts.next(),
        &mut |_, _| {},
        &mut |_| Ok::<(), std::convert::Infallible>(()),
        log,
    );
    match finish_fresh(bp, batch, run, timestamp) {
        Ok(report) => report,
        Err(p) => match p.source {},
    }
}

/// Drop pending keys that are not on-chain resources, and any record left empty (F-86a4c19c,
/// backpack.py:857-870), so a stale `{gold: N}` record cannot wedge a retry.
pub fn strip_unknown_pending_keys(bp: &mut BackpackState) -> bool {
    let mut stripped = false;
    for rec in bp.pending_settlements.iter_mut() {
        let before = rec.deltas.len();
        rec.deltas.retain(|k, _| token_code(k).is_some());
        stripped |= rec.deltas.len() != before;
    }
    bp.pending_settlements.retain(|r| !r.deltas.is_empty());
    stripped
}

/// Retry every pending record (`_retry_pending`, backpack.py:819-1040) with no persistence
/// point. Returns how many records cleared at least one key. See
/// [`retry_pending_with_persist`].
pub fn retry_pending<T: LedgerTransport + ?Sized>(
    bp: &mut BackpackState,
    run_id: &str,
    timestamp: &str,
    transport: &mut T,
    log: &mut ReadLog,
) -> usize {
    match retry_pending_with_persist(
        bp,
        run_id,
        timestamp,
        transport,
        &mut |_| Ok::<(), std::convert::Infallible>(()),
        log,
    ) {
        Ok(moved) => moved,
        Err(p) => match p.source {},
    }
}

/// Retry every pending record, calling `persist` after each confirmed payment (see
/// [`run_batch`] for the exact per-payment order). Returns how many records cleared at least one
/// key, or [`PersistFailed`] when a save failed (the retry stops there).
///
/// - ledger-CRIT-2: each confirmed key is removed from its pending record the moment it clears,
///   so a later pass can never resubmit it, and a record that empties is dropped from the queue.
/// - ENG-A-08: each confirmed delta is folded into the baseline, so the next fresh settlement is
///   measured against a baseline that already accounts for it.
/// - ledger-003: the on-chain memo carries the record's full pre-narrowing delta set; the stored
///   memo of a record that is narrowed is refreshed to the narrowed set.
///
/// The record being retried stays in `bp.pending_settlements` throughout, narrowed in place, so
/// every snapshot handed to `persist` still holds every payment not yet made.
pub fn retry_pending_with_persist<T: LedgerTransport + ?Sized, E>(
    bp: &mut BackpackState,
    run_id: &str,
    timestamp: &str,
    transport: &mut T,
    persist: &mut dyn FnMut(&BackpackState) -> Result<(), E>,
    log: &mut ReadLog,
) -> Result<usize, PersistFailed<E>> {
    strip_unknown_pending_keys(bp);
    if bp.pending_settlements.is_empty() {
        // Departs from Python (`_retry_pending` returns at backpack.py:851 and leaves the flag when
        // the queue was already empty on entry; it clears it only when stripping emptied the
        // queue, :873). Rust clears it whenever nothing is pending: the flag drives the "last
        // settlement failed" notice, and with no unsettled checkpoint left that notice would be
        // false. A flag stuck on with an empty queue (reachable from a hand-edited or older save)
        // heals on the next settle instead of nagging the player forever.
        bp.last_settle_failed = false;
        return Ok(0);
    }
    let mut moved = 0usize;
    let mut i = 0usize;
    while i < bp.pending_settlements.len() {
        let record = bp.pending_settlements[i].clone();
        let batch = build_batch(bp, run_id, record.day, &record.location, &record.deltas);
        let mut removed = false;
        let run = run_batch(
            bp,
            &batch,
            timestamp,
            &mut |b, idx| Some(transport.submit_payment(b, idx)),
            &mut |bp, key| {
                let rec = &mut bp.pending_settlements[i];
                rec.deltas.remove(key);
                rec.memo = settlement_memo_text(run_id, rec.day, &rec.deltas);
                if rec.deltas.is_empty() {
                    bp.pending_settlements.remove(i);
                    removed = true;
                }
            },
            persist,
            log,
        );
        if !run.confirmed.is_empty() {
            moved += 1;
        }
        if let Some(p) = run.persist_failed {
            bp.last_settle_failed = !bp.pending_settlements.is_empty();
            return Err(p);
        }
        if !removed {
            i += 1;
        }
    }
    if !bp.pending_settlements.is_empty() {
        bp.last_settle_failed = true;
    } else if moved > 0 {
        bp.last_settle_failed = false;
    }
    Ok(moved)
}

/// Settle a checkpoint through a transport (`BackpackManager.settle`) with no persistence point:
/// the caller saves after it returns. Same as [`settle_checkpoint_with_persist`] with a hook that
/// does nothing.
#[allow(clippy::too_many_arguments)] // mirrors settle(state, location) plus the inputs Python read from globals
pub fn settle_checkpoint<T: LedgerTransport + ?Sized>(
    bp: &mut BackpackState,
    supplies: &SuppliesState,
    run_id: &str,
    day: i32,
    location: &str,
    timestamp: &str,
    transport: &mut T,
    log: &mut ReadLog,
) -> SettleReport {
    match settle_checkpoint_with_persist(
        bp,
        supplies,
        run_id,
        day,
        location,
        timestamp,
        transport,
        &mut |_| Ok::<(), std::convert::Infallible>(()),
        log,
    ) {
        Ok(report) => report,
        Err(p) => match p.source {},
    }
}

/// Settle a checkpoint, saving after each confirmed payment: retry pending records first, then
/// plan, submit and fold the fresh delta. The settlement logic stays pure: it never touches a
/// file; `persist` is the caller's save, called with the state as folded after each confirmed
/// payment (see [`run_batch`] for the exact order and for what a failing `persist` does). All
/// state change is in `bp`; the only outside calls are the transport and `persist`.
///
/// On `Err`, `bp` is ahead of what was last saved by the payment the error names; the caller
/// should keep its last good save, and the next settle from that save may pay that key again
/// (the known window, see `tests/backpack_save_per_payment.rs`).
#[allow(clippy::too_many_arguments)] // mirrors settle(state, location) plus the inputs Python read from globals
pub fn settle_checkpoint_with_persist<T: LedgerTransport + ?Sized, E>(
    bp: &mut BackpackState,
    supplies: &SuppliesState,
    run_id: &str,
    day: i32,
    location: &str,
    timestamp: &str,
    transport: &mut T,
    persist: &mut dyn FnMut(&BackpackState) -> Result<(), E>,
    log: &mut ReadLog,
) -> Result<SettleReport, PersistFailed<E>> {
    if !bp.enabled || bp.wallet_address.is_empty() {
        return Ok(SettleReport {
            status: SettleStatus::NotEnabled,
            confirmed: BTreeMap::new(),
            txids: Vec::new(),
            retried_moved: 0,
        });
    }
    let moved = retry_pending_with_persist(bp, run_id, timestamp, transport, persist, log)?;
    let mut report = match plan_settlement(bp, supplies, run_id, day, location) {
        PlanOutcome::NotEnabled => unreachable!("checked above"),
        PlanOutcome::NoChanges => SettleReport {
            status: SettleStatus::NoChanges,
            confirmed: BTreeMap::new(),
            txids: Vec::new(),
            retried_moved: 0,
        },
        PlanOutcome::Batch(batch) => {
            let run = run_batch(
                bp,
                &batch,
                timestamp,
                &mut |b, idx| Some(transport.submit_payment(b, idx)),
                &mut |_, _| {},
                persist,
                log,
            );
            finish_fresh(bp, &batch, run, timestamp)?
        }
    };
    report.retried_moved = moved;
    Ok(report)
}

// ── RecordedTransport ───────────────────────────────────────────────────────────────────────

/// What a recorded submit response is keyed on: the payment as it would be signed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SubmitExpect {
    account: String,
    destination: String,
    currency: String,
    issuer: String,
    value: String,
    memo_data: String,
}

#[derive(Debug, Clone)]
struct RecordedSubmit {
    expect: SubmitExpect,
    response: Result<Value, String>,
    used: bool,
}

/// Replays recorded XRPL responses. Not a fake that accepts any call: a call with no matching
/// recording returns [`TransportError::NoRecording`], and a recorded submit is matched on the
/// whole payment (accounts, currency, issuer, value and the hex memo).
///
/// Fixture JSON shape:
/// ```json
/// { "account_lines": { "<account>": <result> },
///   "account_tx":    [ { "account": "<a>", "marker": null | <marker>, "result": <result> } ],
///   "submit":        [ { "expect": { "account", "destination", "currency", "issuer", "value",
///                                    "memo_data" }, "result": <result> | "error": "<text>" } ] }
/// ```
#[derive(Debug, Clone, Default)]
pub struct RecordedTransport {
    lines: BTreeMap<String, Value>,
    tx_pages: BTreeMap<(String, String), Value>,
    submits: Vec<RecordedSubmit>,
    /// Every call made, in order, as `kind:detail`.
    pub calls: Vec<String>,
}

fn marker_key(marker: Option<&Value>) -> String {
    match marker {
        None => String::new(),
        Some(v) => v.to_string(),
    }
}

impl RecordedTransport {
    /// Build from a parsed fixture document.
    pub fn from_fixture(doc: &Value) -> Result<Self, String> {
        let mut t = RecordedTransport::default();
        if let Some(obj) = doc.get("account_lines").and_then(Value::as_object) {
            for (acct, result) in obj {
                t.lines.insert(acct.clone(), result.clone());
            }
        }
        if let Some(arr) = doc.get("account_tx").and_then(Value::as_array) {
            for item in arr {
                let account = item
                    .get("account")
                    .and_then(Value::as_str)
                    .ok_or("account_tx entry without account")?;
                let marker = item.get("marker").filter(|m| !m.is_null());
                let result = item
                    .get("result")
                    .ok_or("account_tx entry without result")?;
                t.tx_pages
                    .insert((account.to_string(), marker_key(marker)), result.clone());
            }
        }
        if let Some(arr) = doc.get("submit").and_then(Value::as_array) {
            for item in arr {
                let e = item.get("expect").ok_or("submit entry without expect")?;
                let s = |k: &str| -> Result<String, String> {
                    Ok(e.get(k)
                        .and_then(Value::as_str)
                        .ok_or(format!("submit expect without {k}"))?
                        .to_string())
                };
                let expect = SubmitExpect {
                    account: s("account")?,
                    destination: s("destination")?,
                    currency: s("currency")?,
                    issuer: s("issuer")?,
                    value: s("value")?,
                    memo_data: s("memo_data")?,
                };
                let response = match (item.get("result"), item.get("error")) {
                    (Some(r), None) => Ok(r.clone()),
                    (None, Some(err)) => Err(err.as_str().unwrap_or("error").to_string()),
                    _ => return Err("submit entry needs exactly one of result/error".to_string()),
                };
                t.submits.push(RecordedSubmit {
                    expect,
                    response,
                    used: false,
                });
            }
        }
        Ok(t)
    }

    /// Recorded submit responses never matched by a call.
    pub fn unconsumed_submits(&self) -> usize {
        self.submits.iter().filter(|s| !s.used).count()
    }
}

impl LedgerTransport for RecordedTransport {
    fn balances(&mut self, account: &str) -> Result<Value, TransportError> {
        self.calls.push(format!("account_lines:{account}"));
        self.lines
            .get(account)
            .cloned()
            .ok_or_else(|| TransportError::NoRecording(format!("account_lines:{account}")))
    }

    fn submit_payment(&mut self, batch: &SettlementBatch, index: usize) -> PaymentAttempt {
        let p = &batch.payments[index];
        self.calls
            .push(format!("submit:{}:{}", p.currency, p.value));
        let want = SubmitExpect {
            account: p.account.clone(),
            destination: p.destination.clone(),
            currency: p.currency.clone(),
            issuer: p.issuer.clone(),
            value: p.value.to_string(),
            memo_data: p.memo.memo_data.clone(),
        };
        let hit = self
            .submits
            .iter_mut()
            .find(|s| !s.used && s.expect == want);
        let result = match hit {
            Some(s) => {
                s.used = true;
                s.response.clone().map_err(TransportError::Unreachable)
            }
            None => Err(TransportError::NoRecording(format!(
                "submit:{}:{}:{}",
                p.currency, p.value, p.memo.memo_data
            ))),
        };
        PaymentAttempt {
            key: p.key.clone(),
            result,
        }
    }

    fn tx_memos(&mut self, account: &str, marker: Option<&Value>) -> Result<Value, TransportError> {
        self.calls
            .push(format!("account_tx:{account}:{}", marker_key(marker)));
        self.tx_pages
            .get(&(account.to_string(), marker_key(marker)))
            .cloned()
            .ok_or_else(|| {
                TransportError::NoRecording(format!("account_tx:{account}:{}", marker_key(marker)))
            })
    }
}

/// Set of response-field paths read with `found == false`, split by required/optional.
pub fn missing_reads(log: &ReadLog) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut required = BTreeSet::new();
    let mut optional = BTreeSet::new();
    for r in &log.reads {
        if !r.found {
            if r.optional {
                optional.insert(r.path.clone());
            } else {
                required.insert(r.path.clone());
            }
        }
    }
    (required, optional)
}
