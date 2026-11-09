#!/usr/bin/env python3
"""A Rust file over 3000 lines may not grow compared with the branch's base (docs/layers.md#file-size).

Big files slow incremental builds (game.rs, gfx.rs). Any file may grow up to the limit; a file over it may shrink or
stay the same. A renamed file is compared with its old name, and a new file counts from zero. Files under
third_party/ and files that say they are generated (`@generated` in their first lines) are not checked. There is no
list of files: nothing to update when a file shrinks.

    python3 tools/check_file_size.py --base origin/main
"""

import argparse
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
LIMIT = 3000
DOC = "docs/layers.md#file-size"
EXEMPT_PREFIXES = ("third_party/",)


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True, cwd=ROOT).stdout


def lines(text):
    return text.count("\n") + (1 if text and not text.endswith("\n") else 0)


def generated(text):
    return "@generated" in "\n".join(text.split("\n", 5)[:5])


def comparison_base(base, pr_head=None):
    """Use the base actually tested by CI, even if main advanced after the PR event."""
    if pr_head:
        try:
            expected = git("rev-parse", "--verify", "--end-of-options", f"{pr_head}^{{commit}}").strip()
        except subprocess.CalledProcessError as error:
            raise ValueError(f"Cannot resolve PR head commit: {pr_head}") from error
        checkout, *parents = git("rev-list", "--parents", "-n", "1", "HEAD").split()
        if checkout != expected:
            if len(parents) != 2 or expected not in parents:
                raise ValueError("Checkout must be the PR head or its two-parent test merge")
            return next(parent for parent in parents if parent != expected)
    return git("merge-base", base, "HEAD").strip()


def changes(base, pr_head=None):
    """(path, old path or None) for Rust changes against the selected base, working tree included."""
    merge_base = comparison_base(base, pr_head)
    out = []
    for line in git("diff", "--name-status", "-M", "--diff-filter=AMR", merge_base, "--", "*.rs").splitlines():
        parts = line.split("\t")
        status = parts[0]
        if status.startswith("R"):
            out.append((parts[2], parts[1]))
        elif status == "A":
            out.append((parts[1], None))
        else:
            out.append((parts[1], parts[1]))
    return merge_base, out


def problems(entries, limit=LIMIT):
    """entries: (path, base line count, line count now, generated). The problem lines."""
    out = []
    for path, before, now, is_generated in entries:
        if path.startswith(EXEMPT_PREFIXES) or is_generated:
            continue
        if now > limit and now > before:
            was = f"{before} on the base" if before else "a new file"
            out.append(f"{path}: {now} lines ({was}). A file over {limit} lines may not grow: move code out to a "
                       f"new module ({DOC})")
    return out


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base", help="the git ref the branch starts from (CI: the pull request's base)")
    parser.add_argument("--pr-head", help="the PR head commit, to identify CI's test-merge base parent")
    args = parser.parse_args(argv)
    if args.pr_head and not args.base:
        parser.error("--pr-head requires --base")
    if not args.base:
        print("File size: no base to compare with, nothing checked.")
        return 0
    try:
        merge_base, changed = changes(args.base, args.pr_head)
    except ValueError as error:
        parser.error(str(error))
    if args.pr_head:
        print(f"File size comparison base: {merge_base}")
    entries = []
    for path, old in changed:
        file = ROOT / path
        if not file.exists():
            continue
        text = file.read_text(encoding="utf-8", errors="replace")
        before = lines(git("show", f"{merge_base}:{old}")) if old else 0
        entries.append((path, before, lines(text), generated(text)))
    found = problems(entries)
    if found:
        print(f"File size check failed ({DOC}):")
        for problem in found:
            print(f"  {problem}")
        return 1
    print(f"File size check passed: {len(entries)} changed Rust files, none over {LIMIT} lines grew.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
