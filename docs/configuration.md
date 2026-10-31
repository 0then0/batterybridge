# Configuration and charging policies

[Documentation index](README.md)

## Saved settings

The background service owns one machine-wide policy. Use `batterybridge limit` to change it and `batterybridge config` to inspect the saved TOML:

```toml
version = 2
charge_limit = 80

[start_threshold]
enabled = true
value = 75
```

`version` identifies the configuration format. `charge_limit` is the upper charging percentage. An enabled `start_threshold` requests the lower percentage at which charging resumes. Start must be less than stop, and the backend must support both requested values.

A disabled limit is represented by an omitted `charge_limit` field, not by zero. The `limit off` command also disables the explicit start threshold. Configuration paths are listed in the [service guide](service.md).

Prefer CLI changes over editing a running service's configuration file. For manual edits, stop the service, back up the file, edit it as administrator and restart the service. The service loads configuration at startup; it does not watch the file for external changes.

## Hysteresis

A lower start threshold prevents repeated charging transitions around the upper limit. For a 75–80% band, charging stops at 80% and resumes at the lower boundary.

Without an explicit start threshold, BatteryBridge uses a five-percentage-point band where a native start threshold or software charge gate is available. A device with only an end threshold uses that attribute alone.

On Linux, the backend writes native threshold attributes and reads them back. Off and top-up use start 99 and stop 100 when both attributes exist. A requested stop of 100 with no explicit start also uses start 99. Driver restrictions and rejected values are reported as errors.

On Apple Silicon, the service samples battery state every 30 seconds while awake and controls an SMC charging gate. This is a software policy, so the percentage can cross a boundary between samples. Firmware may change gate state during sleep or shutdown.

On the Lenovo WMI backend, the policy selects a fixed 80% conservation mode or off. Arbitrary percentages and start thresholds are unavailable.

## Temporary modes

Top-up, target charge, pause, hold, active force discharge and calibration exist only in service memory. They are not saved in TOML. Restarting the service restores the saved limit and clears temporary modes, including pause. Stopping the service keeps the saved limit; see [lifecycle](lifecycle.md). Setting or disabling a limit also cancels temporary modes.

Top-up completes when the selected battery reaches 100% or reports full. Force discharge stops at its target and is cancelled on sensor failure, orderly shutdown or macOS sleep.

## Persistence and recovery

The service validates changes, applies them, then saves the policy before acknowledging success. If saving fails, it attempts to restore the previous policy. Hardware and filesystem errors remain visible to the caller.

Configuration is written to a sibling `config.toml.new` file, synced and renamed over the committed file. An existing transaction file is preserved for inspection. Recovery steps for interrupted writes are in the [service guide](service.md#troubleshooting).

Versionless configurations and versions 0 and 1 migrate to version 2 in memory, preserving their limits and disabling new care features. The next successful settings change saves version 2. Older binaries cannot read version 2: back up configuration before upgrading; remove the `care` section and set version 1 for a deliberate downgrade. Unknown fields, invalid limits and future versions are rejected. Invalid configuration prevents service startup; it is not replaced with defaults. A missing configuration starts with the limit disabled.

## Battery-care settings (format 2)

The `[care]` section is optional; omitted values disable the corresponding feature:

```toml
[care]
heat_limit_c = 35
automatic_discharge = false
prevent_sleep = false
led = "system"
schedules = []
```

Omit `heat_limit_c` to disable temperature protection. Its supported range is 25–60°C. `led` accepts `system`, `automatic`, `off`, `green`, `orange`, `slow_blink_orange` or `fast_blink_orange`. The service rejects enabled settings that the selected backend cannot perform.

GUI care updates include the original settings as a compare value. If another client or a scheduled task changed them, saving fails instead of overwriting newer state. Refresh and discard the old draft before saving again. See [battery care](battery-care.md) for schedules, priorities and safe cancellation.
