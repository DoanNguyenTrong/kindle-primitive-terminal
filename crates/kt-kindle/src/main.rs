//! Primitive Terminal on the Kindle. It draws the screen with `kt-render`, shows the output of the Kindle's shell
//! (`kt-engine`), and takes touch and the page buttons (`input`). The screen is updated only where it changed.
//!
//! Environment (set by the launcher's library entry, or by hand over SSH):
//!   KT_DIR   /mnt/us/terminal   the terminal's folder: fonts/, log-terminal.txt
//!
//! Leaving is Back (the page button) or the Back button. The launcher takes the screen back when this exits.

mod eink;
mod input;
mod profile;

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use kt_core::{Action, Input, Terminal};
use kt_engine::{Session, SessionConfig};
use kt_render::Renderer;

/// Events for the main loop: a finished touch or button press, or what the shell printed.
pub enum Ev {
    Input(Input),
    Shell(Vec<u8>),
}

const W: u32 = profile::ACTIVE.screen_w;
const H: u32 = profile::ACTIVE.screen_h;

/// Log a line to stderr and to the terminal's log file (kept small: the file is cut when it passes 256 KB).
pub fn log(msg: &str) {
    eprintln!("[kt] {msg}");
    let path = dir().join("log-terminal.txt");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{msg}");
    }
}

fn dir() -> PathBuf {
    PathBuf::from(std::env::var("KT_DIR").unwrap_or_else(|_| "/mnt/us/terminal".into()))
}

fn font_paths() -> Vec<String> {
    let fonts = dir().join("fonts");
    ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"].iter().map(|f| fonts.join(f).display().to_string()).collect()
}

/// Read the shell's output on a thread and pass it to the main loop, until the shell closes.
fn shell_reader(session: Arc<Mutex<Session>>, tx: Sender<Ev>) {
    loop {
        let bytes = {
            let Ok(mut s) = session.lock() else { return };
            s.read_available()
        };
        match bytes {
            Ok(b) if !b.is_empty() => {
                if tx.send(Ev::Shell(b)).is_err() {
                    return;
                }
            }
            Ok(_) => thread::sleep(Duration::from_millis(30)),
            Err(e) => {
                log(&format!("shell read failed: {e}"));
                return;
            }
        }
    }
}

/// The smallest rectangle (x, y, w, h) where two frames differ, or None when they are the same.
fn changed_region(old: &[u8], new: &[u8], width: u32) -> Option<(u32, u32, u32, u32)> {
    let height = new.len() as u32 / width;
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..height {
        let row = (y * width) as usize;
        let a = &old[row..row + width as usize];
        let b = &new[row..row + width as usize];
        if a == b {
            continue;
        }
        let first = a.iter().zip(b).position(|(p, q)| p != q).unwrap_or(0) as u32;
        let last = a.iter().zip(b).rposition(|(p, q)| p != q).unwrap_or(0) as u32;
        x0 = x0.min(first);
        x1 = x1.max(last);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    (x0 <= x1 && y0 <= y1).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

fn main() {
    log("starting");
    let mut renderer = match Renderer::new(W, H, &font_paths()) {
        Ok(r) => r,
        Err(e) => {
            log(&format!("cannot draw text: {e}"));
            std::process::exit(1);
        }
    };
    let mut panel = match eink::Panel::open() {
        Ok(p) => p,
        Err(e) => {
            log(&format!("no e-ink panel: {e}"));
            std::process::exit(1);
        }
    };
    let mut term = Terminal::new(W as i32, H as i32);

    let config = SessionConfig { shell: "/bin/sh".into(), ..SessionConfig::default() };
    let session = match Session::spawn(&config) {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            log(&format!("the shell did not start: {e}"));
            std::process::exit(1);
        }
    };
    // The terminal shows what the shell prints; the terminal echoes the command itself, so the shell's echo is off.
    if let Ok(mut s) = session.lock() {
        let _ = s.write(b"stty -echo\n");
    }
    thread::sleep(Duration::from_millis(300));
    if let Ok(mut s) = session.lock() {
        let _ = s.read_available();
    }

    let (tx, rx) = channel::<Ev>();
    {
        let session = Arc::clone(&session);
        let tx = tx.clone();
        thread::spawn(move || shell_reader(session, tx));
    }
    match input::find(profile::ACTIVE.touch_name) {
        Some(path) => {
            let t = tx.clone();
            thread::spawn(move || input::touch_loop(path, t));
        }
        None => log("no touch device found"),
    }
    match input::find("gpio-keys") {
        Some(path) => {
            let t = tx.clone();
            thread::spawn(move || input::button_loop(path, t));
        }
        None => log("no page buttons found"),
    }

    let mut previous = renderer.draw(&term);
    let full = vec![0u8; previous.len()];
    let _ = panel.draw(W, (0, 0, W, H), &full, eink::WAVE_GC16, true);
    let _ = panel.draw(W, (0, 0, W, H), &previous, eink::WAVE_GC16, true);
    log("ready");

    while let Ok(ev) = rx.recv() {
        let action = match ev {
            Ev::Shell(bytes) => {
                term.feed(&bytes);
                Action::Redraw
            }
            Ev::Input(input) => term.handle(input),
        };
        let redraw = matches!(action, Action::Redraw | Action::Run(_));
        match action {
            Action::Close => break,
            Action::Run(line) => {
                if let Ok(mut s) = session.lock() {
                    if let Err(e) = s.write(line.as_bytes()) {
                        log(&format!("cannot send the command: {e}"));
                    }
                }
            }
            Action::Interrupt => {
                if let Ok(mut s) = session.lock() {
                    let _ = s.interrupt();
                }
            }
            Action::Redraw | Action::None => {}
        }
        // Redraw when the screen changed (a shell line, a key, a scroll), only where it changed.
        if redraw {
            let next = renderer.draw(&term);
            if let Some((x, y, w, h)) = changed_region(&previous, &next, W) {
                let region = (x, y, w, h);
                let wave = eink::WAVE_GL16;
                if let Err(e) = panel.draw(W, region, &next, wave, false) {
                    log(&format!("draw failed: {e}"));
                }
            }
            previous = next;
        }
    }
    log("leaving");
}
