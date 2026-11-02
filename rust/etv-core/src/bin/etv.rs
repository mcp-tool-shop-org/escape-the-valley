use std::io::Write;
use std::process::ExitCode;

use etv_core::content::{
    embedded_content_pack, parse_cli_args, version_with_content_hash, CliCommand, ContentPack,
};
use etv_core::notices::third_party_notices_text;
use etv_core::snapshot::run_tier2_seed_trace;
use etv_core::validator::{validate_content_with_options, ValidationOptions};

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
        CliCommand::Notices => ExitCode::SUCCESS,
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
