# Changelog

User-visible changes are grouped by release. Links point to the corresponding
GitHub releases and comparisons; hardware capabilities remain device-dependent.

## [0.3.1] - 2026-10-25

### Added

- Optional **Launch at login** setting in all seven GUI languages. It starts the
  current user's GUI in the tray, independently of the charging service. On macOS
  it requires a bundled app and macOS 13+; the rest of the app still supports macOS 11+.
- Public contribution, conduct and security policies, an EditorConfig, issue forms
  and a pull request template.

### Fixed

- Closing the macOS window into the tray now retains the native event loop, so
  the tray menu and **Open BatteryBridge** continue working after repeated closes.
- Hidden startup loads the saved language before creating its first tray menu
  and opens a window if no usable tray is available.
- GUI autostart registration is serialized with graphical macOS uninstall.
  The Windows uninstaller removes matching startup records from loaded user profiles.

### Compatibility

- Charging policy format and IPC are unchanged from 0.3.0. Install over the existing
  package to preserve charging settings. See [installation](docs/install.md) for
  login-item approval and removal behavior.
- macOS and Windows installers remain unsigned; Apple SMC and Lenovo WMI charging
  control remain experimental. Firmware may reset charging gates during sleep.

## [0.3.0] - 2026-10-24

### Added

- Native egui/eframe desktop app with selected fastframe modules, replacing Tauri
  and removing the Node.js, WebView2 and WebKitGTK runtime requirements.
- Temporary charge targets, hold/pause/resume, temperature protection, automatic
  discharge, calibration, calendar schedules and MagSafe light settings where supported.
- Extended battery telemetry and CLI commands usable from macOS Shortcuts.

### Changed

- IPC moved from version 2 to 3; upgrade GUI, CLI and service together.
- Saved policy versions 0 and 1 migrate to version 2. Older binaries cannot read
  version 2; back up configuration before upgrading or rolling back.
- Source GUI builds require Rust 1.98+; CLI and service require Rust 1.90+.

## [0.2.0] - 2026-10-23

- Added native Windows and Linux installers and universal macOS packages for Intel
  and Apple Silicon, including the charging service.
- Hardened Windows installation paths, ACLs and installer lifecycle checks.
- Included the glib safety backport used by the former WebKitGTK GUI.

## [0.1.1] - 2026-10-23

- Added graphical macOS uninstall with charging restoration and service removal.
- Improved charging inhibition before macOS sleep; firmware behavior during sleep
  still requires hardware validation.

## [0.1.0] - 2026-10-23

- Initial stable release with cross-platform monitoring, capability discovery,
  supported charge controls, a background service and a localized tray app.

[0.3.1]: https://github.com/0then0/batterybridge/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/0then0/batterybridge/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/0then0/batterybridge/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/0then0/batterybridge/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/0then0/batterybridge/releases/tag/v0.1.0
