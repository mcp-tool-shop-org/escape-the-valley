use std::collections::BTreeMap;
#[cfg(feature = "authoring")]
use std::fs;
#[cfg(feature = "authoring")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::events_builtin::builtin_hand_authored_events;
use crate::model::RunState;
use crate::save::SaveLoadError;
use crate::worldgen::{parse_event_skeletons_json, EventSkeleton};

/// Logic version string included in the canonical `content_hash` byte stream.
///
/// `content_hash` covers the JSON content files (`content_manifest.json`, `event_skeletons.json`)
/// and `LOGIC_VERSION`. Because the 60 built-in events (`events_builtin.rs`) live in Rust code
/// rather than JSON content files, editing their text or mechanics requires bumping `LOGIC_VERSION`
/// so that `content_hash` changes accordingly.
pub const LOGIC_VERSION: &str = "etv-rust-2.0-logic-v1";

/// Embedded `content_manifest.json` bytes from `rust/etv-core/content/content_manifest.json`.
pub const CONTENT_MANIFEST_JSON: &str = include_str!("../content/content_manifest.json");

/// Embedded `event_skeletons.json` bytes from `rust/etv-core/content/event_skeletons.json`.
pub const EVENT_SKELETONS_JSON: &str = include_str!("../content/event_skeletons.json");

/// Expected 64-hex `content_hash` of the shipped embedded content pack at `LOGIC_VERSION`.
pub const EMBEDDED_CONTENT_HASH_HEX: &str =
    "7836724a01c9db8e8e97b234a817b4f3be44a923677292af96f8426d6540fe74";

/// Manifest describing the content pack schema version and ordered file list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentManifest {
    pub schema_version: i64,
    pub files: Vec<String>,
}

/// Loaded content pack containing raw content files in deterministic `BTreeMap` order
/// and parsed event skeletons in deterministic `Vec` order (no `HashMap` or `HashSet`).
#[derive(Debug, Clone)]
pub struct ContentPack {
    pub logic_version: String,
    pub manifest: Option<ContentManifest>,
    pub files: BTreeMap<String, String>,
    pub raw_manifest_value: Value,
    pub raw_events_value: Value,
    pub json_events: Vec<EventSkeleton>,
    pub all_events: Vec<EventSkeleton>,
    pub content_hash: [u8; 32],
    pub content_hash_hex: String,
}

impl ContentPack {
    /// Load the compile-time embedded content pack (`content_manifest.json` + `event_skeletons.json`).
    pub fn embedded() -> Self {
        Self::from_raw_strings(CONTENT_MANIFEST_JSON, EVENT_SKELETONS_JSON)
            .expect("embedded content pack must be valid")
    }

    /// Construct a `ContentPack` from raw `content_manifest.json` and `event_skeletons.json` strings.
    pub fn from_raw_strings(manifest_json: &str, events_json: &str) -> Result<Self, String> {
        Self::from_raw_strings_with_logic_version(manifest_json, events_json, LOGIC_VERSION)
    }

    /// Construct a `ContentPack` with an explicit `logic_version` string.
    pub fn from_raw_strings_with_logic_version(
        manifest_json: &str,
        events_json: &str,
        logic_version: &str,
    ) -> Result<Self, String> {
        let raw_manifest_value: Value = serde_json::from_str(manifest_json)
            .map_err(|e| format!("invalid content_manifest.json JSON: {e}"))?;
        let raw_events_value: Value = serde_json::from_str(events_json)
            .map_err(|e| format!("invalid event_skeletons.json JSON: {e}"))?;

        let manifest: Option<ContentManifest> =
            serde_json::from_value(raw_manifest_value.clone()).ok();

        let mut files = BTreeMap::new();
        files.insert(
            "content_manifest.json".to_string(),
            manifest_json.to_string(),
        );
        files.insert("event_skeletons.json".to_string(), events_json.to_string());

        let json_events = parse_event_skeletons_json(events_json).unwrap_or_default();
        let mut all_events = builtin_hand_authored_events();
        all_events.extend(json_events.iter().cloned());

        let mut pack = Self {
            logic_version: logic_version.to_string(),
            manifest,
            files,
            raw_manifest_value,
            raw_events_value,
            json_events,
            all_events,
            content_hash: [0u8; 32],
            content_hash_hex: String::new(),
        };
        let hash = try_compute_content_hash(&pack)?;
        pack.content_hash = hash;
        pack.content_hash_hex = hex_encode_32(&hash);
        Ok(pack)
    }

    /// Load a `ContentPack` from a directory on disk at startup (no recompile required).
    /// Only available when compiled with `--features authoring`.
    #[cfg(feature = "authoring")]
    pub fn load_from_dir(dir: &Path) -> Result<Self, String> {
        let manifest_path = dir.join("content_manifest.json");
        let manifest_json = fs::read_to_string(&manifest_path).map_err(|e| {
            format!(
                "failed to read content_manifest.json from {}: {e}",
                manifest_path.display()
            )
        })?;
        let events_path = dir.join("event_skeletons.json");
        let events_json = fs::read_to_string(&events_path).map_err(|e| {
            format!(
                "failed to read event_skeletons.json from {}: {e}",
                events_path.display()
            )
        })?;
        Self::from_raw_strings(&manifest_json, &events_json)
    }
}

/// Load a `ContentPack` from an on-disk directory when compiled with `--features authoring`.
#[cfg(feature = "authoring")]
pub fn load_content_from_dir(dir: &Path) -> Result<ContentPack, String> {
    ContentPack::load_from_dir(dir)
}

/// Return a cached reference to the compile-time embedded `ContentPack`.
pub fn embedded_content_pack() -> &'static ContentPack {
    static EMBEDDED: OnceLock<ContentPack> = OnceLock::new();
    EMBEDDED.get_or_init(ContentPack::embedded)
}

/// Return the 32-byte `content_hash` of the embedded content pack.
pub fn embedded_content_hash() -> [u8; 32] {
    embedded_content_pack().content_hash
}

/// Return the 64-hex `content_hash` string of the embedded content pack.
pub fn embedded_content_hash_hex() -> String {
    embedded_content_pack().content_hash_hex.clone()
}

/// Format a 32-byte SHA-256 digest as lowercase 64-character hex.
pub fn hex_encode_32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Canonicalize a JSON `Value` recursively via `BTreeMap` key sorting, compact UTF-8,
/// `\n` line endings, and rejecting any floating-point numbers in content files.
pub fn canonicalize_json_value_no_floats(val: &Value) -> Result<String, String> {
    let mut out = String::new();
    write_canonical_json_no_floats(val, &mut out)?;
    out.push('\n');
    Ok(out)
}

fn write_canonical_json_no_floats(val: &Value, out: &mut String) -> Result<(), String> {
    match val {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let raw = n.to_string();
            if n.is_f64() || raw.contains('.') || raw.contains('e') || raw.contains('E') {
                return Err(format!(
                    "floating-point number '{raw}' is forbidden in content files"
                ));
            }
            out.push_str(&raw);
        }
        Value::String(s) => {
            let normalized = s.replace("\r\n", "\n").replace('\r', "\n");
            let encoded = serde_json::to_string(&normalized)
                .map_err(|e| format!("failed to encode string: {e}"))?;
            out.push_str(&encoded);
        }
        Value::Array(arr) => {
            out.push('[');
            for (idx, item) in arr.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_canonical_json_no_floats(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut sorted: BTreeMap<&str, &Value> = BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k.as_str(), v);
            }
            out.push('{');
            for (idx, (k, v)) in sorted.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                let normalized_key = k.replace("\r\n", "\n").replace('\r', "\n");
                let encoded_key = serde_json::to_string(&normalized_key)
                    .map_err(|e| format!("failed to encode object key: {e}"))?;
                out.push_str(&encoded_key);
                out.push(':');
                write_canonical_json_no_floats(v, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// Build the canonical byte stream for `content` and compute its SHA-256 digest (`[u8; 32]`).
///
/// Canonical byte stream:
/// `LOGIC_VERSION` string + `0x00` + for each content file in lexicographic relative-path order
/// (`content_manifest.json`, `event_skeletons.json`), relative path UTF-8 + `0x00` +
/// canonicalized JSON bytes (keys sorted recursively via `BTreeMap`, compact UTF-8, `\n` line
/// endings, no floats in content files) + `0x00`.
pub fn try_compute_content_hash(content: &ContentPack) -> Result<[u8; 32], String> {
    let mut hasher = Sha256::new();
    hasher.update(content.logic_version.as_bytes());
    hasher.update([0x00]);

    for (rel_path, raw_json) in &content.files {
        let normalized_rel_path = rel_path.replace('\\', "/");
        let parsed: Value = serde_json::from_str(raw_json)
            .map_err(|e| format!("failed to parse {normalized_rel_path}: {e}"))?;
        let canonical_json = canonicalize_json_value_no_floats(&parsed)?;
        hasher.update(normalized_rel_path.as_bytes());
        hasher.update([0x00]);
        hasher.update(canonical_json.as_bytes());
        hasher.update([0x00]);
    }

    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Ok(out)
}

/// Compute the 32-byte `content_hash` for a valid `ContentPack`.
pub fn compute_content_hash(content: &ContentPack) -> [u8; 32] {
    try_compute_content_hash(content).expect("content pack must canonicalize without floats")
}

/// Compute the 64-hex `content_hash` string for a valid `ContentPack`.
pub fn compute_content_hash_hex(content: &ContentPack) -> String {
    let digest = compute_content_hash(content);
    hex_encode_32(&digest)
}

/// Return the version string including `LOGIC_VERSION` and `content_hash` for `etv --version`.
pub fn version_with_content_hash(content: &ContentPack) -> String {
    format!(
        "etv {} (logic_version={}, content_hash={})",
        env!("CARGO_PKG_VERSION"),
        content.logic_version,
        content.content_hash_hex
    )
}

/// Return the default version string for the embedded content pack.
pub fn embedded_version_string() -> String {
    version_with_content_hash(embedded_content_pack())
}

/// Header struct for the Trail Ledger export carrying `content_hash`.
///
/// Note: `TrailLedgerHeader` is a header type only in Phase 1 until the full Rust Trail Ledger
/// implementation exists in Phase 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrailLedgerHeader {
    pub schema_version: u32,
    pub logic_version: String,
    pub content_hash: String,
    pub run_id: String,
    pub seed: i128,
    pub day: i32,
    pub distance_traveled: i32,
    pub total_distance: i32,
    pub victory: bool,
    pub game_over: bool,
}

impl TrailLedgerHeader {
    /// Construct a `TrailLedgerHeader` from a `RunState`.
    pub fn from_state(state: &RunState) -> Self {
        Self {
            schema_version: 1,
            logic_version: LOGIC_VERSION.to_string(),
            content_hash: state.content_hash.clone(),
            run_id: state.run_id.clone(),
            seed: state.seed,
            day: state.day,
            distance_traveled: state.distance_traveled,
            total_distance: state.total_distance,
            victory: state.victory,
            game_over: state.game_over,
        }
    }

    /// Verify that the ledger header's `content_hash` matches the running content hash.
    pub fn verify_content_hash(&self, running_content_hash: &str) -> Result<(), SaveLoadError> {
        if self.content_hash != running_content_hash {
            return Err(SaveLoadError::ContentVersionMismatch {
                save_content_hash: self.content_hash.clone(),
                running_content_hash: running_content_hash.to_string(),
            });
        }
        Ok(())
    }
}

/// Command-line command parsed for the `etv` binary / CLI helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Version,
    ValidateContent { require_outcomes: bool },
    InspectEvent { event_id: String },
    RunSeed { seed: i128, turns: usize },
}

/// Parsed CLI options for the `etv` binary / CLI helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOptions {
    pub command: CliCommand,
    pub content_dir: Option<PathBuf>,
}

/// Parse CLI arguments (`--version`, `validate-content [--require-outcomes]`,
/// `inspect-event <id>`, `run-seed <seed> [--turns <n>]`, and `--content-dir <dir>`).
///
/// Rejects `--content-dir` in builds compiled without `--features authoring`.
pub fn parse_cli_args(args: &[String]) -> Result<CliOptions, String> {
    #[cfg_attr(not(feature = "authoring"), allow(unused_mut))]
    let mut content_dir: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut require_outcomes = false;
    let mut version_flag = false;
    let mut turns: usize = 60;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--version" || arg == "-V" {
            version_flag = true;
            i += 1;
        } else if arg == "--require-outcomes" {
            require_outcomes = true;
            i += 1;
        } else if arg == "--turns" {
            let val = args
                .get(i + 1)
                .ok_or_else(|| "missing value for '--turns'".to_string())?;
            turns = val
                .parse::<usize>()
                .map_err(|e| format!("invalid '--turns' value '{val}': {e}"))?;
            i += 2;
        } else if arg == "--content-dir" || arg.starts_with("--content-dir=") {
            let (dir_str, step) = if let Some(rest) = arg.strip_prefix("--content-dir=") {
                (rest.to_string(), 1)
            } else {
                let next = args
                    .get(i + 1)
                    .ok_or_else(|| "missing path for '--content-dir'".to_string())?;
                (next.clone(), 2)
            };
            #[cfg(not(feature = "authoring"))]
            {
                let _ = (dir_str, step);
                return Err(
                    "flag '--content-dir' is not available in release builds (requires cargo feature 'authoring')"
                        .to_string(),
                );
            }
            #[cfg(feature = "authoring")]
            {
                content_dir = Some(PathBuf::from(dir_str));
                i += step;
            }
        } else {
            positional.push(arg.clone());
            i += 1;
        }
    }

    if version_flag {
        return Ok(CliOptions {
            command: CliCommand::Version,
            content_dir,
        });
    }

    let sub = positional.first().map(String::as_str).ok_or_else(|| {
        "expected command: --version | validate-content | inspect-event <id> | run-seed <seed>"
            .to_string()
    })?;

    let command = match sub {
        "validate-content" => CliCommand::ValidateContent { require_outcomes },
        "inspect-event" => {
            let event_id = positional
                .get(1)
                .cloned()
                .ok_or_else(|| "inspect-event requires an <event_id> argument".to_string())?;
            CliCommand::InspectEvent { event_id }
        }
        "run-seed" => {
            let seed_str = positional
                .get(1)
                .ok_or_else(|| "run-seed requires a <seed> argument".to_string())?;
            let seed = seed_str
                .parse::<i128>()
                .map_err(|e| format!("invalid seed '{seed_str}': {e}"))?;
            CliCommand::RunSeed { seed, turns }
        }
        other => return Err(format!("unknown subcommand '{other}'")),
    };

    Ok(CliOptions {
        command,
        content_dir,
    })
}
