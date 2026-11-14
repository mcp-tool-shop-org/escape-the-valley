//! Save schema v5: the save holds the pending event or fork (the "pending block").
//!
//! Why: the engine autosaves after every step, but the v4 save has no room for the event or
//! fork that is on screen, so quitting mid-event and resuming drops the player at camp with
//! the event gone. Under [`crate::ruleset::Ruleset::V2`] the engine now writes a v5 save that
//! carries the pending block, and [`crate::engine::StepEngine::resume`] puts it back.
//!
//! Where the block lives: beside `RunState`, not on it. [`SavedGame`] is `{ state, pending }`.
//! `RunState`, `compute_mechanics_state_hash`, every Tier 1 vector and every existing save
//! function are therefore untouched; the old v4 functions in [`crate::save`] keep their
//! behaviour exactly (they are what Legacy writes and loads).
//!
//! Refusals (the file on disk is never modified by a refusal):
//! - Python game saves (`save_version` 0..=3, or none): [`SaveLoadError::PythonSaveRefused`].
//!   No migration: the two engines draw events differently, so a migrated run would not
//!   continue as the same run.
//! - Earlier Rust development saves (`save_version` 4): [`SaveLoadError::DevelopmentSaveRefused`].
//! - Newer than 5: [`SaveLoadError::SaveFormatTooNew`].

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::content::ContentPack;
use crate::engine::{EventChoiceInfo, RouteOption};
use crate::model::{GamePhase, RunState};
use crate::save::{
    atomic_write_text, backup_corrupt_save, parse_state_fields, read_secrets_sidecar,
    state_to_value, write_secrets_sidecar, SaveLoadError, SAVE_DIR, SAVE_FILE,
};

/// Schema version written by the v5 writer.
pub const SAVE_VERSION_V5: i64 = 5;

/// Key of the pending block inside the save value.
pub const PENDING_KEY: &str = "pending";

/// Player-facing refusal for a save from the Python game (format 0..=3).
pub fn python_save_refusal_message(save_version: i64) -> String {
    format!(
        "This save is from Escape the Valley 1.x (save format {save_version}). Version 2 cannot continue it, because the two versions play events differently and a continued run would not be the same run. The 1.3 game still plays it: install it with `pip install \"escape-the-valley<2\"`, then run `trail play` (or `trail tui --continue` for the full-screen game). Your save file has not been changed."
    )
}

/// Player-facing refusal for a save from an earlier Rust development build (format 4).
pub fn development_save_refusal_message(save_version: i64) -> String {
    format!(
        "This save (save format {save_version}) was made by an earlier development build of Escape the Valley 2 and cannot be continued. Please start a new run. Your save file has not been changed."
    )
}

/// Player-facing refusal for a save newer than this build reads.
pub fn save_too_new_message(save_version: i64) -> String {
    format!(
        "This save (save format {save_version}) was made by a newer version of Escape the Valley than this one, which reads save formats up to {SAVE_VERSION_V5}. Please update the game to continue it. Your save file has not been changed."
    )
}

/// What was on screen when the game was saved: an event with its offered choices, or a fork
/// with its offered routes.
///
/// `phase` is `Camp`, `Event` or `Route`. For `Event`, `event_id`, `choices`, `title` and
/// `narration` are the event exactly as shown. For `Route`, `routes` are the routes offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBlock {
    pub phase: GamePhase,
    pub event_id: String,
    pub choices: Vec<EventChoiceInfo>,
    pub title: String,
    pub narration: String,
    pub routes: Vec<RouteOption>,
}

impl PendingBlock {
    fn empty(phase: GamePhase) -> Self {
        Self {
            phase,
            event_id: String::new(),
            choices: Vec::new(),
            title: String::new(),
            narration: String::new(),
            routes: Vec::new(),
        }
    }

    /// A block for an event on screen.
    pub fn event(
        event_id: &str,
        choices: Vec<EventChoiceInfo>,
        title: &str,
        narration: &str,
    ) -> Self {
        Self {
            event_id: event_id.to_string(),
            choices,
            title: title.to_string(),
            narration: narration.to_string(),
            ..Self::empty(GamePhase::Event)
        }
    }

    /// A block for a fork on screen.
    pub fn route(routes: Vec<RouteOption>) -> Self {
        Self {
            routes,
            ..Self::empty(GamePhase::Route)
        }
    }

    fn to_value(&self) -> Option<Value> {
        match self.phase {
            GamePhase::Event => Some(json!({
                "phase": "event",
                "event_id": self.event_id,
                "choices": self.choices,
                "title": self.title,
                "narration": self.narration,
            })),
            GamePhase::Route => Some(json!({
                "phase": "route",
                "routes": self.routes,
            })),
            // Camp and game over need no block.
            GamePhase::Camp | GamePhase::GameOver => None,
        }
    }
}

/// The v5 key that holds [`RunState::turns_stepped`].
const TURNS_STEPPED_KEY: &str = "turns_stepped";

/// A loaded v5 save: the run state and, when the player quit mid-event or mid-fork, the block.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedGame {
    pub state: RunState,
    pub pending: Option<PendingBlock>,
}

/// Convert state plus an optional pending block into a v5 save value. A `Camp` or absent
/// block writes no `pending` key.
pub fn state_to_value_v5(state: &RunState, pending: Option<&PendingBlock>) -> Value {
    let mut value = state_to_value(state);
    let obj = value
        .as_object_mut()
        .expect("state_to_value returns an object");
    obj.insert("save_version".to_string(), json!(SAVE_VERSION_V5));
    obj.insert(TURNS_STEPPED_KEY.to_string(), json!(state.turns_stepped));
    if let Some(block) = pending.and_then(PendingBlock::to_value) {
        obj.insert(PENDING_KEY.to_string(), block);
    }
    value
}

/// Save to `<base_path>/.trail/run.json` as v5, atomically, and update the secrets sidecar.
pub fn save_game_v5(
    state: &RunState,
    pending: Option<&PendingBlock>,
    base_path: &Path,
) -> std::io::Result<PathBuf> {
    let save_dir = base_path.join(SAVE_DIR);
    fs::create_dir_all(&save_dir)?;
    let save_path = save_dir.join(SAVE_FILE);
    let data = state_to_value_v5(state, pending);
    let payload = serde_json::to_string_pretty(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    atomic_write_text(&save_path, &payload, false)?;
    write_secrets_sidecar(&save_dir, state)?;
    Ok(save_path)
}

fn corrupt(msg: impl Into<String>) -> SaveLoadError {
    SaveLoadError::Corrupt(msg.into())
}

fn parse_pending(
    value: &Value,
    content: &ContentPack,
) -> Result<Option<PendingBlock>, SaveLoadError> {
    let obj = value
        .as_object()
        .ok_or_else(|| corrupt("pending block is not an object"))?;
    let phase = match obj.get("phase").and_then(Value::as_str) {
        Some("camp") => GamePhase::Camp,
        Some("event") => GamePhase::Event,
        Some("route") => GamePhase::Route,
        Some(other) => {
            return Err(corrupt(format!(
                "pending block has unknown phase '{other}'"
            )))
        }
        None => return Err(corrupt("pending block has no phase")),
    };
    let has_event_fields = ["event_id", "choices", "title", "narration"]
        .iter()
        .any(|k| obj.contains_key(*k));
    let has_routes = obj.contains_key("routes");
    match phase {
        GamePhase::Event => {
            if has_routes {
                return Err(corrupt("pending block with phase 'event' carries routes"));
            }
            let event_id = match obj.get("event_id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => return Err(corrupt("pending block with phase 'event' has no event_id")),
            };
            if !content.all_events.iter().any(|e| e.event_id == event_id) {
                return Err(corrupt(format!(
                    "pending event '{event_id}' is not in the event library"
                )));
            }
            let choices: Vec<EventChoiceInfo> = serde_json::from_value(
                obj.get("choices")
                    .cloned()
                    .ok_or_else(|| corrupt("pending event has no choices"))?,
            )
            .map_err(|e| corrupt(format!("pending event choices are invalid: {e}")))?;
            if choices.is_empty() {
                return Err(corrupt("pending event has no choices"));
            }
            let title = obj
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| corrupt("pending event has no title"))?;
            let narration = obj
                .get("narration")
                .and_then(Value::as_str)
                .ok_or_else(|| corrupt("pending event has no narration"))?;
            Ok(Some(PendingBlock::event(
                &event_id, choices, title, narration,
            )))
        }
        GamePhase::Route => {
            if has_event_fields {
                return Err(corrupt(
                    "pending block with phase 'route' carries event fields",
                ));
            }
            let routes: Vec<RouteOption> = serde_json::from_value(
                obj.get("routes")
                    .cloned()
                    .ok_or_else(|| corrupt("pending block with phase 'route' has no routes"))?,
            )
            .map_err(|e| corrupt(format!("pending routes are invalid: {e}")))?;
            if routes.is_empty() {
                return Err(corrupt("pending block with phase 'route' has no routes"));
            }
            Ok(Some(PendingBlock::route(routes)))
        }
        // Camp: nothing pending. (A block naming the phase 'over' never reaches here: it is
        // rejected above as an unknown phase.)
        _ => {
            if has_event_fields || has_routes {
                return Err(corrupt(
                    "pending block with phase 'camp' carries event or route fields",
                ));
            }
            Ok(None)
        }
    }
}

/// Reconstruct a [`SavedGame`] from a parsed save value, against `content`'s hash and event
/// library. Refuses Python saves, v4 development saves and saves newer than 5 (see the module
/// docs). A pending event whose id is not in the library is [`SaveLoadError::Corrupt`]
/// naming the id; nothing here panics.
pub fn value_to_saved_game(
    data: &Value,
    content: &ContentPack,
) -> Result<SavedGame, SaveLoadError> {
    let obj = data
        .as_object()
        .ok_or_else(|| corrupt("top-level save data is not an object"))?;
    let save_version = match obj.get("save_version") {
        None => 0,
        Some(v) => v
            .as_i64()
            .ok_or_else(|| corrupt("save_version is not an integer"))?,
    };
    match save_version {
        0..=3 => return Err(SaveLoadError::PythonSaveRefused { save_version }),
        4 => return Err(SaveLoadError::DevelopmentSaveRefused { save_version }),
        SAVE_VERSION_V5 => {}
        _ => return Err(SaveLoadError::SaveFormatTooNew { save_version }),
    }

    let running = content.content_hash_hex.as_str();
    let save_content_hash = match obj.get("content_hash") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(Value::String(_)) | None => {
            return Err(SaveLoadError::ContentVersionMismatch {
                save_content_hash: "<missing>".to_string(),
                running_content_hash: running.to_string(),
            })
        }
        Some(_) => return Err(corrupt("content_hash is not a string")),
    };
    if save_content_hash != running {
        return Err(SaveLoadError::ContentVersionMismatch {
            save_content_hash,
            running_content_hash: running.to_string(),
        });
    }

    let mut state = parse_state_fields(obj, save_content_hash).map_err(|e| {
        if e.starts_with("seed_out_of_range:") {
            SaveLoadError::SeedOutOfRange(e)
        } else {
            SaveLoadError::Corrupt(e)
        }
    })?;
    // A v5 save written before the counter existed reads it as 0.
    state.turns_stepped = match obj.get(TURNS_STEPPED_KEY) {
        None => 0,
        Some(v) => v
            .as_u64()
            .ok_or_else(|| corrupt("turns_stepped is not a non-negative integer"))?,
    };
    let pending = match obj.get(PENDING_KEY) {
        None | Some(Value::Null) => None,
        Some(v) => parse_pending(v, content)?,
    };
    Ok(SavedGame { state, pending })
}

/// Load `<base_path>/.trail/run.json` as a v5 save. Refusals leave the file untouched. A
/// corrupt file is quarantined to `run.json.corrupt-<timestamp>`, as the v4 loader does.
pub fn load_saved_game(
    base_path: &Path,
    content: &ContentPack,
) -> Result<SavedGame, SaveLoadError> {
    let save_dir = base_path.join(SAVE_DIR);
    let save_path = save_dir.join(SAVE_FILE);
    if !save_path.exists() {
        return Err(SaveLoadError::NoSave);
    }
    let raw = match fs::read_to_string(&save_path) {
        Ok(t) => t,
        Err(e) => {
            let _ = backup_corrupt_save(&save_path);
            return Err(SaveLoadError::Corrupt(e.to_string()));
        }
    };
    let data: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            let _ = backup_corrupt_save(&save_path);
            return Err(SaveLoadError::Corrupt(e.to_string()));
        }
    };
    let mut saved = match value_to_saved_game(&data, content) {
        Ok(s) => s,
        Err(err @ SaveLoadError::Corrupt(_)) => {
            let _ = backup_corrupt_save(&save_path);
            return Err(err);
        }
        Err(err) => return Err(err),
    };
    if let Some(secrets) = read_secrets_sidecar(&save_dir, &saved.state.run_id) {
        saved.state.backpack.wallet_secret = secrets.wallet_secret;
        saved.state.backpack.issuer_secret = secrets.issuer_secret;
    }
    Ok(saved)
}
