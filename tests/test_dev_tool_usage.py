"""Dev tools that take paths ask for them, instead of crashing or writing a flag as a directory."""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / "tools"


def run(name: str, *arguments: str) -> tuple[subprocess.CompletedProcess, list[str]]:
    """Runs one tool in an empty folder; returns its result and whatever it left there."""
    with tempfile.TemporaryDirectory() as folder:
        result = subprocess.run([sys.executable, str(TOOLS / name), *arguments],
                                cwd=folder, text=True, capture_output=True)
        left = sorted(path.name for path in Path(folder).iterdir())
    return result, left


class MissingArgumentTests(unittest.TestCase):
    def assert_asked_for_arguments(self, name: str, *arguments: str) -> None:
        result, left = run(name, *arguments)
        self.assertNotIn("Traceback", result.stderr, f"{name} crashed instead of printing usage")
        self.assertNotEqual(result.returncode, 0, f"{name} reported success without its arguments")
        self.assertIn("Usage", result.stderr + result.stdout, f"{name} printed no usage")
        self.assertEqual(left, [], f"{name} wrote files without being told where")


class JsonLoadFixturesTests(MissingArgumentTests):
    def test_no_directory_asks_for_one(self):
        self.assert_asked_for_arguments("json_load_fixtures.py")

    def test_a_flag_is_not_an_output_directory(self):
        # It used to create ./--help and fill it with 66 MB of fixtures.
        self.assert_asked_for_arguments("json_load_fixtures.py", "--help")


class BakeBricolageTests(MissingArgumentTests):
    def test_no_fonts_asks_for_them(self):
        self.assert_asked_for_arguments("bake_bricolage.py")

    def test_one_font_is_not_enough(self):
        self.assert_asked_for_arguments("bake_bricolage.py", "source.ttf")

class PadGlyphsTests(MissingArgumentTests):
    def test_a_flag_is_not_an_output_directory(self):
        # It used to create ./--help and fill it with 14 glyphs.
        self.assert_asked_for_arguments("pad_glyphs/gen.py", "--help")


class FetchDxcTests(MissingArgumentTests):
    def test_a_flag_is_not_a_folder(self):
        # It used to download Microsoft's 22 MB archive into ./--help.
        self.assert_asked_for_arguments("ci/fetch_dxc.py", "--help")


if __name__ == "__main__":
    unittest.main()
