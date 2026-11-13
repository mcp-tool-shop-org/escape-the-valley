//! Taking the terminal over and giving it back.
//!
//! The raw mode and the alternate screen are entered in one place, [`TerminalGuard::enter`], and
//! given back in one place, the guard's `Drop`. A panic hook gives the terminal back too, before
//! the default hook prints, so a crash leaves a usable shell and a readable message. What
//! "give back" does is injectable, so the tests can prove it runs without a terminal.

use std::io;
use std::panic;

use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

/// Leave the alternate screen, show the cursor and leave raw mode. Safe to call more than once,
/// and when the terminal was never taken over.
pub fn restore_terminal() {
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    let _ = disable_raw_mode();
}

/// Run `restore` on any panic, before the default hook prints its message.
pub fn install_panic_hook(restore: impl Fn() + Send + Sync + 'static) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
}

/// Holds the terminal while it is alive. Dropping it gives the terminal back.
pub struct TerminalGuard {
    restore: Option<Box<dyn Fn() + Send + Sync>>,
}

impl TerminalGuard {
    /// Enter raw mode and the alternate screen, and install the panic hook. If entering fails
    /// half way, the terminal is given back before the error is returned.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, Hide) {
            restore_terminal();
            return Err(e);
        }
        install_panic_hook(restore_terminal);
        Ok(Self::with_restore(restore_terminal))
    }

    /// A guard whose `Drop` runs `restore` and touches no terminal itself.
    pub fn with_restore(restore: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            restore: Some(Box::new(restore)),
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Some(restore) = self.restore.take() {
            restore();
        }
    }
}
