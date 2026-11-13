//! `etv_turn_set`: write narrator turn set v1 and print its SHA-256.
//!
//! Usage: `etv_turn_set <out.json>`. Makes no model call and uses the embedded content only.

use std::process::ExitCode;

use etv_core::content::embedded_content_pack;
use etv_core::measure::sha256_hex;
use etv_core::turn_set::{generate_turn_set, turn_set_to_json, TURN_SET_V1_QUOTAS};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [out_path] = args.as_slice() else {
        eprintln!("usage: etv_turn_set <out.json>");
        return ExitCode::from(2);
    };
    let set = generate_turn_set(&TURN_SET_V1_QUOTAS, embedded_content_pack());
    let text = turn_set_to_json(&set);
    if let Err(e) = std::fs::write(out_path, text.as_bytes()) {
        eprintln!("error: could not write {out_path}: {e}");
        return ExitCode::from(1);
    }
    println!("turns {}", set.turns.len());
    println!("sha256 {}", sha256_hex(text.as_bytes()));
    ExitCode::SUCCESS
}
