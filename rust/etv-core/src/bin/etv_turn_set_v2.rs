//! `etv_turn_set_v2`: write narrator turn set v2 and print its SHA-256 and makeup.
//!
//! Usage: `etv_turn_set_v2 <out.json>`. Makes no model call and uses the embedded content only.

use std::collections::BTreeMap;
use std::process::ExitCode;

use etv_core::content::embedded_content_pack;
use etv_core::measure::sha256_hex;
use etv_core::turn_set::{generate_turn_set_v2, turn_set_v2_to_json};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [out_path] = args.as_slice() else {
        eprintln!("usage: etv_turn_set_v2 <out.json>");
        return ExitCode::from(2);
    };
    let set = generate_turn_set_v2(embedded_content_pack());
    let text = turn_set_v2_to_json(&set);
    if let Err(e) = std::fs::write(out_path, text.as_bytes()) {
        eprintln!("error: could not write {out_path}: {e}");
        return ExitCode::from(1);
    }
    let mut tags: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &set.turns {
        for tag in &t.strata {
            *tags.entry(tag.as_str()).or_default() += 1;
        }
    }
    println!("turns {}", set.turns.len());
    println!("sha256 {}", sha256_hex(text.as_bytes()));
    for (tag, n) in tags {
        println!("{tag} {n}");
    }
    ExitCode::SUCCESS
}
