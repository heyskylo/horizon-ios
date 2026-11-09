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
    return subprocess.run(["git", *args], capture_output=True, encoding="utf-8", check=True, cwd=ROOT).stdout


def lines(text):
    return text.count("\n") + (1 if text and not text.endswith("\n") else 0)


def generated(text):
    return "@generated" in "\n".join(text.split("\n", 5)[:5])


def changes(base):
    """(path, old path or None) for each Rust file added, changed or renamed since the merge base with `base`,
    working tree included."""
    merge_base = git("merge-base", base, "HEAD").strip()
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
    args = parser.parse_args(argv)
    if not args.base:
        print("File size: no base to compare with, nothing checked.")
        return 0
    merge_base, changed = changes(args.base)
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
