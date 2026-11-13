use std::io::Write;
use std::process::ExitCode;

use etv_core::content::{
    embedded_content_pack, parse_cli_args, version_with_content_hash, CliCommand, ContentPack,
};
use etv_core::engine::StepEngine;
use etv_core::narrator::{FallbackPhraseTable, NullNarrator};
use etv_core::notices::third_party_notices_text;
use etv_core::play::{run_play, PlayExit};
use etv_core::save::{has_save, load_game_checked_with_content_hash, save_game};
use etv_core::screens::{about_screen_lines, run_settings_screen, SettingsExit};
use etv_core::settings::{load_settings_or_default, save_settings};
use etv_core::snapshot::run_tier2_seed_trace;
use etv_core::validator::{validate_content_with_options, ValidationOptions};
use etv_core::worldgen::create_new_run_with_content;

fn load_selected_content(content_dir: Option<&std::path::Path>) -> Result<ContentPack, String> {
    match content_dir {
        #[cfg(feature = "authoring")]
        Some(dir) => ContentPack::load_from_dir(dir),
        #[cfg(not(feature = "authoring"))]
        Some(_) => Err(
            "flag '--content-dir' is not available in release builds (requires cargo feature 'authoring')"
                .to_string(),
        ),
        None => Ok(embedded_content_pack().clone()),
    }
}

/// A seed for a new run when the player gives none: 31 random bits from the operating system.
fn random_seed() -> i128 {
    let mut bytes = [0u8; 4];
    if getrandom::fill(&mut bytes).is_err() {
        return 1;
    }
    i128::from(u32::from_le_bytes(bytes) >> 1)
}

/// Play a run on stdin and stdout, saving under the current directory after every turn.
///
/// The narrator is not connected in this build: every turn shows the plain text lines, whatever
/// the settings file says.
fn run_play_command(content: &ContentPack, new_run: Option<(Option<i128>, bool)>) -> ExitCode {
    let base = std::path::PathBuf::from(".");
    let state = match new_run {
        Some((seed, force)) => {
            if has_save(&base) && !force {
                eprintln!(
                    "error: a saved run exists here. Resume it with 'etv continue', or replace it with 'etv new --force'."
                );
                return ExitCode::from(1);
            }
            let seed = seed.unwrap_or_else(random_seed);
            let state = create_new_run_with_content(seed, content);
            if let Err(e) = save_game(&state, &base) {
                eprintln!("error: could not write the save file: {e}");
                return ExitCode::from(1);
            }
            println!("New run. Seed {seed}.");
            state
        }
        None => match load_game_checked_with_content_hash(&base, &content.content_hash_hex) {
            Ok(state) => state,
            Err(etv_core::save::SaveLoadError::NoSave) => {
                eprintln!("error: no saved run here. Start one with 'etv new'.");
                return ExitCode::from(1);
            }
            Err(e) => {
                eprintln!("error: the saved run could not be loaded: {e}");
                return ExitCode::from(1);
            }
        },
    };

    let mut engine = StepEngine::with_options(state, true, Some(base));
    engine.event_library = content.all_events.clone();
    let table = FallbackPhraseTable::embedded();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match run_play(
        &mut engine,
        &NullNarrator,
        Some(&table),
        env!("CARGO_PKG_VERSION"),
        std::io::stdin().lock(),
        &mut out,
    ) {
        Ok(PlayExit::Finished | PlayExit::Quit | PlayExit::InputEnded) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: play: {e}");
            ExitCode::from(1)
        }
    }
}

/// Run the settings screen on stdin and stdout, against the settings file under the current
/// directory.
fn run_settings_command() -> ExitCode {
    let base = std::path::PathBuf::from(".");
    let loaded = load_settings_or_default(&base);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if let Some(warning) = &loaded.warning {
        let _ = writeln!(out, "{warning}");
    }
    let exit = match run_settings_screen(loaded.settings, std::io::stdin().lock(), &mut out) {
        Ok(exit) => exit,
        Err(e) => {
            eprintln!("error: settings screen: {e}");
            return ExitCode::from(1);
        }
    };
    match exit {
        SettingsExit::Save(settings) => match save_settings(&settings, &base) {
            Ok(_) => {
                let _ = writeln!(out, "Settings saved.");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: could not save the settings file: {e}");
                ExitCode::from(1)
            }
        },
        SettingsExit::Discard => {
            let _ = writeln!(out, "Settings not saved.");
            ExitCode::SUCCESS
        }
    }
}

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_cli_args(&raw_args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    if opts.command == CliCommand::Notices {
        let text = third_party_notices_text();
        if let Err(e) = std::io::stdout().write_all(text.as_bytes()) {
            eprintln!("error writing notices: {e}");
            return ExitCode::from(1);
        }
        return ExitCode::SUCCESS;
    }

    if opts.command == CliCommand::Settings {
        return run_settings_command();
    }

    let content = match load_selected_content(opts.content_dir.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };

    match opts.command {
        CliCommand::Version => {
            println!("{}", version_with_content_hash(&content));
            ExitCode::SUCCESS
        }
        CliCommand::Notices | CliCommand::Settings => ExitCode::SUCCESS,
        CliCommand::New { seed, force } => run_play_command(&content, Some((seed, force))),
        CliCommand::Continue => run_play_command(&content, None),
        CliCommand::About => {
            for line in about_screen_lines(env!("CARGO_PKG_VERSION"), &content.content_hash_hex) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        CliCommand::ValidateContent { require_outcomes } => {
            let report =
                validate_content_with_options(&content, ValidationOptions { require_outcomes });
            for w in &report.warnings {
                println!("WARNING [{}]: {} (hint: {})", w.code, w.message, w.hint);
            }
            for err in &report.errors {
                eprintln!("ERROR [{}]: {} (hint: {})", err.code, err.message, err.hint);
            }
            println!(
                "validate-content: events={} json_events={} warnings={} errors={} content_hash={}",
                report.event_count,
                report.json_event_count,
                report.warnings.len(),
                report.errors.len(),
                content.content_hash_hex
            );
            if report.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        CliCommand::InspectEvent { event_id } => {
            let Some(ev) = content.all_events.iter().find(|e| e.event_id == event_id) else {
                eprintln!("error: event '{event_id}' not found");
                return ExitCode::from(1);
            };
            println!("event_id: {}", ev.event_id);
            println!("title: {}", ev.title);
            println!("narration: {}", ev.fallback_narration);
            println!("content_hash: {}", content.content_hash_hex);
            for ch in &ev.fallback_choices {
                println!(
                    "choice {}: label={:?} action={} cost_hint={:?} risk_hint={:?} outcome_sentence={:?}",
                    ch.choice_id,
                    ch.label,
                    ch.action,
                    ch.cost_hint,
                    ch.risk_hint,
                    ch.outcome_sentence
                );
            }
            ExitCode::SUCCESS
        }
        CliCommand::RunSeed { seed, turns } => {
            let trace = run_tier2_seed_trace(seed, 0, &content, turns);
            println!(
                "seed={} turns={} content_hash={}",
                trace.seed,
                trace.turns.len(),
                content.content_hash_hex
            );
            for t in &trace.turns {
                if !t.presentation_record.event_title.is_empty()
                    || !t.presentation_record.narration.is_empty()
                {
                    println!(
                        "turn {}: mechanics_hash={} title={:?} narration={:?}",
                        t.turn,
                        t.mechanics_state_hash,
                        t.presentation_record.event_title,
                        t.presentation_record.narration
                    );
                }
                if !t.presentation_record.trail_ledger_lines.is_empty() {
                    for line in &t.presentation_record.trail_ledger_lines {
                        println!("{line}");
                    }
                }
            }
            ExitCode::SUCCESS
        }
    }
}
