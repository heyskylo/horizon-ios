#!/usr/bin/env python3
"""Fills in the numeric GitHub IDs the comment commands decide by (tools/ci/nightly_set.py).

    resolve_reviewer_ids.py           every `<login>` line of .github/trusted-reviewers.txt becomes `<id> <login>`,
                                      and OWNER_ID in tools/ci/nightly_set.py gets ludovic111's ID
    resolve_reviewer_ids.py --check   changes nothing: fails if a listed ID's account now has another login (a rename:
                                      look at it before keeping the line) or no longer exists

Needs `gh` signed in (any account: `users/<login>` is public). A login GitHub doesn't know is printed and left as it
is, never guessed: that line keeps granting nothing until someone fixes it by hand.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LIST = ROOT / ".github/trusted-reviewers.txt"
NIGHTLY_SET = ROOT / "tools/ci/nightly_set.py"
LOGIN = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$")
USER_ID = re.compile(r"^[1-9][0-9]{0,11}$")
OWNER_LINE = re.compile(r"^OWNER_ID: int \| None = .*$", re.M)


def user(ref: str) -> dict | None:
    """GitHub's user for a login, or for `user/<id>`; None when it has none."""
    path = f"user/{ref[5:]}" if ref.startswith("user/") else f"users/{ref}"
    r = subprocess.run(["gh", "api", path, "--jq", '{"id": .id, "login": .login, "type": .type}'],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return None
    return json.loads(r.stdout)


def owner_login() -> str:
    m = re.search(r'^OWNER = "([^"]+)"', NIGHTLY_SET.read_text(), re.M)
    if not m:
        sys.exit("tools/ci/nightly_set.py has no OWNER")
    return m.group(1)


def resolve() -> int:
    problems = []
    out = []
    for raw in LIST.read_text().splitlines():
        body, sep, comment = raw.partition("#")
        words = body.split()
        if len(words) == 1 and words[0].lower() != "paused" and LOGIN.match(words[0]):
            found = user(words[0])
            if not found or found["type"] != "User":
                problems.append(f"{words[0]}: GitHub has no such user; left as it is")
                out.append(raw)
                continue
            line = f"{found['id']} {found['login']}"
            out.append(f"{line:<24} #{comment}" if sep else line)
            print(f"{words[0]} -> {found['id']}")
        else:
            out.append(raw)
    LIST.write_text("\n".join(out) + "\n")
    owner = user(owner_login())
    if not owner or owner["type"] != "User":
        problems.append(f"{owner_login()}: GitHub has no such user; OWNER_ID left as it is")
    else:
        text = NIGHTLY_SET.read_text()
        NIGHTLY_SET.write_text(OWNER_LINE.sub(f"OWNER_ID: int | None = {owner['id']}", text, count=1))
        print(f"OWNER_ID ({owner_login()}) -> {owner['id']}")
    for p in problems:
        print(p, file=sys.stderr)
    return 1 if problems else 0


def check() -> int:
    problems = []
    entries = []
    for raw in LIST.read_text().splitlines():
        words = raw.partition("#")[0].split()
        if len(words) == 2 and USER_ID.match(words[0]):
            entries.append((int(words[0]), words[1]))
        elif words and words != ["paused"]:
            problems.append(f"{raw.strip()!r}: not `<id> <login>`, so it grants nothing")
    m = re.search(r"^OWNER_ID: int \| None = (\d+)$", NIGHTLY_SET.read_text(), re.M)
    if m:
        entries.append((int(m.group(1)), owner_login()))
    else:
        problems.append("OWNER_ID in tools/ci/nightly_set.py isn't set")
    for uid, login in entries:
        found = user(f"user/{uid}")
        if not found:
            problems.append(f"{uid} ({login}): GitHub has no account with this ID any more")
        elif found["login"].lower() != login.lower():
            problems.append(f"{uid} is now @{found['login']}, listed as {login}: a rename; check it's the same person")
    for p in problems:
        print(p, file=sys.stderr)
    print(f"{len(entries)} accounts checked, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--check"]:
        sys.exit(check())
    if sys.argv[1:]:
        sys.exit(__doc__)
    sys.exit(resolve())
