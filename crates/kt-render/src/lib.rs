//! Draws the terminal's screen with the app kit's canvas (`primitive-ui`): the header with the Exit button, the output,
//! the command line, the key bar and the keyboard. The kit does the rest: it turns the picture with the Kindle, inverts
//! it in dark mode, and reverses a control while it is tapped.

use kt_core::{BarKey, Rect, Terminal, GO_LABEL, PROMPT};
use primitive_keyboard::{style, Key, KeyDef};
use primitive_ui::{header, Canvas, TextStyle, BLACK};

const MARGIN_X: i32 = 40;
/// The output's font size and the height of one of its lines.
const OUTPUT_SIZE: i32 = 30;
const LINE_H: i32 = 38;
/// The command line's font size.
const INPUT_SIZE: i32 = 34;

/// Draw the whole terminal.
pub fn draw(canvas: &mut Canvas, t: &Terminal) {
    header(canvas, "Terminal", "Exit");
    let bar = t.title_bar();
    canvas.fill(Rect::new(0, bar.bottom() - 4, bar.w, 4), BLACK);
    output(canvas, t);
    input(canvas, t);
    key_bar(canvas, t);
    canvas.keyboard(t.layer(), GO_LABEL, None);
}

/// The output: the newest rows that fit above the command line. The scroll position is applied by the terminal.
fn output(canvas: &mut Canvas, t: &Terminal) {
    let area = t.output_area();
    let max_rows = (area.h / LINE_H).max(1) as usize;
    let text_w = area.w - 2 * MARGIN_X;
    let mut rows: Vec<String> = Vec::new();
    // Walk back from the newest line until the rows that fit are found.
    for line in t.visible_lines().iter().rev() {
        let mut wrapped = wrap(canvas, line, text_w);
        wrapped.append(&mut rows);
        rows = wrapped;
        if rows.len() >= max_rows {
            break;
        }
    }
    let start = rows.len().saturating_sub(max_rows);
    for (i, row) in rows[start..].iter().enumerate() {
        let y = area.y + 8 + i as i32 * LINE_H;
        if !row.is_empty() {
            canvas.text(Rect::new(MARGIN_X, y, text_w, LINE_H), row, TextStyle::new(OUTPUT_SIZE));
        }
    }
}

/// Split one output line into the rows it takes at `w` pixels: words are kept together where they fit, and a word
/// wider than the screen is cut by character.
fn wrap(canvas: &mut Canvas, line: &str, w: i32) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    for token in line.split_inclusive(' ') {
        let joined = format!("{cur}{token}");
        if canvas.measure(&joined, OUTPUT_SIZE) <= w {
            cur = joined;
            continue;
        }
        if !cur.is_empty() {
            rows.push(cur.trim_end().to_string());
            cur = String::new();
        }
        if canvas.measure(token, OUTPUT_SIZE) <= w {
            cur = token.to_string();
            continue;
        }
        for c in token.chars() {
            let next = format!("{cur}{c}");
            if !cur.is_empty() && canvas.measure(&next, OUTPUT_SIZE) > w {
                rows.push(std::mem::take(&mut cur));
                cur = c.to_string();
            } else {
                cur = next;
            }
        }
    }
    rows.push(cur.trim_end().to_string());
    rows
}

/// The command being typed, after the prompt. A long command shows its end, where the caret is.
fn input(canvas: &mut Canvas, t: &Terminal) {
    let line = t.input_line();
    canvas.fill(Rect::new(0, line.y, line.w, 2), style::TOP_LINE_GRAY);
    let y = line.y + 26;
    let prompt_w = canvas.measure(PROMPT, INPUT_SIZE) + 4;
    canvas.text(Rect::new(MARGIN_X, y, prompt_w, 50), PROMPT, TextStyle::new(INPUT_SIZE).bold());
    let avail = line.w - 2 * MARGIN_X - prompt_w - 24;
    let mut shown: String = t.command.text.iter().collect();
    while !shown.is_empty() && canvas.measure(&shown, INPUT_SIZE) > avail {
        shown.remove(0);
    }
    let x = MARGIN_X + prompt_w;
    if !shown.is_empty() {
        canvas.text(Rect::new(x, y, avail + 20, 50), &shown, TextStyle::new(INPUT_SIZE));
    }
    let caret_x = x + canvas.measure(&shown, INPUT_SIZE) + 2;
    canvas.fill(Rect::new(caret_x, line.y + 22, 3, 46), BLACK);
}

/// The key bar above the keyboard: Ctrl (on while it is armed), Up, Down and Stop, in the keyboard's key look.
fn key_bar(canvas: &mut Canvas, t: &Terminal) {
    let bar = t.key_bar();
    canvas.fill(Rect::new(0, bar.y, bar.w, 2), style::TOP_LINE_GRAY);
    for (key, r) in t.bar_keys() {
        let (label, on) = match key {
            BarKey::Ctrl => ("Ctrl", t.ctrl),
            BarKey::Up => ("Up", false),
            BarKey::Down => ("Down", false),
            BarKey::Stop => ("Stop", false),
        };
        let def = KeyDef { key: Key::Space, label: label.to_string(), width: 1.0, on };
        canvas.key(&def, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Noto Sans fonts: set `KT_TEST_FONTS` to their folder. Without it the drawing test is skipped.
    fn fonts() -> Option<Vec<String>> {
        let dir = std::env::var("KT_TEST_FONTS").ok()?;
        Some(["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"].iter().map(|f| format!("{dir}/{f}")).collect())
    }

    /// The screen, with the control under `press` (if any) drawn as pressed, as the kit draws it before the action.
    fn screen(fonts: &[String], t: &Terminal, press: Option<(f32, f32)>) -> Vec<u8> {
        let mut canvas = Canvas::new(t.width as u32, t.height as u32, fonts).expect("fonts");
        canvas.begin();
        draw(&mut canvas, t);
        if let Some((x, y)) = press {
            assert!(canvas.press_at(x, y), "a control under the tap");
            canvas.begin();
            draw(&mut canvas, t);
        }
        canvas.finish(false)
    }

    #[test]
    fn a_tap_on_exit_reverses_only_the_exit_button() {
        let Some(fonts) = fonts() else { return };
        let t = Terminal::new(1264, 1680);
        let plain = screen(&fonts, &t, None);
        let back = t.back_button();
        let pressed = screen(&fonts, &t, Some((back.x as f32 + 5.0, back.y as f32 + 5.0)));
        let width = t.width as usize;
        for (i, (a, b)) in plain.iter().zip(&pressed).enumerate() {
            if a != b {
                let (x, y) = (i % width, i / width);
                assert!(
                    x >= back.x as usize && x < (back.x + back.w) as usize && y >= back.y as usize && y < (back.y + back.h) as usize,
                    "only the Exit button changes, but ({x}, {y}) did"
                );
            }
        }
        assert_ne!(plain, pressed, "the Exit button reverses");
    }
}
