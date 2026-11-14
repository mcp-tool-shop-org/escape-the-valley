//! The driver: carries out the [`Effect`]s `update` asks for and feeds the results back in.
//!
//! This is the only part of the interface that steps the engine, and it does so only through
//! [`play_turn`], the function line mode plays every turn with. It never draws from the
//! engine's RNG and never changes the run state itself.

use std::io;
use std::path::PathBuf;
use std::rc::Rc;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::engine::StepEngine;
use crate::model::PlayerIntent;
use crate::narrator::{FallbackPhraseTable, Narrator, NarratorSettings};
use crate::play::play_turn;
use crate::settings::save_settings;

use super::model::{key_to_msg, update, Effect, Model, Msg, ViewData};
use super::terminal::TerminalGuard;
use super::view::view;

/// The narrator a session plays with after the player saves new settings, as built by the
/// reconnect hook given to [`App::with_reconnect`].
pub struct Reconnected<'a> {
    /// The narrator the next turn plays with (a [`crate::narrator::NullNarrator`] when the
    /// settings switch it off or it could not be reached).
    pub narrator: Rc<dyn Narrator + 'a>,
    /// `true` when `narrator` is a connected model, which can be slow to answer.
    pub live: bool,
    /// What to tell the player about it, in plain words (added to the log).
    pub lines: Vec<String>,
    /// The narrator's one-line summary, for a connected narrator.
    pub summary: Option<Box<dyn Fn() -> String + 'a>>,
}

impl Reconnected<'static> {
    /// The reconnect result for `connection`, as `etv tui` builds it: the live narrator with
    /// its session summary when one connected, otherwise the plain-text narrator. The
    /// connection's lines are what the log shows.
    pub fn from_connection(connection: crate::play::NarratorConnection) -> Self {
        match connection.narrator {
            Some(live) => {
                let live = Rc::new(live);
                let for_summary = Rc::clone(&live);
                Reconnected {
                    narrator: live,
                    live: true,
                    lines: connection.lines,
                    summary: Some(Box::new(move || {
                        crate::play::narrator_summary_line(&for_summary.stats())
                    })),
                }
            }
            None => Reconnected {
                narrator: Rc::new(crate::narrator::NullNarrator),
                live: false,
                lines: connection.lines,
                summary: None,
            },
        }
    }
}

/// Builds the session's narrator from saved settings.
type ReconnectFn<'a> = Box<dyn FnMut(&NarratorSettings) -> Reconnected<'a> + 'a>;

/// A running session: the model, the engine it shows and what it needs to play turns.
pub struct App<'a> {
    pub model: Model,
    pub engine: StepEngine,
    narrator: &'a dyn Narrator,
    /// The narrator built from settings saved during the session; when set, it replaces
    /// `narrator`.
    replaced: Option<Rc<dyn Narrator + 'a>>,
    reconnect: Option<ReconnectFn<'a>>,
    table: Option<&'a FallbackPhraseTable>,
    base: PathBuf,
    narrator_live: bool,
    summary: Option<Box<dyn Fn() -> String + 'a>>,
    /// `true` once the player has quit.
    pub quit: bool,
}

impl<'a> App<'a> {
    /// `base` is the directory the settings file is written under. `narrator_live` is `true`
    /// when `narrator` is a connected model, which can be slow to answer.
    pub fn new(
        model: Model,
        engine: StepEngine,
        narrator: &'a dyn Narrator,
        table: Option<&'a FallbackPhraseTable>,
        base: PathBuf,
        narrator_live: bool,
    ) -> Self {
        Self {
            model,
            engine,
            narrator,
            replaced: None,
            reconnect: None,
            table,
            base,
            narrator_live,
            summary: None,
            quit: false,
        }
    }

    /// Show `summary()` on the game-over screen after every turn, for a connected narrator.
    pub fn with_summary(mut self, summary: impl Fn() -> String + 'a) -> Self {
        self.summary = Some(Box::new(summary));
        self
    }

    /// Rebuild the narrator with `reconnect` whenever the player saves settings that differ from
    /// the ones last saved, so the next turn plays with them without a restart.
    pub fn with_reconnect(
        mut self,
        reconnect: impl FnMut(&NarratorSettings) -> Reconnected<'a> + 'a,
    ) -> Self {
        self.reconnect = Some(Box::new(reconnect));
        self
    }

    /// The narrator's one-line summary, for a connected narrator (the one playing now).
    pub fn narrator_summary(&self) -> Option<String> {
        self.summary.as_ref().map(|f| f())
    }

    /// Apply saved settings to the narrator, when a reconnect hook was given.
    fn apply_settings(
        &mut self,
        settings: &NarratorSettings,
        before_wait: &mut dyn FnMut(&Model),
    ) -> Option<Msg> {
        let reconnect = self.reconnect.as_mut()?;
        if settings.enabled {
            // Connecting checks the service and may wait for the model to load.
            self.model.busy = true;
            before_wait(&self.model);
            self.model.busy = false;
        }
        let rebuilt = reconnect(settings);
        self.replaced = Some(rebuilt.narrator);
        self.narrator_live = rebuilt.live;
        self.summary = rebuilt.summary;
        Some(Msg::NarratorReconnected {
            lines: rebuilt.lines,
            narrator_summary: self.narrator_summary(),
        })
    }

    /// Feed one message in and carry out everything it leads to. `before_wait` is called with
    /// the model, marked busy, just before a turn that may wait on the narrator, so the caller
    /// can draw the status line first.
    pub fn dispatch(&mut self, msg: Msg, before_wait: &mut dyn FnMut(&Model)) {
        let mut queue = vec![msg];
        while let Some(msg) = queue.pop() {
            for effect in update(&mut self.model, msg) {
                match effect {
                    Effect::Turn {
                        intent,
                        chosen_label,
                    } => queue.push(self.play(&intent, chosen_label.as_deref(), before_wait)),
                    Effect::SaveSettings(settings) => {
                        let changed = settings != self.model.saved_settings;
                        let saved = save_settings(&settings, &self.base)
                            .map(|_| ())
                            .map_err(|e| e.to_string());
                        // Settings that were written take effect on the next turn. The queue
                        // is a stack, so the reconnect message is pushed first and handled
                        // after the saved one.
                        if saved.is_ok() && changed {
                            if let Some(msg) = self.apply_settings(&settings, before_wait) {
                                queue.push(msg);
                            }
                        }
                        queue.push(Msg::SettingsSaved(saved));
                    }
                    Effect::Quit => self.quit = true,
                }
            }
        }
    }

    fn play(
        &mut self,
        intent: &PlayerIntent,
        chosen_label: Option<&str>,
        before_wait: &mut dyn FnMut(&Model),
    ) -> Msg {
        if self.narrator_live {
            self.model.busy = true;
            before_wait(&self.model);
        }
        let narrator: &dyn Narrator = match &self.replaced {
            Some(n) => n.as_ref(),
            None => self.narrator,
        };
        let lines = play_turn(&mut self.engine, narrator, self.table, intent, chosen_label);
        Msg::TurnDone {
            lines,
            view: Box::new(ViewData::from_engine(&self.engine)),
            narrator_summary: self.summary.as_ref().map(|f| f()),
        }
    }

    /// Press one key, as the event loop does.
    pub fn press(&mut self, key: KeyEvent) {
        if let Some(msg) = key_to_msg(&self.model, key) {
            self.dispatch(msg, &mut |_| {});
        }
    }

    /// Press a plain key.
    pub fn press_code(&mut self, code: KeyCode) {
        self.press(KeyEvent::new(code, KeyModifiers::NONE));
    }

    /// Press a letter or digit key.
    pub fn press_char(&mut self, c: char) {
        self.press_code(KeyCode::Char(c));
    }
}

/// Run the interface on the real terminal until the player quits. The terminal is given back
/// on every way out, a panic included.
pub fn run_terminal(app: &mut App<'_>) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let size = terminal.size()?;
    app.dispatch(Msg::Resize(size.width, size.height), &mut |_| {});
    while !app.quit {
        terminal.draw(|frame| view(&app.model, frame))?;
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if let Some(msg) = key_to_msg(&app.model, key) {
                    app.dispatch(msg, &mut |model| {
                        let _ = terminal.draw(|frame| view(model, frame));
                    });
                }
            }
            Event::Resize(w, h) => app.dispatch(Msg::Resize(w, h), &mut |_| {}),
            _ => {}
        }
    }
    Ok(())
}
