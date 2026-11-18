//! Pseudo-terminal calls for a shell session. Unix only.
//!
//! Adapted from KOReader's terminal plugin (`plugins/terminal.koplugin/main.lua`): it opens the master side
//! of a pseudo-terminal, grants and unlocks it, and runs the shell on the slave side. See `lib.rs` for the
//! licence note.

use std::{
    ffi::{CStr, CString},
    io,
    os::unix::io::RawFd,
    time::Duration,
};

/// Open the master side of a new pseudo-terminal, in non-blocking mode.
pub fn open_master() -> io::Result<RawFd> {
    // SAFETY: plain libc calls on a descriptor we own. It is closed on every error path.
    unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::grantpt(fd) != 0 || libc::unlockpt(fd) != 0 {
            let err = io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }
        if let Err(err) = set_nonblocking(fd) {
            libc::close(fd);
            return Err(err);
        }
        Ok(fd)
    }
}

/// The path of the slave side that belongs to `master`.
///
/// `ptsname` returns shared storage, so callers must not run it concurrently. The session spawner holds a
/// lock around it.
pub fn slave_name(master: RawFd) -> io::Result<CString> {
    // SAFETY: the returned pointer is copied out before anything else can reuse the storage.
    let name = unsafe { libc::ptsname(master) };
    if name.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { std::ffi::CStr::from_ptr(name) }.to_owned())
}

/// Open the slave side of a pseudo-terminal, without making it the controlling terminal of this process.
/// The descriptor is close-on-exec, so the shell does not inherit it.
pub fn open_slave(slave: &CStr) -> io::Result<RawFd> {
    // SAFETY: opens a path we were given; the caller owns the returned descriptor.
    let fd = unsafe { libc::open(slave.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

/// Set the window size the shell and its programs see. Shells and full-screen tools read it from here.
///
/// `slave` is an open slave descriptor. macOS refuses the size on the master side, so the slave is used.
pub fn set_window_size(slave: RawFd, cols: u16, rows: u16) -> io::Result<()> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCSWINSZ reads one `winsize` from the pointer, which lives for the whole call.
    let rc = unsafe { libc::ioctl(slave, libc::TIOCSWINSZ as _, &size as *const libc::winsize) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor owned by the caller.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Wait until `fd` is ready for `events` (`POLLIN` or `POLLOUT`), or until `timeout` passes.
/// Returns true when it is ready. A hang-up also counts as ready, so the next read can report it.
pub fn wait(fd: RawFd, events: libc::c_short, timeout: Duration) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    let millis = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
    // SAFETY: one pollfd, which lives for the whole call.
    let rc = unsafe { libc::poll(&mut pfd, 1, millis) };
    if rc < 0 {
        let err = io::Error::last_os_error();
        return if err.kind() == io::ErrorKind::Interrupted {
            Ok(false)
        } else {
            Err(err)
        };
    }
    Ok(rc > 0 && pfd.revents != 0)
}
