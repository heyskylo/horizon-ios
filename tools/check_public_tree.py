#!/usr/bin/env python3
"""Reject local game artifacts even when somebody uses git add -f."""

import argparse
import collections
from pathlib import PurePosixPath
import subprocess
import sys

PRIVATE_ROOTS = {".local", "decompiled", "minecraft", "assets", "run", "logs"}
PRIVATE_EXTENSIONS = {".jar", ".class", ".jfr", ".hprof", ".java"}
ORIGINAL_JDK_TOOLS = {"tools/JfrSummary.java", "tests/java/JfrFixture.java"}
# The prepared folder's files in one file (crates/horizon-core/src/prepared_files.rs): Mojang's data.
PRIVATE_NAMES = {"horizon-data.bin"}
# Vanilla's 1.13 data fixer tables, copied from its code until 2026-11; horizon-prepare now exports them from the
# player's jar (datafix-tables.json). Their old paths stay in history, so only the current tree is checked.
RETIRED_TABLES = {"crates/horizon-world/src/datafix/flat_tables.rs",
                  "crates/horizon-world/src/datafix/block_state_data.txt"}
# A file with this many entries like those tables' holds a copy of them, whatever its name or format:
# ItemStackTheFlatteningFix's `id.damage` keys, and BlockStateData's `id<TAB>name<TAB>` rows (raw or JSON-escaped).
TABLE_FINGERPRINTS = [r"minecraft:[a-z_]+\.[0-9]+", "[0-9]+(\t|\\\\t)minecraft:[a-z_]+(\t|\\\\t)"]
TABLE_ENTRIES = 50


def prohibited(path):
    parsed = PurePosixPath(path)
    # Original JDK-only tools are an exact allowlist; recovered game Java stays private.
    if path in ORIGINAL_JDK_TOOLS:
        return False
    # Original Horizon mod sources may also be tracked.
    if parsed.suffix == ".java" and path.startswith(("mods/horizon/src/", "mods/horizon-lab/src/")):
        return not ("/dev/horizonlab/" in path)
    return (
        bool(parsed.parts and parsed.parts[0] in PRIVATE_ROOTS)
        or parsed.suffix.lower() in PRIVATE_EXTENSIONS
        or parsed.name in PRIVATE_NAMES
    )


def copied_tables():
    """Tracked files (in the index) that hold vanilla's retired data fixer tables."""
    found = {path for path in paths_in_repository() if path in RETIRED_TABLES}
    for pattern in TABLE_FINGERPRINTS:
        # Every match, not matching lines: a minified JSON file is one line.
        result = subprocess.run(["git", "grep", "--cached", "-I", "-z", "-o", "-E", "-e", pattern],
                                capture_output=True, text=True)
        if result.returncode not in (0, 1):
            raise RuntimeError(f"git grep failed: {result.stderr.strip()}")
        counts = collections.Counter(line.split("\0", 1)[0] for line in result.stdout.splitlines())
        found.update(path for path, count in counts.items() if count >= TABLE_ENTRIES)
    return found


def paths_in_repository(history=False, revisions=None):
    if not history:
        result = subprocess.check_output(["git", "ls-files", "-z"])
        return result.decode("utf-8").split("\0")
    # An empty push (including deletions only) must not fall back to HEAD or --all.
    if revisions is not None and not revisions:
        return []
    roots = ["--all"] if revisions is None else revisions
    # Include diffs against merge parents so merge-resolution-only artifacts are caught.
    # Git pushes the stored objects, not the history presented by local replace refs.
    result = subprocess.check_output(["git", "--no-replace-objects", "log", "--format=", "--name-only",
                                      "-z", "-m", *roots, "--"])
    return [path.lstrip("\n") for path in result.decode("utf-8").split("\0")]


def pushed_revisions(stream):
    """Read Git's pre-push ref updates and retain the non-deleted local object IDs."""
    revisions = []
    for line in stream:
        _local_ref, local_oid, _remote_ref, _remote_oid = line.split()
        if local_oid.strip("0"):
            revisions.append(local_oid)
    return revisions


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--history", action="store_true", help="Inspect paths in all local Git history")
    mode.add_argument("--pre-push", action="store_true", help="Inspect history reachable from Git's ref updates on stdin")
    # CI's pull requests: only the commits a merge would add, not every ref's whole history.
    mode.add_argument("--range", metavar="BASE..HEAD", help="Inspect the commits of a revision range")
    args = parser.parse_args()
    revisions = pushed_revisions(sys.stdin) if args.pre_push else [args.range] if args.range else None
    history = args.history or args.pre_push or bool(args.range)
    rejected = sorted({path for path in paths_in_repository(history, revisions)
                       if path and prohibited(path)})
    tables = [] if history else sorted(copied_tables())
    if tables:
        print("Vanilla's data fixer tables come from the player's jar (horizon-prepare), not from Git:",
              file=sys.stderr)
        for path in tables:
            print(f"  {path}", file=sys.stderr)
        return 1
    if rejected:
        print("Private game/profiling artifacts must stay outside Git:", file=sys.stderr)
        for path in rejected:
            print(f"  {path}", file=sys.stderr)
        return 1
    print("Public tree check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
