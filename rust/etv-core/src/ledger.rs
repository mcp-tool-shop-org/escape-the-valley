//! Trail Ledger — story-first end-of-run summary.
//!
//! Pure function from [`RunState`] to lines (`Vec<String>`), ported 1:1 from legacy
//! `src/escape_the_valley/ledger.py`. No RNG draws, no state mutation, no AI calls.

use crate::model::{MemoryCard, RunState, RESOURCE_CATALOG};
use crate::physics::journey_pressure;

/// Build the Trail Ledger lines for an ended (or in-progress) run.
///
/// Mirrors legacy `build_trail_ledger(state: RunState) -> list[str]` in
/// `src/escape_the_valley/ledger.py:13-27`.
///
/// This is the ledger's frozen form, line-exact to the Python game: supplies appear by their
/// keys. What the player is shown is [`trail_ledger_for_player`].
pub fn trail_ledger(state: &RunState) -> Vec<String> {
    build(state, &Naming::KEYS)
}

/// The Trail Ledger as the player reads it: the same lines as [`trail_ledger`], with every
/// supply named by its label (`"The lantern oil ran out first."`) and keys in remembered card
/// text relabelled when shown. The run state and the frozen form are unchanged.
pub fn trail_ledger_for_player(state: &RunState) -> Vec<String> {
    build(state, &Naming::LABELS)
}

/// How the ledger names a supply: by key (the frozen form) or by label (what the player sees).
struct Naming {
    /// A supply after a colon, as a list item.
    item: fn(&str) -> String,
    /// A supply inside a sentence.
    in_sentence: fn(&str) -> String,
    /// Text remembered on a card, written with keys in it.
    card_text: fn(&str) -> String,
}

impl Naming {
    const KEYS: Naming = Naming {
        item: str::to_string,
        in_sentence: str::to_string,
        card_text: str::to_string,
    };
    const LABELS: Naming = Naming {
        item: crate::labels::label,
        in_sentence: crate::labels::label_in_sentence,
        card_text: crate::labels::relabel_text,
    };
}

fn build(state: &RunState, naming: &Naming) -> Vec<String> {
    let mut lines = Vec::new();
    lines.extend(header(state));
    lines.push(String::new());
    lines.extend(journey(state));
    lines.push(String::new());
    lines.extend(roll_call(state));
    lines.push(String::new());
    lines.extend(costliest_day(state));
    lines.extend(closest_call(state, naming));
    lines.extend(what_saved_or_killed(state, naming));
    lines.extend(promise(state, naming));
    lines.extend(doctrine_echo(state));
    lines
}

/// Alias matching legacy Python naming (`build_trail_ledger`).
pub fn build_trail_ledger(state: &RunState) -> Vec<String> {
    trail_ledger(state)
}

fn header(state: &RunState) -> Vec<String> {
    let mut out = vec![
        "========================================".to_string(),
        "TRAIL LEDGER".to_string(),
        "========================================".to_string(),
    ];
    if state.victory {
        out.push("THE VALLEY IS BEHIND YOU.".to_string());
    } else {
        let cause = if state.cause_of_death.is_empty() {
            "The trail"
        } else {
            state.cause_of_death.as_str()
        };
        out.push(format!("Cause: {cause}."));
    }
    out
}

fn journey(state: &RunState) -> Vec<String> {
    let pct = (journey_pressure(state) * 100.0) as i32;
    vec![
        format!("{} days on the trail.", state.day),
        format!(
            "Covered {} of {} miles ({pct}%).",
            state.distance_traveled, state.total_distance
        ),
    ]
}

fn roll_call(state: &RunState) -> Vec<String> {
    let mut out = vec!["The party:".to_string()];
    for m in &state.party.members {
        if m.is_alive() {
            out.push(format!(
                "  {} -- {}% health, {}",
                m.name,
                m.health,
                m.condition.as_str()
            ));
        } else {
            let death_day = find_death_day(state, &m.name);
            let cause = if m.death_cause.is_empty() {
                "unknown".to_string()
            } else {
                m.death_cause.to_lowercase()
            };
            if let Some(d) = death_day.filter(|&d| d != 0) {
                out.push(format!("  {} -- died of {cause}, day {d}", m.name));
            } else {
                out.push(format!("  {} -- died of {cause}", m.name));
            }
        }
    }
    out
}

fn costliest_day(state: &RunState) -> Vec<String> {
    if state.journal.is_empty() {
        return Vec::new();
    }

    let mut worst_day: i32 = 0;
    let mut worst_title = String::new();
    let mut worst_cost: i32 = 0;

    for entry in &state.journal {
        if entry.deltas.is_empty() {
            continue;
        }
        let cost: i32 = entry
            .deltas
            .values()
            .filter(|&&v| v < 0)
            .map(|&v| v.abs())
            .sum();
        if cost > worst_cost {
            worst_cost = cost;
            worst_day = entry.day;
            worst_title = entry.scene_title.clone();
        }
    }

    if worst_day == 0 {
        return Vec::new();
    }
    vec![
        format!("Costliest day: Day {worst_day} \u{2014} {worst_title}"),
        String::new(),
    ]
}

fn closest_call(state: &RunState, naming: &Naming) -> Vec<String> {
    let mut best: Option<&MemoryCard> = None;
    for c in &state.memory_cards {
        if (c.kind == "crisis" || c.kind == "wound")
            && c.salience >= 0.5
            && best.is_none_or(|b| c.salience > b.salience)
        {
            best = Some(c);
        }
    }
    let Some(closest) = best else {
        return Vec::new();
    };
    vec![
        format!("Closest call: {}", (naming.card_text)(&closest.title)),
        format!("  {}", (naming.card_text)(&closest.text)),
        String::new(),
    ]
}

fn what_saved_or_killed(state: &RunState, naming: &Naming) -> Vec<String> {
    let crises = &state.resource_crises_seen;
    if crises.is_empty() {
        if state.victory {
            let mut best_name: Option<&str> = None;
            let mut best_val = i32::MIN;
            for spec in RESOURCE_CATALOG {
                if let Some(&v) = state.supplies.items.get(spec.key) {
                    if v > best_val {
                        best_val = v;
                        best_name = Some(spec.key);
                    }
                }
            }
            for (k, &v) in &state.supplies.items {
                if !RESOURCE_CATALOG.iter().any(|s| s.key == k) && v > best_val {
                    best_val = v;
                    best_name = Some(k.as_str());
                }
            }
            if let Some(best) = best_name {
                if best_val > 0 {
                    return vec![
                        format!("Supplies that carried you: {}", (naming.item)(best)),
                        String::new(),
                    ];
                }
            }
        }
        return Vec::new();
    }

    if crises.len() == 1 {
        return vec![
            format!(
                "The {} ran out first. That changed everything.",
                (naming.in_sentence)(&crises[0])
            ),
            String::new(),
        ];
    }

    let joined = crises
        .iter()
        .map(|c| (naming.in_sentence)(c))
        .collect::<Vec<_>>()
        .join(", ");
    vec![format!("What ran dry: {joined}."), String::new()]
}

fn promise(state: &RunState, naming: &Naming) -> Vec<String> {
    let mut best: Option<&MemoryCard> = None;
    for c in &state.memory_cards {
        if (c.kind == "promise" || c.kind == "omen")
            && best.is_none_or(|b| c.day_created > b.day_created)
        {
            best = Some(c);
        }
    }
    let Some(card) = best else {
        return Vec::new();
    };
    if state.victory {
        vec![
            format!("A sign remembered: {}", (naming.card_text)(&card.title)),
            String::new(),
        ]
    } else {
        vec![
            format!(
                "An omen from the trail: {}",
                (naming.card_text)(&card.title)
            ),
            String::new(),
        ]
    }
}

fn doctrine_echo(state: &RunState) -> Vec<String> {
    let mut out = Vec::new();
    match state.doctrine.as_str() {
        "travel_light" => out.push("You traveled light. Less to carry, more to lose.".to_string()),
        "careful_hands" => {
            out.push("Every nail counted. The wagon remembers careful hands.".to_string())
        }
        "no_debts" => {
            out.push("No debts owed, none collected. The party held together.".to_string())
        }
        _ => {}
    }
    match state.taboo.as_str() {
        "never_night" => out.push("The caravan never traveled at night.".to_string()),
        "never_river" => out.push("They never drank river water.".to_string()),
        "leave_nothing" => out.push("They left nothing behind.".to_string()),
        _ => {}
    }
    if !out.is_empty() {
        out.push(String::new());
    }
    out
}

fn find_death_day(state: &RunState, name: &str) -> Option<i32> {
    for card in &state.memory_cards {
        if card.kind == "wound"
            && card.entities.iter().any(|e| e == name)
            && (card.id.to_lowercase().contains("death")
                || card.text.to_lowercase().contains("died"))
        {
            return Some(card.day_created);
        }
    }
    None
}
