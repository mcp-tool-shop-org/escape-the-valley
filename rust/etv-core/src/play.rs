//! Line-mode play loop: the first playable front end of the Rust game.
//!
//! The loop reads one line per turn, turns it into a [`PlayerIntent`], steps the engine and
//! prints what happened. It never draws from the engine's RNG and never changes `RunState`
//! itself; everything it prints is presentation.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::Arc;

use crate::engine::{EventChoiceInfo, StepEngine};
use crate::ledger::trail_ledger;
use crate::model::{GamePhase, IntentAction, PlayerIntent, RunState};
use crate::narrator::{
    build_narration_context_with_phase, evaluate_turn_matters, profile_interface_name,
    resolve_fallback_line, FallbackPhraseTable, LiveNarrator, Narrator, NarratorSettings,
    NarratorStats, NarratorTransport, PreTurnObservation,
};
use crate::physics::current_node;
use crate::screens::title_screen_lines;

/// The camp menu, shown with the status panel at every camp stop.
pub const CAMP_MENU: &str =
    "T travel   R rest   H hunt   F repair   P pace <slow|steady|hard>   Q save and quit";

/// The last-resort actions, shown under the camp menu.
pub const CAMP_LAST_RESORTS: &str =
    "Last resorts: X abandon cargo   D desperate repair   N hard ration";

/// Hint printed when a camp line is not understood.
pub const CAMP_HINT: &str = "Type T, R, H, F, P <slow|steady|hard>, X, D, N or Q.";

/// Heading shown when the trail forks.
pub const FORK_LINE: &str = "The trail forks.";

/// Line shown when the player saves and quits.
pub const QUIT_SAVED_LINE: &str = "Saved. Come back with: etv continue";

/// One fork option: `  A. Name (N miles)`.
pub fn route_line(letter: &str, route: &crate::engine::RouteOption) -> String {
    format!("  {letter}. {} ({} miles)", route.name, route.distance)
}

/// What a line typed at camp means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampCommand {
    Act(PlayerIntent),
    Quit,
}

/// Read a line typed at camp. Letters and action names are both accepted, in any case.
pub fn parse_camp_input(line: &str) -> Option<CampCommand> {
    let lower = line.trim().to_ascii_lowercase();
    let mut words = lower.split_whitespace();
    let first = words.next()?;
    let second = words.next();
    if words.next().is_some() {
        return None;
    }
    if matches!(first, "p" | "pace") {
        let pace = second.filter(|p| matches!(*p, "slow" | "steady" | "hard"))?;
        return Some(CampCommand::Act(
            PlayerIntent::new(IntentAction::ChangePace).with_pace(pace),
        ));
    }
    let action = match (first, second) {
        ("t" | "travel", None) => IntentAction::Travel,
        ("r" | "rest", None) => IntentAction::Rest,
        ("h" | "hunt", None) => IntentAction::Hunt,
        ("f" | "fix" | "repair", None) => IntentAction::Repair,
        ("x" | "abandon", None) | ("abandon", Some("cargo")) => IntentAction::AbandonCargo,
        ("d" | "desperate", None) | ("desperate", Some("repair")) => IntentAction::DesperateRepair,
        ("n" | "ration", None) | ("hard", Some("ration")) => IntentAction::HardRation,
        ("q" | "quit", None) => return Some(CampCommand::Quit),
        _ => return None,
    };
    Some(CampCommand::Act(PlayerIntent::new(action)))
}

/// Read a line typed at an event or a fork: an offered letter, or its number (1 for the first).
/// Returns the offered id exactly as the engine expects it.
pub fn parse_choice_input(line: &str, offered: &[String]) -> Option<String> {
    let typed = line.trim();
    if typed.is_empty() {
        return None;
    }
    if let Some(id) = offered.iter().find(|id| id.eq_ignore_ascii_case(typed)) {
        return Some(id.clone());
    }
    let n: usize = typed.parse().ok()?;
    offered.get(n.checked_sub(1)?).cloned()
}

fn strip_hint(hint: &str) -> &str {
    hint.trim().trim_end_matches(['.', ';', ',', ' '])
}

/// One choice line: `  A. Label (cost; risk)`. Empty hints are left out, and a hint's trailing
/// punctuation is dropped so the joined text never doubles it. A choice the party cannot
/// afford (V2 rules) ends with a plain tag naming the missing supplies:
/// `  A. Label [cannot afford: rope, parts]`.
pub fn choice_line(choice: &EventChoiceInfo) -> String {
    let hints: Vec<&str> = [choice.cost_hint.as_str(), choice.risk_hint.as_str()]
        .into_iter()
        .map(strip_hint)
        .filter(|h| !h.is_empty())
        .collect();
    let line = if hints.is_empty() {
        format!("  {}. {}", choice.id, choice.label)
    } else {
        format!("  {}. {} ({})", choice.id, choice.label, hints.join("; "))
    };
    if choice.affordable {
        line
    } else {
        format!("{line} [cannot afford: {}]", choice.missing.join(", "))
    }
}

/// One party member as the status panel shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberFacts {
    pub name: String,
    pub health: i32,
    /// The condition word (`healthy`, ...). Not shown for a member who is dead.
    pub condition: String,
    pub alive: bool,
}

/// The facts the status panel shows, before any wording: where the party is, who is alive, the
/// wagon, and every supply the party carries. Every front end draws from this, so none of them
/// can show different numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusFacts {
    pub day: i32,
    pub time_of_day: String,
    /// The place as `Name (biome)`, or the location id when the map has no such node.
    pub place: String,
    pub distance_traveled: i32,
    pub total_distance: i32,
    pub distance_remaining: i32,
    pub party: Vec<MemberFacts>,
    pub morale: i32,
    pub wagon_condition: i32,
    pub animals_health: i32,
    pub pace: String,
    /// Every supply as `(name, count)`, in key order.
    pub supplies: Vec<(String, i32)>,
}

/// Read the status facts from the run state.
pub fn status_facts(state: &RunState) -> StatusFacts {
    let place = current_node(state)
        .map(|n| format!("{} ({})", n.name, n.biome.as_str()))
        .unwrap_or_else(|| state.location_id.clone());
    StatusFacts {
        day: state.day,
        time_of_day: state.time_of_day.as_str().to_string(),
        place,
        distance_traveled: state.distance_traveled,
        total_distance: state.total_distance,
        distance_remaining: state.distance_remaining,
        party: state
            .party
            .members
            .iter()
            .map(|m| MemberFacts {
                name: m.name.clone(),
                health: m.health,
                condition: m.condition.as_str().to_string(),
                alive: m.is_alive(),
            })
            .collect(),
        morale: state.party.morale,
        wagon_condition: state.wagon.condition,
        animals_health: state.wagon.animals_health,
        pace: state.wagon.pace.as_str().to_string(),
        supplies: state
            .supplies
            .items
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect(),
    }
}

/// The status panel shown at camp: where the party is, who is alive, the wagon, and every
/// supply the party carries.
pub fn status_lines(state: &RunState) -> Vec<String> {
    let facts = status_facts(state);
    let party = facts
        .party
        .iter()
        .map(|m| {
            if m.alive {
                format!("{} {} ({})", m.name, m.health, m.condition)
            } else {
                format!("{} (dead)", m.name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let supplies = facts
        .supplies
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        format!(
            "Day {}, {}. {}. {} of {} miles, {} to the next stop.",
            facts.day,
            facts.time_of_day,
            facts.place,
            facts.distance_traveled,
            facts.total_distance,
            facts.distance_remaining
        ),
        format!("Party: {party}. Morale {}.", facts.morale),
        format!(
            "Wagon {}, animals {}, pace {}.",
            facts.wagon_condition, facts.animals_health, facts.pace
        ),
        format!("Supplies: {supplies}."),
    ]
}

/// The changes a turn actually applied, as one line of numbers (`food -2, morale +1`), or `None`
/// when nothing changed. Keys are in alphabetical order.
pub fn deltas_line(deltas: &BTreeMap<String, i32>) -> Option<String> {
    let parts: Vec<String> = deltas
        .iter()
        .filter(|(_, v)| **v != 0)
        .map(|(k, v)| format!("{k} {v:+}"))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Step the engine once and return the lines to show for that turn.
///
/// `chosen_label` is the label of the option the player picked, when the turn resolves an event
/// choice. On such a turn the outcome is shown as the narrator's text when it gives one;
/// otherwise the chosen option is shown beside the plain fallback line, so two turns with the
/// same fallback line still read differently. The numbers the turn applied follow on their own
/// line. The narrator is called on every turn.
pub fn play_turn(
    engine: &mut StepEngine,
    narrator: &dyn Narrator,
    table: Option<&FallbackPhraseTable>,
    intent: &PlayerIntent,
    chosen_label: Option<&str>,
) -> Vec<String> {
    let pre = PreTurnObservation::capture(engine.phase, &engine.state);
    let pre_event_id = engine
        .pending_event()
        .map(|e| e.event_id.clone())
        .unwrap_or_default();
    let msgs = engine.step(intent);
    let event_id = engine
        .pending_event()
        .map(|e| e.event_id.clone())
        .unwrap_or(pre_event_id);
    let flags = evaluate_turn_matters(&pre, intent, &msgs, &engine.state);
    let ctx = build_narration_context_with_phase(
        &engine.state,
        engine.phase,
        &event_id,
        &intent.choice_id,
        &msgs,
        flags,
    );
    let narration = narrator.narrate_turn(&ctx);

    let mut out = msgs.lines.clone();
    if let Some(scene) = narration.scene.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push(scene.to_string());
    }
    let resolved_choice = !msgs.outcome_title.is_empty();
    match narration
        .outcome
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(text) => out.push(text.to_string()),
        None if resolved_choice => {
            let fallback = resolve_fallback_line(&ctx, table);
            match chosen_label.filter(|l| !l.is_empty()) {
                Some(label) => out.push(format!("[{label}] {fallback}")),
                None => out.push(fallback),
            }
        }
        None => {}
    }
    if resolved_choice {
        if let Some(line) = deltas_line(ctx.shown_deltas()) {
            out.push(format!("  {line}"));
        }
    }
    if let Some(ending) = &msgs.ending {
        if !ending.headline.is_empty() && !out.contains(&ending.headline) {
            out.push(ending.headline.clone());
        }
    }
    out
}

/// How a play session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayExit {
    /// The run reached its ending.
    Finished,
    /// The player chose to save and quit.
    Quit,
    /// The input ended (a closed stdin or the end of a script).
    InputEnded,
}

fn write_lines<W: Write>(out: &mut W, lines: &[String]) -> std::io::Result<()> {
    for line in lines {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// Run the play loop until the run ends, the player quits, or the input ends.
///
/// `version` is shown on the title screen. `table` is the fallback phrase table; `None` shows
/// the engine's own plain line instead.
pub fn run_play<R: BufRead, W: Write>(
    engine: &mut StepEngine,
    narrator: &dyn Narrator,
    table: Option<&FallbackPhraseTable>,
    version: &str,
    mut input: R,
    out: &mut W,
) -> std::io::Result<PlayExit> {
    write_lines(out, &title_screen_lines(version))?;
    loop {
        writeln!(out)?;
        let offered: Vec<String> = match engine.phase {
            GamePhase::GameOver => {
                write_lines(out, &trail_ledger(&engine.state))?;
                return Ok(PlayExit::Finished);
            }
            GamePhase::Camp => {
                write_lines(out, &status_lines(&engine.state))?;
                writeln!(out, "{CAMP_MENU}")?;
                writeln!(out, "{CAMP_LAST_RESORTS}")?;
                Vec::new()
            }
            GamePhase::Event => {
                if let Some(event) = engine.pending_event() {
                    writeln!(out, "== {} ==", event.title)?;
                    writeln!(out, "{}", event.fallback_narration)?;
                }
                for choice in engine.pending_event_choices() {
                    writeln!(out, "{}", choice_line(choice))?;
                }
                engine
                    .pending_event_choices()
                    .iter()
                    .map(|c| c.id.clone())
                    .collect()
            }
            GamePhase::Route => {
                writeln!(out, "{FORK_LINE}")?;
                let letters = crate::engine::route_offered_letters(engine.pending_routes());
                for (letter, route) in letters.iter().zip(engine.pending_routes()) {
                    writeln!(out, "{}", route_line(letter, route))?;
                }
                letters
            }
        };

        // Ask until a line is understood, then play one turn.
        let (intent, label) = loop {
            write!(out, "> ")?;
            out.flush()?;
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                writeln!(out)?;
                return Ok(PlayExit::InputEnded);
            }
            if engine.phase == GamePhase::Camp {
                match parse_camp_input(&line) {
                    Some(CampCommand::Quit) => {
                        writeln!(out, "{QUIT_SAVED_LINE}")?;
                        return Ok(PlayExit::Quit);
                    }
                    Some(CampCommand::Act(intent)) => break (intent, None),
                    None => writeln!(out, "{CAMP_HINT}")?,
                }
            } else {
                match parse_choice_input(&line, &offered) {
                    Some(id) => {
                        let label = engine
                            .pending_event_choices()
                            .iter()
                            .find(|c| c.id == id)
                            .map(|c| c.label.clone());
                        break (
                            PlayerIntent::new(IntentAction::Choose).with_choice(&id),
                            label,
                        );
                    }
                    None => writeln!(out, "Choose one of: {}.", offered.join(", "))?,
                }
            }
        };
        let lines = play_turn(engine, narrator, table, &intent, label.as_deref());
        write_lines(out, &lines)?;
    }
}

/// The result of trying to connect the live narrator at the start of a session.
pub struct NarratorConnection {
    /// The connected narrator, or `None` when the session plays with plain text lines.
    pub narrator: Option<LiveNarrator>,
    /// What to tell the player about it, in plain words.
    pub lines: Vec<String>,
}

/// Line shown when the narrator is off or could not be used.
pub const NARRATOR_OFF_LINE: &str =
    "Playing with plain text lines. Change the narrator with: etv settings";

/// Try to connect the live narrator described by `settings` over `transport`.
///
/// `transport` is `None` in a build that has no narrator connection compiled in. The game never
/// waits on a narrator that is not there: if the narrator is switched off, the service does not
/// answer, the model is not installed, or the model does not load within the warm-up time, the
/// session plays with plain text lines and says why.
pub fn connect_narrator(
    settings: &NarratorSettings,
    transport: Option<Arc<dyn NarratorTransport>>,
) -> NarratorConnection {
    let off = |reason: String| NarratorConnection {
        narrator: None,
        lines: vec![reason, NARRATOR_OFF_LINE.to_string()],
    };
    if !settings.enabled {
        return off("The narrator is switched off in the settings.".to_string());
    }
    let Some(transport) = transport else {
        return off("This build has no narrator connection.".to_string());
    };
    let narrator = LiveNarrator::with_transport(settings.clone(), transport);
    let status = narrator.check_startup();
    if !status.available {
        let reason = if status.installed {
            format!(
                "The narrator model '{}' is not installed in Ollama. Install it with: ollama pull {}",
                settings.model, settings.model
            )
        } else {
            format!(
                "The narrator service did not answer at {}. Start it with: ollama serve",
                settings.endpoint
            )
        };
        return off(reason);
    }
    if narrator.warmup().is_err() {
        return off(format!(
            "The narrator model '{}' did not load in time.",
            settings.model
        ));
    }
    let line = format!(
        "Narrator: {}, {}, model {}.",
        profile_interface_name(settings.profile),
        settings.length.as_str(),
        settings.model
    );
    NarratorConnection {
        narrator: Some(narrator),
        lines: vec![line],
    }
}

/// Line shown at the end of a session in which the narrator was paused by the timeout breaker.
pub const NARRATOR_PAUSED_LINE: &str =
    "The narrator stopped answering in time and was paused. Plain text lines took over.";

/// One line saying how the narrator did over a session: lines asked for, lines shown, and lines
/// that fell back to plain text (a timeout, a bad answer, or a line that broke a hard rule).
pub fn narrator_summary_line(stats: &NarratorStats) -> String {
    format!(
        "Narrator: {} lines asked for, {} shown, {} fell back to plain text ({} timed out, {} broke a rule).",
        stats.calls, stats.successes, stats.fallbacks, stats.timeouts, stats.rule_rejects
    )
}
