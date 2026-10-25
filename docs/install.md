# Install BatteryBridge

[Documentation index](README.md)

Version 0.3.0 uses egui/eframe and preserves the installation paths and saved
charging policy. The installers include the GUI, CLI and background service.
Upgrade all three together: the IPC protocol is incompatible with 0.2.0.
Back up configuration before upgrading; the migrated version 2 format cannot be
read by older binaries. See [configuration recovery](configuration.md#persistence-and-recovery)
before downgrading.

When upgrading from 0.2.0 to the native GUI, select your preferred language again
if it differs from the system language. The old WebView preference is not imported;
the service keeps its saved charging settings.

## macOS

The graphical installer supports Intel and Apple Silicon Macs running macOS 11 or newer. It installs both the desktop app and the charging service. No Terminal commands are required.

1. Download **BatteryBridge-macos-universal.pkg** from a [GitHub release](https://github.com/0then0/BatteryBridge/releases).
2. Open it in Finder, follow Installer and enter an administrator password when prompted.
3. Open **Applications → BatteryBridge** from a local administrator account.
4. Choose a stop percentage and **Apply limit**. The saved limit should appear above the controls. If firmware refuses the operation, the app displays an error.

The package is unsigned and not notarized. If macOS blocks it, review its origin and open **System Settings → Privacy & Security** after the blocked attempt. Choose **Open Anyway** if you trust this release, then follow macOS prompts. Do not disable Gatekeeper. Organization-managed Macs may prohibit unsigned installers; Developer ID signing and notarization are still needed to remove this distribution limitation.

A new installation starts with the charge limit off. The service runs after the app closes and starts at boot. Updating closes an existing BatteryBridge app, restarts the service and preserves its settings. Open the installed copy in Applications after an update, rather than a previous development binary.

The GUI should no longer show **Read-only mode** for a local administrator. Standard accounts retain read-only access. Installing the service does not establish firmware compatibility: SMC charging operations are experimental. Intel BCLM thresholds require a readable, compatible key; unsupported models remain in monitoring mode.

To restore normal charging without uninstalling, select **Turn off** in the app.

In version 0.1.1 and newer, choose **Uninstall BatteryBridge** at the bottom of the app, confirm removal and approve the macOS administrator dialog. This restores normal charging, unloads the service and removes the installed app, helpers, saved policy and service log. Removal stops if charging cannot be restored or the service cannot be unloaded. Your per-user language preference is retained.

Moving the app to Trash alone leaves the background service installed. Version 0.1.0 does not include the graphical uninstaller; use the [administrative removal instructions](service.md#remove-the-packaged-macos-service), or install a newer build first.

## Windows

Download **BatteryBridge-windows-x86_64-setup.exe**, open it in Explorer and approve UAC. The installer places the app in **Program Files → BatteryBridge**, creates application shortcuts, installs a protected background task running as SYSTEM and starts it. You do not need an elevated PowerShell window.

The task starts at boot and logon and keeps running after the GUI closes. Existing charging settings are preserved during upgrades. Install in the default Program Files location; custom locations are rejected so that SYSTEM never runs user-writable code.

If you previously installed a GUI-only bundle from a developer archive, remove that GUI-only app before installing the complete setup. Manual installations may leave separate binaries; the native uninstaller only owns files installed by its package.

Remove BatteryBridge through **Settings → Apps → Installed apps**. The uninstaller restores normal charging when a saved limit is enabled, stops and unregisters the task, and removes its settings and binaries. A failed restoration stops removal. The installer is currently unsigned; review the download origin and Windows security prompts. Organization policies may prevent unsigned installation.

## Linux

Use the `.deb` package on Ubuntu 24.04+ or Debian 13+, or `.rpm` on Fedora 41+ with compatible OpenGL/EGL and X11/Wayland libraries. Open it in your desktop's package manager and approve authentication. Package dependencies must be available from the distribution's repositories. The source packages require systemd and native graphics libraries; minimal/headless distributions without a graphical package manager may still require command-line administration.

The package installs the GUI, CLI, daemon and systemd unit, enables the service at boot and starts it immediately. For package installations, existing members of `sudo` (Debian/Ubuntu) or `wheel` (RPM) can change charging settings without running the GUI as root. Other users can inspect battery status through the read-only fallback. The package does not add users to administrator groups or grant world-writable access. Manual source installation retains its separate `batterybridge` group policy.

Open **BatteryBridge** from your application menu. Remove it through the package manager: the service is stopped with an orderly hardware release, disabled and removed with the executables. Saved configuration in `/var/lib/batterybridge` is retained for reinstallation; it does not run background functions. Updates preserve this configuration and restart the service. Do not install both package formats on the same system. A manually installed unit at `/etc/systemd/system/batterybridge.service` overrides the packaged unit; the installer refuses this combination. Stop and remove that manual unit first using the service guide, then install the native package. Saved configuration is reused.

## Launch at login (next release)

This setting is available in source builds after v0.3.0. The v0.3.0 downloads do
not include it yet.

Enable **Launch at login** below the language selector to start BatteryBridge
in the tray after signing in, without opening a window. It is off by default
and applies to the current user. Open the window through the tray menu.
If a tray cannot be created, the app opens a window instead.

The charging service starts separately at system boot. This setting neither
starts nor stops that service and remains available when charging settings
are read-only. Quit exits the current GUI session; turn the setting off to
prevent it from starting at the next sign-in.

- **macOS:** requires macOS 13+ and the packaged `.app`, opened from Applications.
  An unbundled executable or macOS 11/12 cannot register this login item.
  If the app reports that approval is required, allow BatteryBridge in
  **System Settings → General → Login Items**. The graphical uninstaller
  unregisters the current user's GUI login item before removing the service.
- **Windows:** registers the installed executable for the current account.
  The packaged uninstaller removes matching entries in loaded user profiles.
- **Linux:** registers an XDG desktop autostart entry for the current account.
  Keep the executable at its registered path. Disable the setting before
  package removal to remove this per-user entry; otherwise the entry remains
  but cannot run the removed executable.

## Developer archives

The archives contain standalone binaries and the platform's desktop bundles and packaging resources. Running a GUI extracted from an archive does not install its service. Use the native installer for ordinary installation, or follow [manual service setup](service.md).

## Verify a download

The release includes `SHA256SUMS.txt` for all assets. These checksums detect download corruption; they do not replace Developer ID signing or notarization. Obtain the installer only from this repository's release page.
