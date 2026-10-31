# Service installation and recovery

[Documentation index](README.md)

The background service maintains one machine-wide charging policy and performs privileged hardware operations. CLI and GUI remain ordinary applications. Install the service only when you need charging control; status and compatibility reports also work without it.

The Linux and macOS commands below assume a source checkout with `cargo build --release --locked` completed. From an extracted platform archive, use `./batterybridge` and `./batterybridge-service` in place of the `target/release/` executable paths; the `packaging/` paths stay the same. Installation requires administrator privileges. For Windows, use an extracted platform archive or the source-build staging steps below. Keep installed executables and their parent directories protected from ordinary-user replacement.

## Native installers

For ordinary use, [install the platform package](install.md). It installs the GUI and starts its background component together. The manual instructions below are intended for source builds and developer archives, not for installation over a native package.

Debian and RPM packages use `/usr/bin` and `/usr/lib/systemd/system/batterybridge.service`, with IPC access for the existing `sudo` or `wheel` administrator group respectively. Windows setup uses a protected `Program Files/BatteryBridge/service` directory and a SYSTEM scheduled task. The universal macOS package uses the same root helper paths on Intel and Apple Silicon.

## Linux: systemd

Run from the repository root. Replace `YOUR_USER` with the account that will control charging. Skip `groupadd` if the group already exists.

```sh
sudo groupadd --system batterybridge
sudo install -m 0755 target/release/batterybridge target/release/batterybridge-service /usr/local/bin/
sudo install -m 0644 packaging/systemd/batterybridge.service /etc/systemd/system/
sudo usermod -aG batterybridge YOUR_USER
sudo systemctl daemon-reload
sudo systemctl enable --now batterybridge
```

Sign out and back in after changing group membership. Then verify access as your ordinary user:

```sh
batterybridge status
batterybridge capabilities
```

The unit creates `/run/batterybridge` with owner/group `root:batterybridge` and mode `0750`, and `/var/lib/batterybridge` with mode `0700`. The socket is `/run/batterybridge/control.sock`; configuration is `/var/lib/batterybridge/config.toml`.

Kernel controls in `/sys/class/power_supply` resolve into `/sys/devices`, so the unit grants the service write access to that subtree. The service rechecks native thresholds at its 30-second interval after resume.

### Remove the service

`systemctl stop` keeps a saved native threshold. Reset first, which restores normal charging and clears saved limits and schedules, and verify it on the device:

```sh
batterybridge reset
sudo systemctl disable --now batterybridge
sudo rm /etc/systemd/system/batterybridge.service
sudo systemctl daemon-reload
```

Configuration and executables remain available for reuse or separate removal.

## macOS: launchd

For the graphical developer preview, use the [macOS installer](install.md). It installs the app and service together. The commands below are for manual source installations; do not run them over a packaged installation.

macOS charge control uses privileged private SMC operations. Members of the local `admin` group can send charging commands over IPC. A source-built app bundle does not install the service; the native package does.

Run from the repository root:

```sh
sudo install -d -o root -g wheel -m 0755 /usr/local/bin
sudo install -m 0755 target/release/batterybridge target/release/batterybridge-service /usr/local/bin/
sudo install -d -o root -g admin -m 0750 /var/run/batterybridge
sudo install -d -o root -g wheel -m 0700 '/Library/Application Support/BatteryBridge'
sudo install -d -o root -g wheel -m 0755 /usr/local/libexec
sudo install -m 0755 packaging/launchd/batterybridge-launcher /usr/local/libexec/
sudo install -m 0644 packaging/launchd/io.github.batterybridge.plist /Library/LaunchDaemons/
sudo launchctl bootstrap system /Library/LaunchDaemons/io.github.batterybridge.plist
```

Run `batterybridge status` as your ordinary user to verify the connection. The launcher recreates the runtime directory after reboot before starting the service.

Socket: `/var/run/batterybridge/control.sock`. Configuration: `/Library/Application Support/BatteryBridge/config.toml`. Logs: `/var/log/batterybridge.log`; administrators should include this file in their log rotation policy.

The service registers IOKit sleep/wake notifications, releases adapter isolation before sleep and reconciles policy on wake. launchd restarts it after exit with throttling. Firmware can still reset private SMC gates; see [macOS compatibility](hardware.md#macos-intel-and-apple-silicon-smc).

### Remove the service

Reset, which restores normal charging and clears saved limits and schedules, and verify it before unloading:

```sh
batterybridge reset
sudo launchctl bootout system /Library/LaunchDaemons/io.github.batterybridge.plist
sudo rm /Library/LaunchDaemons/io.github.batterybridge.plist
```

The launcher, executables and configuration remain until removed separately. Use orderly service shutdown while adapter isolation is active.

### Remove the packaged macOS service

In version 0.1.1 and newer, use **Uninstall BatteryBridge** at the bottom of the app. After confirmation, macOS requests administrator authorization. The uninstaller restores normal charging through the installed service before unloading it, then removes the app, helper binaries, launchd registration, saved policy and service log. It restarts a stopped service to restore charging first. A restoration or unload failure stops removal and displays the original error. If the service cannot access its controller, use the recovery instructions below before retrying.

For version 0.1.0 or manual administration, first select **Turn off** in the app and verify that normal charging resumes. Quit BatteryBridge from its tray menu. Administrators can then remove the packaged service and app:

```sh
sudo launchctl bootout system /Library/LaunchDaemons/io.github.batterybridge.plist
sudo rm /Library/LaunchDaemons/io.github.batterybridge.plist
sudo rm /Library/PrivilegedHelperTools/io.github.batterybridge.service
sudo rm /Library/PrivilegedHelperTools/io.github.batterybridge.cli
sudo rm /Library/PrivilegedHelperTools/io.github.batterybridge.launcher
sudo rm -rf /Applications/BatteryBridge.app
sudo pkgutil --forget io.github.batterybridge.installer
```

Stopping the service releases its overrides but keeps a saved limit; the uninstaller therefore resets first. See [lifecycle](lifecycle.md).

The package stores its executables in root-owned `/Library/PrivilegedHelperTools`, independently of a user's source checkout or `/usr/local/bin`. Settings remain in `/Library/Application Support/BatteryBridge/config.toml` for reuse. Removing the GUI alone does not unload the service. The packaged installer preserves the same `admin` IPC access policy as manual installation.

## Windows: Task Scheduler

From an extracted Windows archive, open an elevated PowerShell and run:

```powershell
.\packaging\windows\install-task.ps1
```

For a source build, first prepare the archive layout from the repository root in an ordinary PowerShell:

```powershell
cargo build --release --locked
New-Item -ItemType Directory -Force target\windows-package | Out-Null
Copy-Item target\release\batterybridge-service.exe target\windows-package\
Copy-Item -Recurse -Force packaging target\windows-package\
```

Then open an elevated PowerShell at the repository root and run:

```powershell
.\target\windows-package\packaging\windows\install-task.ps1
```

The installer locates the service executable two directories above its script. It installs to `%ProgramFiles%\BatteryBridge`, protects code and state ACLs, and registers a SYSTEM task named `BatteryBridge`. The task starts at boot/logon, allows battery operation, prevents concurrent instances and retries failures. Existing tasks are stopped before executable replacement.

Run the CLI from your build/archive location, or from a directory on your PATH, as an ordinary user to verify access. The installer installs the service executable; it does not add the CLI to PATH.

Pipe: `\\.\pipe\BatteryBridge.v1`. Configuration: `%ProgramData%\BatteryBridge\config.toml`. The pipe name is stable; the request protocol version is validated separately. Interactive local users can send commands; network logons are denied.

The installer rejects reparse points and directories owned by other users. The service reasserts conservation mode at its 30-second interval after resume. Vendor-driver and firmware requirements are described in [Windows compatibility](hardware.md#windows-lenovo-wmi).

### Remove the service

Run `batterybridge reset` from your CLI location and verify normal charging, then execute the shipped `packaging/windows/uninstall-task.ps1` in an elevated PowerShell. The script unregisters the task; configuration and executables remain on disk. Task Scheduler termination is abrupt and runs no cleanup, so reset before stopping it.

## Troubleshooting

### Status works, but charging settings are unavailable

Check that the service is running and the user has IPC access. Read-only fallback shows local battery information but cannot retrieve the service policy. On a manual Linux installation, verify `batterybridge` group membership; for Debian and RPM packages the socket group is `sudo` and `wheel` respectively; on macOS, verify `admin` membership. On Windows, inspect the `BatteryBridge` scheduled task and the installed directories.

Useful service checks:

```sh
# Linux
systemctl status batterybridge
journalctl -u batterybridge
# macOS
sudo launchctl print system/io.github.batterybridge
tail -n 100 /var/log/batterybridge.log
```

In PowerShell, use `Get-ScheduledTask -TaskName BatteryBridge` and `Get-ScheduledTaskInfo -TaskName BatteryBridge`.

### Package removal is refused

Debian and RPM removal runs `batterybridge reset` and stops the service before deleting anything. If the service cannot start or reset fails, removal stops with the original error and the service stays installed. Fix the cause (see the service log) and retry. As a last resort for a service that cannot run, remove the package with your package manager's force option after confirming normal charging by other means; a native threshold may still be set in firmware.

### Configuration prevents startup

Stop the service, back up the configuration and correct the TOML as administrator. Invalid or future configuration versions are rejected rather than reset. Restart after editing. A valid policy that the current device cannot enforce remains visible as an enforcement error.

### An interrupted save left `config.toml.new`

Stop the service and inspect both files before making changes. The committed `config.toml` remains authoritative. Preserve any settings you need from the transaction file, remove or move it as administrator, then restart. Temporary top-up and discharge state is not restored from configuration.

### A Unix endpoint appears stale

Startup reclaims a stale socket under an OS file lock after confirming that no listener is active. Non-socket files and live listeners are preserved. Stop the service before inspecting runtime files; removing a lock file while a service is running can defeat instance ownership. Runtime directories are normally recreated after reboot.

For supervised development, `batterybridge-service --config PATH --socket ENDPOINT` overrides the default resources. This still uses real device discovery and can write hardware. Use the fake-controller tests for IPC or policy changes that do not require a device.
