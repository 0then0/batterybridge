"""Test package lifecycle hooks with fake systemd and IPC, never the host daemon."""
import importlib.util
import sys
from pathlib import Path
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parents[1] / 'linux'
spec = importlib.util.spec_from_file_location('linux_builder', HERE / 'build.py')
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class LinuxPackageTests(unittest.TestCase):
    def run_hook(self, package, hook, argument, *, cli=0, stop=0, status=0, manual=False, active=True, reset=0):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            trace = root / 'trace'
            def command(name, body):
                path = root / name
                path.write_text(f'#!/bin/sh\necho "{name} $*" >> "{trace}"\n{body}\n')
                path.chmod(0o755)
                return str(path)
            if manual:
                (root / 'manual.service').touch()
            replacements = {
                '/etc/systemd/system/batterybridge.service': str(root / 'manual.service'),
                'systemctl': command('systemctl', f'case "$1" in stop) exit {stop};; show) echo {status};; is-active) exit {0 if active else 3};; esac'),
                '/usr/bin/batterybridge': command('client', f'case "$1" in reset) exit {reset};; *) exit {cli};; esac'),
                'sleep 1': command('sleep', 'exit 0'),
                'getent': command('getent', 'exit 0'),
                '/run/systemd/system': str(root),
            }
            text = (HERE / f'{package}-{hook}.sh').read_text()
            for source, target in replacements.items():
                text = text.replace(source, target)
            script = root / 'hook'
            script.write_text(text)
            result = subprocess.run(['/bin/sh', str(script), argument], capture_output=True, text=True)
            return result, trace.read_text() if trace.exists() else ''

    def test_manual_service_override_is_rejected_before_installation(self):
        for package in ('deb', 'rpm'):
            result, _ = self.run_hook(package, 'preinstall', 'install', manual=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('manually installed', result.stderr)
            result, _ = self.run_hook(package, 'preinstall', 'install')
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_install_starts_and_checks_the_daemon(self):
        for package in ('deb', 'rpm'):
            result, trace = self.run_hook(package, 'postinstall', 'configure')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertLess(trace.index('daemon-reload'), trace.index('restart'))
            self.assertLess(trace.index('restart'), trace.index('client config'))
            result, trace = self.run_hook(package, 'postinstall', 'configure', cli=1)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(trace.count('client config'), 10)

    def test_remove_resets_then_stops_and_upgrade_keeps_registration_and_saved_limit(self):
        for package, remove, upgrade in [('deb', 'remove', 'upgrade'), ('rpm', '0', '1')]:
            result, trace = self.run_hook(package, 'preremove', remove)
            self.assertEqual(result.returncode, 0, result.stderr)
            # Stopping no longer clears a saved native threshold: reset must come first.
            self.assertLess(trace.index('client reset'), trace.index('stop'))
            self.assertLess(trace.index('stop'), trace.index('disable'))
            self.assertNotIn('start', trace.replace('restart', ''))
            result, trace = self.run_hook(package, 'preremove', upgrade)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(trace, '')

    def test_remove_starts_a_stopped_service_so_reset_can_reach_the_hardware(self):
        for package, remove in [('deb', 'remove'), ('rpm', '0')]:
            result, trace = self.run_hook(package, 'preremove', remove, active=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertLess(trace.index('systemctl start'), trace.index('client config'))
            self.assertLess(trace.index('client config'), trace.index('client reset'))
            self.assertLess(trace.index('client reset'), trace.index('systemctl stop'))
            result, trace = self.run_hook(package, 'preremove', remove, active=False, cli=1)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('client reset', trace)
            self.assertNotIn('disable', trace)

    def test_failed_reset_or_unclean_stop_blocks_removal_without_unregistering(self):
        for package, remove in [('deb', 'remove'), ('rpm', '0')]:
            result, trace = self.run_hook(package, 'preremove', remove, reset=1)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Normal charging could not be restored', result.stderr)
            self.assertNotIn('systemctl stop', trace)
            self.assertNotIn('disable', trace)
            for failure in ({'stop': 1}, {'status': 1}):
                result, trace = self.run_hook(package, 'preremove', remove, **failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('disable', trace)

    def test_package_contents_use_system_paths_and_administrator_groups(self):
        for package, group in [('deb', 'sudo'), ('rpm', 'wheel')]:
            with tempfile.TemporaryDirectory() as temporary:
                build = Path(temporary) / 'build'
                build.mkdir()
                for name in ('batterybridge', 'batterybridge-service', 'batterybridge-gui'):
                    (build / name).write_text('fixture')
                payload = Path(temporary) / 'payload'
                builder.stage(payload, build, package)
                daemon = payload / 'usr/bin/batterybridge-service'
                self.assertEqual(daemon.stat().st_mode & 0o777, 0o755)
                self.assertTrue((payload / 'usr/share/applications/BatteryBridge.desktop').is_file())
                self.assertTrue((payload / 'usr/share/licenses/batterybridge/Inter-OFL.txt').is_file())
                self.assertEqual((payload / 'usr/lib/systemd/system/batterybridge.service').read_text(), (HERE / f'{package}.service').read_text())
            dependencies = builder.DEB_DEPENDS if package == 'deb' else builder.RPM_DEPENDS
            self.assertNotIn('webkit', ' '.join(dependencies))
            self.assertNotIn('libgtk', ' '.join(dependencies))
            unit = (HERE / f'{package}.service').read_text()
            self.assertIn(f'Group={group}', unit)
            self.assertIn('ExecStart=/usr/bin/batterybridge-service', unit)
            self.assertIn('RuntimeDirectoryMode=0750', unit)
            self.assertIn('ProtectSystem=strict', unit)
