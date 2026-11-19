# Install Primitive Terminal

## Install with Store

After [setting up PrimitiveOS](https://github.com/DoanNguyenTrong/kindle-primitive-launcher/blob/main/docs/INSTALL.md#first-time-setup-over-usb),
open **Store** from Primitive Launcher. Select **Primitive Terminal** under **Apps** and tap **Get**. Open Terminal
from the app page or the Launcher grid. Apply later versions from **Updates**.

## Install over USB

The USB package works on a Kindle Oasis 3 running firmware 5.18.2 with library script support already installed.

1. Obtain `primitive-terminal-<version>-armhf.zip` and its `.sha256` file. If you built the package locally, both files
   are in `dist/`.
2. Compare the ZIP's SHA-256 with the value in the `.sha256` file, then unzip it.
3. Connect the Kindle by USB with the Kindle library open. Merge the extracted `terminal/` and `documents/` folders
   into the root of the Kindle drive.
4. Eject the drive and open **Terminal** from the Kindle library or the Primitive Launcher grid.

## Install over SSH

With the Kindle's SSH server running, build and copy the binary and library entry from the checkout.

```sh
cargo zigbuild --release -p kt-kindle --target armv7-unknown-linux-musleabihf
KINDLE_HOST=<kindle-ip> sh deploy/install-ssh.sh
```

Run `sh scripts/build-device.sh` to bump the patch version, build and install in one step.

## Build the Store package locally

Use the same device build as above, then stage the installed files and run the shared packager from this checkout.

```sh
sh scripts/stage.sh armhf
python3 ../doannt-kindle-build/scripts/package.py --id primitive-terminal --dir terminal \
  --version 0.1.8 --arch armhf --stage build/stage --license LICENSE \
  --docs-license LICENSE-DOCS --install INSTALL.md --out dist
```

Use the version in `Cargo.toml` when packaging another version.
