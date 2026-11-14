//! `narrator_measure`: thin command-line wrapper around [`etv_core::measure::run_cli`].
//!
//! Built only with the `narrator-ollama` feature, since it talks to a local Ollama. It makes
//! model calls when run without `--check-only`; do not run it against a model without a granted
//! window and a frozen turn set.
//!
//! With the `narrator-claude` feature it also takes the `--cloud` flags (see
//! [`etv_core::cloud_claude::CLOUD_USAGE`]) and sends the same prompts to the Claude API. That
//! costs money: it needs its own grant, a key file and a cap.

use std::sync::Arc;

use etv_core::cloud_claude::{split_cloud_args, CloudArgs};
use etv_core::narrator::UreqTransport;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cloud, args) = match split_cloud_args(&args) {
        Ok(split) => split,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let mut stdout = std::io::stdout();
    let code = match cloud {
        None => match etv_core::measure::run_cli(&args, Arc::new(UreqTransport), &mut stdout) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                2
            }
        },
        Some(cloud) => run_cloud(cloud, &args, &mut stdout),
    };
    std::process::exit(code);
}

#[cfg(not(feature = "narrator-claude"))]
fn run_cloud(_cloud: CloudArgs, _args: &[String], _stdout: &mut std::io::Stdout) -> i32 {
    eprintln!("--cloud needs a build with the narrator-claude feature");
    2
}

/// Runs the harness against the Claude API. Exit code 3 means the run halted (cap, billing,
/// authentication, or a burst of busy answers); the halt file says why.
#[cfg(feature = "narrator-claude")]
fn run_cloud(cloud: CloudArgs, args: &[String], stdout: &mut std::io::Stdout) -> i32 {
    use etv_core::cloud_claude::{nano_as_usd, ApiKey, ClaudeTransport, UreqHttpsPost};

    let key = match ApiKey::read_from_file(&cloud.key_file) {
        Ok(key) => key,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let transport = match ClaudeTransport::new(key, cloud.cfg, Box::new(UreqHttpsPost::default())) {
        Ok(transport) => Arc::new(transport),
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let result = etv_core::measure::run_cli(args, transport.clone(), stdout);
    let summary = transport.summary();
    eprintln!(
        "cloud: {} calls this process; ledger {} requests, ${} spent",
        summary.calls,
        summary.ledger.requests,
        nano_as_usd(summary.ledger.spent_nano)
    );
    if let Some(reason) = summary.halted {
        eprintln!("cloud: HALTED: {reason}");
        return 3;
    }
    match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            2
        }
    }
}
