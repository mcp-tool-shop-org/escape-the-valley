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
    check_generated_line_all, plain_apostrophes, profile_interface_name, NarrationContext,
    NarratorLength,
};
use crate::narrator_experiment::{checker_context_for, ExperimentArms};
use crate::narrator_truth::{
    opening_two_words, opens_with_event_text, says_what_happened_failures, TruthFacts,
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
/// checker's own order. The first entry is the rule the checker reported when the line was
/// written.
///
/// The line is checked as the run's arms check it (see [`checker_context_for`]), so a voice
/// an arm lets say "oxen" is not listed as breaking the draft-animal rule. The repeated-
/// opening rule is left out here: it depends on the line before, which a row does not hold.
pub fn all_rule_breaks(
    candidate: &str,
    ctx: &NarrationContext,
    length: NarratorLength,
    arms: ExperimentArms,
    is_outcome: bool,
) -> Vec<RuleBreak> {
    let checker = checker_context_for(ctx, length, ctx.gm_profile(), arms, is_outcome, None);
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

/// Rules 1a to 1e of `narrator_truth` applied to a line, whatever arm wrote it: does the line
/// say what happened? Used to score lines already written.
pub fn truth_breaks(candidate: &str, ctx: &NarrationContext, is_outcome: bool) -> Vec<RuleBreak> {
    let facts = TruthFacts::from_context(ctx, is_outcome);
    says_what_happened_failures(&plain_apostrophes(candidate), &facts)
        .into_iter()
        .map(|f| RuleBreak {
            code: f.code.as_str().to_string(),
            detail: f.detail,
        })
        .collect()
}

/// A word and the number of accepted lines of a cell that contain it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordCount {
    pub word: String,
    pub lines: usize,
}

/// How many of the most used content words a cell report lists.
pub const TOP_CONTENT_WORDS: usize = 8;

/// Words left out of the content-word count: articles, pronouns, prepositions, auxiliaries and
/// other words that carry no image. Everything else of four letters or more counts.
const STOP_WORDS: [&str; 44] = [
    "that", "this", "with", "from", "into", "over", "under", "through", "their", "there", "where",
    "when", "what", "which", "while", "have", "does", "been", "were", "will", "than", "then",
    "them", "they", "each", "every", "only", "still", "beyond", "behind", "beneath", "before",
    "after", "above", "below", "between", "against", "toward", "towards", "like", "more", "some",
    "none", "just",
];

/// The content words of a line, each once: lowercased, four letters or more, not a stop word.
pub fn content_words(line: &str) -> Vec<String> {
    let mut out: Vec<String> = line
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|w| w.len() >= 4 && !STOP_WORDS.contains(&w.as_str()))
        .collect();
    out.sort();
    out.dedup();
    out
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
    let mut swh_pass = 0;
    let mut swh_fail: BTreeMap<String, usize> = BTreeMap::new();
    let mut copying = 0;
    let mut repeating = 0;
    let mut oxen_lines = 0;
    let mut content: BTreeMap<String, usize> = BTreeMap::new();
    // The previous accepted line's opening, within one run (rows are in turn-set order).
    let mut previous: Option<(String, String)> = None;
    for row in rows {
        for (slot, is_outcome) in [(&row.scene, false), (&row.outcome, true)] {
            if slot.status == "accepted" {
                if let Some(candidate) = &slot.candidate {
                    if slot.truth_breaks.is_empty() {
                        swh_pass += 1;
                    }
                    for b in &slot.truth_breaks {
                        *swh_fail.entry(b.code.clone()).or_insert(0) += 1;
                    }
                    if slot.copies_event_text {
                        copying += 1;
                    }
                    let opening = opening_two_words(candidate);
                    let same_run = previous
                        .as_ref()
                        .is_some_and(|(run, _)| *run == row.run_key);
                    if is_outcome
                        && same_run
                        && previous.as_ref().is_some_and(|(_, open)| *open == opening)
                    {
                        repeating += 1;
                    }
                    previous = Some((row.run_key.clone(), opening));
                    let words = content_words(candidate);
                    if words.iter().any(|w| w == "oxen")
                        || candidate
                            .to_ascii_lowercase()
                            .split(|c: char| !c.is_ascii_alphabetic())
                            .any(|w| w == "ox")
                    {
                        oxen_lines += 1;
                    }
                    for w in words {
                        *content.entry(w).or_insert(0) += 1;
                    }
                }
            }
        }
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
    cell.says_what_happened_pass = swh_pass;
    cell.says_what_happened_rate = if accepted_lines == 0 {
        0.0
    } else {
        swh_pass as f64 / accepted_lines as f64
    };
    cell.says_what_happened_fail_by_code = swh_fail;
    cell.good_line_rate = if cell.slots == 0 {
        0.0
    } else {
        swh_pass as f64 / cell.slots as f64
    };
    cell.accepted_copying_event_text = copying;
    cell.accepted_repeating_opening = repeating;
    cell.oxen_share = if accepted_lines == 0 {
        0.0
    } else {
        oxen_lines as f64 / accepted_lines as f64
    };
    let mut top_words: Vec<WordCount> = content
        .into_iter()
        .map(|(word, lines)| WordCount { word, lines })
        .collect();
    top_words.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.word.cmp(&b.word)));
    top_words.truncate(TOP_CONTENT_WORDS);
    cell.top_content_words = top_words;
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

/// Recompute the fields read off a line for rows read from a file: every rule it breaks (as
/// `arms` check it), whether it says what happened, whether it copies the event's text, the
/// run it belongs to, and the turn's time. The turn set the run was made on and the checker
/// of this build are used. Rows written before these fields existed get them this way. The
/// status and first-rule code of each slot are left as the run recorded them.
pub fn recompute_row_details(
    rows: &mut [TurnRow],
    set: &AnyTurnSet,
    arms: ExperimentArms,
) -> Result<(), String> {
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
        row.run_key = turns[*at].run_key();
        for (slot, is_outcome) in [(&mut row.scene, false), (&mut row.outcome, true)] {
            fill_slot_details(slot, &ctx, length, arms, is_outcome);
        }
        row.latency_ms_turn = match (row.scene.latency_ms, row.outcome.latency_ms) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };
    }
    Ok(())
}

/// Fill the fields of one slot that are read off its line: every rule it breaks as `arms`
/// check it, whether it says what happened (rules 1a to 1e, whatever arm wrote it), and
/// whether it opens with the event's own text.
pub fn fill_slot_details(
    slot: &mut crate::measure::SlotRow,
    ctx: &NarrationContext,
    length: NarratorLength,
    arms: ExperimentArms,
    is_outcome: bool,
) {
    match &slot.candidate {
        Some(candidate) => {
            slot.all_rule_breaks = all_rule_breaks(candidate, ctx, length, arms, is_outcome);
            slot.truth_breaks = truth_breaks(candidate, ctx, is_outcome);
            slot.copies_event_text =
                opens_with_event_text(&plain_apostrophes(candidate), ctx.check_event_seed());
        }
        None => {
            slot.all_rule_breaks = Vec::new();
            slot.truth_breaks = Vec::new();
            slot.copies_event_text = false;
        }
    }
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
            says_what_happened_pass: 0,
            says_what_happened_rate: 0.0,
            says_what_happened_fail_by_code: BTreeMap::new(),
            good_line_rate: 0.0,
            accepted_copying_event_text: 0,
            accepted_repeating_opening: 0,
            oxen_share: 0.0,
            top_content_words: Vec::new(),
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
    arms: ExperimentArms,
) -> Result<RowsSummary, String> {
    recompute_row_details(&mut rows, set, arms)?;
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
