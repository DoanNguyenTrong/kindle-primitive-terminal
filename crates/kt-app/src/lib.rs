//! Primitive Terminal as an app-kit app (`primitive-app`): the terminal's state (`kt-core`), its screen (`kt-render`)
//! and its shell (`kt-engine`) behind the kit's `App` trait. The Kindle driver (`kt-kindle`) and the desktop driver
//! (`kt-desktop`) both run this app, so the desktop tests run the same code as the device.

use std::io;
use std::time::{Duration, Instant};

use kt_core::{Action, Input as TerminalInput, Terminal};
use kt_engine::Session;
use kt_render::draw as draw_screen;
use primitive_app::{App, Change};
use primitive_ui::{Canvas, Input};

/// How long the loop waits for an event before it reads the shell's output again.
const POLL: Duration = Duration::from_millis(40);

pub struct TerminalApp {
    pub term: Terminal,
    session: Session,
}

impl TerminalApp {
    /// The terminal on `session`, `width` by `height` pixels. The shell's echo is turned off, because the terminal
    /// echoes the command itself; what the shell printed while it started is dropped.
    pub fn new(mut session: Session, width: i32, height: i32) -> io::Result<TerminalApp> {
        session.write(b"stty -echo\n")?;
        std::thread::sleep(Duration::from_millis(300));
        let _ = session.read_available();
        Ok(TerminalApp { term: Terminal::new(width, height), session })
    }

    /// Do what the terminal asks the driver to do after a key, a tap or a swipe.
    pub fn act(&mut self, action: Action) -> Change {
        match action {
            Action::None => Change::None,
            Action::Interrupt => {
                if let Err(e) = self.session.interrupt() {
                    eprintln!("[kt] cannot stop the command: {e}");
                }
                Change::None
            }
            Action::Redraw => Change::Partial,
            Action::Run(line) => {
                self.send(line.as_bytes());
                Change::Partial
            }
            Action::Send(bytes) => {
                self.send(&bytes);
                Change::Partial
            }
            Action::Close => Change::Exit,
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        if let Err(e) = self.session.write(bytes) {
            eprintln!("[kt] cannot send to the shell: {e}");
        }
    }

    /// Whether the shell has exited.
    pub fn shell_closed(&self) -> bool {
        self.session.is_closed()
    }
}

impl App for TerminalApp {
    fn draw(&self, canvas: &mut Canvas) {
        draw_screen(canvas, &self.term);
    }

    fn input(&mut self, input: Input) -> Change {
        let input = match input {
            Input::Tap(x, y) => TerminalInput::Tap(x, y),
            Input::Swipe { dy, start_y, .. } => TerminalInput::Swipe { dy, start_y },
            Input::PageBack => TerminalInput::Back,
            _ => return Change::None,
        };
        let action = self.term.handle(input);
        self.act(action)
    }

    fn deadline(&self) -> Option<Instant> {
        Some(Instant::now() + POLL)
    }

    /// The shell's output: what it printed since the last look is shown.
    fn tick(&mut self, _now: Instant) -> Change {
        match self.session.read_available() {
            Ok(bytes) if !bytes.is_empty() => {
                self.term.feed(&bytes);
                Change::Partial
            }
            Ok(_) => Change::None,
            Err(e) => {
                eprintln!("[kt] shell read failed: {e}");
                Change::None
            }
        }
    }
}
