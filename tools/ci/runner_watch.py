#!/usr/bin/env python3
"""Every 30 minutes: are jobs waiting for a self-hosted runner nobody runs? (.github/workflows/runner-watch.yml)

    python3 tools/ci/runner_watch.py [--dry-run] [--hours 2]

Several jobs run only on the maintainer's machines (docs/ci-runners.md): the vanilla harnesses need his own copy
of Minecraft, the macOS builds his Mac. When those machines are off, GitHub keeps such a job queued for up to a day
and then fails it, and nothing says why: triage commands go unanswered, Nightlies stop, a pull request's parity
check stays yellow. This script lists the jobs of this repository's queued and running workflow runs that wait for
a self-hosted runner, and when any has waited longer than --hours in a pool where no job runs at all (so the
runners are offline, not just busy) it opens one issue (or updates its text) saying
which runners are missing, what stops because of it and what to do (docs/operations.md). When nothing waits any
more, it comments and closes that issue. Needs only the workflow token's `actions: read` and `issues: write`, so it
runs on GitHub's own runners even when every self-hosted one is gone.
"""

import argparse
import datetime as dt
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
TITLE = "CI: self-hosted runners are not taking jobs"
GUIDE = "https://github.com/horizon-rust/horizon/blob/main/docs/operations.md#self-hosted-runners-are-offline"

# What stops while no runner with this label takes jobs, and what the Team does meanwhile. A job asks for a set of
# labels; the first of these it asks for names it.
EFFECTS = [
    ("horizon-light", "Small jobs (triage and claim commands, pull request labels, overlap notes, the Mojang "
                      "republish check, a pull request's parity scope) wait instead of running.",
     "An organisation owner sets the repository variable `SELF_HOSTED_CI` to `off`: they run on GitHub's runners."),
    ("horizon-checks", "`verify` and `launcher` of the pull requests whose number ends in 0 or 1 wait, so those pull "
                       "requests can't merge.",
     "An organisation owner sets `SELF_HOSTED_CI` to `off` (or `light`), then the authors push again or a Team "
     "member re-runs the checks."),
    ("horizon-parity-main", "parity-main doesn't run, so no newer commit of main is green for the Nightly: Nightlies "
                            "keep shipping the last green commit or nothing.",
     "Nothing to do on GitHub: the harnesses need the maintainer's Minecraft. Merge on `verify` as usual; label "
     "risky pull requests `needs-proof`."),
    ("horizon-nightly", "The Nightly's macOS builds wait, so no Nightly is published.",
     "Players keep the last Nightly. Nothing to do on GitHub until a Mac runner is back."),
    ("horizon-release", "The launcher's macOS builds wait, so no launcher release is published.",
     "Players keep their launcher. Nothing to do on GitHub until a Mac runner is back."),
    ("horizon-game", "A pull request's own parity job waits (it isn't required: merging still works).",
     "Merge on `verify` after reading the diff; label pull requests that touch gameplay `needs-proof`."),
]


def parse_time(text):
    return dt.datetime.fromisoformat(text.replace("Z", "+00:00"))


def waiting_jobs(jobs, now, hours):
    """The jobs queued for a self-hosted runner for at least `hours`, in pools where no job runs at all (a busy
    pool works through its queue; an idle one with jobs waiting is offline): [{name, labels, waited_h, url}]."""
    hosted = [job for job in jobs if "self-hosted" in (job.get("labels") or [])]
    running = {pool(job["labels"]) for job in hosted if job.get("status") == "in_progress"}
    out = []
    for job in hosted:
        labels = job["labels"]
        if job.get("status") != "queued" or pool(labels) in running or not job.get("created_at"):
            continue
        waited = (now - parse_time(job["created_at"])).total_seconds() / 3600
        if waited >= hours:
            out.append({"name": job.get("name", "?"), "labels": labels, "waited_h": waited,
                        "url": job.get("html_url", "")})
    return out


def pool(labels):
    """The runner pool a job waits for: the first EFFECTS label it asks for, else its labels joined."""
    return next((label for label, _, _ in EFFECTS if label in labels),
                ", ".join(label for label in labels if label != "self-hosted"))


def body(waiting, now):
    pools = {}
    for job in waiting:
        pools.setdefault(pool(job["labels"]), []).append(job)
    lines = [f"🤖 Agent: written by `tools/ci/runner_watch.py` (runner-watch workflow), updated every 30 minutes; "
             f"last {now:%Y-%m-%d %H:%M} UTC.", "",
             "Jobs are waiting for self-hosted runners and none of those runners is running anything: they are "
             "offline (the maintainer's machines are probably off). GitHub fails such a job after a day.", "",
             "| Runner label | Jobs waiting | Longest wait | Example |", "| --- | --- | --- | --- |"]
    for name, jobs in sorted(pools.items()):
        oldest = max(jobs, key=lambda job: job["waited_h"])
        lines.append(f"| `{name}` | {len(jobs)} | {oldest['waited_h']:.0f} h | [{oldest['name']}]({oldest['url']}) |")
    lines += ["", "**What stops, and what to do:**", ""]
    for label, effect, action in EFFECTS:
        if label in pools:
            lines.append(f"- `{label}`: {effect} {action}")
    lines += ["", f"The whole procedure: [docs/operations.md]({GUIDE}). This issue closes itself when no job "
                  "waits any more."]
    return "\n".join(lines) + "\n"


def plan(waiting, issue, now):
    """(action, text): open, update or close the issue, or nothing. issue: the open issue's number or None."""
    if waiting:
        return ("update" if issue else "open"), body(waiting, now)
    if issue:
        return "close", "🤖 Agent: no job waits for a self-hosted runner any more: closing."
    return None, ""


def gh(*arguments):
    return subprocess.run(["gh", *arguments], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def all_jobs(repo, since):
    jobs = []
    for status in ("queued", "in_progress"):
        runs = json.loads(gh("api", f"repos/{repo}/actions/runs?status={status}&per_page=100&created=%3E%3D{since}",
                             "--jq", "[.workflow_runs[].id]") or "[]")
        for run in runs:
            jobs += json.loads(gh("api", f"repos/{repo}/actions/runs/{run}/jobs?per_page=100", "--jq", ".jobs") or "[]")
    return jobs


def open_issue(repo):
    listed = json.loads(gh("issue", "list", "--repo", repo, "--state", "open", "--search", f'"{TITLE}" in:title',
                           "--json", "number,title") or "[]")
    return next((issue["number"] for issue in listed if issue["title"] == TITLE), None)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default="horizon-rust/horizon")
    parser.add_argument("--hours", type=float, default=2.0, help="how long a job may wait before it counts")
    parser.add_argument("--dry-run", action="store_true", help="print what it would do, change nothing")
    args = parser.parse_args()
    now = dt.datetime.now(dt.timezone.utc)
    since = (now - dt.timedelta(days=2)).strftime("%Y-%m-%d")
    waiting = waiting_jobs(all_jobs(args.repo, since), now, args.hours)
    issue = open_issue(args.repo)
    action, text = plan(waiting, issue, now)
    print(f"{len(waiting)} job(s) waiting over {args.hours:g} h for a self-hosted runner; issue: {issue}; "
          f"action: {action}")
    if args.dry_run or action is None:
        print(text)
        return 0
    if action == "open":
        gh("issue", "create", "--repo", args.repo, "--title", TITLE, "--body", text)
    elif action == "update":
        gh("issue", "edit", str(issue), "--repo", args.repo, "--body", text)
    else:
        gh("issue", "close", str(issue), "--repo", args.repo, "--comment", text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
