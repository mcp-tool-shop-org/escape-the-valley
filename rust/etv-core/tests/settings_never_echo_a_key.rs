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
    /// An HTTP-status error whose text holds the request URL and every header sent, the key's
    /// header included.
    StatusError,
    /// A 200 answer whose narration names the request URL.
    UrlReply,
    /// A 200 answer whose narration names the key sent in the `authorization` header, its first
    /// letter written as a JSON `\uXXXX` escape inside the narration's JSON.
    EscapedKeyReply,
    /// A 200 answer with `{"extra": 1e400, ...}` carrying a `\uXXXX`-escaped key.
    HugeNumberEscapedKeyReply,
    /// A 200 answer that starts with `{` (`{"extra": NaN, ...}`) so it is JSON-shaped but fails
    /// to parse as JSON while carrying a `\uXXXX`-escaped key.
    MalformedJsonEscapedKeyReply,
}

/// An OpenAI-compatible 200 answer whose message content is `content`.
fn chat_reply(content: &str) -> HttpReply {
    HttpReply {
        status: 200,
        body: serde_json::json!({"choices": [{"message": {"content": content}}]}).to_string(),
        request_id: None,
        retry_after_s: None,
    }
}

/// `key` with its first character written as a JSON `\uXXXX` escape.
fn escaped_first_char_key(key: &str) -> String {
    let mut chars = key.chars();
    let first = chars.next().unwrap();
    format!(r"\u{:04x}{}", first as u32, chars.as_str())
}

/// The narration JSON naming `key`, with the key's first character escaped as `\uXXXX`.
fn escaped_key_narration(key: &str) -> String {
    let escaped = escaped_first_char_key(key);
    format!(r#"{{"sentences": ["Rain beads on the canvas near {escaped}."]}}"#)
}

fn bearer_key<'a>(headers: &'a [(&str, &str)]) -> &'a str {
    headers
        .iter()
        .find(|(n, _)| *n == "authorization")
        .and_then(|(_, v)| v.strip_prefix("Bearer "))
        .unwrap_or("")
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
        headers: &[(&str, &str)],
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
            Echo::StatusError => Err(TransportError::HttpStatus(
                502,
                format!("bad gateway for {url} with {headers:?}"),
            )),
            Echo::UrlReply => Ok(chat_reply(&format!(
                r#"{{"sentences": ["Rain beads on the canvas near {url}."]}}"#
            ))),
            Echo::EscapedKeyReply => Ok(chat_reply(&escaped_key_narration(bearer_key(headers)))),
            Echo::HugeNumberEscapedKeyReply => {
                let escaped = escaped_first_char_key(bearer_key(headers));
                Ok(chat_reply(&format!(
                    r#"{{"extra": 1e400, "sentences": ["Rain beads on the canvas near {escaped}."]}}"#
                )))
            }
            Echo::MalformedJsonEscapedKeyReply => {
                let escaped = escaped_first_char_key(bearer_key(headers));
                Ok(chat_reply(&format!(
                    r#"{{"extra": NaN, "sentences": ["Rain beads on the canvas near {escaped}."]}}"#
                )))
            }
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

/// A local narrator service that lists `0` as installed and answers every request, so a
/// narrator connects and the connect line is "Narrator: …, model …".
struct Answering(&'static str);

impl NarratorTransport for Answering {
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Ok((
            200,
            serde_json::json!({"models": [{"name": self.0}]}).to_string(),
        ))
    }
    fn post_generate(
        &self,
        _endpoint: &str,
        _body_json: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        Ok((200, r#"{"response":""}"#.to_string()))
    }
}

#[derive(Clone, Copy)]
enum Local {
    Dead,
    Listing,
    /// A service that lists this name as installed and answers.
    Lists(&'static str),
}

impl Local {
    fn transport(self) -> Option<Arc<dyn NarratorTransport>> {
        Some(match self {
            Local::Dead => Arc::new(DeadLocal),
            Local::Listing => Arc::new(ListingLocal),
            Local::Lists(name) => Arc::new(Answering(name)),
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
        Case::raw(&body.to_string(), key)
    }

    /// A settings file written as `body`, which need not be JSON at all.
    fn raw(body: &str, key: &str) -> Case {
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

        // Line mode's options screen, with the empty model prompt (4, then Enter).
        let mut shown_options = Vec::new();
        etv_core::screens::run_settings_screen(
            loaded.settings.clone(),
            std::io::Cursor::new("4\n\nq\n"),
            &mut shown_options,
        )
        .unwrap();
        assert_clean(
            "line mode's options screen",
            &String::from_utf8(shown_options).unwrap(),
            key,
        );

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
        )
        .with_installed(connection.installed.clone());
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
        // The empty model prompt: 4, then Enter, keeps the model and says so.
        app.press_char('4');
        assert_screen_clean("the model prompt", &app.model, key);
        app.press_code(crossterm::event::KeyCode::Enter);
        assert_screen_clean("the options view after an empty prompt", &app.model, key);
        assert_clean("the options note", &app.model.settings_note, key);
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
    // error, carries the request URL with the key in its path (a query is refused at load).
    // Nothing the player sees does.
    for key in [KEY, ODD_KEY] {
        for echo in [
            Echo::ErrorBody,
            Echo::TransportError,
            Echo::StatusError,
            Echo::UrlReply,
        ] {
            let asked = Case::new(
                serde_json::json!({
                    "provider": "openai-compatible",
                    "model": "some-model",
                    "provider_url": format!("https://example.com/v1/{key}"),
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
        r#"{"provider": "claude", "model": "claude-haiku-5-5", "key": {"env": "ETV_UNSET"}}"#,
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
}

/// The lines a settings file gives at start, with `local` as the local service.
fn connect_lines(body: &str, local: Local) -> (Option<String>, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    let loaded = load_settings_or_default(dir.path());
    let lines = connect_narrator_with(&loaded.settings, |s| {
        narrator_transport(s, local.transport(), None, &|_| None)
    })
    .lines;
    (loaded.warning, lines)
}

/// A made-up key of 32 letters and digits, with no known prefix.
const PLAIN_KEY: &str = "Q7xK2mN9pL4vR8tW1zY6bC3dF5gH0jAs";

#[test]
fn a_prefixless_key_as_the_model_is_not_shown() {
    // A key of model-name characters passes the name check, so it is never shown as free text:
    // for the local narrator (service listing other models, and service not answering), for an
    // OpenAI-compatible provider, and for Claude (which refuses any model it does not list).
    for key in [NAME_SHAPED_KEY, PLAIN_KEY] {
        Case::new(serde_json::json!({"model": key}), key)
            .local(Local::Listing)
            .check();
        Case::new(serde_json::json!({"model": key}), key).check();
        Case::new(
            serde_json::json!({
                "provider": "openai-compatible",
                "model": key,
                "provider_url": "https://example.com/v1",
                "key": {"env": "ETV_SET"},
            }),
            key,
        )
        .check();
        Case::new(
            serde_json::json!({"provider": "claude", "model": key, "key": {"env": "ETV_SET"}}),
            key,
        )
        .check();
    }
}

#[test]
fn a_key_with_odd_characters_as_the_model_is_not_shown() {
    Case::new(serde_json::json!({"model": ODD_KEY}), ODD_KEY)
        .local(Local::Listing)
        .check();
}

#[test]
fn a_key_in_the_endpoint_path_is_not_shown() {
    for key in [KEY, ODD_KEY] {
        Case::new(
            serde_json::json!({"endpoint": format!("http://127.0.0.1:11434/{key}")}),
            key,
        )
        .check();
    }
}

#[test]
fn malformed_json_holding_a_key_is_not_shown() {
    // A syntax error (the key alone) and an unexpected end (an unclosed string).
    for key in [KEY, NAME_SHAPED_KEY] {
        Case::raw(key, key).check();
        Case::raw(&format!("{{\"model\": \"{key}"), key).check();
    }
}

#[test]
fn a_known_model_is_still_named() {
    // A model the local service lists as installed is named.
    let (warning, lines) = connect_lines(r#"{"model": "qwen3:4b"}"#, Local::Listing);
    assert_eq!(warning, None);
    assert_eq!(
        lines[0],
        "The narrator model 'qwen3:4b' did not load in time."
    );
    // The built-in default is named, with its pull hint, when it is not installed.
    let (warning, lines) = connect_lines("{}", Local::Listing);
    assert_eq!(warning, None);
    assert_eq!(
        lines[0],
        format!(
            "The narrator model '{0}' is not installed in Ollama. Install it with: ollama pull {0}",
            etv_core::narrator::DEFAULT_NARRATOR_MODEL
        )
    );
    // A real model with a prefix once refused is accepted again, and, not installed, unnamed.
    let (warning, lines) = connect_lines(r#"{"model": "xai-org/grok-1"}"#, Local::Listing);
    assert_eq!(warning, None, "xai-org/grok-1 is a model name");
    assert_eq!(
        lines[0],
        "The configured narrator model is not installed in Ollama."
    );
    // The options screen names a Claude model, and not a free-text one.
    let claude = NarratorSettings::default().with_cloud();
    let screen = etv_core::screens::settings_screen_lines(&claude).join(
        "
",
    );
    assert!(screen.contains("4. Model     claude-haiku-5-5"), "{screen}");
    let custom = NarratorSettings {
        model: "xai-org/grok-1".to_string(),
        ..NarratorSettings::default()
    };
    let screen = etv_core::screens::settings_screen_lines(&custom).join(
        "
",
    );
    assert!(
        screen.contains("4. Model     the configured model"),
        "{screen}"
    );
    // A name the player types on the screen is shown back there.
    let mut typed = etv_core::screens::SettingsScreen::new(custom);
    typed.handle_line("4");
    let said = typed.handle_line("mistral:7b");
    assert_eq!(
        said,
        etv_core::screens::SettingsEvent::Changed("Model is now mistral:7b.".to_string())
    );
    assert!(typed
        .lines()
        .join(
            "
"
        )
        .contains("4. Model     mistral:7b"));
}

#[test]
fn an_endpoint_error_carries_no_endpoint_text() {
    use etv_core::narrator::{validate_endpoint, EndpointError};
    let mut seen = std::collections::BTreeSet::new();
    for key in [KEY, ODD_KEY, NAME_SHAPED_KEY] {
        for endpoint in [
            format!("ftp://{key}"),
            format!("http://{key}:11434"),
            format!("http://127.0.0.1:{key}"),
            format!("http://{key}@127.0.0.1:11434"),
            format!("http://127.0.0.1:11434/?key={key}"),
            format!("http://[{key}]:11434"),
        ] {
            for allow in [false, true] {
                let Err(e) = validate_endpoint(&endpoint, allow) else {
                    continue;
                };
                seen.insert(format!("{e:?}"));
                for text in [format!("{e:?}"), format!("{e}")] {
                    assert_clean("an endpoint error", &text, key);
                }
            }
        }
    }
    // Every variant was produced at least once.
    for variant in [
        EndpointError::InvalidScheme,
        EndpointError::InvalidAuthority,
        EndpointError::NonLoopbackRefused,
    ] {
        assert!(
            seen.contains(&format!("{variant:?}")),
            "{variant:?} never produced"
        );
    }
}

#[test]
fn a_connected_narrator_names_a_model_the_local_service_lists() {
    let (warning, lines) = connect_lines(r#"{"model": "qwen3:4b"}"#, Local::Lists("qwen3:4b"));
    assert_eq!(warning, None);
    assert_eq!(
        lines,
        vec!["Narrator: Fireside, Brief, model qwen3:4b.".to_string()]
    );
}

#[test]
fn a_remote_services_listing_never_names_a_model() {
    // A service on another host (allowed by the settings) may list anything, a pasted key
    // included: its listing does not make the name showable.
    let (warning, lines) = connect_lines(
        r#"{"endpoint": "http://192.168.1.5:11434", "allow_non_loopback": true, "model": "qwen3:4b"}"#,
        Local::Lists("qwen3:4b"),
    );
    assert_eq!(warning, None);
    assert_eq!(
        lines,
        vec!["Narrator: Fireside, Brief, model the configured model.".to_string()]
    );
    Case::new(
        serde_json::json!({
            "endpoint": "http://192.168.1.5:11434",
            "allow_non_loopback": true,
            "model": NAME_SHAPED_KEY,
        }),
        NAME_SHAPED_KEY,
    )
    .local(Local::Lists(NAME_SHAPED_KEY))
    .check();
}

#[test]
fn a_claude_file_without_a_model_takes_the_claude_default() {
    use etv_core::narrator_provider::CLOUD_DEFAULT_MODEL;
    // No model named: the Claude default, and the file loads.
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"provider": "claude", "key": {"env": "ETV_SET"}}"#,
    )
    .unwrap();
    let loaded = load_settings_or_default(dir.path());
    assert_eq!(loaded.warning, None);
    assert!(loaded.from_file);
    assert_eq!(loaded.settings.model, CLOUD_DEFAULT_MODEL);
    // An explicit model the provider does not list is still refused, unshown.
    for model in ["qwen3:4b", NAME_SHAPED_KEY] {
        let body =
            serde_json::json!({"provider": "claude", "model": model, "key": {"env": "ETV_SET"}})
                .to_string();
        std::fs::write(&path, &body).unwrap();
        let loaded = load_settings_or_default(dir.path());
        let warning = loaded.warning.unwrap();
        assert!(
            warning.contains("the claude provider needs one of its models"),
            "{warning}"
        );
        assert!(!warning.contains(model), "{warning}");
        assert!(!loaded.from_file);
    }
    // Another provider without a model keeps the local default, as before.
    std::fs::write(&path, r#"{"provider": "ollama"}"#).unwrap();
    assert_eq!(
        load_settings_or_default(dir.path()).settings.model,
        etv_core::narrator::DEFAULT_NARRATOR_MODEL
    );
}

#[test]
fn the_options_screen_refuses_a_model_the_claude_provider_does_not_list() {
    use etv_core::screens::{run_settings_screen, SettingsEvent, SettingsExit, SettingsScreen};
    let claude = NarratorSettings::default().with_cloud();
    let mut screen = SettingsScreen::new(claude.clone());
    assert_eq!(screen.handle_line("4"), SettingsEvent::AskModel);
    let said = screen.handle_line(NAME_SHAPED_KEY);
    let SettingsEvent::Rejected(why) = said else {
        panic!("not refused: {said:?}");
    };
    assert_eq!(
        why,
        format!(
            "Model not changed: {}.",
            etv_core::narrator_provider::claude_model_refusal()
        )
    );
    assert!(!why.contains(NAME_SHAPED_KEY));
    assert_eq!(
        screen.settings.model, claude.model,
        "the model is unchanged"
    );
    // A listed model is taken.
    screen.handle_line("4");
    assert!(matches!(
        screen.handle_line("claude-opus-5-5"),
        SettingsEvent::Changed(_)
    ));
    assert_eq!(screen.settings.model, "claude-opus-5-5");

    // Typed and saved in line mode: what is saved is not the refused name.
    let mut out = Vec::new();
    let exit = run_settings_screen(
        claude.clone(),
        std::io::Cursor::new(format!("4\n{NAME_SHAPED_KEY}\ns\n")),
        &mut out,
    )
    .unwrap();
    let SettingsExit::Save(saved) = exit else {
        panic!("expected a save");
    };
    assert_eq!(saved.model, claude.model);
    assert_clean(
        "line mode's options screen",
        &String::from_utf8(out).unwrap(),
        NAME_SHAPED_KEY,
    );
}

// ---------------------------------------------------------------------------------------------
// Follow-ups from the #97 reviews: one loopback rule, real IPv6 and IPv4 hosts, `_` in host
// names, the endpoint path, provider_url checked whole, every hosted error redacted, and the
// options screen naming a model the local service lists.
// ---------------------------------------------------------------------------------------------

/// The settings a file gives, with its warning.
fn load(body: &str) -> etv_core::settings::LoadedSettings {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    load_settings_or_default(dir.path())
}

#[test]
fn loopback_hosts_load_by_default_and_are_shown_in_normal_form() {
    for (endpoint, shown) in [
        ("http://LOCALHOST:11434", "http://localhost:11434"),
        ("http://LocalHost:11434/api", "http://localhost:11434"),
        ("http://127.1.2.3:11434", "http://127.1.2.3:11434"),
        ("http://[0:0:0:0:0:0:0:1]:11434", "http://[::1]:11434"),
        ("https://[::1]:8443/", "https://[::1]:8443"),
    ] {
        let body = serde_json::json!({ "endpoint": endpoint }).to_string();
        let loaded = load(&body);
        assert_eq!(loaded.warning, None, "{endpoint} loads by default");
        assert!(loaded.from_file, "{endpoint}");
        assert!(
            etv_core::narrator::is_loopback_endpoint(endpoint),
            "{endpoint}"
        );
        assert_eq!(endpoint_for_display(endpoint), shown, "{endpoint}");
        let (_, lines) = connect_lines(&body, Local::Dead);
        assert_eq!(
            lines[0],
            format!("The narrator service did not answer at {shown}. Start it with: ollama serve"),
            "{endpoint}"
        );
    }
}

/// The 32-hex-digit made-up key used as a bracketed host.
const HEX_KEY: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn malformed_addresses_are_refused_with_fixed_text() {
    let refused = "The settings file was not used: the endpoint is not valid (a host, an optional numeric port and an optional path only; no user name, query or fragment). Using the default settings: the local narrator (Ollama), until the settings are fixed.";
    for endpoint in [
        "http://127.008.0.1:11434".to_string(),
        "http://127.0.0.999:11434".to_string(),
        "http://999.999.999.999:11434".to_string(),
        "http://[::::::]:11434".to_string(),
        "http://[.]:11434".to_string(),
        "http://[fe80::1%eth0]:11434".to_string(),
        format!("http://[{HEX_KEY}]:11434"),
        "http://127.0.0.1:0".to_string(),
        "http://127.0.0.1:".to_string(),
        "http://:11434".to_string(),
        "http://".to_string(),
    ] {
        for allow in [false, true] {
            let loaded = load(
                &serde_json::json!({ "endpoint": endpoint, "allow_non_loopback": allow })
                    .to_string(),
            );
            assert_eq!(
                loaded.warning.as_deref(),
                Some(refused),
                "{endpoint} (allow_non_loopback {allow})"
            );
            assert!(!loaded.from_file);
            assert!(!etv_core::narrator::is_loopback_endpoint(&endpoint));
            assert_eq!(endpoint_for_display(&endpoint), "the configured endpoint");
        }
    }
    // An IPv4-mapped address is not the IPv6 loopback: refused without the setting, and with
    // it loaded but never shown.
    let mapped = "http://[::ffff:127.0.0.1]:11434";
    assert!(!etv_core::narrator::is_loopback_endpoint(mapped));
    assert_eq!(endpoint_for_display(mapped), "the configured endpoint");
    let loaded = load(&serde_json::json!({ "endpoint": mapped }).to_string());
    assert!(loaded
        .warning
        .unwrap()
        .contains("the endpoint is not on this computer"));
    let loaded =
        load(&serde_json::json!({ "endpoint": mapped, "allow_non_loopback": true }).to_string());
    assert_eq!(loaded.warning, None);
    let (_, lines) = connect_lines(
        &serde_json::json!({ "endpoint": mapped, "allow_non_loopback": true }).to_string(),
        Local::Dead,
    );
    assert_eq!(
        lines[0],
        "The narrator service did not answer at the configured endpoint. Start it with: ollama serve"
    );
    // The bracketed key, through every line, the log and every screen.
    for allow in [false, true] {
        Case::new(
            serde_json::json!({
                "endpoint": format!("http://[{HEX_KEY}]:11434"),
                "allow_non_loopback": allow,
            }),
            HEX_KEY,
        )
        .check();
    }
}

#[test]
fn a_host_name_with_an_underscore_loads_when_allowed_and_is_never_shown() {
    let body = r#"{"endpoint": "http://ollama_box:11434", "allow_non_loopback": true}"#;
    let loaded = load(body);
    assert_eq!(loaded.warning, None);
    assert!(loaded.from_file);
    assert_eq!(loaded.settings.endpoint, "http://ollama_box:11434");
    assert_eq!(
        endpoint_for_display("http://ollama_box:11434"),
        "the configured endpoint"
    );
    let (_, lines) = connect_lines(body, Local::Dead);
    assert_eq!(
        lines[0],
        "The narrator service did not answer at the configured endpoint. Start it with: ollama serve"
    );
    // Not loopback, so refused without the setting.
    let loaded = load(r#"{"endpoint": "http://ollama_box:11434"}"#);
    assert!(loaded
        .warning
        .unwrap()
        .contains("the endpoint is not on this computer"));
    // A key-shaped host name with an underscore is never shown either.
    let key = "sk_FAKE_0123456789abcdefghij";
    Case::new(
        serde_json::json!({"endpoint": format!("http://{key}:11434"), "allow_non_loopback": true}),
        key,
    )
    .check();
}

#[test]
fn an_endpoint_path_with_whitespace_or_a_control_character_is_refused() {
    for endpoint in [
        "http://127.0.0.1:11434/a b",
        "http://127.0.0.1:11434/a\tb",
        "http://127.0.0.1:11434/a\u{7}b",
        "http://127.0.0.1:11434/a\u{a0}b",
    ] {
        let loaded = load(&serde_json::json!({ "endpoint": endpoint }).to_string());
        assert!(
            loaded
                .warning
                .as_deref()
                .is_some_and(|w| w.contains("the endpoint is not valid")),
            "{endpoint:?}: {:?}",
            loaded.warning
        );
    }
    // A path of ordinary characters still loads.
    let loaded = load(r#"{"endpoint": "http://127.0.0.1:11434/ollama/v1"}"#);
    assert_eq!(loaded.warning, None);
    // A key after a space in the path: refused, never shown.
    for key in [KEY, ODD_KEY] {
        Case::new(
            serde_json::json!({"endpoint": format!("http://127.0.0.1:11434/x {key}")}),
            key,
        )
        .check();
    }
}

#[test]
fn a_key_in_the_path_of_a_non_loopback_endpoint_is_not_shown() {
    for key in [KEY, ODD_KEY] {
        let body = serde_json::json!({
            "endpoint": format!("http://192.168.1.5:11434/{key}"),
            "allow_non_loopback": true,
        });
        // Allowed by the settings, and it loads.
        assert_eq!(load(&body.to_string()).warning, None);
        for local in [Local::Dead, Local::Listing, Local::Lists("qwen3:4b")] {
            Case::new(body.clone(), key).local(local).check();
        }
    }
}

#[test]
fn a_key_as_the_provider_urls_user_name_query_or_fragment_is_refused_unshown() {
    use etv_core::narrator_provider::PROVIDER_URL_REFUSAL;
    for key in [KEY, ODD_KEY] {
        for url in [
            format!("https://{key}@example.com/v1"),
            format!("https://user:{key}@example.com/v1"),
            format!("https://example.com/v1?key={key}"),
            format!("https://example.com/v1#{key}"),
            format!("https://example.com:{key}/v1"),
            format!("https://example.com/v1 {key}"),
        ] {
            let body = serde_json::json!({
                "provider": "openai-compatible",
                "model": "some-model",
                "provider_url": url,
                "key": {"env": "ETV_SET"},
            });
            let loaded = load(&body.to_string());
            assert_eq!(
                loaded.warning,
                Some(format!(
                    "The settings file was not used: {PROVIDER_URL_REFUSAL}. Using the default settings: the local narrator (Ollama), until the settings are fixed."
                )),
                "{url}"
            );
            let asked = Case::new(body, key).check();
            assert_eq!(asked, 0, "a refused provider_url is never asked: {url}");
        }
    }
    // An empty host, port 0, an empty port and an IPv6 host that is not an address.
    for url in [
        "https://",
        "https:///v1",
        "https://:443/v1",
        "https://example.com:0/v1",
        "https://example.com:/v1",
        "https://[::::::]/v1",
    ] {
        let loaded = load(
            &serde_json::json!({
                "provider": "openai-compatible",
                "model": "m",
                "provider_url": url,
                "key": {"env": "ETV_SET"},
            })
            .to_string(),
        );
        assert_eq!(
            loaded.warning,
            Some(format!(
                "The settings file was not used: {PROVIDER_URL_REFUSAL}. Using the default settings: the local narrator (Ollama), until the settings are fixed."
            )),
            "{url}"
        );
    }
    // A plain URL, with a port and a path, still loads.
    let loaded = load(
        r#"{"provider": "openai-compatible", "model": "m", "provider_url": "https://openrouter.ai:443/api/v1", "key": {"env": "ETV_SET"}}"#,
    );
    assert_eq!(loaded.warning, None);
}

/// An HTTPS client that fails every request with one kind of error, its text holding every
/// header it was sent (the key's header included).
struct EchoesHeaders(fn(String) -> TransportError);

impl HttpsPost for EchoesHeaders {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        _body: &str,
        _timeout: Duration,
    ) -> Result<HttpReply, TransportError> {
        Err((self.0)(format!("{url} {headers:?}")))
    }
}

#[test]
fn every_hosted_error_with_text_has_the_key_taken_out() {
    use etv_core::cloud_claude::ApiKey;
    use etv_core::narrator_provider::{HostedApi, HostedTransport};
    let kinds: [fn(String) -> TransportError; 2] = [TransportError::ConnectionFailed, |text| {
        TransportError::HttpStatus(500, text)
    }];
    let request = serde_json::json!({
        "model": "m",
        "system": "You narrate.",
        "prompt": "A turn.",
        "options": {"num_predict": 40},
    })
    .to_string();
    for api in [
        HostedApi::Claude,
        HostedApi::OpenaiCompatible {
            base_url: "https://example.com/v1".to_string(),
        },
    ] {
        for kind in kinds {
            let transport = HostedTransport::new(
                api.clone(),
                "claude-haiku-5-5",
                ApiKey::from_text(GOOD_KEY).unwrap(),
                Box::new(EchoesHeaders(kind)),
            );
            let err = transport
                .post_generate("", &request, Duration::from_secs(1))
                .unwrap_err();
            let text = format!("{err:?}");
            assert!(text.contains("[redacted]"), "{text}");
            assert_clean("a hosted error", &text, GOOD_KEY);
        }
    }
    // And through every line, the log and every screen, with the key's header echoed.
    Case::new(
        serde_json::json!({
            "provider": "openai-compatible",
            "model": "some-model",
            "provider_url": "https://example.com/v1",
            "key": {"env": "ETV_SET"},
        }),
        GOOD_KEY,
    )
    .echo(Echo::StatusError)
    .check();
}

/// The full-screen interface's model for `body`, connected over `local`, as `etv tui` builds it.
fn interface(dir: &std::path::Path, body: &str, local: Local) -> Model {
    let path = settings_path(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    let loaded = load_settings_or_default(dir);
    let connection = connect_narrator_with(&loaded.settings, |s| {
        narrator_transport(s, local.transport(), None, &|_| None)
    });
    let content = embedded_content_pack();
    let engine = StepEngine::with_content(create_new_run_with_content(42, content), content);
    Model::new(
        ViewData::from_engine(&engine),
        loaded.settings.clone(),
        "TEST",
        "HASH",
        connection.lines.clone(),
        false,
    )
    .with_installed(connection.installed.clone())
}

/// The model line of the options view as drawn.
fn drawn_model_line(model: &Model) -> String {
    draw(model, (120, 40))
        .lines()
        .find(|l| l.contains("4. Model"))
        .unwrap_or_default()
        .to_string()
}

fn app_for<'a>(model: Model, dir: &std::path::Path, local: Local) -> App<'a> {
    let content = embedded_content_pack();
    let engine = StepEngine::with_content(create_new_run_with_content(42, content), content);
    App::new(
        model,
        engine,
        &NullNarrator,
        Some(table()),
        dir.into(),
        false,
    )
    .with_reconnect(move |settings: &NarratorSettings| {
        Reconnected::from_connection(connect_narrator_with(settings, |s| {
            narrator_transport(s, local.transport(), None, &|_| None)
        }))
    })
}

#[test]
fn the_options_screen_names_a_model_the_local_service_lists() {
    // Loopback, local provider, the model listed at startup -> reconnect returns a different
    // listing that drops it: the reopened options view drops the old name.
    let dir = tempfile::tempdir().unwrap();
    let model = interface(
        dir.path(),
        r#"{"model": "qwen3:4b"}"#,
        Local::Lists("qwen3:4b"),
    );
    assert_eq!(model.log[0], "Narrator: Fireside, Brief, model qwen3:4b.");
    let mut app = app_for(model, dir.path(), Local::Lists("llama3.2:3b"));
    app.press_code(crossterm::event::KeyCode::Enter);
    app.press_char('o');
    assert!(app
        .model
        .settings
        .lines()
        .contains(&"  4. Model     qwen3:4b".to_string()));
    assert!(drawn_model_line(&app.model).contains("4. Model     qwen3:4b"));
    // After a save that reconnects, the new connection's listing replaces the startup one.
    app.press_char('2');
    app.press_char('s');
    app.press_char('o');
    let line = drawn_model_line(&app.model);
    assert!(line.contains("4. Model     the configured model"), "{line}");
    assert!(!line.contains("qwen3:4b"), "{line}");

    // And the opposite transition: missing at startup, listed by the reconnect service.
    let dir2 = tempfile::tempdir().unwrap();
    let model2 = interface(
        dir2.path(),
        r#"{"model": "llama3.2:3b"}"#,
        Local::Lists("qwen3:4b"),
    );
    let mut app2 = app_for(model2, dir2.path(), Local::Lists("llama3.2:3b"));
    app2.press_code(crossterm::event::KeyCode::Enter);
    app2.press_char('o');
    assert!(drawn_model_line(&app2.model).contains("4. Model     the configured model"));
    app2.press_char('2');
    app2.press_char('s');
    app2.press_char('o');
    assert!(drawn_model_line(&app2.model).contains("4. Model     llama3.2:3b"));

    // The model missing from the listing, or a remote service listing it: "the configured
    // model".
    for (body, local) in [
        (r#"{"model": "qwen3:4b"}"#, Local::Lists("llama3.2:3b")),
        (
            r#"{"model": "qwen3:4b", "endpoint": "http://192.168.1.5:11434", "allow_non_loopback": true}"#,
            Local::Lists("qwen3:4b"),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let model = interface(dir.path(), body, local);
        let mut app = app_for(model, dir.path(), local);
        app.press_code(crossterm::event::KeyCode::Enter);
        app.press_char('o');
        assert!(
            drawn_model_line(&app.model).contains("4. Model     the configured model"),
            "{body}"
        );
    }

    // The screen itself uses a listing only for the local provider on a loopback endpoint.
    use etv_core::screens::SettingsScreen;
    let listed = vec!["qwen3:4b".to_string()];
    let local = NarratorSettings {
        model: "qwen3:4b".to_string(),
        ..NarratorSettings::default()
    };
    let named = SettingsScreen::with_installed(local.clone(), listed.clone()).lines();
    assert!(named.contains(&"  4. Model     qwen3:4b".to_string()));
    let remote = NarratorSettings {
        endpoint: "http://192.168.1.5:11434".to_string(),
        allow_non_loopback: true,
        ..local.clone()
    };
    let hosted = NarratorSettings {
        provider: etv_core::narrator_provider::NarratorProvider::OpenaiCompatible,
        provider_url: "https://example.com/v1".to_string(),
        ..local.clone()
    };
    for settings in [remote, hosted] {
        let lines = SettingsScreen::with_installed(settings, listed.clone()).lines();
        assert!(lines.contains(&"  4. Model     the configured model".to_string()));
    }
    // With no listing it behaves as before.
    assert!(SettingsScreen::new(local)
        .lines()
        .contains(&"  4. Model     the configured model".to_string()));
}

/// Types `name` as the model on the options view (4, the name, Enter).
fn type_model(app: &mut App<'_>, name: &str) {
    app.press_char('4');
    for c in name.chars() {
        app.press_char(c);
    }
    app.press_code(crossterm::event::KeyCode::Enter);
}

#[test]
fn a_typed_model_is_unnamed_after_save_discard_and_reopen() {
    for typed in ["mistral:7b", PLAIN_KEY] {
        // Typed and saved: named while the view is open, "the configured model" once reopened.
        let dir = tempfile::tempdir().unwrap();
        let model = interface(
            dir.path(),
            r#"{"model": "xai-org/grok-1"}"#,
            Local::Lists("qwen3:4b"),
        );
        let mut app = app_for(model, dir.path(), Local::Lists("qwen3:4b"));
        app.press_code(crossterm::event::KeyCode::Enter);
        app.press_char('o');
        type_model(&mut app, typed);
        assert!(
            drawn_model_line(&app.model).contains(&format!("4. Model     {typed}")),
            "the typed name is shown back while the view is open"
        );
        app.press_char('s');
        assert_eq!(app.model.saved_settings.model, typed);
        assert_clean("the log after a save", &app.model.log.join("\n"), typed);
        assert_screen_clean("the screen after a save", &app.model, typed);
        app.press_char('o');
        assert!(
            drawn_model_line(&app.model).contains("4. Model     the configured model"),
            "after save and reopen: {}",
            drawn_model_line(&app.model)
        );
        assert_screen_clean("the options view reopened after a save", &app.model, typed);

        // Another name typed and discarded: the saved model again, unnamed.
        type_model(&mut app, "llama3.2:3b");
        app.press_char('q');
        app.press_char('o');
        assert!(
            drawn_model_line(&app.model).contains("4. Model     the configured model"),
            "after discard and reopen"
        );
        assert_eq!(app.model.settings.settings.model, typed);
        assert_screen_clean(
            "the options view reopened after a discard",
            &app.model,
            typed,
        );
    }

    // A key-shaped name typed, then discarded, over a saved model the service does not list.
    let dir = tempfile::tempdir().unwrap();
    let model = interface(
        dir.path(),
        r#"{"model": "xai-org/grok-1"}"#,
        Local::Lists("qwen3:4b"),
    );
    let mut app = app_for(model, dir.path(), Local::Lists("qwen3:4b"));
    app.press_code(crossterm::event::KeyCode::Enter);
    app.press_char('o');
    type_model(&mut app, PLAIN_KEY);
    app.press_char('q');
    app.press_char('o');
    assert!(drawn_model_line(&app.model).contains("4. Model     the configured model"));
    assert_eq!(app.model.settings.settings.model, "xai-org/grok-1");
    assert_screen_clean("the options view after a discard", &app.model, PLAIN_KEY);

    // A typed name that the service lists is named after a reopen, by the listing (a rule
    // names it).
    let dir = tempfile::tempdir().unwrap();
    let model = interface(dir.path(), "{}", Local::Lists("qwen3:4b"));
    let mut app = app_for(model, dir.path(), Local::Lists("qwen3:4b"));
    app.press_code(crossterm::event::KeyCode::Enter);
    app.press_char('o');
    type_model(&mut app, "qwen3:4b");
    app.press_char('s');
    app.press_char('o');
    assert!(drawn_model_line(&app.model).contains("4. Model     qwen3:4b"));
}

#[test]
fn the_one_line_model_command() {
    use etv_core::screens::{run_settings_screen, SettingsEvent, SettingsExit, SettingsScreen};
    // An accepted name: changed, and shown back while the screen is open.
    let mut screen = SettingsScreen::new(NarratorSettings::default());
    assert_eq!(
        screen.handle_line("model mistral:7b"),
        SettingsEvent::Changed("Model is now mistral:7b.".to_string())
    );
    assert!(screen
        .lines()
        .contains(&"  4. Model     mistral:7b".to_string()));
    // A refused character: one character named, never the name, and the model unchanged.
    for name in [format!("{KEY}!"), format!("{PLAIN_KEY}$")] {
        let mut screen = SettingsScreen::new(NarratorSettings::default());
        let said = screen.handle_line(&format!("model {name}"));
        let SettingsEvent::Rejected(why) = said else {
            panic!("not refused: {said:?}");
        };
        assert!(
            why.starts_with("Model not changed: the model name contains "),
            "{why}"
        );
        assert_clean("a refused model command", &why, &name);
        assert_eq!(
            screen.settings.model,
            etv_core::narrator::DEFAULT_NARRATOR_MODEL
        );
    }
    // A name the claude provider does not list: refused with fixed text, unshown.
    let claude = NarratorSettings::default().with_cloud();
    for name in ["mistral:7b", NAME_SHAPED_KEY, PLAIN_KEY] {
        let mut screen = SettingsScreen::new(claude.clone());
        let said = screen.handle_line(&format!("model {name}"));
        assert_eq!(
            said,
            SettingsEvent::Rejected(format!(
                "Model not changed: {}.",
                etv_core::narrator_provider::claude_model_refusal()
            ))
        );
        assert_eq!(screen.settings.model, claude.model);
        // In line mode, saved: the refused name is neither shown nor saved.
        let mut out = Vec::new();
        let exit = run_settings_screen(
            claude.clone(),
            std::io::Cursor::new(format!("model {name}\ns\n")),
            &mut out,
        )
        .unwrap();
        let SettingsExit::Save(saved) = exit else {
            panic!("expected a save");
        };
        assert_eq!(saved.model, claude.model);
        assert_clean(
            "line mode's options screen",
            &String::from_utf8(out).unwrap(),
            name,
        );
    }
    // Through every line, the log and every screen, with a key-shaped model in the file.
    Case::new(serde_json::json!({"model": PLAIN_KEY}), PLAIN_KEY)
        .local(Local::Lists("qwen3:4b"))
        .check();
}

/// A made-up key of lower-case letters and `-` only, the shape the narration checker passes.
const LOWER_KEY: &str = "test-key-not-real-abcdefghijxyz";

#[test]
fn a_key_escaped_in_a_hosted_reply_is_taken_out_after_decoding() {
    use etv_core::cloud_claude::ApiKey;
    use etv_core::narrator_provider::{HostedApi, HostedTransport};
    struct Replies(String);
    impl HttpsPost for Replies {
        fn post(
            &self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &str,
            _timeout: Duration,
        ) -> Result<HttpReply, TransportError> {
            Ok(chat_reply(&self.0))
        }
    }
    let request = serde_json::json!({
        "model": "m",
        "system": "You narrate.",
        "prompt": "A turn.",
        "options": {"num_predict": 40},
    })
    .to_string();
    // A key of lower-case letters, and one with digits and capitals: what the transport hands
    // the narrator holds neither, once its JSON escapes are decoded.
    for key in [LOWER_KEY, GOOD_KEY] {
        let transport = HostedTransport::new(
            HostedApi::OpenaiCompatible {
                base_url: "https://example.com/v1".to_string(),
            },
            "some-model",
            ApiKey::from_text(key).unwrap(),
            Box::new(Replies(escaped_key_narration(key))),
        );
        let (status, body) = transport
            .post_generate("", &request, Duration::from_secs(1))
            .unwrap();
        assert_eq!(status, 200);
        let outer: serde_json::Value = serde_json::from_str(&body).unwrap();
        let inner: serde_json::Value =
            serde_json::from_str(outer["response"].as_str().unwrap()).unwrap();
        let decoded = inner["sentences"][0].as_str().unwrap();
        assert!(decoded.contains("[redacted]"), "{decoded}");
        assert_clean("the decoded narration", decoded, key);
        assert_clean("the transport's answer", &body, key);
    }
    // Through every line, the log and every screen: the narration checker would pass the
    // lower-case key, so only the redaction keeps it off the screen.
    for key in [LOWER_KEY, GOOD_KEY] {
        let asked = Case::new(
            serde_json::json!({
                "provider": "openai-compatible",
                "model": "some-model",
                "provider_url": "https://example.com/v1",
                "key": {"env": "ETV_SET"},
            }),
            key,
        )
        .env("ETV_SET", key)
        .echo(Echo::EscapedKeyReply)
        .check();
        assert!(asked > 0, "the hosted client was called");
    }
}

#[test]
fn a_json_shaped_reply_that_fails_to_parse_cannot_pass_an_escaped_key_through() {
    use etv_core::cloud_claude::ApiKey;
    use etv_core::narrator_provider::{HostedApi, HostedTransport};
    struct Replies(String);
    impl HttpsPost for Replies {
        fn post(
            &self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &str,
            _timeout: Duration,
        ) -> Result<HttpReply, TransportError> {
            Ok(chat_reply(&self.0))
        }
    }
    let request = serde_json::json!({
        "model": "m",
        "system": "You narrate.",
        "prompt": "A turn.",
        "options": {"num_predict": 40},
    })
    .to_string();
    for key in [LOWER_KEY, GOOD_KEY] {
        let escaped = escaped_first_char_key(key);
        let huge_number =
            format!(r#"{{"extra": 1e400, "sentences": ["Rain beads near {escaped}."]}}"#);
        let bad_object =
            format!(r#"{{"extra": NaN, "sentences": ["Rain beads near {escaped}."]}}"#);
        let bad_array = format!(r#"["Rain beads near {escaped}""#);

        // `1e400` with arbitrary_precision parses and redacts; without it, fails closed to "".
        // In either case neither the key nor its escaped form can remain in the response.
        let transport = HostedTransport::new(
            HostedApi::OpenaiCompatible {
                base_url: "https://example.com/v1".to_string(),
            },
            "some-model",
            ApiKey::from_text(key).unwrap(),
            Box::new(Replies(huge_number)),
        );
        let (status, body) = transport
            .post_generate("", &request, Duration::from_secs(1))
            .unwrap();
        assert_eq!(status, 200);
        let outer: serde_json::Value = serde_json::from_str(&body).unwrap();
        let resp = outer["response"].as_str().unwrap();
        assert!(resp.is_empty() || resp.contains("[redacted]"), "{resp}");
        assert!(!resp.contains(&escaped), "{resp}");
        assert!(!body.contains(&escaped), "{body}");
        assert_clean("the transport's 1e400 answer", &body, key);

        // Unparseable JSON starting with `{` or `[` fails closed to an empty response.
        for bad in [bad_object, bad_array] {
            let transport = HostedTransport::new(
                HostedApi::OpenaiCompatible {
                    base_url: "https://example.com/v1".to_string(),
                },
                "some-model",
                ApiKey::from_text(key).unwrap(),
                Box::new(Replies(bad)),
            );
            let (status, body) = transport
                .post_generate("", &request, Duration::from_secs(1))
                .unwrap();
            assert_eq!(status, 200);
            let outer: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(outer["response"], "");
            assert!(!body.contains(&escaped), "{body}");
            assert_clean("the transport's unparseable JSON answer", &body, key);
        }

        // End-to-end through `play_turn` and the screens.
        for echo in [
            Echo::HugeNumberEscapedKeyReply,
            Echo::MalformedJsonEscapedKeyReply,
        ] {
            let asked = Case::new(
                serde_json::json!({
                    "provider": "openai-compatible",
                    "model": "some-model",
                    "provider_url": "https://example.com/v1",
                    "key": {"env": "ETV_SET"},
                }),
                key,
            )
            .env("ETV_SET", key)
            .echo(echo)
            .check();
            assert!(asked > 0, "the hosted client was called");
        }
    }
}

#[test]
fn key_redaction_catches_case_changes_and_substrings_at_the_threshold() {
    use etv_core::cloud_claude::{ApiKey, REDACT_SUBSTRING_MIN_CHARS};
    use etv_core::narrator_provider::{HostedApi, HostedTransport};

    struct FixedReply(String);
    impl HttpsPost for FixedReply {
        fn post(
            &self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &str,
            _timeout: Duration,
        ) -> Result<HttpReply, TransportError> {
            Ok(chat_reply(&self.0))
        }
    }

    let request = serde_json::json!({
        "model": "m",
        "system": "You narrate.",
        "prompt": "A turn.",
        "options": {"num_predict": 40},
    })
    .to_string();

    let upper_key = LOWER_KEY.to_ascii_uppercase();
    let lower_key = LOWER_KEY.to_string();
    let at_threshold = &LOWER_KEY[..REDACT_SUBSTRING_MIN_CHARS];
    let below_threshold = &LOWER_KEY[..REDACT_SUBSTRING_MIN_CHARS - 1];

    // 1. Upper-cased stored key vs lower-cased reply, and lower-cased stored key vs upper-cased
    //    reply: both are redacted at the transport and never reach `play_turn` output.
    for (stored_key, echoed_text) in [(&upper_key, &lower_key), (&lower_key, &upper_key)] {
        let narration =
            format!(r#"{{"sentences": ["Rain beads on the canvas near {echoed_text}."]}}"#);
        let transport = HostedTransport::new(
            HostedApi::OpenaiCompatible {
                base_url: "https://example.com/v1".to_string(),
            },
            "some-model",
            ApiKey::from_text(stored_key).unwrap(),
            Box::new(FixedReply(narration.clone())),
        );
        let (status, body) = transport
            .post_generate("", &request, Duration::from_secs(1))
            .unwrap();
        assert_eq!(status, 200);
        let outer: serde_json::Value = serde_json::from_str(&body).unwrap();
        let resp = outer["response"].as_str().unwrap();
        assert!(resp.contains("[redacted]"), "{resp}");
        assert!(!resp.to_ascii_lowercase().contains(&lower_key), "{resp}");

        let settings = NarratorSettings {
            provider: etv_core::narrator_provider::NarratorProvider::OpenaiCompatible,
            provider_url: "https://example.com/v1".to_string(),
            model: "some-model".to_string(),
            key: etv_core::narrator_provider::KeySource {
                env: Some("ETV_SET".to_string()),
                file: None,
            },
            ..NarratorSettings::default()
        };
        let env_val = stored_key.clone();
        let connection = connect_narrator_with(&settings, |s| {
            narrator_transport(s, None, Some(Box::new(FixedReply(narration))), &|name| {
                (name == "ETV_SET").then(|| env_val.clone())
            })
        });
        let live = connection.narrator.expect("hosted narrator connected");
        let content = embedded_content_pack();
        let mut engine =
            StepEngine::with_content(create_new_run_with_content(42, content), content);
        let shown = play_turn(
            &mut engine,
            &live,
            Some(table()),
            &PlayerIntent::new(IntentAction::Travel),
            None,
        )
        .join("\n");
        assert!(
            !shown.to_ascii_lowercase().contains(&lower_key),
            "turn output leaked case-changed key: {shown}"
        );
    }

    // 2. Contiguous substring at threshold is redacted; one char shorter is left alone.
    for (snippet, should_redact) in [(at_threshold, true), (below_threshold, false)] {
        let narration = format!(r#"{{"sentences": ["Rain beads on the canvas near {snippet}."]}}"#);
        let transport = HostedTransport::new(
            HostedApi::OpenaiCompatible {
                base_url: "https://example.com/v1".to_string(),
            },
            "some-model",
            ApiKey::from_text(LOWER_KEY).unwrap(),
            Box::new(FixedReply(narration)),
        );
        let (status, body) = transport
            .post_generate("", &request, Duration::from_secs(1))
            .unwrap();
        assert_eq!(status, 200);
        let outer: serde_json::Value = serde_json::from_str(&body).unwrap();
        let resp = outer["response"].as_str().unwrap();
        if should_redact {
            assert!(resp.contains("[redacted]"), "{resp}");
            assert!(!resp.contains(snippet), "{resp}");
        } else {
            assert!(!resp.contains("[redacted]"), "{resp}");
            assert!(resp.contains(snippet), "{resp}");
        }
    }
}
