"""Exercise installer payload and lifecycle with disposable files and fake OS commands."""

import importlib.util
import plistlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from xml.etree import ElementTree as ET
from unittest.mock import patch

HERE = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("build_pkg", HERE / "build-pkg.py")
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)
spec = importlib.util.spec_from_file_location("build_universal", HERE / "build-universal.py")
universal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(universal)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.trace = self.root / "trace"

    def command(self, name, body):
        path = self.root / name
        path.write_text(f'#!/bin/sh\necho "{name} $*" >> "{self.trace}"\n{body}\n')
        path.chmod(0o755)
        return str(path)

    def run_script(self, name, *, mode="755", owner="0", bootstrap=0,
                   config=0, volume="/", link=False, bootout=0):
        # All commands that can touch the host are replaced. Never run installer
        # scripts directly in a test: they are intended to run as root.
        text = (HERE / "scripts" / name).read_text()
        replacements = {
            "/bin/launchctl": self.command("launchctl", f'case "$1" in bootstrap) exit {bootstrap};; bootout) exit {bootout};; *) exit 0;; esac'),
            "/usr/bin/pkill": self.command("pkill", "exit 1"),
            "/usr/bin/install": self.command("install", "exit 0"),
            "/bin/sleep": self.command("sleep", "exit 0"),
            "/usr/bin/stat": self.command("stat", f'case "$2" in %u) echo {owner};; *) echo {mode};; esac'),
            "/Library/PrivilegedHelperTools/io.github.batterybridge.cli": self.command("client", f'exit {config}'),
        }
        # stat is invoked as: stat -f FORMAT PATH.
        helpers = self.root / "helpers"
        if link:
            target = self.root / "untrusted"
            target.mkdir()
            helpers.symlink_to(target)
        else:
            helpers.mkdir()
        for source, target in replacements.items():
            text = text.replace(source, target)
        for index, source in enumerate(("/Library/PrivilegedHelperTools", "/Library/LaunchDaemons",
                                        "/Library/Application Support/BatteryBridge",
                                        "/var/run/batterybridge", "/Applications/BatteryBridge.app")):
            text = text.replace(source, str(helpers if index == 0 else self.root / f"path{index}"))
        script = self.root / "script"
        script.write_text(text)
        return subprocess.run(["/bin/sh", str(script), "package", "/", volume], capture_output=True, text=True)

    def test_payload_contains_app_and_root_helper_launchd_paths(self):
        build = self.root / "build"
        app = build / "bundle/macos/BatteryBridge.app/Contents"
        app.mkdir(parents=True)
        (app / "Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": "io.github.batterybridge"}))
        for name in ("batterybridge", "batterybridge-service"):
            (build / name).write_text("fixture")
        payload = self.root / "payload"
        builder.stage_payload(payload, build)
        daemon = plistlib.loads((payload / "Library/LaunchDaemons/io.github.batterybridge.plist").read_bytes())
        self.assertEqual(daemon["UserName"], "root")
        launcher = daemon["ProgramArguments"][0]
        self.assertTrue((payload / launcher.lstrip("/")).is_file())
        self.assertIn("/Library/PrivilegedHelperTools/io.github.batterybridge.service",
                      (payload / launcher.lstrip("/")).read_text())
        self.assertEqual((payload / builder.HELPERS / builder.SERVICE).stat().st_mode & 0o777, 0o755)
        self.assertTrue((payload / "Applications/BatteryBridge.app/Contents/Info.plist").is_file())
        self.assertFalse((payload / "Library/Application Support/BatteryBridge/config.toml").exists())

    def test_hidden_login_agent_is_inside_bundle_before_signing(self):
        build = self.root / "build"
        build.mkdir()
        (build / "batterybridge-gui").write_text("fixture")
        def sign(command, **kwargs):
            self.assertEqual(command[:4], ["codesign", "--force", "--sign", "-"])
            app = Path(command[4]) / "Contents"
            agent = plistlib.loads((app / "Library/LaunchAgents/io.github.batterybridge.autostart.plist").read_bytes())
            self.assertEqual(agent["Label"], "io.github.batterybridge.autostart")
            self.assertEqual(agent["ProgramArguments"], ["/usr/bin/open", "-g", "-b", "io.github.batterybridge", "--args", "--start-hidden"])
            self.assertTrue(agent["RunAtLoad"])
            self.assertNotIn("KeepAlive", agent)
            self.assertEqual(agent["LimitLoadToSessionType"], "Aqua")
        with patch.object(universal.subprocess, "run", side_effect=sign) as signer:
            app = universal.bundle(build)
        signer.assert_called_once()
        payload = self.root / "payload"
        for name in ("batterybridge", "batterybridge-service"):
            (build / name).write_text("fixture")
        builder.stage_payload(payload, build)
        relative = Path("Contents/Library/LaunchAgents/io.github.batterybridge.autostart.plist")
        self.assertEqual((payload / "Applications/BatteryBridge.app" / relative).read_bytes(), (app / relative).read_bytes())

    def test_distribution_targets_current_system_and_both_mac_architectures(self):
        root = ET.fromstring(builder.distribution("0.1.0.1"))
        self.assertEqual(root.find("options").get("hostArchitectures"), "arm64,x86_64")
        self.assertEqual(root.find("domains").get("enable_anywhere"), "false")
        self.assertEqual(root.find("domains").get("enable_currentUserHome"), "false")

    def test_installer_resources_cover_all_app_languages(self):
        resources = self.root / "resources"
        builder.stage_resources(resources)
        catalogs = sorted((builder.REPO / "apps/gui/locales").glob("*.json"))
        self.assertEqual(len(catalogs), 7)
        for catalog in catalogs:
            for name in ("Welcome.html", "Conclusion.html"):
                page = resources / f"{catalog.stem}.lproj" / name
                content = page.read_text(encoding="utf-8")
                self.assertIn(f'lang="{catalog.stem}"', content)
                self.assertIn("BatteryBridge", content)
                self.assertNotIn("{percent}", content)

    def test_preinstall_stops_old_service_before_gui(self):
        result = self.run_script("preinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        trace = self.trace.read_text()
        self.assertLess(trace.index("launchctl bootout"), trace.index("pkill"))

    def test_preinstall_rejects_symlink_without_stopping_service(self):
        result = self.run_script("preinstall", link=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("symbolic link", result.stderr)
        self.assertFalse(self.trace.exists())

    def test_preinstall_rejects_user_writable_helper_directory(self):
        result = self.run_script("preinstall", mode="775")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("launchctl", self.trace.read_text())

    def test_preinstall_rejects_non_root_owner(self):
        result = self.run_script("preinstall", owner="501")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("launchctl", self.trace.read_text())

    def test_preinstall_rejects_other_volume(self):
        result = self.run_script("preinstall", volume="/Volumes/Other")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.trace.exists())

    def test_preinstall_aborts_when_existing_service_cannot_stop(self):
        result = self.run_script("preinstall", bootout=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("pkill", self.trace.read_text())

    def test_postinstall_waits_for_live_config_ipc(self):
        result = self.run_script("postinstall")
        self.assertEqual(result.returncode, 0, result.stderr)
        trace = self.trace.read_text()
        self.assertLess(trace.index("launchctl bootstrap"), trace.index("client config"))

    def test_postinstall_fails_if_service_never_answers(self):
        result = self.run_script("postinstall", config=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.trace.read_text().count("client config"), 10)

    def test_postinstall_propagates_bootstrap_failure(self):
        result = self.run_script("postinstall", bootstrap=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("client config", self.trace.read_text())

    @unittest.skipUnless(sys.platform == "darwin" and
                         (builder.REPO / "target/universal-apple-darwin/release/bundle/macos/BatteryBridge.app").exists(),
                         "requires a macOS release build")
    def test_real_package_build_and_expand_without_installation(self):
        output = self.root / "BatteryBridge.pkg"
        subprocess.run([sys.executable, str(HERE / "build-pkg.py"), "--output", str(output), "--build-dir", str(builder.REPO / "target/universal-apple-darwin/release")],
                       check=True, capture_output=True, text=True)
        expanded = self.root / "expanded"
        subprocess.run(["pkgutil", "--expand-full", str(output), str(expanded)], check=True)
        component = expanded / "component.pkg"
        info = ET.parse(component / "PackageInfo").getroot()
        self.assertEqual(info.get("auth"), "root")
        self.assertEqual(info.get("version"), builder.RECEIPT)
        self.assertEqual(info.get("relocatable"), "false")
        self.assertEqual(info.find("bundle").get("CFBundleVersion"), universal.BUILD)
        self.assertIsNotNone(info.find("scripts/preinstall"))
        self.assertIsNotNone(info.find("scripts/postinstall"))
        self.assertEqual(len(info.find("relocate")), 0)
        app_info = plistlib.loads((component / "Payload/Applications/BatteryBridge.app/Contents/Info.plist").read_bytes())
        self.assertEqual(app_info["CFBundleShortVersionString"], universal.VERSION)
        self.assertEqual(set(app_info["CFBundleLocalizations"]), {"en", "ru", "es", "de", "fr", "ja", "pt-BR"})
        for locale in app_info["CFBundleLocalizations"]:
            self.assertTrue((expanded / "Resources" / f"{locale}.lproj" / "Welcome.html").is_file())
        bom = subprocess.check_output(["lsbom", str(component / "Bom")], text=True)
        self.assertIn("./Library/PrivilegedHelperTools/io.github.batterybridge.service\t100755\t0/0", bom)
        self.assertIn("./Library/LaunchDaemons/io.github.batterybridge.plist\t100644\t0/0", bom)


if __name__ == "__main__":
    unittest.main()
