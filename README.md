<p align="center"><img src="assets/icon.svg" alt="BatteryBridge icon" width="128" height="128"></p>

# BatteryBridge

[![CI](https://img.shields.io/github/actions/workflow/status/0then0/BatteryBridge/ci.yml?label=CI)](https://github.com/0then0/BatteryBridge/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-1f6b4f)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.90%2B-934b32)](https://www.rust-lang.org/)
[![Status](https://img.shields.io/badge/status-experimental-99743b)](docs/hardware.md)

BatteryBridge is a battery charge-control tool for Linux, macOS and Windows. It provides a standalone CLI, a background service and a native egui tray app, with shared charging policies implemented in Rust.

Available controls depend on your laptop's kernel driver, firmware and vendor interfaces. BatteryBridge detects these capabilities and explains which features are available on your device.

**Stable release:** 0.4.0, with an explicit stop, pause, reset and removal contract: stopping the service keeps a saved native limit (see [lifecycle](docs/lifecycle.md)). CI tests and builds the project on all three operating systems. Apple SMC and Lenovo WMI charge control remain experimental and require device-level validation; see [hardware compatibility](docs/hardware.md) for the current support boundaries.

## Features

- Read battery percentage, charging state, power source and available health information.
- Set charge limits and start thresholds where supported by the device.
- Temporarily charge to 100%, hold another target, pause control or discharge to a safe target.
- Configure temperature protection, automatic discharge and calibration on compatible SMC hardware.
- Schedule charging actions by local calendar time, including monthly tasks.
- Read available temperature, voltage, current, power, cycles and raw capacity information.
- Control compatible MagSafe lights and prevent idle sleep during active battery care.
- Use installed CLI commands from macOS Shortcuts.
- Keep charging settings active through a separate background service.
- Optionally launch the GUI in the tray at login, independently of that service.
- Inspect device capabilities and export a compatibility report.
- Use the CLI independently of the tray app.
- Use the desktop app in English, Russian, Spanish, German, French, Japanese or Brazilian Portuguese, with system light/dark appearance.

See [battery care and schedules](docs/battery-care.md) for hardware requirements, cancellation behavior and differences from other charge managers.

## Downloads

<!-- downloads:start -->

Direct downloads for stable release [v0.4.0](https://github.com/0then0/BatteryBridge/releases/tag/v0.4.0):

| Platform | Architecture | Download | Installation |
| --- | --- | --- | --- |
| macOS 11+ | Intel + Apple Silicon (universal) | [Installer (.pkg)](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/BatteryBridge-macos-universal.pkg) | App and background service |
| Windows 10/11 | x86_64 | [Installer (.exe)](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/BatteryBridge-windows-x86_64-setup.exe) | App and SYSTEM background task |
| Ubuntu 24.04+ / Debian 13+ | x86_64 | [Debian package (.deb)](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/BatteryBridge-linux-x86_64.deb) | App and systemd service; requires the `sudo` group |
| Fedora 41+ / compatible RPM systems | x86_64 | [RPM package (.rpm)](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/BatteryBridge-linux-x86_64.rpm) | App and systemd service; requires the `wheel` group |

Developer archives: [macOS universal](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/batterybridge-macos-universal.tar.gz), [Windows](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/batterybridge-windows-x86_64.zip), [Linux](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/batterybridge-linux-x86_64.tar.gz).

[SHA-256 checksums](https://github.com/0then0/BatteryBridge/releases/download/v0.4.0/SHA256SUMS.txt) · [Installation instructions](docs/install.md)

The installers include the charging service; no separate Terminal or PowerShell setup is needed. macOS and Windows installers are unsigned. Linux packages require a running systemd system and native OpenGL/EGL graphics libraries; they are not universal Linux binaries. Hardware charge control remains device-dependent and experimental on macOS and Lenovo WMI.

<!-- downloads:end -->

## Getting started

Download the installer for your system above, open it in Finder, Explorer or your desktop's package manager, and approve the administrator prompt. Open **BatteryBridge** from Applications or the application menu. The installer starts the background charging service automatically.

Use the system's app/package manager to remove Windows and Linux packages. On macOS, use **Uninstall BatteryBridge** in the app. See [installation](docs/install.md) for account permissions, updates, unsigned-package prompts and removal behavior.

Enable **Launch at login** below the language selector to start the GUI automatically.
It starts the GUI in the tray after sign-in, independently of the charging
service. See [autostart requirements](docs/install.md#launch-at-login).

### Build from source

Build the CLI and service with Rust 1.90 or newer:

```sh
git clone https://github.com/0then0/BatteryBridge.git
cd BatteryBridge
cargo build --release --locked
./target/release/batterybridge status
./target/release/batterybridge capabilities
./target/release/batterybridge probe --json
```

On Windows, use `.\target\release\batterybridge.exe` in place of `./target/release/batterybridge`.

Status and compatibility reports work without installing the service. To change charging settings, follow the [service installation guide](docs/service.md), then run the CLI as your ordinary user:

```sh
batterybridge limit 80
batterybridge limit 80 --start 75
batterybridge limit off
```

A backend may support only some of these settings. For example, the Lenovo WMI backend accepts a fixed 80% limit or off. Check `batterybridge capabilities` before changing settings.

### Desktop app

The source GUI uses egui/eframe with selected fastframe modules. With Rust 1.98+ and the native graphics prerequisites from the [build guide](docs/build.md):

```sh
cargo run -p batterybridge-gui --locked
cargo build -p batterybridge-gui --release --locked
```

The app connects directly to the shared Rust IPC client and needs no web server. A source-built app needs separate service setup; native platform installers include the service. Without service access, it displays battery information in read-only mode.

See [building from source](docs/build.md) for native installers, universal macOS packaging and safe previews.

## Compatibility

- **Linux:** generic `charge_control_end_threshold` support, with a separate start threshold when exposed by the kernel. No vendor restriction.
- **macOS:** battery monitoring through IOKit; experimental SMC charge control on Intel and Apple Silicon. Intel BCLM supports firmware thresholds of 50–100% when detected; other models may be monitoring-only.
- **Windows:** standard battery monitoring; experimental Lenovo WMI conservation mode with a fixed 80% limit, restricted to devices with one detected battery. Other vendor controllers are not yet implemented.

Capabilities use four states: `supported`, `experimental`, `unsupported` and `unknown`. A readable interface does not guarantee that firmware will permit a write. Full feature descriptions and validation requirements are in [hardware compatibility](docs/hardware.md).

## Documentation

Start with the [documentation index](docs/README.md), or open a guide directly:

- [Build and run](docs/build.md)
- [CLI and desktop usage](docs/usage.md)
- [Service installation and recovery](docs/service.md)
- [Configuration and charging policies](docs/configuration.md)
- [Hardware compatibility and testing](docs/hardware.md)
- [Architecture](docs/architecture.md)
- [Security and IPC](docs/security.md)

Compatibility reports include device model, OS version and detected charging interfaces. They exclude serial numbers, usernames and network identifiers. Review a report before attaching it to an [issue](https://github.com/0then0/BatteryBridge/issues).

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) covers local checks, bug reports and pull requests.
Read the [Code of Conduct](CODE_OF_CONDUCT.md), [security policy](SECURITY.md) and
[changelog](CHANGELOG.md) for participation rules, private vulnerability reporting
and release changes. To add device support, read the [backend development guide](docs/contributing-backends.md). The [repository guide](docs/github.md) explains CI, build artifacts and the release process.

## Roadmap

Current priorities are device validation of the existing backends, broader Lenovo support, and controllers for ASUS, Dell, HP, MSI and Acer. Further work includes additional Linux vendor interfaces, firmware-managed Apple Silicon limits and a stable backend API.

## License

BatteryBridge and its original SVG icon are licensed under [Apache-2.0](LICENSE). Hardware-interface references are listed in the [compatibility guide](docs/hardware.md#interface-references).
