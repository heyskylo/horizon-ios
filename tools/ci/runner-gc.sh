#!/usr/bin/env bash
# Keeps the Mac mini's Horizon runners from filling its disk. launchd runs a copy taken from main
# (~/.actions-runners/runner-gc.sh, never a pull request's checkout) every 30 minutes; see docs/release-flow.md.
#   - A runner in the middle of a job is left alone.
#   - A checkout whose cargo build folders (target/ and the harness probes' targets under .local/, the
#     directories holding a CACHEDIR.TAG) pass MAX_TARGET_GB loses them: sccache (tools/ci/runner-env.sh)
#     makes the next build cheap. Symlinks into the shared vanilla data are never followed.
#   - Harness work folders under .local/ (vanilla exports, probe outputs: real directories, not the links into
#     the shared vanilla data) that nothing touched for STALE_DAYS go; a harness that runs again rebuilds its own.
#     .local/parity-runner (run state, oracle cache) stays.
#   - The shared prepared resources keep their KEEP_PREPARED newest preparations, only while no runner works.
#   - Java processes harnesses left behind (vanilla servers, oracles) go once ORPHAN_HOURS old: those whose parent
#     is gone (adopted by launchd, init or systemd --user) and whose command line names a runner's checkout or a
#     worktree's .local/. A job cancelled while it ran left such servers for days (2026-10-26); tools/parity.py now
#     stops its harnesses on SIGTERM, and this catches what it can't (SIGKILL, a harness run by hand).
set -euo pipefail
root=${RUNNERS:-$HOME/.actions-runners}
max_kb=$(( ${MAX_TARGET_GB:-12} * 1024 * 1024 ))
keep=${KEEP_PREPARED:-2}
stale_min=$(( ${STALE_DAYS:-3} * 24 * 60 ))
busy() { pgrep -f "$1/bin[^/]*/Runner.Worker" > /dev/null; }

any_busy=0
for runner in "$root"/horizon "$root"/horizon-[0-9]*; do
  [ -d "$runner/_work" ] || continue
  if busy "$runner"; then echo "$(basename "$runner"): in a job, skipped"; any_busy=1; continue; fi
  rm -rf "${runner:?}/_work/_temp/"* 2>/dev/null || true
  checkout="$runner/_work/horizon/horizon"
  [ -d "$checkout" ] || continue
  stale=0
  for dir in "$checkout"/.local/*/; do
    dir=${dir%/}
    [ -L "$dir" ] || [ "$(basename "$dir")" = parity-runner ] && continue
    if [ -z "$(find "$dir" -mmin -"$stale_min" -print -quit 2>/dev/null)" ]; then rm -rf "$dir"; stale=$((stale + 1)); continue; fi
    # Per-run scratch folders (<harness>-<8 random chars>, made by tempfile.mkdtemp) are never reused: 917 of them
    # filled the Mac on 2026-11-07. They go once untouched for 2 hours.
    if [[ $(basename "$dir") =~ -[a-z0-9_]{8}$ ]] && [ -z "$(find "$dir" -mmin -120 -print -quit 2>/dev/null)" ]; then
      rm -rf "$dir"; stale=$((stale + 1)); continue
    fi
    # A few harness folders reach 5+ GB (stored_effect_notifications, attribute_map_queries; the Mac fell to 1.3 GB
    # free on 2026-11-07): drop folders over 2 GB once untouched for 6 hours, not 3 days.
    if [ -z "$(find "$dir" -mmin -360 -print -quit 2>/dev/null)" ] && [ "$(du -sk "$dir" | cut -f1)" -gt 2097152 ]; then
      rm -rf "$dir"; stale=$((stale + 1))
    fi
  done
  [ "$stale" = 0 ] || echo "$(basename "$runner"): removed $stale harness folders untouched for ${STALE_DAYS:-3} days"
  targets=()
  [ -f "$checkout/target/CACHEDIR.TAG" ] && targets+=("$checkout/target")
  while IFS= read -r tag; do targets+=("$(dirname "$tag")"); done < <(
    find "$checkout/.local" -mindepth 2 -maxdepth 2 -name CACHEDIR.TAG -type f 2>/dev/null)
  # The probes' build folder lives in shared-local (a symlink from .local), which the scan above doesn't follow:
  # it grew to 19-24 GB per runner (2026-11-06, the Mac down to 6 GB free).
  [ -d "$runner/_work/horizon/shared-local/parity-target" ] && targets+=("$runner/_work/horizon/shared-local/parity-target")
  [ ${#targets[@]} -gt 0 ] || continue
  size=$(du -sk "${targets[@]}" | awk '{s += $1} END {print s}')
  if [ "$size" -gt "$max_kb" ]; then
    rm -rf "${targets[@]}"
    # .local/parity-target is a symlink into shared-local: cargo can't create a directory through a dangling
    # link ("Not a directory"), which failed every harness of parity-main on 2026-11-06. Recreate it empty.
    mkdir -p "$runner/_work/horizon/shared-local/parity-target" 2>/dev/null || true
    echo "$(basename "$runner"): removed ${#targets[@]} build folders ($(( size / 1024 / 1024 )) GB)"
  fi
done

# [[dd-]hh:]mm:ss, as ps prints a process's age, in minutes.
minutes() {
  local t=$1 d=0 h=0 m a b c
  if [[ $t == *-* ]]; then d=${t%%-*}; t=${t#*-}; fi
  IFS=: read -r a b c <<< "$t"
  if [ -n "$c" ]; then h=$a m=$b; else m=$a; fi
  echo $(( 10#$d * 1440 + 10#$h * 60 + 10#$m ))
}
orphan_min=$(( ${ORPHAN_HOURS:-3} * 60 ))
ps -A -o pid=,ppid=,etime=,command= | while read -r pid ppid etime command; do
  case "$command" in *java*) ;; *) continue ;; esac
  case "$command" in *"$root"/*/_work/*|*/.claude/worktrees/*/.local/*|*/code/horizon/.local/*) ;; *) continue ;; esac
  [ "$(minutes "$etime")" -ge "$orphan_min" ] || continue
  if [ "$ppid" != 1 ]; then
    case "$(ps -o command= -p "$ppid" 2>/dev/null)" in *"systemd --user"*) ;; *) continue ;; esac
  fi
  if kill "$pid" 2>/dev/null; then echo "orphaned java $pid ($etime old) stopped: ${command:0:160}"; fi
done

prepared="$root/horizon/_work/horizon/shared-local/prepared"
if [ "$any_busy" = 0 ] && [ -d "$prepared" ] && mkdir "$prepared/.lock" 2>/dev/null; then
  trap 'rmdir "$prepared/.lock" 2>/dev/null || true' EXIT
  ls -1t "$prepared" | tail -n +"$(( keep + 1 ))" | while read -r old; do
    rm -rf "${prepared:?}/$old"
    echo "prepared resources $old removed"
  done
fi
df -h "$root" | tail -1
