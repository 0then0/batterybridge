# BatteryBridge documentation

BatteryBridge separates battery monitoring from charging control. You can inspect a device immediately after building the CLI; changing settings requires a supported interface and the background service.

## Start here

For graphical installation on each supported platform, start with [Install BatteryBridge](install.md). For source builds and developer archives:

1. [Build BatteryBridge](build.md) or use an available platform build.
2. Read [hardware compatibility](hardware.md) and run `batterybridge capabilities`.
3. [Install the service](service.md) if you want to change charging settings.
4. Follow the [usage guide](usage.md) to set a limit or use the tray app.

## User guides

- [Building from source](build.md): prerequisites, CLI, service and desktop builds.
- [Using BatteryBridge](usage.md): commands, tray behavior and compatibility reports.
- [Battery care and schedules](battery-care.md): temporary operations, temperature protection, calibration, telemetry and automation.
- [Configuration and charging policies](configuration.md): thresholds, top-up and persistence.
- [Service installation and recovery](service.md): systemd, launchd and Windows Task Scheduler.
- [Lifecycle: stop, sleep, pause, reset and removal](lifecycle.md): what each action leaves on the hardware, per backend.
- [Hardware compatibility and validation](hardware.md): backend capabilities, limitations and device testing.
- [Security and IPC](security.md): privileges, local access and diagnostic privacy.

## Developer guides

- [Contributing](../CONTRIBUTING.md): development setup, checks and pull requests.
- [Architecture](architecture.md): workspace layout and request flow.
- [Developing a backend](contributing-backends.md): discovery, capabilities, adapters and tests.
- [Repository and releases](github.md): CI, workflow artifacts and release preparation.
- [Changelog](../CHANGELOG.md): user-visible changes and upgrade requirements.
- [Security policy](../SECURITY.md) and [Code of Conduct](../CODE_OF_CONDUCT.md): private reporting and project participation.

## Choosing the right interface

Use the CLI for scripts and terminal workflows. Use the tray app for interactive status and charging settings. Both use the same background service for hardware changes, so the service can keep enforcing a policy after the GUI exits.

The Rust workspace also exposes the protocol, policy and platform layers for contributors and integrations. These APIs are under development and may change between versions.
