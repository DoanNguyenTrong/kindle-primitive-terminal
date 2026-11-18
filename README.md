# Primitive Terminal

A shell for jailbroken Kindles, on grayscale e-ink. Type a command on the on-screen keyboard, run it, and read the
output. It is a separate app of the [Primitive Launcher](../doannt-kindle-launcher), which starts it from its grid.

- **Shell.** One `/bin/sh` runs for the whole session, so `cd` and variables carry over. Output appears as it is
  printed. Stop (Ctrl-C) stops the running command.
- **Keyboard.** The shared keyboard from `tools/` (the same one as Primitive Launcher and Primitive Browser), with
  the layers, Shift, Del and Run.
- **Scrolling.** Swipe down for older output, swipe up for the newest.
- **Leaving.** Back (the button, or the page button) returns to the launcher.

## Layout of the repo

```text
crates/kt-engine   the shell on a pseudo-terminal (ported from KOReader's terminal plugin, AGPL-3.0)
crates/kt-core     the screen state: output lines, the command typed, scrolling, taps (no device, no shell)
crates/kt-render   drawing: text with parley, shapes with vello_cpu, the keyboard in the shared look
crates/kt-kindle   the Kindle binary: e-ink panel, touch and page buttons (copied from the launcher), the shell
crates/kt-desktop  a driver for the Mac: runs the Mac's shell and writes screenshots, for testing
deploy/            install-ssh.sh (copies over SSH) and Terminal.sh (the library entry the launcher lists)
scripts/           build-device.sh (patch bump, build, install)
tools/             the shared keyboard, quick-settings figures and icons (a submodule)
```

## Build and test

```sh
cargo test --workspace                          # the screen state, the renderer, the engine
cargo build --release -p kt-kindle --target armv7-unknown-linux-musleabihf   # or: cargo zigbuild (as the launcher does)
printf 'type ls\ngo\nwait 400\nshot ls\nquit\n' | cargo run -p kt-desktop -- --out out   # screenshots on the Mac
```

Install on the Kindle over Wi-Fi, with the launcher's SSH server running: `scripts/build-device.sh`.

## Licence

AGPL-3.0. The shell engine is ported from KOReader's terminal plugin (`plugins/terminal.koplugin`, copyright the
KOReader contributors, AGPL-3.0); see the header of `crates/kt-engine/src/lib.rs`. The e-ink and input code in
`crates/kt-kindle` is copied from Primitive Launcher and Primitive Browser (AGPL-3.0). The Noto Sans fonts are under
the SIL Open Font License (`assets/fonts/NOTO-LICENSE.txt`).
