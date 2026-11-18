#!/bin/sh
# Copy Primitive Terminal onto the Kindle over SSH (Wi-Fi).
#
#   deploy/install-ssh.sh [path/to/kt-terminal]
#
# Needs the SSH server running on the Kindle (the launcher's Settings > SSH & DNS). The root password is asked for
# once: every step shares one connection. Nothing secret is stored in this repository.
#
# Environment: KINDLE_HOST (default 192.168.1.28), KINDLE_SSH_PORT (default 2222).
#
# Files go to /mnt/us/terminal (the binary and fonts) and /mnt/us/documents/Terminal.sh (the library entry the
# launcher lists). Each is copied only when its SHA-256 differs, under a temporary name and then renamed.
set -eu

HERE=$(cd "$(dirname "$0")/.." && pwd)
HOST=${KINDLE_HOST:-192.168.1.28}
PORT=${KINDLE_SSH_PORT:-2222}
BIN=${1:-$HERE/target/armv7-unknown-linux-musleabihf/release/kt-terminal}

[ -f "$BIN" ] || { echo "binary not found: $BIN" >&2; exit 1; }
file "$BIN" | grep -q "ARM" || { echo "not an ARM binary: $BIN" >&2; exit 1; }

CTL="/tmp/ktssh-$$"
SSH="ssh -p $PORT -o ControlMaster=auto -o ControlPath=$CTL -o ControlPersist=120 -o StrictHostKeyChecking=accept-new root@$HOST"
cleanup() {
    ssh -p "$PORT" -o ControlPath="$CTL" -O exit "root@$HOST" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "connecting to root@$HOST:$PORT"
$SSH 'mkdir -p /mnt/us/terminal/fonts /mnt/us/documents'

upload() {
    $SSH "cat > '$2.new' && sync && mv '$2.new' '$2'" < "$1"
    echo "copied: $2"
}

# Copy only when the device's copy differs (SHA-256).
sync_file() {
    local_sum=$(shasum -a 256 "$1" | cut -c1-64)
    remote_sum=$($SSH "sha256sum '$2' 2>/dev/null" | cut -c1-64 || true)
    if [ "$local_sum" != "$remote_sum" ]; then
        upload "$1" "$2"
    fi
}

sync_file "$BIN" /mnt/us/terminal/kt-terminal
sync_file "$HERE/deploy/Terminal.sh" /mnt/us/documents/Terminal.sh
for font in NotoSans-Regular.ttf NotoSans-Bold.ttf NOTO-LICENSE.txt; do
    sync_file "$HERE/assets/fonts/$font" "/mnt/us/terminal/fonts/$font"
done
$SSH 'chmod 755 /mnt/us/terminal/kt-terminal /mnt/us/documents/Terminal.sh 2>/dev/null || true'

local_sum=$(shasum -a 256 "$BIN" | cut -c1-64)
remote_sum=$($SSH 'sha256sum /mnt/us/terminal/kt-terminal' | cut -c1-64)
if [ "$local_sum" != "$remote_sum" ]; then
    echo "checksum mismatch: local $local_sum, device $remote_sum" >&2
    exit 1
fi
echo "installed: /mnt/us/terminal/kt-terminal ($(wc -c < "$BIN" | tr -d ' ') bytes, sha256 $local_sum)"
