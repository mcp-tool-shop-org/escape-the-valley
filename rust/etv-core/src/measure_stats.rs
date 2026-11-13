//! Summaries computed from measurement rows: every rule a line breaks (not only the first),
//! repeated openings, latency per turn, and the paired per-turn comparison of two runs.
//!
//! Nothing here runs while the game is played. The checker a player's line goes through still
//! stops at the first broken rule; these functions are for reading a measurement run.
//!
//! The turn is the unit of comparison. Each turn of a set is asked many times in a run (once
//! per profile, length and slot), and those lines share the turn's facts, so they are not
//! independent. A turn's pass rate is the share of its lines that were accepted, and two runs
//! on the same turns are compared by the per-turn difference.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::measure::{AnyTurnSet, CellReport, LatencySummary, TurnRow};
use crate::model::GMProfile;
use crate::narrator::{
    check_generated_line_all, plain_apostrophes, profile_interface_name, CheckerContext,
    NarrationContext, NarratorLength,
};

/// One rule a line breaks, by the checker's reason-code name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleBreak {
    pub code: String,
    pub detail: String,
}

/// An opening (the first three words of an accepted line) and how many accepted lines of the
/// cell start with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningCount {
    pub opening: String,
    pub count: usize,
}

/// How many of the most repeated openings a cell report lists.
pub const TOP_OPENINGS: usize = 5;

/// Every rule `candidate` breaks for this turn at `length`, at most one entry per rule, in the
/// checker's own order. The first entry is the rule the play-time checker reports.
pub fn all_rule_breaks(
    candidate: &str,
    ctx: &NarrationContext,
    length: NarratorLength,
) -> Vec<RuleBreak> {
    let checker = CheckerContext::from_narration_context(ctx, length);
    check_generated_line_all(&plain_apostrophes(candidate), &checker)
        .into_iter()
        .map(|f| RuleBreak {
            code: f.code.as_str().to_string(),
            detail: f.detail,
        })
        .collect()
}

/// The opening of a line: its first three words, lowercased, with punctuation trimmed from
/// each word. A line of fewer than three words is its own opening.
pub fn opening_key(line: &str) -> String {
    line.split_whitespace()
        .take(3)
        .map(|w| {
            w.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// `true` when a unit-rule break names `coin` or `coins` (counted on its own because coin is
/// also a supply the turn may have changed).
pub fn is_coin_unit_break(b: &RuleBreak) -> bool {
    b.code == "numbers_digits_or_units"
        && (b.detail.contains("'coin'") || b.detail.contains("'coins'"))
}

/// Fill the fields of `cell` that are read off its rows: the all-rules counts, the coin count,
/// the openings, and latency per turn. `rows` are the cell's rows only.
pub fn fill_derived(cell: &mut CellReport, rows: &[TurnRow]) {
    let mut all: BTreeMap<String, usize> = BTreeMap::new();
    let mut lines_checked = 0;
    let mut lines_breaking_more_than_one = 0;
    let mut coin = 0;
    let mut openings: BTreeMap<String, usize> = BTreeMap::new();
    let mut accepted_lines = 0;
    let mut turn_latencies = Vec::new();
    for row in rows {
        for slot in [&row.scene, &row.outcome] {
            if let Some(candidate) = &slot.candidate {
                lines_checked += 1;
                for b in &slot.all_rule_breaks {
                    *all.entry(b.code.clone()).or_insert(0) += 1;
                    if is_coin_unit_break(b) {
                        coin += 1;
                    }
                }
                if slot.all_rule_breaks.len() > 1 {
                    lines_breaking_more_than_one += 1;
                }
                if slot.status == "accepted" {
                    accepted_lines += 1;
                    *openings.entry(opening_key(candidate)).or_insert(0) += 1;
                }
            }
        }
        if let Some(ms) = row.latency_ms_turn {
            turn_latencies.push(ms);
        }
    }
    let unique = openings.values().filter(|n| **n == 1).count();
    let mut top: Vec<OpeningCount> = openings
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(opening, count)| OpeningCount {
            opening: opening.clone(),
            count: *count,
        })
        .collect();
    // Most repeated first; ties in alphabetical order, so the list is stable.
    top.sort_by(|a, b| b.count.cmp(&a.count).then(a.opening.cmp(&b.opening)));
    top.truncate(TOP_OPENINGS);

    cell.lines_checked = lines_checked;
    cell.rejects_all_rules = all;
    cell.lines_breaking_more_than_one_rule = lines_breaking_more_than_one;
    cell.unit_rule_coin_rejects = coin;
    cell.accepted_lines = accepted_lines;
    cell.distinct_openings = openings.len();
    cell.unique_opening_share = if accepted_lines == 0 {
        0.0
    } else {
        unique as f64 / accepted_lines as f64
    };
    cell.top_openings = top;
    cell.latency_turn = LatencySummary::from_samples(&turn_latencies);
}

/// The profile for an interface name as rows carry it (`Chronicler`, `Fireside`,
/// `Lantern-Bearer`).
pub fn profile_from_interface_name(name: &str) -> Option<GMProfile> {
    [
        GMProfile::Chronicler,
        GMProfile::Fireside,
        GMProfile::Lantern,
    ]
    .into_iter()
    .find(|p| profile_interface_name(*p) == name)
}

/// The length for the lowercase name rows carry (`brief`, `standard`, `rich`).
pub fn length_from_row_name(name: &str) -> Option<NarratorLength> {
    match name {
        "brief" => Some(NarratorLength::Brief),
        "standard" => Some(NarratorLength::Standard),
        "rich" => Some(NarratorLength::Rich),
        _ => None,
    }
}

/// Recompute `all_rule_breaks` and `latency_ms_turn` for rows read from a file, using the
/// turn set the run was made on and the checker of this build. Rows written before these
/// fields existed get them this way. The status and first-rule code of each slot are left as
/// the run recorded them.
pub fn recompute_row_details(rows: &mut [TurnRow], set: &AnyTurnSet) -> Result<(), String> {
    let turns = set.measured_turns();
    let by_id: BTreeMap<&str, usize> = turns.iter().enumerate().map(|(i, t)| (t.id(), i)).collect();
    for row in rows.iter_mut() {
        let profile = profile_from_interface_name(&row.profile)
            .ok_or_else(|| format!("unknown profile {:?} in a row", row.profile))?;
        let length = length_from_row_name(&row.length)
            .ok_or_else(|| format!("unknown length {:?} in a row", row.length))?;
        let at = by_id
            .get(row.turn_id.as_str())
            .ok_or_else(|| format!("turn {:?} is not in the turn set", row.turn_id))?;
        let ctx = turns[*at].context(profile);
        for slot in [&mut row.scene, &mut row.outcome] {
            slot.all_rule_breaks = match &slot.candidate {
                Some(candidate) => all_rule_breaks(candidate, &ctx, length),
                None => Vec::new(),
            };
        }
        row.latency_ms_turn = match (row.scene.latency_ms, row.outcome.latency_ms) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };
    }
    Ok(())
}

/// Cell reports rebuilt from rows alone, one per profile and length in the order they first
/// appear. The counts are read from each slot's recorded status, so a report rebuilt from a
/// run's rows equals the report the run wrote.
pub fn cell_reports_from_rows(rows: &[TurnRow]) -> Vec<CellReport> {
    let mut order: Vec<(String, String)> = Vec::new();
    for row in rows {
        let key = (row.profile.clone(), row.length.clone());
        if !order.contains(&key) {
            order.push(key);
        }
    }
    let mut cells = Vec::new();
    for (profile, length) in order {
        let mine: Vec<TurnRow> = rows
            .iter()
            .filter(|r| r.profile == profile && r.length == length)
            .cloned()
            .collect();
        let mut cell = CellReport {
            profile,
            length,
            turns: mine.len(),
            slots: mine.len() * 2,
            accepted_slots: 0,
            fallback_slots: 0,
            turns_with_fallback: 0,
            timeouts: 0,
            bad_json: 0,
            transport_or_http_errors: 0,
            not_called: 0,
            rule_rejects: 0,
            rejects_by_code: BTreeMap::new(),
            fallback_rate_slots: 0.0,
            fallback_rate_turns: 0.0,
            latency: LatencySummary::from_samples(&[]),
            lines_checked: 0,
            rejects_all_rules: BTreeMap::new(),
            lines_breaking_more_than_one_rule: 0,
            unit_rule_coin_rejects: 0,
            accepted_lines: 0,
            distinct_openings: 0,
            unique_opening_share: 0.0,
            top_openings: Vec::new(),
            latency_turn: LatencySummary::from_samples(&[]),
            calls_made: 0,
        };
        let mut latencies = Vec::new();
        for row in &mine {
            let mut fell_back = false;
            for slot in [&row.scene, &row.outcome] {
                if let Some(ms) = slot.latency_ms {
                    latencies.push(ms);
                }
                // Rows written before `attempts` existed made one call per line asked.
                cell.calls_made += if slot.attempts > 0 {
                    slot.attempts
                } else {
                    usize::from(slot.status != "not_called")
                };
                match slot.status.as_str() {
                    "accepted" => cell.accepted_slots += 1,
                    other => {
                        cell.fallback_slots += 1;
                        fell_back = true;
                        match other {
                            "timeout" => cell.timeouts += 1,
                            "bad_envelope" | "bad_slot_json" => cell.bad_json += 1,
                            "transport_error" | "http_status" => cell.transport_or_http_errors += 1,
                            "not_called" => cell.not_called += 1,
                            "rule_reject" => {
                                cell.rule_rejects += 1;
                                if let Some(code) = &slot.reject_code {
                                    *cell.rejects_by_code.entry(code.clone()).or_insert(0) += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            if fell_back {
                cell.turns_with_fallback += 1;
            }
        }
        cell.fallback_rate_slots = ratio(cell.fallback_slots, cell.slots);
        cell.fallback_rate_turns = ratio(cell.turns_with_fallback, cell.turns);
        cell.latency = LatencySummary::from_samples(&latencies);
        fill_derived(&mut cell, &mine);
        cells.push(cell);
    }
    cells
}

fn ratio(num: usize, den: usize) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

/// Per turn: `(lines accepted, lines asked)` over every row given for that turn.
pub fn turn_pass_counts(rows: &[TurnRow]) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for row in rows {
        let entry = out.entry(row.turn_id.clone()).or_insert((0, 0));
        for slot in [&row.scene, &row.outcome] {
            entry.1 += 1;
            if slot.status == "accepted" {
                entry.0 += 1;
            }
        }
    }
    out
}

/// The mean over turns of each turn's pass rate.
pub fn mean_turn_pass_rate(rows: &[TurnRow]) -> f64 {
    let counts = turn_pass_counts(rows);
    if counts.is_empty() {
        return 0.0;
    }
    let sum: f64 = counts.values().map(|(ok, n)| *ok as f64 / *n as f64).sum();
    sum / counts.len() as f64
}

/// Two runs compared on the turns they share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedSummary {
    /// Turns present in both runs. Turns in only one are left out and counted below.
    pub turns: usize,
    pub turns_only_in_a: usize,
    pub turns_only_in_b: usize,
    /// Mean per-turn pass rate of each run, over the shared turns.
    pub mean_pass_rate_a: f64,
    pub mean_pass_rate_b: f64,
    /// Mean of the per-turn differences, b minus a.
    pub mean_difference: f64,
    /// Sample standard deviation of the per-turn differences.
    pub sd_difference: f64,
    /// 95% interval for the mean difference: mean plus or minus 1.96 standard errors (normal
    /// approximation; with a few hundred turns it is close to the exact interval).
    pub ci95_low: f64,
    pub ci95_high: f64,
}

/// Compare run `b` with run `a` turn by turn. Each run may hold any number of rows per turn
/// (cells, slots); a turn's pass rate is over all of them.
pub fn paired_difference(a: &[TurnRow], b: &[TurnRow]) -> PairedSummary {
    let ca = turn_pass_counts(a);
    let cb = turn_pass_counts(b);
    let mut ra = Vec::new();
    let mut rb = Vec::new();
    for (id, (ok_a, n_a)) in &ca {
        if let Some((ok_b, n_b)) = cb.get(id) {
            ra.push(*ok_a as f64 / *n_a as f64);
            rb.push(*ok_b as f64 / *n_b as f64);
        }
    }
    let n = ra.len();
    let mean = |v: &[f64]| {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    let diffs: Vec<f64> = ra.iter().zip(&rb).map(|(x, y)| y - x).collect();
    let mean_d = mean(&diffs);
    let sd = if n < 2 {
        0.0
    } else {
        (diffs.iter().map(|d| (d - mean_d).powi(2)).sum::<f64>() / (n as f64 - 1.0)).sqrt()
    };
    let half = if n == 0 {
        0.0
    } else {
        1.96 * sd / (n as f64).sqrt()
    };
    PairedSummary {
        turns: n,
        turns_only_in_a: ca.len() - n,
        turns_only_in_b: cb.len() - n,
        mean_pass_rate_a: mean(&ra),
        mean_pass_rate_b: mean(&rb),
        mean_difference: mean_d,
        sd_difference: sd,
        ci95_low: mean_d - half,
        ci95_high: mean_d + half,
    }
}

/// What `narrator_measure --summarise-rows` prints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowsSummary {
    pub schema: String,
    pub turn_set_sha256: String,
    pub rows: usize,
    pub turns: usize,
    /// Mean over turns of the share of each turn's lines that were accepted.
    pub mean_turn_pass_rate: f64,
    pub cells: Vec<CellReport>,
    /// Present when a second set of rows was given with `--paired-with`: that run (b) compared
    /// with this one (a).
    pub paired: Option<PairedSummary>,
}

/// Schema tag of [`RowsSummary`].
pub const ROWS_SUMMARY_SCHEMA: &str = "etv-narrator-rows-summary-1";

/// Read rows files (one JSON row per line) into one list, in the order given.
pub fn read_rows_files(paths: &[String]) -> Result<Vec<TurnRow>, String> {
    let mut rows = Vec::new();
    for path in paths {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("cannot read rows {path}: {e}"))?;
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let row: TurnRow = serde_json::from_str(line)
                .map_err(|e| format!("{path} line {}: not a row: {e}", n + 1))?;
            rows.push(row);
        }
    }
    Ok(rows)
}

/// Summarise rows read from files against the turn set they were run on, and optionally
/// compare a second run with them.
pub fn summarise_rows(
    set: &AnyTurnSet,
    set_sha256: &str,
    mut rows: Vec<TurnRow>,
    paired_with: Option<Vec<TurnRow>>,
) -> Result<RowsSummary, String> {
    recompute_row_details(&mut rows, set)?;
    let paired = paired_with.map(|b| paired_difference(&rows, &b));
    Ok(RowsSummary {
        schema: ROWS_SUMMARY_SCHEMA.to_string(),
        turn_set_sha256: set_sha256.trim().to_ascii_lowercase(),
        rows: rows.len(),
        turns: turn_pass_counts(&rows).len(),
        mean_turn_pass_rate: mean_turn_pass_rate(&rows),
        cells: cell_reports_from_rows(&rows),
        paired,
    })
}
