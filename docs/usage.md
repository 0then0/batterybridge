# Using BatteryBridge

[Documentation index](README.md)

## Inspect your device

```sh
batterybridge status
batterybridge capabilities
batterybridge probe
```

`status` shows detected batteries, power source, available health information and the selected backend. When connected to the service, it also shows the saved charge limit and temporary top-up state.

`capabilities` lists each charging feature with its availability and an explanation. `probe` collects a compatibility report using read-only local discovery. Both the CLI and tray app can read device information without the service. In that mode, charging policy is shown as unavailable because the service's state is unknown.

## Set a charge limit

Install the [background service](service.md), then run these commands as your ordinary user:

```sh
batterybridge limit 80
batterybridge limit 80 --start 75
batterybridge config
```

The first command sets an upper limit of 80%. The second also requests a 75% start threshold. Explicit start thresholds require the corresponding capability. Stop values range from 1 to 100; hardware may accept a narrower range. The Lenovo WMI backend accepts only 80% or off.

`config` displays the saved TOML policy. Successful limit changes are saved by the service and reapplied after it restarts. See [charging policies](configuration.md) for hysteresis and persistence behavior.

To disable the percentage limit (while retaining enabled care settings):

```sh
batterybridge limit off
```

Use `batterybridge reset` to disable all care settings and schedules as well and restore normal charging. Stopping the service does not clear a saved limit; see [lifecycle](lifecycle.md). On sysfs devices with both attributes, disabling the limit sets start 99 and stop 100. It does not restore another charge manager's previous settings. Run one charge-policy manager at a time.

## Charge to 100% temporarily

```sh
batterybridge top-up
```

Top-up requires an existing saved limit and a backend with the `temporary_full_charge` capability. The service temporarily releases the limit, then restores it when the selected battery reaches 100% or reports full. Restarting the service also ends top-up and restores the saved policy.

## Force discharge

```sh
batterybridge discharge 80
```

This advanced command requires a backend with force-discharge support. It stops at the target percentage and has a minimum target of 20%. Keep the device under supervision during discharge. Sensor failure, service restart, orderly shutdown and macOS sleep cancel the temporary discharge mode.

## Advanced care and automation

Version 0.3.0 adds target charging, hold/pause/resume, automatic discharge, temperature protection, calibration and calendar schedules. Open **Temporary operations** or **Battery care** in the desktop app. Available controls depend on detected hardware. See [battery care and schedules](battery-care.md) for exact behavior and CLI commands.

## Use the tray app

The app shows battery status, charging settings, the active backend and device capabilities. In read-only mode, status and reports remain available while controls are disabled.

Closing the window hides it. Choose **Open BatteryBridge** from the tray or menu-bar icon to show it again. **Quit BatteryBridge** exits the GUI; the separately installed service continues enforcing its policy.

## Share a compatibility report

```sh
batterybridge probe --json > compatibility.json
```

You can also choose **Generate compatibility report** in the app, review the JSON and save it. Reports include model and OS information, local battery ordinals and detected interfaces. They exclude serial numbers, usernames, network addresses and installation identifiers. Review the content before attaching it to an issue.

The `--json` option also works with `status`, `capabilities` and `config`. Increase logging with `-v` or `-vv`, or set `RUST_LOG`. Logs are written to stderr so they can be kept separate from JSON output.

## When a command fails

An unsupported feature includes a reason in the capabilities output. If policy is unavailable, check the service and your local access permissions using the [installation and recovery guide](service.md#troubleshooting). Permission and hardware errors can also come from firmware or a missing vendor driver; a successful read-only probe does not establish write access.

## Desktop language

The app supports English (`en`), Russian (`ru`), Spanish (`es`), German (`de`), French (`fr`), Japanese (`ja`) and Brazilian Portuguese (`pt-BR`). The initial choice follows the first supported language in the system preferences, with English as the fallback. Portuguese regional preferences use the Brazilian translation.

Use **Language** at the bottom of the window to override the system choice. The language names are written in their own languages. The preference is saved locally and changing it does not apply, discard or reset charging inputs. Choose **System default** to return to automatic selection. Theme selection always follows the system independently of language.

Labels, battery states, capability explanations, app notices and tray actions are translated. The macOS Installer welcome and completion pages use the system language, independently of the app's saved choice. CLI output, compatibility JSON and unknown OS/firmware diagnostic details retain their original text and field names for support and automation.

## Export from the desktop app

Choose **Generate compatibility report**, review the JSON and use **Copy JSON**
or **Save JSON report**. The native app proposes a file in Downloads when that
folder exists, otherwise in your home directory. You can edit the destination.
Saving never overwrites an existing file; choose a new name if it already exists.
