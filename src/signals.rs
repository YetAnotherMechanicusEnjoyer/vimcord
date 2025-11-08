use std::sync::Once;
use std::{io, process};

use crossterm::terminal::disable_raw_mode;
use crossterm::{execute, terminal::LeaveAlternateScreen};

static INIT: Once = Once::new();

pub fn restore_terminal() {
    INIT.call_once(|| {
        eprintln!("\nAttempting terminal cleanup...");

        match disable_raw_mode() {
            Ok(_) => (),
            Err(e) => eprintln!("Failed to disable raw mode: {e}"),
        }

        let mut stdout = io::stdout();
        match execute!(stdout, LeaveAlternateScreen) {
            Ok(_) => (),
            Err(e) => eprintln!("Failed to leave alternate screen: {e}"),
        }

        match execute!(stdout, crossterm::cursor::Show) {
            Ok(_) => (),
            Err(e) => eprintln!("Failed to show cursor: {e}"),
        }
    });
}

pub fn setup_ctrlc_handler() {
    if let Err(e) = ctrlc::set_handler(move || {
        restore_terminal();
        process::exit(1);
    }) {
        eprintln!("Error setting Ctrl-C handler: {}", e);
    }
}
