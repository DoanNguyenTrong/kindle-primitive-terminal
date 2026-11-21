# Changelog

## 0.2.0

- Primitive Terminal is a kit app now: it runs on the app kit (`primitive-app`) like the other Primitive apps. The
  kit draws the screen, turns it with the Kindle, follows Primitive Launcher (sleep, quick settings, the theme), and
  shows a tap on a control by reversing it: the Exit button, the keys, the key bar, while the tap is held.
- The terminal's own device code is gone (the e-ink driver, touch and page buttons, accelerometer). The shell, the
  output and the keys are the same.
- The library entry gives the terminal the fonts of its own folder when it opens without the launcher.
- The Primitive Browser stays on its own screen code for now (see the umbrella repository's README, "Known exceptions
  to the app kit").

## 0.1.10

- The Exit button is shown reversed while it is tapped, as the keys are.

Each device build gets a new patch version.

## 0.1.9

- Under Primitive Launcher the terminal follows the system layer. It follows the launcher's light or dark mode, and
  redraws the whole screen without a flash when the launcher asks (SIGUSR1, after a wake or a theme change). The
  swipe down from the top edge opens the launcher's quick settings; the terminal no longer scrolls on it. Opened from
  the Kindle library, the terminal works as before.

## 0.1.8

- Package the standalone Terminal for ARMHF and ARMEL.
- Ship the AGPL software license and the separate documentation license.

## 0.1.3

- The Turn button is gone. The screen turns with the Kindle, as the browser and the launcher do: the framework's
  accelerometer property, or the accelerometer itself when the framework is stopped.

## 0.1.2

- A key bar above the keyboard: Ctrl (the next letter is sent as a control character), Up and Down (past commands),
  and Stop (Ctrl-C, for `top` and long commands). Stop moved from the title strip to the bar.
- Turn: a button turns the screen upside down (for left-handed use); the Kindle's accelerometer turns it too, as
  Primitive Launcher's Auto does.
- Stands alone: the library entry hides the status bar while the terminal runs, gives it back on exit, and opens
  the library. It no longer needs Primitive Launcher (it still runs from the launcher's grid).

## 0.1.0

- The first version, as its own app: a shell with the shared keyboard, streamed output, scrolling, Stop, and Back.
  The shell engine moved here from Primitive Browser, where it was the base of the Diagnostics page.
