#!/usr/bin/env python3
"""One-off: fills in `owner_ids` for the registry's entries that predate them (crates/horizon-mods, `Entry`).

    registry_backfill_owner_ids.py <registry checkout>           shows what it would write (changes nothing)
    registry_backfill_owner_ids.py <registry checkout> --write   writes `owner_ids = [...]` into mods/<id>.toml

Run it in a checkout of horizon-rust/horizon-mods, with `gh` signed in, after the registry's CI runs a
cargo-horizon-mod that knows the field (older ones refuse unknown fields). Commit the result as one pull request.

For each entry without `owner_ids`, every login in `owners` is looked up (`gh api users/<login>`). An entry is
written only when every owner resolves, because once an entry has IDs, only they count. A login GitHub no longer
knows, or whose current account differs from the one the form recorded when it submitted for that login (the
`Co-authored-by: <login> <<id>+<login>@users.noreply.github.com>` lines of the entry's commits), is printed and the
entry is left alone: that is either a rename (look at who it is) or a login someone else now holds. Nothing is
guessed. The exit status is 1 when anything was left alone.
"""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

LOGIN = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$")
OWNERS = re.compile(r"^owners[ \t]*=[ \t]*\[(?P<list>[^\]\n]*)\][ \t]*$", re.M)
OWNER_IDS = re.compile(r"^owner_ids\s*=", re.M)
CO_AUTHOR = re.compile(r"^Co-authored-by: (?P<login>[^<\s]+) <(?P<id>\d+)\+(?P=login)@users\.noreply\.github\.com>\s*$",
                       re.M | re.I)


def owners_of(text: str) -> list[str] | None:
    """The `owners` array, when it is on one line as the registry writes it; None otherwise."""
    m = OWNERS.search(text)
    if not m:
        return None
    return [o.strip().strip('"').strip("'") for o in m.group("list").split(",") if o.strip()]


def with_owner_ids(text: str, ids: list[int]) -> str:
    """`text` with `owner_ids = [...]` on the line after `owners`."""
    m = OWNERS.search(text)
    assert m, "no owners line"
    line = f"owner_ids = [{', '.join(str(i) for i in ids)}]"
    return text[: m.end()] + "\n" + line + text[m.end():]


def recorded_ids(log: str) -> dict[str, set[int]]:
    """login (lowercase) -> the account IDs the form recorded for it in these commit messages."""
    found: dict[str, set[int]] = {}
    for m in CO_AUTHOR.finditer(log):
        found.setdefault(m.group("login").lower(), set()).add(int(m.group("id")))
    return found


def lookup(login: str) -> dict | None:
    r = subprocess.run(["gh", "api", f"users/{login}", "--jq", '{"id": .id, "login": .login, "type": .type}'],
                       capture_output=True, text=True)
    return json.loads(r.stdout) if r.returncode == 0 else None


def history(repo: Path, path: Path) -> str:
    r = subprocess.run(["git", "-C", str(repo), "log", "--follow", "--format=%B", "--", str(path.relative_to(repo))],
                       capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else ""


def plan(text: str, log: str, resolve) -> tuple[list[int] | None, list[str]]:
    """The IDs to write for one entry (None: leave it alone) and the problems found. `resolve(login)` is
    GitHub's user for a login, or None."""
    if OWNER_IDS.search(text):
        return None, []
    owners = owners_of(text)
    if not owners:
        return None, ["no one-line `owners = [...]`: fill it in by hand"]
    recorded = recorded_ids(log)
    ids, problems = [], []
    for login in owners:
        if not LOGIN.match(login):
            problems.append(f"{login!r} is not a GitHub login")
            continue
        user = resolve(login)
        if not user or user.get("type") != "User":
            problems.append(f"@{login}: GitHub has no such user now (deleted or renamed)")
            continue
        seen = recorded.get(login.lower(), set())
        if seen and user["id"] not in seen:
            problems.append(f"@{login} is account {user['id']} now, but the form recorded {sorted(seen)} for that "
                            "login: a rename or a reclaimed login; decide by hand")
            continue
        ids.append(int(user["id"]))
    return (None if problems else ids), problems


def main(argv: list[str]) -> int:
    if not argv or argv[0].startswith("-") or any(a not in ("--write",) for a in argv[1:]):
        sys.exit(__doc__)
    repo, write = Path(argv[0]).resolve(), "--write" in argv[1:]
    mods = sorted((repo / "mods").glob("*.toml"))
    if not mods:
        sys.exit(f"{repo}/mods has no entries: is this a checkout of the registry?")
    cache: dict[str, dict | None] = {}

    def resolve(login: str) -> dict | None:
        key = login.lower()
        if key not in cache:
            cache[key] = lookup(login)
        return cache[key]

    left, done = 0, 0
    for path in mods:
        text = path.read_text()
        ids, problems = plan(text, history(repo, path), resolve)
        if problems:
            left += 1
            for p in problems:
                print(f"{path.name}: {p}", file=sys.stderr)
        if ids is None:
            continue
        done += 1
        print(f"{path.name}: owner_ids = {ids}")
        if write:
            path.write_text(with_owner_ids(text, ids))
    print(f"{done} entries {'written' if write else 'to write (dry run: --write writes them)'}, {left} left alone")
    return 1 if left else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
