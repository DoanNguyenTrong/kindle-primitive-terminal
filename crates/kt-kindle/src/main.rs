//! Primitive Terminal on the Kindle. It draws the screen with `kt-render`, shows the output of the Kindle's shell
//! (`kt-engine`), and takes touch and the page buttons (`input`). The screen is updated only where it changed.
//!
//! Environment (set by the library entry deploy/Terminal.sh, or by hand over SSH):
//!   KT_DIR   /mnt/us/terminal   the terminal's folder: fonts/, log-terminal.txt
//!   PRIMITIVE_LAUNCHER=1         opened from Primitive Launcher, the system layer: it owns sleep, quick settings
//!                                and the theme. The terminal follows the theme, redraws on SIGUSR1, and leaves the
//!                                swipe down from the top edge to the launcher.
//!   KL_DIR   /mnt/us/launcher    the launcher's folder (its settings.txt has the theme)
//!
//! Leaving is Back (the page button) or the Back button. The library entry gives the status bar back when this exits.

mod eink;
mod input;
mod profile;

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Under Primitive Launcher: wakes the loop for the launcher's redraw request (`REDRAW`).
    Redraw,
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

/// Opened from Primitive Launcher (`PRIMITIVE_LAUNCHER=1`).
fn under_launcher() -> bool {
    std::env::var("PRIMITIVE_LAUNCHER").as_deref() == Ok("1")
}

/// Whether the launcher's settings turn dark mode on (`dark_mode=on`).
fn launcher_dark_mode(settings: &str) -> bool {
    settings.lines().any(|line| line.split_once('=').is_some_and(|(k, v)| k.trim() == "dark_mode" && v.trim() == "on"))
}

/// The system theme: dark when the launcher's dark mode is on. Opened without the launcher, the terminal is light,
/// as before.
fn system_dark(launcher: bool) -> bool {
    if !launcher {
        return false;
    }
    let dir = std::env::var("KL_DIR").unwrap_or_else(|_| "/mnt/us/launcher".into());
    launcher_dark_mode(&std::fs::read_to_string(format!("{dir}/settings.txt")).unwrap_or_default())
}

/// Set by SIGUSR1: the launcher asks for the whole screen again (after a wake, or a theme change).
static REDRAW: AtomicBool = AtomicBool::new(false);

extern "C" fn on_redraw(_: libc::c_int) {
    REDRAW.store(true, Ordering::Relaxed);
}

/// Under the launcher, take SIGUSR1 as a redraw request. Set before the shell starts: the library entry ignores the
/// signal, and the shell would keep ignoring it otherwise (a handler, unlike an ignored signal, is not passed on).
fn take_redraw_signal() {
    // SAFETY: the handler only stores to an atomic, which is safe in a signal handler.
    unsafe { libc::signal(libc::SIGUSR1, on_redraw as *const () as libc::sighandler_t) };
}

/// A thread checks for the redraw request and wakes the main loop. The loop takes the request itself, before the
/// event that woke it, so nothing queued while the terminal was frozen is drawn in the old theme first.
fn wake_on_redraw(tx: Sender<Ev>) {
    thread::spawn(move || {
        let mut woken = false;
        loop {
            thread::sleep(Duration::from_millis(150));
            let pending = REDRAW.load(Ordering::Relaxed);
            if pending && !woken && tx.send(Ev::Redraw).is_err() {
                return;
            }
            woken = pending;
        }
    });
}

/// Dark mode is the picture inverted: black text on white becomes white text on black.
fn invert(gray: &mut [u8]) {
    for px in gray.iter_mut() {
        *px = 255 - *px;
    }
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

/// Whether the picture is drawn turned: the Kindle's turn, combined with the framebuffer's own rotation, as the
/// launcher does. Upright on both, or turned on both, shows upright.
fn picture_flip(t: &Terminal, panel: &eink::Panel) -> bool {
    (t.rotation.turn() + panel.turn()) % 4 == 2
}

/// The turn the picture is shown in: the framebuffer's rotation, plus the flip the picture was drawn with. Touch
/// must follow it, because the touch panel is not turned by the framework (the picture is).
fn displayed_turn(t: &Terminal, panel: &eink::Panel) -> u32 {
    let flip = if picture_flip(t, panel) { 2 } else { 0 };
    (panel.turn() + flip) % 4
}

/// The frame for the panel: the terminal's picture, inverted in dark mode, and turned 180 degrees when the picture
/// is flipped (the buffer is row-major, so turning it is reversing it).
fn frame(renderer: &mut Renderer, t: &Terminal, panel: &eink::Panel, dark: bool) -> Vec<u8> {
    let mut out = renderer.draw(t);
    if dark {
        invert(&mut out);
    }
    if picture_flip(t, panel) {
        out.reverse();
    }
    out
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
    let launcher = under_launcher();
    if launcher {
        take_redraw_signal();
    }
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
    let mut dark = system_dark(launcher);
    if launcher {
        wake_on_redraw(tx.clone());
        log(&format!("under Primitive Launcher: it owns sleep, quick settings and the theme (dark {dark})"));
    }
    match input::find(profile::ACTIVE.touch_name) {
        Some(path) => {
            let t = tx.clone();
            thread::spawn(move || input::touch_loop(path, t, launcher));
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

    // The screen starts in the Kindle's turn (the framework's property), not in upright.
    if let Some(turn) = kindle_turn(accel_value_path().as_ref()) {
        term.rotation = turn;
    }
    input::TURN.store(displayed_turn(&term, &panel), Ordering::Relaxed);
    {
        let tx = tx.clone();
        let start = term.rotation;
        thread::spawn(move || orientation_loop(tx, start));
    }

    let mut previous = frame(&mut renderer, &term, &panel, dark);
    let _ = panel.draw(W, (0, 0, W, H), &previous, eink::WAVE_GC16, true);
    log(&format!("ready: framebuffer turn {}, picture flipped {}, touch turn {}", panel.turn(), picture_flip(&term, &panel), displayed_turn(&term, &panel)));

    while let Ok(ev) = rx.recv() {
        // The launcher's redraw request (SIGUSR1, sent while the terminal was frozen) comes first: read the theme
        // again and draw the whole screen, before the event that woke the loop. No flash: after a theme switch the
        // launcher has just flashed the screen, and a second flash would show the old theme for a moment.
        if launcher && REDRAW.swap(false, Ordering::Relaxed) {
            dark = system_dark(true);
            let next = frame(&mut renderer, &term, &panel, dark);
            match panel.draw(W, (0, 0, W, H), &next, eink::WAVE_GC16, false) {
                Ok(true) => log(&format!("redraw for the launcher (dark {dark})")),
                Ok(false) => log("redraw: the framebuffer is not in a drawable state"),
                Err(e) => log(&format!("redraw: {e}")),
            }
            previous = next;
        }
        let turned_before = term.rotation;
        let action = match ev {
            Ev::Shell(bytes) => {
                term.feed(&bytes);
                Action::Redraw
            }
            Ev::Input(input) => {
                // A key is drawn as pressed before its action runs: the tap is seen at once.
                if term.press_feedback(input) {
                    let next = frame(&mut renderer, &term, &panel, dark);
                    if let Some((x, y, w, h)) = changed_region(&previous, &next, W) {
                        let _ = panel.draw(W, (x, y, w, h), &next, eink::WAVE_DU, false);
                    }
                    previous = next;
                }
                term.handle(input)
            }
            Ev::Turn(turn) => {
                term.rotation = turn;
                Action::Redraw
            }
            // Only a wake-up: the request was taken above.
            Ev::Redraw => Action::None,
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
        let flip = picture_flip(&term, &panel);
        if turned {
            // The touch mapping follows the turn, and the whole picture is drawn again.
            input::TURN.store(displayed_turn(&term, &panel), Ordering::Relaxed);
            log(&format!("turned: {:?}, framebuffer turn {}, picture flipped {flip}, touch turn {}", term.rotation, panel.turn(), displayed_turn(&term, &panel)));
        }
        if turned || redraw {
            let next = frame(&mut renderer, &term, &panel, dark);
            if turned {
                match panel.draw(W, (0, 0, W, H), &next, eink::WAVE_GC16, true) {
                    Ok(true) => log("turned picture drawn"),
                    Ok(false) => log("turned picture: the framebuffer is not in a drawable state"),
                    Err(e) => log(&format!("turned picture: {e}")),
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_is_the_launchers_dark_mode() {
        assert!(launcher_dark_mode("rotation=upright\ndark_mode=on\n"));
        assert!(launcher_dark_mode(" dark_mode = on "));
        assert!(!launcher_dark_mode("dark_mode=off"));
        assert!(!launcher_dark_mode(""), "no setting is light");
        assert!(!system_dark(false), "without the launcher the terminal is light");
    }

    #[test]
    fn dark_mode_inverts_the_picture() {
        let mut gray = vec![0, 128, 255];
        invert(&mut gray);
        assert_eq!(gray, [255, 127, 0]);
    }
}
