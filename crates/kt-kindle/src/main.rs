//! Primitive Terminal on the Kindle. It draws the screen with `kt-render`, shows the output of the Kindle's shell
//! (`kt-engine`), and takes touch and the page buttons (`input`). The screen is updated only where it changed.
//!
//! Environment (set by the library entry deploy/Terminal.sh, or by hand over SSH):
//!   KT_DIR   /mnt/us/terminal   the terminal's folder: fonts/, log-terminal.txt
//!
//! Leaving is Back (the page button) or the Back button. The library entry gives the status bar back when this exits.

mod eink;
mod input;
mod profile;

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use kt_core::rotation::{parse_accel, turn_from_accel, Debouncer};
use kt_core::{Action, Input, Rotation, Terminal};
use kt_engine::{Session, SessionConfig};
use kt_render::Renderer;

/// Events for the main loop: a finished touch or button press, what the shell printed, or a turn of the Kindle.
pub enum Ev {
    Input(Input),
    Shell(Vec<u8>),
    Turn(Rotation),
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

/// The accelerometer's `value` file (the input device named `bma2x2`, found by name: the numbers can change).
fn accel_value_path() -> Option<PathBuf> {
    for entry in std::fs::read_dir("/sys/class/input").ok()?.flatten() {
        let dir = entry.path();
        if std::fs::read_to_string(dir.join("name")).is_ok_and(|n| n.trim() == "bma2x2") && dir.join("value").exists() {
            return Some(dir.join("value"));
        }
    }
    None
}

/// The Kindle's turn, as the browser reads it: the framework's accelerometer property (`U` upright, `D` upside
/// down). Without the framework, the accelerometer's own file (the launcher's way). `None` when neither answers.
fn kindle_turn(sensor: Option<&PathBuf>) -> Option<Rotation> {
    if let Ok(out) = std::process::Command::new("lipc-get-prop").args(["com.lab126.winmgr", "accelerometer"]).output() {
        match String::from_utf8_lossy(&out.stdout).trim() {
            "U" => return Some(Rotation::Upright),
            "D" => return Some(Rotation::UpsideDown),
            _ => {}
        }
    }
    let text = std::fs::read_to_string(sensor?).ok()?;
    let (x, y, z) = parse_accel(&text)?;
    turn_from_accel(x, y, z)
}

/// Follow the Kindle's turn, 4 readings a second. A new turn is sent after it has been read for 800 ms in a row,
/// so a Kindle tilted back and forth does not flip the screen.
fn orientation_loop(tx: Sender<Ev>, start: Rotation) {
    let sensor = accel_value_path();
    log(&format!("turn: framework property, sensor {:?}", sensor.as_ref().map(|p| p.display().to_string())));
    let mut debounce = Debouncer::new(start, 800);
    let began = std::time::Instant::now();
    loop {
        thread::sleep(Duration::from_millis(250));
        let reading = kindle_turn(sensor.as_ref());
        let now = began.elapsed().as_millis() as u64;
        if let Some(turn) = debounce.feed(reading, now) {
            if tx.send(Ev::Turn(turn)).is_err() {
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

/// Send bytes to the shell; a failure is logged, not fatal.
fn send(session: &Arc<Mutex<Session>>, bytes: &[u8]) {
    if let Ok(mut s) = session.lock() {
        if let Err(e) = s.write(bytes) {
            log(&format!("cannot send to the shell: {e}"));
        }
    }
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

    input::TURN.store(term.rotation.turn(), Ordering::Relaxed);
    {
        let tx = tx.clone();
        let start = term.rotation;
        thread::spawn(move || orientation_loop(tx, start));
    }

    let mut previous = renderer.draw(&term);
    let _ = panel.draw(W, (0, 0, W, H), &previous, eink::WAVE_GC16, true);
    log("ready");

    while let Ok(ev) = rx.recv() {
        let turned_before = term.rotation;
        let action = match ev {
            Ev::Shell(bytes) => {
                term.feed(&bytes);
                Action::Redraw
            }
            Ev::Input(input) => term.handle(input),
            Ev::Turn(turn) => {
                term.rotation = turn;
                Action::Redraw
            }
        };
        let redraw = matches!(action, Action::Redraw | Action::Run(_) | Action::Send(_));
        match action {
            Action::Close => break,
            Action::Run(line) => send(&session, line.as_bytes()),
            Action::Send(bytes) => send(&session, &bytes),
            Action::Interrupt => {
                if let Ok(mut s) = session.lock() {
                    let _ = s.interrupt();
                }
            }
            Action::Redraw | Action::None => {}
        }
        let turned = term.rotation != turned_before;
        if turned {
            // The touch mapping follows the turn, and the whole picture is drawn again (the panel turns it).
            input::TURN.store(term.rotation.turn(), Ordering::Relaxed);
            log(&format!("turned: {:?}", term.rotation));
        }
        if turned || redraw {
            let next = renderer.draw(&term);
            if turned {
                let _ = panel.draw(W, (0, 0, W, H), &next, eink::WAVE_GC16, true);
            } else if let Some((x, y, w, h)) = changed_region(&previous, &next, W) {
                if let Err(e) = panel.draw(W, (x, y, w, h), &next, eink::WAVE_GL16, false) {
                    log(&format!("draw failed: {e}"));
                }
            }
            previous = next;
        }
    }
    log("leaving");
}
