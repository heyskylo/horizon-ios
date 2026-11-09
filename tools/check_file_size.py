#!/usr/bin/env python3
"""A Rust file over 3000 lines may not grow compared with the branch's base (docs/layers.md#file-size).

Big files slow incremental builds (game.rs, gfx.rs). Any file may grow up to the limit; a file over it may shrink or
stay the same. A renamed file is compared with its old name, and a new file counts from zero. Files under
third_party/ and files that say they are generated (`@generated` in their first lines) are not checked. There is no
list of files: nothing to update when a file shrinks.

Inline test modules don't count, on the base or now: a normal build doesn't compile them. A test module is a
`#[cfg(test)]` line directly followed by `mod name {` (or `pub mod`, `pub(crate) mod`...) at the same indentation,
through its closing `}`. Other `#[cfg(test)]` items count. When in doubt the module counts, so the rule can't skip
real code. A small lexer tells code from strings, raw strings, chars and (nested) comments, and two readings of the
end must agree:
- braces: the module ends on the line where the braces in its code close;
- layout: rustfmt indents everything in the module deeper than `mod` and puts the closing `}` alone at its
  indentation, so the first later line that starts in code at that indentation or less must be that bare `}`.
Unformatted code, or a lexer mistake, makes them disagree, and the module counts. The two start lines must begin in
code too, so Rust source quoted in a string is no module.

    python3 tools/check_file_size.py --base origin/main
"""

import argparse
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
LIMIT = 3000
DOC = "docs/layers.md#file-size"
EXEMPT_PREFIXES = ("third_party/",)
CFG_TEST = re.compile(r"([ \t]*)#\[cfg\(test\)\]\s*")
TEST_MOD = re.compile(r"([ \t]*)(?:pub(?:\([a-z]+\))? )?mod \w+ \{\s*")
# Where code may stop: a newline, a comment, a raw string (with its b or c prefix), a string, a quote.
TOKEN = re.compile(r"\n|//|/\*|(?<!\w)[bc]?r(#*)\"|\"|'")
COMMENT_EDGE = re.compile(r"/\*|\*/")
BRACE = re.compile(r"[{}]")
STRING_REST = re.compile(r'(?:[^"\\]|\\.)*"', re.S)


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, encoding="utf-8", check=True, cwd=ROOT).stdout


def lines(text):
    return text.count("\n") + (1 if text and not text.endswith("\n") else 0)


def lex(text):
    """For each line of `text`: whether it starts in code rather than inside a string or a comment, the `{` minus the
    `}` in its code, and the lowest that count reaches along the line (`} mod live {` is 0 but dips to -1)."""
    starts, braces, lows, pos, end = [True], [0], [0], 0, len(text)

    def code(stop):
        for brace in BRACE.finditer(text, pos, stop):
            braces[-1] += 1 if brace.group() == "{" else -1
            lows[-1] = min(lows[-1], braces[-1])

    def skip_to(stop):
        newlines = text.count("\n", pos, stop)
        starts.extend([False] * newlines)
        braces.extend([0] * newlines)
        lows.extend([0] * newlines)
        return stop

    while token := TOKEN.search(text, pos):
        code(token.start())
        kind, pos = token.group(), token.end()
        if kind == "\n":
            starts.append(True)
            braces.append(0)
            lows.append(0)
        elif kind == "//":
            stop = text.find("\n", pos)
            pos = end if stop < 0 else stop
        elif kind == "/*":
            depth, stop = 1, pos
            while depth and (edge := COMMENT_EDGE.search(text, stop)):
                depth, stop = depth + (1 if edge.group() == "/*" else -1), edge.end()
            pos = skip_to(end if depth else stop)
        elif kind == "'":
            # A char literal ('x', '{', '\'', '\u{7b}') is skipped; a lifetime or label ('a) is not.
            if text.startswith("\\", pos):
                stop = text.find("'", pos + 2)
                pos = end if stop < 0 else stop + 1
            elif text.startswith("'", pos + 1):
                pos += 2
        elif kind == '"':
            rest = STRING_REST.match(text, pos)
            pos = skip_to(rest.end() if rest else end)
        else:
            stop = text.find('"' + token.group(1), pos)
            pos = skip_to(end if stop < 0 else stop + 1 + len(token.group(1)))
    code(end)
    return starts, braces, lows


def test_module_end(rows, starts, braces, lows, i):
    """The last line of the test module whose `#[cfg(test)]` is on line `i`, or None where the readings disagree."""
    indent, depth = TEST_MOD.fullmatch(rows[i + 1]).group(1), 1  # `mod name {` opens the one brace
    for j in range(i + 2, len(rows)):
        closed = depth + lows[j] <= 0
        depth += braces[j]
        if starts[j] and rows[j].strip() and len(rows[j]) - len(rows[j].lstrip()) <= len(indent):
            return j if depth == 0 and rows[j].rstrip() == indent + "}" else None
        if closed:
            return None
    return None


def test_module_lines(text):
    """How many lines of `text` are inline `#[cfg(test)] mod name { ... }` modules (see the module docstring)."""
    rows, (starts, braces, lows) = text.split("\n"), lex(text)
    skipped, i = 0, 0
    while i + 1 < len(rows):
        cfg, module = CFG_TEST.fullmatch(rows[i]), TEST_MOD.fullmatch(rows[i + 1])
        if cfg and module and cfg.group(1) == module.group(1) and starts[i] and starts[i + 1]:
            end = test_module_end(rows, starts, braces, lows, i)
            if end is not None:
                skipped += end - i + 1
                i = end + 1
                continue
        i += 1
    return skipped


def code_lines(text):
    """The lines that count toward the limit: all but inline test modules."""
    return lines(text) - test_module_lines(text)


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
            tests = path.rsplit("/", 1)[-1].removesuffix(".rs") + "_tests.rs"
            out.append(f"{path}: {now} lines without test modules ({was}). A file over {limit} lines may not grow. "
                       f"New tests go into a sibling {tests} (`#[path = \"{tests}\"] mod tests;`); code moves out "
                       f"to a new module in a pure-move PR first, not inside a fix ({DOC})")
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
        before = code_lines(git("show", f"{merge_base}:{old}")) if old else 0
        entries.append((path, before, code_lines(text), generated(text)))
    found = problems(entries)
    if found:
        print(f"File size check failed ({DOC}):")
        for problem in found:
            print(f"  {problem}")
        return 1
    print(f"File size check passed: {len(entries)} changed Rust files, none over {LIMIT} lines grew "
          f"(test modules not counted).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
