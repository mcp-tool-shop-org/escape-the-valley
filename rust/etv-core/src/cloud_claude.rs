//! Claude Messages API transport for the narrator measurement harness.
//!
//! [`ClaudeTransport`] is measurement only: the game never uses it. (The game's own cloud
//! narrator is `narrator_provider`, which shares this module's key holder, HTTPS seam and
//! request translation, and has no ledger.) It lets `narrator_measure` send the same
//! prompts the local narrator gets to a hosted Claude model, so the two can be compared on the
//! same turns. [`ClaudeTransport`] implements [`NarratorTransport`], so the harness, the
//! checker and the row format are untouched; the transport translates each Ollama-shaped
//! request into one `POST /v1/messages` call and the answer back.
//!
//! What differs from the local arm, by the API's own rules:
//! - no sampling control and no seed (temperature, top_p, top_k and seed are dropped), so a
//!   run cannot be replayed byte for byte; every raw response is kept in the call log instead;
//! - no context-size or GPU options (dropped);
//! - thinking is turned off in the way each model allows;
//! - the system prompt carries a cache marker, and the usage fields say whether it cached.
//!
//! Safety rails, all enforced here rather than by the caller:
//! - the key is read once from a file outside any repository, never from the environment,
//!   never printed, and stripped from anything this module writes;
//! - a running cost is checked against a hard cap before each call. Each process keeps its
//!   own ledger file; the cap counts every ledger file in that folder, so cells may run side
//!   by side and stages may follow one another under one cap;
//! - a billing error, an authentication or permission error, any other unexpected status, or a
//!   burst of rate-limit or overload answers halts the run: a halt file is written and no
//!   process that shares the folder makes a further call.
//!
//! The network client itself ([`UreqHttpsPost`]) is compiled only with the `narrator-claude`
//! feature. Everything else is plain logic and is tested with a scripted client.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::narrator::{NarratorTransport, TransportError};

/// The one URL this transport ever calls.
pub const CLAUDE_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
/// The API version header sent with every call.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
/// Attempts made for one call when the API answers "rate limited" or "overloaded".
pub const MAX_ATTEMPTS: u32 = 5;
/// Rate-limit or overload answers in a row (across calls) that halt the run.
pub const BUSY_BURST_LIMIT: u32 = 8;
/// The longest single wait honoured between attempts.
pub const MAX_RETRY_WAIT_S: u64 = 60;
/// Network failures in a row (no answer at all, timeouts aside) that halt the run.
pub const NETWORK_FAILURE_LIMIT: u32 = 5;

/// Prices in billionths of a dollar per token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prices {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

/// List prices for prompts of 100,000 tokens or fewer (far above anything the narrator
/// sends). Only the models this harness is allowed to call are listed; any other model is
/// refused before a call is made.
pub fn prices_for(model: &str) -> Option<Prices> {
    match model {
        // $0.10 in, $0.50 out per million; cache write 1.25x, cache read 0.1x.
        "claude-haiku-5-5" => Some(Prices {
            input: 100,
            output: 500,
            cache_write: 125,
            cache_read: 10,
        }),
        // $2 in, $10 out per million; cache write 1.25x, cache read $0.10 per million.
        "claude-sonnet-5-5" => Some(Prices {
            input: 2_000,
            output: 10_000,
            cache_write: 2_500,
            cache_read: 100,
        }),
        // $4 in, $20 out per million; cache write $5, cache read $0.20 per million.
        "claude-opus-5-5" => Some(Prices {
            input: 4_000,
            output: 20_000,
            cache_write: 5_000,
            cache_read: 200,
        }),
        // $8 in, $40 out per million; cache write $10, cache read $0.20 per million.
        "claude-fable-5-2" => Some(Prices {
            input: 8_000,
            output: 40_000,
            cache_write: 10_000,
            cache_read: 200,
        }),
        _ => None,
    }
}

/// True for the models whose thinking cannot be turned off. Their thinking is billed as
/// output and counts against `max_tokens`, so they get [`THINKING_TOKEN_ALLOWANCE`] on top of
/// the answer's own limit and run at the lowest effort.
pub fn always_thinks(model: &str) -> bool {
    matches!(model, "claude-opus-5-5" | "claude-fable-5-2")
}

/// True for the models that get [`THINKING_TOKEN_ALLOWANCE`]: those that always think, and
/// Sonnet 5.5, so the ceiling cells all have the same room.
pub fn gets_headroom(model: &str) -> bool {
    always_thinks(model) || model == "claude-sonnet-5-5"
}

/// Extra `max_tokens` for models that always think, so thinking cannot eat the answer.
pub const THINKING_TOKEN_ALLOWANCE: u64 = 2_000;

/// Token counts from one response's `usage` object.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    /// Reads the usage fields; `None` when the two required counts are missing.
    pub fn from_response(response: &Value) -> Option<Usage> {
        let usage = response.get("usage")?;
        let field = |name: &str| usage.get(name).and_then(Value::as_u64);
        Some(Usage {
            input_tokens: field("input_tokens")?,
            output_tokens: field("output_tokens")?,
            cache_creation_input_tokens: field("cache_creation_input_tokens").unwrap_or(0),
            cache_read_input_tokens: field("cache_read_input_tokens").unwrap_or(0),
        })
    }

    /// The cost of this usage in billionths of a dollar.
    pub fn cost_nano(&self, prices: Prices) -> u64 {
        self.input_tokens * prices.input
            + self.cache_creation_input_tokens * prices.cache_write
            + self.cache_read_input_tokens * prices.cache_read
            + self.output_tokens * prices.output
    }
}

/// An API key held in memory. It has no `Display`, and its `Debug` shows nothing.
#[derive(Clone)]
pub struct ApiKey(String);

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(redacted)")
    }
}

impl ApiKey {
    /// Reads the key from `path`. The file must sit outside every git work tree (no folder
    /// above it may hold a `.git` entry), and must hold one token with no spaces in it.
    /// Errors never include the file's contents.
    pub fn read_from_file(path: &Path) -> Result<ApiKey, String> {
        let full =
            fs::canonicalize(path).map_err(|_| "the key file cannot be found".to_string())?;
        let mut dir = full.parent();
        while let Some(d) = dir {
            if d.join(".git").exists() {
                return Err("the key file sits inside a git repository; refused".to_string());
            }
            dir = d.parent();
        }
        let text =
            fs::read_to_string(&full).map_err(|_| "the key file cannot be read".to_string())?;
        ApiKey::from_text(&text).map_err(|why| format!("the key file {why}"))
    }

    /// A key from text read elsewhere (an environment variable): trimmed, and it must be one
    /// token of 20 characters or more with no spaces. Errors never include the text.
    ///
    /// A `"` or `\` is refused too: no provider issues one, and JSON writes them escaped, so a
    /// key holding one would not match itself in a logged line and could slip past redaction.
    pub fn from_text(text: &str) -> Result<ApiKey, String> {
        let key = text.trim().trim_start_matches('\u{feff}').trim();
        if key.len() < 20
            || key
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\\')
        {
            return Err("does not hold one key".to_string());
        }
        Ok(ApiKey(key.to_string()))
    }

    /// The key, for a request header only.
    pub(crate) fn expose_for_header(&self) -> &str {
        &self.0
    }

    /// For tests only: a key from a literal.
    #[cfg(test)]
    pub(crate) fn for_tests(value: &str) -> ApiKey {
        ApiKey(value.to_string())
    }

    fn expose(&self) -> &str {
        &self.0
    }

    /// `text` with every occurrence of the key replaced.
    pub fn redact(&self, text: &str) -> String {
        text.replace(self.0.as_str(), "[redacted]")
    }

    /// `value` with the key replaced in every string it holds, object keys included, so the key
    /// is gone before the value is written out as JSON (which would escape a `"` or `\`).
    fn redact_value(&self, value: &mut Value) {
        match value {
            Value::String(s) => {
                if s.contains(self.0.as_str()) {
                    *s = self.redact(s);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.redact_value(v)),
            Value::Object(map) => {
                let old = std::mem::take(map);
                for (k, mut v) in old {
                    self.redact_value(&mut v);
                    map.insert(self.redact(&k), v);
                }
            }
            _ => {}
        }
    }
}

/// One HTTP answer, reduced to what the transport needs. No other header is ever read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub body: String,
    /// The API's `request-id` response header.
    pub request_id: Option<String>,
    /// The `retry-after` response header, in whole seconds.
    pub retry_after_s: Option<u64>,
}

/// The HTTPS seam. Tests script it; real runs use [`UreqHttpsPost`].
pub trait HttpsPost: Send + Sync {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
        timeout: Duration,
    ) -> Result<HttpReply, TransportError>;
}

/// Where a run keeps its money count and its call log, and what it may spend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudConfig {
    /// The one model this process may call; a request naming another is refused.
    pub model: String,
    /// Hard cap for everything recorded in the ledger, in billionths of a dollar.
    pub cap_nano: u64,
    /// Hard cap on HTTP requests recorded in the ledger.
    pub max_requests: u64,
    /// This process's running totals. Every other `ledger*.json` in the same folder counts
    /// towards the caps too.
    pub ledger_path: PathBuf,
    /// One JSON line per HTTP attempt.
    pub log_path: PathBuf,
    /// Whether the system block carries a cache marker. It pays only when the same system
    /// prompt is sent again within minutes, which here means a repeated candidate.
    pub cache_marker: bool,
    /// Limit on the visible answer in tokens, replacing the request's own (`None` keeps the
    /// local model's limit).
    pub answer_tokens: Option<u64>,
}

impl CloudConfig {
    /// The halt file this process writes. Any `*.HALT` file in the ledger's folder stops
    /// every process that shares the folder.
    pub fn halt_path(&self) -> PathBuf {
        let mut name = self
            .ledger_path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".HALT");
        self.ledger_path.with_file_name(name)
    }
}

/// The running totals kept in the ledger file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ledger {
    pub spent_nano: u64,
    pub requests: u64,
}

impl Ledger {
    /// Reads the ledger; a missing file is a fresh run, an unreadable one is an error.
    pub fn load(path: &Path) -> Result<Ledger, String> {
        if !path.exists() {
            return Ok(Ledger::default());
        }
        let text = fs::read_to_string(path).map_err(|e| format!("ledger unreadable: {e}"))?;
        let value: Value =
            serde_json::from_str(&text).map_err(|e| format!("ledger is not JSON: {e}"))?;
        let field = |name: &str| {
            value
                .get(name)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("ledger has no {name}"))
        };
        Ok(Ledger {
            spent_nano: field("spent_nano")?,
            requests: field("requests")?,
        })
    }

    /// Writes the ledger through a temporary file, so a crash never leaves half a ledger.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = json!({
            "spent_nano": self.spent_nano,
            "requests": self.requests,
            "spent_usd": nano_as_usd(self.spent_nano),
        })
        .to_string();
        let mut tmp_name = path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        tmp_name.push(".tmp");
        let tmp = path.with_file_name(tmp_name);
        fs::write(&tmp, text).map_err(|e| format!("ledger not written: {e}"))?;
        // Another process may be reading the ledger at this instant; try again briefly.
        let mut tries = 0;
        loop {
            match fs::rename(&tmp, path) {
                Ok(()) => return Ok(()),
                Err(e) if tries >= 20 => return Err(format!("ledger not replaced: {e}")),
                Err(_) => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }
    }

    /// The totals of every other `ledger*.json` beside `own`. A file that cannot be read
    /// just now (it is being replaced) is read again; one that stays unreadable is an error.
    pub fn others_beside(own: &Path) -> Result<Ledger, String> {
        let dir = match own.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let mut total = Ledger::default();
        let entries = fs::read_dir(&dir).map_err(|e| format!("ledger folder unreadable: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("ledger") || !name.ends_with(".json") {
                continue;
            }
            if Some(entry.file_name().as_os_str()) == own.file_name() {
                continue;
            }
            let mut tries = 0;
            let other = loop {
                match Ledger::load(&path) {
                    Ok(ledger) => break ledger,
                    Err(e) if tries >= 20 => return Err(e),
                    Err(_) => {
                        tries += 1;
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
            };
            total.spent_nano = total.spent_nano.saturating_add(other.spent_nano);
            total.requests = total.requests.saturating_add(other.requests);
        }
        Ok(total)
    }
}

/// The first `*.HALT` file in `dir`, if any.
fn halt_file_in(dir: &Path) -> Option<PathBuf> {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("HALT"))
}

/// Billionths of a dollar as a dollar string with six decimals (for people; never parsed).
pub fn nano_as_usd(nano: u64) -> String {
    format!(
        "{}.{:06}",
        nano / 1_000_000_000,
        (nano % 1_000_000_000) / 1_000
    )
}

/// Parses a dollar amount such as `15` or `0.25` into billionths of a dollar.
pub fn usd_to_nano(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(whole) || (!frac.is_empty() && !digits(frac)) || frac.len() > 9 {
        return Err(format!("not a dollar amount: {text}"));
    }
    let whole: u64 = whole
        .parse()
        .map_err(|_| format!("not a dollar amount: {text}"))?;
    let frac_nano: u64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<9}")
            .parse()
            .map_err(|_| format!("not a dollar amount: {text}"))?
    };
    whole
        .checked_mul(1_000_000_000)
        .and_then(|w| w.checked_add(frac_nano))
        .ok_or_else(|| format!("dollar amount too large: {text}"))
}

/// Schema keywords the API's structured output does not accept. They are removed from the
/// schema that is sent; the checker enforces the same limits on the answer.
const UNSUPPORTED_SCHEMA_KEYS: [&str; 7] = [
    "minItems",
    "maxItems",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "multipleOf",
];

fn sanitise_schema(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            for key in UNSUPPORTED_SCHEMA_KEYS {
                map.remove(key);
            }
            if map.get("type").and_then(Value::as_str) == Some("object") {
                map.insert("additionalProperties".to_string(), Value::Bool(false));
            }
            for (_, child) in map.iter_mut() {
                sanitise_schema(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sanitise_schema),
        _ => {}
    }
}

/// A Messages API request built from an Ollama-shaped one, and what could not be carried over.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltRequest {
    pub body: Value,
    /// Names of the Ollama options that have no counterpart and were left out.
    pub dropped: Vec<String>,
    pub max_tokens: u64,
}

/// True for the harness's warm-up request, which loads a local model and means nothing here.
pub fn is_warmup(ollama: &Value) -> bool {
    ollama.get("system").is_none()
        && ollama.get("prompt").and_then(Value::as_str) == Some("Warm up.")
        && ollama["options"]["num_predict"].as_u64() == Some(1)
}

/// Translates one Ollama `/api/generate` body into a `POST /v1/messages` body.
///
/// The system prompt becomes one text block with a cache marker; the prompt becomes the one
/// user message (there is never an assistant turn, so nothing is prefilled); `format` becomes
/// `output_config.format`; `num_predict` becomes `max_tokens`; `stop` becomes
/// `stop_sequences`. Sampling options, seed, context size and GPU options are dropped.
/// `cache_marker` puts a cache marker on the system block. `answer_limit`, when given,
/// replaces `num_predict` as the limit on the visible answer.
pub fn build_messages_request(
    ollama: &Value,
    model: &str,
    cache_marker: bool,
    answer_limit: Option<u64>,
) -> Result<BuiltRequest, String> {
    let asked = ollama.get("model").and_then(Value::as_str).unwrap_or("");
    if asked != model {
        return Err(format!("the request names model {asked:?}, not {model:?}"));
    }
    let prompt = ollama
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or_else(|| "the request has no prompt".to_string())?;
    let options = ollama.get("options").cloned().unwrap_or_else(|| json!({}));
    let answer_tokens = options
        .get("num_predict")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or_else(|| "the request has no num_predict".to_string())?;
    // The local limit was sized for the local model's tokenizer; a run may set its own.
    let answer_tokens = answer_limit.unwrap_or(answer_tokens);
    let max_tokens = if gets_headroom(model) {
        answer_tokens + THINKING_TOKEN_ALLOWANCE
    } else {
        answer_tokens
    };

    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{ "role": "user", "content": prompt }],
    });
    if let Some(system) = ollama.get("system").and_then(Value::as_str) {
        body["system"] = if cache_marker {
            json!([{ "type": "text", "text": system, "cache_control": { "type": "ephemeral" } }])
        } else {
            json!([{ "type": "text", "text": system }])
        };
    }
    // Thinking off in the way each model accepts; where it cannot be turned off, the
    // `thinking` field is left out and the effort is set to its lowest. No `fallbacks`
    // field is ever sent: a silent reroute would mix models in one arm.
    if always_thinks(model) {
        body["output_config"] = json!({ "effort": "low" });
    } else if model == "claude-sonnet-5-5" {
        body["thinking"] = json!({ "type": "between_tools" });
    } else {
        body["thinking"] = json!({ "type": "disabled" });
    }
    if let Some(format) = ollama.get("format") {
        let mut schema = format.clone();
        sanitise_schema(&mut schema);
        body["output_config"]["format"] = json!({ "type": "json_schema", "schema": schema });
    }
    if let Some(stop) = options.get("stop").and_then(Value::as_array) {
        if !stop.is_empty() {
            body["stop_sequences"] = Value::Array(stop.clone());
        }
    }
    let mut dropped: Vec<String> = options
        .as_object()
        .map(|o| {
            o.keys()
                .filter(|k| !matches!(k.as_str(), "num_predict" | "stop"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if ollama.get("think").is_some() {
        dropped.push("think".to_string());
    }
    dropped.sort();
    Ok(BuiltRequest {
        body,
        dropped,
        max_tokens,
    })
}

/// The text of a Messages API response: its text blocks joined in order.
pub fn response_text(response: &Value) -> String {
    response
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct State {
    ledger: Ledger,
    halted: Option<String>,
    busy_in_a_row: u32,
    network_failures_in_a_row: u32,
    call_no: u64,
}

/// What a finished process reports about its cloud calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudSummary {
    pub ledger: Ledger,
    pub halted: Option<String>,
    pub calls: u64,
}

/// The Claude transport. See the module notes for what it guarantees.
pub struct ClaudeTransport {
    key: ApiKey,
    cfg: CloudConfig,
    prices: Prices,
    http: Box<dyn HttpsPost>,
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
    state: Mutex<State>,
}

impl std::fmt::Debug for ClaudeTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeTransport")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

impl ClaudeTransport {
    /// Opens the run's ledger and refuses to start when the model has no listed price, the
    /// ledger is unreadable, or an earlier process of the run halted.
    pub fn new(key: ApiKey, cfg: CloudConfig, http: Box<dyn HttpsPost>) -> Result<Self, String> {
        Self::with_sleep(key, cfg, http, Box::new(std::thread::sleep))
    }

    /// [`ClaudeTransport::new`] with the waiting function supplied (tests record the waits).
    pub fn with_sleep(
        key: ApiKey,
        cfg: CloudConfig,
        http: Box<dyn HttpsPost>,
        sleep: Box<dyn Fn(Duration) + Send + Sync>,
    ) -> Result<Self, String> {
        let prices = prices_for(&cfg.model)
            .ok_or_else(|| format!("no listed price for model {:?}; refused", cfg.model))?;
        let folder = cfg.ledger_path.parent().unwrap_or(Path::new("."));
        if let Some(halt) = halt_file_in(folder) {
            return Err(format!(
                "this run halted earlier (see {}); refused",
                halt.display()
            ));
        }
        let ledger = Ledger::load(&cfg.ledger_path)?;
        Ok(Self {
            key,
            cfg,
            prices,
            http,
            sleep,
            state: Mutex::new(State {
                ledger,
                halted: None,
                busy_in_a_row: 0,
                network_failures_in_a_row: 0,
                call_no: 0,
            }),
        })
    }

    /// Totals so far and whether the run halted.
    pub fn summary(&self) -> CloudSummary {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        CloudSummary {
            ledger: state.ledger,
            halted: state.halted.clone(),
            calls: state.call_no,
        }
    }

    fn log_line(&self, mut line: Value) {
        line["ts_ms"] = json!(unix_ms());
        line["model"] = json!(self.cfg.model);
        // Redact the values before they are written as JSON, then the written line as well.
        self.key.redact_value(&mut line);
        let text = self.key.redact(&line.to_string());
        if let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.cfg.log_path)
        {
            let _ = writeln!(file, "{text}");
        }
    }

    fn halt(&self, state: &mut State, reason: &str) {
        let reason = self.key.redact(reason);
        if state.halted.is_none() {
            state.halted = Some(reason.clone());
            let _ = fs::write(self.cfg.halt_path(), format!("{reason}\n"));
            self.log_line(json!({ "event": "halt", "reason": reason }));
        }
    }

    fn halted_error(reason: &str) -> TransportError {
        TransportError::ConnectionFailed(format!("cloud run halted: {reason}"))
    }
}

/// The error type and message from an API error body, when it has that shape.
fn api_error(body: &str) -> (Option<String>, Option<String>) {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return (None, None);
    };
    let text = |name: &str| {
        value["error"][name]
            .as_str()
            .map(|s| s.chars().take(300).collect::<String>())
    };
    (text("type"), text("message"))
}

impl NarratorTransport for ClaudeTransport {
    /// Answered locally: the one model this run may call, in the shape the harness reads.
    fn get_tags(
        &self,
        _endpoint: &str,
        _timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        let body = json!({
            "models": [{
                "name": self.cfg.model,
                "digest": format!("anthropic-api:{ANTHROPIC_VERSION}"),
                "details": { "quantization_level": "hosted" }
            }]
        });
        Ok((200, body.to_string()))
    }

    fn post_generate(
        &self,
        _endpoint: &str,
        body_json: &str,
        timeout: Duration,
    ) -> Result<(u16, String), TransportError> {
        let ollama: Value = serde_json::from_str(body_json)
            .map_err(|e| TransportError::ConnectionFailed(format!("request is not JSON: {e}")))?;
        if is_warmup(&ollama) {
            // Nothing to load on a hosted model; no call, no cost.
            return Ok((200, json!({ "response": "" }).to_string()));
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(reason) = &state.halted {
            return Err(Self::halted_error(reason));
        }
        let built = match build_messages_request(
            &ollama,
            &self.cfg.model,
            self.cfg.cache_marker,
            self.cfg.answer_tokens,
        ) {
            Ok(built) => built,
            Err(why) => {
                self.halt(&mut state, &format!("request could not be built: {why}"));
                return Err(Self::halted_error(&why));
            }
        };
        let request = built.body.to_string();
        let request_sha256 = sha256_hex(&request);
        state.call_no += 1;
        let call_no = state.call_no;

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            // The most this attempt could cost: every request byte a freshly cached token
            // (a token is never shorter than a byte), plus a full-length answer.
            let worst = request.len() as u64 * self.prices.cache_write
                + built.max_tokens * self.prices.output;
            let folder = self.cfg.ledger_path.parent().unwrap_or(Path::new("."));
            if let Some(other_halt) = halt_file_in(folder) {
                // Another process of the run halted: stop here too, without a second file.
                let reason = format!("halt file present: {}", other_halt.display());
                state.halted = Some(reason.clone());
                return Err(Self::halted_error(&reason));
            }
            let others = match Ledger::others_beside(&self.cfg.ledger_path) {
                Ok(others) => others,
                Err(why) => {
                    self.halt(&mut state, &why);
                    return Err(Self::halted_error(&why));
                }
            };
            let spent = state.ledger.spent_nano.saturating_add(others.spent_nano);
            let requests = state.ledger.requests.saturating_add(others.requests);
            if spent.saturating_add(worst) > self.cfg.cap_nano {
                let reason = format!(
                    "cost cap reached: spent ${} of ${}",
                    nano_as_usd(spent),
                    nano_as_usd(self.cfg.cap_nano)
                );
                self.halt(&mut state, &reason);
                return Err(Self::halted_error(&reason));
            }
            if requests >= self.cfg.max_requests {
                let reason = format!("request cap reached: {requests} requests");
                self.halt(&mut state, &reason);
                return Err(Self::halted_error(&reason));
            }
            state.ledger.requests += 1;
            if let Err(why) = state.ledger.save(&self.cfg.ledger_path) {
                self.halt(&mut state, &why);
                return Err(Self::halted_error(&why));
            }

            let headers = [
                ("x-api-key", self.key.expose()),
                ("anthropic-version", ANTHROPIC_VERSION),
                ("content-type", "application/json"),
            ];
            let started = Instant::now();
            let reply = self
                .http
                .post(CLAUDE_MESSAGES_URL, &headers, &request, timeout);
            let latency_ms = started.elapsed().as_millis() as u64;
            let reply = match reply {
                Ok(reply) => reply,
                Err(err) => {
                    let (kind, detail) = match &err {
                        TransportError::Timeout => ("timeout", String::new()),
                        TransportError::ConnectionFailed(text) => {
                            ("transport", text.chars().take(200).collect())
                        }
                        _ => ("transport", String::new()),
                    };
                    self.log_line(json!({
                        "event": "attempt", "call_no": call_no, "attempt": attempt,
                        "outcome": kind, "latency_ms": latency_ms, "error": detail,
                        "request_sha256": request_sha256,
                    }));
                    if kind == "timeout" {
                        return Err(TransportError::Timeout);
                    }
                    state.network_failures_in_a_row += 1;
                    if state.network_failures_in_a_row >= NETWORK_FAILURE_LIMIT {
                        let reason = format!(
                            "{} network failures in a row: {detail}",
                            state.network_failures_in_a_row
                        );
                        self.halt(&mut state, &reason);
                    }
                    return Err(TransportError::ConnectionFailed(
                        "network error".to_string(),
                    ));
                }
            };

            state.network_failures_in_a_row = 0;
            if reply.status == 200 {
                state.busy_in_a_row = 0;
                let response: Value = serde_json::from_str(&reply.body).unwrap_or(Value::Null);
                let Some(usage) = Usage::from_response(&response) else {
                    self.log_line(json!({
                        "event": "attempt", "call_no": call_no, "attempt": attempt,
                        "outcome": "no_usage", "http_status": 200,
                        "request_id": reply.request_id, "latency_ms": latency_ms,
                        "request_sha256": request_sha256, "raw_response": reply.body,
                    }));
                    let reason = "a response carried no usage; cost cannot be tracked";
                    self.halt(&mut state, reason);
                    return Err(Self::halted_error(reason));
                };
                let cost = usage.cost_nano(self.prices);
                state.ledger.spent_nano = state.ledger.spent_nano.saturating_add(cost);
                let saved = state.ledger.save(&self.cfg.ledger_path);
                let stop_reason = response["stop_reason"].as_str().unwrap_or("").to_string();
                let text = if stop_reason == "refusal" {
                    String::new()
                } else {
                    response_text(&response)
                };
                self.log_line(json!({
                    "event": "attempt", "call_no": call_no, "attempt": attempt,
                    "outcome": "ok", "http_status": 200,
                    "request_id": reply.request_id, "latency_ms": latency_ms,
                    "stop_reason": stop_reason,
                    "stop_details": response["stop_details"],
                    "input_tokens": usage.input_tokens,
                    "cache_creation_input_tokens": usage.cache_creation_input_tokens,
                    "cache_read_input_tokens": usage.cache_read_input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cost_nano": cost,
                    "spent_nano": state.ledger.spent_nano,
                    "max_tokens": built.max_tokens,
                    "dropped_options": built.dropped,
                    "seed_ignored": ollama["options"]["seed"],
                    "request_sha256": request_sha256,
                    "raw_response": reply.body,
                }));
                if let Err(why) = saved {
                    self.halt(&mut state, &why);
                    return Err(Self::halted_error(&why));
                }
                return Ok((200, json!({ "response": text }).to_string()));
            }

            let (error_type, error_message) = api_error(&reply.body);
            self.log_line(json!({
                "event": "attempt", "call_no": call_no, "attempt": attempt,
                "outcome": "http_error", "http_status": reply.status,
                "request_id": reply.request_id, "latency_ms": latency_ms,
                "error_type": error_type, "error_message": error_message,
                "retry_after_s": reply.retry_after_s,
                "request_sha256": request_sha256,
            }));
            let busy = matches!(reply.status, 429 | 500 | 502 | 503 | 504 | 529);
            if !busy {
                // 400, 401, 402, 403, 404 and anything else: stop at once.
                let reason = format!(
                    "HTTP {} ({}), request-id {}",
                    reply.status,
                    error_type.as_deref().unwrap_or("no error type"),
                    reply.request_id.as_deref().unwrap_or("none")
                );
                self.halt(&mut state, &reason);
                return Ok((reply.status, json!({ "error": error_type }).to_string()));
            }
            state.busy_in_a_row += 1;
            if state.busy_in_a_row >= BUSY_BURST_LIMIT || attempt >= MAX_ATTEMPTS {
                let reason = format!(
                    "burst of busy answers: {} in a row, last HTTP {}, request-id {}",
                    state.busy_in_a_row,
                    reply.status,
                    reply.request_id.as_deref().unwrap_or("none")
                );
                self.halt(&mut state, &reason);
                return Ok((reply.status, json!({ "error": error_type }).to_string()));
            }
            let wait = reply
                .retry_after_s
                .unwrap_or(1u64 << attempt.min(5))
                .clamp(1, MAX_RETRY_WAIT_S);
            (self.sleep)(Duration::from_secs(wait));
        }
    }
}

/// The cloud flags of `narrator_measure`, split from the harness's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudArgs {
    pub key_file: PathBuf,
    pub cfg: CloudConfig,
}

/// Usage text for the cloud flags.
pub const CLOUD_USAGE: &str = "cloud flags (all required with --cloud):
  --cloud claude            send the prompts to the Claude API instead of a local model
  --cloud-key-file PATH     file holding the API key (outside any repository)
  --cloud-cap-usd N         hard cap in dollars for everything in the ledger
  --cloud-max-requests N    hard cap on HTTP requests in the ledger
  --cloud-ledger PATH       running totals, shared by every process of the run
  --cloud-log PATH          one JSON line per HTTP attempt
optional:
  --cloud-no-cache-marker   send the system prompt without a cache marker
  --cloud-answer-tokens N   limit on the visible answer, in place of the local model's";

/// Splits the cloud flags out of `args`. Returns `None` and the arguments unchanged when
/// `--cloud` is absent. The model comes from the harness's own `--model` flag.
pub fn split_cloud_args(args: &[String]) -> Result<(Option<CloudArgs>, Vec<String>), String> {
    let mut rest = Vec::new();
    let mut found: std::collections::BTreeMap<&'static str, String> = Default::default();
    const FLAGS: [&str; 7] = [
        "--cloud-answer-tokens",
        "--cloud",
        "--cloud-key-file",
        "--cloud-cap-usd",
        "--cloud-max-requests",
        "--cloud-ledger",
        "--cloud-log",
    ];
    let mut cache_marker = true;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--cloud-no-cache-marker" {
            cache_marker = false;
            i += 1;
            continue;
        }
        if let Some(flag) = FLAGS.iter().find(|f| **f == args[i]) {
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("{flag} needs a value"))?;
            found.insert(*flag, value.clone());
            i += 2;
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }
    if found.is_empty() && cache_marker {
        return Ok((None, rest));
    }
    if found.get("--cloud").map(String::as_str) != Some("claude") {
        return Err(format!("--cloud takes one value: claude\n{CLOUD_USAGE}"));
    }
    let need = |flag: &'static str| {
        found
            .get(flag)
            .cloned()
            .ok_or_else(|| format!("{flag} is required with --cloud\n{CLOUD_USAGE}"))
    };
    let model = rest
        .iter()
        .position(|a| a == "--model")
        .and_then(|p| rest.get(p + 1))
        .cloned()
        .ok_or_else(|| "--model is required with --cloud".to_string())?;
    let max_requests: u64 = need("--cloud-max-requests")?
        .parse()
        .map_err(|_| "--cloud-max-requests needs a whole number".to_string())?;
    Ok((
        Some(CloudArgs {
            key_file: PathBuf::from(need("--cloud-key-file")?),
            cfg: CloudConfig {
                model,
                cap_nano: usd_to_nano(&need("--cloud-cap-usd")?)?,
                max_requests,
                ledger_path: PathBuf::from(need("--cloud-ledger")?),
                log_path: PathBuf::from(need("--cloud-log")?),
                cache_marker,
                answer_tokens: match found.get("--cloud-answer-tokens") {
                    Some(text) => {
                        Some(text.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| {
                            "--cloud-answer-tokens needs a whole number".to_string()
                        })?)
                    }
                    None => None,
                },
            },
        }),
        rest,
    ))
}

/// The real HTTPS client: one connection pool for the process, HTTPS only.
#[cfg(feature = "narrator-claude")]
pub struct UreqHttpsPost {
    agent: ureq::Agent,
}

#[cfg(feature = "narrator-claude")]
impl Default for UreqHttpsPost {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    // The system's own certificate store decides what is trusted.
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build()
            .new_agent();
        Self { agent }
    }
}

#[cfg(feature = "narrator-claude")]
impl HttpsPost for UreqHttpsPost {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
        timeout: Duration,
    ) -> Result<HttpReply, TransportError> {
        let mut request = self.agent.post(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut resp = request
            .config()
            .timeout_global(Some(timeout))
            .build()
            .send(body)
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => TransportError::Timeout,
                // The client's error text names the failure (DNS, TLS, reset); it never
                // holds a header. The transport strips the key from it all the same.
                other => TransportError::ConnectionFailed(other.to_string()),
            })?;
        let status = resp.status().as_u16();
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let request_id = header("request-id");
        let retry_after_s = header("retry-after").and_then(|v| v.trim().parse::<u64>().ok());
        let body = resp
            .body_mut()
            .with_config()
            .limit(crate::narrator::NARRATOR_MAX_BODY_BYTES)
            .read_to_string()
            .map_err(|_| TransportError::ConnectionFailed("network error".to_string()))?;
        Ok(HttpReply {
            status,
            body,
            request_id,
            retry_after_s,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const TEST_KEY: &str = "test-key-0123456789-abcdefghij-KLMNOPQRST";

    /// One request as the scripted client saw it: the URL, the headers and the body.
    type SeenRequest = (String, Vec<(String, String)>, String);

    #[derive(Default)]
    struct Script {
        replies: Mutex<Vec<Result<HttpReply, TransportError>>>,
        seen: Mutex<Vec<SeenRequest>>,
    }

    struct Scripted(Arc<Script>);

    impl HttpsPost for Scripted {
        fn post(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &str,
            _timeout: Duration,
        ) -> Result<HttpReply, TransportError> {
            self.0.seen.lock().unwrap().push((
                url.to_string(),
                headers
                    .iter()
                    .map(|(n, v)| (n.to_string(), v.to_string()))
                    .collect(),
                body.to_string(),
            ));
            let mut replies = self.0.replies.lock().unwrap();
            assert!(!replies.is_empty(), "an unscripted HTTP call was made");
            replies.remove(0)
        }
    }

    fn ok_reply(text: &str, usage: Value) -> Result<HttpReply, TransportError> {
        Ok(HttpReply {
            status: 200,
            body: json!({
                "content": [{ "type": "text", "text": text }],
                "stop_reason": "end_turn",
                "usage": usage,
            })
            .to_string(),
            request_id: Some("req_ok_1".to_string()),
            retry_after_s: None,
        })
    }

    fn error_reply(status: u16, kind: &str, message: &str) -> Result<HttpReply, TransportError> {
        Ok(HttpReply {
            status,
            body: json!({ "type": "error", "error": { "type": kind, "message": message } })
                .to_string(),
            request_id: Some(format!("req_err_{status}")),
            retry_after_s: None,
        })
    }

    struct Rig {
        dir: tempfile::TempDir,
        script: Arc<Script>,
        waits: Arc<Mutex<Vec<u64>>>,
    }

    impl Rig {
        fn new(replies: Vec<Result<HttpReply, TransportError>>) -> Rig {
            Rig {
                dir: tempfile::tempdir().unwrap(),
                script: Arc::new(Script {
                    replies: Mutex::new(replies),
                    seen: Mutex::new(Vec::new()),
                }),
                waits: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn cfg(&self, model: &str, cap_nano: u64) -> CloudConfig {
            CloudConfig {
                model: model.to_string(),
                cap_nano,
                max_requests: 1_000,
                ledger_path: self.dir.path().join("ledger.json"),
                log_path: self.dir.path().join("calls.jsonl"),
                cache_marker: true,
                answer_tokens: None,
            }
        }

        fn transport(&self, model: &str, cap_nano: u64) -> Result<ClaudeTransport, String> {
            let waits = self.waits.clone();
            ClaudeTransport::with_sleep(
                ApiKey::for_tests(TEST_KEY),
                self.cfg(model, cap_nano),
                Box::new(Scripted(self.script.clone())),
                Box::new(move |d| waits.lock().unwrap().push(d.as_secs())),
            )
        }

        fn calls_made(&self) -> usize {
            self.script.seen.lock().unwrap().len()
        }

        fn log(&self) -> String {
            fs::read_to_string(self.dir.path().join("calls.jsonl")).unwrap_or_default()
        }
    }

    fn ollama_body(model: &str) -> String {
        json!({
            "model": model,
            "system": "You narrate one turn.",
            "prompt": "Turn facts: the party reached the ford.",
            "stream": false,
            "think": false,
            "format": {
                "type": "object",
                "properties": { "sentences": {
                    "type": "array", "items": { "type": "string" },
                    "minItems": 1, "maxItems": 8
                } },
                "required": ["sentences"],
                "additionalProperties": false
            },
            "options": {
                "temperature": 0.7, "top_p": 0.8, "top_k": 20,
                "seed": 12345, "num_ctx": 2048, "num_predict": 110
            }
        })
        .to_string()
    }

    const HAIKU: &str = "claude-haiku-5-5";
    const ONE_DOLLAR: u64 = 1_000_000_000;

    #[test]
    fn the_request_is_a_messages_call_with_no_sampling_and_no_prefill() {
        let rig = Rig::new(vec![ok_reply(
            "{\"sentences\":[\"The ford lies behind.\"]}",
            json!({ "input_tokens": 400, "output_tokens": 12 }),
        )]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        let (status, body) = t
            .post_generate(
                "http://127.0.0.1:11434",
                &ollama_body(HAIKU),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(status, 200);
        let outer: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            outer["response"],
            "{\"sentences\":[\"The ford lies behind.\"]}"
        );

        let seen = rig.script.seen.lock().unwrap();
        let (url, headers, sent) = &seen[0];
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
        assert_eq!(
            headers,
            &vec![
                ("x-api-key".to_string(), TEST_KEY.to_string()),
                ("anthropic-version".to_string(), "2026-10-21".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ]
        );
        let sent: Value = serde_json::from_str(sent).unwrap();
        assert_eq!(
            sent,
            json!({
                "model": "claude-haiku-5-5",
                "max_tokens": 110,
                "messages": [{ "role": "user", "content": "Turn facts: the party reached the ford." }],
                "system": [{
                    "type": "text",
                    "text": "You narrate one turn.",
                    "cache_control": { "type": "ephemeral" }
                }],
                "thinking": { "type": "disabled" },
                "output_config": { "format": { "type": "json_schema", "schema": {
                    "type": "object",
                    "properties": { "sentences": { "type": "array", "items": { "type": "string" } } },
                    "required": ["sentences"],
                    "additionalProperties": false
                } } }
            })
        );
    }

    #[test]
    fn dropped_options_are_named_and_a_stop_becomes_stop_sequences() {
        let mut body: Value = serde_json::from_str(&ollama_body(HAIKU)).unwrap();
        body.as_object_mut().unwrap().remove("format");
        body["options"]["stop"] = json!(["."]);
        body["options"]["num_gpu"] = json!(18);
        let built = build_messages_request(&body, HAIKU, true, None).unwrap();
        // A run may set its own limit on the visible answer.
        let wider = build_messages_request(&body, HAIKU, true, Some(300)).unwrap();
        assert_eq!(wider.body["max_tokens"], 300);
        assert_eq!(built.body["stop_sequences"], json!(["."]));
        assert!(built.body.get("output_config").is_none());
        assert_eq!(
            built.dropped,
            vec![
                "num_ctx",
                "num_gpu",
                "seed",
                "temperature",
                "think",
                "top_k",
                "top_p"
            ]
        );
        for gone in ["temperature", "top_p", "top_k", "seed"] {
            assert!(built.body.get(gone).is_none(), "{gone} must not be sent");
        }
    }

    #[test]
    fn sonnet_turns_thinking_off_its_own_way_and_other_models_are_refused() {
        let body: Value = serde_json::from_str(&ollama_body("claude-sonnet-5-5")).unwrap();
        let built = build_messages_request(&body, "claude-sonnet-5-5", true, Some(300)).unwrap();
        assert_eq!(built.body["thinking"], json!({ "type": "between_tools" }));
        // 300 for the answer plus the same 2,000 headroom the thinking models get.
        assert_eq!(built.body["max_tokens"], 2_300);
        // A request for a model other than the run's own is refused.
        assert!(build_messages_request(&body, HAIKU, true, None).is_err());
        // A model with no listed price cannot start a run.
        let rig = Rig::new(vec![]);
        assert!(rig.transport("claude-opus-4-8", ONE_DOLLAR).is_err());
    }

    #[test]
    fn models_that_always_think_get_low_effort_and_room_to_think() {
        for model in ["claude-opus-5-5", "claude-fable-5-2"] {
            let body: Value = serde_json::from_str(&ollama_body(model)).unwrap();
            let built = build_messages_request(&body, model, false, None).unwrap();
            assert!(built.body.get("thinking").is_none());
            assert!(built.body.get("fallbacks").is_none());
            assert_eq!(built.body["output_config"]["effort"], "low");
            assert_eq!(built.body["output_config"]["format"]["type"], "json_schema");
            // 110 for the answer plus 2,000 for thinking.
            assert_eq!(built.body["max_tokens"], 2_110);
            assert_eq!(built.max_tokens, 2_110);
            // No cache marker when it is switched off.
            assert_eq!(
                built.body["system"],
                json!([{ "type": "text", "text": "You narrate one turn." }])
            );
        }
        // 1,000 x 4,000 + 200 x 5,000 + 300 x 200 + 50 x 20,000 = 6,060,000.
        let u = Usage {
            input_tokens: 1000,
            cache_creation_input_tokens: 200,
            cache_read_input_tokens: 300,
            output_tokens: 50,
        };
        assert_eq!(
            u.cost_nano(prices_for("claude-opus-5-5").unwrap()),
            6_060_000
        );
        // 1,000 x 8,000 + 200 x 10,000 + 300 x 200 + 50 x 40,000 = 12,060,000.
        assert_eq!(
            u.cost_nano(prices_for("claude-fable-5-2").unwrap()),
            12_060_000
        );
    }

    #[test]
    fn cost_is_counted_from_usage_and_kept_across_processes() {
        // 1,000 x 100 + 200 x 125 + 300 x 10 + 50 x 500 = 153,000 billionths of a dollar.
        let usage = json!({
            "input_tokens": 1000, "cache_creation_input_tokens": 200,
            "cache_read_input_tokens": 300, "output_tokens": 50
        });
        let rig = Rig::new(vec![ok_reply("{}", usage.clone()), ok_reply("{}", usage)]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        t.post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            t.summary().ledger,
            Ledger {
                spent_nano: 153_000,
                requests: 1
            }
        );
        drop(t);
        // A second process of the same run starts from the ledger file.
        let t2 = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        t2.post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            t2.summary().ledger,
            Ledger {
                spent_nano: 306_000,
                requests: 2
            }
        );
        assert_eq!(
            Ledger::load(&rig.dir.path().join("ledger.json")).unwrap(),
            Ledger {
                spent_nano: 306_000,
                requests: 2
            }
        );
        // Sonnet's prices: 1,000 x 2,000 + 200 x 2,500 + 300 x 100 + 50 x 10,000 = 3,030,000.
        let u = Usage {
            input_tokens: 1000,
            cache_creation_input_tokens: 200,
            cache_read_input_tokens: 300,
            output_tokens: 50,
        };
        assert_eq!(
            u.cost_nano(prices_for("claude-sonnet-5-5").unwrap()),
            3_030_000
        );
    }

    #[test]
    fn the_cap_stops_the_run_before_the_call_that_could_cross_it() {
        let rig = Rig::new(vec![]);
        // The request is a few hundred bytes; a cap of 1,000 billionths cannot cover it.
        let t = rig.transport(HAIKU, 1_000).unwrap();
        let err = t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap_err();
        assert!(matches!(err, TransportError::ConnectionFailed(ref m) if m.contains("cost cap")));
        assert_eq!(rig.calls_made(), 0);
        assert!(rig.cfg(HAIKU, 1_000).halt_path().exists());
        // Nothing more is sent, and a new process of the run refuses to start.
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_err());
        assert_eq!(rig.calls_made(), 0);
        assert!(rig.transport(HAIKU, ONE_DOLLAR).is_err());
    }

    #[test]
    fn ledgers_side_by_side_share_the_cap_and_a_halt_stops_them_all() {
        let rig = Rig::new(vec![ok_reply(
            "{}",
            json!({ "input_tokens": 1, "output_tokens": 1 }),
        )]);
        // Another process of the run has already spent 0.9 of the one-dollar cap.
        Ledger {
            spent_nano: 900_000_000,
            requests: 7,
        }
        .save(&rig.dir.path().join("ledger-other.json"))
        .unwrap();
        // A file that is not a ledger is not counted.
        fs::write(rig.dir.path().join("notes.json"), "{}").unwrap();
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_ok());
        assert_eq!(
            t.summary().ledger,
            Ledger {
                spent_nano: 600,
                requests: 1
            }
        );
        // The other process spends up to the cap: this one stops before its next call.
        Ledger {
            spent_nano: 999_999_999,
            requests: 8,
        }
        .save(&rig.dir.path().join("ledger-other.json"))
        .unwrap();
        let err = t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap_err();
        assert!(matches!(err, TransportError::ConnectionFailed(ref m) if m.contains("cost cap")));
        assert_eq!(rig.calls_made(), 1);

        // A halt file written by any process stops a process that is already running.
        let rig = Rig::new(vec![]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        fs::write(rig.dir.path().join("ledger-other.json.HALT"), "billing").unwrap();
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_err());
        assert_eq!(rig.calls_made(), 0);
        assert!(t.summary().halted.unwrap().contains("halt file present"));
        assert!(rig.transport(HAIKU, ONE_DOLLAR).is_err());
    }

    #[test]
    fn the_request_cap_stops_the_run() {
        let rig = Rig::new(vec![ok_reply(
            "{}",
            json!({ "input_tokens": 1, "output_tokens": 1 }),
        )]);
        let cfg = CloudConfig {
            max_requests: 1,
            ..rig.cfg(HAIKU, ONE_DOLLAR)
        };
        let t = ClaudeTransport::new(
            ApiKey::for_tests(TEST_KEY),
            cfg,
            Box::new(Scripted(rig.script.clone())),
        )
        .unwrap();
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_ok());
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_err());
        assert_eq!(rig.calls_made(), 1);
        assert!(t.summary().halted.unwrap().contains("request cap"));
    }

    #[test]
    fn billing_and_authentication_errors_halt_at_once() {
        for (status, kind) in [
            (402, "billing_error"),
            (401, "authentication_error"),
            (403, "permission_error"),
            (400, "invalid_request_error"),
        ] {
            let rig = Rig::new(vec![error_reply(status, kind, "no")]);
            let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
            let (got, _) = t
                .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
                .unwrap();
            assert_eq!(got, status);
            assert!(t
                .summary()
                .halted
                .unwrap()
                .contains(&format!("HTTP {status} ({kind})")));
            assert!(t
                .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
                .is_err());
            assert_eq!(rig.calls_made(), 1, "no call after the halt");
            assert!(
                rig.waits.lock().unwrap().is_empty(),
                "no retry for HTTP {status}"
            );
        }
    }

    #[test]
    fn a_rate_limit_waits_as_told_then_tries_again() {
        let mut limited = error_reply(429, "rate_limit_error", "slow down").unwrap();
        limited.retry_after_s = Some(2);
        let rig = Rig::new(vec![
            Ok(limited),
            error_reply(529, "overloaded_error", "busy"),
            ok_reply("{}", json!({ "input_tokens": 10, "output_tokens": 2 })),
        ]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        let (status, _) = t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap();
        assert_eq!(status, 200);
        // The first wait is the header's 2 s; the second has no header and backs off 4 s.
        assert_eq!(*rig.waits.lock().unwrap(), vec![2, 4]);
        let s = t.summary();
        assert_eq!(s.halted, None);
        assert_eq!(s.ledger.requests, 3);
        assert_eq!(s.ledger.spent_nano, 10 * 100 + 2 * 500);
    }

    #[test]
    fn a_burst_of_busy_answers_halts_the_run() {
        let busy = || error_reply(529, "overloaded_error", "busy");
        let rig = Rig::new((0..MAX_ATTEMPTS).map(|_| busy()).collect());
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        let (status, _) = t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap();
        assert_eq!(status, 529);
        assert_eq!(rig.calls_made(), MAX_ATTEMPTS as usize);
        assert!(t
            .summary()
            .halted
            .unwrap()
            .contains("burst of busy answers"));
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_err());
        assert_eq!(rig.calls_made(), MAX_ATTEMPTS as usize);
    }

    #[test]
    fn the_log_carries_the_request_id_and_never_the_key() {
        // The error message echoes the key on purpose: it must still not reach any file.
        let rig = Rig::new(vec![
            ok_reply("{}", json!({ "input_tokens": 10, "output_tokens": 2 })),
            error_reply(401, "authentication_error", &format!("bad key {TEST_KEY}")),
        ]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        t.post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap();
        let (_, error_body) = t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap();
        assert!(!error_body.contains(TEST_KEY));
        let log = rig.log();
        let lines: Vec<Value> = log
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["request_id"], "req_ok_1");
        assert_eq!(lines[0]["outcome"], "ok");
        assert_eq!(lines[0]["cost_nano"], 2_000);
        assert_eq!(lines[0]["seed_ignored"], 12345);
        assert_eq!(lines[1]["request_id"], "req_err_401");
        assert_eq!(lines[1]["error_type"], "authentication_error");
        assert_eq!(lines[1]["error_message"], "bad key [redacted]");
        assert_eq!(lines[2]["event"], "halt");
        // No header name or value is written anywhere, and the key is in no file at all.
        for entry in fs::read_dir(rig.dir.path()).unwrap() {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(!text.contains(TEST_KEY));
            assert!(!text.contains(&TEST_KEY[19..39]));
            assert!(!text.contains("x-api-key"));
        }
        assert_eq!(
            format!("{:?}", ApiKey::for_tests(TEST_KEY)),
            "ApiKey(redacted)"
        );
        assert!(!format!("{t:?}").contains(TEST_KEY));
    }

    #[test]
    fn a_key_with_a_quote_or_backslash_is_refused_and_never_logged() {
        // Such a key is refused where keys come in.
        for bad in [
            "test-key-0123456789\"abcdefghij",
            "test-key-0123456789\\abcdefghij",
        ] {
            assert_eq!(
                ApiKey::from_text(bad).unwrap_err(),
                "does not hold one key",
                "{bad}"
            );
        }
        assert!(ApiKey::from_text(TEST_KEY).is_ok());

        // And if one got in anyway, an error body that echoes it still never reaches the log,
        // neither as written nor in its JSON-escaped form.
        let key = "test-key-0123456789\"quote\\slash-KLMNOPQRST";
        let rig = Rig::new(vec![error_reply(
            401,
            "authentication_error",
            &format!("bad key {key}"),
        )]);
        let waits = rig.waits.clone();
        let t = ClaudeTransport::with_sleep(
            ApiKey::for_tests(key),
            rig.cfg(HAIKU, ONE_DOLLAR),
            Box::new(Scripted(rig.script.clone())),
            Box::new(move |d| waits.lock().unwrap().push(d.as_secs())),
        )
        .unwrap();
        let _ = t.post_generate("", &ollama_body(HAIKU), Duration::from_secs(5));
        let quoted = serde_json::to_string(key).unwrap();
        let escaped = &quoted[1..quoted.len() - 1];
        assert_ne!(escaped, key, "the key must change when escaped");
        let log = rig.log();
        assert!(!log.is_empty());
        assert!(!log.contains(key), "{log}");
        assert!(!log.contains(escaped), "{log}");
        let first: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(first["error_message"], "bad key [redacted]");
    }

    #[test]
    fn warm_up_and_the_model_list_make_no_call() {
        let rig = Rig::new(vec![]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        let warm = json!({
            "model": HAIKU, "prompt": "Warm up.", "stream": false, "think": false,
            "options": { "num_predict": 1 }
        })
        .to_string();
        assert_eq!(
            t.post_generate("", &warm, Duration::from_secs(5)).unwrap(),
            (200, "{\"response\":\"\"}".to_string())
        );
        let (status, tags) = t.get_tags("", Duration::from_secs(5)).unwrap();
        assert_eq!(status, 200);
        let tags: Value = serde_json::from_str(&tags).unwrap();
        assert_eq!(tags["models"][0]["name"], HAIKU);
        assert_eq!(rig.calls_made(), 0);
        assert_eq!(t.summary().ledger, Ledger::default());
    }

    #[test]
    fn a_refusal_or_a_missing_usage_is_handled() {
        let refusal = Ok(HttpReply {
            status: 200,
            body: json!({
                "content": [{ "type": "text", "text": "partial" }],
                "stop_reason": "refusal",
                "usage": { "input_tokens": 5, "output_tokens": 1 }
            })
            .to_string(),
            request_id: Some("req_refused".to_string()),
            retry_after_s: None,
        });
        let no_usage = Ok(HttpReply {
            status: 200,
            body: json!({ "content": [], "stop_reason": "end_turn" }).to_string(),
            request_id: None,
            retry_after_s: None,
        });
        let rig = Rig::new(vec![refusal, no_usage]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        let (_, body) = t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap();
        assert_eq!(body, "{\"response\":\"\"}");
        assert!(rig.log().contains("\"stop_reason\":\"refusal\""));
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_err());
        assert!(t.summary().halted.unwrap().contains("no usage"));
    }

    #[test]
    fn a_timeout_passes_through_and_costs_nothing() {
        let rig = Rig::new(vec![Err(TransportError::Timeout)]);
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        let err = t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .unwrap_err();
        assert!(matches!(err, TransportError::Timeout));
        assert_eq!(t.summary().halted, None);
        assert_eq!(
            t.summary().ledger,
            Ledger {
                spent_nano: 0,
                requests: 1
            }
        );
    }

    #[test]
    fn network_failures_in_a_row_halt_the_run() {
        let fail = || {
            Err(TransportError::ConnectionFailed(
                "tls handshake failed".to_string(),
            ))
        };
        let rig = Rig::new((0..NETWORK_FAILURE_LIMIT).map(|_| fail()).collect());
        let t = rig.transport(HAIKU, ONE_DOLLAR).unwrap();
        for _ in 0..NETWORK_FAILURE_LIMIT {
            assert!(t
                .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
                .is_err());
        }
        assert!(t.summary().halted.unwrap().contains("tls handshake failed"));
        assert!(t
            .post_generate("", &ollama_body(HAIKU), Duration::from_secs(5))
            .is_err());
        assert_eq!(rig.calls_made(), NETWORK_FAILURE_LIMIT as usize);
        assert!(rig.log().contains("\"error\":\"tls handshake failed\""));
    }

    #[test]
    fn the_key_file_must_sit_outside_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("key.txt");
        fs::write(&good, format!("\u{feff}{TEST_KEY}\r\n")).unwrap();
        let key = ApiKey::read_from_file(&good).unwrap();
        assert_eq!(key.expose(), TEST_KEY);

        let repo = dir.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("sub")).unwrap();
        let inside = repo.join("sub").join("key.txt");
        fs::write(&inside, TEST_KEY).unwrap();
        let err = ApiKey::read_from_file(&inside).unwrap_err();
        assert!(err.contains("inside a git repository"));
        assert!(!err.contains(TEST_KEY));

        let bad = dir.path().join("two.txt");
        fs::write(&bad, "two words here and more words to pass length").unwrap();
        assert!(ApiKey::read_from_file(&bad).is_err());
        assert!(ApiKey::read_from_file(&dir.path().join("missing.txt")).is_err());
    }

    #[test]
    fn dollars_parse_to_billionths_and_print_back() {
        assert_eq!(usd_to_nano("15").unwrap(), 15_000_000_000);
        assert_eq!(usd_to_nano("0.25").unwrap(), 250_000_000);
        assert_eq!(usd_to_nano("1.000000001").unwrap(), 1_000_000_001);
        assert!(usd_to_nano("1.0000000001").is_err());
        assert!(usd_to_nano("-1").is_err());
        assert!(usd_to_nano("abc").is_err());
        assert!(usd_to_nano("").is_err());
        assert_eq!(nano_as_usd(153_000), "0.000153");
        assert_eq!(nano_as_usd(15_000_000_000), "15.000000");
    }

    #[test]
    fn cloud_flags_are_split_from_the_harness_flags() {
        let args: Vec<String> = [
            "--turn-set",
            "t.json",
            "--cloud",
            "claude",
            "--model",
            HAIKU,
            "--cloud-key-file",
            "k.txt",
            "--cloud-cap-usd",
            "15",
            "--cloud-max-requests",
            "30000",
            "--cloud-ledger",
            "l.json",
            "--cloud-log",
            "c.jsonl",
            "--candidates",
            "3",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (cloud, rest) = split_cloud_args(&args).unwrap();
        let cloud = cloud.unwrap();
        assert_eq!(
            rest,
            [
                "--turn-set",
                "t.json",
                "--model",
                HAIKU,
                "--candidates",
                "3"
            ]
        );
        assert_eq!(cloud.key_file, PathBuf::from("k.txt"));
        assert_eq!(cloud.cfg.model, HAIKU);
        assert_eq!(cloud.cfg.cap_nano, 15_000_000_000);
        assert_eq!(cloud.cfg.max_requests, 30_000);
        assert_eq!(cloud.cfg.halt_path(), PathBuf::from("l.json.HALT"));
        assert!(cloud.cfg.cache_marker);
        assert_eq!(cloud.cfg.answer_tokens, None);
        let mut wider = args.clone();
        wider.extend(["--cloud-answer-tokens".to_string(), "300".to_string()]);
        assert_eq!(
            split_cloud_args(&wider)
                .unwrap()
                .0
                .unwrap()
                .cfg
                .answer_tokens,
            Some(300)
        );
        let mut no_marker = args.clone();
        no_marker.push("--cloud-no-cache-marker".to_string());
        let (cloud, rest2) = split_cloud_args(&no_marker).unwrap();
        assert!(!cloud.unwrap().cfg.cache_marker);
        assert_eq!(rest2, rest);

        let plain: Vec<String> = vec!["--turn-set".into(), "t.json".into()];
        assert_eq!(split_cloud_args(&plain).unwrap(), (None, plain.clone()));
        let missing: Vec<String> = ["--cloud", "claude", "--model", HAIKU]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(split_cloud_args(&missing)
            .unwrap_err()
            .contains("is required with --cloud"));
        let wrong: Vec<String> = ["--cloud", "other"].iter().map(|s| s.to_string()).collect();
        assert!(split_cloud_args(&wrong).is_err());
    }
}
