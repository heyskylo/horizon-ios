"""tools/ci/git-slim.sh keeps a runner's persistent checkout from piling up packs (docs/ci-runners.md).

actions/checkout sets gc.auto=0 and fetches into the same .git at every job, so nothing repacks: 1051 packs filled the
desktop's disk on 2026-11-09. The script runs where no fetch is going on; these tests build small repositories shaped
like such a checkout and look at what is left.
"""
import os
import shutil
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "tools/ci/git-slim.sh"
ENV = {
    **os.environ,
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@localhost",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@localhost",
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull,
}
# The bash on PATH, as in test_pr_ready: Windows' CreateProcess looks in System32 before PATH, so a bare "bash" there is
# WSL's (#5383), which can't see the host's paths.
BASH = shutil.which("bash")
POSIX_BASH = BASH is not None and not (
    os.name == "nt" and os.path.dirname(os.path.normcase(BASH)).endswith(os.path.normcase(r"\system32")))


def git(repo, *args, check=True):
    return subprocess.run(["git", "-C", str(repo), *args], env=ENV, check=check, capture_output=True, text=True)


def slim(repo, **env):
    return subprocess.run([BASH, str(SCRIPT), str(repo)], env={**ENV, **{k: str(v) for k, v in env.items()}},
                          check=True, capture_output=True, text=True).stdout


def packs(repo):
    return sorted((Path(repo) / ".git/objects/pack").glob("*.pack"))


def checkout_with_packs(root, count):
    """A checkout as the runners leave it: gc.auto=0 and one more pack for every job's fetch."""
    repo = Path(root) / "checkout"
    subprocess.run(["git", "init", "-q", str(repo)], env=ENV, check=True)
    git(repo, "config", "gc.auto", "0")
    # The runners' packs pile up (1051 of them), but git 2.54's automatic maintenance after a commit repacks them
    # whatever gc.auto says: the 12 packs below came back as 1.
    git(repo, "config", "maintenance.auto", "false")
    for i in range(count):
        (repo / f"file{i}.txt").write_text("".join(f"line {j} of file {i}\n" for j in range(200)))
        git(repo, "add", ".")
        git(repo, "commit", "-q", "-m", f"job {i}")
        git(repo, "repack", "-d", "-q")  # what that job's fetch added
    return repo


@unittest.skipUnless(POSIX_BASH, "git-slim.sh needs a bash that is not WSL's")
class GitSlim(unittest.TestCase):
    def test_many_packs_fold_without_losing_history(self):
        with tempfile.TemporaryDirectory() as root:
            repo = checkout_with_packs(root, 12)
            commits = git(repo, "rev-list", "--all").stdout.split()
            self.assertEqual(len(packs(repo)), 12)
            slim(repo, MAX_PACKS=5)
            self.assertLess(len(packs(repo)), 5)
            self.assertEqual(git(repo, "rev-list", "--all").stdout.split(), commits)
            git(repo, "fsck", "--full", "--strict")

    def test_few_packs_are_left_alone(self):
        with tempfile.TemporaryDirectory() as root:
            repo = checkout_with_packs(root, 4)
            before = packs(repo)
            slim(repo, MAX_PACKS=5)
            self.assertEqual(packs(repo), before)

    def test_a_git_dir_over_the_limit_is_collected_into_one_pack(self):
        with tempfile.TemporaryDirectory() as root:
            repo = checkout_with_packs(root, 6)
            # A pull request's merge commit of an earlier job: in the packs, no ref reaches it any more.
            stray = subprocess.run(["git", "-C", str(repo), "hash-object", "-w", "--stdin"], env=ENV, check=True,
                                   input="unreachable\n", capture_output=True, text=True).stdout.strip()
            git(repo, "repack", "-d", "-q")
            git(repo, "prune-packed")
            commits = git(repo, "rev-list", "--all").stdout.split()
            out = slim(repo, MAX_GIT_MB=0)
            self.assertIn("git gc", out)
            self.assertEqual(len(packs(repo)), 1)
            self.assertNotEqual(git(repo, "cat-file", "-e", stray, check=False).returncode, 0, "unreachable object kept")
            self.assertEqual(git(repo, "rev-list", "--all").stdout.split(), commits)
            git(repo, "fsck", "--full", "--strict")

    def test_cut_short_fetches_leave_nothing_but_a_running_one_is_kept(self):
        with tempfile.TemporaryDirectory() as root:
            repo = checkout_with_packs(root, 1)
            pack_dir = repo / ".git/objects/pack"
            old, young = pack_dir / "tmp_pack_aaaaaa", pack_dir / "tmp_pack_bbbbbb"
            for f in (old, young):
                f.write_bytes(b"x" * 4096)
            then = time.time() - 3 * 3600
            os.utime(old, (then, then))
            slim(repo)
            self.assertFalse(old.exists())
            self.assertTrue(young.exists(), "a fetch that is still writing loses its pack")

    def test_a_folder_that_is_no_repository_is_not_an_error(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertEqual(slim(root), "")


if __name__ == "__main__":
    unittest.main()
