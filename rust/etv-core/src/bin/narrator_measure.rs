//! `narrator_measure`: thin command-line wrapper around [`etv_core::measure::run_cli`].
//!
//! Built only with the `narrator-ollama` feature, since it talks to a local Ollama. It makes
//! model calls when run without `--check-only`; do not run it against a model without a granted
//! window and a frozen turn set.

use std::sync::Arc;

use etv_core::narrator::UreqTransport;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout();
    if let Err(message) = etv_core::measure::run_cli(&args, Arc::new(UreqTransport), &mut stdout) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
