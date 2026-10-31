# Security and IPC

[Documentation index](README.md)

For supported versions and private vulnerability reporting, see the
[security policy](../SECURITY.md). This guide describes the implementation boundary.

## Privilege boundary

The background service owns the writable charge controller and machine-wide policy. CLI and GUI run as ordinary applications. They request a fixed set of operations: status, capabilities, compatibility reports, configuration reads, validated limits, temporary charging operations, bounded discharge, calibration and calendar care settings.

The service exposes local IPC only. Its commands accept typed charging settings rather than shell commands, filesystem paths, SMC keys or WMI method names. Both clients and service validate request values, and each backend checks its hardware-specific constraints.

## Local access

On Unix, sockets are created in an administrator-owned directory that is not group/world writable. Socket mode is `0660`. Manual Linux installation grants access to root and the `batterybridge` group. Native Linux packages instead use the existing local administrator group (`sudo` for Debian, `wheel` for RPM), without changing membership; macOS grants access to root and the local `admin` group. Group membership permits changes to the machine's charging policy, shared by all users.

An OS file lock protects Unix endpoint ownership. Startup can reclaim a stale socket after confirming that no listener is active. Active listeners and non-socket paths are preserved.

Windows uses a named pipe with a protected DACL. Network logons are denied. SYSTEM and Administrators have full access, and interactive local users have read/write access. Any authorized interactive user can change the machine-wide charging policy.

The Windows installer rejects reparse points and trees owned by other users before copying code. It replaces complete file/directory DACLs so that executable and state directories are writable only by Administrators/SYSTEM. The implementation uses safe `interprocess` security-descriptor wrappers. WMI queries are local.

## Protocol

The current request protocol is version 3. Messages are Serde JSON with a four-byte big-endian length prefix and a maximum payload of 64 KiB. Frame reads and writes each have a three-second deadline. On Windows, an empty nonblocking transfer waits for progress within that deadline. Connections are configured once before exchanging frames.

At most eight accepted requests wait for the single policy thread. Hardware operations are serialized. The listener blocks while idle, and the policy thread waits for requests, power events or its 30-second reconciliation interval. Client traffic does not indefinitely postpone policy reconciliation. On macOS, queued mutations are rejected while the service handles sleep.

Upgrade CLI, GUI and service together when the protocol changes. Incompatible versions return an explicit error. Read-only fallback is limited to status and capabilities when the service is unavailable or access is denied; configuration reads and mutations require the service.

In an offline status response, `config` is null and all policy state, including temporary modes, is unavailable. Clients must not interpret the temporary-mode placeholder fields as current state.

## Policy persistence

Configuration stores desired settings, not temporary hardware state. The service applies a validated change and persists it before acknowledging success. If persistence fails, it attempts to restore the previous policy.

Writes use an exclusively created, synced sibling file and same-filesystem replacement. An existing transaction file is preserved for recovery. See [configuration](configuration.md) and [service recovery](service.md#troubleshooting) for operational details.

## Hardware safety

Private SMC interfaces can reject writes even with root privileges. Read-only discovery does not establish write permission. Platform authorization and firmware protections remain in effect.

Manual and automatic discharge have a minimum target of 20%. The explicitly requested calibration cycle alone may discharge to 10%. Adapter disable is bounded by the same 20% floor; indefinite isolation is not exposed. The service releases isolation before macOS sleep and on orderly shutdown; a saved native limit is retained on shutdown and cleared only by `limit off`, `reset` or uninstall (see [lifecycle](lifecycle.md)). Firmware changes and abrupt termination can still affect charging state; supervised validation is described in the [hardware guide](hardware.md#device-validation).

## Diagnostic privacy

Compatibility reports use an allowlist of device metadata and interface results. They exclude serials, usernames, network identifiers, installation IDs and raw WMI instance paths. Logs avoid raw provider payloads and identifiers. Review reports and logs before sharing, especially when adding information collected outside BatteryBridge.

## Native GUI dependency boundary

The source GUI uses egui/eframe with an OpenGL renderer. It does not embed a web
runtime or accept JavaScript commands. The app directly calls the shared typed
Rust IPC boundary as an ordinary user. Privileged charging operations remain in
the service.

The former GTK3/WebKitGTK dependency graph and local glib backport were removed
with the native GUI migration. Version 0.2.0 used the upstream backport for
[GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g).
The current graph must be checked again when changing renderer, tray or dialog
features; native rendering does not imply that dependencies are free of defects.

Compatibility reports are exported by the ordinary-user GUI. Saving creates a
new file and refuses to overwrite an existing destination. Graphical macOS
uninstall executes only the embedded, fixed removal script after administrator
authentication; no file path or script from the UI is passed to that helper.

Care settings and scheduled actions use the existing local authorization boundary. Schedules accept only typed operations, not shell commands, paths or URLs. At most 16 entries with unique IDs are accepted. Updating care settings checks the expected previous value to prevent stale clients from reactivating completed tasks. No web/deep-link mutation endpoint is exposed for Shortcuts.

Temperature protection uses battery telemetry only. Missing temperature inhibits charging while protection is enabled; this is a software check at the normal awake reconciliation interval, not a replacement for firmware thermal protection. The macOS idle-sleep assertion is scoped to the service process and is released on cancellation or exit; global power settings are unchanged. Lid-close and manual sleep remain possible.
