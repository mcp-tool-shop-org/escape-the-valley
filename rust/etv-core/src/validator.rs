use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::content::ContentPack;
use crate::events::advertised_inventory_cost;
use crate::model::{is_valid_resource_key, Trait, INVENTORY_COST_KEYS};
use crate::worldgen::{EventSkeleton, LANDMARK_NAMES, TOWN_NAMES};

pub const ERR_MISSING_SCHEMA_VERSION: &str = "MISSING_SCHEMA_VERSION";
pub const ERR_DUPLICATE_ID: &str = "DUPLICATE_ID";
pub const ERR_EMPTY_GATE_HINT: &str = "EMPTY_GATE_HINT";
pub const ERR_NO_AFFORDABLE_FALLBACK_CHOICE: &str = "NO_AFFORDABLE_FALLBACK_CHOICE";
pub const ERR_UNKNOWN_REFERENCE_ID: &str = "UNKNOWN_REFERENCE_ID";
pub const ERR_MISSING_OUTCOME_SENTENCES: &str = "MISSING_OUTCOME_SENTENCES";
pub const WARN_MISSING_OUTCOME_SENTENCES: &str = "MISSING_OUTCOME_SENTENCES";

/// Structured validation diagnostic (error or warning) with stable code, message, and fix hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationDiagnostic {
    pub code: String,
    pub message: String,
    pub hint: String,
}

/// Options controlling content validation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ValidationOptions {
    pub require_outcomes: bool,
}

/// Result of running `validate_content` / `validate_content_with_options` on a `ContentPack`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub schema_version: Option<i64>,
    pub event_count: usize,
    pub json_event_count: usize,
    pub missing_outcome_choices: usize,
    pub missing_json_outcome_choices: usize,
    pub missing_builtin_outcome_choices: usize,
    pub errors: Vec<ValidationDiagnostic>,
    pub warnings: Vec<ValidationDiagnostic>,
}

impl ValidationReport {
    /// Returns `true` when validation succeeded with zero errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validate a `ContentPack` using default options (`require_outcomes = false`).
pub fn validate_content(content: &ContentPack) -> ValidationReport {
    validate_content_with_options(content, ValidationOptions::default())
}

/// Validate a `ContentPack` with explicit options (`--require-outcomes`).
pub fn validate_content_with_options(
    content: &ContentPack,
    options: ValidationOptions,
) -> ValidationReport {
    let mut errors: Vec<ValidationDiagnostic> = Vec::new();
    let mut warnings: Vec<ValidationDiagnostic> = Vec::new();

    // ── Rule 1: schema_version present and equal to 1 ───────────────────────
    let mut schema_version: Option<i64> = None;
    match content.raw_manifest_value.get("schema_version") {
        Some(Value::Number(n)) => {
            if let Some(v) = n.as_i64() {
                schema_version = Some(v);
                if v != 1 {
                    errors.push(ValidationDiagnostic {
                        code: ERR_MISSING_SCHEMA_VERSION.to_string(),
                        message: format!(
                            "content_manifest.json has unsupported schema_version {v} (expected 1)"
                        ),
                        hint: "Set '\"schema_version\": 1' in content_manifest.json.".to_string(),
                    });
                }
            } else {
                errors.push(ValidationDiagnostic {
                    code: ERR_MISSING_SCHEMA_VERSION.to_string(),
                    message: "content_manifest.json schema_version must be integer 1".to_string(),
                    hint: "Set '\"schema_version\": 1' in content_manifest.json.".to_string(),
                });
            }
        }
        Some(_) => {
            errors.push(ValidationDiagnostic {
                code: ERR_MISSING_SCHEMA_VERSION.to_string(),
                message: "content_manifest.json schema_version must be integer 1".to_string(),
                hint: "Set '\"schema_version\": 1' in content_manifest.json.".to_string(),
            });
        }
        None => {
            errors.push(ValidationDiagnostic {
                code: ERR_MISSING_SCHEMA_VERSION.to_string(),
                message: "content_manifest.json is missing required field 'schema_version'"
                    .to_string(),
                hint: "Add '\"schema_version\": 1' to content_manifest.json.".to_string(),
            });
        }
    }

    if let Value::Object(obj) = &content.raw_events_value {
        match obj.get("schema_version") {
            Some(Value::Number(n)) if n.as_i64() == Some(1) => {}
            Some(other) => {
                errors.push(ValidationDiagnostic {
                    code: ERR_MISSING_SCHEMA_VERSION.to_string(),
                    message: format!(
                        "event_skeletons.json object wrapper has invalid schema_version {other} (expected 1)"
                    ),
                    hint: "Set '\"schema_version\": 1' in event_skeletons.json.".to_string(),
                });
            }
            None => {
                errors.push(ValidationDiagnostic {
                    code: ERR_MISSING_SCHEMA_VERSION.to_string(),
                    message:
                        "event_skeletons.json object wrapper is missing required field 'schema_version'"
                            .to_string(),
                    hint: "Add '\"schema_version\": 1' to event_skeletons.json.".to_string(),
                });
            }
        }
    }

    // ── Rule 2: Unique event_id across library and unique choice_id per event ─
    let mut seen_event_ids: Vec<String> = Vec::new();
    for ev in &content.all_events {
        if ev.event_id.trim().is_empty() {
            errors.push(ValidationDiagnostic {
                code: ERR_DUPLICATE_ID.to_string(),
                message: "event has an empty event_id".to_string(),
                hint: "Provide a unique non-empty 'id' for every event.".to_string(),
            });
        } else if seen_event_ids.iter().any(|id| id == &ev.event_id) {
            errors.push(ValidationDiagnostic {
                code: ERR_DUPLICATE_ID.to_string(),
                message: format!("duplicate event_id '{}' across event library", ev.event_id),
                hint: format!(
                    "Rename or remove the duplicate event_id '{}' so every event has a unique ID.",
                    ev.event_id
                ),
            });
        } else {
            seen_event_ids.push(ev.event_id.clone());
        }

        let mut seen_choice_ids: Vec<String> = Vec::new();
        for ch in &ev.fallback_choices {
            if ch.choice_id.trim().is_empty() {
                errors.push(ValidationDiagnostic {
                    code: ERR_DUPLICATE_ID.to_string(),
                    message: format!(
                        "event '{}' has a choice with an empty choice_id",
                        ev.event_id
                    ),
                    hint: "Assign distinct non-empty choice_id values (e.g. \"A\", \"B\", \"C\")."
                        .to_string(),
                });
            } else if seen_choice_ids.iter().any(|cid| cid == &ch.choice_id) {
                errors.push(ValidationDiagnostic {
                    code: ERR_DUPLICATE_ID.to_string(),
                    message: format!(
                        "event '{}' has duplicate choice_id '{}'",
                        ev.event_id, ch.choice_id
                    ),
                    hint: format!(
                        "Give each choice in event '{}' a unique choice_id.",
                        ev.event_id
                    ),
                });
            } else {
                seen_choice_ids.push(ch.choice_id.clone());
            }
        }
    }

    // Also inspect raw JSON events for explicit duplicate choice_ids before index assignment
    if let Some(raw_events) = extract_raw_events_slice(&content.raw_events_value) {
        for raw_ev in raw_events {
            let ev_id = raw_ev
                .get("id")
                .or_else(|| raw_ev.get("event_id"))
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            if let Some(Value::Array(raw_choices)) = raw_ev.get("choices") {
                let mut raw_seen_cids: Vec<String> = Vec::new();
                for (idx, raw_ch) in raw_choices.iter().enumerate() {
                    if let Some(cid) = raw_ch
                        .get("choice_id")
                        .or_else(|| raw_ch.get("id"))
                        .and_then(Value::as_str)
                    {
                        if raw_seen_cids.iter().any(|c| c == cid)
                            && !errors.iter().any(|e| {
                                e.code == ERR_DUPLICATE_ID
                                    && e.message.contains(ev_id)
                                    && e.message.contains(cid)
                            })
                        {
                            errors.push(ValidationDiagnostic {
                                code: ERR_DUPLICATE_ID.to_string(),
                                message: format!(
                                    "event '{ev_id}' has duplicate choice_id '{cid}' at choice index {idx}"
                                ),
                                hint: format!(
                                    "Assign distinct choice_id values within event '{ev_id}'."
                                ),
                            });
                        } else {
                            raw_seen_cids.push(cid.to_string());
                        }
                    }
                }
            }
        }
    }

    // ── Rule 3: Non-empty cost_hint and risk_hint on any gated choice ────────
    for ev in &content.all_events {
        for ch in &ev.fallback_choices {
            let Some(outcome) = ev.outcome_templates.get(&ch.choice_id) else {
                continue;
            };
            let adv_cost = advertised_inventory_cost(Some(ch), outcome);
            let is_gated = !adv_cost.is_empty();
            if is_gated && (ch.cost_hint.trim().is_empty() || ch.risk_hint.trim().is_empty()) {
                errors.push(ValidationDiagnostic {
                    code: ERR_EMPTY_GATE_HINT.to_string(),
                    message: format!(
                        "gated choice '{}' on event '{}' must have non-empty cost_hint and risk_hint",
                        ch.choice_id, ev.event_id
                    ),
                    hint: format!(
                        "Populate both 'cost_hint' and 'risk_hint' on choice '{}' of event '{}'.",
                        ch.choice_id, ev.event_id
                    ),
                });
            }
        }
    }

    if let Some(raw_events) = extract_raw_events_slice(&content.raw_events_value) {
        for raw_ev in raw_events {
            let ev_id = raw_ev
                .get("id")
                .or_else(|| raw_ev.get("event_id"))
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            if let Some(Value::Array(raw_choices)) = raw_ev.get("choices") {
                for (idx, raw_ch) in raw_choices.iter().enumerate() {
                    let cid = raw_ch
                        .get("choice_id")
                        .or_else(|| raw_ch.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            ["A", "B", "C", "D"]
                                .get(idx)
                                .copied()
                                .unwrap_or("A")
                                .to_string()
                        });
                    let explicit_gated = raw_ch.get("gated").and_then(Value::as_bool) == Some(true)
                        || raw_ch
                            .get("required_supplies")
                            .and_then(Value::as_object)
                            .is_some_and(|m| !m.is_empty())
                        || raw_ch
                            .get("required_trait")
                            .and_then(Value::as_str)
                            .is_some_and(|s| !s.trim().is_empty())
                        || raw_ch.get("cost_hint").is_some()
                        || raw_ch.get("risk_hint").is_some();
                    if explicit_gated {
                        let cost_ok = raw_ch
                            .get("cost_hint")
                            .and_then(Value::as_str)
                            .is_some_and(|s| !s.trim().is_empty());
                        let risk_ok = raw_ch
                            .get("risk_hint")
                            .and_then(Value::as_str)
                            .is_some_and(|s| !s.trim().is_empty());
                        if (!cost_ok || !risk_ok)
                            && !errors.iter().any(|e| {
                                e.code == ERR_EMPTY_GATE_HINT
                                    && e.message.contains(ev_id)
                                    && e.message.contains(&cid)
                            })
                        {
                            errors.push(ValidationDiagnostic {
                                code: ERR_EMPTY_GATE_HINT.to_string(),
                                message: format!(
                                    "gated choice '{cid}' on event '{ev_id}' has empty or missing cost_hint/risk_hint"
                                ),
                                hint: format!(
                                    "Provide non-empty 'cost_hint' and 'risk_hint' strings on gated choice '{cid}' of event '{ev_id}'."
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    // ── Rule 4: At least one affordable fallback choice per event ────────────
    for ev in &content.all_events {
        if !event_has_affordable_fallback(ev) {
            errors.push(ValidationDiagnostic {
                code: ERR_NO_AFFORDABLE_FALLBACK_CHOICE.to_string(),
                message: format!(
                    "event '{}' has no affordable fallback choice (all choices require inventory or deduct supplies)",
                    ev.event_id
                ),
                hint: format!(
                    "Ensure event '{}' has at least one choice with no inventory gate and no negative supply delta.",
                    ev.event_id
                ),
            });
        }
    }

    // ── Rule 5: Every trait, place, or event ID referenced exists ────────────
    validate_references(content, &seen_event_ids, &mut errors);

    // ── Rule 6: Optional outcome_sentence on every choice ────────────────────
    let mut missing_builtin_outcome_choices = 0usize;
    let mut missing_json_outcome_choices = 0usize;

    let builtin_count = content
        .all_events
        .len()
        .saturating_sub(content.json_events.len());
    for (idx, ev) in content.all_events.iter().enumerate() {
        for ch in &ev.fallback_choices {
            let has_outcome = ch
                .outcome_sentence
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
            if !has_outcome {
                if idx < builtin_count {
                    missing_builtin_outcome_choices += 1;
                } else {
                    missing_json_outcome_choices += 1;
                }
            }
        }
    }

    let missing_outcome_choices = missing_builtin_outcome_choices + missing_json_outcome_choices;

    if missing_outcome_choices > 0 {
        let diag = ValidationDiagnostic {
            code: if options.require_outcomes {
                ERR_MISSING_OUTCOME_SENTENCES.to_string()
            } else {
                WARN_MISSING_OUTCOME_SENTENCES.to_string()
            },
            message: format!(
                "{missing_outcome_choices} choices have no outcome sentence ({missing_json_outcome_choices} in event_skeletons.json, {missing_builtin_outcome_choices} in built-in events)"
            ),
            hint: "Populate non-empty 'outcome_sentence' on every choice before enabling '--require-outcomes' (Phase 3 gate).".to_string(),
        };
        if options.require_outcomes {
            errors.push(diag);
        } else {
            warnings.push(diag);
        }
    }

    ValidationReport {
        schema_version,
        event_count: content.all_events.len(),
        json_event_count: content.json_events.len(),
        missing_outcome_choices,
        missing_json_outcome_choices,
        missing_builtin_outcome_choices,
        errors,
        warnings,
    }
}

fn event_has_affordable_fallback(ev: &EventSkeleton) -> bool {
    if ev.fallback_choices.is_empty() {
        return false;
    }
    for ch in &ev.fallback_choices {
        let Some(outcome) = ev.outcome_templates.get(&ch.choice_id) else {
            continue;
        };
        let adv_cost = advertised_inventory_cost(Some(ch), outcome);
        let has_negative_supply = outcome
            .supplies_delta
            .iter()
            .any(|(k, &v)| INVENTORY_COST_KEYS.contains(&k.as_str()) && v < 0);
        if adv_cost.is_empty() && !has_negative_supply {
            return true;
        }
    }
    false
}

fn extract_raw_events_slice(raw: &Value) -> Option<&[Value]> {
    match raw {
        Value::Array(arr) => Some(arr.as_slice()),
        Value::Object(obj) => obj
            .get("events")
            .and_then(Value::as_array)
            .map(Vec::as_slice),
        _ => None,
    }
}

fn is_valid_trait_id(name: &str) -> bool {
    Trait::ALL.iter().any(|t| t.as_str() == name)
}

fn is_valid_place_id(name: &str) -> bool {
    name == "start"
        || name == "valley"
        || TOWN_NAMES.contains(&name)
        || LANDMARK_NAMES.contains(&name)
}

fn is_valid_precondition_expr(
    cond: &str,
    known_event_ids: &[String],
) -> Result<(), (String, String)> {
    let trimmed = cond.trim();
    if let Some(rest) = trimmed
        .strip_prefix("trait:")
        .or_else(|| trimmed.strip_prefix("has_trait:"))
        .or_else(|| trimmed.strip_prefix("trait=="))
    {
        let t = rest.trim();
        if !is_valid_trait_id(t) {
            return Err((
                format!("unknown trait '{t}' referenced in precondition '{trimmed}'"),
                "Use a canonical trait ID: tough, healer, tracker, mechanic, lucky, anxious, steady, sharp_eye.".to_string(),
            ));
        }
        return Ok(());
    }

    if let Some(rest) = trimmed
        .strip_prefix("place:")
        .or_else(|| trimmed.strip_prefix("at_place:"))
        .or_else(|| trimmed.strip_prefix("location:"))
        .or_else(|| trimmed.strip_prefix("place=="))
    {
        let p = rest.trim();
        if !is_valid_place_id(p) {
            return Err((
                format!("unknown place '{p}' referenced in precondition '{trimmed}'"),
                "Use a canonical place name from TOWN_NAMES or LANDMARK_NAMES.".to_string(),
            ));
        }
        return Ok(());
    }

    if let Some(rest) = trimmed
        .strip_prefix("event:")
        .or_else(|| trimmed.strip_prefix("seen_event:"))
        .or_else(|| trimmed.strip_prefix("after_event:"))
        .or_else(|| trimmed.strip_prefix("event=="))
    {
        let eid = rest.trim();
        if !known_event_ids.iter().any(|k| k == eid) {
            return Err((
                format!("unknown event_id '{eid}' referenced in precondition '{trimmed}'"),
                "Reference an existing event_id present in the event library.".to_string(),
            ));
        }
        return Ok(());
    }

    for op in [">=", "<=", "==", ">", "<"] {
        if let Some((lhs, rhs)) = trimmed.split_once(op) {
            let var = lhs.trim();
            let val = rhs.trim();
            let valid_var = matches!(
                var,
                "day"
                    | "party_alive"
                    | "morale"
                    | "wagon"
                    | "weirdness"
                    | "distance_traveled"
                    | "distance_remaining"
            ) || is_valid_resource_key(var);
            if !valid_var {
                return Err((
                    format!("unknown variable '{var}' in precondition '{trimmed}'"),
                    "Use a valid state variable (day, party_alive, morale, wagon, weirdness, supply key) or prefix with trait:/place:/event:.".to_string(),
                ));
            }
            if val.parse::<i64>().is_err() {
                return Err((
                    format!("non-integer threshold '{val}' in precondition '{trimmed}'"),
                    "Use an integer threshold in numeric preconditions.".to_string(),
                ));
            }
            return Ok(());
        }
    }

    Err((
        format!("unrecognized precondition reference '{trimmed}'"),
        "Use '<var><op><int>', 'trait:<id>', 'place:<name>', or 'event:<id>'.".to_string(),
    ))
}

fn validate_references(
    content: &ContentPack,
    known_event_ids: &[String],
    errors: &mut Vec<ValidationDiagnostic>,
) {
    let Some(raw_events) = extract_raw_events_slice(&content.raw_events_value) else {
        return;
    };

    for raw_ev in raw_events {
        let ev_id = raw_ev
            .get("id")
            .or_else(|| raw_ev.get("event_id"))
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");

        if let Some(Value::Array(preconds)) = raw_ev.get("preconditions") {
            for p in preconds {
                if let Some(cond_str) = p.as_str() {
                    if let Err((msg, hint)) = is_valid_precondition_expr(cond_str, known_event_ids)
                    {
                        errors.push(ValidationDiagnostic {
                            code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                            message: format!("event '{ev_id}': {msg}"),
                            hint,
                        });
                    }
                }
            }
        }

        if let Some(place_str) = raw_ev.get("place_id").and_then(Value::as_str) {
            if !is_valid_place_id(place_str) {
                errors.push(ValidationDiagnostic {
                    code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                    message: format!("event '{ev_id}' references unknown place_id '{place_str}'"),
                    hint: "Use a canonical place name from TOWN_NAMES or LANDMARK_NAMES."
                        .to_string(),
                });
            }
        }

        if let Some(Value::Array(raw_choices)) = raw_ev.get("choices") {
            for (idx, raw_ch) in raw_choices.iter().enumerate() {
                let cid = raw_ch
                    .get("choice_id")
                    .or_else(|| raw_ch.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        ["A", "B", "C", "D"]
                            .get(idx)
                            .copied()
                            .unwrap_or("A")
                            .to_string()
                    });

                if let Some(req_trait) = raw_ch.get("required_trait").and_then(Value::as_str) {
                    if !is_valid_trait_id(req_trait) {
                        errors.push(ValidationDiagnostic {
                            code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                            message: format!(
                                "event '{ev_id}' choice '{cid}' references unknown trait '{req_trait}'"
                            ),
                            hint: "Use a canonical trait ID: healer, mechanic, hunter, scout, cook, storyteller, musician, sharpshooter.".to_string(),
                        });
                    }
                }

                if let Some(Value::Object(profile)) = raw_ch.get("engine_effect_profile") {
                    for (k, v) in profile {
                        if matches!(
                            k.as_str(),
                            "follow_up_event_id" | "next_event_id" | "trigger_event" | "event_id"
                        ) {
                            if let Some(target_ev) = v.as_str() {
                                if !known_event_ids.iter().any(|id| id == target_ev) {
                                    errors.push(ValidationDiagnostic {
                                        code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                                        message: format!(
                                            "event '{ev_id}' choice '{cid}' outcome references unknown event_id '{target_ev}'"
                                        ),
                                        hint: "Reference an existing event_id present in the event library.".to_string(),
                                    });
                                }
                            }
                        } else if matches!(k.as_str(), "trait" | "required_trait" | "grant_trait") {
                            if let Some(t) = v.as_str() {
                                if !is_valid_trait_id(t) {
                                    errors.push(ValidationDiagnostic {
                                        code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                                        message: format!(
                                            "event '{ev_id}' choice '{cid}' outcome references unknown trait '{t}'"
                                        ),
                                        hint: "Use a canonical trait ID from Trait::ALL.".to_string(),
                                    });
                                }
                            }
                        } else if matches!(k.as_str(), "place" | "place_id" | "destination") {
                            if let Some(p) = v.as_str() {
                                if !is_valid_place_id(p) {
                                    errors.push(ValidationDiagnostic {
                                        code: ERR_UNKNOWN_REFERENCE_ID.to_string(),
                                        message: format!(
                                            "event '{ev_id}' choice '{cid}' outcome references unknown place '{p}'"
                                        ),
                                        hint: "Use a canonical place name from TOWN_NAMES or LANDMARK_NAMES.".to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
