//! Save and load system (`save.py`):
//! - Schema v3 (`SAVE_VERSION = 3`) and v0/v1/v2 → v3 migration;
//! - Corrupt-save quarantine (`run.json.corrupt-<timestamp>`);
//! - Supply clamps on load (`SuppliesState::set`, floor 0, catalog filter);
//! - Atomic write of `.trail/run.json` and `.trail/secrets.json` (keyed by `run_id`,
//!   0o600 on POSIX and owner-only ACL on Windows);
//! - Audit L-2 fix: `last_settle_failed` and `minted_initial` persisted directly.

use crate::model::{
    is_valid_resource_key, BackpackState, Biome, Condition, GMProfile, JournalEntry, MapNode,
    MemoryCard, Pace, ParcelRecord, PartyMember, PartyState, PermitRecord, RunState,
    SentParcelRecord, SettlementRecord, SuppliesState, TimeOfDay, Trait, TwistModifier, WagonState,
};
use crate::rng::coerce_rng_state;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SAVE_DIR: &str = ".trail";
pub const SAVE_FILE: &str = "run.json";
pub const SECRETS_FILE: &str = "secrets.json";

/// Python 1.3.1 writes `save_version = 1`. Design v1 refers to the full 1.3.x schema
/// (with `rng_state`, `backpack`, `memory_cards`, `last_spoilage_day`) as schema v3.
/// We accept versions 0, 1, 2, and 3 on load (migrating older schemas to v3 in memory)
/// and report versions > 3 as `"incompatible"`.
pub const PYTHON_SAVE_VERSION: i64 = 1;
pub const MAX_SUPPORTED_SAVE_VERSION: i64 = 3;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq)]
pub struct LoadResult {
    pub ok: bool,
    pub state: Option<RunState>,
    pub reason: String,
}

impl LoadResult {
    pub fn success(state: RunState) -> Self {
        Self {
            ok: true,
            state: Some(state),
            reason: String::new(),
        }
    }

    pub fn failure(reason: &str) -> Self {
        Self {
            ok: false,
            state: None,
            reason: reason.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RunSecrets {
    pub wallet_secret: String,
    pub issuer_secret: String,
}

/// Check if `.trail/run.json` exists under `base_path`.
pub fn has_save(base_path: &Path) -> bool {
    base_path.join(SAVE_DIR).join(SAVE_FILE).exists()
}

/// Save the current `RunState` to `<base_path>/.trail/run.json` atomically,
/// and update `<base_path>/.trail/secrets.json` atomically.
pub fn save_game(state: &RunState, base_path: &Path) -> std::io::Result<PathBuf> {
    let save_dir = base_path.join(SAVE_DIR);
    fs::create_dir_all(&save_dir)?;
    let save_path = save_dir.join(SAVE_FILE);

    let data = state_to_value(state);
    let payload = serde_json::to_string_pretty(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    atomic_write_text(&save_path, &payload, false)?;
    write_secrets_sidecar(&save_dir, state)?;

    Ok(save_path)
}

/// Load a saved game from `<base_path>/.trail/run.json`, returning `Option<RunState>`.
pub fn load_game(base_path: &Path) -> Option<RunState> {
    load_game_result(base_path).state
}

/// Load a saved game, returning a structured `LoadResult` (`save.py::load_game_result`).
/// On a corrupt save, renames `run.json` to `run.json.corrupt-<timestamp>` before returning.
pub fn load_game_result(base_path: &Path) -> LoadResult {
    let save_dir = base_path.join(SAVE_DIR);
    let save_path = save_dir.join(SAVE_FILE);

    if !save_path.exists() {
        return LoadResult::failure("no_save");
    }

    let raw_text = match fs::read_to_string(&save_path) {
        Ok(t) => t,
        Err(_) => {
            let _ = backup_corrupt_save(&save_path);
            return LoadResult::failure("corrupt");
        }
    };

    let data: Value = match serde_json::from_str(&raw_text) {
        Ok(v) => v,
        Err(_) => {
            let _ = backup_corrupt_save(&save_path);
            return LoadResult::failure("corrupt");
        }
    };

    let Some(obj) = data.as_object() else {
        let _ = backup_corrupt_save(&save_path);
        return LoadResult::failure("corrupt");
    };

    if let Some(v) = obj.get("save_version").and_then(|v| v.as_i64()) {
        if v > MAX_SUPPORTED_SAVE_VERSION {
            return LoadResult::failure("incompatible");
        }
    }

    let mut state = match value_to_state(&data) {
        Ok(s) => s,
        Err(ref e) if e.starts_with("seed_out_of_range:") => {
            return LoadResult::failure("seed_out_of_range");
        }
        Err(_) => {
            let _ = backup_corrupt_save(&save_path);
            return LoadResult::failure("corrupt");
        }
    };

    if let Some(secrets) = read_secrets_sidecar(&save_dir, &state.run_id) {
        state.backpack.wallet_secret = secrets.wallet_secret;
        state.backpack.issuer_secret = secrets.issuer_secret;
    }

    LoadResult::success(state)
}

/// Write `text` to `target` atomically via a temp file in the same directory, `sync_all()`,
/// optional restrictive permissions (`0o600` on POSIX / owner-only ACL on Windows), and `fs::rename`.
pub fn atomic_write_text(
    target: &Path,
    text: &str,
    restrict_permissions: bool,
) -> std::io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("save.json");
    let pid = std::process::id();
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".{file_name}.{pid}.{nanos}.{seq}.tmp"));

    let write_res = (|| -> std::io::Result<()> {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if restrict_permissions {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp_path)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);

        if restrict_permissions {
            apply_owner_only_permissions(&tmp_path)?;
        }

        fs::rename(&tmp_path, target)?;
        Ok(())
    })();

    if write_res.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    write_res
}

fn apply_owner_only_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        // Apply an owner-only DACL via icacls: grant current user full control and remove inheritance.
        if let Ok(username) = std::env::var("USERNAME") {
            if !username.is_empty() {
                let grant_arg = format!("{username}:(F)");
                let _ = std::process::Command::new("icacls")
                    .arg(path)
                    .arg("/inheritance:r")
                    .arg("/grant:r")
                    .arg(&grant_arg)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

/// Persist wallet/issuer seeds to `.trail/secrets.json`, keyed by `run_id`, atomically.
/// Preserves other runs' entries (`save.py::_write_secrets_sidecar`).
pub fn write_secrets_sidecar(save_dir: &Path, state: &RunState) -> std::io::Result<()> {
    let secrets_path = save_dir.join(SECRETS_FILE);
    let mut store: BTreeMap<String, RunSecrets> = if secrets_path.exists() {
        fs::read_to_string(&secrets_path)
            .ok()
            .and_then(|txt| serde_json::from_str::<Value>(&txt).ok())
            .and_then(|val| val.as_object().cloned())
            .map(|obj| {
                let mut map = BTreeMap::new();
                for (k, v) in obj {
                    if let Some(entry_obj) = v.as_object() {
                        let ws = entry_obj
                            .get("wallet_secret")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();
                        let is = entry_obj
                            .get("issuer_secret")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string();
                        map.insert(
                            k,
                            RunSecrets {
                                wallet_secret: ws,
                                issuer_secret: is,
                            },
                        );
                    }
                }
                map
            })
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };

    let bp = &state.backpack;
    if !bp.wallet_secret.is_empty() || !bp.issuer_secret.is_empty() {
        store.insert(
            state.run_id.clone(),
            RunSecrets {
                wallet_secret: bp.wallet_secret.clone(),
                issuer_secret: bp.issuer_secret.clone(),
            },
        );
    } else {
        store.remove(&state.run_id);
    }

    let payload = serde_json::to_string_pretty(&store)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    atomic_write_text(&secrets_path, &payload, true)
}

/// Read wallet/issuer seeds for `run_id` from `.trail/secrets.json`.
pub fn read_secrets_sidecar(save_dir: &Path, run_id: &str) -> Option<RunSecrets> {
    let secrets_path = save_dir.join(SECRETS_FILE);
    if !secrets_path.exists() {
        return None;
    }
    let raw = fs::read_to_string(&secrets_path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    let obj = val.as_object()?;
    let entry = obj.get(run_id)?.as_object()?;
    Some(RunSecrets {
        wallet_secret: entry
            .get("wallet_secret")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        issuer_secret: entry
            .get("issuer_secret")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// Quarantine a corrupt `run.json` to `run.json.corrupt-<timestamp>[-<counter>]` (`save.py::_backup_corrupt_save`).
pub fn backup_corrupt_save(save_path: &Path) -> Option<PathBuf> {
    if !save_path.exists() {
        return None;
    }
    let ts = format_utc_timestamp();
    let file_name = save_path.file_name()?.to_str()?;
    let parent = save_path.parent()?;

    let mut backup = parent.join(format!("{file_name}.corrupt-{ts}"));
    if backup.exists() {
        let mut counter = 1u64;
        loop {
            backup = parent.join(format!("{file_name}.corrupt-{ts}-{counter}"));
            if !backup.exists() {
                break;
            }
            counter += 1;
        }
    }

    if fs::rename(save_path, &backup).is_ok() {
        Some(backup)
    } else {
        None
    }
}

fn format_utc_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let micros = now.subsec_micros();

    // Civil date from Unix timestamp (Howard Hinnant's algorithm)
    let z = (secs / 86400) as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    let rem = secs % 86400;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;

    format!("{year:04}{m:02}{d:02}T{hour:02}{minute:02}{second:02}{micros:06}")
}

/// Convert `RunState` into a `serde_json::Value` matching `save.py::_state_to_dict`.
///
/// Audit L-2: `backpack.last_settle_failed` and `backpack.minted_initial` are persisted
/// directly on `backpack` when non-default (and always supported on load), while keeping
/// full compatibility with Python 1.3.1 saves.
pub fn state_to_value(state: &RunState) -> Value {
    let bp = &state.backpack;
    let mut bp_map = serde_json::Map::new();
    bp_map.insert("enabled".to_string(), json!(bp.enabled));
    bp_map.insert("wallet_address".to_string(), json!(bp.wallet_address));
    bp_map.insert("issuer_address".to_string(), json!(bp.issuer_address));
    bp_map.insert("trust_lines_ready".to_string(), json!(bp.trust_lines_ready));
    bp_map.insert(
        "last_settled_supplies".to_string(),
        json!(bp.last_settled_supplies),
    );
    bp_map.insert(
        "last_settlement_day".to_string(),
        json!(bp.last_settlement_day),
    );
    if !bp.minted_initial.is_empty() {
        bp_map.insert("minted_initial".to_string(), json!(bp.minted_initial));
    }
    bp_map.insert("settlements".to_string(), json!(bp.settlements));
    bp_map.insert(
        "pending_settlements".to_string(),
        json!(bp.pending_settlements),
    );
    bp_map.insert("parcels".to_string(), json!(bp.parcels));
    bp_map.insert("sent_parcels".to_string(), json!(bp.sent_parcels));
    bp_map.insert("permits".to_string(), json!(bp.permits));
    bp_map.insert("nudge_shown".to_string(), json!(bp.nudge_shown));
    bp_map.insert("nudge_dismissed".to_string(), json!(bp.nudge_dismissed));
    if bp.last_settle_failed {
        bp_map.insert(
            "last_settle_failed".to_string(),
            json!(bp.last_settle_failed),
        );
    }

    let seed_val = if let Ok(s64) = i64::try_from(state.seed) {
        json!(s64)
    } else {
        json!(state.seed.to_string())
    };

    json!({
        "save_version": PYTHON_SAVE_VERSION,
        "run_id": state.run_id,
        "seed": seed_val,
        "day": state.day,
        "time_of_day": state.time_of_day,
        "location_id": state.location_id,
        "destination_id": state.destination_id,
        "distance_remaining": state.distance_remaining,
        "total_distance": state.total_distance,
        "distance_traveled": state.distance_traveled,
        "mode": state.mode,
        "weirdness_level": state.weirdness_level,
        "uncanny_tokens": state.uncanny_tokens,
        "gm_profile": state.gm_profile,
        "twists": state.twists,
        "game_over": state.game_over,
        "victory": state.victory,
        "cause_of_death": state.cause_of_death,
        "rng_counter": state.rng_counter,
        "rng_state": state.rng_state,
        "recent_event_tags": state.recent_event_tags,
        "party": state.party,
        "wagon": state.wagon,
        "supplies": state.supplies.to_dict(),
        "journal": state.journal,
        "map_nodes": state.map_nodes,
        "memory_cards": state.memory_cards,
        "resource_crises_seen": state.resource_crises_seen,
        "doctrine": state.doctrine,
        "taboo": state.taboo,
        "rationing_steps": state.rationing_steps,
        "escape_valve_cooldown": state.escape_valve_cooldown,
        "last_action": state.last_action,
        "maintained_turns_remaining": state.maintained_turns_remaining,
        "last_spoilage_day": state.last_spoilage_day,
        "backpack": Value::Object(bp_map),
        "callout_level": state.callout_level,
    })
}

fn req_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str, String> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing or non-string required key '{key}'"))
}

fn opt_str(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    default: &str,
) -> Result<String, String> {
    match obj.get(key) {
        None => Ok(default.to_string()),
        Some(v) => v
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| format!("non-string key '{key}'")),
    }
}

fn req_i32(obj: &serde_json::Map<String, Value>, key: &str) -> Result<i32, String> {
    let v = obj
        .get(key)
        .ok_or_else(|| format!("missing required key '{key}'"))?;
    val_to_i32(v).ok_or_else(|| format!("non-integer required key '{key}'"))
}

fn opt_i32(obj: &serde_json::Map<String, Value>, key: &str, default: i32) -> Result<i32, String> {
    match obj.get(key) {
        None => Ok(default),
        Some(v) => val_to_i32(v).ok_or_else(|| format!("non-integer key '{key}'")),
    }
}

fn opt_bool(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    default: bool,
) -> Result<bool, String> {
    match obj.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("non-boolean key '{key}'")),
    }
}

fn val_to_i32(v: &Value) -> Option<i32> {
    if v.is_boolean() {
        return None;
    }
    if let Some(i) = v.as_i64() {
        i32::try_from(i).ok()
    } else {
        None
    }
}

fn parse_seed(v: &Value) -> Result<i128, String> {
    if v.is_boolean() {
        return Err("seed cannot be boolean".to_string());
    }
    if let Some(i) = v.as_i64() {
        return Ok(i as i128);
    }
    if let Some(u) = v.as_u64() {
        return Ok(u as i128);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<i128>().map_err(|e| match e.kind() {
            std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
                format!("seed_out_of_range: {e}")
            }
            _ => format!("invalid seed string: {e}"),
        });
    }
    if v.is_number() {
        let raw = v.to_string();
        return raw.parse::<i128>().map_err(|e| match e.kind() {
            std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
                format!("seed_out_of_range: {e}")
            }
            _ => "invalid seed value".to_string(),
        });
    }
    Err("invalid seed value".to_string())
}

/// Reconstruct `RunState` from a parsed JSON `Value` (`save.py::_dict_to_state`),
/// migrating v0/v1/v2 saves to schema v3 and clamping supplies through `SuppliesState::set`.
pub fn value_to_state(data: &Value) -> Result<RunState, String> {
    let obj = data
        .as_object()
        .ok_or_else(|| "top-level save data is not an object".to_string())?;

    let save_version = match obj.get("save_version") {
        None => 0,
        Some(v) => v
            .as_i64()
            .ok_or_else(|| "save_version is not an integer".to_string())?,
    };
    if save_version > MAX_SUPPORTED_SAVE_VERSION {
        return Err(format!(
            "save_version {save_version} newer than supported {MAX_SUPPORTED_SAVE_VERSION}"
        ));
    }

    let party_obj = obj
        .get("party")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "missing or invalid 'party'".to_string())?;
    let members_arr = party_obj
        .get("members")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "missing or invalid 'party.members'".to_string())?;

    let mut members = Vec::with_capacity(members_arr.len());
    for m_val in members_arr {
        let m_obj = m_val
            .as_object()
            .ok_or_else(|| "party member is not an object".to_string())?;
        let name = req_str(m_obj, "name")?.to_string();
        let health = req_i32(m_obj, "health")?;
        let cond_val = m_obj
            .get("condition")
            .ok_or_else(|| "missing member condition".to_string())?;
        let condition: Condition = serde_json::from_value(cond_val.clone())
            .map_err(|e| format!("invalid condition: {e}"))?;
        let traits_arr = m_obj
            .get("traits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "missing member traits".to_string())?;
        let mut traits = Vec::with_capacity(traits_arr.len());
        for t_val in traits_arr {
            let t: Trait =
                serde_json::from_value(t_val.clone()).map_err(|e| format!("invalid trait: {e}"))?;
            traits.push(t);
        }
        let death_cause = opt_str(m_obj, "death_cause", "")?;
        members.push(PartyMember {
            name,
            health,
            condition,
            traits,
            death_cause,
        });
    }
    let morale = opt_i32(party_obj, "morale", 70)?;

    let wagon_obj = obj
        .get("wagon")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "missing or invalid 'wagon'".to_string())?;
    let wagon_condition = req_i32(wagon_obj, "condition")?;
    let animals_health = opt_i32(wagon_obj, "animals_health", 90)?;
    let capacity = opt_i32(wagon_obj, "capacity", 200)?;
    let pace: Pace = match wagon_obj.get("pace") {
        None => Pace::Steady,
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("invalid pace: {e}"))?,
    };

    let supplies_val = obj
        .get("supplies")
        .ok_or_else(|| "missing 'supplies'".to_string())?;
    let supplies = load_supplies(supplies_val);

    let run_id = req_str(obj, "run_id")?.to_string();
    let seed_val = obj
        .get("seed")
        .ok_or_else(|| "missing 'seed'".to_string())?;
    let seed = parse_seed(seed_val)?;
    let day = req_i32(obj, "day")?;
    let time_of_day: TimeOfDay = match obj.get("time_of_day") {
        None => TimeOfDay::Morning,
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|e| format!("invalid time_of_day: {e}"))?
        }
    };
    let location_id = req_str(obj, "location_id")?.to_string();
    let destination_id = opt_str(obj, "destination_id", "")?;
    let distance_remaining = req_i32(obj, "distance_remaining")?;
    let total_distance = opt_i32(obj, "total_distance", 0)?;
    let distance_traveled = opt_i32(obj, "distance_traveled", 0)?;
    let mode = opt_str(obj, "mode", "offline")?;
    let weirdness_level = opt_i32(obj, "weirdness_level", 2)?;
    let uncanny_tokens = opt_i32(obj, "uncanny_tokens", 2)?;
    let gm_profile: GMProfile = match obj.get("gm_profile") {
        None => GMProfile::Fireside,
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|e| format!("invalid gm_profile: {e}"))?
        }
    };

    let mut twists = Vec::new();
    if let Some(tw_val) = obj.get("twists") {
        let tw_arr = tw_val
            .as_array()
            .ok_or_else(|| "twists is not an array".to_string())?;
        for t in tw_arr {
            let tm: TwistModifier =
                serde_json::from_value(t.clone()).map_err(|e| format!("invalid twist: {e}"))?;
            twists.push(tm);
        }
    }

    let game_over = opt_bool(obj, "game_over", false)?;
    let victory = opt_bool(obj, "victory", false)?;
    let cause_of_death = opt_str(obj, "cause_of_death", "")?;
    let rng_counter = match obj.get("rng_counter") {
        None => 0u64,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| "invalid rng_counter".to_string())?,
    };
    let rng_state = obj.get("rng_state").and_then(coerce_rng_state);

    let recent_event_tags: Vec<String> = match obj.get("recent_event_tags") {
        None => Vec::new(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| format!("invalid recent_event_tags: {e}"))?,
    };

    let mut journal = Vec::new();
    if let Some(j_val) = obj.get("journal") {
        let j_arr = j_val
            .as_array()
            .ok_or_else(|| "journal is not an array".to_string())?;
        for e_val in j_arr {
            let e_obj = e_val
                .as_object()
                .ok_or_else(|| "journal entry is not an object".to_string())?;
            let j_day = req_i32(e_obj, "day")?;
            let j_loc = req_str(e_obj, "location")?.to_string();
            let j_ev = req_str(e_obj, "event_id")?.to_string();
            let scene_title = opt_str(e_obj, "scene_title", "")?;
            let narration = opt_str(e_obj, "narration", "")?;
            let choice_made = opt_str(e_obj, "choice_made", "")?;
            let outcome = opt_str(e_obj, "outcome", "")?;
            let deltas: BTreeMap<String, i32> = match e_obj.get("deltas") {
                None => BTreeMap::new(),
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("invalid journal deltas: {e}"))?,
            };
            let tags: Vec<String> = match e_obj.get("tags") {
                None => Vec::new(),
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("invalid journal tags: {e}"))?,
            };
            journal.push(JournalEntry {
                day: j_day,
                location: j_loc,
                event_id: j_ev,
                scene_title,
                narration,
                choice_made,
                outcome,
                deltas,
                tags,
            });
        }
    }

    let mut map_nodes = Vec::new();
    if let Some(m_val) = obj.get("map_nodes") {
        let m_arr = m_val
            .as_array()
            .ok_or_else(|| "map_nodes is not an array".to_string())?;
        for n_val in m_arr {
            let n_obj = n_val
                .as_object()
                .ok_or_else(|| "map_node is not an object".to_string())?;
            let node_id = req_str(n_obj, "node_id")?.to_string();
            let name = req_str(n_obj, "name")?.to_string();
            let b_val = n_obj
                .get("biome")
                .ok_or_else(|| "missing node biome".to_string())?;
            let biome: Biome =
                serde_json::from_value(b_val.clone()).map_err(|e| format!("invalid biome: {e}"))?;
            let hazard = req_i32(n_obj, "hazard")?;
            let water_available = opt_bool(n_obj, "water_available", true)?;
            let temperature = opt_i32(n_obj, "temperature", 15)?;
            let is_town = opt_bool(n_obj, "is_town", false)?;
            let connections: Vec<String> = match n_obj.get("connections") {
                None => Vec::new(),
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("invalid node connections: {e}"))?,
            };
            let distance_to: BTreeMap<String, i32> = match n_obj.get("distance_to") {
                None => BTreeMap::new(),
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("invalid node distance_to: {e}"))?,
            };
            let cache_supplies: Option<BTreeMap<String, i32>> = match n_obj.get("cache_supplies") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    serde_json::from_value(v.clone())
                        .map_err(|e| format!("invalid cache_supplies: {e}"))?,
                ),
            };
            map_nodes.push(MapNode {
                node_id,
                name,
                biome,
                hazard,
                water_available,
                temperature,
                is_town,
                connections,
                distance_to,
                cache_supplies,
            });
        }
    }

    let mut memory_cards = Vec::new();
    if let Some(mc_val) = obj.get("memory_cards") {
        let mc_arr = mc_val
            .as_array()
            .ok_or_else(|| "memory_cards is not an array".to_string())?;
        for c_val in mc_arr {
            let c_obj = c_val
                .as_object()
                .ok_or_else(|| "memory_card is not an object".to_string())?;
            let id = req_str(c_obj, "id")?.to_string();
            let kind = req_str(c_obj, "kind")?.to_string();
            let title = req_str(c_obj, "title")?.to_string();
            let text = req_str(c_obj, "text")?.to_string();
            let tags: Vec<String> = match c_obj.get("tags") {
                None => Vec::new(),
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("invalid card tags: {e}"))?,
            };
            let day_created = opt_i32(c_obj, "day_created", 1)?;
            let day_last_seen = opt_i32(c_obj, "day_last_seen", 1)?;
            let entities: Vec<String> = match c_obj.get("entities") {
                None => Vec::new(),
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("invalid card entities: {e}"))?,
            };
            let salience = match c_obj.get("salience") {
                None => 0.5,
                Some(v) => v
                    .as_f64()
                    .ok_or_else(|| "invalid card salience".to_string())?,
            };
            let cooldown_until = opt_i32(c_obj, "cooldown_until", 0)?;
            let source = opt_str(c_obj, "source", "engine")?;
            memory_cards.push(MemoryCard {
                id,
                kind,
                title,
                text,
                tags,
                day_created,
                day_last_seen,
                entities,
                salience,
                cooldown_until,
                source,
            });
        }
    }

    let resource_crises_seen: Vec<String> = match obj.get("resource_crises_seen") {
        None => Vec::new(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| format!("invalid resource_crises_seen: {e}"))?,
    };

    let doctrine = opt_str(obj, "doctrine", "")?;
    let taboo = opt_str(obj, "taboo", "")?;
    let rationing_steps = opt_i32(obj, "rationing_steps", 0)?;
    let escape_valve_cooldown = opt_i32(obj, "escape_valve_cooldown", 0)?;
    let last_action = opt_str(obj, "last_action", "")?;
    let maintained_turns_remaining = opt_i32(obj, "maintained_turns_remaining", 0)?;
    let last_spoilage_day = opt_i32(obj, "last_spoilage_day", 0)?;
    let backpack = match obj.get("backpack") {
        None => BackpackState::default(),
        Some(v) => load_backpack(v)?,
    };
    let callout_level = opt_str(obj, "callout_level", "verbose")?;

    Ok(RunState {
        run_id,
        seed,
        day,
        time_of_day,
        location_id,
        destination_id,
        distance_remaining,
        total_distance,
        distance_traveled,
        mode,
        weirdness_level,
        uncanny_tokens,
        gm_profile,
        twists,
        game_over,
        victory,
        cause_of_death,
        ending: None,
        party: PartyState { members, morale },
        wagon: WagonState {
            condition: wagon_condition,
            animals_health,
            capacity,
            pace,
        },
        supplies,
        journal,
        map_nodes,
        rng_counter,
        rng_state,
        recent_event_tags,
        memory_cards,
        resource_crises_seen,
        doctrine,
        taboo,
        rationing_steps,
        escape_valve_cooldown,
        last_action,
        maintained_turns_remaining,
        last_spoilage_day,
        backpack,
        callout_level,
    })
}

/// Overlay saved supply values onto `DEFAULT_SUPPLIES` through `SuppliesState::set` (floor 0)
/// and drop keys not in `RESOURCE_CATALOG` (`save.py::_load_supplies`).
pub fn load_supplies(data: &Value) -> SuppliesState {
    let mut supplies = SuppliesState::with_defaults();
    let Some(obj) = data.as_object() else {
        return supplies;
    };
    for (key, val) in obj {
        if !is_valid_resource_key(key) {
            continue;
        }
        let parsed = if let Some(i) = val_to_i32(val) {
            Some(i)
        } else if let Some(f) = val.as_f64() {
            Some(f as i32)
        } else if let Some(s) = val.as_str() {
            s.trim().parse::<i32>().ok()
        } else {
            None
        };
        if let Some(v) = parsed {
            supplies.set(key, v);
        }
    }
    supplies
}

fn load_backpack(data: &Value) -> Result<BackpackState, String> {
    if data.is_null() {
        return Ok(BackpackState::default());
    }
    let Some(obj) = data.as_object() else {
        return Err("backpack is not an object".to_string());
    };
    if obj.is_empty() {
        return Ok(BackpackState::default());
    }

    let enabled = opt_bool(obj, "enabled", false)?;
    let wallet_address = opt_str(obj, "wallet_address", "")?;
    let wallet_secret = opt_str(obj, "wallet_secret", "")?;
    let issuer_address = opt_str(obj, "issuer_address", "")?;
    let issuer_secret = opt_str(obj, "issuer_secret", "")?;
    let trust_lines_ready = opt_bool(obj, "trust_lines_ready", false)?;
    let last_settled_supplies: BTreeMap<String, i32> = match obj.get("last_settled_supplies") {
        None => BTreeMap::new(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| format!("invalid last_settled_supplies: {e}"))?,
    };
    let last_settlement_day = opt_i32(obj, "last_settlement_day", 0)?;
    let minted_initial: BTreeMap<String, i32> = match obj.get("minted_initial") {
        None => BTreeMap::new(),
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|e| format!("invalid minted_initial: {e}"))?
        }
    };

    let settlements = parse_settlement_list(obj.get("settlements"))?;
    let pending_settlements = parse_settlement_list(obj.get("pending_settlements"))?;

    let mut parcels = Vec::new();
    if let Some(p_val) = obj.get("parcels") {
        let p_arr = p_val
            .as_array()
            .ok_or_else(|| "parcels is not an array".to_string())?;
        for item in p_arr {
            let p_obj = item
                .as_object()
                .ok_or_else(|| "parcel is not an object".to_string())?;
            parcels.push(ParcelRecord {
                parcel_id: opt_str(p_obj, "parcel_id", "")?,
                sender: opt_str(p_obj, "sender", "")?,
                contents: match p_obj.get("contents") {
                    None => BTreeMap::new(),
                    Some(v) => serde_json::from_value(v.clone())
                        .map_err(|e| format!("invalid parcel contents: {e}"))?,
                },
                txid: opt_str(p_obj, "txid", "")?,
                accepted: opt_bool(p_obj, "accepted", false)?,
                day_received: opt_i32(p_obj, "day_received", 0)?,
            });
        }
    }

    let mut sent_parcels = Vec::new();
    if let Some(sp_val) = obj.get("sent_parcels") {
        let sp_arr = sp_val
            .as_array()
            .ok_or_else(|| "sent_parcels is not an array".to_string())?;
        for item in sp_arr {
            let sp_obj = item
                .as_object()
                .ok_or_else(|| "sent_parcel is not an object".to_string())?;
            sent_parcels.push(SentParcelRecord {
                recipient: opt_str(sp_obj, "recipient", "")?,
                supply: opt_str(sp_obj, "supply", "")?,
                amount: opt_i32(sp_obj, "amount", 0)?,
                txid: opt_str(sp_obj, "txid", "")?,
                day_sent: opt_i32(sp_obj, "day_sent", 0)?,
                memo: opt_str(sp_obj, "memo", "")?,
            });
        }
    }

    let mut permits = Vec::new();
    if let Some(pm_val) = obj.get("permits") {
        let pm_arr = pm_val
            .as_array()
            .ok_or_else(|| "permits is not an array".to_string())?;
        for item in pm_arr {
            let pm_obj = item
                .as_object()
                .ok_or_else(|| "permit is not an object".to_string())?;
            permits.push(PermitRecord {
                permit_id: opt_str(pm_obj, "permit_id", "")?,
                txid: opt_str(pm_obj, "txid", "")?,
                used: opt_bool(pm_obj, "used", false)?,
                day_earned: opt_i32(pm_obj, "day_earned", 0)?,
                day_used: opt_i32(pm_obj, "day_used", 0)?,
            });
        }
    }

    let nudge_shown = opt_bool(obj, "nudge_shown", false)?;
    let nudge_dismissed = opt_bool(obj, "nudge_dismissed", false)?;
    let last_settle_failed = opt_bool(obj, "last_settle_failed", false)?;

    Ok(BackpackState {
        enabled,
        wallet_address,
        wallet_secret,
        issuer_address,
        issuer_secret,
        trust_lines_ready,
        last_settled_supplies,
        last_settlement_day,
        minted_initial,
        settlements,
        pending_settlements,
        parcels,
        sent_parcels,
        permits,
        nudge_shown,
        nudge_dismissed,
        last_settle_failed,
    })
}

fn parse_settlement_list(val: Option<&Value>) -> Result<Vec<SettlementRecord>, String> {
    let Some(v) = val else {
        return Ok(Vec::new());
    };
    let arr = v
        .as_array()
        .ok_or_else(|| "settlements is not an array".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let s_obj = item
            .as_object()
            .ok_or_else(|| "settlement is not an object".to_string())?;
        out.push(SettlementRecord {
            day: opt_i32(s_obj, "day", 0)?,
            location: opt_str(s_obj, "location", "")?,
            deltas: match s_obj.get("deltas") {
                None => BTreeMap::new(),
                Some(d) => serde_json::from_value(d.clone())
                    .map_err(|e| format!("invalid settlement deltas: {e}"))?,
            },
            txids: match s_obj.get("txids") {
                None => Vec::new(),
                Some(t) => serde_json::from_value(t.clone())
                    .map_err(|e| format!("invalid settlement txids: {e}"))?,
            },
            status: opt_str(s_obj, "status", "pending")?,
            memo: opt_str(s_obj, "memo", "")?,
            timestamp: opt_str(s_obj, "timestamp", "")?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::create_new_run;

    // preservation: missing save returns LoadResult::failure("no_save") and None from load_game
    #[test]
    fn test_missing_save_returns_no_save() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(!has_save(tmp.path()));
        let res = load_game_result(tmp.path());
        assert!(!res.ok);
        assert_eq!(res.reason, "no_save");
        assert!(res.state.is_none());
        assert!(load_game(tmp.path()).is_none());
    }

    // preservation: structural corruption in value_to_state returns Err and load_game_result quarantines
    #[test]
    fn test_value_to_state_structural_corruption() {
        // Non-object top-level JSON
        assert!(value_to_state(&json!([1, 2, 3])).is_err());

        let base_state = create_new_run(42);
        let valid_val = state_to_value(&base_state);

        // Missing required keys
        for req_key in [
            "party",
            "wagon",
            "supplies",
            "run_id",
            "seed",
            "day",
            "location_id",
            "distance_remaining",
        ] {
            let mut v = valid_val.clone();
            v.as_object_mut().unwrap().remove(req_key);
            assert!(
                value_to_state(&v).is_err(),
                "removing required key '{req_key}' must fail value_to_state"
            );
        }

        // Wrong types on fields and nested objects
        let bad_mutations: Vec<(&str, Value)> = vec![
            ("save_version", json!("not_an_int")),
            ("save_version", json!(99)),
            ("party", json!("not_an_object")),
            ("party", json!({"members": "not_a_list"})),
            ("party", json!({"members": ["not_an_obj"]})),
            ("party", json!({"members": [{"name": "A", "health": 100, "condition": "bad_cond", "traits": []}]})),
            ("wagon", json!("not_an_object")),
            ("wagon", json!({"condition": "bad"})),
            ("seed", json!(true)),
            ("rng_counter", json!(-1)),
            ("twists", json!("not_a_list")),
            ("journal", json!([{"day": 1}])),
            ("map_nodes", json!([{"node_id": "n1", "name": "N", "biome": "invalid_biome", "hazard": 1}])),
            ("memory_cards", json!([{"id": "c1", "kind": "k", "title": "t", "text": "x", "salience": "bad"}])),
            ("backpack", json!("not_an_object")),
        ];

        for (key, bad_val) in bad_mutations {
            let mut v = valid_val.clone();
            v.as_object_mut().unwrap().insert(key.to_string(), bad_val);
            assert!(
                value_to_state(&v).is_err(),
                "corrupting '{key}' must fail value_to_state"
            );
        }

        // Verify load_game_result quarantines a structurally corrupt JSON object
        let tmp = tempfile::tempdir().expect("tempdir");
        let save_dir = tmp.path().join(SAVE_DIR);
        fs::create_dir_all(&save_dir).unwrap();
        let save_path = save_dir.join(SAVE_FILE);
        fs::write(&save_path, "{\"save_version\": 1}").unwrap();
        let res = load_game_result(tmp.path());
        assert!(!res.ok);
        assert_eq!(res.reason, "corrupt");
        assert!(!save_path.exists());
    }

    // Fails on b888cb8 for two reasons:
    // 1. Bare JSON integer seed > u64::MAX (within i128) failed serde_json::from_str::<Value> without arbitrary_precision.
    // 2. Seeds outside i128 (> i128::MAX or < i128::MIN) were quarantined as "corrupt" instead of returning "seed_out_of_range".
    #[test]
    fn test_seed_above_u64_max_and_seed_overflow_outside_i128() {
        let base_state = create_new_run(42);
        let mut base_val = state_to_value(&base_state);

        // 1. Bare integer seed in (u64::MAX as i128 + 1)..=i128::MAX loads cleanly
        let seed_above_u64: i128 = (u64::MAX as i128) + 999_999;
        base_val
            .as_object_mut()
            .unwrap()
            .insert("seed".to_string(), json!(0));
        let raw_valid_big = serde_json::to_string(&base_val)
            .unwrap()
            .replace("\"seed\":0", &format!("\"seed\":{seed_above_u64}"));
        let parsed_val: Value = serde_json::from_str(&raw_valid_big).unwrap();
        let loaded = value_to_state(&parsed_val).expect("seed > u64::MAX within i128 must parse");
        assert_eq!(loaded.seed, seed_above_u64);

        // 2. Bare integer seed > i128::MAX (2^127) is rejected as seed_out_of_range (not quarantined)
        let above_i128_max = "170141183460469231731687303715884105728"; // i128::MAX + 1
        let raw_overflow_pos = serde_json::to_string(&base_val)
            .unwrap()
            .replace("\"seed\":0", &format!("\"seed\":{above_i128_max}"));
        let tmp = tempfile::tempdir().expect("tempdir");
        let save_dir = tmp.path().join(SAVE_DIR);
        fs::create_dir_all(&save_dir).unwrap();
        let save_path = save_dir.join(SAVE_FILE);
        fs::write(&save_path, &raw_overflow_pos).unwrap();

        let res_pos = load_game_result(tmp.path());
        assert!(!res_pos.ok);
        assert_eq!(res_pos.reason, "seed_out_of_range");
        assert!(
            save_path.exists(),
            "save with seed > i128::MAX must not be quarantined as corrupt"
        );

        // 3. Bare integer seed < i128::MIN (-2^127 - 1) and string seed > i128::MAX also return seed_out_of_range
        let below_i128_min = "-170141183460469231731687303715884105729"; // i128::MIN - 1
        let raw_overflow_neg = serde_json::to_string(&base_val)
            .unwrap()
            .replace("\"seed\":0", &format!("\"seed\":{below_i128_min}"));
        let val_neg: Value = serde_json::from_str(&raw_overflow_neg).unwrap();
        let err_neg = value_to_state(&val_neg).unwrap_err();
        assert!(err_neg.starts_with("seed_out_of_range:"));

        let mut val_str_ov = base_val.clone();
        val_str_ov
            .as_object_mut()
            .unwrap()
            .insert("seed".to_string(), json!(above_i128_max));
        let err_str = value_to_state(&val_str_ov).unwrap_err();
        assert!(err_str.starts_with("seed_out_of_range:"));
    }
}
