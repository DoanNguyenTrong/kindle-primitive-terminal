#!/bin/sh
# Stage the files installed by Primitive Store or copied from the package over USB.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

ARCH=${1:-armhf}
case "$ARCH" in
    armhf) TARGET=armv7-unknown-linux-musleabihf ;;
    armel) TARGET=armv7-unknown-linux-musleabi ;;
    *) echo "unsupported package architecture: $ARCH" >&2; exit 1 ;;
esac

BIN=${CARGO_TARGET_DIR:-target}/$TARGET/release/kt-terminal
[ -x "$BIN" ] || { echo "build the device binary first: $BIN" >&2; exit 1; }
file "$BIN" | grep -q 'ELF.*ARM' || { echo "not an ARM ELF binary: $BIN" >&2; exit 1; }

STAGE=build/stage
rm -rf "$STAGE"
mkdir -p "$STAGE/terminal/fonts" "$STAGE/documents"
install -m 755 "$BIN" "$STAGE/terminal/kt-terminal"
install -m 755 deploy/Terminal.sh "$STAGE/documents/Terminal.sh"
cp assets/fonts/NotoSans-Regular.ttf assets/fonts/NotoSans-Bold.ttf assets/fonts/NOTO-LICENSE.txt \
    "$STAGE/terminal/fonts/"
echo "staged $STAGE"
