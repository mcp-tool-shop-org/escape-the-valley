//! No internal key reaches the player. The engine names supplies and changes by keys
//! (`lantern_oil`, `time_days`, `wagon_condition`); everything the player reads names them by
//! the one label table in `etv_core::labels`. These tests play whole runs through the real line
//! mode (`run_play`) and the real full-screen interface (`App` drawn with `view`), over several
//! seeds and styles of play, through camp, events, forks, crises, the end of a run and the
//! Trail Ledger, under both rulesets, and look for a raw key in every line shown: a key-shaped
//! token (`[a-z]+_[a-z_]+`), or a key whose label is a different word (`ammo`, `meds`) in any
//! case. They also check that every key the engine emits on the way has its own entry in the
//! table, since `label()` would quietly turn an unknown key into words.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::OnceLock;

use crossterm::event::KeyCode;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use etv_core::content::embedded_content_pack;
use etv_core::engine::{EventChoiceInfo, StepEngine};
use etv_core::labels::{label, label_in_sentence, relabel_text, LABELS, PHRASE_ONLY_KEYS};
use etv_core::memory::emit_resource_crisis_card;
use etv_core::model::{GamePhase, RunState, RESOURCE_CATALOG};
use etv_core::narrator::{
    compute_applied_turn_deltas, FallbackPhraseTable, NarratorSettings, NullNarrator,
    PreTurnObservation,
};
use etv_core::play::{choice_line, deltas_line, run_play, status_lines};
use etv_core::ruleset::Ruleset;
use etv_core::tui::{view, App, Model, Screen, ViewData};
use etv_core::worldgen::create_new_run_with_content;
use etv_core::{trail_ledger, trail_ledger_for_player};

const SEEDS: [i128; 5] = [1, 7, 42, 99, 2026];

/// The keys whose label is a different word: every supply in the engine's catalogue and every
/// change key in the phrase table's order, kept when its label, lowercased, is not the key
/// itself (`ammo` -> "Ammunition", `meds` -> "Medicine", `oil` -> "Lamp oil", and every key
/// with an underscore). `food` -> "Food" is the same word and is left out.
fn renamed_keys() -> &'static Vec<String> {
    static KEYS: OnceLock<Vec<String>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys: BTreeSet<String> =
            RESOURCE_CATALOG.iter().map(|r| r.key.to_string()).collect();
        keys.extend(table().ordered_keys().iter().map(|k| k.to_string()));
        keys.remove("none");
        // `oil` and `parts` are ordinary words too ("lamp oil", "in these parts"): prose keeps
        // them as written, so only the places that name a supply are checked for them (the
        // supplies panel and supply line tests below).
        keys.into_iter()
            .filter(|k| label(k).to_lowercase() != *k && !PHRASE_ONLY_KEYS.contains(&k.as_str()))
            .collect()
    })
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `line` with every label in the table and every approved fallback phrase blanked out, in any
/// case, so a key that is part of player text ("oil" in "Lamp oil" or in the approved "uses
/// lamp oil") is not taken for the key.
fn without_labels(line: &str) -> String {
    let mut out: Vec<char> = line.chars().collect();
    let lower: Vec<char> = line.to_lowercase().chars().collect();
    if lower.len() != out.len() {
        return line.to_string();
    }
    let approved = table()
        .phrases
        .values()
        .flat_map(|dirs| dirs.values().map(String::as_str));
    for l in LABELS.iter().map(|(_, l)| *l).chain(approved) {
        let needle: Vec<char> = l.to_lowercase().chars().collect();
        let mut i = 0;
        while i + needle.len() <= lower.len() {
            if lower[i..i + needle.len()] == needle[..] {
                for c in &mut out[i..i + needle.len()] {
                    *c = ' ';
                }
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    out.into_iter().collect()
}

/// Every raw key in `text`, one entry per hit with the line it was on:
/// - a key-shaped token: a lowercase letter, an underscore, then a lowercase letter or another
///   underscore (`[a-z]+_[a-z_]+`);
/// - a key from [`renamed_keys`] as a whole word, in any case (`ammo`, `Meds`, `AMMO`), outside
///   any label or approved phrase. "Ammunition" and "Lamp oil" never match.
fn key_tokens(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        for word in line.split(|c: char| !(c.is_ascii_lowercase() || c == '_')) {
            let b = word.as_bytes();
            let shaped = (1..b.len().saturating_sub(1)).any(|i| {
                b[i] == b'_'
                    && b[i - 1].is_ascii_lowercase()
                    && (b[i + 1].is_ascii_lowercase() || b[i + 1] == b'_')
            });
            if shaped {
                found.push(format!("{word}  <-  {}", line.trim()));
            }
        }
        let masked = without_labels(line);
        for word in masked.split(|c: char| !is_word_char(c)) {
            let lower = word.to_lowercase();
            if renamed_keys().contains(&lower) {
                found.push(format!("{word}  <-  {}", line.trim()));
            }
        }
    }
    found
}

/// A drawn screen as the text of each box: the rows are split at the box edges (`│`), and the
/// cells of consecutive rows with the same boxes are joined, so a label wrapped across rows
/// ("Lamp" / "oil") reads as one label again. One line per box.
fn screen_boxes(screen: &str) -> String {
    let mut out = Vec::new();
    let mut group: Vec<Vec<String>> = Vec::new();
    let flush = |group: &mut Vec<Vec<String>>, out: &mut Vec<String>| {
        if let Some(first) = group.first() {
            for k in 0..first.len() {
                let joined: Vec<&str> = group.iter().map(|cells| cells[k].trim()).collect();
                out.push(joined.join(" "));
            }
        }
        group.clear();
    };
    for row in screen.lines() {
        let cells: Vec<String> = row.split('\u{2502}').map(str::to_string).collect();
        if group.first().is_some_and(|g| g.len() != cells.len()) {
            flush(&mut group, &mut out);
        }
        group.push(cells);
    }
    flush(&mut group, &mut out);
    out.join("\n")
}

/// The keys the engine has emitted in `state`: every supply it holds, the supplies each journal
/// entry changed, the supplies that ran out, and the supply each crisis card names.
fn emitted_keys(state: &RunState, keys: &mut BTreeSet<String>) {
    keys.extend(state.supplies.items.keys().cloned());
    for entry in &state.journal {
        keys.extend(entry.deltas.keys().cloned());
    }
    keys.extend(state.resource_crises_seen.iter().cloned());
    for card in &state.memory_cards {
        // A resource crisis card is tagged with the supply first; other crisis cards (an escape
        // valve, a wagon breakdown) are tagged with what happened, not with a key.
        if card.source == "engine" && card.id.starts_with("eng_crisis_") {
            keys.extend(card.tags.first().cloned());
        }
    }
}

/// Every emitted key must have its own entry in the table: `label()` turns an unknown key into
/// words, so a new or misspelt key would never show as a raw key.
fn assert_all_labelled(keys: &BTreeSet<String>, must_include: &[&str]) {
    let missing: Vec<&String> = keys
        .iter()
        .filter(|k| !LABELS.iter().any(|(t, _)| t == k))
        .collect();
    assert!(
        missing.is_empty(),
        "emitted keys with no label: {missing:?}"
    );
    // The emitted set must reach past the supplies, or it proves little.
    for &k in must_include {
        assert!(keys.contains(k), "{k} was never emitted: {keys:?}");
    }
}

/// Each key shown, once, with one line it was shown on.
fn report(hits: &[String]) -> String {
    let mut first: std::collections::BTreeMap<String, String> = Default::default();
    for h in hits {
        let (key, line) = h.split_once("  <-  ").unwrap_or((h, ""));
        let key = key.rsplit(' ').next().unwrap_or(key).to_string();
        first
            .entry(key)
            .or_insert_with(|| line.chars().take(200).collect());
    }
    let shown: Vec<String> = first.iter().map(|(k, l)| format!("{k}: {l}")).collect();
    format!("{} lines show a key:\n{}", hits.len(), shown.join("\n"))
}

fn table() -> &'static FallbackPhraseTable {
    static TABLE: OnceLock<FallbackPhraseTable> = OnceLock::new();
    TABLE.get_or_init(FallbackPhraseTable::embedded)
}

fn new_engine(seed: i128) -> StepEngine {
    let content = embedded_content_pack();
    StepEngine::with_content(create_new_run_with_content(seed, content), content)
}

fn new_engine_with(seed: i128, ruleset: Ruleset) -> StepEngine {
    let mut engine = new_engine(seed);
    engine.set_ruleset(ruleset);
    engine
}

/// The styles of play, as line-mode input. A line the current screen does not take is refused
/// with a hint (also checked) and the next line is read, so every script moves the run on.
const LINE_SCRIPTS: [&str; 4] = [
    // Travel and take the first option.
    "t\na\n",
    // Hard pace, travel, last-resort actions, the second option.
    "pace hard\nt\nb\nn\nx\nd\nt\nb\n",
    // Careful: rest, hunt, repair, slow pace, a third option where there is one.
    "r\nh\nf\npace slow\nt\nc\na\n",
    // Everything else a player might type.
    "zzz\nt\n2\nhunt\nrepair\npace steady\nration\nabandon cargo\ndesperate repair\nt\n1\n",
];

#[test]
fn line_mode_shows_no_internal_key() {
    let mut endings = 0;
    let mut crises = BTreeSet::new();
    let mut ledgers = 0;
    let mut hits = Vec::new();
    let mut emitted = BTreeSet::new();
    // Under both rulesets with the shipped phrase table, and Legacy without one (the engine's
    // own fallback lines).
    let runs = [Ruleset::Legacy, Ruleset::V2]
        .into_iter()
        .flat_map(|ruleset| {
            SEEDS.iter().flat_map(move |&seed| {
                LINE_SCRIPTS
                    .iter()
                    .map(move |&script| (ruleset, seed, script, true))
            })
        })
        .chain(
            SEEDS
                .iter()
                .map(|&seed| (Ruleset::Legacy, seed, LINE_SCRIPTS[1], false)),
        );
    let mut run_count = 0;
    let mut v2_endings = 0;
    for (ruleset, seed, script, with_table) in runs {
        run_count += 1;
        {
            let mut engine = new_engine_with(seed, ruleset);
            let mut out = Vec::new();
            run_play(
                &mut engine,
                &NullNarrator,
                with_table.then(table),
                "TEST",
                // V2 refuses a choice the party cannot afford, so every script also offers the
                // other options in turn.
                Cursor::new(if ruleset == Ruleset::V2 {
                    format!(
                        "{script}a
b
c
d
"
                    )
                    .repeat(1500)
                } else {
                    script.repeat(1500)
                }),
                &mut out,
            )
            .unwrap();
            let text = String::from_utf8(out).unwrap();
            if engine.state.game_over {
                endings += 1;
                if ruleset == Ruleset::V2 {
                    v2_endings += 1;
                }
            }
            emitted_keys(&engine.state, &mut emitted);
            if text.contains("TRAIL LEDGER") {
                ledgers += 1;
            }
            crises.extend(engine.state.resource_crises_seen.iter().cloned());
            hits.extend(
                key_tokens(&text)
                    .into_iter()
                    .map(|h| format!("{ruleset:?} seed {seed}: {h}")),
            );
        }
    }
    assert!(hits.is_empty(), "{}", report(&hits));
    assert_eq!(endings, run_count, "every run must end");
    assert_eq!(v2_endings, SEEDS.len() * LINE_SCRIPTS.len());
    assert_eq!(ledgers, endings);
    assert!(crises.len() >= 3, "the runs must reach crises: {crises:?}");
    // Line mode shows the engine only through its output, so the keys here are the ones the
    // final states hold; the full-screen test also takes each turn's changes.
    assert_all_labelled(&emitted, &["lantern_oil", "meds", "ammo"]);
}

fn screen(model: &Model, size: (u16, u16)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// Keys pressed in turn, by screen. Each style is a cycle of camp keys and of option keys.
const TUI_STYLES: [(&str, &str); 3] = [("t", "a"), ("thtrft", "ba"), ("ntxtdt", "cab")];

#[test]
fn the_full_screen_interface_shows_no_internal_key() {
    let mut hits = BTreeSet::new();
    let mut endings = 0;
    let mut screens_seen = BTreeSet::new();
    let mut emitted = BTreeSet::new();
    let mut runs = 0;
    let mut unfinished: Vec<String> = Vec::new();
    for ruleset in [Ruleset::Legacy, Ruleset::V2] {
        // V2 on three of the seeds, to keep the test's time in bounds.
        let seeds = if ruleset == Ruleset::V2 {
            &SEEDS[..3]
        } else {
            &SEEDS[..]
        };
        for &seed in seeds {
            for (camp_keys, choice_keys) in TUI_STYLES {
                runs += 1;
                // V2 refuses a choice the party cannot afford: offer every option in turn.
                let choice_keys = if ruleset == Ruleset::V2 {
                    format!("{choice_keys}abcd")
                } else {
                    choice_keys.to_string()
                };
                let engine = new_engine_with(seed, ruleset);
                let model = Model::new(
                    ViewData::from_engine(&engine),
                    NarratorSettings::default(),
                    "TEST",
                    "HASH",
                    Vec::new(),
                    false,
                );
                let dir = tempfile::tempdir().unwrap();
                let mut app = App::new(
                    model,
                    engine,
                    &NullNarrator,
                    Some(table()),
                    dir.path().into(),
                    false,
                );
                let (camp, choice): (Vec<char>, Vec<char>) =
                    (camp_keys.chars().collect(), choice_keys.chars().collect());
                // Every screen at 80x24, where labels wrap soonest; the larger size on the
                // fixed screens, every tenth key and the end of the run.
                let check_at = |app: &App<'_>, hits: &mut BTreeSet<String>, both: bool| {
                    let sizes: &[(u16, u16)] = if both {
                        &[(80, 24), (120, 40)]
                    } else {
                        &[(80, 24)]
                    };
                    for &size in sizes {
                        for h in key_tokens(&screen_boxes(&screen(&app.model, size))) {
                            hits.insert(h);
                        }
                    }
                };
                // The title, help, settings and about screens once per run.
                check_at(&app, &mut hits, true);
                app.press_char('?');
                check_at(&app, &mut hits, true);
                app.press_code(KeyCode::Esc);
                app.press_char('o');
                check_at(&app, &mut hits, true);
                app.press_char('q');
                app.press_char('a');
                check_at(&app, &mut hits, true);
                app.press_code(KeyCode::Esc);
                app.press_code(KeyCode::Enter);
                // Each cycle advances only when one of its keys is pressed, so a refused option
                // (V2) is followed by the next one rather than skipped in step with Enter.
                let (mut camp_at, mut choice_at) = (0usize, 0usize);
                for n in 0..4000usize {
                    screens_seen.insert(format!("{:?}", app.model.screen));
                    // What this key press makes the engine emit: the changes a turn applies and,
                    // under V2, the supplies a choice lacks.
                    let pre = PreTurnObservation::capture(app.engine.phase, &app.engine.state);
                    let stepped = app.engine.state.turns_stepped;
                    for choice in app.engine.pending_event_choices() {
                        emitted.extend(choice.missing.iter().cloned());
                    }
                    if app.model.resolved.is_some() {
                        app.press_code(KeyCode::Enter);
                    } else {
                        match app.model.screen {
                            Screen::Camp => {
                                let k = camp[camp_at % camp.len()];
                                camp_at += 1;
                                if k == 'p' {
                                    app.press_char('p');
                                    check_at(&app, &mut hits, true);
                                    app.press_char('h');
                                } else {
                                    app.press_char(k);
                                }
                            }
                            Screen::Event | Screen::Fork => {
                                choice_at += 1;
                                app.press_char(choice[(choice_at - 1) % choice.len()])
                            }
                            Screen::GameOver => {
                                check_at(&app, &mut hits, true);
                                for _ in 0..40 {
                                    app.press_code(KeyCode::PageDown);
                                    check_at(&app, &mut hits, true);
                                }
                                break;
                            }
                            _ => app.press_code(KeyCode::Enter),
                        }
                    }
                    if app.engine.state.turns_stepped != stepped {
                        emitted.extend(
                            compute_applied_turn_deltas(&pre, &app.engine.state).into_keys(),
                        );
                    }
                    check_at(&app, &mut hits, n % 10 == 0);
                }
                if app.engine.phase == GamePhase::GameOver {
                    endings += 1;
                } else {
                    unfinished.push(format!(
                        "{ruleset:?} seed {seed} {camp_keys}/{choice_keys} on {:?} day {}",
                        app.model.screen, app.engine.state.day
                    ));
                }
                emitted_keys(&app.engine.state, &mut emitted);
            }
        }
    }
    let hits: Vec<String> = hits.into_iter().collect();
    assert!(hits.is_empty(), "{}", report(&hits));
    assert_eq!(endings, runs, "every run must end: {unfinished:?}");
    assert_all_labelled(
        &emitted,
        &[
            "lantern_oil",
            "time_days",
            "wagon_condition",
            "animals_health",
            "morale",
            "distance",
        ],
    );
    for s in ["Camp", "Event", "GameOver"] {
        assert!(
            screens_seen.contains(s),
            "{s} never shown: {screens_seen:?}"
        );
    }
}

#[test]
fn every_key_the_engine_can_name_has_a_label() {
    // Every supply, every change a turn can show, and every key a fallback phrase covers.
    let mut keys: BTreeSet<String> = RESOURCE_CATALOG.iter().map(|r| r.key.to_string()).collect();
    let state = create_new_run_with_content(42, embedded_content_pack());
    keys.extend(state.supplies.items.keys().cloned());
    keys.extend(table().ordered_keys().iter().map(|k| k.to_string()));
    keys.remove("none");
    for key in &keys {
        let shown = label(key);
        assert!(
            LABELS.iter().any(|(k, _)| k == key),
            "{key} has no entry in the label table"
        );
        assert!(!shown.contains('_'), "{key} -> {shown}");
        assert!(
            shown.chars().next().unwrap().is_uppercase(),
            "{key} -> {shown}"
        );
        assert!(!label_in_sentence(key).contains('_'));
    }
    // A key the table has never heard of still reads as words, never as a key.
    assert_eq!(label("some_new_key"), "Some new key");
    assert_eq!(label_in_sentence("some_new_key"), "some new key");
    // One entry per key.
    let distinct: BTreeSet<_> = LABELS.iter().map(|(k, _)| *k).collect();
    assert_eq!(distinct.len(), LABELS.len());
}

#[test]
fn the_ledger_names_supplies_by_label_and_keeps_its_frozen_form() {
    // A run that ran out of lamp oil, then food: the crisis lines, the remembered card and
    // the list all name the supply by its label in the ledger the player reads.
    let mut state = create_new_run_with_content(42, embedded_content_pack());
    state.game_over = true;
    state.cause_of_death = "The trail".to_string();
    emit_resource_crisis_card(&mut state, "lantern_oil");
    emit_resource_crisis_card(&mut state, "food");
    let frozen = trail_ledger(&state).join(
        "
",
    );
    let shown = trail_ledger_for_player(&state).join(
        "
",
    );
    assert!(
        frozen.contains("What ran dry: lantern_oil, food."),
        "{frozen}"
    );
    assert!(
        frozen.contains("Closest call: No Lantern_oil Left"),
        "{frozen}"
    );
    assert!(shown.contains("What ran dry: lamp oil, food."), "{shown}");
    assert!(key_tokens(&shown).is_empty(), "{shown}");
    // Only the supply names differ.
    assert_eq!(frozen.lines().count(), shown.lines().count());
    for (f, s) in frozen.lines().zip(shown.lines()) {
        let relabelled = f
            .replace("lantern_oil", "lamp oil")
            .replace("Lantern_oil", "Lamp oil");
        assert_eq!(relabelled, s);
    }

    state.resource_crises_seen.truncate(1);
    let shown = trail_ledger_for_player(&state).join(
        "
",
    );
    assert!(shown.contains("The lamp oil ran out first."), "{shown}");
    assert!(key_tokens(&shown).is_empty(), "{shown}");

    // The card keeps its saved text; only the shown copy is relabelled.
    let card = state
        .memory_cards
        .iter()
        .find(|c| c.tags[0] == "lantern_oil")
        .unwrap();
    assert!(card.text.contains("lantern_oil"));
    assert_eq!(
        relabel_text(&card.text),
        card.text.replace("lantern_oil", "lamp oil")
    );
    assert_eq!(relabel_text("No Lantern_oil Left"), "No Lamp oil Left");
    assert_eq!(relabel_text("an unknown_key stays"), "an unknown_key stays");
    assert_eq!(
        relabel_text("Hunt successful! +12 food. -1 ammo."),
        "Hunt successful! +12 food. -1 ammunition."
    );
    assert_eq!(
        relabel_text("Food for meds; Ammo low"),
        "Food for medicine; Ammunition low"
    );
    assert_eq!(relabel_text("uses lamp oil"), "uses lamp oil");
    assert_eq!(relabel_text("Gammon, ammonia"), "Gammon, ammonia");

    // A victory names the supply that carried the party.
    let mut won = create_new_run_with_content(42, embedded_content_pack());
    won.victory = true;
    won.game_over = true;
    for (k, _) in LABELS.iter().take(13) {
        won.supplies.set(k, 1);
    }
    won.supplies.set("lantern_oil", 90);
    let shown = trail_ledger_for_player(&won).join(
        "
",
    );
    assert!(
        shown.contains("Supplies that carried you: Lamp oil"),
        "{shown}"
    );
}

#[test]
fn the_status_panel_the_changes_line_and_a_choice_name_supplies_by_label() {
    let state = create_new_run_with_content(42, embedded_content_pack());
    let status = status_lines(&state).join(
        "
",
    );
    assert!(status.contains("Lamp oil 6"), "{status}");
    assert!(status.contains("Ammunition 20"), "{status}");
    assert!(status.contains("Medicine 5"), "{status}");
    assert!(key_tokens(&status).is_empty(), "{status}");

    let deltas: std::collections::BTreeMap<String, i32> = [
        ("distance", -4),
        ("morale", -2),
        ("time_days", 1),
        ("wagon_condition", -2),
        ("animals_health", -1),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    assert_eq!(
        deltas_line(&deltas).unwrap(),
        "Animals' health -1, Distance -4, Morale -2, Days +1, Wagon condition -2"
    );

    // Under V2 a choice the party cannot afford names what is missing.
    let choice = EventChoiceInfo {
        id: "A".to_string(),
        label: "Light the way".to_string(),
        risk_hint: String::new(),
        cost_hint: String::new(),
        affordable: false,
        missing: vec!["lantern_oil".to_string(), "rope".to_string()],
    };
    assert_eq!(
        choice_line(&choice),
        "  A. Light the way [cannot afford: lamp oil, rope]"
    );
}

#[test]
fn the_raw_key_check_catches_single_word_keys_and_spares_labels() {
    let keys = renamed_keys();
    for k in [
        "ammo",
        "meds",
        "lantern_oil",
        "time_days",
        "wagon_condition",
    ] {
        assert!(keys.iter().any(|x| x == k), "{k} missing from {keys:?}");
    }
    for k in ["food", "water", "health", "distance", "morale"] {
        assert!(!keys.iter().any(|x| x == k), "{k} is its own label");
    }
    // Ordinary words that prose keeps as written.
    for k in PHRASE_ONLY_KEYS {
        assert!(!keys.iter().any(|x| x == k), "{k} is prose");
    }
    for raw in [
        "Hunt successful! -1 ammo.",
        "Meds.",
        "AMMO LOW",
        "  meds -1",
        "Supplies: meds 3",
        "The party uses lantern_oil.",
        "Days +1, time_days +1",
    ] {
        assert!(!key_tokens(raw).is_empty(), "missed: {raw}");
    }
    for fine in [
        "Supplies: Ammunition 20, Lamp oil 6, Medicine 5.",
        "The party uses lamp oil and medicine.",
        "LAMP OIL 3",
        "Food -2, Days +1, Wagon condition -2, Animals' health -1",
        "Gammon and ammonia; oiled boots.",
        "In these parts the fog parts at dawn.",
        "The party uses lamp oil and spends ammunition.",
        "AMMUNITION 20, LAMP OIL 6",
    ] {
        assert!(
            key_tokens(fine).is_empty(),
            "false hit: {fine} {:?}",
            key_tokens(fine)
        );
    }
}

#[test]
fn a_label_wrapped_across_rows_is_still_a_label() {
    // Two boxes side by side, the right one wrapping "Lamp oil" over two rows: joined box by
    // box, the label reads as one again.
    let screen = "\u{2502} Party   \u{2502}\u{2502} Lamp     \u{2502}\n\u{2502} Ruth 90 \u{2502}\u{2502} oil 6    \u{2502}\n";
    let boxes = screen_boxes(screen);
    assert!(boxes.lines().any(|l| l == "Lamp oil 6"), "{boxes}");
    assert!(boxes.lines().any(|l| l == "Party Ruth 90"), "{boxes}");
    assert!(key_tokens(&boxes).is_empty(), "{boxes}");
    // A real key in a box is still caught.
    let leak = "\u{2502} Party   \u{2502}\u{2502} ammo 20  \u{2502}\n\u{2502} Ruth 90 \u{2502}\u{2502} oil 6    \u{2502}\n";
    assert!(!key_tokens(&screen_boxes(leak)).is_empty());
}

#[test]
fn relabel_handles_case_and_the_plural_possessive() {
    assert_eq!(relabel_text("AMMO."), "AMMUNITION.");
    assert_eq!(relabel_text("Meds."), "Medicine.");
    assert_eq!(relabel_text("the meds' cost"), "the medicine's cost");
    assert_eq!(relabel_text("the meds'"), "the medicine's");
    // An apostrophe that starts a contraction or another word is left alone.
    assert_eq!(relabel_text("ammo's gone"), "ammunition's gone");
    assert_eq!(relabel_text("'ammo'"), "'ammunition'");
    // A plural label keeps a plain apostrophe.
    assert_eq!(relabel_text("the time_days' toll"), "the days' toll");
}

#[test]
fn every_supply_label_uses_the_phrase_tables_word() {
    // Where the table and the approved fallback phrases both name a supply, they use the same
    // words: the panel says "Lamp oil" because the fallback line says "uses lamp oil", and
    // "Spare parts" because it says "uses spare parts". A supply's two phrases differ only in
    // their verb ("uses"/"gains", "wears down"/"gains"), so the words they end on in common are
    // the supply's name, and the label must be exactly that name. The phrase table names lantern
    // oil by its short key, `oil`.
    let phrases = &table().phrases;
    let mut checked = 0;
    for (key, shown) in LABELS {
        let is_supply = RESOURCE_CATALOG.iter().any(|r| r.key == key) || key == "oil";
        if !is_supply {
            continue;
        }
        let phrase_key = if key == "lantern_oil" { "oil" } else { key };
        let dirs = phrases
            .get(phrase_key)
            .unwrap_or_else(|| panic!("the phrase table has no {phrase_key}"));
        let neg: Vec<String> = dirs["neg"]
            .to_lowercase()
            .split(' ')
            .map(str::to_string)
            .collect();
        let pos: Vec<String> = dirs["pos"]
            .to_lowercase()
            .split(' ')
            .map(str::to_string)
            .collect();
        let common = neg
            .iter()
            .rev()
            .zip(pos.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let name = neg[neg.len() - common..].join(" ");
        // The label is that name, capitalised as a list item is: "Lamp oil", "Spare parts".
        let mut expected = name.clone();
        if let Some(first) = expected.get(0..1) {
            expected = first.to_uppercase() + &expected[1..];
        }
        assert_eq!(
            shown, expected,
            "{key} is labelled {shown:?}, but its phrases ({:?}, {:?}) name it {name:?}",
            dirs["neg"], dirs["pos"]
        );
        checked += 1;
    }
    assert_eq!(checked, 13, "the twelve supplies and oil");
    assert_eq!(label("lantern_oil"), "Lamp oil");
    assert_eq!(label("oil"), "Lamp oil");
    assert_eq!(label("parts"), "Spare parts");
}

#[test]
fn parts_in_prose_is_left_as_written() {
    // "parts" is an ordinary word as well as a key: free text keeps it, the narrator's included.
    for prose in [
        "in these parts",
        "The fog parts at dawn.",
        "parts of the wagon",
        "spare-parts",
        "spare
parts",
        "No parts for repairs.",
        "Wagon breakdown! No parts. Damage: 3",
        "The wagon broke down. Damage: 9. No spare parts on hand.",
        "PARTS",
    ] {
        assert_eq!(relabel_text(prose), prose);
    }
    // Where a supply is named, it is "Spare parts".
    let state = create_new_run_with_content(42, embedded_content_pack());
    let status = status_lines(&state).join(
        "
",
    );
    assert!(status.contains("Spare parts 4"), "{status}");
    let deltas: std::collections::BTreeMap<String, i32> =
        [("parts".to_string(), -1)].into_iter().collect();
    assert_eq!(deltas_line(&deltas).unwrap(), "Spare parts -1");
    assert_eq!(label_in_sentence("parts"), "spare parts");
}

#[test]
fn relabel_keeps_quotes_and_capitals() {
    // A quoted key is a quoted word, not a possessive, however many words the quote holds.
    assert_eq!(relabel_text("'meds'"), "'medicine'");
    assert_eq!(relabel_text("'the meds'"), "'the medicine'");
    assert_eq!(
        relabel_text("they said 'no meds'"),
        "they said 'no medicine'"
    );
    // A quote closed earlier on the line, or on another line, leaves a possessive a possessive.
    assert_eq!(
        relabel_text("'done', and the meds' cost"),
        "'done', and the medicine's cost"
    );
    assert_eq!(
        relabel_text(
            "'open
the meds' cost"
        ),
        "'open
the medicine's cost"
    );
    // An apostrophe inside a word opens nothing.
    assert_eq!(
        relabel_text("don't count the meds' cost"),
        "don't count the medicine's cost"
    );
    assert_eq!(
        relabel_text("they said 'meds' twice"),
        "they said 'medicine' twice"
    );
    assert_eq!(relabel_text("the meds' cost"), "the medicine's cost");
    // All capitals give the label in capitals, the possessive too.
    assert_eq!(relabel_text("AMMO'S GONE"), "AMMUNITION'S GONE");
    assert_eq!(relabel_text("THE MEDS' COST"), "THE MEDICINE'S COST");
    assert_eq!(relabel_text("LANTERN_OIL LOW"), "LAMP OIL LOW");
    // A capital first letter alone keeps the label's own case.
    assert_eq!(relabel_text("Ammo's gone"), "Ammunition's gone");
}
