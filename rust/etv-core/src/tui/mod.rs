//! The full-screen terminal interface (`etv tui`), the second front end beside line mode.
//!
//! It is built the Elm way. [`model::Model`] holds everything a screen shows; a key press becomes
//! a [`model::Msg`]; the pure [`model::update`] changes the model and answers with
//! [`model::Effect`]s; the pure [`view::view`] draws the model. The only part that touches the
//! engine or the terminal is [`app`], and it steps the engine through
//! [`crate::play::play_turn`] alone, so this front end and line mode cannot drift apart in rules
//! or wording.

pub mod app;
pub mod layout;
pub mod model;
pub mod terminal;
pub mod view;

pub use app::{run_terminal, App};
pub use model::{
    key_to_msg, no_color_requested, update, ChoiceRow, Effect, Intent, Model, Msg, Overlay,
    Resolved, Screen, ViewData, BUSY_LINE, MIN_HEIGHT, MIN_WIDTH,
};
pub use terminal::{install_panic_hook, restore_terminal, TerminalGuard};
pub use view::{view, HELP_LINES, TOO_SMALL_LINE};
