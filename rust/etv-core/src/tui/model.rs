//! The model, the messages and the pure `update` of the full-screen interface.
//!
//! Nothing here touches the terminal, the engine's RNG or the run state. A key press becomes a
//! [`Msg`] (see [`key_to_msg`]); [`update`] changes the [`Model`] and answers with the
//! [`Effect`]s the driver must carry out (step the engine through [`crate::play::play_turn`],
//! write the settings file, quit). The driver then feeds the result back in as another [`Msg`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::engine::{route_offered_letters, EventChoiceInfo, StepEngine};
use crate::ledger::trail_ledger;
use crate::model::{GamePhase, IntentAction, PlayerIntent};
use crate::narrator::NarratorSettings;
use crate::play::{
    choice_line, parse_camp_input, parse_choice_input, route_line, status_facts, CampCommand,
    StatusFacts, CAMP_HINT, FORK_LINE,
};
use crate::screens::SettingsEvent;
use crate::screens::SettingsScreen;

use super::layout;

/// Smallest window the interface draws in, in columns.
pub const MIN_WIDTH: u16 = 80;
/// Smallest window the interface draws in, in rows.
pub const MIN_HEIGHT: u16 = 24;
/// Most lines the message log keeps; the oldest are dropped first.
pub const LOG_CAP: usize = 2000;
/// Status line drawn while the narrator is being asked for a line.
pub const BUSY_LINE: &str = "The narrator is writing...";
/// The three paces, in the order the pace picker lists them.
pub const PACES: [&str; 3] = ["slow", "steady", "hard"];

/// Which full screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Title,
    Camp,
    Event,
    Fork,
    GameOver,
    Settings,
    About,
}

/// A box drawn over the screen. While one is open it takes every key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    QuitConfirm,
    Pace,
}

/// One choice at an event or a fork, ready to draw. This is the one place a choice becomes a
/// row of text: the screens draw `text` and never build their own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRow {
    /// The id the engine expects (`A`..).
    pub id: String,
    pub label: String,
    /// The row as line mode prints it, for example `  A. Wait it out (A day)`.
    pub text: String,
}

/// What the screens need to know about the run, copied out of the engine after every turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewData {
    pub phase: GamePhase,
    pub status: StatusFacts,
    /// Title and text of the pending event; empty when no event is pending.
    pub event_title: String,
    pub event_text: String,
    /// The choices on offer at an event, or the routes at a fork.
    pub choices: Vec<ChoiceRow>,
    /// The Trail Ledger once the run is over; empty before.
    pub ledger: Vec<String>,
}

impl ViewData {
    /// Read the view data from the engine. This only reads: no step is taken and no random
    /// number is drawn.
    pub fn from_engine(engine: &StepEngine) -> Self {
        let (event_title, event_text) = match (engine.phase, engine.pending_event()) {
            (GamePhase::Event, Some(e)) => (e.title.clone(), e.fallback_narration.clone()),
            _ => (String::new(), String::new()),
        };
        let choices = match engine.phase {
            GamePhase::Event => engine
                .pending_event_choices()
                .iter()
                .map(event_choice_row)
                .collect(),
            GamePhase::Route => route_offered_letters(engine.pending_routes())
                .into_iter()
                .zip(engine.pending_routes())
                .map(|(letter, route)| ChoiceRow {
                    text: route_line(&letter, route),
                    label: route.name.clone(),
                    id: letter,
                })
                .collect(),
            _ => Vec::new(),
        };
        let ledger = if engine.phase == GamePhase::GameOver {
            trail_ledger(&engine.state)
        } else {
            Vec::new()
        };
        Self {
            phase: engine.phase,
            status: status_facts(&engine.state),
            event_title,
            event_text,
            choices,
            ledger,
        }
    }

    /// The ids on offer, as `parse_choice_input` wants them.
    pub fn offered(&self) -> Vec<String> {
        self.choices.iter().map(|c| c.id.clone()).collect()
    }
}

fn event_choice_row(choice: &EventChoiceInfo) -> ChoiceRow {
    ChoiceRow {
        id: choice.id.clone(),
        label: choice.label.clone(),
        text: choice_line(choice),
    }
}

/// The event (or fork) just decided, kept on screen with what happened until the next key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub title: String,
    pub text: String,
    /// Exactly the lines `play_turn` returned for the turn.
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingChoice {
    title: String,
    text: String,
}

/// A way of moving the scrolled text or a list cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// What a key press means, once the screen it was pressed on is taken into account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Enter or Space: go on.
    Confirm,
    /// Esc: close, cancel or leave.
    Back,
    /// A camp action (travel, rest, hunt, repair, a pace, a last resort).
    Act(PlayerIntent),
    /// An offered choice at an event or fork, by id.
    Choose(String),
    OpenPace,
    OpenHelp,
    OpenSettings,
    OpenAbout,
    /// `q`: ask whether to quit.
    AskQuit,
    /// The answer to the quit question.
    Answer(bool),
    /// Ctrl-C: quit now. The engine has already saved after every turn.
    ForceQuit,
    Scroll(Scroll),
    /// A settings-screen line: `1`, `2`, `3`, `4`, `s` or `q`.
    SettingsKey(&'static str),
    TextChar(char),
    TextBackspace,
    /// A key that means nothing here.
    Unrecognised,
}

/// Everything that can happen to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Intent(Intent),
    Resize(u16, u16),
    /// Reserved for a narrator that works off the UI thread; does nothing today.
    Tick,
    /// A turn was played: the lines it produced, the new view of the run and the narrator's
    /// one-line summary when a narrator is connected.
    TurnDone {
        lines: Vec<String>,
        view: Box<ViewData>,
        narrator_summary: Option<String>,
    },
    /// The settings file was written, or could not be.
    SettingsSaved(Result<(), String>),
}

/// Work for the driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Play one turn with this intent. `chosen_label` is the option picked, when there is one.
    Turn {
        intent: PlayerIntent,
        chosen_label: Option<String>,
    },
    SaveSettings(NarratorSettings),
    Quit,
}

/// Everything the screens draw from.
#[derive(Debug, Clone)]
pub struct Model {
    pub screen: Screen,
    /// Where Settings and About return to.
    pub back: Screen,
    pub overlay: Overlay,
    pub pace_cursor: usize,
    pub view: ViewData,
    pub log: Vec<String>,
    /// How many wrapped log lines the view is scrolled up from the newest.
    pub log_scroll: usize,
    /// How many wrapped ledger lines the view is scrolled down from the first.
    pub ledger_scroll: usize,
    pub resolved: Option<Resolved>,
    pending_choice: Option<PendingChoice>,
    /// The settings as last saved or loaded.
    pub saved_settings: NarratorSettings,
    pub settings: SettingsScreen,
    /// What the settings screen last said (a change made, a refusal).
    pub settings_note: String,
    /// The model name being typed, while the settings screen waits for one.
    pub settings_entry: Option<String>,
    pub version: String,
    pub content_hash: String,
    pub narrator_summary: Option<String>,
    /// `true` while the driver waits on the narrator.
    pub busy: bool,
    /// A short hint or result shown at the top right until the next key.
    pub notice: Option<String>,
    pub size: (u16, u16),
    /// Draw in colour. Off when `NO_COLOR` is set.
    pub color: bool,
}

impl Model {
    /// A model at the title screen. `log` is what the log starts with (the narrator's start
    /// lines, a settings warning).
    pub fn new(
        view: ViewData,
        settings: NarratorSettings,
        version: &str,
        content_hash: &str,
        log: Vec<String>,
        color: bool,
    ) -> Self {
        Self {
            screen: Screen::Title,
            back: Screen::Title,
            overlay: Overlay::None,
            pace_cursor: 1,
            view,
            log,
            log_scroll: 0,
            ledger_scroll: 0,
            resolved: None,
            pending_choice: None,
            saved_settings: settings.clone(),
            settings: SettingsScreen::new(settings),
            settings_note: String::new(),
            settings_entry: None,
            version: version.to_string(),
            content_hash: content_hash.to_string(),
            narrator_summary: None,
            busy: false,
            notice: None,
            size: (MIN_WIDTH, MIN_HEIGHT),
            color,
        }
    }

    /// The screen the run itself is on, by the engine's phase.
    fn play_screen(&self) -> Screen {
        match self.view.phase {
            GamePhase::Camp => Screen::Camp,
            GamePhase::Event => Screen::Event,
            GamePhase::Route => Screen::Fork,
            GamePhase::GameOver => Screen::GameOver,
        }
    }

    fn push_log(&mut self, lines: Vec<String>) {
        if !self.log.is_empty() {
            self.log.push(String::new());
        }
        self.log.extend(lines);
        if self.log.len() > LOG_CAP {
            let extra = self.log.len() - LOG_CAP;
            self.log.drain(..extra);
        }
    }
}

/// `true` when the environment asks for no colour: `NO_COLOR` set to a non-empty value.
pub fn no_color_requested(lookup: impl Fn(&str) -> Option<String>) -> bool {
    lookup("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// Turn a key press into a message, given what the model is showing. Returns `None` for keys
/// that mean nothing anywhere (a bare modifier, say).
pub fn key_to_msg(model: &Model, key: KeyEvent) -> Option<Msg> {
    Some(Msg::Intent(key_to_intent(model, key)?))
}

fn key_to_intent(model: &Model, key: KeyEvent) -> Option<Intent> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if ctrl {
        return match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => Some(Intent::ForceQuit),
            _ => None,
        };
    }
    if alt {
        return None;
    }
    // While a model name is being typed, every key is text, a space included.
    let typing_model_name = model.overlay == Overlay::None
        && model.screen == Screen::Settings
        && model.settings_entry.is_some();
    if typing_model_name {
        return Some(match key.code {
            KeyCode::Enter => Intent::Confirm,
            KeyCode::Esc => Intent::Back,
            KeyCode::Backspace => Intent::TextBackspace,
            KeyCode::Char(c) => Intent::TextChar(c),
            _ => Intent::Unrecognised,
        });
    }
    let scroll = match key.code {
        KeyCode::Up => Some(Scroll::Up),
        KeyCode::Down => Some(Scroll::Down),
        KeyCode::PageUp => Some(Scroll::PageUp),
        KeyCode::PageDown => Some(Scroll::PageDown),
        KeyCode::Home => Some(Scroll::Top),
        KeyCode::End => Some(Scroll::Bottom),
        _ => None,
    };
    let confirm = matches!(key.code, KeyCode::Enter) || key.code == KeyCode::Char(' ');
    let back = key.code == KeyCode::Esc;
    let ch = match key.code {
        KeyCode::Char(c) => Some(c),
        _ => None,
    };
    let lower = ch.map(|c| c.to_ascii_lowercase());

    match model.overlay {
        Overlay::Help => {
            return Some(if confirm || back || ch == Some('?') {
                Intent::Back
            } else {
                Intent::Unrecognised
            });
        }
        Overlay::QuitConfirm => {
            return Some(match lower {
                Some('y') => Intent::Answer(true),
                Some('n') => Intent::Answer(false),
                _ if back => Intent::Answer(false),
                _ => Intent::Unrecognised,
            });
        }
        Overlay::Pace => {
            if let Some(pace) = lower.and_then(pace_for_letter) {
                return Some(pace_intent(pace));
            }
            return Some(match scroll {
                Some(s) => Intent::Scroll(s),
                None if confirm => Intent::Confirm,
                None if back => Intent::Back,
                None => Intent::Unrecognised,
            });
        }
        Overlay::None => {}
    }

    if ch == Some('?') {
        return Some(Intent::OpenHelp);
    }
    if confirm {
        return Some(Intent::Confirm);
    }
    if back {
        return Some(Intent::Back);
    }

    match model.screen {
        Screen::Title => Some(match lower {
            Some('o') => Intent::OpenSettings,
            Some('a') => Intent::OpenAbout,
            Some('q') => Intent::AskQuit,
            _ => Intent::Unrecognised,
        }),
        Screen::About => Some(Intent::Unrecognised),
        Screen::Settings => Some(match lower {
            Some('1') => Intent::SettingsKey("1"),
            Some('2') => Intent::SettingsKey("2"),
            Some('3') => Intent::SettingsKey("3"),
            Some('4') => Intent::SettingsKey("4"),
            Some('s') => Intent::SettingsKey("s"),
            Some('q') => Intent::SettingsKey("q"),
            _ => Intent::Unrecognised,
        }),
        Screen::Camp => {
            if let Some(s) = scroll {
                return Some(Intent::Scroll(s));
            }
            let Some(c) = lower else {
                return Some(Intent::Unrecognised);
            };
            Some(match c {
                'p' => Intent::OpenPace,
                'o' => Intent::OpenSettings,
                _ => match parse_camp_input(&c.to_string()) {
                    Some(CampCommand::Act(intent)) => Intent::Act(intent),
                    Some(CampCommand::Quit) => Intent::AskQuit,
                    None => Intent::Unrecognised,
                },
            })
        }
        Screen::Event | Screen::Fork => {
            if lower == Some('q') {
                return Some(Intent::AskQuit);
            }
            if lower == Some('o') {
                return Some(Intent::OpenSettings);
            }
            let Some(c) = ch else {
                return Some(Intent::Unrecognised);
            };
            Some(
                match parse_choice_input(&c.to_string(), &model.view.offered()) {
                    Some(id) => Intent::Choose(id),
                    None => Intent::Unrecognised,
                },
            )
        }
        Screen::GameOver => {
            if let Some(s) = scroll {
                return Some(Intent::Scroll(s));
            }
            Some(match lower {
                Some('q') => Intent::AskQuit,
                _ => Intent::Unrecognised,
            })
        }
    }
}

fn pace_for_letter(c: char) -> Option<&'static str> {
    match c {
        's' => Some("slow"),
        't' => Some("steady"),
        'h' => Some("hard"),
        _ => None,
    }
}

fn pace_intent(pace: &str) -> Intent {
    Intent::Act(PlayerIntent::new(IntentAction::ChangePace).with_pace(pace))
}

/// Apply one message. Returns the work the driver must do.
pub fn update(model: &mut Model, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Resize(w, h) => {
            model.size = (w, h);
            clamp_scroll(model);
            Vec::new()
        }
        Msg::Tick => Vec::new(),
        Msg::TurnDone {
            lines,
            view,
            narrator_summary,
        } => {
            model.busy = false;
            model.push_log(lines.clone());
            model.resolved = model.pending_choice.take().map(|p| Resolved {
                title: p.title,
                text: p.text,
                lines,
            });
            model.view = *view;
            model.screen = model.play_screen();
            model.log_scroll = 0;
            model.ledger_scroll = 0;
            if narrator_summary.is_some() {
                model.narrator_summary = narrator_summary;
            }
            Vec::new()
        }
        Msg::SettingsSaved(result) => {
            match result {
                Ok(()) => {
                    model.saved_settings = model.settings.settings.clone();
                    model.notice = Some("Settings saved.".to_string());
                    model.screen = model.back;
                }
                Err(e) => {
                    model.settings_note = format!("Could not save the settings file: {e}");
                }
            }
            Vec::new()
        }
        Msg::Intent(intent) => update_intent(model, intent),
    }
}

fn update_intent(model: &mut Model, intent: Intent) -> Vec<Effect> {
    model.notice = None;
    if intent == Intent::ForceQuit {
        return vec![Effect::Quit];
    }

    match model.overlay {
        Overlay::Help => {
            if intent == Intent::Back {
                model.overlay = Overlay::None;
            }
            return Vec::new();
        }
        Overlay::QuitConfirm => {
            match intent {
                Intent::Answer(true) => return vec![Effect::Quit],
                Intent::Answer(false) => model.overlay = Overlay::None,
                _ => {}
            }
            return Vec::new();
        }
        Overlay::Pace => {
            match intent {
                Intent::Back => model.overlay = Overlay::None,
                Intent::Scroll(Scroll::Up) => {
                    model.pace_cursor = model.pace_cursor.saturating_sub(1);
                }
                Intent::Scroll(Scroll::Down) => {
                    model.pace_cursor = (model.pace_cursor + 1).min(PACES.len() - 1);
                }
                Intent::Confirm => {
                    model.overlay = Overlay::None;
                    return vec![Effect::Turn {
                        intent: PlayerIntent::new(IntentAction::ChangePace)
                            .with_pace(PACES[model.pace_cursor]),
                        chosen_label: None,
                    }];
                }
                Intent::Act(act) => {
                    model.overlay = Overlay::None;
                    return vec![Effect::Turn {
                        intent: act,
                        chosen_label: None,
                    }];
                }
                _ => {}
            }
            return Vec::new();
        }
        Overlay::None => {}
    }

    // Help and quit work on every screen.
    match intent {
        Intent::OpenHelp => {
            model.overlay = Overlay::Help;
            return Vec::new();
        }
        Intent::AskQuit => {
            model.overlay = Overlay::QuitConfirm;
            return Vec::new();
        }
        _ => {}
    }

    // The outcome of the last choice stays up until a key moves on. Enter and Esc only
    // dismiss it; a key that does something dismisses it and then does it; a key that means
    // nothing leaves it up.
    if model.resolved.is_some() && model.screen != Screen::Settings && model.screen != Screen::About
    {
        match intent {
            Intent::Confirm | Intent::Back => {
                model.resolved = None;
                return Vec::new();
            }
            Intent::Scroll(_) | Intent::Unrecognised => {}
            _ => model.resolved = None,
        }
    }

    match model.screen {
        Screen::Title => match intent {
            Intent::Confirm => model.screen = model.play_screen(),
            Intent::OpenSettings => open_settings(model),
            Intent::OpenAbout => {
                model.back = Screen::Title;
                model.screen = Screen::About;
            }
            _ => {}
        },
        Screen::About => {
            if matches!(intent, Intent::Confirm | Intent::Back) {
                model.screen = Screen::Title;
            }
        }
        Screen::Settings => return update_settings(model, intent),
        Screen::Camp => match intent {
            Intent::Act(act) => {
                return vec![Effect::Turn {
                    intent: act,
                    chosen_label: None,
                }]
            }
            Intent::OpenPace => {
                model.pace_cursor = PACES
                    .iter()
                    .position(|p| *p == model.view.status.pace)
                    .unwrap_or(1);
                model.overlay = Overlay::Pace;
            }
            Intent::OpenSettings => open_settings(model),
            Intent::Scroll(s) => scroll_log(model, s),
            Intent::Unrecognised => model.notice = Some(CAMP_HINT.to_string()),
            _ => {}
        },
        Screen::Event | Screen::Fork => match intent {
            Intent::Choose(id) => {
                let label = model
                    .view
                    .choices
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.label.clone());
                let (title, text) = if model.screen == Screen::Event {
                    (
                        model.view.event_title.clone(),
                        model.view.event_text.clone(),
                    )
                } else {
                    (FORK_LINE.to_string(), String::new())
                };
                model.pending_choice = Some(PendingChoice { title, text });
                let chosen_label = if model.screen == Screen::Event {
                    label
                } else {
                    None
                };
                return vec![Effect::Turn {
                    intent: PlayerIntent::new(IntentAction::Choose).with_choice(&id),
                    chosen_label,
                }];
            }
            Intent::OpenSettings => open_settings(model),
            Intent::Unrecognised => {
                model.notice = Some(format!(
                    "Choose one of: {}.",
                    model.view.offered().join(", ")
                ));
            }
            _ => {}
        },
        Screen::GameOver => {
            if let Intent::Scroll(s) = intent {
                scroll_ledger(model, s);
            }
        }
    }
    Vec::new()
}

fn open_settings(model: &mut Model) {
    model.back = model.screen;
    model.settings = SettingsScreen::new(model.saved_settings.clone());
    model.settings_note.clear();
    model.settings_entry = None;
    model.screen = Screen::Settings;
}

fn update_settings(model: &mut Model, intent: Intent) -> Vec<Effect> {
    if let Some(entry) = model.settings_entry.as_mut() {
        match intent {
            Intent::TextChar(c) => entry.push(c),
            Intent::TextBackspace => {
                entry.pop();
            }
            Intent::Confirm => {
                let line = model.settings_entry.take().unwrap_or_default();
                apply_settings_line(model, &line);
            }
            Intent::Back => {
                model.settings_entry = None;
                apply_settings_line(model, "");
            }
            _ => {}
        }
        return Vec::new();
    }
    match intent {
        Intent::SettingsKey(line) => return apply_settings_line(model, line),
        Intent::Back => return apply_settings_line(model, "q"),
        _ => {}
    }
    Vec::new()
}

fn apply_settings_line(model: &mut Model, line: &str) -> Vec<Effect> {
    match model.settings.handle_line(line) {
        SettingsEvent::Changed(msg) | SettingsEvent::Rejected(msg) => model.settings_note = msg,
        SettingsEvent::AskModel => {
            model.settings_note.clear();
            model.settings_entry = Some(String::new());
        }
        SettingsEvent::Save => {
            return vec![Effect::SaveSettings(model.settings.settings.clone())];
        }
        SettingsEvent::Discard => {
            model.notice = Some("Settings not saved.".to_string());
            model.screen = model.back;
        }
    }
    Vec::new()
}

fn scroll_log(model: &mut Model, by: Scroll) {
    let (total, visible) = layout::log_metrics(model);
    let max = total.saturating_sub(visible);
    model.log_scroll = moved(model.log_scroll, by, visible, max, true);
}

fn scroll_ledger(model: &mut Model, by: Scroll) {
    let (total, visible) = layout::ledger_metrics(model);
    let max = total.saturating_sub(visible);
    model.ledger_scroll = moved(model.ledger_scroll, by, visible, max, false);
}

/// The new scroll offset. `from_bottom` offsets count up from the newest line, so Up raises
/// them; otherwise they count down from the first line, so Up lowers them.
fn moved(current: usize, by: Scroll, page: usize, max: usize, from_bottom: bool) -> usize {
    let page = page.saturating_sub(1).max(1);
    let up = |n: usize| {
        if from_bottom {
            current.saturating_add(n)
        } else {
            current.saturating_sub(n)
        }
    };
    let down = |n: usize| {
        if from_bottom {
            current.saturating_sub(n)
        } else {
            current.saturating_add(n)
        }
    };
    let next = match by {
        Scroll::Up => up(1),
        Scroll::Down => down(1),
        Scroll::PageUp => up(page),
        Scroll::PageDown => down(page),
        Scroll::Top => {
            if from_bottom {
                max
            } else {
                0
            }
        }
        Scroll::Bottom => {
            if from_bottom {
                0
            } else {
                max
            }
        }
    };
    next.min(max)
}

fn clamp_scroll(model: &mut Model) {
    let (total, visible) = layout::log_metrics(model);
    model.log_scroll = model.log_scroll.min(total.saturating_sub(visible));
    let (total, visible) = layout::ledger_metrics(model);
    model.ledger_scroll = model.ledger_scroll.min(total.saturating_sub(visible));
}
