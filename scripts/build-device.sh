#!/bin/sh
# Build and install Primitive Terminal on the Kindle over SSH.
#
#   scripts/build-device.sh
#
# Every device build is a new patch version (0.1.0 -> 0.1.1). The version is the workspace version in Cargo.toml.
# The bump is undone if the build or the install fails.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/bin:$PATH"
MANIFEST="$ROOT/Cargo.toml"

current=$(sed -n 's/^version = "\([0-9]*\.[0-9]*\.[0-9]*\)"/\1/p' "$MANIFEST" | head -n 1)
next=$(python3 -c "import sys; a,b,c=sys.argv[1].split('.'); print(f'{a}.{b}.{int(c)+1}')" "$current")

set_version() {
    python3 - "$MANIFEST" "$1" <<'PY'
import re, sys
path, version = sys.argv[1], sys.argv[2]
text = open(path).read()
text, n = re.subn(r'^version = "[0-9]+\.[0-9]+\.[0-9]+"', f'version = "{version}"', text, count=1, flags=re.M)
assert n == 1, "workspace version line not found"
open(path, "w").write(text)
PY
}

echo "version: $current -> $next"
set_version "$next"
undo() { set_version "$current"; echo "build or install failed: version left at $current"; }
trap undo ERR

cd "$ROOT"
cargo zigbuild --release -p kt-kindle --target armv7-unknown-linux-musleabihf >/dev/null
sh deploy/install-ssh.sh target/armv7-unknown-linux-musleabihf/release/kt-terminal
trap - ERR
echo "deployed Primitive Terminal v$next"
