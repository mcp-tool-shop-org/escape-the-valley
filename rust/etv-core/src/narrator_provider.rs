//! Narrator providers for play: the local Ollama server (the default), Claude, and any
//! OpenAI-compatible endpoint (OpenRouter, a LAN server, another Ollama host's `/v1`).
//!
//! Every provider sits behind the same [`NarratorTransport`], so the prompts, the checker, the
//! candidates and the fallback line are the same whichever is chosen. A hosted provider gets the
//! same Ollama-shaped request the local model gets and translates it to its own API and back.
//!
//! The key for a hosted provider is read from an environment variable or a key file the player
//! names in the settings. Only the variable's name or the file's path is stored. The key itself is
//! held in an [`ApiKey`], which cannot be serialised, has no `Display` and shows nothing in
//! `Debug`, and every error or answer text this module returns has the key stripped out. So the
//! key cannot reach a save, a log, a replay or the narration cache.
//!
//! A hosted call that fails for any reason makes the line fall back to the game's own text. An
//! authentication, permission or billing answer (401, 402, 403) switches the provider off for
//! the rest of the session instead of retrying.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cloud_claude::{
    build_messages_request, is_warmup, response_text, ApiKey, HttpsPost, ANTHROPIC_VERSION,
    CLAUDE_MESSAGES_URL,
};
use crate::narrator::{NarratorTransport, TransportError};

/// Which service writes the narration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NarratorProvider {
    /// The local Ollama server (or another Ollama host the player allows). The default.
    #[default]
    Ollama,
    /// Anthropic's Claude API, on the player's own key. The preselected cloud provider.
    Claude,
    /// Any endpoint that speaks the OpenAI chat-completions API: OpenRouter, a LAN server, or
    /// an Ollama host's `/v1`.
    OpenaiCompatible,
}

impl NarratorProvider {
    /// The name as it is typed in the settings.
    pub fn name(self) -> &'static str {
        match self {
            NarratorProvider::Ollama => "ollama",
            NarratorProvider::Claude => "claude",
            NarratorProvider::OpenaiCompatible => "openai-compatible",
        }
    }

    /// `true` for the local default (used to leave it out of the settings file).
    pub fn is_local(&self) -> bool {
        matches!(self, NarratorProvider::Ollama)
    }

    /// `true` for a provider that needs a key.
    pub fn needs_key(self) -> bool {
        !matches!(self, NarratorProvider::Ollama)
    }
}

/// The provider and model a player gets on turning the cloud narrator on: Claude, on the
/// smallest model (the measured cost per line is the lowest; see the narrator records).
pub const CLOUD_DEFAULT_PROVIDER: NarratorProvider = NarratorProvider::Claude;
pub const CLOUD_DEFAULT_MODEL: &str = "claude-haiku-5-5";

/// The Claude models a player can pick, each with the one line the settings show. The figures
/// are measured in the studio's cloud narrator run of 2026-11-13: per-turn time is the median,
/// and the cost per game assumes a 200-line game. The model is a plain setting; this table only
/// describes the choices.
pub const CLAUDE_MODELS: [(&str, &str); 4] = [
    (
        "claude-haiku-5-5",
        "Haiku 5.5 (default): about 4 s a turn; about $0.04 a game; prose level with the local narrator.",
    ),
    (
        "claude-sonnet-5-5",
        "Sonnet 5.5: about 5.5 s a turn; about $0.55 a game; somewhat better prose than local (measured at Brief only).",
    ),
    (
        "claude-opus-5-5",
        "Opus 5.5: about 7.6 s a turn; about $2.15 a game; better prose than local (measured at Standard).",
    ),
    (
        "claude-fable-5-2",
        "Fable 5.2: about 3.5 s a turn; about $3.60 a game; the best prose we measured.",
    ),
];

/// The settings line for a Claude model, or `None` for a model the table does not list.
pub fn claude_model_description(model: &str) -> Option<&'static str> {
    CLAUDE_MODELS
        .iter()
        .find(|(id, _)| *id == model)
        .map(|(_, line)| *line)
}

/// The OpenRouter base URL. Requests to it carry OpenRouter's routing settings: no data
/// collection, no fallback to another model, and only endpoints that honour every parameter.
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Where a hosted provider's key comes from. Only these names are ever stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySource {
    /// The name of an environment variable that holds the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    /// The path of a file that holds the key (outside any git work tree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

impl KeySource {
    /// `true` when neither a variable nor a file is named.
    pub fn is_empty(&self) -> bool {
        self.env.is_none() && self.file.is_none()
    }

    /// Reads the key: from the environment variable when one is named, else from the file.
    /// `lookup` reads a variable (the process environment in play; a table in tests).
    ///
    /// Errors never carry the key, and never carry the variable's name or the key file's path:
    /// a key may have been pasted where a name belongs, and some keys look like valid names. Each
    /// error says what is wrong with "the environment variable named in the settings" or "the
    /// key file". Any name is looked up, however long or unusual; only an empty name, or one
    /// holding `=` or NUL (which no environment variable can have), is refused, unread.
    pub fn resolve(&self, lookup: &dyn Fn(&str) -> Option<String>) -> Result<ApiKey, String> {
        if let Some(name) = &self.env {
            if name.is_empty() || name.contains(['=', '\0']) {
                return Err(
                    "the environment variable named in the settings cannot be a variable's name"
                        .to_string(),
                );
            }
            let value = lookup(name).ok_or_else(|| {
                "the environment variable named in the settings is not set".to_string()
            })?;
            return ApiKey::from_text(&value)
                .map_err(|why| format!("the environment variable named in the settings {why}"));
        }
        if let Some(path) = &self.file {
            return ApiKey::read_from_file(std::path::Path::new(path));
        }
        Err(
            "no key is named: set the key's environment variable or key file in the settings"
                .to_string(),
        )
    }
}

/// The API a hosted transport speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedApi {
    /// `POST https://api.anthropic.com/v1/messages`.
    Claude,
    /// `POST <base_url>/chat/completions`.
    OpenaiCompatible { base_url: String },
}

/// Translates one Ollama `/api/generate` body into an OpenAI chat-completions body.
///
/// The system prompt becomes the system message and the prompt the one user message;
/// `num_predict` becomes `max_tokens`; temperature, top_p and the seed are kept (the API takes
/// them); `stop` is kept; `format` becomes a JSON-schema `response_format`. Options with no
/// counterpart (context size, GPU layers, top_k) are left out. For OpenRouter the routing
/// settings are added.
pub fn build_chat_request(ollama: &Value, model: &str, openrouter: bool) -> Result<Value, String> {
    let prompt = ollama
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or_else(|| "the request has no prompt".to_string())?;
    let options = ollama.get("options").cloned().unwrap_or_else(|| json!({}));
    let max_tokens = options
        .get("num_predict")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or_else(|| "the request has no num_predict".to_string())?;
    let mut messages = Vec::new();
    if let Some(system) = ollama.get("system").and_then(Value::as_str) {
        messages.push(json!({ "role": "system", "content": system }));
    }
    messages.push(json!({ "role": "user", "content": prompt }));
    let mut body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens,
        "stream": false,
    });
    for key in ["temperature", "top_p", "seed"] {
        if let Some(v) = options.get(key) {
            body[key] = v.clone();
        }
    }
    if let Some(stop) = options.get("stop").and_then(Value::as_array) {
        if !stop.is_empty() {
            body["stop"] = Value::Array(stop.clone());
        }
    }
    if let Some(format) = ollama.get("format") {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": { "name": "narration", "schema": format, "strict": true },
        });
    }
    if openrouter {
        body["provider"] = json!({
            "data_collection": "deny",
            "allow_fallbacks": false,
            "require_parameters": true,
        });
    }
    Ok(body)
}

/// The text of a chat-completions answer: the first choice's message content.
pub fn chat_response_text(response: &Value) -> String {
    response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

/// A hosted provider for play: one request per line, no retry, the key never shown.
pub struct HostedTransport {
    api: HostedApi,
    model: String,
    key: ApiKey,
    http: Box<dyn HttpsPost>,
    /// Set on an authentication, permission or billing answer: no further call is made.
    switched_off: AtomicBool,
}

impl std::fmt::Debug for HostedTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedTransport")
            .field("api", &self.api)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl HostedTransport {
    pub fn new(api: HostedApi, model: &str, key: ApiKey, http: Box<dyn HttpsPost>) -> Self {
        Self {
            api,
            model: model.to_string(),
            key,
            http,
            switched_off: AtomicBool::new(false),
        }
    }

    /// `true` once an authentication, permission or billing answer has switched it off.
    pub fn is_switched_off(&self) -> bool {
        self.switched_off.load(Ordering::SeqCst)
    }

    fn url(&self) -> String {
        match &self.api {
            HostedApi::Claude => CLAUDE_MESSAGES_URL.to_string(),
            HostedApi::OpenaiCompatible { base_url } => {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            }
        }
    }
}

impl NarratorTransport for HostedTransport {
    /// Answered locally: the one model this provider was set to, in the shape the narrator
    /// reads at start-up.
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        let body = json!({ "models": [{ "name": self.model, "details": { "quantization_level": "hosted" } }] });
        Ok((200, body.to_string()))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        body_json: &str,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        if self.is_switched_off() {
            return Err(TransportError::ConnectionFailed(
                "the cloud narrator is switched off for this session".to_string(),
            ));
        }
        let ollama: Value = serde_json::from_str(body_json)
            .map_err(|_| TransportError::ConnectionFailed("request is not JSON".to_string()))?;
        if is_warmup(&ollama) {
            return Ok((200, json!({ "response": "" }).to_string()));
        }
        // The narrator names its own model in the request; the provider's model is used.
        let mut ollama = ollama;
        ollama["model"] = json!(self.model);
        let (body, headers): (Value, Vec<(&str, String)>) = match &self.api {
            HostedApi::Claude => {
                let built = build_messages_request(&ollama, &self.model, false, None)
                    .map_err(TransportError::ConnectionFailed)?;
                (
                    built.body,
                    vec![
                        ("x-api-key", self.key.expose_for_header().to_string()),
                        ("anthropic-version", ANTHROPIC_VERSION.to_string()),
                        ("content-type", "application/json".to_string()),
                    ],
                )
            }
            HostedApi::OpenaiCompatible { base_url } => {
                let openrouter = base_url.trim_end_matches('/') == OPENROUTER_BASE_URL;
                let body = build_chat_request(&ollama, &self.model, openrouter)
                    .map_err(TransportError::ConnectionFailed)?;
                (
                    body,
                    vec![
                        (
                            "authorization",
                            format!("Bearer {}", self.key.expose_for_header()),
                        ),
                        ("content-type", "application/json".to_string()),
                    ],
                )
            }
        };
        let header_refs: Vec<(&str, &str)> =
            headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
        let reply = self
            .http
            .post(&self.url(), &header_refs, &body.to_string(), timeout)
            .map_err(|e| match e {
                TransportError::ConnectionFailed(text) => {
                    TransportError::ConnectionFailed(self.key.redact(&text))
                }
                other => other,
            })?;
        if matches!(reply.status, 401..=403) {
            self.switched_off.store(true, Ordering::SeqCst);
        }
        if reply.status != 200 {
            return Ok((
                reply.status,
                json!({ "error": format!("HTTP {}", reply.status) }).to_string(),
            ));
        }
        let response: Value = serde_json::from_str(&reply.body).unwrap_or(Value::Null);
        let text = match &self.api {
            HostedApi::Claude => {
                if response["stop_reason"].as_str() == Some("refusal") {
                    String::new()
                } else {
                    response_text(&response)
                }
            }
            HostedApi::OpenaiCompatible { .. } => chat_response_text(&response),
        };
        Ok((
            200,
            json!({ "response": self.key.redact(&text) }).to_string(),
        ))
    }
}

/// Checks the provider part of the settings: an OpenAI-compatible provider needs an `https://`
/// base URL (a key is never sent over plain HTTP; a LAN Ollama host is reached with the
/// `ollama` provider and `allow_non_loopback` instead), and a hosted provider needs a key
/// source.
pub fn validate_provider(settings: &crate::narrator::NarratorSettings) -> Result<(), String> {
    match settings.provider {
        NarratorProvider::Ollama => Ok(()),
        NarratorProvider::Claude | NarratorProvider::OpenaiCompatible => {
            if settings.key.is_empty() {
                return Err(format!(
                    "the {} provider needs a key: name its environment variable (key.env) or key file (key.file)",
                    settings.provider.name()
                ));
            }
            if settings.provider == NarratorProvider::OpenaiCompatible {
                let url = settings.provider_url.trim();
                if !url.starts_with("https://") || url.len() <= "https://".len() {
                    return Err(
                        "the openai-compatible provider needs an https:// provider_url (for example https://openrouter.ai/api/v1)"
                            .to_string(),
                    );
                }
            }
            Ok(())
        }
    }
}

/// The transport for a hosted provider, or `None` for the local Ollama server (which the
/// caller connects as before). `lookup` reads an environment variable.
pub fn hosted_transport(
    settings: &crate::narrator::NarratorSettings,
    http: Box<dyn HttpsPost>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<HostedTransport>, String> {
    validate_provider(settings)?;
    let api = match settings.provider {
        NarratorProvider::Ollama => return Ok(None),
        NarratorProvider::Claude => HostedApi::Claude,
        NarratorProvider::OpenaiCompatible => HostedApi::OpenaiCompatible {
            base_url: settings
                .provider_url
                .trim()
                .trim_end_matches('/')
                .to_string(),
        },
    };
    let key = settings.key.resolve(lookup)?;
    Ok(Some(HostedTransport::new(api, &settings.model, key, http)))
}
