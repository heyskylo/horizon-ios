#!/usr/bin/env python3
"""parity-main (.github/workflows/parity-main.yml): the vanilla harnesses run on main, a batch of merges at a time.

    python3 tools/parity_main.py base               # the commit this run checks from: main's last green parity-main
    python3 tools/parity_main.py bisect BASE        # after a failure: the first merge since BASE that broke a harness

`base` prints the newest commit on main whose parity-main run passed and that HEAD contains, else HEAD's first parent,
so a run tests every merge since the last green one (`tools/parity.py --changed <base>`), however many landed while
the previous run went on. `bisect` reads the harnesses that failed (.local/parity-runner/last-run.json), checks out
the first-parent merges between BASE and HEAD by halves and runs only those harnesses, then names, for each of them that
fails again at HEAD and passes at BASE, the first merge it fails on (written to $GITHUB_STEP_SUMMARY and, with --comment, on its pull request). HEAD is checked out again at the end.

The search has --budget minutes: the job's limit otherwise cuts it off with nothing said (seven runs between
2026-11-09 and 11-11, searching up to 1,500 merges). When they are spent it says between which merges each harness
broke and stops. What it learnt stays in .local/parity-runner/bisect.json while the base is the same, so the next
run goes on from there, and a merge it already named is not commented on again.
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
REPO = os.environ.get("GITHUB_REPOSITORY", "horizon-rust/horizon")
LAST_RUN = ROOT / ".local" / "parity-runner" / "last-run.json"
# {"since": the base, "results": {commit: {harness: it failed there}}, "named": {harness: the merge commented on}}
BISECT = ROOT / ".local" / "parity-runner" / "bisect.json"


def git(*arguments, check=True):
    result = subprocess.run(["git", *arguments], cwd=ROOT, capture_output=True, text=True)
    if check and result.returncode:
        sys.exit(f"git {' '.join(arguments)}: {result.stderr.strip()}")
    return result


def green_runs():
    """Head commits of main's passing parity-main runs, newest first."""
    result = subprocess.run(["gh", "run", "list", "--repo", REPO, "--workflow", "parity-main", "--branch", "main",
                             "--status", "success", "--limit", "30", "--json", "headSha"], capture_output=True, text=True)
    if result.returncode:
        print(f"note: no parity-main history ({result.stderr.strip()})", file=sys.stderr)
        return []
    return [run["headSha"] for run in json.loads(result.stdout or "[]")]


def base(head="HEAD", runs=None):
    for sha in green_runs() if runs is None else runs:
        if git("merge-base", "--is-ancestor", sha, head, check=False).returncode == 0:
            return git("rev-parse", sha).stdout.strip()
    return git("rev-parse", f"{head}^1").stdout.strip()


def first_bad(commits, fails):
    """The first of commits (oldest first, the last known to fail; the one before them known to pass) where fails()."""
    good, bad = -1, len(commits) - 1
    while bad - good > 1:
        middle = (good + bad) // 2
        if fails(commits[middle]):
            bad = middle
        else:
            good = middle
    return commits[bad]


def harness_exists(name):
    return (ROOT / "tools" / f"test_{name.split('#')[0]}_parity.py").exists()


def run_harnesses(names, timeout):
    """The harnesses that fail at the checked-out commit (those it doesn't have yet can't), as parity.py records them."""
    present = [name for name in names if harness_exists(name)]
    if not present:
        return []
    if subprocess.run([sys.executable, "tools/parity.py", *present, "--timeout", str(timeout)], cwd=ROOT).returncode == 0:
        return []
    try:
        failed = json.loads(LAST_RUN.read_text(encoding="utf-8")).get("failed") or []
    except (OSError, ValueError):
        failed = []
    # A run that failed before recording its harnesses (a probe that doesn't build) fails them all.
    return [name for name in present if name in failed] or present


class OutOfTime(Exception):
    pass


def earlier_search(since):
    """What earlier runs' searches from this base learnt, or a new record: another base has other merges to search."""
    try:
        state = json.loads(BISECT.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        state = {}
    if state.get("since") != since:
        state = {"since": since, "results": {}, "named": {}}
    return state


def bisect(since, timeout, comment, budget=90):
    failed = []
    if LAST_RUN.exists():
        failed = json.loads(LAST_RUN.read_text(encoding="utf-8")).get("failed") or []
    if not failed:
        print("No failed harness recorded: nothing to bisect.")
        return 1
    head = git("rev-parse", "HEAD").stdout.strip()
    commits = git("rev-list", "--first-parent", "--reverse", f"{since}..{head}").stdout.split()
    if not commits:
        print("No merge since the base.")
        return 1

    state = earlier_search(since)
    results = state["results"]
    deadline = None  # set once HEAD and the base ran: the budget is for the search between them

    def fails(commit, names):
        """Those of names that fail at commit: from an earlier run of them there, else by running them now."""
        known = results.setdefault(commit, {})
        missing = [name for name in names if name not in known]
        if missing:
            if deadline is not None and time.monotonic() >= deadline:
                raise OutOfTime
            print(f"--- {commit[:9]} {git('log', '-1', '--format=%s', commit).stdout.strip()}", flush=True)
            git("checkout", "-q", "--detach", commit)
            failed_here = run_harnesses(missing, timeout)
            known.update({name: name in failed_here for name in missing})
            BISECT.parent.mkdir(parents=True, exist_ok=True)
            BISECT.write_text(json.dumps(state), encoding="utf-8")
        return [name for name in names if known[name]]

    def report(line):
        print(line)
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
                summary.write(f"### {line}\n")

    # Each harness is judged on its own. One that passes when run again at HEAD is a flake (counted as a pass); one
    # that fails at the base too is the runner's or a flake (no merge to blame: the search would end on the oldest
    # merge); the others fail at HEAD and pass at the base, so a merge broke them and the halving search names it.
    # Harnesses that break at different merges are searched again after the first one. The exit code is the run's
    # verdict: 0 only when every failed harness passes when run again at HEAD (the commit is good, so releases may
    # follow it), 1 otherwise.
    blames = []
    remaining = []
    try:
        # HEAD is run again whatever an earlier run saw there: it is the run's verdict.
        results.pop(head, None)
        at_head = fails(head, failed)
        flakes = [name for name in failed if name not in at_head]
        if flakes:
            report(f"parity-main: {', '.join(flakes)} pass when run again at {head[:9]}: a flake, counted as a pass.")
        if not at_head:
            return 0
        at_base = fails(since, at_head)
        deadline = time.monotonic() + budget * 60
        if at_base:
            report(f"parity-main: {', '.join(at_base)} fail at the base {since[:9]} too: the runner or a flake: "
                   "no merge is named.")
        remaining = [name for name in at_head if name not in at_base]

        def failing(commit):
            return fails(commit, remaining)

        def known(commit):
            """Whether a remaining harness fails at commit, or None where not all of them ran yet."""
            ran = results.get(commit, {})
            return any(ran[name] for name in remaining) if all(name in ran for name in remaining) else None

        low = 0
        while remaining:
            # Between the last merge known to pass and the first known to fail: every merge in a first search, fewer
            # once an earlier run, or this run for another harness, tried merges in between.
            high = next(index for index in range(low, len(commits)) if known(commits[index]))
            low = max([index + 1 for index in range(low, high) if known(commits[index]) is False], default=low)
            span = commits[low:high + 1]
            culprit = first_bad(span, failing) if len(span) > 1 else span[0]
            hit = failing(culprit)
            blames.append((culprit, hit))
            remaining = [name for name in remaining if name not in hit]
            low = commits.index(culprit) + 1
    except OutOfTime:
        for name in remaining:
            ran = [(index, results[commit][name]) for index, commit in enumerate(commits)
                   if name in results.get(commit, {})]
            bad = next(index for index, failed_there in ran if failed_there and index >= low)
            good = max([index for index, failed_there in ran if not failed_there and low <= index < bad], default=low - 1)
            report(f"parity-main: {name} passes at {(commits[good] if good >= 0 else since)[:9]} and fails at "
                   f"{commits[bad][:9]}: the search's {budget} minutes ended with {bad - good} merges left. "
                   "The next run goes on from there.")
    finally:
        git("checkout", "-q", "--detach", head)
    for culprit, hit in blames:
        subject = git("log", "-1", "--format=%s", culprit).stdout.strip()
        number = re.search(r"Merge pull request #(\d+)", subject)
        report(f"parity-main: {', '.join(hit)} first fail at {culprit[:9]} ({subject}), "
               f"of {len(commits)} merges since {since[:9]}.")
        hit = [name for name in hit if state["named"].get(name) != culprit]
        if comment and number and hit:
            url = f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/{REPO}/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}"
            body = (f"🤖 Agent: parity-main found that this merge breaks {', '.join(f'`{name}`' for name in hit)} on main "
                    f"(bisected over {len(commits)} merges: {url}). Releases wait until parity-main passes again.")
            if subprocess.run(["gh", "pr", "comment", number.group(1), "--repo", REPO, "--body", body]).returncode == 0:
                state["named"].update({name: culprit for name in hit})
                BISECT.write_text(json.dumps(state), encoding="utf-8")
    return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("base")
    bisecting = commands.add_parser("bisect")
    bisecting.add_argument("since", help="the last commit known to pass (the run's base)")
    bisecting.add_argument("--timeout", type=int, default=2400, help="seconds per harness")
    bisecting.add_argument("--comment", action="store_true", help="comment on the breaking merge's pull request")
    bisecting.add_argument("--budget", type=int, default=90, help="minutes for the search; then it says how far it got")
    args = parser.parse_args()
    if args.command == "base":
        print(base())
        return 0
    return bisect(args.since, args.timeout, args.comment, args.budget)


if __name__ == "__main__":
    sys.exit(main())
