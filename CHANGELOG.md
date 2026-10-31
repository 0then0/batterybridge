# Changelog

User-visible changes are grouped by release. Links point to the corresponding
GitHub releases and comparisons; hardware capabilities remain device-dependent.

## [0.4.0] - 2026-10-31

### Changed

- Stopping or restarting the service (including OS shutdown and reboot, which it
  cannot tell apart) no longer writes "no limit" to native thresholds. Linux
  sysfs, Lenovo conservation mode and Intel `BCLM` keep the **saved** limit; a
  temporary top-up or target is replaced by it. Gates, forced discharge, the
  MagSafe light and the idle-sleep override are still always released. Before this
  change every orderly stop cleared Linux thresholds, so upgrading a package
  or rebooting briefly lifted the limit.
- Each hardware release now has an explicit scope (daemon-held overrides or
  everything). Pause and Reset release everything; Reset is acknowledged only
  after the hardware accepted it and the empty policy was saved. Cleanup steps
  are independent, and a failure names every step that failed.
- Debian and RPM removal now starts a stopped service, runs `batterybridge reset`,
  stops the service and checks it exited cleanly before disabling it. Failure stops
  removal.
- After macOS wake, status reports that hardware state is unverified until the
  first reconciliation finishes. Sleep preparation steps no longer skip each other.
- Shutdown refuses queued requests with an explicit error instead of dropping them
  (the request is read first, and the drain is limited to five seconds).
- `batterybridge status` labels the saved limit as configuration and prints the
  device's own threshold readback separately where the driver exposes it.
- The app's colors now follow the icon's greens in light and dark themes instead of blue,
  and the tray icon is plain white (a black template on macOS, which the menu bar
  recolors). The app icon is a horizontal battery with a limit marker.
- The paused-control message lists what pause releases and that a restart ends it,
  in all seven languages.

### Fixed

- A failed reconciliation (for example an unreadable battery percentage) no longer
  writes "no limit" to a native threshold; it releases daemon-held overrides and
  re-applies the saved threshold. Previously it cleared Linux and Lenovo limits until
  the next successful 30-second check.
- Selected items in drop-down lists keep readable text in both themes.
- The app no longer silently drops an action issued while another request is
  running or leaves the previous success banner in place; it shows a message.
- Service troubleshooting names the correct socket group for Debian and RPM packages.

### Compatibility

- IPC (version 3) and the saved policy format are unchanged.
- The persistence of any threshold or gate while the computer is off is
  **unverified** on every device; no hardware validation report exists. SIGKILL,
  power loss and forced OS shutdown run no cleanup. See [lifecycle](docs/lifecycle.md).
- Version 0.4.0 rather than a patch release: the protocol and configuration are
  compatible, but observable stop and removal behavior changed.

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

[0.4.0]: https://github.com/0then0/batterybridge/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/0then0/batterybridge/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/0then0/batterybridge/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/0then0/batterybridge/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/0then0/batterybridge/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/0then0/batterybridge/releases/tag/v0.1.0
