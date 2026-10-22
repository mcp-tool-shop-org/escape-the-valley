//! Spike S2: Ledger Backpack XRPL operations from Rust (`xrpl-rust = "=1.3.0"`), on Testnet.
//!
//! Exercises:
//! 1. Two faucet wallets (issuer + player) + validated balance check via `AccountInfo`
//! 2. `TrustSet` from player to issuer for `"FOD"` (3-char code used by `backpack_models.py`)
//! 3. IOU `Payment` from issuer to player with a hex-encoded `TRAIL|RUN:...|V:v1` memo
//! 4. `submit_and_wait` validated `tesSUCCESS` (within 60s timeout)
//! 5. Read back via `account_tx` with paging (`limit = 2`, exercising `marker`) + `MemoData` match,
//!    and via `account_lines` comparing the balance as an exact decimal string (no `f64`)
//! 6. XRP `Payment` of 12 drops with a `PARCEL|...` hex memo
//! 7. Forced lapse: submit with an already-passed `LastLedgerSequence`, verify lapse failure and
//!    resolve by looking up the transaction hash via `Tx`.

use std::borrow::Cow;
use std::time::{Duration, Instant};
use url::Url;
use xrpl::asynch::clients::{AsyncJsonRpcClient, XRPLAsyncClient};
use xrpl::asynch::ledger::get_latest_validated_ledger_sequence;
use xrpl::asynch::transaction::{autofill, sign, submit_and_wait};
use xrpl::asynch::wallet::generate_faucet_wallet;
use xrpl::models::requests::account_info::AccountInfo;
use xrpl::models::requests::account_lines::AccountLines;
use xrpl::models::requests::account_tx::AccountTx;
use xrpl::models::requests::tx::Tx as TxRequest;
use xrpl::models::requests::{LedgerIndex, Marker};
use xrpl::models::results::account_info::AccountInfoVersionMap;
use xrpl::models::results::account_lines::AccountLines as AccountLinesResult;
use xrpl::models::results::account_tx::AccountTxVersionMap;
use xrpl::models::results::tx::TxVersionMap;
use xrpl::models::transactions::payment::Payment;
use xrpl::models::transactions::trust_set::TrustSet;
use xrpl::models::transactions::{Memo, Transaction};
use xrpl::models::{Amount, IssuedCurrencyAmount, XRPAmount};

const TESTNET_URL: &str = "https://s.altnet.rippletest.net:51234/";
const ALLOWED_TESTNET_HOSTS: &[&str] = &["s.altnet.rippletest.net", "s.devnet.rippletest.net"];
const REQ_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);

fn xrpl_err<E: std::fmt::Debug>(e: E) -> String {
    format!("{e:?}")
}

pub fn assert_testnet_endpoint(raw_url: &str) -> Result<Url, String> {
    let url = Url::parse(raw_url).map_err(|e| format!("invalid URL: {e}"))?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if !ALLOWED_TESTNET_HOSTS.contains(&host.as_str()) {
        return Err(format!(
            "Ledger Backpack refuses non-testnet host {host:?}; allowed: {ALLOWED_TESTNET_HOSTS:?}"
        ));
    }
    Ok(url)
}

pub fn hex_encode_upper(text: &str) -> String {
    hex::encode_upper(text.as_bytes())
}

pub fn validate_exact_integer_balance(balance_str: &str) -> Result<i64, String> {
    if let Some((int_part, frac_part)) = balance_str.split_once('.') {
        if frac_part.chars().any(|c| c != '0') {
            return Err(format!("non-integer IOU balance: {balance_str:?}"));
        }
        int_part
            .parse::<i64>()
            .map_err(|e| format!("invalid balance integer {balance_str:?}: {e}"))
    } else {
        balance_str
            .parse::<i64>()
            .map_err(|e| format!("invalid balance integer {balance_str:?}: {e}"))
    }
}

fn tx_hash_and_result(tx_map: &TxVersionMap<'_>) -> (String, String, bool) {
    let (hash, validated) = match tx_map {
        TxVersionMap::Default(tx) => (tx.base.hash.to_string(), tx.base.validated.unwrap_or(false)),
        TxVersionMap::V1(tx) => (tx.base.hash.to_string(), tx.base.validated.unwrap_or(false)),
    };
    let code = tx_map
        .get_transaction_metadata()
        .map(|m| m.transaction_result.to_string())
        .unwrap_or_default();
    (hash, code, validated)
}

async fn read_validated_xrp_drops(
    client: &AsyncJsonRpcClient,
    address: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let req = AccountInfo::new(
        None,
        Cow::Borrowed(address),
        None,
        Some(LedgerIndex::Str("validated".into())),
        Some(true),
        None,
        None,
    );
    let resp = tokio::time::timeout(REQ_TIMEOUT, client.request(req.into()))
        .await?
        .map_err(xrpl_err)?;
    let info: AccountInfoVersionMap<'_> = resp.try_into().map_err(xrpl_err)?;
    let root = info.get_account_root();
    let drops_str = root.balance.as_ref().map(|b| b.0.as_ref()).unwrap_or("0");
    Ok(drops_str.parse::<u64>()?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    let url = assert_testnet_endpoint(TESTNET_URL)?;
    println!("[S2] Endpoint verified: {}", url);
    let client = AsyncJsonRpcClient::connect(url);

    // ── Step 1: Two faucet wallets + validated balance check ────────────────
    let t1 = Instant::now();
    let issuer = tokio::time::timeout(
        WRITE_TIMEOUT,
        generate_faucet_wallet(&client, None, None, None, None),
    )
    .await?
    .map_err(xrpl_err)?;
    let player = tokio::time::timeout(
        WRITE_TIMEOUT,
        generate_faucet_wallet(&client, None, None, None, None),
    )
    .await?
    .map_err(xrpl_err)?;

    let issuer_addr = issuer.classic_address.clone();
    let player_addr = player.classic_address.clone();

    let issuer_drops = read_validated_xrp_drops(&client, &issuer_addr).await?;
    let player_drops = read_validated_xrp_drops(&client, &player_addr).await?;
    assert!(issuer_drops > 0, "Issuer validated XRP balance must be > 0");
    assert!(player_drops > 0, "Player validated XRP balance must be > 0");
    println!(
        "[S2][1] PASS ({:.2}s): Faucet wallets funded & validated on-ledger | issuer={} ({} drops) | player={} ({} drops)",
        t1.elapsed().as_secs_f64(),
        issuer_addr,
        issuer_drops,
        player_addr,
        player_drops,
    );

    // ── Step 2: TrustSet from player to issuer for "FOD" ───────────────────
    let t2 = Instant::now();
    let mut trust_tx = TrustSet::new(
        Cow::Owned(player_addr.clone()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        IssuedCurrencyAmount::new(
            "FOD".into(),
            Cow::Owned(issuer_addr.clone()),
            "999999".into(),
        ),
        None,
        None,
    );
    let trust_res = tokio::time::timeout(
        WRITE_TIMEOUT,
        submit_and_wait(
            &mut trust_tx,
            &client,
            Some(&player),
            Some(true),
            Some(true),
        ),
    )
    .await?
    .map_err(xrpl_err)?;
    let (trust_txid, trust_code, trust_val) = tx_hash_and_result(&trust_res);
    assert_eq!(trust_code, "tesSUCCESS");
    assert!(trust_val, "TrustSet must be validated");
    println!(
        "[S2][2] PASS ({:.2}s): TrustSet FOD validated {} | txid={}",
        t2.elapsed().as_secs_f64(),
        trust_code,
        trust_txid,
    );

    // ── Step 3 & 4: IOU Payment from issuer to player with hex settlement memo ──
    let t3 = Instant::now();
    let settlement_memo_plain = "TRAIL|RUN:73475cb4-a1b2c3d4|DAY:1|DELTA:FOD+50|V:v1";
    let settlement_memo_hex = hex_encode_upper(settlement_memo_plain);
    let settlement_memo = Memo {
        memo_data: Some(settlement_memo_hex.clone()),
        memo_format: None,
        memo_type: Some(hex_encode_upper("text/plain")),
    };
    let mut iou_payment = Payment::new(
        Cow::Owned(issuer_addr.clone()),
        None,
        None,
        None,
        None,
        Some(vec![settlement_memo]),
        None,
        None,
        None,
        None,
        Amount::IssuedCurrencyAmount(IssuedCurrencyAmount::new(
            "FOD".into(),
            Cow::Owned(issuer_addr.clone()),
            "50".into(),
        )),
        Cow::Owned(player_addr.clone()),
        None,
        None,
        None,
        None,
        None,
    );
    let iou_res = tokio::time::timeout(
        WRITE_TIMEOUT,
        submit_and_wait(
            &mut iou_payment,
            &client,
            Some(&issuer),
            Some(true),
            Some(true),
        ),
    )
    .await?
    .map_err(xrpl_err)?;
    let (iou_txid, iou_code, iou_val) = tx_hash_and_result(&iou_res);
    assert_eq!(iou_code, "tesSUCCESS");
    assert!(iou_val, "IOU Payment must be validated");
    println!(
        "[S2][3&4] PASS ({:.2}s): IOU Payment 50 FOD with memo validated {} | txid={}",
        t3.elapsed().as_secs_f64(),
        iou_code,
        iou_txid,
    );

    // ── Step 6: XRP Payment of 12 drops with parcel memo (run before Step 5 so player has 4 txs for limit=2 marker paging) ──
    let t6 = Instant::now();
    let parcel_memo_plain = "PARCEL|RUN:73475cb4-a1b2c3d4|DAY:1|food:5";
    let parcel_memo_hex = hex_encode_upper(parcel_memo_plain);
    let parcel_memo = Memo {
        memo_data: Some(parcel_memo_hex.clone()),
        memo_format: None,
        memo_type: Some(hex_encode_upper("text/plain")),
    };
    let mut xrp_payment = Payment::new(
        Cow::Owned(player_addr.clone()),
        None,
        None,
        None,
        None,
        Some(vec![parcel_memo]),
        None,
        None,
        None,
        None,
        Amount::XRPAmount(XRPAmount::from("12")),
        Cow::Owned(issuer_addr.clone()),
        None,
        None,
        None,
        None,
        None,
    );
    let xrp_res = tokio::time::timeout(
        WRITE_TIMEOUT,
        submit_and_wait(
            &mut xrp_payment,
            &client,
            Some(&player),
            Some(true),
            Some(true),
        ),
    )
    .await?
    .map_err(xrpl_err)?;
    let (xrp_txid, xrp_code, xrp_val) = tx_hash_and_result(&xrp_res);
    assert_eq!(xrp_code, "tesSUCCESS");
    assert!(xrp_val, "XRP 12-drop Payment must be validated");
    println!(
        "[S2][6] PASS ({:.2}s): XRP 12-drop Parcel Payment validated {} | txid={}",
        t6.elapsed().as_secs_f64(),
        xrp_code,
        xrp_txid,
    );

    // ── Step 5a: Read back via account_tx with paging (limit = 2, marker exercised) ──
    let t5 = Instant::now();
    let mut marker: Option<Marker<'_>> = None;
    let mut pages_fetched = 0usize;
    let mut marker_exercised = false;
    let mut found_settlement_memo: Option<String> = None;
    let mut found_parcel_memo: Option<String> = None;
    let mut faucet_funder_addr: Option<String> = None;

    loop {
        let req = AccountTx::new(
            None,
            Cow::Owned(player_addr.clone()),
            None,
            None,
            Some(false),
            None,
            None,
            None,
            Some(2),
            marker.clone(),
        );
        let resp = tokio::time::timeout(REQ_TIMEOUT, client.request(req.into()))
            .await?
            .map_err(xrpl_err)?;
        let page: AccountTxVersionMap<'_> = resp.try_into().map_err(xrpl_err)?;
        pages_fetched += 1;

        let next_marker = match &page {
            AccountTxVersionMap::Default(res) => {
                for entry in &res.base.transactions {
                    let hash = entry.hash.as_ref();
                    let validated = entry.base.validated.unwrap_or(false);
                    if validated {
                        if let Some(tx_json) = &entry.tx_json {
                            if let Some(acct) = tx_json.get("Account").and_then(|a| a.as_str()) {
                                if acct != player_addr && acct != issuer_addr {
                                    faucet_funder_addr = Some(acct.to_owned());
                                }
                            }
                            let memo_data = tx_json
                                .get("Memos")
                                .and_then(|m| m.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|item| item.get("Memo"))
                                .and_then(|memo| memo.get("MemoData"))
                                .and_then(|d| d.as_str())
                                .map(str::to_owned);
                            if hash == iou_txid {
                                found_settlement_memo = memo_data;
                            } else if hash == xrp_txid {
                                found_parcel_memo = memo_data;
                            }
                        }
                    }
                }
                res.base.marker.clone()
            }
            AccountTxVersionMap::V1(res) => {
                for entry in &res.base.transactions {
                    let validated = entry.base.validated.unwrap_or(false);
                    if validated {
                        if let Some(tx_obj) = &entry.tx {
                            if let Some(acct) = tx_obj.get("Account").and_then(|a| a.as_str()) {
                                if acct != player_addr && acct != issuer_addr {
                                    faucet_funder_addr = Some(acct.to_owned());
                                }
                            }
                            let hash = tx_obj.get("hash").and_then(|h| h.as_str()).unwrap_or("");
                            let memo_data = tx_obj
                                .get("Memos")
                                .and_then(|m| m.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|item| item.get("Memo"))
                                .and_then(|memo| memo.get("MemoData"))
                                .and_then(|d| d.as_str())
                                .map(str::to_owned);
                            if hash == iou_txid {
                                found_settlement_memo = memo_data;
                            } else if hash == xrp_txid {
                                found_parcel_memo = memo_data;
                            }
                        }
                    }
                }
                res.base.marker.clone()
            }
        };

        if next_marker.is_some() {
            marker_exercised = true;
        }
        if next_marker.is_none() || pages_fetched >= 10 {
            break;
        }
        marker = next_marker;
    }

    // Note: rippled clamps `account_tx` limit < 10 up to 10 (`RPC::Tuning::account_tx`),
    // so a fresh 4-tx wallet fits in 1 page. Exercise multi-page `Marker::Sequence`
    // continuation against the faucet funder account (>10 txs) if player fit on 1 page.
    if !marker_exercised {
        let paging_acct = faucet_funder_addr
            .clone()
            .expect("Expected faucet funder account in player account_tx");
        let mut page_marker: Option<Marker<'_>> = None;
        let mut page1_first_hash = String::new();
        let mut page2_first_hash = String::new();
        for page_idx in 0..2 {
            let req = AccountTx::new(
                None,
                Cow::Owned(paging_acct.clone()),
                None,
                None,
                Some(false),
                None,
                None,
                None,
                Some(2),
                page_marker.clone(),
            );
            let resp = tokio::time::timeout(REQ_TIMEOUT, client.request(req.into()))
                .await?
                .map_err(xrpl_err)?;
            let page: AccountTxVersionMap<'_> = resp.try_into().map_err(xrpl_err)?;
            pages_fetched += 1;
            let (first_hash, next_m) = match &page {
                AccountTxVersionMap::Default(res) => (
                    res.base
                        .transactions
                        .first()
                        .map(|e| e.hash.to_string())
                        .unwrap_or_default(),
                    res.base.marker.clone(),
                ),
                AccountTxVersionMap::V1(res) => (
                    res.base
                        .transactions
                        .first()
                        .and_then(|e| e.tx.as_ref())
                        .and_then(|t| t.get("hash"))
                        .and_then(|h| h.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    res.base.marker.clone(),
                ),
            };
            if page_idx == 0 {
                assert!(next_m.is_some(), "Expected marker on page 1 of account_tx");
                marker_exercised = true;
                page1_first_hash = first_hash;
                page_marker = next_m;
            } else {
                page2_first_hash = first_hash;
            }
        }
        assert_ne!(
            page1_first_hash, page2_first_hash,
            "Page 2 with marker must return different transactions than page 1"
        );
    }

    assert!(
        marker_exercised && pages_fetched >= 2,
        "Expected marker paging across >= 2 pages"
    );
    assert_eq!(
        found_settlement_memo.as_deref(),
        Some(settlement_memo_hex.as_str()),
        "Settlement MemoData mismatch"
    );
    assert_eq!(
        found_parcel_memo.as_deref(),
        Some(parcel_memo_hex.as_str()),
        "Parcel MemoData mismatch"
    );

    // ── Step 5b: Read back via account_lines (exact decimal string, no f64) ──
    let lines_req = AccountLines::new(
        None,
        Cow::Owned(player_addr.clone()),
        None,
        Some(LedgerIndex::Str("validated".into())),
        None,
        Some(Cow::Owned(issuer_addr.clone())),
    );
    let lines_resp = tokio::time::timeout(REQ_TIMEOUT, client.request(lines_req.into()))
        .await?
        .map_err(xrpl_err)?;
    let lines_res: AccountLinesResult<'_> = lines_resp.try_into().map_err(xrpl_err)?;
    let fod_line = lines_res
        .lines
        .iter()
        .find(|l| l.currency == "FOD" && l.account == issuer_addr.as_str())
        .expect("FOD trust line must exist");
    assert_eq!(fod_line.balance.as_ref(), "50");
    let exact_int = validate_exact_integer_balance(fod_line.balance.as_ref())?;
    assert_eq!(exact_int, 50);
    println!(
        "[S2][5] PASS ({:.2}s): account_tx paged {} pages (marker exercised={}) & MemoData matched; account_lines FOD balance={:?} (exact int={})",
        t5.elapsed().as_secs_f64(),
        pages_fetched,
        marker_exercised,
        fod_line.balance,
        exact_int,
    );

    // ── Step 7: Forced lapse (LastLedgerSequence already passed + hash lookup) ──
    let t7 = Instant::now();
    let mut lapse_tx = Payment::new(
        Cow::Owned(player_addr.clone()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Amount::XRPAmount(XRPAmount::from("12")),
        Cow::Owned(issuer_addr.clone()),
        None,
        None,
        None,
        None,
        None,
    );
    tokio::time::timeout(REQ_TIMEOUT, autofill(&mut lapse_tx, &client, None))
        .await?
        .map_err(xrpl_err)?;
    let validated_seq =
        tokio::time::timeout(REQ_TIMEOUT, get_latest_validated_ledger_sequence(&client))
            .await?
            .map_err(xrpl_err)?;
    let passed_lls = validated_seq.saturating_sub(1);
    lapse_tx.common_fields.last_ledger_sequence = Some(passed_lls);
    sign(&mut lapse_tx, &player, false).map_err(xrpl_err)?;
    let lapse_hash = lapse_tx.get_hash().map_err(xrpl_err)?.to_string();

    let lapse_wait = tokio::time::timeout(
        WRITE_TIMEOUT,
        submit_and_wait(
            &mut lapse_tx,
            &client,
            Some(&player),
            Some(false),
            Some(false),
        ),
    )
    .await?;
    assert!(
        lapse_wait.is_err(),
        "Expected submit_and_wait to fail on lapsed LastLedgerSequence"
    );
    let lapse_err_msg = format!("{:?}", lapse_wait.unwrap_err());

    let tx_lookup_req = TxRequest::new(
        None,
        Some(false),
        None,
        None,
        Some(Cow::Borrowed(lapse_hash.as_str())),
    );
    let tx_lookup_resp = tokio::time::timeout(REQ_TIMEOUT, client.request(tx_lookup_req.into()))
        .await?
        .map_err(xrpl_err)?;
    let lookup_resolution = if !tx_lookup_resp.is_success() {
        let err_code = tx_lookup_resp
            .raw_result
            .as_ref()
            .and_then(|r| r.get("error"))
            .and_then(|e| e.as_str())
            .or(tx_lookup_resp.error.as_deref())
            .unwrap_or("unknown");
        assert_eq!(err_code, "txnNotFound");
        format!("error={err_code} (permanently lapsed, never entered ledger)")
    } else {
        let tx_map: TxVersionMap<'_> = tx_lookup_resp.try_into().map_err(xrpl_err)?;
        let validated = match &tx_map {
            TxVersionMap::Default(tx) => tx.base.validated.unwrap_or(false),
            TxVersionMap::V1(tx) => tx.base.validated.unwrap_or(false),
        };
        assert!(
            !validated && validated_seq > passed_lls,
            "Lapsed transaction must never be validated (validated={validated}, validated_seq={validated_seq}, lls={passed_lls})"
        );
        format!("validated=false at validated_ledger={validated_seq} > LastLedgerSequence={passed_lls} (permanently lapsed)")
    };
    println!(
        "[S2][7] PASS ({:.2}s): Forced lapse (validated_ledger={}, set LastLedgerSequence={}) -> submit_and_wait returned Err({}) & Tx lookup hash={} resolved to {}",
        t7.elapsed().as_secs_f64(),
        validated_seq,
        passed_lls,
        lapse_err_msg,
        lapse_hash,
        lookup_resolution,
    );

    println!(
        "[S2] ALL 7 STEPS PASSED in {:.2}s",
        total_start.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testnet_guard_rejects_mainnet() {
        assert!(assert_testnet_endpoint("https://s.altnet.rippletest.net:51234/").is_ok());
        assert!(assert_testnet_endpoint("https://xrplcluster.com/").is_err());
        assert!(assert_testnet_endpoint("https://s1.ripple.com:51234/").is_err());
    }

    #[test]
    fn exact_decimal_balance_parser_rejects_fractional() {
        assert_eq!(validate_exact_integer_balance("50").unwrap(), 50);
        assert_eq!(validate_exact_integer_balance("50.0").unwrap(), 50);
        assert_eq!(validate_exact_integer_balance("-14").unwrap(), -14);
        assert!(validate_exact_integer_balance("50.25").is_err());
    }
}
