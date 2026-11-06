#!/usr/bin/env python3
"""Compare adult/baby Happy Ghast leash dispatch and forces with pinned vanilla.

A pre-existing path and move-control target expose unwanted close-range navigation
and the elastic-pull callback. Cow cases retain the ordinary six/twelve-block leash.
This checks one leash update, not a rendered flight trajectory.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

try:
    from . import lab
    from .integration_env import game_environment
except ImportError:
    import lab
    from integration_env import game_environment


def main():
    work = Path(tempfile.mkdtemp(prefix="happy-ghast-leash-", dir=lab.LOCAL))
    env = game_environment(os.environ, work)
    env["JAVA_TOOL_OPTIONS"] = env.get("JAVA_TOOL_OPTIONS", "") + " -Dhorizon.happyGhastLeashReference=true"
    subprocess.run([sys.executable, str(lab.ROOT / "tools/test_command_exec_parity.py"),
                    "--only", "scoreboard", "--reference-only", "--work-dir", str(work)],
                   cwd=lab.ROOT, env=env, check=True)
    reference = work / "happy-ghast-leash-reference.json"
    rows = json.loads(reference.read_text())
    if len(rows) != 36 or sum(r["snapped"] for r in rows) != 7:
        raise SystemExit("vanilla fixture must cover 36 leash cases and seven snapped leads")
    env["HORIZON_HAPPY_GHAST_LEASH_REFERENCE"] = str(reference)
    env["HORIZON_TEST_RESOURCES"] = os.environ.get("HORIZON_TEST_RESOURCES", str(lab.RESOURCES))
    subprocess.run(["cargo", "test", "-p", "horizon-client-lib", "--lib",
                    "mobs::leash::tests::happy_ghast_leash_dispatch_matches_vanilla",
                    "--", "--exact", "--ignored"], cwd=lab.ROOT, env=env, check=True)
    print("Happy Ghast leash: 36 adult/baby/cow dispatch, threshold and force cases match vanilla")


if __name__ == "__main__":
    main()
