"""The frame identity harness's PNG comparison: pixels decide, not how the encoder filtered its rows."""
import math
import re
import struct
import tempfile
import unittest
import zlib
from pathlib import Path
from types import SimpleNamespace

from tools import test_frame_identity_parity as identity


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    return a if pa <= pb and pa <= pc else b if pb <= pc else c


def encode(path, width, height, depth, rows, kind, colour=2):
    """An RGB PNG of `rows` (bytes), every row filtered with PNG filter `kind` (0 None, 1 Sub, 2 Up, 3 Average, 4 Paeth)."""
    bpp = {0: 1, 2: 3, 4: 2, 6: 4}[colour] * depth // 8
    stream, previous = b'', bytes(width * bpp)
    for row in rows:
        out = bytearray()
        for i, value in enumerate(row):
            a = row[i - bpp] if i >= bpp else 0
            b = previous[i]
            c = previous[i - bpp] if i >= bpp else 0
            predicted = (0, a, b, (a + b) >> 1, paeth(a, b, c))[kind]
            out.append((value - predicted) & 255)
        stream += bytes([kind]) + out
        previous = row

    def chunk(name, body):
        return struct.pack('>I', len(body)) + name + body + struct.pack('>I', zlib.crc32(name + body))
    header = struct.pack('>IIBBBBB', width, height, depth, colour, 0, 0, 0)
    Path(path).write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(stream))
                           + chunk(b'IEND', b''))


def picture(width, height, depth):
    """A gradient with every byte value somewhere, so each filter's arithmetic wraps."""
    bpp = 3 * depth // 8
    return [bytes((x * 37 + y * 101 + i * 59) & 255 for x in range(width) for i in range(bpp)) for y in range(height)]


class CompareTests(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())

    def test_the_same_pixels_filtered_differently_are_identical(self):
        for depth in (8, 16):
            rows = picture(7, 5, depth)
            encode(self.dir / 'a.png', 7, 5, depth, rows, 0)
            for kind in (1, 2, 3, 4):
                encode(self.dir / 'b.png', 7, 5, depth, rows, kind)
                self.assertIsNone(identity.compare(self.dir / 'a.png', self.dir / 'b.png', self.dir / 'd.png'),
                                  f'{depth}-bit, filter {kind}')
            self.assertFalse((self.dir / 'd.png').exists())

    def test_one_changed_pixel_is_counted_and_marked_red(self):
        rows = picture(6, 4, 8)
        changed = [bytearray(r) for r in rows]
        changed[2][3 * 4 + 1] ^= 4  # 🤖 Agent: pixel (4, 2), green, by 4 steps
        encode(self.dir / 'a.png', 6, 4, 8, rows, 4)
        encode(self.dir / 'b.png', 6, 4, 8, [bytes(r) for r in changed], 4)
        problem = identity.compare(self.dir / 'a.png', self.dir / 'b.png', self.dir / 'd.png')
        self.assertEqual(problem, '1 of 24 pixels (4.167%), largest step 4')
        width, height, bpp, raw, _ = identity.read_png(self.dir / 'd.png')
        diff = identity.unfilter(width, height, bpp, raw)
        reds = [(x, y) for y, row in enumerate(diff) for x in range(width)
                if row[3 * x] == 255 and row[3 * x + 1] < 255]
        self.assertEqual(reds, [(4, 2)])

    def test_a_16_bit_step_counts_in_16_bit_units(self):
        rows = picture(3, 2, 16)
        changed = [bytearray(r) for r in rows]
        low = changed[1][5]
        changed[1][5] = (low + 1) & 255  # 🤖 Agent: pixel (0, 1), green's low byte
        carry = 255 if low == 255 else 1
        encode(self.dir / 'a.png', 3, 2, 16, rows, 0)
        encode(self.dir / 'b.png', 3, 2, 16, [bytes(r) for r in changed], 2)
        problem = identity.compare(self.dir / 'a.png', self.dir / 'b.png', self.dir / 'd.png')
        self.assertEqual(problem, f'1 of 6 pixels (16.667%), largest step {carry}')

    def test_a_different_size_is_reported(self):
        encode(self.dir / 'a.png', 4, 4, 8, picture(4, 4, 8), 0)
        encode(self.dir / 'b.png', 4, 3, 8, picture(4, 3, 8), 0)
        self.assertIn('size or format differs',
                      identity.compare(self.dir / 'a.png', self.dir / 'b.png', self.dir / 'd.png'))

    def test_same_bytes_with_different_pixel_formats_are_not_identical(self):
        rows = [bytes((1, 2, 3, 4))]
        encode(self.dir / 'a.png', 1, 1, 8, rows, 0, colour=6)
        encode(self.dir / 'b.png', 1, 1, 16, rows, 0, colour=4)
        self.assertIn('size or format differs',
                      identity.compare(self.dir / 'a.png', self.dir / 'b.png', self.dir / 'd.png'))


class SessionCheckTests(unittest.TestCase):
    """A session that didn't do its job would pass as identical in both builds: the harness says so instead."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.rows = picture(4, 4, 8)

    def shot(self, name, rows=None):
        encode(self.dir / name, 4, 4, 8, rows or self.rows, 0)

    def test_a_missing_scene_cannot_pass_when_other_screenshots_exist(self):
        scenes = identity.SESSIONS['vanilla'][2]
        for scene in scenes[1:]:
            self.shot(f'vanilla.{scene}.png')
        self.assertEqual(identity.session_checks(self.dir, ['vanilla']),
                         [f'vanilla.{scenes[0]}: screenshot missing'])

    def test_a_pack_that_changes_nothing_and_hdr_that_stays_off_are_reported(self):
        scenes = identity.SESSIONS['golden_hour'][2]
        for scene in identity.SESSIONS['vanilla'][2]:
            self.shot(f'vanilla.{scene}.png')
        for scene in scenes:
            self.shot(f'golden_hour.{scene}.png')
        for scene in identity.SESSIONS['hdr_toggle'][2]:
            self.shot(f'hdr_toggle.{scene}.png')
            self.shot(f'hdr_toggle.off.{scene}.png')
            self.shot(f'hdr_toggle.off.{scene}-hdr.png')
        hdr_scenes = identity.SESSIONS['golden_hour_hdr'][2]
        for scene in hdr_scenes:
            self.shot(f'golden_hour_hdr.{scene}.png')
            self.shot(f'golden_hour_hdr.{scene}-hdr.png')
        problems = identity.session_checks(self.dir, ['vanilla', 'golden_hour', 'golden_hour_hdr', 'hdr_toggle'])
        self.assertEqual(sorted(problems), sorted(
            [f'golden_hour.{s}: the same as without the pack: it didn\'t run' for s in scenes]
            + [f'golden_hour_hdr.{s}: the same as without the pack: it didn\'t run' for s in hdr_scenes]
            + [f'hdr_toggle.{s}: HDR output was off' for s in identity.SESSIONS['hdr_toggle'][2]]
            + [f'hdr_toggle.off.{s}: HDR output was on' for s in identity.SESSIONS['hdr_toggle'][2]]))

    def test_sessions_that_did_their_job_pass(self):
        other = [bytes(255 - v for v in r) for r in self.rows]
        for scene in identity.SESSIONS['vanilla'][2]:
            self.shot(f'vanilla.{scene}.png')
        for scene in identity.SESSIONS['golden_hour'][2]:
            self.shot(f'golden_hour.{scene}.png', other)
        for scene in identity.SESSIONS['golden_hour_hdr'][2]:
            self.shot(f'golden_hour_hdr.{scene}.png', other)
            self.shot(f'golden_hour_hdr.{scene}-hdr.png', other)
        for scene in identity.SESSIONS['hdr_toggle'][2]:
            self.shot(f'hdr_toggle.{scene}.png')
            self.shot(f'hdr_toggle.{scene}-hdr.png')
            self.shot(f'hdr_toggle.off.{scene}.png')
        self.assertEqual(identity.session_checks(
            self.dir, ['vanilla', 'golden_hour', 'golden_hour_hdr', 'hdr_toggle']), [])

    def test_float_oit_must_change_the_float_picture(self):
        for scene in identity.SESSIONS['float_hdr'][2]:
            for name in ('float_hdr', 'float_oit'):
                for suffix in ('.png', '-hdr.png'):
                    self.shot(f'{name}.{scene}{suffix}')
        self.assertEqual(identity.session_checks(self.dir, ['float_hdr', 'float_oit']),
                         ["float_oit: the same as without Improved Transparency: it didn't run"])
        other = [bytes(255 - v for v in r) for r in self.rows]
        self.shot('float_oit.glass-hdr.png', other)
        self.assertEqual(identity.session_checks(self.dir, ['float_hdr', 'float_oit']), [])

    def test_float_visibility_buffer_needs_its_gpu_log(self):
        for scene in identity.SESSIONS['float_visbuffer'][2]:
            for suffix in ('.png', '-hdr.png'):
                self.shot(f'float_visbuffer.{scene}{suffix}')
        self.assertEqual(identity.session_checks(self.dir, ['float_visbuffer']),
                         ["float_visbuffer: the visibility buffer didn't run"])
        (self.dir / 'float_visbuffer.log').write_text('drawing through a visibility buffer')
        self.assertEqual(identity.session_checks(self.dir, ['float_visbuffer']), [])


class HdrGateTests(unittest.TestCase):
    """The HDR gates' arithmetic, against the standards and the shader's own checkpoints, and what they report."""

    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())

    def test_pq_is_st_2084(self):
        # 🤖 Agent: SMPTE ST 2084's signal at 0, 100, 203 (ITU-R BT.2408's reference white, 58%), 1000 and 10 000 nits.
        for nits, signal in ((0, 0.0), (100, 0.5081), (203, 0.5807), (1000, 0.7518), (10000, 1.0)):
            self.assertAlmostEqual(identity.pq(nits), signal, places=4)

    def test_unshoulder_inverts_the_screenshot_shoulder(self):
        # 🤖 Agent: hdr.rs sdr_screenshots_roll_off_the_world_and_keep_the_hud_as_drawn's checkpoints, light to shot.
        for light, shot in ((0.5, 0.5), (1.0, 0.8), (2.0, 0.9111111), (4.0, 0.9578947), (8.0, 0.9794872)):
            for got, want in zip(identity.unshoulder([shot, shot / 2, 0.0]), (light, light / 2, 0.0)):
                self.assertAlmostEqual(got, want, places=4)

    def unlit(self, sdr, hdr):
        """Every float_unlit scene shot as `sdr` with its HDR shot `hdr`, and HDR off's as `self.vanilla`."""
        for scene in identity.SESSIONS['float_unlit'][2]:
            encode(self.dir / f'vanilla.{scene}.png', 4, 4, 8, self.vanilla, 0)
            encode(self.dir / f'float_unlit.{scene}.png', 4, 4, 8, sdr, 0)
            encode(self.dir / f'float_unlit.{scene}-hdr.png', 4, 4, 16, hdr, 0)
        return identity.hdr_gates(self.dir, ['vanilla', 'float_unlit'])

    def hdr(self, rows, darker=0, sun=()):
        """The HDR10 shot of SDR `rows` at 203 nits, as the game writes it (10 bits in 16), `darker` codes down, and
        the pixels at `sun`, (x, y), four times paper white."""
        def word(code):
            code = max(round(code) - darker, 0)
            return (code << 6 | code >> 4).to_bytes(2, 'big')
        return [b''.join(word(c) for x in range(4) for c in identity.hdr10(
            [4.0] * 3 if (x, y) in sun else [(v / 255) ** 2.2 for v in row[3 * x:3 * x + 3]], 203))
            for y, row in enumerate(rows)]

    def test_hdr_lighting_off_must_be_sdr(self):
        self.vanilla = picture(4, 4, 8)
        self.assertEqual(self.unlit(self.vanilla, self.hdr(self.vanilla)), [])
        # 🤖 Agent: A step of rounding apart is the float frame's.
        stepped = [bytes(min(v + 1, 255) for v in r) for r in self.vanilla]
        self.assertEqual(self.unlit(stepped, self.hdr(stepped)), [])
        scenes = len(identity.SESSIONS['float_unlit'][2])
        darker = self.unlit(self.vanilla, self.hdr(self.vanilla, darker=8))
        self.assertEqual(len(darker), scenes)
        self.assertIn('the HDR shot isn\'t its SDR screenshot\'s picture: 16 pixels', darker[0])
        self.assertEqual(len(self.unlit(self.vanilla, self.hdr(self.vanilla, darker=-8))), scenes)
        moved = [bytes(min(v + 3, 255) for v in r) for r in self.vanilla]
        moved_problems = self.unlit(moved, self.hdr(moved))
        self.assertEqual(len(moved_problems), scenes)
        self.assertTrue(all('the SDR screenshot isn\'t HDR off\'s within rounding' in p for p in moved_problems))

    def test_only_where_the_sun_passes_white_is_the_hdr_shot_brighter_and_hdr_off_not_the_picture(self):
        sun = {(0, 0), (1, 0), (0, 1), (1, 1)}
        # 🤖 Agent: HDR off clips the sun's blue.
        self.vanilla = [bytes(255 if (x // 3, y) in sun and x % 3 == 2 else v for x, v in enumerate(r))
                        for y, r in enumerate(picture(4, 4, 8))]
        # 🤖 Agent: The screenshot clips the sun and its later blends differ from HDR off's, there only.
        moved = [bytes(min(v + 3, 255) if (x // 3, y) in sun else v for x, v in enumerate(r))
                 for y, r in enumerate(self.vanilla)]
        problems = self.unlit(moved, self.hdr(moved, sun=sun))
        self.assertEqual(identity.above_white(self.dir / 'float_unlit.sun-hdr.png', 203), sun)
        self.assertEqual([p for p in problems if '.sun:' in p or '.hud_sun:' in p], [])
        self.assertEqual(len(problems), 2 * (len(identity.SESSIONS['float_unlit'][2]) - 2))
        # 🤖 Agent: Outside it both checks stay two-sided.
        brighter = self.hdr(self.vanilla, darker=-8, sun=sun)
        self.assertIn('float_unlit.hud_sun: the HDR shot isn\'t its SDR screenshot\'s picture: 12 pixels',
                      '\n'.join(self.unlit(self.vanilla, brighter)))
        # 🤖 Agent: Light the build lifts above white where HDR off didn't clip excuses nothing.
        self.vanilla = picture(4, 4, 8)
        moved = [bytes(min(v + 3, 255) if (x // 3, y) in sun else v for x, v in enumerate(r))
                 for y, r in enumerate(self.vanilla)]
        lifted = '\n'.join(self.unlit(moved, self.hdr(moved, sun=sun)))
        for scene in ('sun', 'hud_sun'):
            self.assertIn(f'float_unlit.{scene}: the SDR screenshot isn\'t HDR off\'s within rounding', lifted)
            self.assertIn(f'float_unlit.{scene}: the HDR shot isn\'t its SDR screenshot\'s picture: 4 pixels', lifted)


class BaseTests(unittest.TestCase):
    def test_only_files_no_binary_is_built_from_let_the_base_reuse_the_change(self):
        changed = ['tools/test_frame_identity_parity.py', 'tests/test_frame_identity.py', '.github/workflows/ci.yml',
                   'docs/parity-workflow.md', 'README.md', 'docs/modpacks/atm10-mods.csv', 'crates/x/README.md',
                   'crates/horizon-client-render/src/gfx.rs', 'tools/native-hook-fragments/player.json']
        original = identity.git
        identity.git = lambda *args, cwd=None: '\n'.join(changed)
        try:
            self.assertEqual(identity.needs_own_build('base'), changed[5:])
        finally:
            identity.git = original


class PrepareTests(unittest.TestCase):
    def test_the_player_stands_on_a_block_while_the_scenes_are_built(self):
        # Left in the air, the player fell into the sea, and the boat recipes' toast showed over the first shots.
        commands = []
        game = SimpleNamespace(command=commands.append, ctl=lambda *words, **options: None, settle=lambda: None)
        identity.prepare(game)
        tp = next(i for i, c in enumerate(commands) if ' run tp @s ' in c)
        where, x, y, z = re.search(r'in minecraft:(\S+) run tp @s (\S+) (\S+) (\S+)', commands[tp]).groups()
        below = (f'execute in minecraft:{where} run setblock {math.floor(float(x))} {round(float(y)) - 1} '
                 f'{math.floor(float(z))} minecraft:stone')
        self.assertIn(below, commands[:tp])


if __name__ == '__main__':
    unittest.main()
