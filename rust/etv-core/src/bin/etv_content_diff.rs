//! `etv_content_diff <old_dir> <new_dir> [--md <out.md>] [--html <out.html>] [--all]`
//!
//! Renders the difference between two content directories as a table. See
//! `etv_core::content_diff` for the format.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = etv_core::content_diff::run_cli(&args);
    let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    std::process::exit(out.code);
}
