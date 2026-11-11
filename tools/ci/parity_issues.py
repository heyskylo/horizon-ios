#!/usr/bin/env python3
"""Turns the nightly audit's results into GitHub issues (.github/workflows/nightly.yml).

    python3 tools/ci/parity_issues.py --since 2026-10-15T03:00:00 [--dry-run]

Reads tools/parity.py's state (.local/parity-runner/state.json) for the harnesses that finished since --since,
shards merged into their harness, and compares them with the previous audit (.local/parity-runner/audit.json):

- a harness that fails now and passed in the previous audit, or was in none before (one merged while its pull
  request's parity job failed or was skipped: nine by 2026-11-10, #15158, which no audit reported), gets one open
  issue labelled parity-regression, titled "Parity regression: <harness>" (an open one is commented on instead of
  opening another);
- a harness with an open issue that still fails gets a comment on it;
- a harness with an open issue that passes again gets its issue closed.

When more than MASS_FAILURE harnesses newly fail at once (a commit that does not build, a broken runner), they get
one issue titled "Nightly audit: N harnesses newly fail" instead of one each (2026-11-06: a main commit that did not
compile opened 421), and their previous results are kept so the next audit compares them again.

The first audit only records the results. Needs the gh CLI; --dry-run prints what it would do.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
STATE = ROOT / ".local" / "parity-runner" / "state.json"
AUDIT = ROOT / ".local" / "parity-runner" / "audit.json"
LOGS = ROOT / ".local" / "parity-runner" / "logs"
LABEL = "parity-regression"
TITLE = "Parity regression: "
MASS_FAILURE = 20


def results(state, since):
    """{harness: "pass" | "fail"} of the harnesses that finished since `since`; a harness fails if a shard does."""
    found = {}
    for name, record in state.get("harnesses", {}).items():
        if record.get("finished", "") < since or record.get("shared"):
            continue
        harness = name.split("#")[0]
        found[harness] = "fail" if record["status"] != "pass" or found.get(harness) == "fail" else "pass"
    return found


def plan(now, previous, open_issues):
    """The actions to take: (kind, harness) with kind open, comment or close. open_issues: {harness: number}."""
    actions = []
    for harness, status in sorted(now.items()):
        if status == "fail" and harness in open_issues:
            actions.append(("comment", harness))
        elif status == "fail" and previous.get(harness, "pass") == "pass":
            actions.append(("open", harness))
        elif status == "pass" and harness in open_issues:
            actions.append(("close", harness))
    return actions


def mass_failure(actions):
    """The harnesses to report in one issue instead of one each, or [] when the audit looks normal."""
    opened = [harness for kind, harness in actions if kind == "open"]
    return opened if len(opened) > MASS_FAILURE else []


def first_errors(lines=40):
    """The first compiler errors in the audit's logs: the likely reason for a mass failure."""
    found = []
    for path in sorted(LOGS.glob("*.log")):
        text = path.read_text(errors="replace").splitlines()
        for index, line in enumerate(text):
            if line.lstrip().startswith("error[") or line.lstrip().startswith("error: "):
                found = text[index:index + lines]
                break
        if found:
            break
    return "\n".join(found)


def log_tail(harness, lines=60):
    logs = sorted(LOGS.glob(f"{harness}*.log"))
    text = "\n".join(path.read_text(errors="replace") for path in logs)
    return "\n".join(text.splitlines()[-lines:])


def gh(*arguments, dry_run=False):
    if dry_run:
        print("would run: gh " + " ".join(arguments[:4]) + " ...")
        return ""
    return subprocess.run(["gh", *arguments], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--since", required=True, help="the audit's start (the state's local time format)")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    now = results(json.loads(STATE.read_text()), args.since)
    previous = json.loads(AUDIT.read_text()) if AUDIT.exists() else None
    listed = json.loads(gh("issue", "list", "--label", LABEL, "--state", "open", "--limit", "1000",
                           "--json", "number,title") or "[]")
    open_issues = {issue["title"][len(TITLE):]: issue["number"] for issue in listed if issue["title"].startswith(TITLE)}
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    run = f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/{os.environ.get('GITHUB_REPOSITORY', '')}" \
          f"/actions/runs/{os.environ.get('GITHUB_RUN_ID', '')}"
    failing = sorted(harness for harness, status in now.items() if status == "fail")
    print(f"{len(now)} harnesses ran: {len(now) - len(failing)} passed, {len(failing)} failed"
          + (f" ({', '.join(failing)})" if failing else ""))
    if previous is None:
        print("First audit: results recorded, no issues opened.")
        actions = plan(now, now, open_issues)
    else:
        actions = plan(now, previous, open_issues)
    mass = mass_failure(actions)
    if mass:
        actions = [action for action in actions if action[0] != "open"]
        print(f"open: one issue for {len(mass)} harnesses that newly fail at once")
        gh("issue", "create", "--label", LABEL, "--title", f"Nightly audit: {len(mass)} harnesses newly fail",
           "--body", f"{len(mass)} harnesses passed in the previous nightly audit, or are new, and fail in the audit of "
           f"`{commit[:12]}` ({run}): usually a commit that does not build or a broken runner, so they get this "
           f"one issue instead of one each.\n\n{', '.join(f'`{harness}`' for harness in mass)}\n\n"
           f"<details><summary>First compiler error in the logs</summary>\n\n```\n{first_errors()}\n```\n"
           f"</details>\n", dry_run=args.dry_run)
    for kind, harness in actions:
        details = (f"Nightly audit of `{commit[:12]}` ({run}).\n\n"
                   f"Reproduce: `python3 tools/parity.py {harness}`\n\n<details><summary>End of the log</summary>\n\n"
                   f"```\n{log_tail(harness)}\n```\n</details>\n")
        print(f"{kind}: {harness}")
        if kind == "open":
            gh("issue", "create", "--label", LABEL, "--title", TITLE + harness, "--body",
               (f"`{harness}` passed in the previous nightly audit and fails now.\n\n" if harness in (previous or {})
                else f"`{harness}` fails, and no earlier nightly audit has a result for it.\n\n") + details,
               dry_run=args.dry_run)
        elif kind == "comment":
            gh("issue", "comment", str(open_issues[harness]), "--body", "Still failing.\n\n" + details, dry_run=args.dry_run)
        else:
            gh("issue", "close", str(open_issues[harness]), "--comment",
               f"Passes again in the nightly audit of `{commit[:12]}` ({run}).", dry_run=args.dry_run)
    if not args.dry_run:
        merged = dict(previous or {})
        merged.update({harness: status for harness, status in now.items() if harness not in mass})
        AUDIT.write_text(json.dumps(merged, indent=1, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
