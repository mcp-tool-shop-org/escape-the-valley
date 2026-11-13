//! Phase 3: the player settings file, the settings screen, and the AI disclosure screens.
//!
//! Expected screen text is written out here as literals, so a wording change has to be made in
//! two places on purpose.

use std::io::Cursor;
use std::process::{Command, Stdio};

use etv_core::content::{embedded_content_pack, parse_cli_args, CliCommand};
use etv_core::model::GMProfile;
use etv_core::narrator::{NarratorLength, NarratorSettings};
use etv_core::screens::{
    about_screen_lines, run_settings_screen, settings_screen_lines, title_screen_lines,
    SettingsEvent, SettingsExit, SettingsScreen, CREDITS_DISCLOSURE, TITLE_DISCLOSURE,
};
use etv_core::settings::{
    load_settings_or_default, save_settings, settings_path, settings_to_json, validate_model_name,
};

const APPROVED_TITLE: &str = "When a narrator model is running, the story is written and voiced by AI, live, as you play, and every telling is new. Without one, the game shows plain text lines instead.";
const APPROVED_CREDITS: &str = "Narration written and voiced live by AI (local models you choose).";

#[test]
fn disclosure_wording_is_the_approved_wording() {
    assert_eq!(TITLE_DISCLOSURE, APPROVED_TITLE);
    assert_eq!(CREDITS_DISCLOSURE, APPROVED_CREDITS);
}

#[test]
fn title_screen_shows_the_disclosure() {
    assert_eq!(
        title_screen_lines("9.9.9"),
        vec![
            "ESCAPE THE VALLEY".to_string(),
            "version 9.9.9".to_string(),
            String::new(),
            APPROVED_TITLE.to_string(),
        ]
    );
}

#[test]
fn about_screen_shows_the_credits_disclosure_and_profile_names() {
    assert_eq!(
        about_screen_lines("9.9.9", "abc123"),
        vec![
            "ESCAPE THE VALLEY".to_string(),
            "version 9.9.9".to_string(),
            "content abc123".to_string(),
            String::new(),
            APPROVED_CREDITS.to_string(),
            "Narrator profiles: Chronicler, Fireside, Lantern-Bearer.".to_string(),
            String::new(),
            "MIT licence. Third-party licences: etv --notices".to_string(),
        ]
    );
}

#[test]
fn readme_carries_the_credits_disclosure() {
    let readme = include_str!("../../../README.md");
    assert!(readme.contains(APPROVED_CREDITS));
}

#[test]
fn settings_screen_renders_the_defaults() {
    assert_eq!(
        settings_screen_lines(&NarratorSettings::default()),
        vec![
            "SETTINGS".to_string(),
            "  1. Narrator  on".to_string(),
            "  2. Profile   Fireside - Serious campfire narrator. Subtle uncanny moments. The default."
                .to_string(),
            "  3. Length    Standard (1-2 sentences)".to_string(),
            "  4. Model     llama3.2".to_string(),
            "  S. Save and leave    Q. Leave without saving".to_string(),
            String::new(),
            APPROVED_TITLE.to_string(),
        ]
    );
}

#[test]
fn settings_screen_renders_every_profile_and_length() {
    let mut s = NarratorSettings {
        enabled: false,
        profile: GMProfile::Chronicler,
        length: NarratorLength::Brief,
        ..NarratorSettings::default()
    };
    let lines = settings_screen_lines(&s);
    assert_eq!(lines[1], "  1. Narrator  off");
    assert_eq!(
        lines[2],
        "  2. Profile   Chronicler - Grounded, practical, spare. Minimal folklore. Reports what happened."
    );
    assert_eq!(lines[3], "  3. Length    Brief (1 sentence)");

    s.profile = GMProfile::Lantern;
    s.length = NarratorLength::Rich;
    let lines = settings_screen_lines(&s);
    assert_eq!(
        lines[2],
        "  2. Profile   Lantern-Bearer - Uncanny and liminal, but still grounded in consequences. The weird one."
    );
    assert_eq!(lines[3], "  3. Length    Rich (2-3 sentences)");
}

fn changed(msg: &str) -> SettingsEvent {
    SettingsEvent::Changed(msg.to_string())
}

#[test]
fn number_keys_cycle_each_setting_in_order_and_wrap() {
    let mut screen = SettingsScreen::new(NarratorSettings::default());

    assert_eq!(screen.handle_line("1\n"), changed("Narrator is now off."));
    assert!(!screen.settings.enabled);
    assert_eq!(screen.handle_line("1"), changed("Narrator is now on."));
    assert!(screen.settings.enabled);

    // Fireside -> Lantern-Bearer -> Chronicler -> Fireside.
    assert_eq!(
        screen.handle_line("2"),
        changed("Profile is now Lantern-Bearer.")
    );
    assert_eq!(
        screen.handle_line("2"),
        changed("Profile is now Chronicler.")
    );
    assert_eq!(screen.handle_line("2"), changed("Profile is now Fireside."));
    assert_eq!(screen.settings.profile, GMProfile::Fireside);

    // Standard -> Rich -> Brief -> Standard.
    assert_eq!(screen.handle_line("3"), changed("Length is now Rich."));
    assert_eq!(screen.handle_line("3"), changed("Length is now Brief."));
    assert_eq!(screen.handle_line("3"), changed("Length is now Standard."));
    assert_eq!(screen.settings.length, NarratorLength::Standard);
}

#[test]
fn names_set_a_setting_directly() {
    let mut screen = SettingsScreen::new(NarratorSettings::default());
    assert_eq!(
        screen.handle_line("  Chronicler  "),
        changed("Profile is now Chronicler.")
    );
    assert_eq!(
        screen.handle_line("LANTERN-BEARER"),
        changed("Profile is now Lantern-Bearer.")
    );
    assert_eq!(
        screen.handle_line("lantern"),
        changed("Profile is now Lantern-Bearer.")
    );
    assert_eq!(
        screen.handle_line("fireside"),
        changed("Profile is now Fireside.")
    );
    assert_eq!(screen.handle_line("brief"), changed("Length is now Brief."));
    assert_eq!(screen.handle_line("Rich"), changed("Length is now Rich."));
    assert_eq!(
        screen.handle_line("standard"),
        changed("Length is now Standard.")
    );
    assert_eq!(screen.handle_line("off"), changed("Narrator is now off."));
    assert_eq!(screen.handle_line("off"), changed("Narrator is now off."));
    assert_eq!(screen.handle_line("on"), changed("Narrator is now on."));
}

#[test]
fn model_is_asked_on_the_next_line_and_validated() {
    let mut screen = SettingsScreen::new(NarratorSettings::default());

    assert_eq!(screen.handle_line("4"), SettingsEvent::AskModel);
    assert!(screen.awaiting_model());
    // While waiting, the line is read as a model name, whatever it is.
    assert_eq!(
        screen.handle_line("Qwen3:4B"),
        changed("Model is now Qwen3:4B.")
    );
    assert!(!screen.awaiting_model());
    assert_eq!(screen.settings.model, "Qwen3:4B");
    assert!(screen.settings.enabled);

    // An empty line keeps the current model.
    assert_eq!(screen.handle_line("4"), SettingsEvent::AskModel);
    assert_eq!(
        screen.handle_line("\n"),
        changed("Model is still Qwen3:4B.")
    );
    assert_eq!(screen.settings.model, "Qwen3:4B");

    // A bad name is refused and the model is unchanged.
    assert_eq!(screen.handle_line("4"), SettingsEvent::AskModel);
    assert_eq!(
        screen.handle_line("bad name"),
        SettingsEvent::Rejected(
            "Model not changed: the model name contains ' '; use letters, digits and . _ - : / only."
                .to_string()
        )
    );
    assert!(!screen.awaiting_model());
    assert_eq!(screen.settings.model, "Qwen3:4B");

    // The one-line form keeps the case as typed.
    assert_eq!(
        screen.handle_line("MODEL hf.co/Org/Name-GGUF:Q4_K_M"),
        changed("Model is now hf.co/Org/Name-GGUF:Q4_K_M.")
    );
}

#[test]
fn model_name_rules() {
    assert_eq!(
        validate_model_name("  qwen3:4b \n"),
        Ok("qwen3:4b".to_string())
    );
    assert_eq!(
        validate_model_name("   "),
        Err("the model name is empty".to_string())
    );
    assert_eq!(validate_model_name(&"a".repeat(100)), Ok("a".repeat(100)));
    assert_eq!(
        validate_model_name(&"a".repeat(101)),
        Err("the model name is longer than 100 characters".to_string())
    );
    assert_eq!(
        validate_model_name("a\"b"),
        Err("the model name contains '\"'; use letters, digits and . _ - : / only".to_string())
    );
}

#[test]
fn unknown_input_is_rejected_and_changes_nothing() {
    let mut screen = SettingsScreen::new(NarratorSettings::default());
    for bad in ["", "5", "lamp-bearer", "medium", "model"] {
        let before = screen.settings.clone();
        let event = screen.handle_line(bad);
        if bad == "model" {
            // "model" alone asks for the name on the next line, like "4".
            assert_eq!(event, SettingsEvent::AskModel);
            assert_eq!(screen.handle_line(""), changed("Model is still llama3.2."));
        } else {
            assert_eq!(
                event,
                SettingsEvent::Rejected("Type 1, 2, 3, 4, S or Q.".to_string()),
                "input {bad:?}"
            );
        }
        assert_eq!(screen.settings, before);
    }
    assert_eq!(screen.handle_line("S"), SettingsEvent::Save);
    assert_eq!(screen.handle_line("save"), SettingsEvent::Save);
    assert_eq!(screen.handle_line("q"), SettingsEvent::Discard);
    assert_eq!(screen.handle_line("Quit"), SettingsEvent::Discard);
}

#[test]
fn screen_loop_saves_only_when_asked() {
    // Change three settings, then save.
    let mut out = Vec::new();
    let exit = run_settings_screen(
        NarratorSettings::default(),
        Cursor::new("2\n3\n4\nqwen3:4b\ns\n"),
        &mut out,
    )
    .unwrap();
    let SettingsExit::Save(saved) = exit else {
        panic!("expected Save, got {exit:?}");
    };
    assert_eq!(saved.profile, GMProfile::Lantern);
    assert_eq!(saved.length, NarratorLength::Rich);
    assert_eq!(saved.model, "qwen3:4b");
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("SETTINGS\n  1. Narrator  on\n"));
    assert!(text.contains("Profile is now Lantern-Bearer.\n"));
    assert!(text.contains("Model name (empty keeps the current one):\n> "));
    assert!(text.contains("  4. Model     qwen3:4b\n"));
    assert!(!text.contains('\r'));

    // Quit discards.
    let mut out = Vec::new();
    let exit =
        run_settings_screen(NarratorSettings::default(), Cursor::new("1\nq\n"), &mut out).unwrap();
    assert_eq!(exit, SettingsExit::Discard);

    // The end of input discards, even after changes.
    let mut out = Vec::new();
    let exit =
        run_settings_screen(NarratorSettings::default(), Cursor::new("1\n2\n"), &mut out).unwrap();
    assert_eq!(exit, SettingsExit::Discard);

    // A rejected line prints the hint and does not redraw the whole screen.
    let mut out = Vec::new();
    run_settings_screen(
        NarratorSettings::default(),
        Cursor::new("zzz\nq\n"),
        &mut out,
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.matches("SETTINGS").count(), 1);
    assert!(text.contains("> Type 1, 2, 3, 4, S or Q.\n> "));
}

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

#[test]
fn settings_file_round_trips_and_has_a_fixed_shape() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        settings_path(dir.path()),
        dir.path().join(".trail").join("settings.json")
    );

    // No file: the defaults, no warning.
    let loaded = load_settings_or_default(dir.path());
    assert_eq!(loaded.settings, NarratorSettings::default());
    assert!(!loaded.from_file);
    assert_eq!(loaded.warning, None);

    let chosen = NarratorSettings {
        enabled: false,
        profile: GMProfile::Chronicler,
        model: "qwen3:4b".to_string(),
        length: NarratorLength::Brief,
        ..NarratorSettings::default()
    };
    assert_eq!(settings_to_json(&chosen), EXPECTED_SETTINGS_JSON);
    let path = save_settings(&chosen, dir.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        EXPECTED_SETTINGS_JSON
    );

    let loaded = load_settings_or_default(dir.path());
    assert_eq!(loaded.settings, chosen);
    assert!(loaded.from_file);
    assert_eq!(loaded.warning, None);

    // The settings file is not the save: no run.json appears.
    assert!(!dir.path().join(".trail").join("run.json").exists());
}

#[test]
fn unusable_settings_files_fall_back_to_defaults_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    let cases: [(&str, &str); 3] = [
        (
            "{ not json",
            "The settings file could not be read. Using the default settings.",
        ),
        // A non-loopback endpoint without the explicit opt-in is refused by the loader.
        (
            r#"{"endpoint": "http://example.com:11434"}"#,
            "The settings file could not be read. Using the default settings.",
        ),
        (
            r#"{"model": "two words"}"#,
            "The settings file was ignored (the model name contains ' '; use letters, digits and . _ - : / only). Using the default settings.",
        ),
    ];
    for (body, warning) in cases {
        std::fs::write(&path, body).unwrap();
        let loaded = load_settings_or_default(dir.path());
        assert_eq!(loaded.settings, NarratorSettings::default(), "body {body}");
        assert!(!loaded.from_file);
        assert_eq!(loaded.warning.as_deref(), Some(warning));
        // The unusable file is left as it was.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
    }

    // A partial file fills the rest from the defaults.
    std::fs::write(&path, r#"{"profile": "lantern", "length": "rich"}"#).unwrap();
    let loaded = load_settings_or_default(dir.path());
    assert!(loaded.from_file);
    assert_eq!(loaded.settings.profile, GMProfile::Lantern);
    assert_eq!(loaded.settings.length, NarratorLength::Rich);
    assert_eq!(loaded.settings.model, "llama3.2");
}

#[test]
fn cli_parses_settings_and_about() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        parse_cli_args(&args(&["settings"])).unwrap().command,
        CliCommand::Settings
    );
    assert_eq!(
        parse_cli_args(&args(&["about"])).unwrap().command,
        CliCommand::About
    );
    assert_eq!(
        parse_cli_args(&args(&["credits"])).unwrap().command,
        CliCommand::About
    );
    assert_eq!(
        parse_cli_args(&[]).unwrap_err(),
        "expected command: --version | --notices | settings | about | validate-content | inspect-event <id> | run-seed <seed>"
    );
}

fn run_etv_settings(dir: &std::path::Path, stdin_text: &str) -> (Option<i32>, String) {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_etv"))
        .arg("settings")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run etv settings");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_text.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
    )
}

#[test]
fn etv_settings_binary_saves_discards_and_survives_closed_stdin() {
    // Save.
    let dir = tempfile::tempdir().unwrap();
    let (code, stdout) =
        run_etv_settings(dir.path(), "chronicler\nbrief\noff\nmodel qwen3:4b\ns\n");
    assert_eq!(code, Some(0));
    assert!(stdout.ends_with("> Settings saved.\n"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(settings_path(dir.path())).unwrap(),
        EXPECTED_SETTINGS_JSON
    );

    // The next start shows what was saved.
    let (_, stdout) = run_etv_settings(dir.path(), "q\n");
    assert!(stdout.contains("  1. Narrator  off\n"));
    assert!(stdout.contains("  3. Length    Brief (1 sentence)\n"));
    assert!(stdout.ends_with("> Settings not saved.\n"));

    // Discard and closed stdin write nothing.
    for input in ["2\nq\n", "2\n", ""] {
        let dir = tempfile::tempdir().unwrap();
        let (code, stdout) = run_etv_settings(dir.path(), input);
        assert_eq!(code, Some(0));
        assert!(stdout.ends_with("Settings not saved.\n"), "{stdout}");
        assert!(!dir.path().join(".trail").exists());
    }
}

#[test]
fn etv_about_binary_prints_the_credits_screen() {
    let out = Command::new(env!("CARGO_BIN_EXE_etv"))
        .arg("about")
        .output()
        .expect("run etv about");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    let expected = about_screen_lines(
        env!("CARGO_PKG_VERSION"),
        &embedded_content_pack().content_hash_hex,
    )
    .join("\n")
        + "\n";
    assert_eq!(stdout, expected);
    assert!(stdout.contains(APPROVED_CREDITS));
}
