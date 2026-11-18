//! Shell command engine. It runs commands in a shell on a pseudo-terminal and returns each command's output,
//! exit status, and whether it timed out. The browser's troubleshooting tools are built on it; there is no
//! interface here.
//!
//! The engine is ported from KOReader's terminal emulator (`plugins/terminal.koplugin/main.lua`, copyright
//! the KOReader contributors, GNU AGPL-3.0). The port keeps its steps: open a pseudo-terminal, start the shell
//! on the slave side, read and write the master side without blocking, and stop a command with Ctrl-C or
//! hang-up. The code is rewritten in Rust. This crate is AGPL-3.0 as well.
//!
//! Unix only. Pseudo-terminals exist on macOS (where the tests run) and on the Kindle's Linux.

mod pty;
mod session;

pub use session::{resolve_shell, Session, SessionConfig};

/// What one command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Everything the command wrote, with ANSI control sequences removed and line endings as LF.
    pub text: String,
    /// The exit status, or `None` if the command timed out or the status was not reported.
    pub status: Option<i32>,
    /// True if the command was still running when its timeout passed. It was then stopped with Ctrl-C.
    pub timed_out: bool,
}

/// Remove ANSI escape sequences (colours, cursor moves, window titles) and carriage returns, which a
/// terminal would otherwise render. Newlines and tabs are kept.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if c == '\n' || c == '\t' || !c.is_control() {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            // CSI: parameters, then a final byte from '@' to '~'.
            Some('[') => {
                for d in chars.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            // OSC: ends at BEL, or at ESC '\'.
            Some(']') => {
                let mut prev = '\0';
                for d in chars.by_ref() {
                    if d == '\u{7}' || (prev == '\u{1b}' && d == '\\') {
                        break;
                    }
                    prev = d;
                }
            }
            // Designation sequences such as ESC ( B take one more character.
            Some('(' | ')' | '#' | '%' | ' ') => {
                chars.next();
            }
            // Any other two-character sequence (ESC =, ESC 7, ...) is complete.
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::strip_ansi;

    #[test]
    fn removes_colours_cursor_moves_and_titles() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("a\u{1b}[2K\u{1b}[1;1Hb"), "ab");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
        assert_eq!(strip_ansi("\u{1b}]8;;x\u{1b}\\link"), "link");
        assert_eq!(strip_ansi("\u{1b}(Bok\u{1b}="), "ok");
    }

    #[test]
    fn keeps_newlines_and_tabs_and_drops_other_controls() {
        assert_eq!(strip_ansi("a\tb\nc\r\n"), "a\tb\nc\n");
        assert_eq!(strip_ansi("bell\u{7}"), "bell");
    }
}
