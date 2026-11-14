//! The full-screen interface, part 1 (`etv tui`).
//!
//! Every screen is drawn into a ratatui `TestBackend`; no real terminal is used anywhere. The
//! golden screens in `tests/fixtures/tui/` are a regression pin of what the player sees. When a
//! change to the drawn text is meant, write them again with
//! `ETV_WRITE_GOLDEN=1 cargo test --test tui_part1 golden` and read the diff before committing.

use std::io::Cursor;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;

use etv_core::content::{embedded_content_pack, parse_cli_args, CliCommand};
use etv_core::engine::StepEngine;
use etv_core::model::GamePhase;
use etv_core::narrator::{FallbackPhraseTable, NarratorSettings, NullNarrator};
use etv_core::play::{run_play, PlayExit};
use etv_core::save::{state_to_value, SAVE_DIR, SAVE_FILE};
use etv_core::screens::{run_settings_screen, SettingsExit, TITLE_DISCLOSURE};
use etv_core::settings::save_settings;
use etv_core::tui::{
    install_panic_hook, key_to_msg, no_color_requested, update, view, App, Effect, Intent, Model,
    Msg, Overlay, Screen, TerminalGuard, ViewData, BUSY_LINE, HELP_LINES, TOO_SMALL_LINE,
};
use etv_core::worldgen::create_new_run_with_content;

const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (120, 40)];

/// The line-mode script of `tests/phase3_play_line_mode.rs`.
const SCRIPT_SEED_42: &str = "t\nzzz\na\nt\nt\nt\nhunt\npace hard\nt\n1\nt\nrest\na\nq\n";

/// What the line-mode settings test expects for these choices.
const EXPECTED_SETTINGS_JSON: &str = r#"{
  "enabled": false,
  "profile": "chronicler",
  "endpoint": "http://127.0.0.1:11434",
  "allow_non_loopback": false,
  "model": "qwen3:4b",
  "length": "brief",
  "warmup_timeout_ms": 30000,
  "per_call_timeout_ms": 10000
}
"#;

fn table() -> &'static FallbackPhraseTable {
    static TABLE: OnceLock<FallbackPhraseTable> = OnceLock::new();
    TABLE.get_or_init(FallbackPhraseTable::embedded)
}

fn new_engine(seed: i128) -> StepEngine {
    let content = embedded_content_pack();
    StepEngine::with_content(create_new_run_with_content(seed, content), content)
}

fn new_app_in(seed: i128, size: (u16, u16), base: &std::path::Path, color: bool) -> App<'static> {
    let engine = new_engine(seed);
    let mut model = Model::new(
        ViewData::from_engine(&engine),
        NarratorSettings::default(),
        "TEST",
        "HASH",
        Vec::new(),
        color,
    );
    model.size = size;
    App::new(
        model,
        engine,
        &NullNarrator,
        Some(table()),
        base.to_path_buf(),
        false,
    )
}

fn new_app(seed: i128, size: (u16, u16)) -> App<'static> {
    new_app_in(seed, size, std::path::Path::new("."), true)
}

fn enter(app: &mut App<'_>) {
    app.press_code(KeyCode::Enter);
}

fn draw(model: &Model, size: (u16, u16)) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    terminal.backend().buffer().clone()
}

fn text_of(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in 0..area.height {
        let mut row = String::new();
        for x in 0..area.width {
            row.push_str(buffer[(x, y)].symbol());
        }
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}

fn screen_text(model: &Model, size: (u16, u16)) -> String {
    text_of(&draw(model, size))
}

/// The screen as running text: box-drawing characters become spaces and runs of white space
/// collapse, so a sentence the screen wrapped can be found whole.
fn flat(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if "│─┌┐└┘├┤┬┴┼".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn golden(name: &str, size: (u16, u16), actual: &str) {
    let path = format!(
        "{}/tests/fixtures/tui/{name}_{}x{}.txt",
        env!("CARGO_MANIFEST_DIR"),
        size.0,
        size.1
    );
    if std::env::var_os("ETV_WRITE_GOLDEN").is_some() {
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("golden screen {path}: {e}"))
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "golden screen {name} at {size:?}");
}

/// Travel from camp until the first event is pending.
fn travel_to_event(app: &mut App<'_>) {
    for _ in 0..60 {
        if app.model.screen == Screen::Event {
            return;
        }
        app.press_char('t');
    }
    panic!("no event in 60 travel steps");
}

/// Play `t`, `a` until the run ends, dismissing the last outcome so the ledger shows.
fn play_to_game_over(app: &mut App<'_>) {
    enter(app);
    for _ in 0..20_000 {
        if app.model.screen == Screen::GameOver && app.model.resolved.is_none() {
            return;
        }
        if app.model.resolved.is_some() {
            enter(app);
        } else {
            app.press_char('t');
            app.press_char('a');
        }
    }
    panic!("the run did not end");
}

// ----------------------------------------------------------------------------------------
// 1. Golden screens at three sizes, and the facts each must show
// ----------------------------------------------------------------------------------------

#[test]
fn golden_title_screen() {
    for size in SIZES {
        let app = new_app(42, size);
        let text = screen_text(&app.model, size);
        golden("title", size, &text);
        assert!(flat(&text).contains(&flat(TITLE_DISCLOSURE)));
        assert!(flat(&text).contains("ESCAPE THE VALLEY"));
        assert!(flat(&text).contains("version TEST"));
    }
}

#[test]
fn golden_camp_screen_at_the_start_of_seed_42() {
    for size in SIZES {
        let mut app = new_app(42, size);
        enter(&mut app);
        assert_eq!(app.model.screen, Screen::Camp);
        let text = screen_text(&app.model, size);
        golden("camp", size, &text);
        let flat_text = flat(&text);
        for supply in [
            "Ammunition 20",
            "Boots 2",
            "Cloth 5",
            "Firewood 15",
            "Food 50",
            "Lantern oil 6",
            "Medicine 5",
            "Parts 4",
            "Rope 2",
            "Salt 8",
            "Tools 1",
            "Water 50",
        ] {
            assert!(
                flat_text.contains(supply),
                "supply {supply} at {size:?}
{text}"
            );
        }
        assert!(!flat_text.contains("lantern_oil"), "{text}");
        for fact in [
            "Coldspring (plains)",
            "Ruth 100 healthy",
            "Josiah 94 healthy",
            "Elias 87 healthy",
            "Agnes 80 healthy",
            "morale 70",
            "0 of 396",
            "Next stop 21",
            "Place Coldspring",
            "Wagon 80",
            "Animals 90",
            "Pace steady",
        ] {
            assert!(
                flat_text.contains(&flat(fact)),
                "{fact:?} at {size:?}\n{text}"
            );
        }
        for key in [
            "T Travel",
            "R Rest",
            "H Hunt",
            "F Repair",
            "P Pace",
            "Settings",
            "Help",
            "Quit",
            "Abandon cargo",
            "Desperate repair",
            "Hard ration",
        ] {
            assert!(flat_text.contains(key), "key bar {key} at {size:?}");
        }
    }
}

#[test]
fn golden_first_event_on_seed_42_before_and_after_choosing() {
    for size in SIZES {
        let mut app = new_app(42, size);
        enter(&mut app);
        travel_to_event(&mut app);
        let before = screen_text(&app.model, size);
        golden("event", size, &before);
        let flat_before = flat(&before);
        assert!(flat_before.contains("Persistent Rain"));
        let choices = app.model.view.choices.clone();
        assert!(choices.len() >= 2);
        for choice in &choices {
            assert!(
                flat_before.contains(&flat(&choice.label)),
                "choice label {:?} at {size:?}\n{before}",
                choice.label
            );
        }
        assert!(flat_before.contains("Continue despite the wet"));

        app.press_char('a');
        let after = screen_text(&app.model, size);
        golden("event_after", size, &after);
        let flat_after = flat(&after);
        assert!(
            flat_after.contains(
                "[Continue despite the wet] The party damages the wagon, loses heart, falls back on the trail and loses time."
            ),
            "{after}"
        );
        assert!(flat_after.contains("Distance -4, Morale -2, Days +1, Wagon condition -2"));
        assert!(flat_after.contains("Persistent Rain"));
        assert!(flat_after.contains("Enter Continue"));
    }
}

#[test]
fn golden_settings_screen() {
    for size in SIZES {
        let mut app = new_app(42, size);
        enter(&mut app);
        app.press_char('o');
        assert_eq!(app.model.screen, Screen::Settings);
        let text = screen_text(&app.model, size);
        golden("settings", size, &text);
        let flat_text = flat(&text);
        assert!(flat_text.contains("SETTINGS"));
        assert!(flat_text.contains("1. Narrator on"));
        assert!(flat_text.contains("2. Profile Fireside"));
        assert!(flat_text.contains("3. Length Standard"));
        assert!(flat_text.contains("4. Model llama3.2"));
        assert!(flat_text.contains(&flat(TITLE_DISCLOSURE)));
        assert!(flat_text.contains("S Save and leave"));
    }
}

#[test]
fn golden_help_overlay() {
    for size in SIZES {
        let mut app = new_app(42, size);
        enter(&mut app);
        app.press_char('?');
        assert_eq!(app.model.overlay, Overlay::Help);
        let text = screen_text(&app.model, size);
        golden("help", size, &text);
        let flat_text = flat(&text);
        for line in HELP_LINES {
            assert!(flat_text.contains(&flat(line)), "{line:?} at {size:?}");
        }
        // The camp screen is still drawn behind it.
        assert!(flat_text.contains("Supplies"));
    }
}

#[test]
fn golden_game_over_screen() {
    for size in SIZES {
        let mut app = new_app(42, size);
        play_to_game_over(&mut app);
        let text = screen_text(&app.model, size);
        golden("game_over", size, &text);
        let ledger = etv_core::trail_ledger(&app.engine.state);
        assert!(flat(&text).contains(&flat(&ledger[0])), "{text}");
        assert!(flat(&text).contains("Trail Ledger"));
    }
}

// ----------------------------------------------------------------------------------------
// 2. Too small
// ----------------------------------------------------------------------------------------

#[test]
fn a_too_small_window_asks_for_a_larger_one_and_does_not_panic() {
    let mut app = new_app(42, (60, 15));
    for step in 0..4 {
        match step {
            1 => enter(&mut app),
            2 => app.press_char('o'),
            3 => app.press_char('?'),
            _ => {}
        }
        for size in [(60, 15), (79, 24), (80, 23), (1, 1), (0, 0), (200, 3)] {
            let text = screen_text(&app.model, size);
            if size.0 >= 60 && size.1 > 0 {
                assert!(flat(&text).starts_with(&flat(TOO_SMALL_LINE)), "{size:?}");
            }
        }
    }
    let text = screen_text(&app.model, (60, 15));
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "one line only:\n{text}");
    assert_eq!(lines[0].trim(), TOO_SMALL_LINE);
    // Centred: the same number of blanks each side, within one column.
    let row = text.lines().find(|l| !l.trim().is_empty()).unwrap();
    let left = row.len() - row.trim_start().len();
    assert!(left > 0 && left.abs_diff(60 - left - TOO_SMALL_LINE.len()) <= 1);
    // Exactly the minimum is drawn normally.
    let ok = screen_text(&app.model, (80, 24));
    assert!(!ok.contains(TOO_SMALL_LINE));
}

// ----------------------------------------------------------------------------------------
// 3. update is pure; the two front ends are one game
// ----------------------------------------------------------------------------------------

fn comparable(engine: &StepEngine) -> serde_json::Value {
    let mut value = state_to_value(&engine.state);
    value.as_object_mut().unwrap().remove("run_id");
    value
}

/// The keys that stand for one line of the line-mode script.
fn keys_for_line(line: &str) -> Vec<char> {
    match line {
        "hunt" => vec!['h'],
        "rest" => vec!['r'],
        "pace hard" => vec!['p', 'h'],
        "zzz" => vec!['z', 'z', 'z'],
        other => {
            assert_eq!(other.len(), 1, "script line {other:?}");
            vec![other.chars().next().unwrap()]
        }
    }
}

#[test]
fn the_tui_plays_the_same_game_as_line_mode() {
    // Line mode, on the script.
    let mut line_engine = new_engine(42);
    let mut out = Vec::new();
    let exit = run_play(
        &mut line_engine,
        &NullNarrator,
        Some(table()),
        "TEST",
        Cursor::new(SCRIPT_SEED_42),
        &mut out,
    )
    .unwrap();
    assert_eq!(exit, PlayExit::Quit);

    // The TUI, on the keys that stand for the same lines (all but the closing quit, which
    // would only ask for confirmation).
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    let lines: Vec<&str> = SCRIPT_SEED_42.lines().collect();
    assert_eq!(*lines.last().unwrap(), "q");
    for line in &lines[..lines.len() - 1] {
        for key in keys_for_line(line) {
            app.press_char(key);
        }
    }
    assert!(!app.quit);
    assert_eq!(comparable(&app.engine), comparable(&line_engine));
    assert_eq!(app.engine.rng.counter, line_engine.rng.counter);
    // The script does play turns: the run is not still at its start.
    assert!(app.engine.rng.counter > new_engine(42).rng.counter);
    assert!(app.engine.state.day > 1);
}

#[test]
fn update_changes_only_the_model() {
    // `update` has no engine to touch: it answers with the work to do. A camp action asks
    // for a turn and changes nothing about the run on its own.
    let engine = new_engine(42);
    let counter_before = engine.rng.counter;
    let mut model = Model::new(
        ViewData::from_engine(&engine),
        NarratorSettings::default(),
        "TEST",
        "HASH",
        Vec::new(),
        true,
    );
    assert!(update(&mut model, Msg::Intent(Intent::Confirm)).is_empty());
    assert_eq!(model.screen, Screen::Camp);
    let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
    let msg = key_to_msg(&model, key).unwrap();
    let effects = update(&mut model, msg);
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        &effects[0],
        Effect::Turn {
            chosen_label: None,
            ..
        }
    ));
    assert_eq!(engine.rng.counter, counter_before);
    assert_eq!(comparable(&engine), comparable(&new_engine(42)));
    // Resize and Tick answer with no work.
    assert!(update(&mut model, Msg::Resize(100, 30)).is_empty());
    assert_eq!(model.size, (100, 30));
    assert!(update(&mut model, Msg::Tick).is_empty());
}

// ----------------------------------------------------------------------------------------
// 4. Whole runs
// ----------------------------------------------------------------------------------------

#[test]
fn a_whole_run_by_keys_ends_on_the_game_over_screen_with_the_ledger() {
    for seed in [1i128, 42, 2026] {
        let mut app = new_app(seed, (100, 30));
        play_to_game_over(&mut app);
        assert_eq!(app.engine.phase, GamePhase::GameOver, "seed {seed}");
        assert!(app.engine.state.game_over);
        assert_eq!(app.model.screen, Screen::GameOver);
        let ledger = etv_core::trail_ledger(&app.engine.state);
        let screen = screen_text(&app.model, (100, 30));
        assert!(
            flat(&screen).contains(&flat(&ledger[0])),
            "seed {seed}: first ledger line {:?} not on\n{screen}",
            ledger[0]
        );
        // The same run, played by line mode, ends in the same state.
        let mut line_engine = new_engine(seed);
        let mut out = Vec::new();
        let script = "t\na\n".repeat(3000);
        run_play(
            &mut line_engine,
            &NullNarrator,
            Some(table()),
            "TEST",
            Cursor::new(script),
            &mut out,
        )
        .unwrap();
        assert_eq!(comparable(&app.engine), comparable(&line_engine));
        assert_eq!(app.engine.rng.counter, line_engine.rng.counter);
    }
}

#[test]
fn the_fork_screen_lists_the_routes_and_choosing_one_plays_the_turn() {
    // Find a seed that reaches a fork before the end by playing t/a and watching the screen.
    let mut found = false;
    for seed in [1i128, 2, 3, 7, 42, 2026] {
        let mut app = new_app(seed, (80, 24));
        enter(&mut app);
        for _ in 0..6000 {
            match app.model.screen {
                Screen::Fork if app.model.resolved.is_none() => {
                    found = true;
                    let text = screen_text(&app.model, (80, 24));
                    assert!(flat(&text).contains("The trail forks."));
                    for choice in &app.model.view.choices {
                        assert!(flat(&text).contains(&flat(&choice.text)), "{text}");
                    }
                    assert!(flat(&text).contains("miles"));
                    app.press_char('z');
                    assert!(app
                        .model
                        .notice
                        .as_deref()
                        .unwrap()
                        .starts_with("Choose one of: A"));
                    let miles = app.engine.state.distance_traveled;
                    app.press_char('1');
                    assert_ne!(app.engine.phase, GamePhase::Route);
                    assert!(app.engine.state.distance_traveled >= miles);
                    let after = screen_text(&app.model, (80, 24));
                    assert!(flat(&after).contains("The trail forks."));
                    assert!(flat(&after).contains("Enter Continue"));
                    break;
                }
                Screen::GameOver => break,
                _ if app.model.resolved.is_some() => enter(&mut app),
                Screen::Event => app.press_char('a'),
                _ => app.press_char('t'),
            }
        }
        if found {
            break;
        }
    }
    assert!(found, "no seed reached a fork");
}

// ----------------------------------------------------------------------------------------
// 5. Settings
// ----------------------------------------------------------------------------------------

#[test]
fn settings_changed_by_keys_are_saved_exactly_as_line_mode_saves_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app_in(42, (80, 24), dir.path(), true);
    enter(&mut app);
    app.press_char('o');
    assert_eq!(app.model.screen, Screen::Settings);
    app.press_char('1'); // narrator off
    app.press_char('2'); // fireside -> lantern
    app.press_char('2'); // lantern -> chronicler
    app.press_char('3'); // standard -> rich
    app.press_char('3'); // rich -> brief
    app.press_char('4'); // the model name comes next
    assert!(app.model.settings_entry.is_some());
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("Model name"));
    // Typing 1, 2, 3, 4, s or q now is text, not a command.
    for c in "x1s".chars() {
        app.press_char(c);
    }
    app.press_code(KeyCode::Backspace);
    app.press_code(KeyCode::Backspace);
    app.press_code(KeyCode::Backspace);
    for c in "qwen3:4b".chars() {
        app.press_char(c);
    }
    enter(&mut app);
    assert!(app.model.settings_entry.is_none());
    app.press_char('s');
    assert_eq!(app.model.screen, Screen::Camp);
    assert_eq!(app.model.notice.as_deref(), Some("Settings saved."));

    let path = dir.path().join(SAVE_DIR).join("settings.json");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(saved, EXPECTED_SETTINGS_JSON);
    // The settings file is not the save.
    assert!(!dir.path().join(SAVE_DIR).join(SAVE_FILE).exists());

    // Line mode, given the same choices, writes the same bytes.
    let mut out = Vec::new();
    let exit = run_settings_screen(
        NarratorSettings::default(),
        Cursor::new("1\n2\n2\n3\n3\n4\nqwen3:4b\ns\n"),
        &mut out,
    )
    .unwrap();
    let SettingsExit::Save(line_settings) = exit else {
        panic!("expected Save");
    };
    let line_dir = tempfile::tempdir().unwrap();
    let line_path = save_settings(&line_settings, line_dir.path()).unwrap();
    assert_eq!(std::fs::read_to_string(line_path).unwrap(), saved);
}

#[test]
fn settings_can_be_left_without_saving_and_a_bad_model_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app_in(42, (80, 24), dir.path(), true);
    enter(&mut app);
    app.press_char('o');
    app.press_char('1');
    assert!(!app.model.settings.settings.enabled);
    app.press_char('4');
    for c in "bad name".chars() {
        app.press_char(c);
    }
    enter(&mut app);
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("Model not changed"));
    app.press_char('4');
    app.press_code(KeyCode::Esc); // keeps the current one
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("Model is still llama3.2."));
    app.press_char('q');
    assert_eq!(app.model.screen, Screen::Camp);
    assert_eq!(app.model.notice.as_deref(), Some("Settings not saved."));
    assert!(!dir.path().join(SAVE_DIR).join("settings.json").exists());
    // Opening it again starts from what was saved, not from the discarded edit.
    app.press_char('o');
    assert!(app.model.settings.settings.enabled);
    // Esc leaves too.
    app.press_code(KeyCode::Esc);
    assert_eq!(app.model.screen, Screen::Camp);

    // A write that fails is said, and the screen stays.
    let mut model = app.model.clone();
    model.screen = Screen::Settings;
    update(&mut model, Msg::SettingsSaved(Err("disk full".to_string())));
    assert_eq!(model.screen, Screen::Settings);
    assert!(flat(&screen_text(&model, (80, 24)))
        .contains("Could not save the settings file: disk full"));

    // From the title screen settings return to the title.
    let mut app = new_app_in(42, (80, 24), dir.path(), true);
    app.press_char('o');
    assert_eq!(app.model.screen, Screen::Settings);
    app.press_char('q');
    assert_eq!(app.model.screen, Screen::Title);
}

// ----------------------------------------------------------------------------------------
// 6. Quit needs confirmation
// ----------------------------------------------------------------------------------------

#[test]
fn quit_needs_a_yes() {
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    app.press_char('q');
    assert_eq!(app.model.overlay, Overlay::QuitConfirm);
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("Y Yes N No"));
    app.press_char('n');
    assert_eq!(app.model.overlay, Overlay::None);
    assert!(!app.quit);
    // Esc answers no. Other keys wait.
    app.press_char('q');
    app.press_char('t');
    assert_eq!(app.model.overlay, Overlay::QuitConfirm);
    assert_eq!(app.engine.rng.counter, new_engine(42).rng.counter);
    app.press_code(KeyCode::Esc);
    assert!(!app.quit);
    app.press_char('q');
    app.press_char('y');
    assert!(app.quit);

    // Ctrl-C quits at once, from anywhere; Ctrl with another key does nothing.
    let mut app = new_app(42, (80, 24));
    app.press(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert!(!app.quit);
    app.press(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.quit);
}

#[test]
fn quit_works_on_every_screen_that_has_a_key_for_it() {
    // Title.
    let mut app = new_app(42, (80, 24));
    app.press_char('q');
    app.press_char('y');
    assert!(app.quit);
    // Event.
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    travel_to_event(&mut app);
    app.press_char('q');
    assert_eq!(app.model.overlay, Overlay::QuitConfirm);
    app.press_char('y');
    assert!(app.quit);
    // Game over.
    let mut app = new_app(42, (80, 24));
    play_to_game_over(&mut app);
    app.press_char('q');
    app.press_char('y');
    assert!(app.quit);
}

// ----------------------------------------------------------------------------------------
// 7. The terminal is always given back; not a terminal
// ----------------------------------------------------------------------------------------

#[test]
fn the_terminal_is_restored_on_drop_and_on_panic() {
    // Drop.
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    let guard = TerminalGuard::with_restore(move || {
        c.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(guard);
    assert_eq!(count.load(Ordering::SeqCst), 1);

    // A guard dropped by an unwinding panic restores too.
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    let result = std::panic::catch_unwind(move || {
        let _guard = TerminalGuard::with_restore(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        panic!("boom in the guard test");
    });
    assert!(result.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);

    // The panic hook restores before the default hook prints, with no guard at all.
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    let saved = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    install_panic_hook(move || {
        c.fetch_add(1, Ordering::SeqCst);
    });
    let result = std::panic::catch_unwind(|| {
        panic!("boom in the hook test");
    });
    let _ = std::panic::take_hook();
    std::panic::set_hook(saved);
    assert!(result.is_err());
    assert!(
        count.load(Ordering::SeqCst) >= 1,
        "the restore action did not run on panic"
    );
}

fn run_etv(
    dir: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_etv"))
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run etv")
        .wait_with_output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
    )
}

#[test]
fn etv_tui_without_a_terminal_says_so_in_one_line_and_exits_1() {
    for args in [
        vec!["tui"],
        vec!["tui", "--seed", "42"],
        vec!["tui", "--continue"],
        vec!["tui", "--force"],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (code, stdout, stderr) = run_etv(dir.path(), &args, &[]);
        assert_eq!(code, Some(1), "{args:?}");
        assert_eq!(stdout, "", "{args:?}");
        assert_eq!(
            stderr,
            "error: etv tui needs a terminal. Use 'etv new' or 'etv continue' to play here.\n",
            "{args:?}"
        );
        // It refused before touching any save.
        assert!(!dir.path().join(SAVE_DIR).exists(), "{args:?}");
    }
}

// ----------------------------------------------------------------------------------------
// 8. Colour
// ----------------------------------------------------------------------------------------

fn coloured_cells(buffer: &Buffer) -> usize {
    let area = buffer.area;
    let mut n = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buffer[(x, y)];
            if cell.fg != Color::Reset || cell.bg != Color::Reset {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn no_color_draws_no_coloured_cell_on_any_screen() {
    let lookup = |set: Option<&'static str>| {
        move |k: &str| (k == "NO_COLOR").then(|| set.map(str::to_string)).flatten()
    };
    assert!(no_color_requested(lookup(Some("1"))));
    assert!(!no_color_requested(lookup(Some(""))));
    assert!(!no_color_requested(lookup(None)));

    let dir = tempfile::tempdir().unwrap();
    for size in SIZES {
        let mut plain = new_app_in(42, size, dir.path(), !no_color_requested(lookup(Some("1"))));
        let mut colour = new_app_in(42, size, dir.path(), true);
        assert!(!plain.model.color);
        let check = |plain: &mut App<'_>, colour: &mut App<'_>, what: &str| {
            let p = draw(&plain.model, size);
            let c = draw(&colour.model, size);
            assert_eq!(
                coloured_cells(&p),
                0,
                "{what} at {size:?} has colour with NO_COLOR"
            );
            assert!(
                coloured_cells(&c) > 0,
                "{what} at {size:?}: colour is never on"
            );
            // Same characters either way: colour is never the only mark.
            assert_eq!(text_of(&p), text_of(&c), "{what} at {size:?}");
        };
        check(&mut plain, &mut colour, "title");
        for app in [&mut plain, &mut colour] {
            enter(app);
        }
        check(&mut plain, &mut colour, "camp");
        for app in [&mut plain, &mut colour] {
            app.press_char('p');
        }
        check(&mut plain, &mut colour, "pace picker");
        for app in [&mut plain, &mut colour] {
            app.press_code(KeyCode::Esc);
            app.press_char('?');
        }
        check(&mut plain, &mut colour, "help");
        for app in [&mut plain, &mut colour] {
            app.press_code(KeyCode::Esc);
            app.press_char('o');
        }
        check(&mut plain, &mut colour, "settings");
        for app in [&mut plain, &mut colour] {
            app.press_char('q');
            travel_to_event(app);
        }
        check(&mut plain, &mut colour, "event");
        for app in [&mut plain, &mut colour] {
            app.press_char('a');
        }
        check(&mut plain, &mut colour, "outcome");
    }
    let mut plain = new_app_in(42, (80, 24), dir.path(), false);
    play_to_game_over(&mut plain);
    assert_eq!(coloured_cells(&draw(&plain.model, (80, 24))), 0);
}

// ----------------------------------------------------------------------------------------
// The rest of the screens and keys
// ----------------------------------------------------------------------------------------

#[test]
fn the_selected_pace_is_marked_in_text_and_chosen_by_letter_or_cursor() {
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    app.press_char('p');
    assert_eq!(app.model.overlay, Overlay::Pace);
    let text = screen_text(&app.model, (80, 24));
    // The cursor starts on the current pace, and the marker is a character, not just colour.
    assert!(text.contains("> T steady (now)"), "{text}");
    assert!(text.contains("  S slow"));
    assert!(text.contains("  H hard"));
    app.press_code(KeyCode::Down);
    assert!(screen_text(&app.model, (80, 24)).contains("> H hard"));
    app.press_code(KeyCode::Up);
    app.press_code(KeyCode::Up);
    assert!(screen_text(&app.model, (80, 24)).contains("> S slow"));
    enter(&mut app);
    assert_eq!(app.model.overlay, Overlay::None);
    assert_eq!(app.engine.state.wagon.pace.as_str(), "slow");
    // By letter, and Esc cancels.
    app.press_char('p');
    app.press_char('h');
    assert_eq!(app.engine.state.wagon.pace.as_str(), "hard");
    app.press_char('p');
    app.press_code(KeyCode::Esc);
    assert_eq!(app.model.overlay, Overlay::None);
    assert_eq!(app.engine.state.wagon.pace.as_str(), "hard");
}

#[test]
fn camp_keys_play_the_turns_line_mode_plays_and_unknown_keys_get_the_line_mode_hint() {
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    app.press_char('z');
    assert_eq!(app.model.notice.as_deref(), Some(etv_core::play::CAMP_HINT));
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("Type T, R, H, F"));
    assert_eq!(app.engine.rng.counter, new_engine(42).rng.counter);
    // Upper case works too; any key clears the hint.
    app.press_char('R');
    assert_eq!(app.model.notice, None);
    assert!(app.engine.state.day >= 1);
    let mut line_engine = new_engine(42);
    let mut out = Vec::new();
    run_play(
        &mut line_engine,
        &NullNarrator,
        Some(table()),
        "TEST",
        Cursor::new("z\nR\n"),
        &mut out,
    )
    .unwrap();
    assert_eq!(comparable(&app.engine), comparable(&line_engine));
    // The log shows what happened, and the lines are the ones line mode printed.
    let printed = String::from_utf8(out).unwrap();
    for line in app.model.log.iter().filter(|l| !l.is_empty()) {
        assert!(
            printed.contains(line.as_str()),
            "{line:?} not in line mode output"
        );
    }
    assert!(!app.model.log.is_empty());
}

#[test]
fn the_last_resorts_have_keys() {
    for (key, action) in [
        ('x', etv_core::model::IntentAction::AbandonCargo),
        ('d', etv_core::model::IntentAction::DesperateRepair),
        ('n', etv_core::model::IntentAction::HardRation),
        ('f', etv_core::model::IntentAction::Repair),
        ('h', etv_core::model::IntentAction::Hunt),
    ] {
        let mut model = new_app(42, (80, 24)).model;
        update(&mut model, Msg::Intent(Intent::Confirm));
        let msg = key_to_msg(
            &model,
            KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
        )
        .unwrap();
        let effects = update(&mut model, msg);
        match &effects[..] {
            [Effect::Turn { intent, .. }] => assert_eq!(intent.action, action, "key {key}"),
            other => panic!("key {key}: {other:?}"),
        }
    }
}

#[test]
fn an_outcome_stays_until_a_key_moves_on_and_a_camp_key_both_dismisses_and_acts() {
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    travel_to_event(&mut app);
    app.press_char('a');
    assert!(app.model.resolved.is_some());
    // A key that means nothing leaves it up; help opens over it.
    app.press_char('z');
    assert!(app.model.resolved.is_some());
    app.press_char('?');
    assert!(app.model.resolved.is_some());
    app.press_code(KeyCode::Esc);
    // Enter dismisses without acting.
    let counter = app.engine.rng.counter;
    enter(&mut app);
    assert!(app.model.resolved.is_none());
    assert_eq!(app.engine.rng.counter, counter);
    assert_eq!(app.model.screen, Screen::Camp);
    // The outcome stays in the log.
    let log = app.model.log.join("\n");
    assert!(log.contains("[Continue despite the wet] The party damages the wagon"));
    assert!(log.contains("Distance -4, Morale -2, Days +1, Wagon condition -2"));

    // Another event, and this time a camp key dismisses and travels.
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    travel_to_event(&mut app);
    app.press_char('a');
    let counter = app.engine.rng.counter;
    app.press_char('t');
    assert!(app.model.resolved.is_none() || app.model.screen != Screen::Camp);
    assert!(app.engine.rng.counter > counter);
}

#[test]
fn the_log_and_the_ledger_scroll_and_never_past_their_ends() {
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    for _ in 0..30 {
        app.press_char('t');
        if app.model.resolved.is_some() {
            enter(&mut app);
        }
        if app.model.screen != Screen::Camp {
            app.press_char('a');
            enter(&mut app);
        }
    }
    assert!(app.model.log.len() > 20);
    let (total, visible) = etv_core::tui::layout::log_metrics(&app.model);
    assert!(total > visible);
    let max = total - visible;
    assert_eq!(app.model.log_scroll, 0);
    app.press_code(KeyCode::Up);
    assert_eq!(app.model.log_scroll, 1);
    app.press_code(KeyCode::PageUp);
    assert_eq!(app.model.log_scroll, 1 + visible - 1);
    app.press_code(KeyCode::Home);
    assert_eq!(app.model.log_scroll, max);
    app.press_code(KeyCode::Up);
    assert_eq!(app.model.log_scroll, max);
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("Log (scrolled)"));
    app.press_code(KeyCode::PageDown);
    app.press_code(KeyCode::Down);
    assert!(app.model.log_scroll < max);
    app.press_code(KeyCode::End);
    assert_eq!(app.model.log_scroll, 0);
    // A resize re-clamps.
    app.press_code(KeyCode::Home);
    app.dispatch(Msg::Resize(120, 40), &mut |_| {});
    let (total, visible) = etv_core::tui::layout::log_metrics(&app.model);
    assert!(app.model.log_scroll <= total.saturating_sub(visible));

    // The ledger.
    let mut app = new_app(42, (80, 24));
    play_to_game_over(&mut app);
    let (total, visible) = etv_core::tui::layout::ledger_metrics(&app.model);
    assert!(
        total > visible,
        "the ledger fits the screen; nothing to scroll"
    );
    let first = etv_core::trail_ledger(&app.engine.state)[0].clone();
    assert_eq!(app.model.ledger_scroll, 0);
    app.press_code(KeyCode::Up);
    assert_eq!(app.model.ledger_scroll, 0);
    app.press_code(KeyCode::Down);
    assert_eq!(app.model.ledger_scroll, 1);
    app.press_code(KeyCode::PageDown);
    app.press_code(KeyCode::End);
    assert_eq!(app.model.ledger_scroll, total - visible);
    app.press_code(KeyCode::Down);
    assert_eq!(app.model.ledger_scroll, total - visible);
    let bottom = screen_text(&app.model, (80, 24));
    assert!(!flat(&bottom).contains(&flat(&first)) || visible >= total);
    app.press_code(KeyCode::PageUp);
    app.press_code(KeyCode::Home);
    assert_eq!(app.model.ledger_scroll, 0);
    assert!(flat(&screen_text(&app.model, (80, 24))).contains(&flat(&first)));
}

#[test]
fn the_narrator_status_line_and_summary_are_drawn() {
    let mut app = new_app(42, (80, 24));
    enter(&mut app);
    app.model.busy = true;
    assert!(screen_text(&app.model, (80, 24)).contains(BUSY_LINE));
    app.model.busy = false;
    assert!(!screen_text(&app.model, (80, 24)).contains(BUSY_LINE));

    // A live narrator is drawn busy before the turn, via the hook the driver offers.
    let engine = new_engine(42);
    let model = Model::new(
        ViewData::from_engine(&engine),
        NarratorSettings::default(),
        "TEST",
        "HASH",
        vec!["Narrator: Fireside, standard, model llama3.2.".to_string()],
        true,
    );
    let mut live = App::new(
        model,
        engine,
        &NullNarrator,
        Some(table()),
        ".".into(),
        true,
    )
    .with_summary(|| "Narrator: 3 lines asked for.".to_string());
    enter(&mut live);
    assert!(flat(&screen_text(&live.model, (80, 24))).contains("Narrator: Fireside, standard"));
    let mut seen_busy = Vec::new();
    let msg = key_to_msg(
        &live.model,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
    )
    .unwrap();
    live.dispatch(msg, &mut |m| {
        seen_busy.push((m.busy, screen_text(m, (80, 24)).contains(BUSY_LINE)));
    });
    assert_eq!(seen_busy, vec![(true, true)]);
    assert!(!live.model.busy);
    assert_eq!(
        live.model.narrator_summary.as_deref(),
        Some("Narrator: 3 lines asked for.")
    );
    // The summary is on the game-over screen.
    for _ in 0..20_000 {
        if live.model.screen == Screen::GameOver && live.model.resolved.is_none() {
            break;
        }
        if live.model.resolved.is_some() {
            enter(&mut live);
        } else {
            live.press_char('t');
            live.press_char('a');
        }
    }
    assert!(flat(&screen_text(&live.model, (80, 24))).contains("Narrator: 3 lines asked for."));
}

#[test]
fn the_title_about_and_help_screens_open_and_close() {
    let mut app = new_app(42, (80, 24));
    assert_eq!(app.model.screen, Screen::Title);
    app.press_char('a');
    assert_eq!(app.model.screen, Screen::About);
    let text = flat(&screen_text(&app.model, (80, 24)));
    assert!(text.contains("version TEST"));
    assert!(text.contains("content HASH"));
    assert!(text.contains("MIT licence. Third-party licences: etv --notices"));
    enter(&mut app);
    assert_eq!(app.model.screen, Screen::Title);
    // Help closes with ?, Esc or Enter, and takes the other keys while open.
    for closer in [KeyCode::Char('?'), KeyCode::Esc, KeyCode::Enter] {
        app.press_char('?');
        assert_eq!(app.model.overlay, Overlay::Help);
        app.press_char('t');
        assert_eq!(app.model.overlay, Overlay::Help);
        app.press_code(closer);
        assert_eq!(app.model.overlay, Overlay::None);
    }
    // Space starts too; Alt and bare keys without a meaning do nothing.
    app.press(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    assert_eq!(app.model.screen, Screen::Title);
    app.press_code(KeyCode::Char(' '));
    assert_eq!(app.model.screen, Screen::Camp);
}

#[test]
fn a_run_loaded_mid_game_opens_on_the_screen_of_its_phase() {
    // The model follows the engine's phase, whatever it is when the run is loaded.
    let mut engine = new_engine(42);
    for _ in 0..60 {
        if engine.phase == GamePhase::Event {
            break;
        }
        engine.step(&etv_core::model::PlayerIntent::new(
            etv_core::model::IntentAction::Travel,
        ));
    }
    assert_eq!(engine.phase, GamePhase::Event);
    let model = Model::new(
        ViewData::from_engine(&engine),
        NarratorSettings::default(),
        "TEST",
        "HASH",
        Vec::new(),
        true,
    );
    let mut app = App::new(
        model,
        engine,
        &NullNarrator,
        Some(table()),
        ".".into(),
        false,
    );
    enter(&mut app);
    assert_eq!(app.model.screen, Screen::Event);
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("Persistent Rain"));
}

#[test]
fn cli_parses_tui() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        parse_cli_args(&args(&["tui"])).unwrap().command,
        CliCommand::Tui {
            seed: None,
            force: false,
            resume: false
        }
    );
    assert_eq!(
        parse_cli_args(&args(&["tui", "--seed", "42", "--force"]))
            .unwrap()
            .command,
        CliCommand::Tui {
            seed: Some(42),
            force: true,
            resume: false
        }
    );
    assert_eq!(
        parse_cli_args(&args(&["tui", "--continue"]))
            .unwrap()
            .command,
        CliCommand::Tui {
            seed: None,
            force: false,
            resume: true
        }
    );
    for conflict in [
        vec!["tui", "--continue", "--seed", "1"],
        vec!["tui", "--continue", "--force"],
    ] {
        assert_eq!(
            parse_cli_args(&args(&conflict)).unwrap_err(),
            "'--continue' cannot be used with '--seed' or '--force'"
        );
    }
    // Line mode's commands are unchanged.
    assert_eq!(
        parse_cli_args(&args(&["continue"])).unwrap().command,
        CliCommand::Continue
    );
}

#[test]
fn wrapping_keeps_words_whole_and_hangs_choice_rows() {
    use etv_core::tui::layout::wrap;
    let lines = vec![
        "  A. Wait it out (A day; Time cost, animal recovers)".to_string(),
        String::new(),
        "a-very-long-word-that-cannot-fit-in-ten".to_string(),
    ];
    let wrapped = wrap(&lines, 20);
    assert_eq!(wrapped[0], "  A. Wait it out (A");
    assert!(wrapped[1].starts_with("     day;"), "{wrapped:?}");
    assert!(
        wrapped.iter().all(|l| l.chars().count() <= 20),
        "{wrapped:?}"
    );
    assert!(wrapped.contains(&String::new()));
    let long: Vec<&String> = wrapped
        .iter()
        .skip_while(|l| !l.is_empty())
        .skip(1)
        .collect();
    assert_eq!(long.len(), 2);
    assert_eq!(wrap(&["short".to_string()], 20), vec!["short".to_string()]);
}

/// A narrator that counts its calls and always answers `line`.
struct CountingNarrator {
    calls: Arc<AtomicUsize>,
    line: &'static str,
}

impl etv_core::narrator::Narrator for CountingNarrator {
    fn narrate(&self, _ctx: &etv_core::narrator::NarrationContext) -> Option<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Some(self.line.to_string())
    }
}

/// A rebuilt narrator that counts its own calls and the calls of every rebuilt narrator.
struct RebuiltNarrator {
    total: Arc<AtomicUsize>,
    own: Arc<AtomicUsize>,
}

impl etv_core::narrator::Narrator for RebuiltNarrator {
    fn narrate(&self, _ctx: &etv_core::narrator::NarrationContext) -> Option<String> {
        self.total.fetch_add(1, Ordering::SeqCst);
        self.own.fetch_add(1, Ordering::SeqCst);
        Some("The rebuilt narrator speaks.".to_string())
    }
}

#[test]
fn settings_saved_in_the_interface_change_the_next_turn_without_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let first_calls = Arc::new(AtomicUsize::new(0));
    let rebuilt_calls = Arc::new(AtomicUsize::new(0));
    let first = CountingNarrator {
        calls: Arc::clone(&first_calls),
        line: "The first narrator speaks.",
    };
    let engine = new_engine(42);
    let model = Model::new(
        ViewData::from_engine(&engine),
        NarratorSettings::default(),
        "TEST",
        "HASH",
        Vec::new(),
        true,
    );
    let reconnects = Arc::new(AtomicUsize::new(0));
    let last_reconnect: Arc<std::sync::Mutex<Option<NarratorSettings>>> =
        Arc::new(std::sync::Mutex::new(None));
    let (rebuilt_for_hook, reconnects_for_hook) =
        (Arc::clone(&rebuilt_calls), Arc::clone(&reconnects));
    let last_for_hook = Arc::clone(&last_reconnect);
    // One counter per narrator the hook builds, in the order built.
    let built: Arc<std::sync::Mutex<Vec<Arc<AtomicUsize>>>> = Arc::default();
    let built_for_hook = Arc::clone(&built);
    let mut app = App::new(
        model,
        engine,
        &first,
        Some(table()),
        dir.path().into(),
        true,
    )
    .with_summary(|| "Narrator: the first one.".to_string())
    .with_reconnect(move |settings: &NarratorSettings| {
        reconnects_for_hook.fetch_add(1, Ordering::SeqCst);
        *last_for_hook.lock().unwrap() = Some(settings.clone());
        if settings.enabled {
            let own = Arc::new(AtomicUsize::new(0));
            built_for_hook.lock().unwrap().push(Arc::clone(&own));
            etv_core::tui::Reconnected {
                narrator: std::rc::Rc::new(RebuiltNarrator {
                    total: Arc::clone(&rebuilt_for_hook),
                    own,
                }),
                live: true,
                lines: vec!["Narrator: rebuilt.".to_string()],
                summary: Some(Box::new(|| "Narrator: the rebuilt one.".to_string())),
            }
        } else {
            etv_core::tui::Reconnected {
                narrator: std::rc::Rc::new(NullNarrator),
                live: false,
                lines: vec!["The narrator is switched off in the settings.".to_string()],
                summary: None,
            }
        }
    });
    enter(&mut app);
    assert_eq!(app.model.screen, Screen::Camp);

    // Play exactly one turn, whatever screen the run is on (travel at camp, the first option on
    // an event or a fork), and check it was played.
    let play_one_turn = |app: &mut App<'_>| {
        let before = format!("{:?}", app.engine.state);
        let key = if app.model.screen == Screen::Camp {
            't'
        } else {
            'a'
        };
        app.press_char(key);
        while app.model.resolved.is_some() {
            enter(app);
        }
        assert!(
            format!("{:?}", app.engine.state) != before,
            "no turn was played on {:?}",
            app.model.screen
        );
    };
    play_one_turn(&mut app);
    let after_first = first_calls.load(Ordering::SeqCst);
    assert!(
        after_first > 0,
        "the first narrator is asked on the first turn"
    );

    // Switch the narrator off and save: the next turn asks no narrator at all.
    app.press_char('o');
    app.press_char('1');
    app.press_char('s');
    assert_eq!(app.model.notice.as_deref(), Some("Settings saved."));
    assert_eq!(reconnects.load(Ordering::SeqCst), 1);
    assert!(app.model.narrator_summary.is_none());
    assert!(flat(&screen_text(&app.model, (80, 24))).contains("switched off"));
    play_one_turn(&mut app);
    assert_eq!(first_calls.load(Ordering::SeqCst), after_first);
    assert_eq!(rebuilt_calls.load(Ordering::SeqCst), 0);
    assert!(app.narrator_summary().is_none());

    // Saving the same settings again does not reconnect.
    app.press_char('o');
    app.press_char('s');
    assert_eq!(reconnects.load(Ordering::SeqCst), 1);

    // Switch it back on: the next turn is asked of the rebuilt narrator.
    app.press_char('o');
    app.press_char('1');
    app.press_char('s');
    assert_eq!(reconnects.load(Ordering::SeqCst), 2);
    play_one_turn(&mut app);
    assert_eq!(first_calls.load(Ordering::SeqCst), after_first);
    assert!(rebuilt_calls.load(Ordering::SeqCst) > 0);
    assert_eq!(
        app.narrator_summary().as_deref(),
        Some("Narrator: the rebuilt one.")
    );
    assert_eq!(
        app.model.narrator_summary.as_deref(),
        Some("Narrator: the rebuilt one.")
    );

    // The profile, the length and the model each reconnect too, with the edited value, and the
    // next turn is asked of the narrator built from it.
    let edits: [(&str, &[char]); 3] = [
        ("profile", &['2']),
        ("length", &['3']),
        (
            "model",
            &['4', 't', 'e', 's', 't', '-', 'm', 'o', 'd', 'e', 'l'],
        ),
    ];
    for (n, (field, keys)) in edits.iter().enumerate() {
        let before = app.model.saved_settings.clone();
        app.press_char('o');
        for &k in *keys {
            app.press_char(k);
        }
        if *field == "model" {
            enter(&mut app);
        }
        app.press_char('s');
        assert_eq!(
            app.model.notice.as_deref(),
            Some("Settings saved."),
            "{field}"
        );
        assert_eq!(reconnects.load(Ordering::SeqCst), 3 + n, "{field}");
        let used = last_reconnect.lock().unwrap().clone().unwrap();
        assert_eq!(used, app.model.saved_settings, "{field}");
        match *field {
            "profile" => assert_ne!(used.profile, before.profile),
            "length" => assert_ne!(used.length, before.length),
            _ => assert_eq!(used.model, "test-model"),
        }
        let asked = rebuilt_calls.load(Ordering::SeqCst);
        let counts_before: Vec<usize> = built
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.load(Ordering::SeqCst))
            .collect();
        play_one_turn(&mut app);
        assert!(rebuilt_calls.load(Ordering::SeqCst) > asked, "{field}");
        assert_eq!(first_calls.load(Ordering::SeqCst), after_first);
        // The turn was asked of the narrator this edit built, and of no earlier one.
        let counts_after: Vec<usize> = built
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.load(Ordering::SeqCst))
            .collect();
        let newest = counts_after.len() - 1;
        assert_eq!(
            counts_before[newest], 0,
            "{field}: the new narrator is fresh"
        );
        assert!(
            counts_after[newest] > 0,
            "{field}: the new narrator was asked"
        );
        assert_eq!(
            counts_after[..newest],
            counts_before[..newest],
            "{field}: an earlier narrator was asked"
        );
    }
    // Enabled twice (the first switch back on, then the three edits): four narrators built.
    assert_eq!(built.lock().unwrap().len(), 4);
}
