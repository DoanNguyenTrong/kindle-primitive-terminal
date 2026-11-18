#!/bin/sh
# Name: Terminal
# Author: Primitive AI
# DontUseFBInk
#
# Library entry for Primitive Terminal. It stands alone: it opens from the Kindle's library and needs no other app.
# While it runs it hides the status bar (the framework keeps running, as Primitive Browser's does), and when it exits
# it gives the status bar back and opens the library. Opened from Primitive Launcher's grid (PRIMITIVE_LAUNCHER=1),
# the launcher has already taken the screen, so this script leaves the Kindle's UI alone.
#
# Files live in /mnt/us/terminal: kt-terminal, fonts/, log-terminal.txt, and the status-bar marker.

DIR=/mnt/us/terminal
LOG=$DIR/log-terminal.txt
MARK=$DIR/status-bar-hidden
LAUNCHED=${PRIMITIVE_LAUNCHER:-0}
cd "$DIR" || exit 1

log() { echo "[terminal $(date '+%H:%M:%S')] $*" >> "$LOG"; }

# Keep the log small on the Kindle's flash: keep the last ~200 KB.
if [ -f "$LOG" ] && [ "$(wc -c < "$LOG")" -gt 204800 ]; then
    tail -c 102400 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi

# Give the status bar back, and open the library. A marker from an earlier run (killed here) is undone first.
restore_ui() {
    [ -f "$MARK" ] || return 0
    log "status bar back"
    lipc-set-prop com.lab126.pillow disableEnablePillow enable 2>/dev/null
    rm -f "$MARK"
}

if [ "$LAUNCHED" != 1 ]; then
    restore_ui
    echo hide > "$MARK"
    log "status bar hidden"
    lipc-set-prop com.lab126.pillow disableEnablePillow disable 2>/dev/null
    trap 'restore_ui' EXIT
    trap 'exit 130' INT TERM HUP
fi

log "start"
env KT_DIR="$DIR" ./kt-terminal >> "$LOG" 2>&1
status=$?
log "exit $status"

if [ "$LAUNCHED" != 1 ]; then
    restore_ui
    lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home 2>/dev/null
fi
exit $status
