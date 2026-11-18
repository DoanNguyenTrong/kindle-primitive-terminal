//! A shell running on a pseudo-terminal, and the command runner on top of it.
//!
//! Adapted from KOReader's terminal plugin (`plugins/terminal.koplugin/main.lua`): `spawn` follows its
//! `spawnShell` (master side, slave side, `setsid`, `dup2` onto stdio, `exec`), and `read_available`,
//! `write` and `Drop` follow its `receive`, `transmit` and `killShell`. See `lib.rs` for the licence note.

use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, OsString},
    io,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        io::RawFd,
    },
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use crate::{pty, strip_ansi, Output};

/// Serialises spawns. `ptsname` uses shared storage, and a fork must not interleave with another spawn.
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

/// Most output kept from one command. Anything beyond it is dropped, so a runaway command cannot fill memory.
const MAX_OUTPUT: usize = 1024 * 1024;

/// How the shell is started.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// A shell name looked up in `PATH`, or an absolute path.
    pub shell: String,
    /// Arguments after the shell name. `-i` makes the shell read commands as an interactive shell.
    pub args: Vec<String>,
    /// Directory the shell starts in. The inherited directory when `None`.
    pub cwd: Option<PathBuf>,
    /// Variables added to, or replacing, the inherited environment.
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            shell: "sh".into(),
            args: vec!["-i".into()],
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
        }
    }
}

/// A shell on a pseudo-terminal. Dropping it hangs up the shell, then kills it if it has not exited.
pub struct Session {
    master: RawFd,
    pid: libc::pid_t,
    /// Set once the shell has closed its side of the terminal.
    closed: bool,
    markers: u64,
    /// The parent's slave descriptor, held only during start-up.
    holder: Option<RawFd>,
}

impl Session {
    /// Start `config.shell` on a new pseudo-terminal.
    ///
    /// The prompt is emptied, the user's shell start-up files are skipped, and `TERM` is `dumb`, so that
    /// programs print plain text. The start-up output is discarded before this returns.
    pub fn spawn(config: &SessionConfig) -> io::Result<Session> {
        let shell_path = resolve_shell(&config.shell).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("shell not found: {}", config.shell))
        })?;
        let shell = cstring(shell_path.as_os_str().as_bytes())?;

        let mut argv = vec![cstring(config.shell.as_bytes())?];
        for arg in &config.args {
            argv.push(cstring(arg.as_bytes())?);
        }
        let argv_ptrs: Vec<*const libc::c_char> = argv
            .iter()
            .map(|s| s.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();

        let envp = environment(config)?;
        let envp_ptrs: Vec<*const libc::c_char> = envp
            .iter()
            .map(|s| s.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();

        let cwd = config
            .cwd
            .as_ref()
            .map(|dir| cstring(dir.as_os_str().as_bytes()))
            .transpose()?;

        let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let master = pty::open_master()?;
        let slave = match pty::slave_name(master) {
            Ok(name) => name,
            Err(err) => {
                // SAFETY: closing the descriptor opened above.
                unsafe { libc::close(master) };
                return Err(err);
            }
        };
        // Held open until the shell has connected. While no slave is open, the master reports the shell
        // as gone, so the parent keeps this descriptor until the start-up handshake is done.
        let holder = match pty::open_slave(&slave) {
            Ok(fd) => fd,
            Err(err) => {
                // SAFETY: closing the descriptor opened above.
                unsafe { libc::close(master) };
                return Err(err);
            }
        };
        if let Err(err) = pty::set_window_size(holder, config.cols, config.rows) {
            // SAFETY: closing the descriptors opened above.
            unsafe {
                libc::close(holder);
                libc::close(master);
            }
            return Err(err);
        }

        // SAFETY: after fork the child runs only `exec_child`, which makes async-signal-safe calls and never
        // returns. Everything it needs was prepared above.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            let err = io::Error::last_os_error();
            // SAFETY: closing descriptors opened above.
            unsafe {
                libc::close(master);
                libc::close(holder);
            }
            return Err(err);
        }
        if pid == 0 {
            unsafe {
                exec_child(
                    master,
                    &slave,
                    &shell,
                    &argv_ptrs,
                    &envp_ptrs,
                    cwd.as_deref(),
                )
            };
        }
        drop(_guard);

        let mut session = Session {
            master,
            pid,
            closed: false,
            markers: 0,
            holder: Some(holder),
        };
        session.await_ready(Instant::now() + Duration::from_secs(10))?;
        session.release_holder();
        Ok(session)
    }

    /// Wait until the shell answers, and turn off echo so commands do not come back in the output.
    ///
    /// Input sent before the shell has started reading is lost, so the setup command is sent again each
    /// time half a second passes without an answer. The shell's start-up output is discarded.
    fn await_ready(&mut self, deadline: Instant) -> io::Result<()> {
        loop {
            self.read_available()?;
            let marker = self.new_marker();
            self.write(marker.script("stty -echo 2>/dev/null").as_bytes())?;
            let mut raw = Vec::new();
            let try_until = (Instant::now() + Duration::from_millis(500)).min(deadline);
            if self.wait_for_marker(&mut raw, &marker.text(), try_until)?.is_some() {
                // Let any copies of the setup command that were sent late finish before the first command.
                thread::sleep(Duration::from_millis(100));
                self.read_available()?;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "the shell did not start"));
            }
        }
    }

    /// A marker no earlier run has used.
    fn new_marker(&mut self) -> Marker {
        self.markers += 1;
        Marker {
            pid: self.pid,
            seq: self.markers,
        }
    }

    /// Close the parent's slave descriptor. From here on the master reports the shell's exit, as EOF.
    fn release_holder(&mut self) {
        if let Some(fd) = self.holder.take() {
            // SAFETY: closing the descriptor this session owns.
            unsafe { libc::close(fd) };
        }
    }

    /// Run `command` in the shell and wait for it to finish, up to `timeout`.
    ///
    /// The command is followed by a `printf` line that prints a unique marker and the exit status. The
    /// output is everything before that marker. On timeout, Ctrl-C stops the command, and the marker is
    /// awaited again so that the next run starts clean.
    pub fn run(&mut self, command: &str, timeout: Duration) -> io::Result<Output> {
        if self.closed {
            return Err(shell_exited());
        }
        // Output nobody asked for (an earlier command that was stopped, a prompt) is discarded.
        self.read_available()?;

        let marker = self.new_marker();
        self.write(marker.script(command).as_bytes())?;

        let mut raw = Vec::new();
        let deadline = Instant::now() + timeout;
        if let Some((bytes, status)) = self.wait_for_marker(&mut raw, &marker.text(), deadline)? {
            return Ok(output(&bytes, status, false));
        }

        // Timed out: stop the command and let the shell reach the marker.
        self.interrupt()?;
        let resync = Instant::now() + Duration::from_secs(2);
        match self.wait_for_marker(&mut raw, &marker.text(), resync)? {
            Some((bytes, _)) => Ok(output(&bytes, None, true)),
            None => Ok(output(&raw, None, true)),
        }
    }

    /// Send Ctrl-C to the shell, which stops the command that is running.
    pub fn interrupt(&mut self) -> io::Result<()> {
        self.write(b"\x03")
    }

    /// Write bytes to the shell's input.
    pub fn write(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(shell_exited());
        }
        let started = Instant::now();
        while !bytes.is_empty() {
            // SAFETY: writes from a slice we hold, to the master descriptor we own.
            let n = unsafe { libc::write(self.master, bytes.as_ptr() as *const libc::c_void, bytes.len()) };
            if n > 0 {
                bytes = &bytes[n as usize..];
                continue;
            }
            let err = io::Error::last_os_error();
            match err.kind() {
                io::ErrorKind::Interrupted => continue,
                io::ErrorKind::WouldBlock => {
                    if started.elapsed() > Duration::from_secs(10) {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "shell input is blocked"));
                    }
                    pty::wait(self.master, libc::POLLOUT, Duration::from_millis(100))?;
                }
                _ => {
                    self.closed = true;
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    /// Read what the shell has written so far, without waiting. Sets the closed flag when the shell has exited.
    pub fn read_available(&mut self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while out.len() < MAX_OUTPUT {
            // SAFETY: reads into a buffer we own, from the master descriptor we own.
            let n = unsafe { libc::read(self.master, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n > 0 {
                out.extend_from_slice(&buf[..n as usize]);
                continue;
            }
            if n == 0 {
                self.closed = true;
                break;
            }
            let err = io::Error::last_os_error();
            match err.kind() {
                io::ErrorKind::WouldBlock => break,
                io::ErrorKind::Interrupted => continue,
                _ if err.raw_os_error() == Some(libc::EIO) => {
                    // Linux reports a closed shell as EIO on the master.
                    self.closed = true;
                    break;
                }
                _ => return Err(err),
            }
        }
        Ok(out)
    }

    /// Collect bytes into `raw` until `marker` arrives, or `deadline` passes. Returns the output before the
    /// marker and the exit status it carries.
    fn wait_for_marker(
        &mut self,
        raw: &mut Vec<u8>,
        marker: &str,
        deadline: Instant,
    ) -> io::Result<Option<(Vec<u8>, Option<i32>)>> {
        loop {
            if let Some((end, status)) = find_marker(raw, marker.as_bytes()) {
                return Ok(Some((raw[..end].to_vec(), status)));
            }
            if self.closed {
                return Err(shell_exited());
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            pty::wait(self.master, libc::POLLIN, deadline - now)?;
            raw.extend(self.read_available()?);
            if raw.len() > MAX_OUTPUT {
                raw.truncate(MAX_OUTPUT);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.release_holder();
        // Closing the terminal hangs up the shell, and an interactive shell exits on hang-up.
        // SAFETY: closing the descriptor this session owns.
        unsafe { libc::close(self.master) };
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut status: libc::c_int = 0;
        loop {
            // SAFETY: waitpid on our own child.
            let rc = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
            if rc != 0 {
                return; // exited, or no such child any more
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        // SAFETY: the shell ignored the hang-up; kill it and reap it.
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            libc::waitpid(self.pid, &mut status, 0);
        }
    }
}

/// Runs in the forked child. Only async-signal-safe calls are made, and the child always exits.
///
/// # Safety
/// Must be called only in the child after `fork`. All pointers must stay valid for the call.
unsafe fn exec_child(
    master: RawFd,
    slave: &CStr,
    shell: &CStr,
    argv: &[*const libc::c_char],
    envp: &[*const libc::c_char],
    cwd: Option<&CStr>,
) -> ! {
    libc::close(master);
    libc::setsid();
    let pts = libc::open(slave.as_ptr(), libc::O_RDWR);
    if pts < 0 {
        libc::_exit(126);
    }
    // The shell is now the terminal's foreground group. A shell that reads input from the background is
    // stopped by SIGTTIN, so this must be set before the shell reads anything.
    libc::tcsetpgrp(pts, libc::getpid());
    libc::dup2(pts, 0);
    libc::dup2(pts, 1);
    libc::dup2(pts, 2);
    if pts > 2 {
        libc::close(pts);
    }
    if let Some(dir) = cwd {
        if libc::chdir(dir.as_ptr()) != 0 {
            libc::_exit(126);
        }
    }
    libc::execve(shell.as_ptr(), argv.as_ptr(), envp.as_ptr());
    libc::_exit(127)
}

/// The environment for the shell: the inherited one, with the session defaults and the configured values.
fn environment(config: &SessionConfig) -> io::Result<Vec<CString>> {
    let mut vars: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (key, value) in [
        ("TERM", "dumb"),
        ("PS1", ""),
        ("PS2", ""),
        ("ENV", ""),
        ("BASH_ENV", ""),
    ] {
        vars.insert(key.into(), value.into());
    }
    for (key, value) in &config.env {
        vars.insert(key.into(), value.into());
    }
    vars.into_iter()
        .map(|(key, value)| {
            let mut entry = key.into_vec();
            entry.push(b'=');
            entry.extend_from_slice(value.as_bytes());
            cstring(&entry)
        })
        .collect()
}

/// A marker that ends one command's output.
struct Marker {
    pid: libc::pid_t,
    seq: u64,
}

impl Marker {
    /// The text the shell prints, without the exit status.
    fn text(&self) -> String {
        format!("__KBT_{}_{}__", self.pid, self.seq)
    }

    /// The command, then a line that prints the marker and the exit status. The printf assembles the marker
    /// from parts, so the echoed copy of this line (when echo is on) never contains it whole.
    fn script(&self, command: &str) -> String {
        format!(
            "{command}\nprintf '\\n__KBT_%s_%s__%s\\n' '{}' '{}' \"$?\"\n",
            self.pid, self.seq
        )
    }
}

/// The end of the output, and the exit status, if `marker` is in `raw` and its status line is complete.
fn find_marker(raw: &[u8], marker: &[u8]) -> Option<(usize, Option<i32>)> {
    let at = raw.windows(marker.len()).position(|w| w == marker)?;
    // The printf line starts with a newline, which the terminal turns into CR LF.
    let end = if raw[..at].ends_with(b"\r\n") {
        at - 2
    } else if raw[..at].ends_with(b"\n") {
        at - 1
    } else {
        at
    };
    let rest = &raw[at + marker.len()..];
    let line_end = rest.iter().position(|&b| b == b'\n')?;
    let digits = String::from_utf8_lossy(&rest[..line_end]);
    let status = digits.trim_end_matches('\r').parse::<i32>().ok();
    Some((end, status))
}

fn output(bytes: &[u8], status: Option<i32>, timed_out: bool) -> Output {
    let mut trimmed = bytes;
    if let Some(stripped) = trimmed.strip_suffix(b"\r\n") {
        trimmed = stripped;
    } else if let Some(stripped) = trimmed.strip_suffix(b"\n") {
        trimmed = stripped;
    }
    Output {
        text: strip_ansi(&String::from_utf8_lossy(trimmed)),
        status,
        timed_out,
    }
}

fn shell_exited() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "the shell has exited")
}

fn cstring(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "text contains a NUL byte"))
}

/// The path of an executable named `name`: `name` itself if it contains a slash, otherwise the first match on `PATH`.
pub fn resolve_shell(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        return is_executable(Path::new(name)).then(|| PathBuf::from(name));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(c) = cstring(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: access(2) on a NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session::spawn(&SessionConfig::default()).expect("a shell should start")
    }

    const LONG: Duration = Duration::from_secs(10);

    #[test]
    fn runs_a_command_and_returns_its_output() {
        let mut s = session();
        let out = s.run("echo hello", LONG).unwrap();
        assert_eq!(out.text, "hello");
        assert_eq!(out.status, Some(0));
        assert!(!out.timed_out);
    }

    #[test]
    fn reports_the_exit_status() {
        let mut s = session();
        assert_eq!(s.run("false", LONG).unwrap().status, Some(1));
        assert_eq!(s.run("sh -c 'exit 7'", LONG).unwrap().status, Some(7));
    }

    #[test]
    fn keeps_multi_line_and_empty_output_exact() {
        let mut s = session();
        assert_eq!(s.run("printf 'a\\nb\\n'", LONG).unwrap().text, "a\nb");
        assert_eq!(s.run("printf 'no newline'", LONG).unwrap().text, "no newline");
        assert_eq!(s.run("true", LONG).unwrap().text, "");
    }

    #[test]
    fn the_shell_keeps_state_between_commands() {
        let mut s = session();
        s.run("KBT_VALUE=42; cd /", LONG).unwrap();
        assert_eq!(s.run("echo $KBT_VALUE", LONG).unwrap().text, "42");
        assert_eq!(s.run("pwd", LONG).unwrap().text, "/");
    }

    #[test]
    fn starts_in_the_configured_directory() {
        let config = SessionConfig {
            cwd: Some(PathBuf::from("/usr")),
            ..SessionConfig::default()
        };
        let mut s = Session::spawn(&config).unwrap();
        assert_eq!(s.run("pwd -P", LONG).unwrap().text, "/usr");
    }

    #[test]
    fn passes_configured_environment_variables() {
        let config = SessionConfig {
            env: vec![("KBT_ENV_TEST".into(), "yes".into())],
            ..SessionConfig::default()
        };
        let mut s = Session::spawn(&config).unwrap();
        assert_eq!(s.run("echo $KBT_ENV_TEST", LONG).unwrap().text, "yes");
    }

    #[test]
    fn times_out_then_recovers_for_the_next_command() {
        let mut s = session();
        let slow = s.run("echo started; sleep 30", Duration::from_millis(500)).unwrap();
        assert!(slow.timed_out);
        assert_eq!(slow.status, None);
        assert!(slow.text.starts_with("started"));

        let next = s.run("echo after", LONG).unwrap();
        assert_eq!(next.text, "after");
        assert_eq!(next.status, Some(0));
    }

    #[test]
    fn a_shell_that_exits_is_reported() {
        let mut s = session();
        let err = s.run("exit 3", LONG).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        assert!(s.run("echo late", LONG).is_err());
    }

    #[test]
    fn finds_the_marker_and_its_status() {
        let raw = b"hi\r\n\r\n__M1__0\r\n";
        assert_eq!(find_marker(raw, b"__M1__"), Some((4, Some(0))));
        assert_eq!(find_marker(b"x\r\n__M2__12", b"__M2__"), None, "status line not finished");
        assert_eq!(find_marker(b"x\r\n__M2__12\r\n", b"__M2__"), Some((1, Some(12))));
        assert_eq!(find_marker(b"no marker here", b"__M3__"), None);
    }

    #[test]
    fn resolves_shells_on_path_and_rejects_missing_ones() {
        assert!(resolve_shell("sh").is_some());
        assert!(resolve_shell("/bin/sh").is_some());
        assert!(resolve_shell("kb-no-such-shell").is_none());
        assert!(Session::spawn(&SessionConfig {
            shell: "kb-no-such-shell".into(),
            ..SessionConfig::default()
        })
        .is_err());
    }
}
