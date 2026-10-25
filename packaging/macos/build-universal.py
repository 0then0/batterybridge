#!/usr/bin/env python3
"""Build native Intel/Apple Silicon binaries and assemble the universal app."""
from pathlib import Path
import os
import plistlib
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app import BUILD, IDENTIFIER, LOCALES, REPO, VERSION, licenses

TARGETS = ('aarch64-apple-darwin', 'x86_64-apple-darwin')
BINARIES = ('batterybridge', 'batterybridge-service', 'batterybridge-gui')


def bundle(build):
    app = build / 'bundle/macos/BatteryBridge.app/Contents'
    if app.parent.exists():
        shutil.rmtree(app.parent)
    (app / 'MacOS').mkdir(parents=True, exist_ok=True)
    resources = app / 'Resources'
    resources.mkdir(exist_ok=True)
    shutil.copy2(build / 'batterybridge-gui', app / 'MacOS/batterybridge-gui')
    shutil.copy2(REPO / 'apps/gui/icons/icon.icns', resources / 'icon.icns')
    licenses(resources / 'licenses')
    info = {
        'CFBundleIdentifier': IDENTIFIER, 'CFBundleName': 'BatteryBridge',
        'CFBundleDisplayName': 'BatteryBridge', 'CFBundleExecutable': 'batterybridge-gui',
        'CFBundlePackageType': 'APPL', 'CFBundleIconFile': 'icon.icns',
        'CFBundleShortVersionString': VERSION, 'CFBundleVersion': BUILD,
        'CFBundleLocalizations': LOCALES, 'CFBundleDevelopmentRegion': 'en',
        'LSMinimumSystemVersion': '11.0', 'LSUIElement': True,
        'NSHighResolutionCapable': True, 'NSPrincipalClass': 'NSApplication',
    }
    (app / 'Info.plist').write_bytes(plistlib.dumps(info))
    agents = app / 'Library/LaunchAgents'
    agents.mkdir(parents=True)
    shutil.copy2(REPO / 'packaging/launchd/io.github.batterybridge.autostart.plist',
                 agents / 'io.github.batterybridge.autostart.plist')
    subprocess.run(['codesign', '--force', '--sign', '-', str(app.parent)], check=True)
    return app.parent


def main():
    environment = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='11.0')
    for target in TARGETS:
        subprocess.run(['cargo', 'build', '--release', '--locked', '--target', target,
                        '-p', 'batterybridge-cli', '-p', 'batterybridge-service', '-p', 'batterybridge-gui'],
                       cwd=REPO, env=environment, check=True)
    output = REPO / 'target/universal-apple-darwin/release'
    output.mkdir(parents=True, exist_ok=True)
    for binary in BINARIES:
        inputs = [str(REPO / 'target' / target / 'release' / binary) for target in TARGETS]
        subprocess.run(['lipo', '-create', *inputs, '-output', str(output / binary)], check=True)
        subprocess.run(['lipo', str(output / binary), '-verify_arch', 'arm64', 'x86_64'], check=True)
    print(bundle(output))


if __name__ == '__main__':
    main()
