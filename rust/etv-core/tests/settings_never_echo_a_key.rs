//! A key pasted into the wrong place in the settings file is never shown. No warning, connect
//! line, turn line, session summary or log line carries free text from the settings file: a
//! JSON error says only its kind and where, no variable name, key file path or provider URL is
//! ever shown, a model name that looks like a key is refused unshown, and an endpoint is shown
//! only as a loopback `scheme://host:port`.
//!
//! Each case writes a settings file holding a made-up key, loads it the way `etv play` and
//! `etv tui` do, connects the narrator (through a hosted client that echoes the request URL
//! back in its errors), plays turns when a narrator connected, and checks every line the player
//! sees, the full-screen log and the drawn screen (before Enter, at camp, with the options view
//! open, after a reconnect and after a turn) for the key: as written, JSON-escaped, and in short
//! pieces, so a key wrapped across two screen rows is caught too.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use etv_core::cloud_claude::{HttpReply, HttpsPost};
use etv_core::content::embedded_content_pack;
use etv_core::engine::StepEngine;
use etv_core::model::{IntentAction, PlayerIntent};
use etv_core::narrator::{
    FallbackPhraseTable, Narrator, NarratorSettings, NarratorTransport, NullNarrator,
    TransportError,
};
use etv_core::play::{
    connect_narrator_with, endpoint_for_display, narrator_summary_line, narrator_transport,
    play_turn,
};
use etv_core::settings::{load_settings_or_default, settings_path};
use etv_core::tui::{view, App, Model, Reconnected, ViewData};
use etv_core::worldgen::create_new_run_with_content;

/// A made-up key. It has no character JSON escapes, so its escaped form is the same.
const KEY: &str = "sk-ant-FAKE-0123456789abcdefghij";
/// A second made-up key, holding a backslash and a quote, so its JSON-escaped form differs.
const ODD_KEY: &str = "sk-ant-FAKE\\0123456789\"abcdefghij";
/// A made-up key of letters and digits only, 64 characters or fewer: a valid variable name.
const NAME_SHAPED_KEY: &str = "FAKEKEY0123456789abcdefghijABCDEFGHIJ";
/// A well-formed key a test variable holds, so a hosted transport can be built.
const GOOD_KEY: &str = "test-key-not-real-0123456789XYZxyz";

/// How the hosted client answers: an error body that echoes the request URL, or a transport
/// error that does.
#[derive(Clone, Copy)]
enum Echo {
    ErrorBody,
    TransportError,
}

/// Records every URL asked for, and fails every request with the URL in the failure.
struct Echoing {
    mode: Echo,
    urls: Arc<Mutex<Vec<String>>>,
}

impl HttpsPost for Echoing {
    fn post(
        &self,
        url: &str,
        _headers: &[(&str, &str)],
        _body: &str,
        _timeout: Duration,
    ) -> Result<HttpReply, TransportError> {
        self.urls.lock().unwrap().push(url.to_string());
        match self.mode {
            Echo::ErrorBody => Ok(HttpReply {
                status: 500,
                body: serde_json::json!({"error": {"message": format!("bad request to {url}")}})
                    .to_string(),
                request_id: None,
                retry_after_s: None,
            }),
            Echo::TransportError => Err(TransportError::ConnectionFailed(format!(
                "cannot reach {url}"
            ))),
        }
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

/// A local narrator service that answers, without the settings' model installed, so the connect
/// line names the model ("The narrator model '…' is not installed").
struct ListingLocal;

impl NarratorTransport for ListingLocal {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Ok((200, r#"{"models":[{"name":"qwen3:4b"}]}"#.to_string()))
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

#[derive(Clone, Copy)]
enum Local {
    Dead,
    Listing,
}

impl Local {
    fn transport(self) -> Option<Arc<dyn NarratorTransport>> {
        Some(match self {
            Local::Dead => Arc::new(DeadLocal),
            Local::Listing => Arc::new(ListingLocal),
        })
    }
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

/// `text` with every space, line break and box-drawing character taken out, so a key the
/// screen wrapped across rows reads as one run of characters again.
fn squeezed(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !('\u{2500}'..='\u{257f}').contains(c))
        .collect()
}

fn assert_clean(where_: &str, text: &str, key: &str) {
    for form in forms(key) {
        assert!(!text.contains(&form), "{where_} shows the key: {text}");
        // No piece of ten characters or more, even across a wrapped row.
        let flat = squeezed(text);
        let chars: Vec<char> = squeezed(&form).chars().collect();
        for piece in chars.windows(10) {
            let piece: String = piece.iter().collect();
            assert!(
                !flat.contains(&piece),
                "{where_} shows a piece of the key ({piece:?}): {text}"
            );
        }
    }
}

fn draw(model: &Model, size: (u16, u16)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal.draw(|frame| view(model, frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

fn assert_screen_clean(where_: &str, model: &Model, key: &str) {
    for size in [(80u16, 24u16), (120, 40)] {
        assert_clean(&format!("{where_} at {size:?}"), &draw(model, size), key);
    }
}

/// One case: a settings file, the key it hides, the local service, how the hosted client
/// echoes, and the environment.
struct Case {
    body: String,
    key: String,
    local: Local,
    echo: Echo,
    env: BTreeMap<String, String>,
}

impl Case {
    fn new(body: serde_json::Value, key: &str) -> Case {
        let mut env = BTreeMap::new();
        env.insert("ETV_SET".to_string(), GOOD_KEY.to_string());
        Case {
            body: body.to_string(),
            key: key.to_string(),
            local: Local::Dead,
            echo: Echo::ErrorBody,
            env,
        }
    }

    fn local(mut self, local: Local) -> Case {
        self.local = local;
        self
    }

    fn echo(mut self, echo: Echo) -> Case {
        self.echo = echo;
        self
    }

    fn env(mut self, name: &str, value: &str) -> Case {
        self.env.insert(name.to_string(), value.to_string());
        self
    }

    /// Load, connect, play and look, as `etv play` and `etv tui` do. Returns how many requests
    /// reached the hosted client.
    fn check(self) -> usize {
        let key = self.key.as_str();
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &self.body).unwrap();
        let loaded = load_settings_or_default(dir.path());

        let urls = Arc::new(Mutex::new(Vec::new()));
        let env = self.env.clone();
        let lookup = move |name: &str| env.get(name).cloned();
        let (local, echo) = (self.local, self.echo);
        let transport_for = {
            let urls = Arc::clone(&urls);
            let lookup = lookup.clone();
            move |s: &NarratorSettings| {
                narrator_transport(
                    s,
                    local.transport(),
                    Some(Box::new(Echoing {
                        mode: echo,
                        urls: Arc::clone(&urls),
                    })),
                    &lookup,
                )
            }
        };
        let connection = connect_narrator_with(&loaded.settings, &transport_for);

        // Line mode prints the warning, then the connect lines.
        let mut lines: Vec<String> = loaded.warning.iter().cloned().collect();
        lines.extend(connection.lines.iter().cloned());
        assert!(!lines.is_empty());
        for line in &lines {
            assert_clean("line mode", line, key);
        }

        // A connected narrator plays turns: every line they show, and the session summary.
        let content = embedded_content_pack();
        if let Some(live) = &connection.narrator {
            let mut engine =
                StepEngine::with_content(create_new_run_with_content(42, content), content);
            for _ in 0..3 {
                let shown = play_turn(
                    &mut engine,
                    live,
                    Some(table()),
                    &PlayerIntent::new(IntentAction::Travel),
                    None,
                );
                assert_clean("a turn in line mode", &shown.join("\n"), key);
            }
            assert_clean(
                "the session summary",
                &narrator_summary_line(&live.stats()),
                key,
            );
        }

        // The full-screen interface: the same lines in its log, the real reconnect builder.
        let engine = StepEngine::with_content(create_new_run_with_content(42, content), content);
        let model = Model::new(
            ViewData::from_engine(&engine),
            loaded.settings.clone(),
            "TEST",
            "HASH",
            lines.clone(),
            false,
        );
        let narrator: &dyn Narrator = match &connection.narrator {
            Some(live) => live,
            None => &NullNarrator,
        };
        let reconnects = Arc::new(AtomicUsize::new(0));
        let reconnects_for_hook = Arc::clone(&reconnects);
        let mut app = App::new(
            model,
            engine,
            narrator,
            Some(table()),
            dir.path().into(),
            connection.narrator.is_some(),
        )
        .with_reconnect(move |settings: &NarratorSettings| {
            reconnects_for_hook.fetch_add(1, Ordering::SeqCst);
            Reconnected::from_connection(connect_narrator_with(settings, &transport_for))
        });
        if let Some(live) = &connection.narrator {
            app = app.with_summary(|| narrator_summary_line(&live.stats()));
        }
        assert_screen_clean("the title screen", &app.model, key);
        app.press_code(crossterm::event::KeyCode::Enter);
        assert_screen_clean("the camp screen", &app.model, key);
        app.press_char('o');
        assert_screen_clean("the options view", &app.model, key);
        // Saving changed settings in the interface reconnects with them, once.
        app.press_char('2');
        assert_screen_clean("the options view after a change", &app.model, key);
        app.press_char('s');
        assert_eq!(reconnects.load(Ordering::SeqCst), 1, "one reconnect");
        assert_clean("the interface's log", &app.model.log.join("\n"), key);
        assert_screen_clean("the screen after the reconnect", &app.model, key);
        // A turn, and what it shows.
        app.press_char('t');
        while app.model.resolved.is_some() {
            app.press_code(crossterm::event::KeyCode::Enter);
        }
        assert_clean(
            "the interface's log after a turn",
            &app.model.log.join("\n"),
            key,
        );
        assert_screen_clean("the screen after a turn", &app.model, key);
        if let Some(summary) = app.narrator_summary() {
            assert_clean("the interface's summary", &summary, key);
        }
        let asked = urls.lock().unwrap().len();
        asked
    }
}

#[test]
fn a_key_pasted_as_the_key_setting_is_not_shown() {
    for key in [KEY, ODD_KEY] {
        Case::new(serde_json::json!({"provider": "claude", "key": key}), key).check();
    }
}

#[test]
fn a_key_pasted_as_the_provider_is_not_shown() {
    for key in [KEY, ODD_KEY] {
        Case::new(serde_json::json!({"provider": key}), key).check();
    }
}

#[test]
fn a_key_pasted_as_the_variable_name_is_not_shown() {
    // Looked up like any name (none is refused for its shape), and never named.
    for key in [KEY, ODD_KEY, NAME_SHAPED_KEY] {
        Case::new(
            serde_json::json!({"provider": "claude", "key": {"env": key}}),
            key,
        )
        .check();
    }
}

#[test]
fn a_key_pasted_as_the_key_file_is_not_shown() {
    for key in [KEY, ODD_KEY] {
        Case::new(
            serde_json::json!({"provider": "claude", "key": {"file": key}}),
            key,
        )
        .check();
    }
}

#[test]
fn a_key_with_junk_in_the_key_file_or_the_variable_is_not_shown() {
    // The file or variable holds the key plus something after it: not one key, and the
    // refusal must not quote what it found.
    for key in [KEY, ODD_KEY] {
        let keys = tempfile::tempdir().unwrap();
        let file = keys.path().join("narrator.key");
        std::fs::write(&file, format!("{key} trailing junk")).unwrap();
        Case::new(
            serde_json::json!({"provider": "claude", "key": {"file": file.to_str().unwrap()}}),
            key,
        )
        .check();
        Case::new(
            serde_json::json!({"provider": "claude", "key": {"env": "ETV_JUNK"}}),
            key,
        )
        .env("ETV_JUNK", &format!("{key} trailing junk"))
        .check();
    }
}

#[test]
fn a_key_in_the_provider_url_is_not_shown_even_when_echoed_back() {
    // A working key source, so the hosted client is called: its error body, or its transport
    // error, carries the request URL with the key in its query. Nothing the player sees does.
    for key in [KEY, ODD_KEY] {
        for echo in [Echo::ErrorBody, Echo::TransportError] {
            let asked = Case::new(
                serde_json::json!({
                    "provider": "openai-compatible",
                    "model": "some-model",
                    "provider_url": format!("https://example.com/v1?key={key}"),
                    "key": {"env": "ETV_SET"},
                }),
                key,
            )
            .echo(echo)
            .check();
            assert!(asked > 0, "the hosted client was called");
        }
    }
}

#[test]
fn a_key_as_an_invalid_provider_url_is_not_shown() {
    for key in [KEY, ODD_KEY] {
        for url in [
            format!("http://{key}"),
            format!("ftp://{key}"),
            key.to_string(),
        ] {
            Case::new(
                serde_json::json!({
                    "provider": "openai-compatible",
                    "provider_url": url,
                    "key": {"env": "ETV_SET"},
                }),
                key,
            )
            .check();
        }
    }
}

#[test]
fn a_key_in_the_endpoint_is_not_shown() {
    for key in [KEY, ODD_KEY] {
        // As the host, with another host allowed; as the port; as userinfo; in the query.
        Case::new(
            serde_json::json!({"endpoint": format!("http://{key}:11434"), "allow_non_loopback": true}),
            key,
        )
        .check();
        Case::new(
            serde_json::json!({"endpoint": format!("http://{key}"), "allow_non_loopback": true}),
            key,
        )
        .check();
        Case::new(
            serde_json::json!({"endpoint": format!("http://127.0.0.1:{key}")}),
            key,
        )
        .check();
        Case::new(
            serde_json::json!({"endpoint": format!("http://{key}@127.0.0.1:11434")}),
            key,
        )
        .check();
        Case::new(
            serde_json::json!({"endpoint": format!("http://127.0.0.1:1/?key={key}")}),
            key,
        )
        .check();
    }
}

#[test]
fn only_a_loopback_endpoint_is_shown() {
    assert_eq!(
        endpoint_for_display("http://127.0.0.1:11434/api"),
        "http://127.0.0.1:11434"
    );
    assert_eq!(
        endpoint_for_display("http://localhost:11434/"),
        "http://localhost:11434"
    );
    assert_eq!(
        endpoint_for_display("https://[::1]:8443"),
        "https://[::1]:8443"
    );
    assert_eq!(endpoint_for_display("http://127.1.2.3"), "http://127.1.2.3");
    for hidden in [
        format!("http://{KEY}"),
        format!("http://{KEY}:11434"),
        format!("http://127.0.0.1:{KEY}"),
        format!("http://{KEY}@127.0.0.1:11434"),
        "http://192.168.1.5:11434".to_string(),
        "http://127.0.0.1:99999".to_string(),
        "http://127.0.0.256:1".to_string(),
        format!("http://[{KEY}]:11434"),
        format!("ftp://{KEY}"),
    ] {
        assert_eq!(
            endpoint_for_display(&hidden),
            "the configured endpoint",
            "{hidden}"
        );
    }
}

#[test]
fn a_variable_name_is_never_named() {
    // Even a valid name is not repeated: a key can look like one.
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
        narrator_transport(s, None, Some(Box::new(NoCall)), &|_| None)
    });
    assert_eq!(
        connection.lines[0],
        "The cloud narrator could not start: the environment variable named in the settings is not set."
    );
}

/// A long or unusual variable name is looked up like any other.
#[test]
fn a_long_or_unusual_variable_name_is_still_looked_up() {
    let long = "ETV_".repeat(30);
    for name in [long.as_str(), "my-narrator.key", "1STARTS_WITH_A_DIGIT"] {
        let settings = NarratorSettings {
            provider: etv_core::narrator_provider::NarratorProvider::Claude,
            model: "claude-haiku-5-5".to_string(),
            key: etv_core::narrator_provider::KeySource {
                env: Some(name.to_string()),
                file: None,
            },
            ..NarratorSettings::default()
        };
        let looked = AtomicUsize::new(0);
        let lookup = |n: &str| {
            assert_eq!(n, name);
            looked.fetch_add(1, Ordering::SeqCst);
            Some(GOOD_KEY.to_string())
        };
        let transport = narrator_transport(&settings, None, Some(Box::new(NoCall)), &lookup);
        assert!(transport.is_ok(), "{name}");
        assert_eq!(looked.load(Ordering::SeqCst), 1, "{name}");
    }
    // Only an impossible name is refused, unread and unnamed.
    for bad in ["", "A=B", "A\0B"] {
        let s = etv_core::narrator_provider::KeySource {
            env: Some(bad.to_string()),
            file: None,
        };
        let err = s.resolve(&|_| panic!("looked up {bad:?}")).unwrap_err();
        assert_eq!(
            err,
            "the environment variable named in the settings cannot be a variable's name"
        );
    }
}

/// An HTTPS client the test never expects to be called.
struct NoCall;

impl HttpsPost for NoCall {
    fn post(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &str,
        _timeout: Duration,
    ) -> Result<HttpReply, TransportError> {
        panic!("no request expected")
    }
}

#[test]
fn a_key_pasted_as_the_model_is_not_shown() {
    // The model name is shown back ("The narrator model '…' is not installed"), so a key there
    // is refused first, with a reason that does not repeat it.
    for key in [
        KEY,
        "sk-or-FAKE0123456789abcdefghij",
        "AIzaFAKE0123456789abcdefghij",
        "hf_FAKE0123456789abcdefghij",
        "gsk_FAKE0123456789abcdefghij",
        "xai-FAKE0123456789abcdefghij",
        "ghp_FAKE0123456789abcdefghij",
        "github_pat_FAKE0123456789abcdefghij",
    ] {
        Case::new(serde_json::json!({"model": key}), key)
            .local(Local::Listing)
            .check();
    }
    // A real model name is still shown.
    let loaded_line = {
        let dir = tempfile::tempdir().unwrap();
        let path = settings_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"model": "llama3.2:3b"}"#).unwrap();
        let loaded = load_settings_or_default(dir.path());
        connect_narrator_with(&loaded.settings, |s| {
            narrator_transport(s, Local::Listing.transport(), None, &|_| None)
        })
        .lines[0]
            .clone()
    };
    assert!(loaded_line.contains("llama3.2:3b"), "{loaded_line}");
}
