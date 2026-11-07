#!/usr/bin/env python3
"""The fork's review batch issue and its batch-due signal (docs/fork-workflow.md)."""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone

BASE = os.environ.get("BASE", "perf-platform")
BATCH_SIZE = int(os.environ.get("BATCH_SIZE", "5"))
BATCH_HOURS = float(os.environ.get("BATCH_HOURS", "6"))
OWNER = os.environ.get("FORK_OWNER", "")
GREEN = {"SUCCESS", "SKIPPED", "NEUTRAL"}
STATE = re.compile(r"<!-- review-batch-state (\{.*?\}) -->", re.S)
LABELS = {
    "review-batch": ("0e8a16", "The issue that lists pull requests ready for review"),
    "batch-ready": ("d93f0b", "A review batch is due"),
    "needs-changes": ("e4e669", "Reviewed: waiting on the author"),
}


@dataclass
class Pr:
    number: int
    title: str
    author: str
    head: str
    draft: bool
    conflict: bool
    labels: set[str]
    checks: list[dict] = field(default_factory=list)

    @staticmethod
    def from_api(d: dict) -> "Pr":
        return Pr(d["number"], d["title"], (d.get("author") or {}).get("login", "?"), d["headRefOid"],
                  d["isDraft"], d.get("mergeable") == "CONFLICTING", {l["name"] for l in d.get("labels", [])},
                  d.get("statusCheckRollup") or [])


def check_state(checks: list[dict]) -> tuple[str, datetime | None]:
    """`green`, `pending`, `failing` or `none`, and when the last check finished."""
    if not checks:
        return "none", None
    last, pending, failing = None, False, False
    for c in checks:
        if c.get("__typename") == "StatusContext":
            state, done = c.get("state"), c.get("startedAt")
            if state == "PENDING":
                pending = True
            elif state != "SUCCESS":
                failing = True
        else:
            if c.get("status") != "COMPLETED":
                pending = True
                continue
            if (c.get("conclusion") or "") not in GREEN:
                failing = True
            done = c.get("completedAt")
        if done:
            t = datetime.fromisoformat(done.replace("Z", "+00:00"))
            last = t if last is None or t > last else last
    return ("failing" if failing else "pending" if pending else "green"), last


def classify(pr: Pr) -> tuple[str, datetime | None]:
    """`ready`, or why not: `draft`, `conflict`, `needs-changes`, `checks pending`, `checks failing`, `no checks`."""
    if pr.draft:
        return "draft", None
    if "needs-changes" in pr.labels:
        return "needs-changes", None
    if pr.conflict:
        return "conflict", None
    state, last = check_state(pr.checks)
    if state == "green":
        return "ready", last
    return {"pending": "checks pending", "failing": "checks failing", "none": "no checks"}[state], None


def due(ready: list[tuple[Pr, datetime]], signalled: dict[str, str], now: datetime) -> list[Pr]:
    """The ready pull requests to signal now, oldest first; empty when no batch is due."""
    fresh = sorted(((p, t) for p, t in ready if signalled.get(str(p.number)) != p.head), key=lambda x: x[1])
    if not fresh:
        return []
    if len(fresh) >= BATCH_SIZE or now - fresh[0][1] >= timedelta(hours=BATCH_HOURS):
        return [p for p, _ in fresh]
    return []


def body(ready: list[tuple[Pr, datetime]], waiting: list[tuple[Pr, str]], state: dict, now: datetime) -> str:
    lines = ["Pull requests into `" + BASE + "` that are ready for review (tools/fork/review_batch.py, updated "
             + now.strftime("%Y-%m-%d %H:%M UTC") + "). A batch is due at " + str(BATCH_SIZE)
             + " ready pull requests or after " + f"{BATCH_HOURS:g}" + " h.", "", "**Ready**", ""]
    lines += [f"- #{p.number} {p.title} (@{p.author}, `{p.head[:10]}`, ready since {t.strftime('%m-%d %H:%M')})"
              for p, t in sorted(ready, key=lambda x: x[1])] or ["- none"]
    lines += ["", "**Waiting**", ""]
    lines += [f"- #{p.number} {p.title} (@{p.author}): {why}" for p, why in waiting] or ["- none"]
    lines += ["", f"<!-- review-batch-state {json.dumps(state, sort_keys=True)} -->"]
    return "\n".join(lines)


def gh(*args: str, dry: bool = False) -> str:
    if dry and args[0] in ("issue", "pr", "label") and args[1] in ("create", "edit", "comment"):
        print("would run: gh " + " ".join(args[:3]), file=sys.stderr)
        return ""
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def update(repo: str, dry: bool) -> int:
    now = datetime.now(timezone.utc)
    for name, (color, desc) in LABELS.items():
        gh("label", "create", name, "--repo", repo, "--color", color, "--description", desc, "--force", dry=dry)
    fields = "number,title,author,headRefOid,isDraft,mergeable,labels,statusCheckRollup"
    prs = [Pr.from_api(d) for d in json.loads(gh("pr", "list", "--repo", repo, "--base", BASE, "--state", "open",
                                                  "--limit", "200", "--json", fields))]
    issues = json.loads(gh("issue", "list", "--repo", repo, "--label", "review-batch", "--state", "open",
                           "--json", "number,body,labels", "--limit", "1"))
    issue = issues[0] if issues else None
    m = STATE.search(issue["body"]) if issue else None
    state = json.loads(m.group(1)) if m else {"signalled": {}, "reviewed": {}}
    ready, waiting = [], []
    for p in prs:
        reviewed = state["reviewed"].get(str(p.number))
        if "needs-changes" in p.labels and reviewed and reviewed != p.head:
            gh("pr", "edit", str(p.number), "--repo", repo, "--remove-label", "needs-changes", dry=dry)
            p.labels.discard("needs-changes")
        why, since = classify(p)
        (ready.append((p, since)) if why == "ready" else waiting.append((p, why)))
    open_numbers = {str(p.number) for p in prs}
    state = {k: {n: h for n, h in v.items() if n in open_numbers} for k, v in state.items()}
    batch = due(ready, state["signalled"], now)
    for p in batch:
        state["signalled"][str(p.number)] = p.head
    text = body(ready, waiting, state, now)
    if issue is None:
        url = gh("issue", "create", "--repo", repo, "--title", "Review batch", "--label", "review-batch",
                 "--body", text, dry=dry).strip()
        number = url.rsplit("/", 1)[-1] if url else ""
    else:
        number = str(issue["number"])
        gh("issue", "edit", number, "--repo", repo, "--body", text, dry=dry)
    if batch and number:
        ping = f" @{OWNER}" if OWNER else ""
        gh("issue", "comment", number, "--repo", repo, "--body", "🤖 Review batch due:" + ping + " "
           + " ".join(f"#{p.number}@{p.head[:10]}" for p in batch), dry=dry)
        gh("issue", "edit", number, "--repo", repo, "--add-label", "batch-ready", dry=dry)
    print(f"{len(ready)} ready, {len(waiting)} waiting, batch: {[p.number for p in batch]}")
    return 0


def reviewed(repo: str, number: str, head: str, verdict: str) -> int:
    """Records a review: `changes` labels it `needs-changes` until a new head; `merged` only clears its state."""
    issue = json.loads(gh("issue", "list", "--repo", repo, "--label", "review-batch", "--state", "open",
                          "--json", "number,body", "--limit", "1"))[0]
    m = STATE.search(issue["body"])
    state = json.loads(m.group(1)) if m else {"signalled": {}, "reviewed": {}}
    state["reviewed"][number] = head
    if verdict == "changes":
        gh("pr", "edit", number, "--repo", repo, "--add-label", "needs-changes")
    text = STATE.sub(f"<!-- review-batch-state {json.dumps(state, sort_keys=True)} -->", issue["body"])
    gh("issue", "edit", str(issue["number"]), "--repo", repo, "--body", text)
    return 0


if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["update"] and args[1:] in ([], ["--dry-run"]):
        sys.exit(update(os.environ["REPO"], args[1:] == ["--dry-run"]))
    if args[:1] == ["reviewed"] and len(args) == 4 and args[3] in ("changes", "merged"):
        sys.exit(reviewed(os.environ["REPO"], args[1], args[2], args[3]))
    sys.exit("usage: review_batch.py update [--dry-run] | reviewed PR HEAD changes|merged (docs/fork-workflow.md)")
