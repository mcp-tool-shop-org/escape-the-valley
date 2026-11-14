//! Player settings file (`<base>/.trail/settings.json`).
//!
//! Holds the narrator choices a player makes on the settings screen: narrator on or off, the
//! profile, the model name and the length. It is presentation only. Nothing here is read by the
//! engine, written to the save, or included in `content_hash` or the mechanics hash.

use std::path::{Path, PathBuf};

use crate::narrator::NarratorSettings;
use crate::save::{atomic_write_text, SAVE_DIR};

/// File name of the player settings file inside the `.trail` directory.
pub const SETTINGS_FILE: &str = "settings.json";

/// Longest model name the settings screen accepts, in characters.
pub const MAX_MODEL_NAME_CHARS: usize = 100;

/// Path of the player settings file under `base_path`.
pub fn settings_path(base_path: &Path) -> PathBuf {
    base_path.join(SAVE_DIR).join(SETTINGS_FILE)
}

/// Result of reading the player settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSettings {
    pub settings: NarratorSettings,
    /// `true` when a settings file was found and accepted.
    pub from_file: bool,
    /// A plain message for the player when a file was found but could not be used. The defaults
    /// are returned alongside it, and the unusable file is left untouched.
    pub warning: Option<String>,
}

fn defaults_with_warning(warning: String) -> LoadedSettings {
    LoadedSettings {
        settings: NarratorSettings::default(),
        from_file: false,
        warning: Some(warning),
    }
}

/// Load the player settings under `base_path`, or the defaults when no file exists.
///
/// A file that cannot be read or parsed never stops the game: the defaults are returned with a
/// warning to show the player.
pub fn load_settings_or_default(base_path: &Path) -> LoadedSettings {
    let path = settings_path(base_path);
    if !path.exists() {
        return LoadedSettings {
            settings: NarratorSettings::default(),
            from_file: false,
            warning: None,
        };
    }
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return defaults_with_warning(format!(
            "The settings file could not be read. {USING_DEFAULTS_LINE}"
        ));
    };
    // A file that reads but does not hold usable settings says why: a cloud provider with no
    // key, a non-https provider URL, a refused endpoint or broken JSON. The defaults it falls
    // back to are named, so a player who chose a cloud narrator is told it is not in use.
    let settings = match NarratorSettings::from_json_str(&raw) {
        Ok(settings) => settings,
        Err(reason) => {
            return defaults_with_warning(format!(
                "The settings file was not used: {reason}. {USING_DEFAULTS_LINE}"
            ))
        }
    };
    match validate_model_name(&settings.model) {
        Ok(_) => LoadedSettings {
            settings,
            from_file: true,
            warning: None,
        },
        Err(reason) => defaults_with_warning(format!(
            "The settings file was ignored ({reason}). {USING_DEFAULTS_LINE}"
        )),
    }
}

/// What a settings warning ends with: the settings the game uses instead.
pub const USING_DEFAULTS_LINE: &str =
    "Using the default settings: the local narrator (Ollama), until the settings are fixed.";

/// Serialise `settings` as the pretty JSON text written to the settings file (LF line endings,
/// trailing newline).
pub fn settings_to_json(settings: &NarratorSettings) -> String {
    let mut text =
        serde_json::to_string_pretty(settings).expect("NarratorSettings always serialises to JSON");
    text.push('\n');
    text
}

/// Write `settings` to `<base_path>/.trail/settings.json` atomically and return the path.
pub fn save_settings(settings: &NarratorSettings, base_path: &Path) -> std::io::Result<PathBuf> {
    let path = settings_path(base_path);
    atomic_write_text(&path, &settings_to_json(settings), false)?;
    Ok(path)
}

/// Check a model name typed on the settings screen and return it trimmed.
///
/// A name is 1 to [`MAX_MODEL_NAME_CHARS`] characters of ASCII letters, digits and `. _ - : /`.
pub fn validate_model_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("the model name is empty".to_string());
    }
    if name.chars().count() > MAX_MODEL_NAME_CHARS {
        return Err(format!(
            "the model name is longer than {MAX_MODEL_NAME_CHARS} characters"
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/')))
    {
        return Err(format!(
            "the model name contains {bad:?}; use letters, digits and . _ - : / only"
        ));
    }
    Ok(name.to_string())
}

/// The model name as it may be shown to the player, or `None` when it may not be.
///
/// A model name comes from the settings file, where a key may have been pasted, so it is shown
/// only when it is exactly one the game knows: the built-in default
/// ([`crate::narrator::DEFAULT_NARRATOR_MODEL`]), one of the Claude models
/// ([`crate::narrator_provider::CLAUDE_MODELS`]), or a name the local Ollama service listed as
/// installed (`installed`; pass none for a hosted provider, whose listing is its own setting).
/// Every place that shows a model asks this; anything else reads "the configured model".
pub fn model_for_display<'a>(model: &'a str, installed: &[String]) -> Option<&'a str> {
    let known = model == crate::narrator::DEFAULT_NARRATOR_MODEL
        || crate::narrator_provider::CLAUDE_MODELS
            .iter()
            .any(|(id, _)| *id == model)
        || installed.iter().any(|n| n == model);
    known.then_some(model)
}

/// What a model is called where it may not be named.
pub const CONFIGURED_MODEL: &str = "the configured model";
