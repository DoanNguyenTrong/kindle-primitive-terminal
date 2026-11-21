//! Primitive Terminal on the Kindle: the terminal app (`kt-app`) on the app kit (`primitive-app`). The kit draws the
//! e-ink screen, takes touch and the page buttons, turns the picture with the Kindle, follows the system layer under
//! Primitive Launcher (sleep, quick settings, the theme), and shows taps on the controls.
//!
//! Environment (set by the library entry deploy/Terminal.sh, or by hand over SSH):
//!   KT_DIR   /mnt/us/terminal   the terminal's folder (the kit's app folder: settings.txt, log-app.txt)
//!   KL_DIR   the launcher's folder (its settings.txt and fonts); the library entry points it at the terminal's
//!            folder when the terminal is opened without the launcher, because that folder has the fonts.
//!   PRIMITIVE_LAUNCHER=1   opened from Primitive Launcher (read by the kit)

use std::path::PathBuf;

use kt_app::TerminalApp;
use kt_engine::{Session, SessionConfig};
use primitive_app::{HEIGHT, WIDTH};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::var("KT_DIR").unwrap_or_else(|_| "/mnt/us/terminal".into()));
    let config = SessionConfig { shell: "/bin/sh".into(), ..SessionConfig::default() };
    let session = Session::spawn(&config).map_err(|e| format!("the shell did not start: {e}"))?;
    let app = TerminalApp::new(session, WIDTH as i32, HEIGHT as i32).map_err(|e| format!("the shell did not answer: {e}"))?;
    primitive_app::run(app, &dir)
}
