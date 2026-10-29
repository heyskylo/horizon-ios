#!/bin/bash
# Daily on the Mac mini, from launchd (tools/ci/dev.horizon.version-port.plist; docs/version-ports.md, "Detect"):
# when Mojang releases a Minecraft version Horizon doesn't pin, port it as far as the report: prepare, diff,
# regen (ports/NEW on the local branch port/NEW), every harness against it, the report, and publish's dry run.
# It never publishes (version_port.py publish --yes stays a person's call) and never pushes.
#
# It runs from a copy taken from main, never from a checkout (it updates its own worktree of origin/main):
#   curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/tools/ci/version-port-daily.sh \
#     -o ~/code/horizon-parity/version-port-daily.sh
#
# HORIZON_REPO       the clone to make the worktree from (default ~/code/horizon)
# HORIZON_PORT_MAIN  the worktree of origin/main it works in (default ~/code/horizon-parity/port-main)
# HORIZON_PORT_REF   what that worktree checks out (default origin/main)
# HORIZON_PORT_VERSION=V  port release V now, without asking Mojang what is new (by hand, or to replay one)
# HORIZON_PORT_SNAPSHOTS=1  snapshots too: prepared, diffed and reported only (no regen, no harnesses)
# HORIZON_PORT_KIRA_CARD=N  post a note on Kira card N when a port report is ready
set -euo pipefail
repo=${HORIZON_REPO:-$HOME/code/horizon}
root=${HORIZON_PORT_MAIN:-$HOME/code/horizon-parity/port-main}
ref=${HORIZON_PORT_REF:-origin/main}
shared=$HOME/code/horizon-parity/shared-local
log() { echo "$(date -u +%FT%TZ) $*"; }

if [ ! -e "$root/.git" ]; then
  git -C "$repo" fetch -q origin
  git -C "$repo" worktree add -q --detach "$root" "$ref"
fi
cd "$root"
git fetch -q origin
git checkout -q --detach "$ref"
if [ ! -d .local ]; then
  mkdir .local
  for d in minecraft horizon-client tools datagen; do ln -s "$shared/$d" ".local/$d"; done
fi
# shellcheck disable=SC1091
source "$HOME/code/horizon-parity/env.sh"

# --dry-run: what is new is remembered (versions/detect-state.json) only once its port went through, so a port
# that fails is tried again the next day.
if [ -n "${HORIZON_PORT_VERSION:-}" ]; then
  kind=release new=$HORIZON_PORT_VERSION
else
  set +e
  python3 tools/version_port.py detect --dry-run ${HORIZON_PORT_SNAPSHOTS:+--snapshots} > "$root/.local/detect.txt"
  found=$?
  set -e
  cat "$root/.local/detect.txt"
  case $found in
    0) exit 0 ;;
    10) kind=release ;;
    11) kind=snapshot ;;
    *) log "detect failed ($found)"; exit "$found" ;;
  esac
  new=$(sed -n "s/^New $kind: \([^ ]*\) .*/\1/p" "$root/.local/detect.txt" | head -1)
fi
old=$(python3 -c 'import json; print(json.load(open("research-lock.json"))["minecraft"]["version"])')
log "Minecraft $new ($kind) is out; Horizon pins $old"

if [ "$kind" = snapshot ]; then
  python3 tools/version_port.py port "$new" --no-regen
else
  python3 tools/version_port.py port "$new"   # prepare both, diff, regen, report
  # The baseline: every harness on main against the pinned version (what passed before is not rerun).
  python3 tools/parity.py > "$root/.local/baseline.log" 2>&1 || true
  python3 tools/version_port.py harnesses "$new" --old "$old" --baseline .local/parity-runner/state.json
  python3 tools/version_port.py report "$old" "$new" --lines 30
  python3 tools/version_port.py publish "$old" "$new" --check-github --show 0
fi
[ -n "${HORIZON_PORT_VERSION:-}" ] || python3 tools/version_port.py detect ${HORIZON_PORT_SNAPSHOTS:+--snapshots} > /dev/null || true
report=${HORIZON_VERSIONS:-$HOME/code/horizon-parity/versions}/diff-$old-$new/report.md
log "Port report: $report"
if [ -n "${HORIZON_PORT_KIRA_CARD:-}" ]; then
  text="Minecraft $new ($kind) is out. Port report ready on the Mac mini ($report); publish's dry run is in the log."
  node "$HOME/code/horizon-parity/tools/kira.mjs" card_note \
    "$(python3 -c 'import json,sys; print(json.dumps({"ref": "card " + sys.argv[1], "text": sys.argv[2]}))' \
      "$HORIZON_PORT_KIRA_CARD" "$text")" > /dev/null || true
fi
