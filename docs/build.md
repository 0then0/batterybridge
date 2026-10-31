# Building from source

[Documentation index](README.md)

## Requirements

CLI and service builds require stable Rust 1.90 or newer and a native toolchain.
The desktop GUI requires Rust 1.98 or newer. CI uses stable Rust. The app uses
`egui` and `eframe` with selected fastframe modules; Node.js and a WebView are
not required.

On macOS, install Xcode Command Line Tools. On Windows, use the Rust MSVC toolchain
with the C++ build tools and Windows SDK. On Ubuntu 24.04, install the native
graphics libraries:

```sh
sudo apt-get update
sudo apt-get install -y libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libgl1-mesa-dev libegl1-mesa-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev fonts-noto-cjk
```

Use the committed `Cargo.lock`. fastframe modules are pinned to release `v0.14.1`
and its resolved Git commit in the lockfile. The app uses released egui/eframe
0.36.2 without upstream forks. Windows uses WGPU so the GUI does not require a
separate OpenGL driver; macOS and Linux use Glow. Native package builders require
Python 3.9+.

## CLI and service

```sh
cargo build --release --locked
./target/release/batterybridge --help
./target/release/batterybridge status
```

The default workspace members produce `batterybridge` and
`batterybridge-service`. Windows executables have an `.exe` suffix. Building does
not install or start the service; see [service installation](service.md).

## Desktop application

```sh
cargo run -p batterybridge-gui --locked
cargo build -p batterybridge-gui --release --locked
```

The binary is `target/release/batterybridge-gui` (`.exe` on Windows). The GUI
uses the same ordinary-user IPC client as the CLI. Closing its window leaves the
tray available when the desktop has a tray host; choosing Quit exits the GUI.
Charging policy remains in the independent service. Without a tray host, closing
the window exits instead of leaving an unreachable process.

The interface follows the system theme. Existing JSON catalogs cover English,
Russian, Spanish, German, French, Japanese and Brazilian Portuguese.
fastframe-fonts bundles Inter and finds installed fallback fonts. Install a CJK
font package for Japanese on Linux; the native packages recommend Noto CJK.

## Native installers

Build CLI, service and GUI before packaging:

```sh
cargo build --workspace --release --locked
```

On macOS, the universal builder compiles all three binaries for both architectures,
merges them, creates the `.app` and applies an ad-hoc application signature:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
python3 packaging/macos/build-universal.py
python3 packaging/macos/build-pkg.py --output target/BatteryBridge-macos-universal.pkg
```

The output is under `target/universal-apple-darwin/release`. The package builder
refuses a payload missing either architecture. The Installer package is unsigned
and the app is not notarized.

On Windows:

```powershell
python packaging/windows/build.py
```

The builder downloads the pinned NSIS 3.13 toolchain and verifies its SHA-256
before extraction. Output is under `target/release/bundle/nsis`. The installer
retains the protected SYSTEM task, administrator prompt and charging restoration
on removal.

On Linux, install `dpkg-deb` for Debian packages and `rpmbuild` for RPM packages
(the Ubuntu package is `rpm`), then run:

```sh
python3 packaging/linux/build.py deb
python3 packaging/linux/build.py rpm
```

Outputs are under `target/release/bundle/deb` and `rpm`. Packages contain the GUI,
CLI, service, systemd unit, desktop entry, icons and license notices. They require
native OpenGL/EGL and X11/Wayland libraries. Developer archives contain the GUI
binary; they require these system libraries too.

## Checks and safe previews

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
python3 -m unittest discover -s packaging/tests -v
python3 -m unittest discover -s packaging/macos/tests -v
```

GUI tests exercise translations, font coverage, narrow/wide layouts, read-only
state, editable settings and limit warnings. A synthetic preview performs no IPC
or hardware operations:

```sh
cargo run -p batterybridge-gui --locked -- --preview apps/gui/tests/status.json --language ru
```

Install/uninstall integration scripts run only on disposable CI machines. Do not
run `test-installer` or `test-preinstall` scripts on your personal system.

## Application icon

`assets/icon.svg` is the source. `apps/gui/icons` holds its PNG, `.ico` and `.icns`
renditions, produced by a dependency-free script that mirrors the SVG geometry:

```sh
python3 assets/render-icons.py
```

The `.icns` file needs macOS `iconutil`. `packaging/tests/test_icons.py` fails if the
committed files drift from the script or the SVG colors.
