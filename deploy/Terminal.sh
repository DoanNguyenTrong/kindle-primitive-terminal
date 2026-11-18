#!/bin/sh
# Name: Terminal
# Author: Primitive AI
# DontUseFBInk
#
# Library entry for Primitive Terminal. It runs from Primitive Launcher's grid, which has stopped the Kindle's
# framework and owns the screen; the terminal draws the screen itself and runs the Kindle's shell. Leaving it
# (Back) returns to the launcher. Files live in /mnt/us/terminal: kt-terminal, fonts/, log-terminal.txt.

DIR=/mnt/us/terminal
cd "$DIR" || exit 1
exec env KT_DIR="$DIR" ./kt-terminal >> "$DIR/log-terminal.txt" 2>&1
