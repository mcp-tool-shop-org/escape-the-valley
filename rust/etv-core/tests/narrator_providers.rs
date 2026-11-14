//! Narrator providers: the local default, Claude as the cloud default, and an OpenAI-compatible
//! client (OpenRouter and the like). Every test uses a scripted HTTPS client; no live key and no
//! network call anywhere in this file.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use etv_core::cloud_claude::{HttpReply, HttpsPost, CLAUDE_MESSAGES_URL};
use etv_core::narrator::{
    NarratorLength, NarratorSettings, NarratorTransport, TransportError, DEFAULT_NARRATOR_MODEL,
};
use etv_core::narrator_provider::{
    build_chat_request, claude_model_description, hosted_transport, validate_provider, KeySource,
    NarratorProvider, CLAUDE_MODELS, CLOUD_DEFAULT_MODEL, OPENROUTER_BASE_URL,
};
use serde_json::{json, Value};

/// A made-up key: long enough to pass the shape check, and found nowhere else.
const FAKE_KEY: &str = "test-key-not-real-0123456789abcdefXYZ";

type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>, String)>>>;

/// Answers every request with `status` and `body`, and records what was sent.
struct Scripted {
    status: u16,
    body: String,
    seen: Seen,
}

impl HttpsPost for Scripted {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
        _timeout: Duration,
    ) -> Result<HttpReply, TransportError> {
        self.seen.lock().unwrap().push((
            url.to_string(),
            headers
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
            body.to_string(),
        ));
        Ok(HttpReply {
            status: self.status,
            body: self.body.clone(),
            request_id: None,
            retry_after_s: None,
        })
    }
}

fn scripted(status: u16, body: Value) -> (Box<dyn HttpsPost>, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    (
        Box::new(Scripted {
            status,
            body: body.to_string(),
            seen: seen.clone(),
        }),
        seen,
    )
}

fn env(name: &str) -> Option<String> {
    (name == "ETV_TEST_KEY").then(|| FAKE_KEY.to_string())
}

fn hosted(provider: NarratorProvider, model: &str, url: &str) -> NarratorSettings {
    NarratorSettings {
        provider,
        model: model.to_string(),
        provider_url: url.to_string(),
        key: KeySource {
            env: Some("ETV_TEST_KEY".to_string()),
            file: None,
        },
        ..NarratorSettings::default()
    }
}

fn ollama_body() -> String {
    json!({
        "model": "whatever-the-narrator-names",
        "system": "You are the narrator.",
        "prompt": "Generate a scene response.",
        "stream": false,
        "think": false,
        "format": { "type": "object" },
        "options": { "num_predict": 110, "temperature": 0.7, "top_p": 0.8, "top_k": 20, "seed": 7, "num_ctx": 2048 }
    })
    .to_string()
}

#[test]
fn the_default_is_local_ollama_at_brief_and_cloud_preselects_claude() {
    let d = NarratorSettings::default();
    assert_eq!(d.provider, NarratorProvider::Ollama);
    assert_eq!(d.length, NarratorLength::Brief);
    assert_eq!(d.model, DEFAULT_NARRATOR_MODEL);
    assert_eq!(d.model, "qwen3:4b-instruct-2507-q4_K_M");
    assert!(d.key.is_empty());
    let cloud = d.with_cloud();
    assert_eq!(cloud.provider, NarratorProvider::Claude);
    assert_eq!(cloud.model, CLOUD_DEFAULT_MODEL);
    assert_eq!(cloud.length, NarratorLength::Brief);
    // The local provider makes no hosted transport.
    let (http, _) = scripted(200, json!({}));
    assert!(hosted_transport(&d, http, &env).unwrap().is_none());
}

#[test]
fn a_hosted_provider_needs_a_key_source_and_an_https_url() {
    let mut s = hosted(NarratorProvider::Claude, "claude-haiku-5-5", "");
    assert_eq!(validate_provider(&s), Ok(()));
    s.key = KeySource::default();
    assert!(validate_provider(&s).unwrap_err().contains("needs a key"));
    let mut s = hosted(
        NarratorProvider::OpenaiCompatible,
        "m",
        "http://192.168.1.5:11434/v1",
    );
    assert!(validate_provider(&s).unwrap_err().contains("https://"));
    s.provider_url = OPENROUTER_BASE_URL.to_string();
    assert_eq!(validate_provider(&s), Ok(()));
    // A variable that is not set is named; the key is never in an error.
    let s = NarratorSettings {
        key: KeySource {
            env: Some("ETV_NOT_SET".to_string()),
            file: None,
        },
        ..s
    };
    let (http, _) = scripted(200, json!({}));
    let err = hosted_transport(&s, http, &env).unwrap_err();
    assert_eq!(err, "the environment variable ETV_NOT_SET is not set");
}

#[test]
fn the_settings_store_only_where_the_key_is_never_the_key() {
    let s = hosted(NarratorProvider::Claude, "claude-haiku-5-5", "");
    let (http, _) = scripted(200, json!({}));
    let transport = hosted_transport(&s, http, &env).unwrap().unwrap();
    let saved = serde_json::to_string(&s).unwrap();
    assert!(saved.contains("\"env\":\"ETV_TEST_KEY\""));
    assert!(!saved.contains(FAKE_KEY));
    // Settings read back are the same; the key is not part of them.
    assert_eq!(NarratorSettings::from_json_str(&saved).unwrap(), s);
    // Debug output of the transport (what a log would show) carries no key.
    assert!(!format!("{transport:?}").contains(FAKE_KEY));
}

#[test]
fn claude_gets_the_messages_api_with_the_key_in_its_header_only() {
    let s = hosted(NarratorProvider::Claude, "claude-haiku-5-5", "");
    let answer = json!({
        "content": [{ "type": "text", "text": "{\"sentences\":[\"Mud holds the wheels.\"]}" }],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 10, "output_tokens": 5 }
    });
    let (http, seen) = scripted(200, answer);
    let t = hosted_transport(&s, http, &env).unwrap().unwrap();
    let (status, body) = t
        .post_generate("", &ollama_body(), Duration::from_secs(5))
        .unwrap();
    assert_eq!(status, 200);
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        body["response"],
        "{\"sentences\":[\"Mud holds the wheels.\"]}"
    );
    let seen = seen.lock().unwrap();
    let (url, headers, request) = &seen[0];
    assert_eq!(url, CLAUDE_MESSAGES_URL);
    assert!(headers
        .iter()
        .any(|(n, v)| n == "x-api-key" && v == FAKE_KEY));
    assert!(!request.contains(FAKE_KEY));
    let request: Value = serde_json::from_str(request).unwrap();
    assert_eq!(request["model"], "claude-haiku-5-5");
    assert_eq!(
        request["messages"][0]["content"],
        "Generate a scene response."
    );
}

#[test]
fn openrouter_gets_chat_completions_with_its_routing_settings() {
    let s = hosted(
        NarratorProvider::OpenaiCompatible,
        "mistralai/ministral-8b-2512",
        "https://openrouter.ai/api/v1/",
    );
    let answer = json!({ "choices": [{ "message": { "content": "Rain beads on the canvas." } }] });
    let (http, seen) = scripted(200, answer);
    let t = hosted_transport(&s, http, &env).unwrap().unwrap();
    let (_, body) = t
        .post_generate("", &ollama_body(), Duration::from_secs(5))
        .unwrap();
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["response"], "Rain beads on the canvas.");
    let seen = seen.lock().unwrap();
    let (url, headers, request) = &seen[0];
    assert_eq!(url, "https://openrouter.ai/api/v1/chat/completions");
    assert!(headers
        .iter()
        .any(|(n, v)| n == "authorization" && *v == format!("Bearer {FAKE_KEY}")));
    let request: Value = serde_json::from_str(request).unwrap();
    assert_eq!(request["model"], "mistralai/ministral-8b-2512");
    assert_eq!(
        request["messages"][0],
        json!({ "role": "system", "content": "You are the narrator." })
    );
    assert_eq!(request["max_tokens"], 110);
    assert_eq!(request["temperature"], json!(0.7));
    assert_eq!(request["seed"], 7);
    assert!(request.get("top_k").is_none() && request.get("num_ctx").is_none());
    assert_eq!(
        request["provider"],
        json!({ "data_collection": "deny", "allow_fallbacks": false, "require_parameters": true })
    );
    assert_eq!(request["response_format"]["type"], "json_schema");
}

#[test]
fn another_openai_compatible_host_gets_no_openrouter_routing() {
    let ollama: Value = serde_json::from_str(&ollama_body()).unwrap();
    let body = build_chat_request(&ollama, "m", false).unwrap();
    assert!(body.get("provider").is_none());
}

#[test]
fn an_auth_or_billing_answer_switches_the_provider_off_without_retrying() {
    for status in [401u16, 402, 403] {
        let s = hosted(NarratorProvider::Claude, "claude-haiku-5-5", "");
        let (http, seen) = scripted(
            status,
            json!({ "error": { "type": "authentication_error" } }),
        );
        let t = hosted_transport(&s, http, &env).unwrap().unwrap();
        let (got, _) = t
            .post_generate("", &ollama_body(), Duration::from_secs(5))
            .unwrap();
        assert_eq!(got, status);
        assert!(t.is_switched_off());
        assert!(t
            .post_generate("", &ollama_body(), Duration::from_secs(5))
            .is_err());
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "no second call after HTTP {status}"
        );
    }
    // A busy answer falls back for that line only.
    let s = hosted(NarratorProvider::Claude, "claude-haiku-5-5", "");
    let (http, seen) = scripted(529, json!({}));
    let t = hosted_transport(&s, http, &env).unwrap().unwrap();
    assert_eq!(
        t.post_generate("", &ollama_body(), Duration::from_secs(5))
            .unwrap()
            .0,
        529
    );
    assert!(!t.is_switched_off());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn an_answer_that_echoes_the_key_has_it_stripped() {
    let s = hosted(
        NarratorProvider::OpenaiCompatible,
        "m",
        "https://example.invalid/v1",
    );
    let answer = json!({ "choices": [{ "message": { "content": format!("echo {FAKE_KEY}") } }] });
    let (http, _) = scripted(200, answer);
    let t = hosted_transport(&s, http, &env).unwrap().unwrap();
    let (_, body) = t
        .post_generate("", &ollama_body(), Duration::from_secs(5))
        .unwrap();
    assert!(!body.contains(FAKE_KEY));
}

#[test]
fn every_claude_model_has_a_settings_line_and_the_default_is_haiku() {
    assert_eq!(CLOUD_DEFAULT_MODEL, "claude-haiku-5-5");
    assert_eq!(
        CLAUDE_MODELS.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [
            "claude-haiku-5-5",
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-fable-5-2"
        ]
    );
    assert!(claude_model_description(CLOUD_DEFAULT_MODEL)
        .unwrap()
        .starts_with("Haiku 5.5 (default): about 4 s a turn; about $0.04 a game;"));
    assert_eq!(
        claude_model_description("claude-fable-5-2"),
        Some("Fable 5.2: about 3.5 s a turn; about $3.60 a game; the best prose we measured.")
    );
    assert_eq!(claude_model_description("some-other-model"), None);
}

#[test]
fn the_settings_screen_shows_the_cloud_provider_and_the_models_line_and_nothing_for_local() {
    use etv_core::screens::settings_screen_lines;
    let local = settings_screen_lines(&NarratorSettings::default());
    assert!(!local.iter().any(|l| l.contains("Provider")));
    let cloud = settings_screen_lines(&NarratorSettings::default().with_cloud());
    assert!(cloud.contains(&"     Provider  claude".to_string()));
    assert!(cloud.contains(&format!(
        "     {}",
        claude_model_description("claude-haiku-5-5").unwrap()
    )));
}
