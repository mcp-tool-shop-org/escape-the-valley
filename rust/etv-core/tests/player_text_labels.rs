//! No internal key reaches the player. The engine names supplies and changes by keys
//! (`lantern_oil`, `time_days`, `wagon_condition`); everything the player reads names them by
//! the one label table in `etv_core::labels`. These tests play whole runs through the real line
//! mode (`run_play`) and the real full-screen interface (`App` drawn with `view`), over several
//! seeds and styles of play, through camp, events, forks, crises, the end of a run and the
//! Trail Ledger, and look for a key-shaped token (`[a-z]+_[a-z_]+`) in every line shown.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::OnceLock;

use crossterm::event::KeyCode;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use etv_core::content::embedded_content_pack;
use etv_core::engine::{EventChoiceInfo, StepEngine};
use etv_core::labels::{label, label_in_sentence, relabel_text, LABELS};
use etv_core::memory::emit_resource_crisis_card;
use etv_core::model::{GamePhase, RESOURCE_CATALOG};
use etv_core::narrator::{FallbackPhraseTable, NarratorSettings, NullNarrator};
use etv_core::play::{choice_line, deltas_line, run_play, status_lines};
use etv_core::tui::{view, App, Model, Screen, ViewData};
use etv_core::worldgen::create_new_run_with_content;
use etv_core::{trail_ledger, trail_ledger_for_player};

const SEEDS: [i128; 5] = [1, 7, 42, 99, 2026];

/// The keys whose label is a different word: every supply in the engine's catalogue and every
/// change key in the phrase table's order, kept when its label, lowercased, is not the key
/// itself (`ammo` -> "Ammunition", `meds` -> "Medicine", `oil` -> "Lantern oil", and every key
/// with an underscore). `food` -> "Food" is the same word and is left out.
fn renamed_keys() -> &'static Vec<String> {
    static KEYS: OnceLock<Vec<String>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys: BTreeSet<String> =
            RESOURCE_CATALOG.iter().map(|r| r.key.to_string()).collect();
        keys.extend(table().ordered_keys().iter().map(|k| k.to_string()));
        keys.remove("none");
        keys.into_iter()
            .filter(|k| label(k).to_lowercase() != *k)
            .collect()
    })
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `line` with every label in the table and every approved fallback phrase blanked out, in any
/// case, so a key that is part of player text ("oil" in "Lantern oil" or in the approved "uses
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
/// - a key from [`renamed_keys`] as a whole word, in its raw lowercase form (`ammo`, `meds`,
///   `oil`), outside any label. "Ammunition" and "Lantern oil" never match.
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
            if renamed_keys().iter().any(|k| k == word) {
                found.push(format!("{word}  <-  {}", line.trim()));
            }
        }
    }
    found
}

/// Each key shown, once, with one line it was shown on.
fn report(hits: &[String]) -> String {
    let mut first: std::collections::BTreeMap<String, String> = Default::default();
    for h in hits {
        let (key, line) = h.split_once("  <-  ").unwrap_or((h, ""));
        let key = key.rsplit(' ').next().unwrap_or(key).to_string();
        first.entry(key).or_insert_with(|| line.to_string());
    }
    let shown: Vec<String> = first.iter().map(|(k, l)| format!("{k}: {l}")).collect();
    format!(
        "{} lines show a key:
{}",
        hits.len(),
        shown.join(
            "
"
        )
    )
}

fn table() -> &'static FallbackPhraseTable {
    static TABLE: OnceLock<FallbackPhraseTable> = OnceLock::new();
    TABLE.get_or_init(FallbackPhraseTable::embedded)
}

fn new_engine(seed: i128) -> StepEngine {
    let content = embedded_content_pack();
    StepEngine::with_content(create_new_run_with_content(seed, content), content)
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
    // With the shipped phrase table, and without one (the engine's own fallback lines).
    let runs = SEEDS
        .iter()
        .flat_map(|&seed| LINE_SCRIPTS.iter().map(move |&script| (seed, script, true)))
        .chain(SEEDS.iter().map(|&seed| (seed, LINE_SCRIPTS[1], false)));
    let mut run_count = 0;
    for (seed, script, with_table) in runs {
        run_count += 1;
        {
            let mut engine = new_engine(seed);
            let mut out = Vec::new();
            run_play(
                &mut engine,
                &NullNarrator,
                with_table.then(table),
                "TEST",
                Cursor::new(script.repeat(1500)),
                &mut out,
            )
            .unwrap();
            let text = String::from_utf8(out).unwrap();
            if engine.state.game_over {
                endings += 1;
            }
            if text.contains("TRAIL LEDGER") {
                ledgers += 1;
            }
            crises.extend(engine.state.resource_crises_seen.iter().cloned());
            hits.extend(
                key_tokens(&text)
                    .into_iter()
                    .map(|h| format!("seed {seed}: {h}")),
            );
        }
    }
    assert!(hits.is_empty(), "{}", report(&hits));
    assert_eq!(endings, run_count, "every run must end");
    assert_eq!(ledgers, endings);
    assert!(crises.len() >= 3, "the runs must reach crises: {crises:?}");
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
    for seed in SEEDS {
        for (camp_keys, choice_keys) in TUI_STYLES {
            let engine = new_engine(seed);
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
            let check = |app: &App<'_>, hits: &mut BTreeSet<String>| {
                for size in [(80, 24), (120, 40)] {
                    for h in key_tokens(&screen(&app.model, size)) {
                        hits.insert(h);
                    }
                }
            };
            // The title, help, settings and about screens once per run.
            check(&app, &mut hits);
            app.press_char('?');
            check(&app, &mut hits);
            app.press_code(KeyCode::Esc);
            app.press_char('o');
            check(&app, &mut hits);
            app.press_char('q');
            app.press_char('a');
            check(&app, &mut hits);
            app.press_code(KeyCode::Esc);
            app.press_code(KeyCode::Enter);
            for n in 0..4000usize {
                screens_seen.insert(format!("{:?}", app.model.screen));
                if app.model.resolved.is_some() {
                    app.press_code(KeyCode::Enter);
                } else {
                    match app.model.screen {
                        Screen::Camp => {
                            let k = camp[n % camp.len()];
                            if k == 'p' {
                                app.press_char('p');
                                check(&app, &mut hits);
                                app.press_char('h');
                            } else {
                                app.press_char(k);
                            }
                        }
                        Screen::Event | Screen::Fork => app.press_char(choice[n % choice.len()]),
                        Screen::GameOver => {
                            check(&app, &mut hits);
                            for _ in 0..40 {
                                app.press_code(KeyCode::PageDown);
                                check(&app, &mut hits);
                            }
                            break;
                        }
                        _ => app.press_code(KeyCode::Enter),
                    }
                }
                check(&app, &mut hits);
            }
            if app.engine.phase == GamePhase::GameOver {
                endings += 1;
            }
        }
    }
    let hits: Vec<String> = hits.into_iter().collect();
    assert!(hits.is_empty(), "{}", report(&hits));
    assert_eq!(
        endings,
        SEEDS.len() * TUI_STYLES.len(),
        "every run must end"
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
    // A run that ran out of lantern oil, then food: the crisis lines, the remembered card and
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
    assert!(
        shown.contains("What ran dry: lantern oil, food."),
        "{shown}"
    );
    assert!(key_tokens(&shown).is_empty(), "{shown}");
    // Only the supply names differ.
    assert_eq!(frozen.lines().count(), shown.lines().count());
    for (f, s) in frozen.lines().zip(shown.lines()) {
        let relabelled = f
            .replace("lantern_oil", "lantern oil")
            .replace("Lantern_oil", "Lantern oil");
        assert_eq!(relabelled, s);
    }

    state.resource_crises_seen.truncate(1);
    let shown = trail_ledger_for_player(&state).join(
        "
",
    );
    assert!(shown.contains("The lantern oil ran out first."), "{shown}");
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
        card.text.replace("lantern_oil", "lantern oil")
    );
    assert_eq!(relabel_text("No Lantern_oil Left"), "No Lantern oil Left");
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
        shown.contains("Supplies that carried you: Lantern oil"),
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
    assert!(status.contains("Lantern oil 6"), "{status}");
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
        "  A. Light the way [cannot afford: lantern oil, rope]"
    );
}

#[test]
fn the_raw_key_check_catches_single_word_keys_and_spares_labels() {
    let keys = renamed_keys();
    for k in [
        "ammo",
        "meds",
        "oil",
        "lantern_oil",
        "time_days",
        "wagon_condition",
    ] {
        assert!(keys.iter().any(|x| x == k), "{k} missing from {keys:?}");
    }
    for k in ["food", "water", "health", "distance", "morale"] {
        assert!(!keys.iter().any(|x| x == k), "{k} is its own label");
    }
    for raw in [
        "Hunt successful! -1 ammo.",
        "  meds -1",
        "Supplies: oil 3",
        "The party uses lantern_oil.",
        "Days +1, time_days +1",
    ] {
        assert!(!key_tokens(raw).is_empty(), "missed: {raw}");
    }
    for fine in [
        "Supplies: Ammunition 20, Lantern oil 6, Medicine 5.",
        "The party uses lantern oil and medicine.",
        "LANTERN OIL 3",
        "Food -2, Days +1, Wagon condition -2, Animals' health -1",
        "Gammon and ammonia; oiled boots.",
        "The party uses lamp oil and spends ammunition.",
    ] {
        assert!(
            key_tokens(fine).is_empty(),
            "false hit: {fine} {:?}",
            key_tokens(fine)
        );
    }
}
