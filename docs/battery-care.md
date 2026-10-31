# Battery care and schedules

[Documentation index](README.md)

These controls are available starting with 0.3.0. Upgrade the app, CLI and service together; 0.2.0 packages do not include them. The service reports what your hardware can perform; monitoring values that the OS does not expose remain unavailable.

## Saved policy and temporary operations

The saved limit and lower resume threshold form a charging band, sometimes called sailing mode. They remain machine-wide and are enforced by the service after the GUI exits. Temporary operations preserve that saved policy:

```sh
batterybridge charge-to 90
batterybridge discharge 80
batterybridge hold
batterybridge pause
batterybridge resume
batterybridge cancel
batterybridge adapter off
batterybridge adapter on
```

- `charge-to` charges to the requested level and maintains that temporary target until cancellation. It can override a lower saved limit. It does not force an already higher battery percentage down.
- `discharge` uses battery power while plugged in until its target, then restores the saved limit. The minimum is 20%.
- `hold` stops charging immediately until cancellation; it requires a real charging gate.
- `pause` releases all BatteryBridge hardware controls: the percentage limit (including a native threshold), the charging gate, forced discharge, LED and idle-sleep overrides. Normal charging is allowed. Saved settings are retained; `resume` restores them. Pause lasts until `resume` or a service restart, which re-applies the saved policy.
- `cancel` restores the saved policy and ends temporary operations. It also suppresses automatic discharge until `resume`, a limit change or service restart, so cancellation does not immediately start another discharge.
- `adapter off` is bounded discharge to 20%, not indefinite power isolation. `adapter on` restores the saved policy. Both require adapter-gate support.

Changing or disabling a limit cancels temporary modes. `limit off` disables percentage control but preserves enabled care settings, including temperature protection and schedules. Use **Pause battery control** to release all controls temporarily. `batterybridge reset` permanently disables the limit, care settings and schedules, then restores normal charging. Native removal runs this reset first and then stops the service; stopping the service alone does not clear a saved limit. See [lifecycle](lifecycle.md).

Temporary modes are cleared by service restart. Discharge and calibration also stop on battery-sensor failure, lost external power, macOS sleep and orderly shutdown. On shutdown a saved native threshold is kept, and a temporary top-up or target is replaced by the saved limit. A failed hardware update cancels temporary modes during rollback. Abrupt termination can leave private hardware gates latched until recovery; see [hardware limitations](hardware.md).

## Temperature protection and automatic discharge

Open **Battery care** in the app, enable the required settings and choose **Save care settings and schedules**. New preferences default to disabled.

Temperature protection requires a battery temperature sensor and a charging gate. It stops charging at the selected temperature (25–60°C) and resumes 2°C below it. Missing battery percentage or temperature inhibits charging while protection is enabled. It takes priority over top-up, discharge and calibration. This is sampled every 30 seconds while awake, so it is not instantaneous and does not replace firmware protection.

Automatic discharge reduces charge above the saved limit when AC is connected and the adapter gate permits it. Saved limits below 20% are incompatible with this mode. It never runs during a temporary charge target or top-up. Turning automatic discharge off ends an automatic discharge already in progress; it does not cancel an explicitly requested manual discharge.

On macOS, **Prevent idle sleep** holds a process-scoped assertion during active charging/discharging on AC. It does not prevent lid-close or manual sleep. BatteryBridge does not alter global sleep settings. Software limits during closed-lid sleep are not guaranteed.

Failures of the MagSafe light or idle-sleep assertion are reported in service status
without releasing the charging limit. A successful settings update confirms that
preferences were saved; check service status for hardware diagnostics. Before sleep,
the service requests charging inhibition for saved limits and temporary targets below
100%, even if releasing the idle assertion fails. An explicit top-up or 100% temporary
target remains allowed to charge.

## Calibration

The explicitly requested calibration operation alone permits discharge below 20%:

```sh
batterybridge calibrate
batterybridge cancel
```

The cycle charges to full, discharges to **10%**, recharges to full, holds full charge for one hour, and restores the saved policy. If automatic discharge is enabled, it can then return to the saved limit. The GUI asks for agreement to the 10% discharge before starting or adding a calibration schedule.

Keep AC connected throughout. Temperature protection remains active, including during calibration. Sleep, power loss, sensor failure, service restart, a detected backwards clock change or a monotonic 24-hour deadline aborts the cycle. Calibration is an observable charge cycle, not a battery repair or a guarantee that firmware estimates improve. Its hardware behavior requires supervised device validation.

## Calendar schedules

Use **Battery care → Schedules**. Enter a future local date/time as `YYYY-MM-DD HH:MM`, choose an action and repetition, add it to the draft, then save care settings. Tasks can be enabled, disabled or removed. A scheduled limit uses the chosen upper percentage without an explicit lower threshold; the normal default band applies.

Supported repetitions: once, daily, weekdays, weekly, every two weeks, monthly and yearly. The service uses its computer's local timezone. The original day and wall-clock time are retained: January 31 becomes the last day of February and returns to March 31; February 29 returns in leap years. During spring-forward gaps, repeated tasks run at the first valid minute after the gap; autumn repeated times run at the first occurrence only. The initial GUI date must exist, and an ambiguous initial time selects its first occurrence. If the computer's timezone changes, the UTC anchor is interpreted in the new local timezone, which can shift the local execution time. Recreate the task if you need to retain its former wall-clock time.

The service checks due tasks on its normal 30-second interval and on wake/start. Missed repetitions coalesce into one run, followed by the first future occurrence. Tasks run in list order when several are due. The service does not wake a sleeping computer for a task.

Due entries are advanced and saved **before** execution. A crash in between can skip a task, but cannot replay a claimed calibration on restart. Hardware failures are logged with the task ID and remain visible in status until a successful settings operation or a subsequent successful scheduled batch. They are not retried automatically. A failed schedule save does not prevent the existing battery policy from being enforced. At most 16 tasks are accepted.

For scripts, export all care settings, edit the JSON and submit it:

```sh
batterybridge care > care.json
batterybridge care --file care.json
```

A schedule entry uses this structure inside the `schedules` array:

```json
{
  "id": 1,
  "enabled": true,
  "next_at": 1893506400,
  "anchor_at": null,
  "repeat": "monthly",
  "action": { "action": "limit", "stop": 80, "start": null }
}
```

`next_at` and optional `anchor_at` are Unix seconds. Let the service populate the anchor on first execution. Replace the example timestamp with your desired future time. Action tags are `limit`, `top_up`, `charge_to`, `discharge`, `hold`, `pause`, `resume` and `calibrate`. Percentages and hardware availability are validated before saving. State-dependent conditions, such as an existing saved limit for top-up, are checked when the task executes. Updates include a comparison with the current settings; stale updates fail and require refreshing the draft.

## Measurements and tray controls

**Battery measurements** shows available battery temperature, voltage, signed current/power, cycle count, health and estimated remaining time. Positive current/power means charging; negative means discharging. Rated adapter power and wall consumption are not inferred from battery readings. Windows currently exposes remaining runtime only; other measurements depend on OS and battery firmware.

Raw percentage uses the reported capacity ratio when available. It only affects display; control always uses the OS percentage. The charge trend stores up to 240 readings in GUI memory for the current session and is not persisted or uploaded. The tray tooltip refreshes percentage/state every 30 seconds, including when the window is hidden; the menu provides top-up, pause and restore actions.

On compatible MagSafe adapters, LED settings include system control, automatic charge-state indication, off, green, orange and slow/fast orange blink. Automatic uses orange while charging/discharging and green while idle. Pause and orderly shutdown restore system control. A readable SMC key does not guarantee the connected adapter obeys the override.

## macOS Shortcuts

Open **Automation and Shortcuts** in the app and copy a command. In Shortcuts, add **Run Shell Script**, paste the command and save the shortcut. For example:

```sh
"/Library/PrivilegedHelperTools/io.github.batterybridge.cli" limit 80
```

No Terminal session is needed to run the saved shortcut. The installed service and normal local authorization are required. These are CLI-based Shortcuts, not native App Intents actions. BatteryBridge does not install a URL handler that changes charging settings from web links.

## Reference projects and scope

The public feature descriptions of [AlDente](https://apphousekitchen.com/aldente-overview/features/), [battery](https://github.com/actuallymentor/battery) and [Battery Toolkit](https://github.com/mhaeuser/Battery-Toolkit) informed these controls. AlDente's current application is proprietary; its implementation and assets are not copied. The BSD-licensed Battery Toolkit SMC interface reference is credited in packaged notices.

BatteryBridge implements the charging controls above using its existing cross-platform service. It does not claim full product parity: hardware availability, closed-lid behavior, native App Intents, firmware-managed limits on every Mac, historical analytics outside the running session, email notifications and commercial appearance/licensing features differ or are unavailable. No unsupported feature is presented as a working hardware control.
