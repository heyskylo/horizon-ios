import contextlib
import io
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from tools import check_file_size


def entry(path, before, now, generated=False):
    return (path, before, now, generated)


class ProblemsTests(unittest.TestCase):
    def test_a_big_file_may_not_grow(self):
        found = check_file_size.problems([entry("crates/a/src/game.rs", 21000, 21001)])
        self.assertEqual(len(found), 1)
        self.assertIn("docs/layers.md#file-size", found[0])

    def test_a_big_file_may_shrink_or_stay(self):
        self.assertEqual(check_file_size.problems([entry("a.rs", 21000, 20000), entry("b.rs", 5000, 5000)]), [])

    def test_small_files_grow_up_to_the_limit(self):
        self.assertEqual(check_file_size.problems([entry("a.rs", 2000, 3000)]), [])
        self.assertEqual(len(check_file_size.problems([entry("a.rs", 2000, 3001)])), 1)

    def test_a_new_big_file_fails(self):
        self.assertIn("a new file", check_file_size.problems([entry("a.rs", 0, 3500)])[0])

    def test_third_party_and_generated_files_are_not_checked(self):
        self.assertEqual(check_file_size.problems([entry("third_party/naga/src/x.rs", 8000, 9000),
                                                   entry("a.rs", 4000, 4100, generated=True)]), [])


class GitTests(unittest.TestCase):
    """Against a real repository: renames compare with the old name, the working tree counts."""

    def test_rename_and_growth_in_a_repository(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t.invalid",
                       GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t.invalid")

            def run(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, env=env)

            run("init", "-q", "-b", "main")
            (root / "big.rs").write_text("x\n" * 3100)
            (root / "small.rs").write_text("y\n" * 10)
            run("add", ".")
            run("commit", "-q", "-m", "base")
            run("checkout", "-q", "-b", "work")
            run("mv", "big.rs", "moved.rs")
            run("commit", "-q", "-m", "rename")
            (root / "small.rs").write_text("y\n" * 20)
            with mock.patch.object(check_file_size, "ROOT", root), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(check_file_size.main(["--base", "main"]), 0)
                (root / "moved.rs").write_text("x\n" * 3101)
                self.assertEqual(check_file_size.main(["--base", "main"]), 1)


if __name__ == "__main__":
    unittest.main()
