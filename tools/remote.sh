#!/usr/bin/env bash
# Run a command on a Linux build host (default playhorizon) at a worktree's commit, and get its output back.
#
#   tools/remote.sh [--host H] [--name N] [--cpus LIST] [--exclusive] [--jobs J] <worktree> <command...>
#   tools/remote.sh .claude/worktrees/my-fix cargo test -p horizon-world --lib
#   tools/remote.sh . python3 tools/parity.py food light
#   tools/remote.sh --cpus 0-3 --exclusive . python3 tools/perf_bench.py   # a benchmark: CPUs 0-3 to itself
#   tools/remote.sh --list                          # checkouts on the host, their size and last use
#   tools/remote.sh --gc [DAYS]                     # remove checkouts unused for DAYS (default 2)
#
# The host fetches the commit from GitHub itself: commit and push first (uncommitted changes are not sent, and
# nothing else leaves this machine: no files, no credentials, no environment). Each name (default: the worktree's
# folder) keeps one checkout there, ~/remote-work/<name>, with its own target/: the next run checks out the new
# commit over it, so only what changed rebuilds (sccache shares the registry crates between names). One run per
# name at a time. The command runs in bash at the checkout's root, niced in the host's CI slice, with the host's
# toolchains, a virtual screen and scratch homes for the game and launcher. .local/ links to the host's vanilla
# data; a command that runs harnesses (it names parity, lan_e2e or horizon-prepare) also gets .local/horizon-client,
# prepared as CI prepares it (tools/ci/vanilla-data.sh). Output streams back and is kept in ~/remote-work/<name>.logs/.
# What the command leaves running is stopped when it ends; stopping this script stops the command on the host.
# The exit status is the command's.
#
# CPUs: playhorizon's CI runner has CPUs 4-7 (a systemd drop-in pins it, docs/release-flow.md), so runs here get
# 0-3 by default and never slow a pull request's parity job. A run holds its CPU set's lock shared; --exclusive (a
# benchmark: name it with --cpus and say so in its pull request) holds it alone, so it starts only when no other run
# uses those CPUs, and the others wait for it to end. --cpus 4-7 or all compete with CI: only when it is idle.
#
# The host is lent by a partner who keeps hypervisor access: never send secrets, tokens or credentials there,
# and never treat a result from it as a CI result.
set -euo pipefail

host=playhorizon name="" jobs="" cpus=0-3 exclusive=0
usage() { sed -n '2,9s/^# \{0,1\}//p' "$0" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case $1 in
    --host) host=$2; shift 2;;
    --name) name=$2; shift 2;;
    --jobs) jobs=$2; shift 2;;
    --cpus) cpus=$2; shift 2;;
    --exclusive) exclusive=1; shift;;
    --list) exec ssh "ludovic@$host" 'cd ~/remote-work 2>/dev/null && for d in */; do d=${d%/};
      printf "%-32s %8s  last used %s\n" "$d" "$(du -sh "$d" 2>/dev/null | cut -f1)" "$(stat -c %y "$d.logs" 2>/dev/null | cut -c1-16)"; done';;
    --gc) days=${2:-2}; exec ssh "ludovic@$host" "cd ~/remote-work 2>/dev/null || exit 0
      find . -maxdepth 1 -name '*.logs' -mtime +$days | while read -r logs; do d=\${logs%.logs};
        flock -n \"\$d.lock\" rm -rf \"\$d\" \"\$logs\" \"\$d.lock\" && echo \"removed \${d#./}\"; done";;
    -h|--help) usage;;
    *) break;;
  esac
done
[ $# -ge 2 ] || usage
tree=$(cd "$1" && git rev-parse --show-toplevel); shift
sha=$(git -C "$tree" rev-parse HEAD)
name=${name:-$(basename "$tree")}
[[ $name =~ ^[A-Za-z0-9._-]+$ ]] || { echo "remote.sh: name '$name' must be letters, digits, . _ -" >&2; exit 2; }
if ! git -C "$tree" diff --quiet HEAD -- 2>/dev/null; then
  echo "remote.sh: uncommitted changes in $tree are not sent: the host runs ${sha:0:9}" >&2
fi
if [ -z "$(git -C "$tree" branch -r --contains "$sha" 2>/dev/null)" ]; then
  git -C "$tree" fetch -q origin 2>/dev/null || true
  [ -n "$(git -C "$tree" branch -r --contains "$sha" 2>/dev/null)" ] ||
    { echo "remote.sh: ${sha:0:9} is not on GitHub yet: push it first (the host fetches from GitHub)" >&2; exit 2; }
fi
command=$(printf '%q ' "$@")

# The host side reads its script and these four values on stdin (nothing is re-split by a remote shell); its
# own environment comes from the runner's files there.
{
[[ $cpus =~ ^[0-9,-]+$|^all$ ]] || { echo "remote.sh: --cpus takes a list such as 0-3, or all" >&2; exit 2; }
printf 'name=%q sha=%q jobs=%q cpus=%q exclusive=%q command=%q\n' "$name" "$sha" "$jobs" "$cpus" "$exclusive" "$command"
cat <<'REMOTE'
set -euo pipefail
if [ "$cpus" = all ]; then pin=(); jobs=${jobs:-$(nproc)}; else pin=(taskset -c "$cpus"); jobs=${jobs:-$(taskset -c "$cpus" nproc)}; fi
runner=~/.actions-runners/horizon
if [ -e "$runner/paused" ]; then echo "remote.sh: this host is paused ($runner/paused)" >&2; exit 75; fi
set -a; . "$runner/.env"; set +a
export PATH="$(cat "$runner/.path")" DISPLAY=":$(cat "$runner/display")" RUSTC_WRAPPER="$(command -v sccache || true)"
export CARGO_BUILD_JOBS=$jobs FLEET_LOAD_JOBS=$jobs PARITY_LOCAL_JOBS=$(( jobs > 1 ? jobs / 2 : 1 ))
unset WAYLAND_DISPLAY
mkdir -p ~/remote-work && cd ~/remote-work
exec 9> "$name.lock"
flock -n 9 || { echo "remote.sh: $name is busy with another run" >&2; exit 75; }
# The CPU set's lock: shared by ordinary runs, held alone by an --exclusive one (a benchmark).
exec 8> ".cpus-$cpus.lock"
if [ "$exclusive" = 1 ]; then
  flock -x -n 8 || { echo "remote.sh: other runs use CPUs $cpus; try again when they are done (--list)" >&2; exit 75; }
elif ! flock -s -n 8; then
  echo "remote.sh: an exclusive run (a benchmark) has CPUs $cpus; waiting for it to end" >&2
  flock -s 8
fi
if [ ! -d "$name/.git" ]; then
  git clone -q --filter=blob:none --no-checkout https://github.com/horizon-rust/horizon "$name" 8>&- 9>&-
fi
cd "$name"
# The lock's descriptor stays out of everything started here (8>&- 9>&-): a daemon one of them starts (sccache's server,
# from the preparer's build or the command's) would hold the lock after the run.
git fetch -q --no-tags origin "$sha" 8>&- 9>&-
git checkout -q --force --detach "$sha"
git clean -qfd -e /target -e /.local
# The host's vanilla data, linked as CI's runner links it; for harnesses also .local/horizon-client, prepared by
# this commit's own preparer (once per preparer version, kept beside the data and shared by every name).
if [[ $command =~ parity|lan_e2e|horizon-prepare ]]; then
  RUNNER_WORKSPACE=$(dirname "$HORIZON_SHARED_SOURCE") RUNNER_TEMP=$(mktemp -d) bash tools/ci/vanilla-data.sh >&2 8>&- 9>&-
else
  mkdir -p .local
  for d in minecraft tools datagen; do
    if [ -e "$HORIZON_SHARED_SOURCE/$d" ]; then ln -sfn "$HORIZON_SHARED_SOURCE/$d" ".local/$d"; fi
  done
fi
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export HORIZON_HOME=$scratch/game HORIZON_LAUNCHER_HOME=$scratch/launcher XDG_DATA_HOME=$scratch/data \
  XDG_CONFIG_HOME=$scratch/config HORIZON_NO_AUDIO=1 HORIZON_LAUNCHER_NO_SELF_UPDATE=1 HORIZON_TELEMETRY_URL=off
mkdir -p "$HORIZON_HOME" "$HORIZON_LAUNCHER_HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" ../"$name.logs"
log=../"$name.logs/$(date -u +%Y%m%dT%H%M%SZ)-${sha:0:9}.log"
echo "remote.sh: $(hostname) at ${sha:0:9} in ~/remote-work/$name, CPUs $cpus, -j $jobs; log ~/remote-work/${log#../}" >&2
# The command runs in a scope of its own, stopped with everything in it once the command is done (a game or a
# daemon it left behind), when the caller goes away first (the ssh session ends), and before the next run here.
unit=remote-$name.scope
systemctl --user stop "$unit" 2>/dev/null || true
session=$PPID
( while kill -0 "$session" 2>/dev/null; do sleep 2; done; systemctl --user stop "$unit" 2>/dev/null ) \
  < /dev/null > /dev/null 2>&1 8>&- 9>&- &
watcher=$!
status=0
systemd-run --user --scope --quiet --slice=horizon-ci.slice --unit="$unit" \
  "${pin[@]}" nice -n 10 bash -c "$command" 2>&1 < /dev/null 8>&- 9>&- | tee "$log" || status=${PIPESTATUS[0]}
kill "$watcher" 2>/dev/null || true
systemctl --user stop "$unit" 2>/dev/null || true
exit "$status"
REMOTE
} | ssh -o BatchMode=yes "ludovic@$host" bash -s &
# Stopping this script ends the session, and the host then stops the command (above).
trap 'kill $! 2>/dev/null' INT TERM HUP
wait $!
