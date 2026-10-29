#!/usr/bin/env python3
"""Trusted reviewers triage issues and pull requests with comment commands (docs/release-flow.md "Trusted reviewers").

    triage_command.py command     one comment (triage-command.yml): checks who asked, acts, answers in the thread

A comment by Ludovic or by a login in .github/trusted-reviewers.txt, on the first line:

    /close [completed|not planned]     closes an issue (a pull request: closes it without merging)
    /close duplicate of #123           comments "Duplicate of #123" and closes it as a duplicate
    /reopen
    /label bug, good first issue       existing labels only; never creates one
    /unlabel bug

Refused with a one-line answer: a label with `release` in it or `full-parity`; anything by Ludovic (his issues and
pull requests); a merged pull request; a locked thread; more than 60 actions per reviewer in 24 hours. No code of any
pull request is read or run: only the comment's text, its author and the API's answers, which reach this script as
env vars.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import nightly_set as ns  # noqa: E402  (the same list, owner and paused rule as /nightly)

MAX_PER_DAY = 60
PREFIX = "🤖 Triage: "
COMMANDS = ("close", "reopen", "label", "unlabel")
REASONS = {"completed": "completed", "not planned": "not_planned", "not_planned": "not_planned"}
DUPLICATE = re.compile(r"^(?:as\s+)?(?:a\s+)?duplicate\s+of\s+#?(\d{1,7})$", re.I)
USAGE = ("Use `/close`, `/close not planned`, `/close duplicate of #123`, `/reopen`, `/label bug, good first issue` "
         "or `/unlabel bug`.")


class Refused(Exception):
    """A command that changes nothing; its text is the answer."""


@dataclass
class Command:
    verb: str  # close, reopen, label, unlabel
    reason: str | None = None  # close: completed, not_planned, duplicate
    original: int | None = None  # close duplicate of #N
    labels: tuple[str, ...] = ()


def safe(text: str) -> str:
    """Text of a comment, fit to quote back: no markdown, no mentions."""
    return re.sub(r"[^\w#:. -]", "", text)[:60].strip() or "?"


def parse(body: str) -> Command | None:
    """The command on a comment's first line; None when it isn't one. Raises Refused when it is one, but wrong."""
    line = next((line.strip() for line in body.splitlines() if line.strip()), "")
    m = re.match(r"^/(\w+)(?:\s+(.*))?$", line)
    if not m or m.group(1).lower() not in COMMANDS:
        return None
    verb, rest = m.group(1).lower(), (m.group(2) or "").strip()
    if verb == "reopen":
        if rest:
            raise Refused(f"`/reopen` takes no arguments. {USAGE}")
        return Command("reopen")
    if verb == "close":
        if not rest:
            return Command("close", "completed")
        dup = DUPLICATE.match(rest)
        if dup:
            return Command("close", "duplicate", int(dup.group(1)))
        reason = REASONS.get(" ".join(rest.lower().split()))
        if not reason:
            raise Refused(f"`{safe(rest)}` isn't a way to close. {USAGE}")
        return Command("close", reason)
    names = tuple(dict.fromkeys(n.strip() for n in rest.split(",") if n.strip()))
    if not names:
        raise Refused(f"`/{verb}` needs a label name. {USAGE}")
    if verb == "unlabel" and len(names) > 1:
        raise Refused(f"`/unlabel` takes one label. {USAGE}")
    if len(names) > 10:
        raise Refused("At most 10 labels at once.")
    return Command(verb, labels=names)


def label_refusal(name: str) -> str | None:
    low = name.lower()
    if "release" in low or low == "full-parity":
        return f"`{safe(name)}` is a release or parity label: only Ludovic's side touches those."
    return None


def authorize(login: str, reviewers: ns.Reviewers) -> None:
    """Ludovic always; a listed reviewer unless the list is paused; nobody else."""
    if ns.is_owner(login):
        return
    if login.lower() not in reviewers.logins:
        raise Refused(f"Only Ludovic and the trusted reviewers listed in `{ns.REVIEWERS}` can triage this way.")
    if reviewers.paused:
        raise Refused(f"Triage commands are paused: Ludovic stopped them in `{ns.REVIEWERS}`.")


def target_refusal(issue: dict, cmd: Command, login: str) -> str | None:
    """Why this thread can't be acted on (None when it can). `issue` is GitHub's issue object."""
    is_pr = "pull_request" in issue
    kind = "pull request" if is_pr else "issue"
    if not ns.is_owner(login) and ns.is_owner((issue.get("user") or {}).get("login", "")):
        return f"This {kind} is Ludovic's: triage commands don't touch it."
    if is_pr and (issue["pull_request"] or {}).get("merged_at"):
        return "That pull request is already merged."
    if issue.get("locked"):
        return f"This {kind} is locked: triage commands don't act on it."
    closed = issue.get("state") == "closed"
    if cmd.verb == "close" and closed:
        return f"This {kind} is already closed."
    if cmd.verb == "reopen" and not closed:
        return f"This {kind} is already open."
    return None


def names_of(issue: dict) -> set[str]:
    return {str(l.get("name", "") if isinstance(l, dict) else l).lower() for l in issue.get("labels", [])}


# --- GitHub ------------------------------------------------------------------------------------------------------


def gh(*args: str, check: bool = True) -> str:
    r = subprocess.run(["gh", *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"gh {args[0]} {args[1] if len(args) > 1 else ''}: {r.stderr.strip()[:300]}")
    return r.stdout.strip() if r.returncode == 0 else ""


class Api:
    """The few GitHub calls this needs; tests replace it."""

    def __init__(self, repo: str):
        self.repo = repo

    def issue(self, n: int) -> dict | None:
        raw = gh("api", f"repos/{self.repo}/issues/{n}", check=False)
        return json.loads(raw) if raw else None

    def label(self, name: str) -> str | None:
        """The label's real name, or None when the repository has no such label."""
        raw = gh("api", f"repos/{self.repo}/labels/{name.replace('/', '%2F').replace(' ', '%20')}", check=False)
        return json.loads(raw)["name"] if raw else None

    def add_labels(self, n: int, names: list[str]) -> None:
        args = ["api", f"repos/{self.repo}/issues/{n}/labels"]
        for name in names:
            args += ["-f", f"labels[]={name}"]
        gh(*args)

    def remove_label(self, n: int, name: str) -> None:
        gh("api", "-X", "DELETE", f"repos/{self.repo}/issues/{n}/labels/{name.replace(' ', '%20')}")

    def close(self, n: int, reason: str | None) -> None:
        args = ["api", "-X", "PATCH", f"repos/{self.repo}/issues/{n}", "-f", "state=closed"]
        if reason:
            args += ["-f", f"state_reason={reason}"]
        gh(*args)

    def reopen(self, n: int) -> None:
        gh("api", "-X", "PATCH", f"repos/{self.repo}/issues/{n}", "-f", "state=open")

    def comment(self, n: int, text: str) -> None:
        gh("api", f"repos/{self.repo}/issues/{n}/comments", "-f", f"body={text}")

    def recent_comments(self, since: str) -> list[dict]:
        """Comments of the whole repository since a time (ISO 8601), as {user, body}."""
        raw = gh("api", "--paginate", f"repos/{self.repo}/issues/comments?since={since}&per_page=100", "--jq",
                 '.[] | {user: .user.login, body: .body}')
        return [json.loads(line) for line in raw.splitlines() if line.strip()]


def actions_by(api, login: str, now: datetime) -> int:
    """Actions this workflow answered for `login` in the last 24 h, counted from its own comments."""
    since = (now - timedelta(hours=24)).strftime("%Y-%m-%dT%H:%M:%SZ")
    tag = f"on behalf of @{login.lower()}"
    return sum(1 for c in api.recent_comments(since)
               if c["user"] == "github-actions[bot]" and c["body"].startswith(PREFIX)
               and c["body"].split("\n", 1)[0].lower().count(tag))


def say(action: str, login: str, url: str) -> str:
    return f"{PREFIX}{action}, on behalf of @{login} ([their comment]({url}))."


def run(api, cmd: Command, login: str, number: int, url: str, reviewers: ns.Reviewers, now: datetime) -> str:
    """Does the command; returns the comment to post. Raises Refused for a refusal."""
    authorize(login, reviewers)
    for name in cmd.labels:
        if (why := label_refusal(name)):
            raise Refused(why)
    issue = api.issue(number)
    if issue is None:
        raise Refused("There is no such issue or pull request.")
    if (why := target_refusal(issue, cmd, login)):
        raise Refused(why)
    if cmd.original is not None:
        if cmd.original == number:
            raise Refused("An item cannot be a duplicate of itself.")
        if api.issue(cmd.original) is None:
            raise Refused(f"There is no #{cmd.original} in Horizon.")
    if not ns.is_owner(login) and actions_by(api, login, now) >= MAX_PER_DAY:
        raise Refused(f"You used your {MAX_PER_DAY} triage actions of the last 24 hours. Try again later.")
    is_pr = "pull_request" in issue
    if cmd.verb == "reopen":
        api.reopen(number)
        return say("reopened", login, url)
    if cmd.verb == "close":
        if cmd.reason == "duplicate":
            api.comment(number, f"Duplicate of #{cmd.original}")
            api.close(number, None if is_pr else "duplicate")
            return say(f"closed as a duplicate of #{cmd.original}", login, url)
        api.close(number, None if is_pr else cmd.reason)
        how = "closed without merging" if is_pr else f"closed as {cmd.reason.replace('_', ' ')}"
        return say(how, login, url)
    have = names_of(issue)
    real: list[str] = []
    for name in cmd.labels:
        found = api.label(name)
        if found is None:
            raise Refused(f"There is no label `{safe(name)}` in Horizon: triage doesn't create new ones.")
        if (why := label_refusal(found)):
            raise Refused(why)
        real.append(found)
    if cmd.verb == "label":
        new = [n for n in real if n.lower() not in have]
        if not new:
            raise Refused("It already has " + ("that label." if len(real) == 1 else "those labels."))
        api.add_labels(number, new)
        return say("added " + ", ".join(f"`{n}`" for n in new), login, url)
    name = real[0]
    if name.lower() not in have:
        raise Refused(f"It doesn't have `{name}`.")
    api.remove_label(number, name)
    return say(f"removed `{name}`", login, url)


def command() -> int:
    body, login = os.environ["BODY"], os.environ["AUTHOR"]
    number, url, repo = int(os.environ["ISSUE"]), os.environ.get("COMMENT_URL", ""), os.environ["REPO"]
    api: Api = Api(repo)
    try:
        cmd = parse(body)
        if cmd is None:
            return 0
        reviewers = ns.read_reviewers(Path(ns.REVIEWERS).read_text())
        text = run(api, cmd, login, number, url, reviewers, datetime.now(timezone.utc))
    except Refused as e:
        text = f"{PREFIX}refused, {e} (asked by @{safe(login)})"
    except RuntimeError as e:
        print(e, file=sys.stderr)
        text = f"{PREFIX}GitHub refused that, nothing more was done (asked by @{safe(login)})."
    api.comment(number, text)
    return 0


if __name__ == "__main__":
    if sys.argv[1:] != ["command"]:
        sys.exit(__doc__)
    sys.exit(command())
