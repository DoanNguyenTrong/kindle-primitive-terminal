<div align="center">

<img src="icon.png" alt="Primitive Terminal icon" width="110">

# Primitive Terminal

**Run shell commands on Kindle**

[![CI](https://img.shields.io/github/actions/workflow/status/PrimitiveOS/kindle-primitive-terminal/ci.yml?branch=main&label=CI)](https://github.com/PrimitiveOS/kindle-primitive-terminal/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/PrimitiveOS/kindle-primitive-terminal?label=release)](https://github.com/PrimitiveOS/kindle-primitive-terminal/releases)
[![Tag](https://img.shields.io/github/v/tag/PrimitiveOS/kindle-primitive-terminal?label=tag)](https://github.com/PrimitiveOS/kindle-primitive-terminal/tags)
[![Licence: AGPL-3.0-only](https://img.shields.io/badge/licence-AGPL--3.0--only-blue)](LICENSE)
[![Stars](https://img.shields.io/github/stars/PrimitiveOS/kindle-primitive-terminal?style=social)](https://github.com/PrimitiveOS/kindle-primitive-terminal/stargazers)
[![Store downloads](https://img.shields.io/endpoint?url=https%3A%2F%2Fprimitiveos.github.io%2Fkindle-primitive-catalog%2Fv1%2Fbadges%2Fprimitive-terminal.json)](https://primitiveos.github.io/kindle-primitive-catalog/v1/index.json)

[**Download**](https://github.com/PrimitiveOS/kindle-primitive-terminal/releases/latest) ·
[**Install guide**](INSTALL.md) ·
[**Changelog**](CHANGELOG.md)

</div>

---

Type a shell command on the on-screen keyboard and read its output. Primitive Terminal opens from the Kindle library and can also run from Primitive Launcher.

## Quick start

After setting up PrimitiveOS, open **Store** from Primitive Launcher. Choose **Primitive Terminal** under **Apps**,
tap **Get**, and open it from the Launcher grid. The [install guide](INSTALL.md) also covers the standalone USB package.

## Screenshots

<table>
  <tr>
    <td align="center"><img src="docs/images/command-entry.png" alt="Terminal with a sample command typed above the on-screen keyboard" width="260"><br><sub>Type a command</sub></td>
    <td align="center"><img src="docs/images/command-output.png" alt="Terminal conversation showing a variable printed by a shell command" width="260"><br><sub>Read the result</sub></td>
    <td align="center"><img src="docs/images/session-directory.png" alt="Terminal showing the result of pwd after changing directories" width="260"><br><sub>Keep working in the same session</sub></td>
  </tr>
</table>

<sub>Captured from the macOS desktop build with sample shell commands.</sub>

## Highlights

- **One shell per session.** Working directory and shell variables carry over between commands. Output appears as
  the command runs. Tap **Stop** to send Ctrl-C.
- **On-screen keyboard.** Use the shared PrimitiveOS keyboard with Shift, symbols, Del, Run, and a Ctrl key.
- **Scroll through output.** Swipe down for older lines and swipe up to move toward the latest output.
- **Return to the library.** Tap **Back** or use the back page button to exit.

## Status

| Device | Status |
|---|---|
| Kindle Oasis 3 (1264 × 1680, firmware 5.18.2) | Tested on hardware |

**Current version: 0.1.9.** Changes are in [CHANGELOG.md](CHANGELOG.md).

## Downloads

The badge at the top shows the number of Store downloads. It counts GitHub downloads of this app's release packages, across all versions and architectures, and is refreshed daily from the [Store catalog](https://primitiveos.github.io/kindle-primitive-catalog/v1/index.json).

## Documentation

[Install guide](INSTALL.md) · [Changelog](CHANGELOG.md)

## Licence

- **Code** is licensed under GNU AGPL-3.0-only. The shell engine is ported from the KOReader terminal plugin,
  copyright the KOReader contributors. The e-ink and input code comes from the AGPL versions of Primitive
  Launcher and Primitive Browser. [Software licence](LICENSE) · [Engine attribution](crates/kt-engine/src/lib.rs)
- **Original documentation and pictures**, including this README and its screenshots, are copyright (C) 2026
  Doan Nguyen and licensed under CC BY-NC-SA 4.0. [Documentation and pictures licence](LICENSE-DOCS)
- **Fonts** use the SIL Open Font License. [Noto Sans licence](assets/fonts/NOTO-LICENSE.txt)
