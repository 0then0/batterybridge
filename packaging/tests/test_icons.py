"""The committed icon files must match their generator and the SVG source."""
import importlib.util
import re
import struct
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('render_icons', REPO / 'assets/render-icons.py')
icons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icons)


def dimensions(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    return struct.unpack('>II', data[16:24])


class IconTests(unittest.TestCase):
    def test_committed_pngs_are_reproduced_by_the_generator(self):
        for size, name in ((32, '32x32.png'), (128, '128x128.png')):
            committed = (REPO / 'apps/gui/icons' / name).read_bytes()
            self.assertEqual(dimensions(committed), (size, size))
            self.assertEqual(icons.png(size), committed, name)

    def test_generated_icon_is_transparent_outside_the_tile_and_opaque_inside(self):
        self.assertIsNone(icons.sample(1, 1))
        self.assertEqual(icons.sample(64, 20), icons.BG)
        self.assertEqual(icons.sample(40, 64), icons.GREEN)
        self.assertEqual(icons.sample(84, 64), icons.CREAM)

    def test_windows_icon_has_every_required_size_as_png(self):
        data = (REPO / 'apps/gui/icons/icon.ico').read_bytes()
        count = struct.unpack('<H', data[4:6])[0]
        found = []
        for index in range(count):
            width, _, _, _, _, _, length, offset = struct.unpack('<BBBBHHII', data[6 + 16 * index:22 + 16 * index])
            size = width or 256
            self.assertEqual(dimensions(data[offset:offset + length]), (size, size))
            found.append(size)
        self.assertEqual(found, list(icons.ICO_SIZES))

    def test_generator_colours_match_the_svg_source(self):
        svg = (REPO / 'assets/icon.svg').read_text().lower()
        for color in (icons.BG, icons.CREAM, icons.GREEN):
            self.assertIn('#%02x%02x%02x' % color, svg)

    def test_macos_icon_contains_the_retina_sizes(self):
        data = (REPO / 'apps/gui/icons/icon.icns').read_bytes()
        self.assertEqual(data[:4], b'icns')
        kinds, position = set(), 8
        while position < len(data):
            kinds.add(data[position:position + 4])
            position += struct.unpack('>I', data[position + 4:position + 8])[0]
        self.assertTrue({b'ic07', b'ic08', b'ic09', b'ic10'} <= kinds, kinds)


if __name__ == '__main__':
    unittest.main()
