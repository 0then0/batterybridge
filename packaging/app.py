"""Shared application metadata for native package builders (Python 3.9+)."""
from pathlib import Path
import re
import shutil

REPO = Path(__file__).resolve().parent.parent
NAME = 'BatteryBridge'
IDENTIFIER = 'io.github.batterybridge'
VERSION = re.search(r'^version = "([^"]+)"', (REPO / 'Cargo.toml').read_text(), re.M)[1]
BUILD = '6'
RECEIPT = f'{VERSION}.{BUILD}'
LOCALES = sorted(path.stem for path in (REPO / 'apps/gui/locales').glob('*.json'))


def licenses(destination):
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(REPO / 'LICENSE', destination / 'BatteryBridge-Apache-2.0.txt')
    for source in (REPO / 'assets/licenses').glob('*.txt'):
        shutil.copy2(source, destination / source.name)
