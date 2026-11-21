//! Desktop driver for the terminal: runs the terminal app (`kt-app`) on the Mac's shell through the app kit's lifecycle,
//! so a tap, a swipe or a key does what it does on the Kindle, and writes PNG screenshots. For testing without the
//! Kindle.
//!
//! Script lines (one per line on stdin):
//!   type TEXT        type TEXT as keys (run the Go key with `go`)
//!   go               run the typed line
//!   wait MS          show the shell's output for MS milliseconds
//!   stop             Stop on the key bar (Ctrl-C)
//!   ctrl             the Ctrl key on the key bar (the next letter is a control character)
//!   tap X Y          a tap in device pixels (the kit's lifecycle: the press, then the action)
//!   pressed X Y NAME a screenshot of the screen as it is drawn while the tap at X Y is pressed, before its action
//!   swipe DY         a swipe (positive down), from the middle of the output
//!   shot NAME        write NAME.png into the output folder
//!   quit             end

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kt_app::TerminalApp;
use kt_core::BarKey;
use kt_engine::{Session, SessionConfig};
use primitive_app::{font_paths, frame, write_png, App, Change, Lifecycle, HEIGHT, WIDTH};
use primitive_ui::{Canvas, Input, Key};

fn key_for(c: char) -> Key {
    if c == ' ' { Key::Space } else { Key::Char(c) }
}

/// Read the shell's output for `ms`, showing it on the screen (the app's own tick reads the shell).
fn pump(app: &mut TerminalApp, ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        app.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().skip_while(|a| a != "--out").nth(1).unwrap_or_else(|| "out".into()));
    std::fs::create_dir_all(&out)?;
    let fonts = font_paths(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts")));
    let mut canvas = Canvas::new(WIDTH, HEIGHT, &fonts)?;
    let config = SessionConfig { shell: "/bin/sh".into(), ..SessionConfig::default() };
    let session = Session::spawn(&config)?;
    let mut app = TerminalApp::new(session, WIDTH as i32, HEIGHT as i32)?;
    let mut lifecycle = Lifecycle::default();
    let on_error = |error: io::Error| eprintln!("[save] {error}");
    pump(&mut app, 100);
    app.term.lines.clear();
    app.term.partial.clear();

    for line in io::stdin().lock().lines() {
        let line = line?;
        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("");
        let mut exit = false;
        match cmd {
            "" => {}
            "type" => {
                for c in arg.chars() {
                    let action = app.term.press(key_for(c));
                    app.act(action);
                }
            }
            "go" => {
                let action = app.term.press(Key::Go);
                app.act(action);
            }
            "wait" => pump(&mut app, arg.parse().unwrap_or(300)),
            "stop" => {
                let action = app.term.bar_key(BarKey::Stop);
                app.act(action);
            }
            "ctrl" => {
                app.term.bar_key(BarKey::Ctrl);
            }
            "tap" => {
                let (x, y) = coords(arg);
                exit = lifecycle.input(&mut app, Input::Tap(x, y), on_error) == Change::Exit;
            }
            "pressed" => {
                let mut v = arg.split_whitespace();
                let x: f32 = v.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let y: f32 = v.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let name = v.next().unwrap_or("pressed");
                match lifecycle.press_feedback(&app, &mut canvas, Input::Tap(x, y), false) {
                    Some(pressed) => {
                        write_png(&out.join(format!("{name}.png")), WIDTH, HEIGHT, &pressed)?;
                        eprintln!("[shot] {name}");
                    }
                    None => eprintln!("[shot] nothing is pressed at {x} {y}"),
                }
            }
            "swipe" => {
                let dy: f32 = arg.parse().unwrap_or(0.0);
                let swipe = Input::Swipe { dx: 0.0, dy, start_y: 400.0 };
                exit = lifecycle.swipe(&mut app, swipe, 600.0, on_error) == Change::Exit;
            }
            "shot" => {
                let gray = frame(&app, &mut canvas, false);
                write_png(&out.join(format!("{arg}.png")), WIDTH, HEIGHT, &gray)?;
                eprintln!("[shot] {arg}");
            }
            "quit" => break,
            other => eprintln!("[script] unknown line: {other}"),
        }
        io::stdout().flush()?;
        if exit {
            eprintln!("[kt] the terminal closed");
            break;
        }
    }
    Ok(())
}

fn coords(arg: &str) -> (f32, f32) {
    let mut v = arg.split_whitespace().map(|s| s.parse::<f32>().unwrap_or(0.0));
    (v.next().unwrap_or(0.0), v.next().unwrap_or(0.0))
}
