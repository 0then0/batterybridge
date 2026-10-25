# Architecture

[Documentation index](README.md)

BatteryBridge separates read-only device discovery, charging policy and privileged hardware access. CLI and GUI share the same commands and service rather than maintaining independent policies.

## Workspace layout

- `crates/protocol`: Serde commands, responses, capabilities, errors and configuration DTOs.
- `crates/core`: platform-independent traits, policy state transitions and configuration persistence. Unsafe code is forbidden.
- `crates/platform`: Linux sysfs, macOS IOKit/SMC and Windows WMI adapters; device discovery and capability resolution.
- `crates/ipc`: framed local communication, platform access controls and connection deadlines.
- `crates/service`: policy ownership, serialized hardware operations and lifecycle handling.
- `crates/cli`: the `batterybridge` command-line application.
- `apps/gui`: native egui/eframe desktop app and shared JSON translation catalogs.
- `packaging`: systemd, launchd and Windows Task Scheduler integration.
- `tests/fixtures`: synthetic device facts used by platform tests.

## Device discovery

`Platform::discover` collects device metadata and reads known interfaces into `Facts`. The pure `resolve` function turns these facts into a `ProbeReport`: candidate selection, capabilities and reasons. Fixture tests exercise the same resolver without accessing a device.

Monitoring is available independently of controller activation. A laptop can report battery state even when its charge-control interface is absent or inaccessible.

## Request flow

1. CLI or GUI validates a typed command.
2. The client sends the request over a Unix socket or Windows named pipe.
3. The service validates protocol version and values, then executes the command on its policy thread.
4. The selected controller performs the hardware operation, and the service persists changed limits or care preferences before responding.

Status and capability reads use local read-only discovery when the service is unavailable. `probe` always runs locally. Configuration reads and mutations require the service. Local discovery does not create a writable controller for the client.

The native desktop app uses the same typed Rust client boundary as the CLI. IPC and export/uninstall work run outside the UI thread. These operations run one at a time; status refreshes preserve unsaved edits. GUI login registration has its own worker and is serialized with uninstall. The tray continues refreshing status every 30 seconds when the window is hidden. The independent daemon continues enforcing policy.

The app directly uses fastframe-shell (tray/window lifecycle), fastframe-tray, fastframe-fonts, fastframe-i18n (system language preferences) and fastframe-autostart (per-user GUI login registration). macOS additionally uses fastframe-dock to retain menu-bar application behavior. eframe supplies system theme following and user preference storage. Existing JSON translations remain the single catalog format for the app and macOS Installer; no gettext conversion or separate UI command protocol is required.

GUI autostart is independent of the charging service. Registration and OS-state
queries run on a separate worker; they do not lock charging controls or replace
unsaved policy drafts. The OS registration is authoritative, with no duplicate
enabled flag in application settings. A login registration passes `--start-hidden`
to fastframe-shell; a missing tray falls back to a normal window. Before a cold
hidden launch, the app reads the saved language from eframe's existing RON file
so that the first tray menu uses the selected language. On macOS 13+, a bundled
SMAppService launch agent opens the app in the background; the package includes
its plist before signing the app. The charging daemon remains a separate job.

On macOS, closing a window into the tray cancels native destruction and hides it,
keeping eframe's AppKit event loop running. Returning from `run_native` into the
headless pump stops status-item clicks on macOS 15.7.9. The tray's Open action shows
and focuses the retained window; Quit and closing without a working tray still
allow native destruction. Windows and Linux retain the shell's existing window
recreation lifecycle.

## Policy lifecycle

The service loads its saved TOML policy at startup, discovers the device and creates a controller when capabilities allow it. It reconciles the desired policy with hardware every 30 seconds while awake. Hardware writes and incoming commands are serialized.

The policy tracks persistent limits separately from temporary top-up, target charge, hold, pause, discharge, calibration and hysteresis state. The core calendar scheduler coalesces missed occurrences; the service durably advances due entries before executing them, so restarting cannot replay a claimed calibration. Service restart clears temporary modes. On macOS, IOKit power notifications coordinate adapter release before sleep and policy reconciliation after wake. Linux and Windows use periodic reconciliation after resume.

BatteryBridge selects the first present battery for control. Other detected batteries remain visible in status. The Windows Lenovo controller requires exactly one battery because its interface does not provide per-pack selection.

## Extending the project

Add platform behavior in the platform layer and expose its availability through capabilities. Shared core logic should remain independent of vendor names, SMC keys and WMI method identifiers. See [backend development](contributing-backends.md) for adapter contracts, fixtures and hardware validation.

The wire protocol, configuration format and diagnostic report each have their own version. Their compatibility rules are described in [security and IPC](security.md) and [configuration](configuration.md).
