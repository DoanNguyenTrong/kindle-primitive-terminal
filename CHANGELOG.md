# Changelog

Each device build gets a new patch version.

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
