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


class PullRequestMergeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t.invalid",
                        GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t.invalid")
        self.git("init", "-q", "-b", "main")
        self.write("upstream.rs", 3100)
        self.write("own.rs", 3100)
        self.commit("base")
        self.stale_base = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return subprocess.run(["git", "-c", "core.hooksPath=", "-c", "commit.gpgsign=false", *args],
                              cwd=self.root, env=self.env, check=True, capture_output=True,
                              text=True).stdout.strip()

    def write(self, path, count):
        (self.root / path).write_text("x\n" * count)

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "-q", "-m", message)

    def checkout_merge(self, own_growth=False, reverse_parents=False):
        self.git("checkout", "-q", "-b", "feature")
        self.write("small.rs", 10)
        if own_growth:
            self.write("own.rs", 3101)
        self.commit("PR change")
        self.pr_head = self.git("rev-parse", "HEAD")
        self.git("checkout", "-q", "main")
        self.write("upstream.rs", 3101)
        self.commit("main advanced after the PR event")
        if reverse_parents:
            self.git("checkout", "-q", "feature")
            self.git("merge", "-q", "--no-ff", "main", "-m", "test merge")
        else:
            self.git("checkout", "-q", "--detach")
            self.git("merge", "-q", "--no-ff", "feature", "-m", "test merge")

    def check(self, *args):
        output = io.StringIO()
        with mock.patch.object(check_file_size, "ROOT", self.root), contextlib.redirect_stdout(output):
            result = check_file_size.main(list(args))
        return result, output.getvalue()

    def test_test_merge_does_not_charge_unrelated_main_growth(self):
        self.checkout_merge()
        # Without PR metadata, the original explicit-base/working-tree mode is unchanged.
        self.assertEqual(self.check("--base", self.stale_base)[0], 1)
        self.assertEqual(self.check("--base", self.stale_base, "--pr-head", self.pr_head)[0], 0)

    def test_own_growth_still_fails_after_main_advances(self):
        self.checkout_merge(own_growth=True)
        result, output = self.check("--base", self.stale_base, "--pr-head", self.pr_head)
        self.assertEqual(result, 1)
        self.assertIn("own.rs: 3101 lines", output)
        self.assertNotIn("upstream.rs:", output)

    def test_direct_pr_checkout_keeps_checking_the_whole_change(self):
        self.checkout_merge(own_growth=True)
        self.git("checkout", "-q", self.pr_head)
        result, output = self.check("--base", "main", "--pr-head", self.pr_head)
        self.assertEqual(result, 1)
        self.assertIn("own.rs: 3101 lines", output)

    def test_working_tree_growth_is_not_exempted(self):
        self.checkout_merge()
        self.write("own.rs", 3101)
        self.assertEqual(self.check("--base", self.stale_base, "--pr-head", self.pr_head)[0], 1)

    def test_test_merge_parent_order_does_not_change_the_result(self):
        self.checkout_merge(reverse_parents=True)
        self.assertEqual(self.check("--base", self.stale_base, "--pr-head", self.pr_head)[0], 0)

    def test_unrelated_checkout_metadata_fails_closed(self):
        self.checkout_merge()
        with contextlib.redirect_stderr(io.StringIO()) as errors, self.assertRaises(SystemExit) as error:
            self.check("--base", self.stale_base, "--pr-head", self.stale_base)
        self.assertEqual(error.exception.code, 2)
        self.assertIn("Checkout must be the PR head", errors.getvalue())

    def test_pr_metadata_requires_an_explicit_base(self):
        with contextlib.redirect_stderr(io.StringIO()) as errors, self.assertRaises(SystemExit) as error:
            self.check("--pr-head", self.stale_base)
        self.assertEqual(error.exception.code, 2)
        self.assertIn("--pr-head requires --base", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
