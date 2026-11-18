//! Desktop driver for the terminal: runs the Mac's shell through `kt-engine`, types commands from a script on stdin,
//! and writes PNG screenshots of the screen. For testing the screen and the shell without the Kindle.
//!
//! Script lines (one per line on stdin):
//!   type TEXT   type TEXT as keys (run the Go key with `go`)
//!   go          run the typed line
//!   wait MS     show the shell's output for MS milliseconds
//!   stop        Stop on the key bar (Ctrl-C)
//!   turn        the Turn button: the screen upside down, or back
//!   ctrl        the Ctrl key on the key bar (the next letter is a control character)
//!   tap X Y     a tap in device pixels
//!   swipe DY    a swipe (positive down), from the middle of the output
//!   shot NAME   write NAME.png into the output folder
//!   quit        end

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kt_core::{Action, Input, Terminal};
use kt_engine::{Session, SessionConfig};
use kt_render::Renderer;
use primitive_keyboard::Key;

const W: u32 = 1264;
const H: u32 = 1680;
/// The fonts: the same Noto Sans the Kindle apps use (`assets/fonts`).
fn font_paths() -> Vec<String> {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts");
    ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"].iter().map(|f| format!("{base}/{f}")).collect()
}

fn write_png(path: &PathBuf, gray: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(io::BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(gray)).expect("write png");
    eprintln!("[shot] {}", path.display());
}

/// Read the shell's output for `ms`, feeding it to the screen.
fn pump(session: &mut Session, term: &mut Terminal, ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    loop {
        match session.read_available() {
            Ok(bytes) if !bytes.is_empty() => term.feed(&bytes),
            Ok(_) => {}
            Err(e) => {
                eprintln!("[shell] read failed: {e}");
                break;
            }
        }
        if Instant::now() >= until {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn key_for(c: char) -> Key {
    if c == ' ' { Key::Space } else { Key::Char(c) }
}

fn main() -> io::Result<()> {
    let out = PathBuf::from(std::env::args().skip_while(|a| a != "--out").nth(1).unwrap_or_else(|| "out".into()));
    std::fs::create_dir_all(&out)?;
    let mut renderer = Renderer::new(W, H, &font_paths()).expect("fonts");
    let mut term = Terminal::new(W as i32, H as i32);
    let config = SessionConfig { shell: "/bin/sh".into(), ..SessionConfig::default() };
    let mut session = Session::spawn(&config)?;
    // The terminal shows what the shell prints, so the shell's own echo is turned off (the terminal echoes the
    // command itself).
    session.write(b"stty -echo\n")?;
    pump(&mut session, &mut term, 300);
    term.lines.clear();
    term.partial.clear();

    for line in io::stdin().lock().lines() {
        let line = line?;
        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("");
        match cmd {
            "" => {}
            "type" => {
                for c in arg.chars() {
                    term.press(key_for(c));
                }
            }
            "go" => {
                if let Action::Run(text) = term.press(Key::Go) {
                    session.write(text.as_bytes())?;
                }
            }
            "wait" => pump(&mut session, &mut term, arg.parse().unwrap_or(300)),
            "stop" => {
                if term.bar_key(kt_core::BarKey::Stop) == Action::Interrupt {
                    session.interrupt()?;
                }
            }
            "turn" => {
                term.handle(Input::Tap(term.turn_button().x as f32 + 5.0, term.turn_button().y as f32 + 5.0));
            }
            "ctrl" => {
                term.bar_key(kt_core::BarKey::Ctrl);
            }
            "tap" => {
                let mut v = arg.split_whitespace().map(|s| s.parse::<f32>().unwrap_or(0.0));
                let (x, y) = (v.next().unwrap_or(0.0), v.next().unwrap_or(0.0));
                match term.handle(Input::Tap(x, y)) {
                    Action::Run(text) => session.write(text.as_bytes())?,
                    Action::Send(bytes) => session.write(&bytes)?,
                    Action::Interrupt => session.interrupt()?,
                    _ => {}
                }
            }
            "swipe" => {
                let dy: f32 = arg.parse().unwrap_or(0.0);
                term.handle(Input::Swipe { dy, start_y: 400.0 });
            }
            "shot" => {
                let gray = renderer.draw(&term);
                write_png(&out.join(format!("{arg}.png")), &gray);
            }
            "quit" => break,
            other => eprintln!("[script] unknown line: {other}"),
        }
        io::stdout().flush()?;
    }
    Ok(())
}
