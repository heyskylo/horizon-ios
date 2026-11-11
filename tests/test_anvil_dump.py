"""anvil_dump prints NBT as JSON; given no file it asks for one instead of crashing."""

import gzip
import json
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOL = Path(__file__).resolve().parents[1] / "tools" / "anvil_dump.py"


def tag_string(text: str) -> bytes:
    data = text.encode("utf-8")
    return struct.pack(">H", len(data)) + data


def entry(tag_id: int, name: str, payload: bytes) -> bytes:
    return struct.pack(">B", tag_id) + tag_string(name) + payload


def compound(entries: list[bytes]) -> bytes:
    """A TAG_Compound payload: its entries, then the TAG_End that closes it."""
    return b"".join(entries) + b"\x00"


def named_root(entries: list[bytes]) -> bytes:
    """A named root compound, the shape a level.dat has."""
    return entry(10, "", compound(entries))


LEVEL = named_root([
    entry(8, "LevelName", tag_string("Horizon")),
    entry(1, "Difficulty", struct.pack(">b", 2)),
    entry(3, "Version", struct.pack(">i", 42)),
    entry(4, "Seed", struct.pack(">q", 1234567890123)),
    entry(9, "Spawn", struct.pack(">bi", 6, 2) + struct.pack(">dd", 1.5, -2.25)),
    entry(7, "Bytes", struct.pack(">i", 10) + bytes(range(10))),
])

CHUNK = named_root([
    entry(3, "xPos", struct.pack(">i", 0)),
    entry(3, "zPos", struct.pack(">i", 0)),
    entry(8, "Status", tag_string("minecraft:full")),
])


def region(chunks: dict[tuple[int, int], bytes]) -> bytes:
    """An Anvil region file with the given chunks, one sector each."""
    header = bytearray(4096)
    blocks = b""
    for index, (position, chunk) in enumerate(sorted(chunks.items())):
        cx, cz = position
        sector = 2 + index
        header[(cx + cz * 32) * 4:(cx + cz * 32) * 4 + 3] = sector.to_bytes(3, "big")
        payload = gzip.compress(chunk)
        blocks += struct.pack(">i", len(payload) + 1) + bytes([1]) + payload
        blocks += b"\x00" * (4096 - len(blocks) % 4096)
    return bytes(header) + b"\x00" * 4096 + blocks


def run(folder: Path, *arguments: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(TOOL), *arguments],
                          cwd=folder, text=True, capture_output=True)


class UsageTests(unittest.TestCase):
    def test_help_prints_the_usage_and_exits_zero(self):
        with tempfile.TemporaryDirectory() as folder:
            result = run(Path(folder), "--help")
            left = sorted(path.name for path in Path(folder).iterdir())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn("Usage", result.stdout)
        for option in ("--full", "--typed", "--help"):
            self.assertIn(option, result.stdout, "the usage does not document " + option)
        self.assertEqual(left, [], "the tool wrote files while being asked for help")

    def test_short_help_prints_the_usage_and_exits_zero(self):
        with tempfile.TemporaryDirectory() as folder:
            result = run(Path(folder), "-h")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn("Usage", result.stdout)

    def test_without_a_file_it_asks_for_one(self):
        with tempfile.TemporaryDirectory() as folder:
            result = run(Path(folder))
            left = sorted(path.name for path in Path(folder).iterdir())
        self.assertNotEqual(result.returncode, 0, "it reported success without a file")
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn("Usage", result.stdout + result.stderr)
        self.assertEqual(left, [], "the tool wrote files without being told where")


class DumpTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.path = Path(self.folder.name)
        (self.path / "level.dat").write_bytes(LEVEL)

    def test_it_dumps_a_dat_file_as_json(self):
        result = run(self.path, "level.dat")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {
            "LevelName": "Horizon",
            "Difficulty": 2,
            "Version": 42,
            "Seed": 1234567890123,
            "Spawn": [1.5, -2.25],
            "Bytes": "B[10: [0, 1, 2, 3]...]",
        })

    def test_a_path_selects_one_value(self):
        result = run(self.path, "level.dat", "LevelName")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), "Horizon")

    def test_a_path_walks_into_a_list(self):
        result = run(self.path, "level.dat", "Spawn", "1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), -2.25)

    def test_typed_numbers_carry_their_nbt_type(self):
        result = run(self.path, "level.dat", "Difficulty", "--typed")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), "2b")

    def test_full_prints_long_arrays_whole(self):
        result = run(self.path, "level.dat", "Bytes", "--full")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), list(range(10)))

    def test_a_gzipped_dat_reads_the_same(self):
        (self.path / "zipped.dat").write_bytes(gzip.compress(LEVEL))
        result = run(self.path, "zipped.dat", "LevelName")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), "Horizon")

    def test_a_region_file_lists_its_chunks(self):
        (self.path / "r.0.0.mca").write_bytes(region({(0, 0): CHUNK}))
        result = run(self.path, "r.0.0.mca")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), [[0, 0]])

    def test_a_region_file_dumps_a_chunk(self):
        (self.path / "r.0.0.mca").write_bytes(region({(0, 0): CHUNK}))
        result = run(self.path, "r.0.0.mca", "0", "0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {
            "xPos": 0,
            "zPos": 0,
            "Status": "minecraft:full",
        })


if __name__ == "__main__":
    unittest.main()
