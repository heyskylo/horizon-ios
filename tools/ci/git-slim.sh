#!/usr/bin/env bash
# Keeps the .git of a runner's persistent checkout from growing without bound.
#
#   bash tools/ci/git-slim.sh [checkout]      # default: $GITHUB_WORKSPACE, else the current directory
#
# Why: our self-hosted runners keep their checkout between jobs, and actions/checkout runs `git config gc.auto 0` in it
# and then fetches into it (every branch and tag, plus the pull request's merge commit) at every job. Nothing ever
# repacks, so each fetch adds a pack, and a fetch cut short by a cancelled job or a runner restart leaves its
# `tmp_pack_*` behind. On 2026-11-03 playhorizon's light runners held 6822 packs (50 GB) each; on 2026-11-09 the desktop's
# checks runners held 1051 packs (checks-3: 111 GB, checks-1: 33 GB) and filled its disk.
#
# Run it where nothing fetches at the same time: before a job's checkout (a runner hook: ACTIONS_RUNNER_HOOK_JOB_STARTED)
# or when the runner is idle (tools/ci/runner-gc.sh). It never touches the working tree, the build folders or a ref.
#   - `tmp_pack_*` and `tmp_idx_*` files older than TMP_MIN minutes (default 30) go: no fetch is running to own them.
#   - A .git over MAX_GIT_MB (default 5120) is collected with `git gc --prune=now`: one pack again, the objects no ref
#     reaches (the pull requests' merge commits of earlier jobs, force-pushed branches) gone.
#   - Otherwise, more than MAX_PACKS packs (default 40) are merged with a geometric repack: the big pack stays as it is,
#     only the many small ones fold into each other.
# Prints one line per action; exits 0 when there is nothing to do or the folder is no repository.
set -euo pipefail

dir=${1:-${GITHUB_WORKSPACE:-.}}
max_mb=${MAX_GIT_MB:-5120}
max_packs=${MAX_PACKS:-40}
tmp_min=${TMP_MIN:-30}

git_dir=$(git -C "$dir" rev-parse --absolute-git-dir 2> /dev/null) || exit 0
pack_dir=$git_dir/objects/pack
[ -d "$pack_dir" ] || exit 0

size_kb() { du -sk "$git_dir" | cut -f1; }
size_mb() { echo $(( $(size_kb) / 1024 )); }
pack_count() { find "$pack_dir" -maxdepth 1 -name '*.pack' | wc -l | tr -d ' '; }

stale=$(find "$pack_dir" -maxdepth 1 -type f \( -name 'tmp_pack_*' -o -name 'tmp_idx_*' -o -name '.tmp-*' \) -mmin +"$tmp_min" -print)
if [ -n "$stale" ]; then
  freed=$(printf '%s\n' "$stale" | xargs du -ck | tail -1 | awk '{print int($1 / 1024)}')
  printf '%s\n' "$stale" | while IFS= read -r f; do rm -f "$f"; done
  echo "git-slim: removed $(printf '%s\n' "$stale" | wc -l | tr -d ' ') leftovers of cut-short fetches (${freed} MB) from $git_dir"
fi

before=$(size_mb)
before_kb=$(size_kb)
packs=$(pack_count)
# One pack at a time, a few threads: the runner's machine is shared (the desktop's CI slice, the Mac mini's builds).
git_gc=(git -C "$dir" -c pack.threads="${GIT_SLIM_THREADS:-2}" -c gc.auto=0)
if [ "$before_kb" -gt $(( max_mb * 1024 )) ]; then
  "${git_gc[@]}" gc --prune=now --quiet
  echo "git-slim: $git_dir was ${before} MB in ${packs} packs (over ${max_mb}): git gc, now $(size_mb) MB in $(pack_count) pack(s)"
elif [ "$packs" -gt "$max_packs" ]; then
  "${git_gc[@]}" repack -d -q --geometric=2
  echo "git-slim: $git_dir had ${packs} packs (over ${max_packs}): repacked to $(pack_count), ${before} -> $(size_mb) MB"
fi
