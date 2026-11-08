import contextlib
import gzip
import io
import struct
import sys
import tempfile
import unittest
from pathlib import Path, PureWindowsPath
from unittest.mock import patch
ROOT = Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

# The CLI adds tools/ to sys.path; do not let it shadow unittest modules.
with patch.object(sys, 'path', sys.path.copy()):
    from tools import test_save_roundtrip_parity as roundtrip

def tag(kind, name, payload):
    raw = name.encode()
    return bytes([kind]) + struct.pack('>H', len(raw)) + raw + payload


def compound(*tags):
    return b''.join(tags) + b'\0'


def integer(name, value):
    return tag(3, name, struct.pack('>i', value))


class WindowsPath(type(Path())):
    """Keep real file access while relative paths have Windows separators."""
    def relative_to(self, *args, **kwargs):
        return PureWindowsPath(*super().relative_to(*args, **kwargs).parts)

    def __truediv__(self, key):
        return super().__truediv__(PureWindowsPath(key).as_posix())


class SaveRoundtripPathsTests(unittest.TestCase):
    def test_windows_paths_accept_expected_changes_but_retain_real_differences(self):
        with tempfile.TemporaryDirectory() as folder:
            vanilla, horizon = WindowsPath(folder) / 'vanilla', WindowsPath(folder) / 'horizon'
            for world, rule, health, slot, score in (
                (vanilla, 24, 20.0, 0, 1),
                (horizon, 30, 18.0, 1, 2),
            ):
                files = {
                    'level.dat': compound(),
                    'data/minecraft/game_rules.dat': compound(
                        tag(10, 'data', compound(integer('minecraft:max_entity_cramming', rule))),
                    ),
                    'players/data/player.dat': compound(
                        tag(5, 'Health', struct.pack('>f', health)),
                        integer('SelectedItemSlot', slot),
                    ),
                    'data/minecraft/custom.dat': compound(integer('score', score)),
                }
                for rel, payload in files.items():
                    path = world / rel
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(gzip.compress(tag(10, '', payload)))
            # This fixture has no region entities or leash holders.
            with patch.object(roundtrip.scene, 'LEADS', 0):
                report = roundtrip.compare_worlds(vanilla, horizon)
            self.assertCountEqual(report, [
                ('data/minecraft/custom.dat.score', 1, 2),
                ('players/data/player.dat.Health', ('f', 20.0), ('f', 18.0)),
                ('players/data/player.dat.SelectedItemSlot', 0, 1),
            ])
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(roundtrip.summarize(report), 2)


if __name__ == '__main__':
    unittest.main()
