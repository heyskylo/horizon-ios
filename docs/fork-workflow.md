# Working on sunrise-org/horizon (the perf platform fork)

The perf platform ([perf-platform.md](perf-platform.md)) is built on this fork. Its default branch is `perf-platform`:
upstream `main` plus the platform's work. The fork's `main` stays an exact mirror of upstream `main`.

## Taking a piece

1. Pick an open piece from the tracker, sunrise-org/horizon#1, whose "needs first" pieces are merged.
2. Comment `/claim` on it (an agent writes `🤖 Agent: /claim`). The bot assigns you, or says why not: someone holds it,
   it is the tracker, or you hold 8 open claims. `/unclaim` gives it back.
3. A claim with no open pull request linking the issue is released after 5 days.

## Opening a pull request

- Branch from `perf-platform`, open the pull request into `perf-platform`, and write `Fixes #N` for the piece.
- PR size is not limited for this project.
- Follow the repo's CONTRIBUTING.md and CLAUDE.md (attribution lines, `python3 tools/pr_ready.py` block, tests with
  signal, one-line comments).
- The `checks` workflow runs `verify` on GitHub's runners. Parity harnesses that need the vanilla game do not run in
  the fork's CI; the reviewer runs the ones the change can affect.
- Keep it a draft until you want a review. Marking it ready is the request.

## How reviews happen

`fork review batch` (`tools/fork/review_batch.py`) runs every 30 minutes and after every `checks` run. It keeps one
issue, labelled `review-batch`, listing:

- **ready**: not a draft, no conflict, no `needs-changes` label, every check green;
- **waiting**: everything else, with the reason.

A batch is due when 5 ready pull requests are new to it, or the oldest has waited 6 hours. The issue then gets the
`batch-ready` label and a comment naming each pull request at its head. The reviewer processes the batch:

- all green: merged into `perf-platform`;
- something to fix: a review comment, and the `needs-changes` label. Pushing a new commit removes the label, and the
  pull request comes back in a later batch.

## Reviewer runbook (the scheduled agent run)

1. If the `review-batch` issue has no `batch-ready` label, stop.
2. For each pull request named in the latest "Review batch due" comment, in order, one subagent each, in its own
   worktree:
   - check the head still matches the comment; skip it if it moved (it comes back next batch);
   - review the diff against the design doc and the repo's rules;
   - run `python3 tools/pr_ready.py` against `perf-platform`, plus the parity harnesses it selects;
   - report: verdict (`merge` or `changes`), findings, test results.
3. Merge the `merge` ones into `perf-platform` in order (`gh pr merge N --repo sunrise-org/horizon --merge`), rebasing or
   re-testing a later one if an earlier merge conflicts with it. Then
   `python3 tools/fork/review_batch.py reviewed N HEAD merged`.
4. For the `changes` ones: post the findings as a review starting with `🤖 Review by UMCEKO's AI agent`, then
   `python3 tools/fork/review_batch.py reviewed N HEAD changes`.
5. Remove the `batch-ready` label and comment a one-line summary on the batch issue: merged, sent back, skipped.

## Keeping up with upstream

`perf-platform` merges upstream `main` regularly (at least weekly). Upstream pull requests are cut from
`perf-platform` with only the platform's commits: the fork's own files (`tools/fork/`, `.github/workflows/fork-*.yml`,
`.github/fork-reviewers.txt`, this page) never go upstream.

## Settings this fork relies on

- Repository variables: `FORK_OWNER` (login) and `FORK_OWNER_ID` (numeric ID) for the claim bot; `SELF_HOSTED_CI=off`
  so `checks` uses GitHub's runners.
- Upstream's own workflows (releases, nightly, parity runners, upstream's claim and triage commands) are disabled on
  the fork; only `checks`, `cancel-on-close` and the `fork-*` workflows run.
