//! `etv_simulate`: balance simulation harness. Uses the embedded content pack only.

use std::process::ExitCode;

use etv_core::content::embedded_content_pack;
use etv_core::ruleset::Ruleset;
use etv_core::simulate::{
    format_text, parse_args, seed_list, simulate_with_levers, SimCommand, USAGE,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(SimCommand::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(SimCommand::Run(o)) => o,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("run 'etv_simulate --help' for usage");
            return ExitCode::from(2);
        }
    };

    let content = embedded_content_pack();
    let seeds = seed_list(&opts);
    let reports: Vec<_> = opts
        .policies
        .iter()
        .map(|&p| {
            simulate_with_levers(&seeds, p, opts.turns, content, opts.ruleset, opts.levers)
        })
        .collect();

    if opts.json {
        match serde_json::to_string_pretty(&reports) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("error: could not serialize report: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        if opts.ruleset == Ruleset::V2 {
            println!("ruleset v2");
        }
        for r in &reports {
            print!("{}", format_text(r));
        }
    }
    ExitCode::SUCCESS
}
