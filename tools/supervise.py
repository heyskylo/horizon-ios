#!/usr/bin/env python3
"""Manage parallel parity work: one worktree and branch per area, one integration branch.

    python3 tools/supervise.py status             # every worktree: branch, progress, last gate
    python3 tools/supervise.py new dispensers     # worktree ../dispensers on parity/dispensers, from the integration branch
    python3 tools/supervise.py save [AREA ...]    # commit and push unfinished work (before stopping agents)
    python3 tools/supervise.py check [BRANCH ...]  # which branches merge cleanly, alone and together (seconds)
    python3 tools/supervise.py integrate parity/dispensers   # merge into the integration branch if the gate passes
    python3 tools/supervise.py integrate parity/a parity/b   # the largest set that merges cleanly, one gate
    python3 tools/supervise.py integrate --ready              # every parity/* branch with new commits

The integration branch (`parity-base` by default) is checked out in its own
worktree; `integrate` merges there, runs tools/gate.py and pushes the
integration branch only when the gate passes, allowing only the harnesses
that already fail on the integration branch to keep failing. Otherwise it undoes the merge
and leaves the branch for its owner to fix.

`check` and `integrate` first trial-merge the candidates in the object database
(git merge-tree; no worktree is touched): each alone, every pair, then the
largest set without conflicts, merged in turn. `integrate` merges that set and
gates it once (formatting, lints and tests come before the long harness run).
If the gate fails on several branches, it tries each half in turn, so the
branches that pass still land. Worktrees live next to the
integration worktree and share its vanilla data: their `.local/` entries
link to `shared-local/` in the same directory.
"""

import argparse
import functools
import json
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
# `git merge-tree --write-tree` (the trial merge, in the object database) only exists since git 2.38.
MIN_GIT_VERSION = (2, 38)
# What the version is needed for, in the message a too-old git gets.
GIT_VERSION_USE = ("this tool trial-merges the branches with `git merge-tree --write-tree`, which only exists since "
                   "git 2.38")


def git(*args, cwd=None, check=True):
    cwd = cwd or ROOT
    result = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if check and result.returncode:
        raise SystemExit(f"git {' '.join(args)} failed in {cwd}:\n{result.stderr.strip()}")
    return result.stdout.strip()


def parse_git_version(text):
    """The version numbers in `git --version` output: "git version 2.38.1" -> (2, 38, 1). Empty when there are none."""
    match = re.search(r"(\d+)\.(\d+)(?:\.(\d+))?", text)
    return tuple(int(part) for part in match.groups(default="0")) if match else ()


@functools.lru_cache(maxsize=1)
def git_version():
    """The local git's version as numbers, e.g. (2, 38, 1); empty when it cannot be read."""
    try:
        result = subprocess.run(["git", "--version"], capture_output=True, text=True)
    except OSError:
        return ()
    return parse_git_version(result.stdout)


def require_git_version(use=GIT_VERSION_USE):
    """The local git's version, or a plain failure when it is too old for what `use` needs."""
    version = git_version()
    if version < MIN_GIT_VERSION:
        found = ".".join(str(part) for part in version) if version else None
        detail = f"this git is {found}." if found else "this git's version could not be read."
        raise SystemExit(f"git {'.'.join(str(part) for part in MIN_GIT_VERSION)} or newer is required: {use}; {detail}")
    return version


def worktrees():
    found, current = [], {}
    for line in git("worktree", "list", "--porcelain").splitlines() + [""]:
        if not line:
            if current:
                found.append(current)
            current = {}
        elif line.startswith("worktree "):
            current["path"] = Path(line[len("worktree "):])
        elif line.startswith("branch "):
            current["branch"] = line[len("branch refs/heads/"):]
    return found


def integration(base):
    for tree in worktrees():
        if tree.get("branch") == base:
            return tree["path"]
    raise SystemExit(f"No worktree has {base} checked out; create one with: git worktree add <path> {base}")


def count(range_, cwd):
    return int(git("rev-list", "--count", range_, cwd=cwd, check=False) or 0)


def status(args):
    base_tree = integration(args.base)
    print(f"{'worktree':<16} {'branch':<26} {'ahead':>5} {'behind':>6} {'dirty':>5} {'unpushed':>8}  gate")
    for tree in worktrees():
        path, branch = tree["path"], tree.get("branch", "(detached)")
        if path.parent != base_tree.parent and path != base_tree:
            continue
        dirty = len(git("status", "--porcelain", cwd=path).splitlines())
        remote = f"origin/{args.push_to if branch == args.base else branch}"
        unpushed = count(f"{remote}..HEAD", path) if git("rev-parse", "--verify", "--quiet", remote, cwd=path, check=False) else "new"
        gate = path / ".local" / "gate.json"
        verdict = "never run"
        if gate.exists():
            result = json.loads(gate.read_text())
            verdict = ("passed" if result["passed"] else "failed") + ("" if result["parity"] else " (no harnesses)")
            if result["commit"] != git("rev-parse", "HEAD", cwd=path) or result["dirty"]:
                verdict += ", older commit"
        print(f"{path.name:<16} {branch:<26} {count(f'{args.base}..HEAD', path):>5} {count(f'HEAD..{args.base}', path):>6}"
              f" {dirty:>5} {unpushed:>8}  {verdict}")


def new(args):
    base_tree = integration(args.base)
    path = base_tree.parent / args.area
    branch = f"parity/{args.area}"
    if path.exists():
        raise SystemExit(f"{path} already exists")
    git("worktree", "add", "-b", branch, str(path), args.base)
    shared = base_tree.parent / "shared-local"
    (path / ".local").mkdir()
    # Link the vanilla data, not the shared runner and datafix state beside it.
    for entry in sorted(shared.iterdir()) if shared.is_dir() else []:
        if entry.is_dir() and entry.name not in {"parity-shared", "datafix"}:
            (path / ".local" / entry.name).symlink_to(entry)
    print(f"{path} on {branch}, from {args.base} at {git('rev-parse', '--short', 'HEAD', cwd=path)}")


def save(args):
    base_tree = integration(args.base)
    for tree in worktrees():
        path, branch = tree["path"], tree.get("branch")
        if not branch or not branch.startswith("parity/") or path.parent != base_tree.parent:
            continue
        if args.areas and path.name not in args.areas:
            continue
        if git("status", "--porcelain", cwd=path):
            git("add", "-A", cwd=path)
            git("commit", "-q", "-m", f"WIP: unfinished {path.name} parity work\n\nWork in progress saved when its agent "
                "stopped; may not build or pass its harnesses yet.\n\n"
                "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>", cwd=path)
            print(f"{path.name}: committed unfinished work")
        git("push", "-q", "origin", f"HEAD:{branch}", cwd=path)
        print(f"{path.name}: {branch} pushed at {git('rev-parse', '--short', 'HEAD', cwd=path)}")


def trial_merge(onto, branch):
    """Merge branch onto the commit onto without touching any worktree: (merged commit or None, conflicting paths)."""
    require_git_version()
    result = subprocess.run(["git", "merge-tree", "--write-tree", "--name-only", "--no-messages", onto, branch],
                            cwd=ROOT, capture_output=True, text=True)
    lines = result.stdout.split()
    if result.returncode == 0:
        return git("commit-tree", lines[0], "-p", onto, "-p", branch, "-m", f"Trial merge of {branch}"), []
    if result.returncode == 1:
        return None, lines[1:]
    raise SystemExit(f"git merge-tree {onto} {branch} failed:\n{result.stderr.strip()}")


def largest_compatible(branches, conflicts):
    """The largest subset of branches with no conflicting pair (earlier branches win ties)."""
    best = []

    def search(chosen, index):
        nonlocal best
        if len(chosen) + len(branches) - index <= len(best):
            return
        if index == len(branches):
            best = list(chosen)
            return
        branch = branches[index]
        if not any(frozenset((branch, other)) in conflicts for other in chosen):
            search(chosen + [branch], index + 1)
        search(chosen, index + 1)

    search([], 0)
    return best


def plan(base, branches, verbose=True):
    """Trial-merge the branches into base: the largest set that merges cleanly, in order, and its merged commit."""
    say = print if verbose else (lambda *_: None)
    started = time.monotonic()
    base_commit = git("rev-parse", base)
    candidates, alone = [], {}
    for branch in branches:
        ahead = count(f"{base_commit}..{branch}", ROOT)
        if not ahead:
            say(f"  {branch:<28} nothing new")
            continue
        merged, files = trial_merge(base_commit, branch)
        alone[branch] = merged
        if merged:
            candidates.append(branch)
            say(f"  {branch:<28} {ahead:>3} commits, merges cleanly")
        else:
            say(f"  {branch:<28} {ahead:>3} commits, CONFLICTS with {base}: {', '.join(files)}")
    conflicts = {}
    for index, first in enumerate(candidates):
        for second in candidates[index + 1:]:
            merged, files = trial_merge(alone[first], second)
            if not merged:
                conflicts[frozenset((first, second))] = files
                say(f"  {first} and {second} conflict: {', '.join(files)}")
    chosen, head = [], base_commit
    for branch in largest_compatible(candidates, conflicts):
        merged, files = trial_merge(head, branch)
        if merged:
            chosen.append(branch)
            head = merged
        else:
            say(f"  {branch} conflicts with {', '.join(chosen)} together: {', '.join(files)}")
    say(f"{len(chosen)} of {len(candidates) + sum(1 for b in alone if not alone[b])} branches merge cleanly together"
        f"{': ' + ', '.join(chosen) if chosen else ''} (checked in {time.monotonic() - started:.1f}s)")
    return chosen, head


def ready_branches(base):
    """Every parity/* branch with commits the integration branch lacks."""
    names = git("for-each-ref", "--format=%(refname:short)", "refs/heads/parity/").splitlines()
    return [name for name in names if name != base and count(f"{base}..{name}", ROOT)]


def check(args):
    branches = args.branches or ready_branches(args.base)
    print(f"Trial merges into {args.base} at {git('rev-parse', '--short', args.base)} (committed work only):")
    plan(args.base, branches)


def integrate(args):
    if not args.branches and not args.ready:
        raise SystemExit("Name the branches to integrate, or pass --ready for every parity/* branch with new commits.")
    print(f"Trial merges into {args.base}:")
    chosen, _ = plan(args.base, args.branches or ready_branches(args.base))
    if not chosen:
        print("Nothing to integrate.")
        return 2 if args.branches else 0
    landed, failed = integrate_set(args, chosen)
    print(f"Integrated: {', '.join(landed) or 'nothing'}. Not integrated: {', '.join(failed) or 'none'}.")
    return 1 if failed else 0


def integrate_set(args, branches):
    """Merge and gate branches together; if the gate fails, try each half. Returns (integrated, rejected)."""
    landed = integrate_once(args, branches)
    if landed is not None:
        return landed, []
    if len(branches) == 1 or args.no_bisect:
        return [], branches
    half = len(branches) // 2
    print(f"Trying {', '.join(branches[:half])} and {', '.join(branches[half:])} apart.")
    first, first_failed = integrate_set(args, branches[:half])
    second, second_failed = integrate_set(args, branches[half:])
    return first + second, first_failed + second_failed


def integrate_once(args, branches):
    """Merge branches into the integration worktree and gate them: the branches integrated, or None if the gate failed."""
    args = argparse.Namespace(**{**vars(args), "branches": branches})
    base_tree = integration(args.base)
    if git("status", "--porcelain", "--untracked-files=no", cwd=base_tree):
        raise SystemExit(f"{base_tree} has uncommitted changes; commit or stash them first")
    before = git("rev-parse", "HEAD", cwd=base_tree)
    state = base_tree / ".local/parity-runner/state.json"
    known = sorted(name for name, record in json.loads(state.read_text())["harnesses"].items()
                   if record["status"] != "pass") if state.exists() else []
    for branch in args.branches:
        merge = subprocess.run(["git", "merge", "--no-ff", "--no-edit", branch], cwd=base_tree, capture_output=True, text=True)
        if merge.returncode:
            conflicts = git("diff", "--name-only", "--diff-filter=U", cwd=base_tree, check=False)
            git("merge", "--abort", cwd=base_tree, check=False)
            git("reset", "--hard", before, cwd=base_tree)
            print(f"{branch} does not merge cleanly into {args.base}. Conflicts:\n{conflicts or merge.stdout + merge.stderr}")
            return None
    names = ", ".join(args.branches)
    if git("rev-parse", "HEAD", cwd=base_tree) == before:
        print(f"{names}: nothing new for {args.base}.")
        return []
    # The flake check covers what these branches bring: harnesses changed since the current integration branch.
    gate = [sys.executable, "tools/gate.py", "--known-failures", ",".join(known), "--flake-base", before]
    gate += ["-j", str(args.jobs)] if args.jobs is not None else []
    gate += ["--no-parity"] if args.no_parity else []
    if subprocess.run(gate, cwd=base_tree).returncode:
        git("reset", "--hard", before, cwd=base_tree)
        print(f"The gate failed, so {names} were not integrated; {args.base} is back at {before[:9]}.")
        return None
    git("push", "-q", "origin", f"{args.base}:{args.push_to}", cwd=base_tree)
    print(f"Integrated {names}; {args.base} pushed to origin/{args.push_to} at {git('rev-parse', '--short', 'HEAD', cwd=base_tree)}.")
    return list(args.branches)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base", default="parity-base", help="the integration branch")
    parser.add_argument("--push-to", default="t3/achieve-full-parity", help="remote branch the integration branch is pushed to")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("status").set_defaults(run=status)
    command = commands.add_parser("new")
    command.add_argument("area")
    command.set_defaults(run=new)
    command = commands.add_parser("save")
    command.add_argument("areas", nargs="*")
    command.set_defaults(run=save)
    command = commands.add_parser("check")
    command.add_argument("branches", nargs="*", help="default: every parity/* branch with new commits")
    command.set_defaults(run=check)
    command = commands.add_parser("integrate")
    command.add_argument("branches", nargs="*", help="the largest set of them that merges cleanly is merged in order, "
                                                     "then gated together")
    command.add_argument("--ready", action="store_true", help="every parity/* branch with new commits")
    command.add_argument("--no-bisect", action="store_true", help="if the gate fails, don't retry halves of the set")
    command.add_argument("-j", "--jobs", type=int, help="harnesses at once on this machine (default: tools/parity.py's)")
    command.add_argument("--no-parity", action="store_true", help="skip the vanilla harnesses in the gate")
    command.set_defaults(run=integrate)
    args = parser.parse_args()
    return args.run(args) or 0


if __name__ == "__main__":
    sys.exit(main())
