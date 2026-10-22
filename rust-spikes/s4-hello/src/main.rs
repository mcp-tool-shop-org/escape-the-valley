//! Spike S4: Minimal binary with `--version` and `--self-test` (ratatui `TestBackend` frame).

use ratatui::{
    backend::TestBackend,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::env;
use std::process::ExitCode;

fn run_self_test() -> Result<String, String> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
    terminal
        .draw(|frame| {
            let block = Block::default()
                .title("Escape the Valley 2.0")
                .borders(Borders::ALL);
            let paragraph =
                Paragraph::new("Trail Ledger ready — TestBackend 80x24 OK").block(block);
            frame.render_widget(paragraph, frame.area());
        })
        .map_err(|e| e.to_string())?;

    let buffer = terminal.backend().buffer();
    let mut rendered = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            rendered.push_str(buffer[(x, y)].symbol());
        }
        rendered.push('\n');
    }
    if !rendered.contains("Escape the Valley 2.0")
        || !rendered.contains("Trail Ledger ready — TestBackend 80x24 OK")
    {
        return Err("Rendered TestBackend buffer missing expected text".to_string());
    }
    Ok("s4-hello self-test PASS (ratatui TestBackend 80x24)".to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("s4-hello {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--self-test") => match run_self_test() {
            Ok(msg) => {
                println!("{msg}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("s4-hello self-test FAIL: {err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            println!(
                "s4-hello {} (use --version or --self-test)",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_test_renders_ratatui_frame() {
        let msg = run_self_test().expect("self_test should succeed");
        assert!(msg.contains("PASS"));
    }
}
