"""tools/parity_main.py: the base of a parity-main run, and the bisect after a failure."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from tools import parity_main


class ParityMainTests(unittest.TestCase):
    def test_first_bad_finds_the_first_failing_merge(self):
        commits = list("abcdefgh")  # h failed; the base before a passed
        for culprit in commits:
            tried = []
            fails = lambda commit: tried.append(commit) or commit >= culprit
            self.assertEqual(parity_main.first_bad(commits, fails), culprit)
            self.assertLessEqual(len(tried), 3, "log2 of 8 merges")
        self.assertEqual(parity_main.first_bad(["a"], lambda commit: self.fail("nothing to try")), "a")

    def test_base_is_the_newest_green_run_head_contains(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(parity_main, "ROOT", Path(tmp)):
            git = lambda *arguments: parity_main.git(*arguments).stdout.strip()
            git("init", "-q")
            for n in range(3):
                git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", str(n))
            head, parent, grandparent = git("rev-parse", "HEAD"), git("rev-parse", "HEAD^1"), git("rev-parse", "HEAD~2")
            git("checkout", "-q", "-b", "side", grandparent)
            git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "side")
            side = git("rev-parse", "HEAD")
            git("checkout", "-q", head)
            self.assertEqual(parity_main.base(runs=[side, grandparent, parent]), grandparent,
                             "a run on a commit HEAD doesn't contain is skipped")
            self.assertEqual(parity_main.base(runs=[]), parent, "no green run yet: the last merge")
            self.assertEqual(parity_main.base(runs=[head]), head)

    def bisect(self, broken_from, merges=4, budgets=None):
        """Run bisect over merges where each harness of broken_from fails from that merge's index on (None: never;
        0: at the base too). Returns the exit code and the pull requests commented on; with budgets, one run after
        another on the same runner, each as (exit code, comments, commits it ran harnesses at, what it reported)."""
        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(parity_main, "ROOT", Path(tmp)):
            git = lambda *arguments: parity_main.git(*arguments).stdout.strip()
            git("init", "-q")
            for n in range(merges):
                git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty",
                    "-m", f"Merge pull request #{n} from x/y")
            commits = git("rev-list", "--reverse", "HEAD").split()
            last_run = Path(tmp) / ".local/parity-runner/last-run.json"
            last_run.parent.mkdir(parents=True)
            last_run.write_text(json.dumps({"failed": list(broken_from)}))
            real_run = parity_main.subprocess.run
            comments, ran, reported = [], [], []
            # git runs for real in the scratch repo; a `gh pr comment` is only recorded, never sent.
            fake_run = lambda command, **kwargs: (comments.append(command) if command[0] == "gh"
                                                  else real_run(command, **kwargs))
            at = lambda: commits.index(git("rev-parse", "HEAD"))
            run = lambda names, timeout: ran.append(at()) or [
                name for name in names if broken_from[name] is not None and at() >= broken_from[name]]
            runs = []
            with mock.patch.object(parity_main, "LAST_RUN", last_run), \
                 mock.patch.object(parity_main, "BISECT", last_run.with_name("bisect.json")), \
                 mock.patch.object(parity_main, "run_harnesses", run), \
                 mock.patch.object(parity_main.subprocess, "run", fake_run), \
                 mock.patch("builtins.print", lambda *lines, **kwargs: reported.extend(lines)):
                for budget in budgets or [90]:
                    del comments[:], ran[:], reported[:]
                    code = parity_main.bisect(commits[0], 60, comment=True, budget=budget)
                    self.assertEqual(git("rev-parse", "HEAD"), commits[-1], "HEAD checked out again")
                    runs.append((code, [(command[3], command[-1].split("breaks ")[1].split(" on main")[0])
                                        for command in comments], list(ran), [line[len("parity-main: "):]
                                        for line in reported if line.startswith("parity-main: ")]))
            return runs if budgets else runs[0][:2]

    def test_bisect_names_a_merge_only_when_head_fails_and_the_base_passes(self):
        self.assertEqual(self.bisect({"music": None}), (0, []), "0 only when the failures pass again at HEAD")
        self.assertEqual(self.bisect({"music": 0}), (1, []), "failing at the base too blames nobody")
        self.assertEqual(self.bisect({"music": 2}), (1, [("2", "`music`")]))

    def test_bisect_judges_each_harness_on_its_own(self):
        # One harness failing at the base too doesn't hide the merge that broke the others (parity-main 10-31).
        self.assertEqual(self.bisect({"lunge": 0, "music": 2, "flaky": None, "sound": 2}),
                         (1, [("2", "`music`, `sound`")]))
        # Two merges broke two harnesses: each is named.
        self.assertEqual(self.bisect({"music": 3, "sound": 1}), (1, [("1", "`sound`"), ("3", "`music`")]))

    def test_a_search_out_of_time_says_how_far_it_got_and_the_next_run_goes_on(self):
        # 2026-11-09 to 11-11: seven runs searched up to 1,500 merges until the job's limit, and named nothing.
        first, second, third = self.bisect({"music": 11}, merges=16, budgets=[0, 90, 90])
        code, comments, ran, reported = first
        self.assertEqual((code, comments, ran), (1, [], [15, 0]), "HEAD and the base, then no time for the search")
        self.assertIn("ended with 15 merges left. The next run goes on from there.", reported[0])
        code, comments, ran, _ = second
        self.assertEqual((code, comments), (1, [("11", "`music`")]))
        self.assertEqual(ran[:1], [15], "HEAD runs again")
        self.assertNotIn(0, ran, "the base is not run again")
        code, comments, ran, reported = third
        self.assertEqual((code, comments, ran), (1, [], [15]), "a merge already named is not commented on again")
        self.assertIn("music first fail at", reported[0])

    def test_a_search_goes_on_between_the_merges_an_earlier_one_tried(self):
        # Finding sound's merge (2) tried merges 7, 3 and 1, where music passed: music's search starts above 7.
        code, comments, ran, _ = self.bisect({"sound": 2, "music": 11}, merges=16, budgets=[90])[0]
        self.assertEqual(comments, [("2", "`sound`"), ("11", "`music`")])
        self.assertEqual(ran, [15, 0, 7, 3, 1, 2, 11, 9, 10])

    def test_a_harness_a_commit_does_not_have_cannot_fail_there(self):
        with mock.patch.object(parity_main.subprocess, "run") as run:
            self.assertEqual(parity_main.run_harnesses(["not_a_harness_yet"], 60), [])
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
