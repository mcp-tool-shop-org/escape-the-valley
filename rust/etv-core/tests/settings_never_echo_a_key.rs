//! A key pasted into the wrong place in the settings file is never shown. No warning, connect
//! line or log line carries free text from the settings file: a JSON error says only its kind
//! and where, a variable name is shown only when it is a valid name, a key file's path and a
//! provider URL are never shown, and an endpoint is shown as its scheme and host only.
//!
//! Each case writes a settings file holding a made-up key, loads it the way `etv play` and
//! `etv tui` do, connects the narrator, and checks the warning, every connect line, the
//! full-screen log and the drawn screen for the key, written as it is and JSON-escaped.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use etv_core::cloud_claude::{HttpReply, HttpsPost};
use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::narrator::{
    FallbackPhraseTable, NarratorSettings, NarratorTransport, NullNarrator, TransportError,
};
use etv_core::play::{connect_narrator_with, endpoint_for_display, narrator_transport};
use etv_core::settings::{load_settings_or_default, settings_path};
use etv_core::tui::{view, App, Model, Reconnected, ViewData};
use etv_core::worldgen::create_new_run_with_content;

/// The made-up key. The backslash and quote make its JSON-escaped form differ from it.
const KEY: &str = "sk-ant-FAKE-0123456789abcdefghij";
/// A second made-up key, with characters JSON escapes, for the free-text fields.
const ODD_KEY: &str = "sk-ant-FAKE\\0123456789\"abcdefghij";

/// Records every URL asked for, and answers every request with a server error.
struct Refusing(Arc<Mutex<Vec<String>>>);

impl HttpsPost for Refusing {
    fn post(
        &self,
        url: &str,
        _headers: &[(&str, &str)],
        _body: &str,
        _timeout: Duration,
    ) -> Result<HttpReply, TransportError> {
        self.0.lock().unwrap().push(url.to_string());
        Ok(HttpReply {
            status: 500,
            body: "{\"error\":{\"message\":\"no\"}}".to_string(),
            request_id: None,
            retry_after_s: None,
        })
    }
}

/// A local narrator service that never answers, so the connect line names the endpoint.
struct DeadLocal;

impl NarratorTransport for DeadLocal {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Err(TransportError::ConnectionFailed("refused".to_string()))
    }
    fn post_generate(
        &self,
        _endpoint: &str,
        _body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Err(TransportError::ConnectionFailed("refused".to_string()))
    }
}

fn dead_local() -> Option<Arc<dyn NarratorTransport>> {
    Some(Arc::new(DeadLocal))
}

fn table() -> &'static FallbackPhraseTable {
    static TABLE: OnceLock<FallbackPhraseTable> = OnceLock::new();
    TABLE.get_or_init(FallbackPhraseTable::embedded)
}

/// The forms of `key` that must never appear: as written, and JSON-escaped.
fn forms(key: &str) -> Vec<String> {
    let quoted = serde_json::to_string(key).unwrap();
    vec![key.to_string(), quoted[1..quoted.len() - 1].to_string()]
}

fn assert_clean(where_: &str, text: &str, key: &str) {
    for form in forms(key) {
        assert!(!text.contains(&form), "{where_} shows the key: {text}");
    }
    // Not even a long piece of it.
    assert!(!text.contains("0123456789abcdefghij"), "{where_}: {text}");
}

/// Write `body` as the settings file, load it, connect the narrator as `etv` does (a variable
/// holding a well-formed key is set, so a hosted transport can be built), and return every line
/// the player is shown, plus the full-screen interface built on them.
fn shown_for(body: &str, key: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    let loaded = load_settings_or_default(dir.path());

    let urls = Arc::new(Mutex::new(Vec::new()));
    let lookup =
        |name: &str| (name == "ETV_SET").then(|| "test-key-not-real-0123456789XYZxyz".to_string());
    let connection = connect_narrator_with(&loaded.settings, |s| {
        narrator_transport(
            s,
            dead_local(),
            Some(Box::new(Refusing(Arc::clone(&urls)))),
            &lookup,
        )
    });

    // Line mode prints the warning, then the connect lines.
    let mut lines: Vec<String> = loaded.warning.iter().cloned().collect();
    lines.extend(connection.lines.iter().cloned());
    assert!(!lines.is_empty());
    for line in &lines {
        assert_clean("line mode", line, key);
    }

    // The full-screen interface logs the same lines and draws them.
    let content = embedded_content_pack();
    let engine = StepEngine::with_content(create_new_run_with_content(42, content), content);
    let model = Model::new(
        ViewData::from_engine(&engine),
        loaded.settings.clone(),
        "TEST",
        "HASH",
        lines.clone(),
        false,
    );
    let reconnects = Arc::new(AtomicUsize::new(0));
    let reconnects_for_hook = Arc::clone(&reconnects);
    let urls_for_hook = Arc::clone(&urls);
    let mut app = App::new(
        model,
        engine,
        &NullNarrator,
        Some(table()),
        dir.path().into(),
        false,
    )
    .with_reconnect(move |settings: &NarratorSettings| {
        reconnects_for_hook.fetch_add(1, Ordering::SeqCst);
        let connection = connect_narrator_with(settings, |s| {
            narrator_transport(
                s,
                dead_local(),
                Some(Box::new(Refusing(Arc::clone(&urls_for_hook)))),
                &|name: &str| {
                    (name == "ETV_SET").then(|| "test-key-not-real-0123456789XYZxyz".to_string())
                },
            )
        });
        Reconnected {
            narrator: std::rc::Rc::new(NullNarrator),
            live: false,
            lines: connection.lines,
            summary: None,
        }
    });
    app.press_code(crossterm::event::KeyCode::Enter);
    // Saving changed settings in the interface reconnects with them.
    app.press_char('o');
    app.press_char('2');
    app.press_char('s');
    assert_clean("the interface's log", &app.model.log.join("\n"), key);
    for size in [(80u16, 24u16), (120, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal.draw(|frame| view(&app.model, frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert_clean("the drawn screen", &text, key);
    }
}

#[test]
fn a_key_pasted_as_the_key_setting_is_not_shown() {
    // (a) The key itself where the key's source belongs: a JSON type error.
    for key in [KEY, ODD_KEY] {
        let body = serde_json::json!({"provider": "claude", "key": key}).to_string();
        shown_for(&body, key);
    }
}

#[test]
fn a_key_pasted_as_the_variable_name_is_not_shown() {
    // (b) The key where the variable's name belongs.
    for key in [KEY, ODD_KEY] {
        let body = serde_json::json!({"provider": "claude", "key": {"env": key}}).to_string();
        shown_for(&body, key);
    }
}

#[test]
fn a_key_pasted_as_the_key_file_is_not_shown() {
    // (c) The key where the key file's path belongs.
    for key in [KEY, ODD_KEY] {
        let body = serde_json::json!({"provider": "claude", "key": {"file": key}}).to_string();
        shown_for(&body, key);
    }
}

#[test]
fn a_key_in_the_provider_url_is_not_shown() {
    // (d) The key in the provider URL's query string, with a working key source.
    for key in [KEY, ODD_KEY] {
        let body = serde_json::json!({
            "provider": "openai-compatible",
            "model": "some-model",
            "provider_url": format!("https://example.com/v1?key={key}"),
            "key": {"env": "ETV_SET"},
        })
        .to_string();
        shown_for(&body, key);
    }
}

#[test]
fn a_key_in_the_endpoint_is_not_shown() {
    // The local endpoint is shown when the service does not answer: as scheme and host only.
    assert_eq!(
        endpoint_for_display(&format!("http://127.0.0.1:11434/api?key={KEY}")),
        "http://127.0.0.1:11434"
    );
    assert_eq!(
        endpoint_for_display("http://localhost:11434/"),
        "http://localhost:11434"
    );
    assert_eq!(
        endpoint_for_display(&format!("ftp://{KEY}")),
        "the configured endpoint"
    );
    let body =
        serde_json::json!({"endpoint": format!("http://127.0.0.1:1/?key={KEY}")}).to_string();
    shown_for(&body, KEY);
}

#[test]
fn a_valid_variable_name_is_still_named() {
    // The strict check lets an ordinary name through, so the player is told which one.
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"provider": "claude", "key": {"env": "ETV_UNSET"}}"#,
    )
    .unwrap();
    let loaded = load_settings_or_default(dir.path());
    assert_eq!(loaded.warning, None);
    let connection = connect_narrator_with(&loaded.settings, |s| {
        narrator_transport(s, None, Some(Box::new(Refusing(Arc::default()))), &|_| None)
    });
    assert_eq!(
        connection.lines[0],
        "The cloud narrator could not start: the environment variable ETV_UNSET is not set."
    );
}
