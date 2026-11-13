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
    let Ok(settings) = NarratorSettings::load_from_file(&path) else {
        return defaults_with_warning(
            "The settings file could not be read. Using the default settings.".to_string(),
        );
    };
    match validate_model_name(&settings.model) {
        Ok(_) => LoadedSettings {
            settings,
            from_file: true,
            warning: None,
        },
        Err(reason) => defaults_with_warning(format!(
            "The settings file was ignored ({reason}). Using the default settings."
        )),
    }
}

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
