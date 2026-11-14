//! Line-mode screens that are the same in every front end: the title screen, the about screen
//! and the settings screen.
//!
//! Every function here is pure text in, text out. The screens read and change only
//! [`NarratorSettings`]; they never see the engine, the RNG or the save.

use std::io::{BufRead, Write};

use crate::model::GMProfile;
use crate::narrator::{profile_interface_name, NarratorLength, NarratorSettings};
use crate::settings::{model_for_display, validate_model_name, CONFIGURED_MODEL};

/// AI disclosure shown on the title screen.
pub const TITLE_DISCLOSURE: &str = "When a narrator model is running, the story is written and voiced by AI, live, as you play, and every telling is new. Without one, the game shows plain text lines instead.";

/// AI disclosure shown on the about screen and in the README.
pub const CREDITS_DISCLOSURE: &str =
    "Narration written and voiced live by AI (local models you choose).";

/// The three narrator profiles in the order the settings screen cycles through them.
pub const PROFILE_ORDER: [GMProfile; 3] = [
    GMProfile::Chronicler,
    GMProfile::Fireside,
    GMProfile::Lantern,
];

/// The three narrator lengths in the order the settings screen cycles through them.
pub const LENGTH_ORDER: [NarratorLength; 3] = [
    NarratorLength::Brief,
    NarratorLength::Standard,
    NarratorLength::Rich,
];

/// One-line description of a profile, as the README's profile list words it.
pub fn profile_description(profile: GMProfile) -> &'static str {
    match profile {
        GMProfile::Chronicler => {
            "Grounded, practical, spare. Minimal folklore. Reports what happened."
        }
        GMProfile::Fireside => "Serious campfire narrator. Subtle uncanny moments. The default.",
        GMProfile::Lantern => {
            "Uncanny and liminal, but still grounded in consequences. The weird one."
        }
    }
}

/// How the settings screen describes a length: short, medium or long. The word caps behind
/// them live in the code only (see [`NarratorLength::word_cap`]).
pub fn length_description(length: NarratorLength) -> String {
    match length {
        NarratorLength::Brief => "short",
        NarratorLength::Standard => "medium",
        NarratorLength::Rich => "long",
    }
    .to_string()
}

/// Prompt shown while the settings screen waits for a model name.
pub const MODEL_PROMPT: &str = "Model name (empty keeps the current one):";

fn on_off(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off"
    }
}

/// Lines of the title screen. `version` is the plain version number, for example `2.0.0`.
pub fn title_screen_lines(version: &str) -> Vec<String> {
    vec![
        "ESCAPE THE VALLEY".to_string(),
        format!("version {version}"),
        String::new(),
        TITLE_DISCLOSURE.to_string(),
    ]
}

/// Lines of the about (credits) screen.
pub fn about_screen_lines(version: &str, content_hash_hex: &str) -> Vec<String> {
    vec![
        "ESCAPE THE VALLEY".to_string(),
        format!("version {version}"),
        format!("content {content_hash_hex}"),
        String::new(),
        CREDITS_DISCLOSURE.to_string(),
        format!(
            "Narrator profiles: {}, {}, {}.",
            profile_interface_name(GMProfile::Chronicler),
            profile_interface_name(GMProfile::Fireside),
            profile_interface_name(GMProfile::Lantern)
        ),
        String::new(),
        "MIT licence. Third-party licences: etv --notices".to_string(),
    ]
}

/// Lines of the settings screen for the current `settings`. The model is named only when it is
/// one the game knows ([`model_for_display`]); a model from the settings file otherwise reads
/// "the configured model". [`SettingsScreen::lines`] also names one the player has just typed.
pub fn settings_screen_lines(settings: &NarratorSettings) -> Vec<String> {
    settings_lines_naming(settings, model_for_display(&settings.model, &[]))
}

fn settings_lines_naming(settings: &NarratorSettings, model: Option<&str>) -> Vec<String> {
    let mut lines = vec![
        "SETTINGS".to_string(),
        format!("  1. Narrator  {}", on_off(settings.enabled)),
        format!(
            "  2. Profile   {} - {}",
            profile_interface_name(settings.profile),
            profile_description(settings.profile)
        ),
        format!(
            "  3. Length    {} ({})",
            settings.length.as_str(),
            length_description(settings.length)
        ),
        format!("  4. Model     {}", model.unwrap_or(CONFIGURED_MODEL)),
    ];
    // A cloud provider is named, with the chosen Claude model's one line, so the player knows
    // what it costs. The local default adds nothing.
    use crate::narrator_provider::{claude_model_description, NarratorProvider};
    if settings.provider != NarratorProvider::Ollama {
        lines.push(format!("     Provider  {}", settings.provider.name()));
        if settings.provider == NarratorProvider::Claude {
            if let Some(line) = claude_model_description(&settings.model) {
                lines.push(format!("     {line}"));
            }
        }
    }
    lines.push("  S. Save and leave    Q. Leave without saving".to_string());
    lines.push(String::new());
    lines.push(TITLE_DISCLOSURE.to_string());
    lines
}

fn next_in<T: Copy + PartialEq>(order: &[T], current: T) -> T {
    let idx = order.iter().position(|v| *v == current).unwrap_or(0);
    order[(idx + 1) % order.len()]
}

fn parse_profile(word: &str) -> Option<GMProfile> {
    match word {
        "chronicler" => Some(GMProfile::Chronicler),
        "fireside" => Some(GMProfile::Fireside),
        "lantern" | "lantern-bearer" | "lantern bearer" => Some(GMProfile::Lantern),
        _ => None,
    }
}

fn parse_length(word: &str) -> Option<NarratorLength> {
    match word {
        "brief" => Some(NarratorLength::Brief),
        "standard" => Some(NarratorLength::Standard),
        "rich" => Some(NarratorLength::Rich),
        _ => None,
    }
}

/// What the settings screen does with one line of input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEvent {
    /// A setting changed, or was confirmed; the message says what it is now.
    Changed(String),
    /// The screen is waiting for the model name on the next line.
    AskModel,
    /// The input was not understood or not accepted; the message says why.
    Rejected(String),
    /// The player asked to save and leave.
    Save,
    /// The player asked to leave without saving.
    Discard,
}

/// State of the settings screen: the settings being edited and whether the next line is a
/// model name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsScreen {
    pub settings: NarratorSettings,
    awaiting_model: bool,
    /// A model name the player typed on this screen. It may be shown back while the screen is
    /// open; a model that came from the settings file is shown only by [`model_for_display`].
    typed_model: Option<String>,
}

impl SettingsScreen {
    pub fn new(settings: NarratorSettings) -> Self {
        Self {
            settings,
            awaiting_model: false,
            typed_model: None,
        }
    }

    /// The screen's lines, naming the model when it is one the game knows or the one the player
    /// just typed here.
    pub fn lines(&self) -> Vec<String> {
        settings_lines_naming(&self.settings, self.shown_model())
    }

    fn shown_model(&self) -> Option<&str> {
        match &self.typed_model {
            Some(typed) if *typed == self.settings.model => Some(typed.as_str()),
            _ => model_for_display(&self.settings.model, &[]),
        }
    }

    /// `true` while the next line of input is read as a model name.
    pub fn awaiting_model(&self) -> bool {
        self.awaiting_model
    }

    fn set_model(&mut self, raw: &str) -> SettingsEvent {
        match validate_model_name(raw) {
            // The claude provider takes only its listed models: refused here as the loader
            // would refuse it at the next start, without showing what was typed.
            Ok(name)
                if self.settings.provider == crate::narrator_provider::NarratorProvider::Claude
                    && !crate::narrator_provider::is_claude_model(&name) =>
            {
                SettingsEvent::Rejected(format!(
                    "Model not changed: {}.",
                    crate::narrator_provider::claude_model_refusal()
                ))
            }
            Ok(name) => {
                self.settings.model = name.clone();
                self.typed_model = Some(name);
                SettingsEvent::Changed(format!("Model is now {}.", self.settings.model))
            }
            Err(reason) => SettingsEvent::Rejected(format!("Model not changed: {reason}.")),
        }
    }

    fn set_enabled(&mut self, enabled: bool) -> SettingsEvent {
        self.settings.enabled = enabled;
        SettingsEvent::Changed(format!("Narrator is now {}.", on_off(enabled)))
    }

    fn set_profile(&mut self, profile: GMProfile) -> SettingsEvent {
        self.settings.profile = profile;
        SettingsEvent::Changed(format!(
            "Profile is now {}.",
            profile_interface_name(profile)
        ))
    }

    fn set_length(&mut self, length: NarratorLength) -> SettingsEvent {
        self.settings.length = length;
        SettingsEvent::Changed(format!("Length is now {}.", length.as_str()))
    }

    /// Apply one line of player input.
    ///
    /// Accepted: `1` to `4`; `s` and `q`; a profile name; a length name; `on` and `off`; and
    /// `model <name>`. After `4`, the next line is the model name, and an empty line keeps the
    /// current one.
    pub fn handle_line(&mut self, line: &str) -> SettingsEvent {
        let trimmed = line.trim();
        if self.awaiting_model {
            self.awaiting_model = false;
            if trimmed.is_empty() {
                return SettingsEvent::Changed(match self.shown_model() {
                    Some(m) => format!("Model is still {m}."),
                    None => "The model is unchanged.".to_string(),
                });
            }
            return self.set_model(trimmed);
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("model ") {
            // Model names are case-sensitive to the server, so take the name as typed.
            return self.set_model(&trimmed["model ".len()..]);
        }
        match lower.as_str() {
            "1" => self.set_enabled(!self.settings.enabled),
            "on" => self.set_enabled(true),
            "off" => self.set_enabled(false),
            "2" => self.set_profile(next_in(&PROFILE_ORDER, self.settings.profile)),
            "3" => self.set_length(next_in(&LENGTH_ORDER, self.settings.length)),
            "4" | "model" => {
                self.awaiting_model = true;
                SettingsEvent::AskModel
            }
            "s" | "save" => SettingsEvent::Save,
            "q" | "quit" => SettingsEvent::Discard,
            other => {
                if let Some(profile) = parse_profile(other) {
                    self.set_profile(profile)
                } else if let Some(length) = parse_length(other) {
                    self.set_length(length)
                } else {
                    SettingsEvent::Rejected("Type 1, 2, 3, 4, S or Q.".to_string())
                }
            }
        }
    }
}

/// How the settings screen ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsExit {
    /// The player chose to save; the caller writes these settings.
    Save(NarratorSettings),
    /// The player left without saving, or the input ended.
    Discard,
}

/// Run the settings screen over a line reader and a writer until the player leaves.
///
/// The end of input counts as leaving without saving, so a closed stdin can never write a file.
pub fn run_settings_screen<R: BufRead, W: Write>(
    settings: NarratorSettings,
    mut input: R,
    output: &mut W,
) -> std::io::Result<SettingsExit> {
    let mut screen = SettingsScreen::new(settings);
    let mut redraw = true;
    loop {
        if redraw {
            for line in screen.lines() {
                writeln!(output, "{line}")?;
            }
        }
        if screen.awaiting_model() {
            writeln!(output, "{MODEL_PROMPT}")?;
        }
        write!(output, "> ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            writeln!(output)?;
            return Ok(SettingsExit::Discard);
        }
        match screen.handle_line(&line) {
            SettingsEvent::Changed(msg) => {
                writeln!(output, "{msg}")?;
                writeln!(output)?;
                redraw = true;
            }
            SettingsEvent::AskModel => redraw = false,
            SettingsEvent::Rejected(msg) => {
                writeln!(output, "{msg}")?;
                redraw = false;
            }
            SettingsEvent::Save => return Ok(SettingsExit::Save(screen.settings)),
            SettingsEvent::Discard => return Ok(SettingsExit::Discard),
        }
    }
}
