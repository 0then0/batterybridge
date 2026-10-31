# Lifecycle: stop, sleep, pause, reset and removal

[Documentation index](README.md)

BatteryBridge drives three kinds of hardware state, and they behave differently
when the service is not running:

- **Native threshold.** A charge limit stored by the kernel driver or firmware
  (Linux `charge_control_end_threshold`/`start_threshold`, the Intel Mac `BCLM`
  key, Lenovo conservation mode). The hardware enforces it without the service.
  Whether the value survives while the computer is **off** is a property of the
  device and is not established by this project, so it is treated as **unknown**.
- **Daemon-held gate.** A binary charging gate (Apple SMC `CHTE`, `CH0B`/`CH0C`)
  that only means something while the service watches the battery and reopens it.
  A closed gate stops charging completely, so it is never left behind.
- **Overrides.** Forced discharge (adapter gate), the MagSafe light override and
  the idle-sleep assertion. These exist only while the service maintains them.

Backends combine these differently, so the service never uses one "release"
for everything. Each hardware release names its scope:

- `Overrides`: forced discharge, charging gate (reopened), MagSafe light, idle-sleep
  assertion. Native thresholds are untouched.
- `Everything`: the above plus every native threshold, returning to normal charging.

## Mechanisms per backend

- **Linux sysfs:** native threshold only (end threshold, optionally start). No gates
  or overrides. The kernel ABI does not document persistence across power-off.
- **Windows Lenovo WMI:** native conservation mode only (fixed 80% or off). The
  getter is unreliable, so there is no readback. Persistence is unverified.
- **macOS Apple Silicon:** daemon-held gate plus overrides. No native threshold.
- **macOS Intel with `BCLM` only:** native threshold only; persistence unverified.
- **macOS Intel with `BCLM` and a gate:** both. Stopping reopens the gate and keeps the
  saved `BCLM` value.

What is confirmed by code and documentation: which calls each backend makes, and that the
service rewrites the saved policy on every start and every 30 seconds. What requires a
physical test on each model: whether a threshold, a gate or a reset value is kept or lost
across power-off, firmware updates and SMC resets. No such report is recorded for any
device; see [device validation](hardware.md#device-validation).

## What each action does

**Stop or restart of the service** (`systemctl stop`/`restart`, `launchctl bootout`,
package upgrade, SIGTERM, and also OS shutdown or reboot, which the service cannot tell
apart from them reliably):

1. New commands, due schedules and reconciliation are refused. Requests already queued
   receive an explicit "service is shutting down" error after their request is read.
   This drain is limited to five seconds.
2. Temporary modes end: top-up, charge target, hold, forced discharge, calibration.
3. Daemon-held overrides are released.
4. The hardware is left holding the **saved** native threshold. A temporary top-up or
   target that had raised it is replaced by the saved limit; a saved limit is not
   cleared. If no limit is saved, normal charging is asserted.
5. The saved configuration is not modified.

Steps 3 and 4 are independent: one failing does not skip the other. The process exits
with a nonzero status and reports every failed step if cleanup was incomplete.

A stop never removes a saved limit, so a threshold-only backend (Linux, Lenovo, Intel
`BCLM`) keeps limiting if the hardware retains it. On a gate backend the limit cannot be
enforced without the service, so charging is simply allowed until the service starts again.

**Sleep (macOS notifications).** The saved policy and native thresholds are kept.
Forced discharge and the idle assertion end, and a gate is closed for limits below
100% because the service cannot enforce while asleep. Preparation steps are independent
and a failure remains visible in status. After wake the service rewrites the hardware from the policy;
until that finishes, status reports that the hardware state is not yet verified. Linux
and Windows have no sleep notification; the next 30-second reconciliation after resume
rewrites the threshold.

**Pause** (`batterybridge pause`, tray, GUI). Releases `Everything`: the percentage limit
(including a native threshold), the charging gate, forced discharge, the MagSafe light
and idle-sleep override. Normal charging is allowed. The saved limit, care settings
and schedules are kept, and `resume` re-applies them. Pause is runtime-only: restarting
the service ends it and re-applies the saved policy. Stopping the service while paused
leaves the hardware released.

**Reset** (`batterybridge reset`). Returns `Everything` to the hardware, trying every step, and only then
saves an empty policy (no limit, care settings or schedules). If any step fails, the
command fails, the saved policy stays as it was, and the error names each failed step.
Because the empty policy is saved with the reset, no schedule can apply a limit again.

**Removal.** Uninstallers run `reset` and then stop the service; they do not depend on stop
clearing the hardware.

- macOS package uninstall: starts the service if needed, `reset`, unload, then removal.
- Debian/RPM removal: starts the service if needed, `reset`, `systemctl stop`, a check
  that it exited cleanly, then disable. A failure at any step stops removal and keeps the
  service installed. Upgrades only restart the service and keep the saved limit.
- Windows uninstall: starts the task if needed, `reset`, stop, unregister.
- Manual installations: run `batterybridge reset` yourself before stopping the service.

Saved settings in `/var/lib/batterybridge` (Linux) are retained by package removal and
reused on reinstall. Reset empties the policy before that, so a reinstall starts with the
limit off.

**Enforcement errors.** If a reconciliation fails (an unreadable battery percentage, a
missing battery, a rejected hardware write), the service cancels temporary modes, releases
daemon-held overrides so no gate stays latched, and writes the saved native threshold again.
It does not clear a saved native limit, because that limit does not depend on the failed
step. The error stays visible in status until a later reconciliation succeeds. Only pause
and reset release native thresholds.

## What cannot be promised

- **Forced termination.** After SIGKILL, a crash, power loss or a forced OS shutdown
  nothing runs. A native threshold stays wherever it was. A gate or override can stay
  latched until the service starts again, or until the firmware clears it. Recovery:
  start the service (it rewrites the saved policy), or run `batterybridge reset`.
  If the service cannot start, see [service recovery](service.md#troubleshooting).
- **Windows.** Task Scheduler ends the service without notice, so no cleanup runs on
  stop. The Lenovo backend has no gate, so nothing is left latched.
- **A powered-off computer.** Starting the service again and observing a threshold does not
  show that it was enforced while the computer was off. Only a supervised test can.
- **No limit saved.** The service asserts normal charging (threshold 100) while running.
  It overrides thresholds set by other tools, so run one charge manager at a time.

## Restart recovery

The service loads the saved TOML policy, discovers the device and opens the
controller, then reconciles. Reconciliation writes the saved value; sysfs writes are
skipped when the readback already matches, so a hardware that kept the threshold sees no
intermediate write. A saved value is **configuration**; `batterybridge status` reports it as
such and shows the kernel's own readback separately where the driver exposes it.
Temporary modes and pause are never restored. A read or write error is shown in
`status` and the GUI until a later reconciliation succeeds. The first reconciliation runs
before queued requests are answered.
