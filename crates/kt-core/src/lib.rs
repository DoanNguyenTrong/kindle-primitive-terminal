//! The terminal's screen state: the output lines, the command being typed, the key bar, the history, the scroll
//! position, the screen's turn, and what a tap or a swipe does. No shell and no device here: the driver runs the
//! shell (`kt-engine`), sends what it prints to `feed`, and sends what `Action` asks for.

pub mod rotation;

use primitive_keyboard::{Editor, Key, Layer, Press};
pub use rotation::Rotation;

/// The most output lines kept. The oldest go first.
pub const MAX_LINES: usize = 2000;
/// The most characters of one command, and how many past commands are kept for Up and Down.
pub const MAX_COMMAND: usize = 200;
pub const MAX_HISTORY: usize = 200;
/// The title strip at the top, the line the command is typed on, and the key bar above the keyboard.
pub const TITLE_H: i32 = 120;
pub const INPUT_H: i32 = 96;
pub const BAR_H: i32 = 96;
/// The keyboard at the bottom: the same area as the launcher's and the browser's (`primitive-keyboard`).
pub const KEYBOARD_H: i32 = primitive_keyboard::style::AREA_H as i32;
/// The key that runs the command.
pub const GO_LABEL: &str = "Run";
/// The prompt written before each command in the output.
pub const PROMPT: &str = "$ ";

/// A rectangle in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x as f32 && x < (self.x + self.w) as f32 && y >= self.y as f32 && y < (self.y + self.h) as f32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

/// What the device gives the terminal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Tap(f32, f32),
    /// A swipe: how far the finger moved (positive down), and where it went down.
    Swipe { dy: f32, start_y: f32 },
    /// The Back key on the device.
    Back,
}

/// What the driver must do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    /// Draw the screen again.
    Redraw,
    /// Send this line to the shell (it ends with a newline).
    Run(String),
    /// Send these bytes to the shell as they are (a control character such as Ctrl-C).
    Send(Vec<u8>),
    /// Stop the command that is running (Ctrl-C).
    Interrupt,
    /// Leave the terminal.
    Close,
}

/// The keys of the key bar, above the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKey {
    /// The next letter typed is sent as a control character (Ctrl-C, Ctrl-D, ...).
    Ctrl,
    /// The previous command, and the next one.
    Up,
    Down,
    /// Ctrl-C: stops a command that is running (`top`, a long download).
    Stop,
}

/// The terminal's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    pub width: i32,
    pub height: i32,
    /// Finished output lines, oldest first. Commands are in them too, with the prompt.
    pub lines: Vec<String>,
    /// The line still being written by the shell (no newline yet).
    pub partial: String,
    /// The command being typed, and the keyboard's layer.
    pub command: Editor,
    /// How many lines the output is scrolled up from its newest line (0: at the bottom).
    pub scroll: usize,
    /// Whether the screen is turned upside down: the Kindle's turn (auto-rotation, as the browser and the launcher).
    pub rotation: Rotation,
    /// Ctrl is on: the next letter is sent as a control character.
    pub ctrl: bool,
    /// The key under the finger, drawn in the on look until its action runs (see `press_feedback`).
    pub pressed: Option<Rect>,
    /// Commands run, oldest first, and the one Up and Down show (None: the command being typed).
    pub history: Vec<String>,
    history_pos: Option<usize>,
}

impl Terminal {
    pub fn new(width: i32, height: i32) -> Terminal {
        Terminal {
            width,
            height,
            lines: Vec::new(),
            partial: String::new(),
            command: Editor::new(MAX_COMMAND),
            scroll: 0,
            rotation: Rotation::Upright,
            ctrl: false,
            pressed: None,
            history: Vec::new(),
            history_pos: None,
        }
    }

    // ----- layout -----

    pub fn title_bar(&self) -> Rect {
        Rect { x: 0, y: 0, w: self.width, h: TITLE_H }
    }

    /// The Back button, at the right of the title strip.
    pub fn back_button(&self) -> Rect {
        Rect { x: self.width - 40 - 170, y: (TITLE_H - 80) / 2, w: 170, h: 80 }
    }

    /// The Turn button, beside Back: turns the screen upside down (or back).
    pub fn turn_button(&self) -> Rect {
        let back = self.back_button();
        Rect { x: back.x - 20 - 170, y: back.y, w: back.w, h: back.h }
    }

    pub fn output_area(&self) -> Rect {
        Rect { x: 0, y: TITLE_H, w: self.width, h: self.height - TITLE_H - INPUT_H - BAR_H - KEYBOARD_H }
    }

    pub fn input_line(&self) -> Rect {
        let out = self.output_area();
        Rect { x: 0, y: out.bottom(), w: self.width, h: INPUT_H }
    }

    /// The key bar: Ctrl, Up, Down and Stop, in a row above the keyboard.
    pub fn key_bar(&self) -> Rect {
        let line = self.input_line();
        Rect { x: 0, y: line.bottom(), w: self.width, h: BAR_H }
    }

    /// The key bar's keys, left to right, with their rectangles.
    pub fn bar_keys(&self) -> Vec<(BarKey, Rect)> {
        let bar = self.key_bar();
        let (margin, gap) = (12, 10);
        let each = (bar.w - 2 * margin - 3 * gap) / 4;
        [BarKey::Ctrl, BarKey::Up, BarKey::Down, BarKey::Stop]
            .into_iter()
            .enumerate()
            .map(|(i, key)| {
                let x = bar.x + margin + i as i32 * (each + gap);
                (key, Rect { x, y: bar.y + 12, w: each, h: bar.h - 24 })
            })
            .collect()
    }

    pub fn keyboard_area(&self) -> Rect {
        Rect { x: 0, y: self.height - KEYBOARD_H, w: self.width, h: KEYBOARD_H }
    }

    /// The keys on screen, with their rectangles in screen pixels.
    pub fn keys(&self) -> Vec<(primitive_keyboard::KeyDef, Rect)> {
        let area = self.keyboard_area();
        primitive_keyboard::layout(self.command.layer, GO_LABEL, area.w as u32, area.h as u32)
            .into_iter()
            .map(|(def, r)| (def, Rect { x: area.x + r.x, y: area.y + r.y, w: r.w, h: r.h }))
            .collect()
    }

    /// The key under a tap, if any.
    pub fn key_at(&self, x: f32, y: f32) -> Option<Key> {
        self.keys().into_iter().find(|(_, r)| r.contains(x, y)).map(|(def, _)| def.key)
    }

    // ----- output -----

    /// Add what the shell printed. ANSI codes are removed, CR LF is one line break, tabs become spaces.
    pub fn feed(&mut self, bytes: &[u8]) {
        let text = kt_engine::strip_ansi(&String::from_utf8_lossy(bytes));
        for c in text.chars() {
            match c {
                '\n' => self.finish_line(),
                '\r' => {}
                '\t' => self.partial.push_str("    "),
                c if c.is_control() => {}
                c => self.partial.push(c),
            }
        }
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(..extra);
        }
        self.scroll = self.scroll.min(self.lines.len());
    }

    fn finish_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.partial));
    }

    /// The output lines the screen can show, oldest first, with the partial line last. The scroll position is
    /// applied: the newest `scroll` lines are left out.
    pub fn visible_lines(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.lines.iter().map(|s| s.as_str()).collect();
        if !self.partial.is_empty() {
            out.push(&self.partial);
        }
        let end = out.len().saturating_sub(self.scroll);
        out.truncate(end);
        out
    }

    // ----- input -----

    /// Handle a device input.
    /// A tap on a key (the keyboard or the key bar) marks the key as pressed, so the driver can show it before the
    /// key's action runs. Returns whether a key was marked; `handle` clears the mark.
    pub fn press_feedback(&mut self, input: Input) -> bool {
        let Input::Tap(x, y) = input else { return false };
        let bar = self.bar_keys().into_iter().find(|(_, r)| r.contains(x, y)).map(|(_, r)| r);
        let keyboard = self.keys().into_iter().find(|(_, r)| r.contains(x, y)).map(|(_, r)| r);
        match bar.or(keyboard) {
            Some(r) => {
                self.pressed = Some(r);
                true
            }
            None => false,
        }
    }

    pub fn handle(&mut self, input: Input) -> Action {
        self.pressed = None;
        match input {
            Input::Back => Action::Close,
            Input::Tap(x, y) => {
                if self.back_button().contains(x, y) {
                    return Action::Close;
                }
                if let Some((key, _)) = self.bar_keys().into_iter().find(|(_, r)| r.contains(x, y)) {
                    return self.bar_key(key);
                }
                if self.keyboard_area().contains(x, y) {
                    return match self.key_at(x, y) {
                        Some(key) => self.press(key),
                        None => Action::None,
                    };
                }
                Action::None
            }
            Input::Swipe { dy, start_y } => {
                if start_y >= self.keyboard_area().y as f32 {
                    return Action::None;
                }
                // A swipe down shows older lines; a swipe up brings the newest ones back. One line per 40 px.
                let step = ((dy.abs() / 40.0) as usize).max(1);
                let before = self.scroll;
                if dy > 8.0 {
                    self.scroll = (self.scroll + step).min(self.lines.len());
                } else if dy < -8.0 {
                    self.scroll = self.scroll.saturating_sub(step);
                }
                if self.scroll == before { Action::None } else { Action::Redraw }
            }
        }
    }

    /// A key of the key bar.
    pub fn bar_key(&mut self, key: BarKey) -> Action {
        match key {
            BarKey::Ctrl => {
                self.ctrl = !self.ctrl;
                Action::Redraw
            }
            BarKey::Up => self.recall(true),
            BarKey::Down => self.recall(false),
            BarKey::Stop => Action::Interrupt,
        }
    }

    /// Up (`older`) and Down: show a past command in the line, to be run again or changed.
    fn recall(&mut self, older: bool) -> Action {
        let len = self.history.len();
        if len == 0 {
            return Action::None;
        }
        let next = match (self.history_pos, older) {
            (None, true) => Some(len - 1),
            (Some(p), true) => Some(p.saturating_sub(1)),
            (Some(p), false) if p + 1 < len => Some(p + 1),
            (Some(_), false) => None,
            (None, false) => return Action::None,
        };
        self.history_pos = next;
        let text = next.map(|p| self.history[p].clone()).unwrap_or_default();
        self.command.text = text.chars().take(MAX_COMMAND).collect();
        self.command.caret = self.command.text.len();
        Action::Redraw
    }

    /// A key press on the keyboard. Go runs the command: it is echoed in the output, kept in the history, and sent
    /// to the shell. With Ctrl on, a letter is sent as its control character instead of being typed.
    pub fn press(&mut self, key: Key) -> Action {
        if key == Key::Go {
            let text = self.command.string();
            self.command = Editor::new(MAX_COMMAND);
            self.lines.push(format!("{PROMPT}{text}"));
            self.scroll = 0;
            if !text.is_empty() && self.history.last() != Some(&text) {
                self.history.push(text.clone());
                if self.history.len() > MAX_HISTORY {
                    self.history.remove(0);
                }
            }
            self.history_pos = None;
            return Action::Run(format!("{text}\n"));
        }
        if let (true, Key::Char(c)) = (self.ctrl, key) {
            if c.is_ascii_alphabetic() {
                self.ctrl = false;
                return Action::Send(vec![(c.to_ascii_lowercase() as u8) & 0x1f]);
            }
        }
        match self.command.press(key) {
            Press::Nothing => Action::None,
            _ => {
                self.history_pos = None;
                Action::Redraw
            }
        }
    }

    /// The layer the keyboard shows.
    pub fn layer(&self) -> Layer {
        self.command.layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term() -> Terminal {
        Terminal::new(1264, 1680)
    }

    /// The centre of a key, found by its key value.
    fn centre_of(t: &Terminal, key: Key) -> (f32, f32) {
        let (_, r) = t.keys().into_iter().find(|(d, _)| d.key == key).expect("key on screen");
        ((r.x + r.w / 2) as f32, (r.y + r.h / 2) as f32)
    }

    fn centre_bar(t: &Terminal, key: BarKey) -> (f32, f32) {
        let (_, r) = t.bar_keys().into_iter().find(|(k, _)| *k == key).unwrap();
        ((r.x + r.w / 2) as f32, (r.y + r.h / 2) as f32)
    }

    fn type_text(t: &mut Terminal, text: &str) {
        for c in text.chars() {
            let key = if c == ' ' { Key::Space } else { Key::Char(c) };
            let (x, y) = centre_of(t, key);
            t.handle(Input::Tap(x, y));
        }
    }

    #[test]
    fn typing_then_go_sends_the_line_and_echoes_it() {
        let mut t = term();
        type_text(&mut t, "ls");
        assert_eq!(t.command.string(), "ls");
        let (x, y) = centre_of(&t, Key::Go);
        assert_eq!(t.handle(Input::Tap(x, y)), Action::Run("ls\n".into()));
        assert_eq!(t.lines, vec!["$ ls".to_string()]);
        assert!(t.command.string().is_empty(), "the line is cleared after Run");
    }

    #[test]
    fn an_empty_run_sends_a_bare_newline_and_is_not_kept_in_history() {
        let mut t = term();
        assert_eq!(t.press(Key::Go), Action::Run("\n".into()));
        assert_eq!(t.lines, vec!["$ ".to_string()]);
        assert!(t.history.is_empty());
    }

    #[test]
    fn output_is_split_into_lines_and_a_partial_line_is_shown() {
        let mut t = term();
        t.feed(b"one\r\ntwo\nthr");
        assert_eq!(t.lines, vec!["one".to_string(), "two".to_string()]);
        assert_eq!(t.partial, "thr");
        assert_eq!(t.visible_lines(), vec!["one", "two", "thr"]);
        t.feed(b"ee\n");
        assert_eq!(t.lines.last().unwrap(), "three");
    }

    #[test]
    fn ansi_codes_are_removed_and_tabs_become_spaces() {
        let mut t = term();
        t.feed(b"\x1b[31mred\x1b[0m\tend\n");
        assert_eq!(t.lines, vec!["red    end".to_string()]);
    }

    #[test]
    fn the_oldest_lines_go_first() {
        let mut t = term();
        for i in 0..MAX_LINES + 5 {
            t.feed(format!("{i}\n").as_bytes());
        }
        assert_eq!(t.lines.len(), MAX_LINES);
        assert_eq!(t.lines[0], "5");
    }

    #[test]
    fn swiping_down_shows_older_lines_and_stops_at_the_oldest() {
        let mut t = term();
        for i in 0..10 {
            t.feed(format!("{i}\n").as_bytes());
        }
        assert_eq!(t.handle(Input::Swipe { dy: 120.0, start_y: 300.0 }), Action::Redraw);
        assert_eq!(t.scroll, 3);
        assert_eq!(t.visible_lines().last().copied(), Some("6"));
        t.handle(Input::Swipe { dy: 4000.0, start_y: 300.0 });
        assert_eq!(t.scroll, 10, "stops at the oldest line");
        t.handle(Input::Swipe { dy: -4000.0, start_y: 300.0 });
        assert_eq!(t.scroll, 0, "back at the newest line");
    }

    #[test]
    fn a_swipe_on_the_keyboard_does_nothing() {
        let mut t = term();
        t.feed(b"a\nb\n");
        let y = t.keyboard_area().y as f32 + 10.0;
        assert_eq!(t.handle(Input::Swipe { dy: 300.0, start_y: y }), Action::None);
    }

    #[test]
    fn back_closes() {
        let mut t = term();
        let b = t.back_button();
        assert_eq!(t.handle(Input::Tap(b.x as f32 + 5.0, b.y as f32 + 5.0)), Action::Close);
        assert_eq!(t.handle(Input::Back), Action::Close);
    }

    #[test]
    fn the_screen_turns_only_with_the_kindle() {
        let mut t = term();
        let b = t.back_button();
        // A tap where a Turn button would be does nothing: the turn follows the Kindle.
        let x = (b.x - 100) as f32;
        assert_eq!(t.handle(Input::Tap(x, b.y as f32 + 5.0)), Action::None);
        assert_eq!(t.rotation, Rotation::Upright);
    }

    #[test]
    fn stop_on_the_key_bar_interrupts_the_command() {
        let mut t = term();
        let (x, y) = centre_bar(&t, BarKey::Stop);
        assert_eq!(t.handle(Input::Tap(x, y)), Action::Interrupt);
    }

    #[test]
    fn ctrl_sends_the_next_letter_as_a_control_character() {
        let mut t = term();
        let (x, y) = centre_bar(&t, BarKey::Ctrl);
        assert_eq!(t.handle(Input::Tap(x, y)), Action::Redraw);
        assert!(t.ctrl);
        let (x, y) = centre_of(&t, Key::Char('c'));
        assert_eq!(t.handle(Input::Tap(x, y)), Action::Send(vec![0x03]), "Ctrl-C");
        assert!(!t.ctrl, "Ctrl is off after one letter");
        assert!(t.command.string().is_empty(), "the letter is not typed");
    }

    #[test]
    fn up_and_down_recall_past_commands() {
        let mut t = term();
        type_text(&mut t, "echo a");
        t.press(Key::Go);
        type_text(&mut t, "echo b");
        t.press(Key::Go);
        let (ux, uy) = centre_bar(&t, BarKey::Up);
        t.handle(Input::Tap(ux, uy));
        assert_eq!(t.command.string(), "echo b");
        t.handle(Input::Tap(ux, uy));
        assert_eq!(t.command.string(), "echo a");
        t.handle(Input::Tap(ux, uy));
        assert_eq!(t.command.string(), "echo a", "stops at the oldest");
        let (dx, dy) = centre_bar(&t, BarKey::Down);
        t.handle(Input::Tap(dx, dy));
        assert_eq!(t.command.string(), "echo b");
        t.handle(Input::Tap(dx, dy));
        assert!(t.command.string().is_empty(), "past the newest, the line is empty again");
    }

    #[test]
    fn a_tap_marks_its_key_pressed_until_the_action_runs() {
        let mut t = term();
        let (x, y) = centre_of(&t, Key::Char('q'));
        assert!(t.press_feedback(Input::Tap(x, y)));
        assert!(t.pressed.is_some());
        t.handle(Input::Tap(x, y));
        assert!(t.pressed.is_none(), "the action clears the mark");
        let (x, y) = centre_bar(&t, BarKey::Up);
        assert!(t.press_feedback(Input::Tap(x, y)), "the key bar marks too");
        assert!(!t.press_feedback(Input::Tap(600.0, 300.0)), "the output marks nothing");
    }

    #[test]
    fn the_screen_has_the_key_bar_and_the_keyboard_at_the_bottom() {
        let t = term();
        assert_eq!(t.keyboard_area().bottom(), 1680);
        assert_eq!(t.key_bar().bottom(), t.keyboard_area().y);
        assert_eq!(t.input_line().bottom(), t.key_bar().y);
        assert_eq!(t.output_area().y, TITLE_H);
    }
}
