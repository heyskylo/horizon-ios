"""The tools that trial-merge with `git merge-tree --write-tree` need git 2.38 or newer."""
import unittest
from unittest import mock

from tools import supervise
from tools.ci import nightly_set


class VersionParsingTests(unittest.TestCase):
    def test_it_reads_the_numbers(self):
        self.assertEqual(supervise.parse_git_version("git version 2.38.1"), (2, 38, 1))
        self.assertEqual(supervise.parse_git_version("git version 2.51.0"), (2, 51, 0))
        self.assertEqual(supervise.parse_git_version("git version 2.39"), (2, 39, 0))

    def test_it_says_so_when_there_is_no_version(self):
        self.assertEqual(supervise.parse_git_version("not git at all"), ())


class TooOldGitTests(unittest.TestCase):
    """An old git gets a plain message, not git's own usage dump."""

    def test_supervise_says_which_version_it_needs(self):
        with mock.patch.object(supervise, "git_version", return_value=(2, 34, 1)):
            with self.assertRaises(SystemExit) as raised:
                supervise.require_git_version()
        message = str(raised.exception)
        self.assertIn("2.38", message)
        self.assertIn("2.34.1", message)
        self.assertIn("merge-tree", message)

    def test_nightly_set_says_which_version_it_needs(self):
        with mock.patch.object(nightly_set, "git_version", return_value=(2, 34, 1)):
            with self.assertRaises(RuntimeError) as raised:
                nightly_set.require_git_version()
        message = str(raised.exception)
        self.assertIn("2.38", message)
        self.assertIn("2.34.1", message)
        self.assertIn("merge-tree", message)

    def test_an_unknown_version_is_reported_too(self):
        with mock.patch.object(supervise, "git_version", return_value=()):
            with self.assertRaises(SystemExit) as raised:
                supervise.require_git_version()
        self.assertIn("could not be read", str(raised.exception))


class NewEnoughGitTests(unittest.TestCase):
    def test_the_check_passes_on_a_new_git(self):
        for module in (supervise, nightly_set):
            with mock.patch.object(module, "git_version", return_value=(2, 38, 0)):
                self.assertEqual(module.require_git_version(), (2, 38, 0))

    def test_the_merge_tree_check_runs_before_the_merge(self):
        """The version is checked where the merge-tree call is, not only when the tool starts."""
        with mock.patch.object(supervise, "git_version", return_value=(2, 34, 1)):
            with self.assertRaises(SystemExit):
                supervise.trial_merge("a" * 40, "b" * 40)
        with mock.patch.object(nightly_set, "git_version", return_value=(2, 34, 1)):
            with self.assertRaises(RuntimeError):
                nightly_set.merge_tree("a" * 40, "b" * 40)


if __name__ == "__main__":
    unittest.main()
