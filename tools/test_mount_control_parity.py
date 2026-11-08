#!/usr/bin/env python3
"""Compare pig/strider client authority with the real 26.3 controlling-passenger methods.

The ignored Rust fixture exercises actual client prediction and MoveVehicle packet
selection, plus the server's moved-by-client guard, without a window or network server.
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
    work = Path(tempfile.mkdtemp(prefix="mount-control-", dir=lab.LOCAL))
    env = game_environment(os.environ, work)
    env["JAVA_TOOL_OPTIONS"] = env.get("JAVA_TOOL_OPTIONS", "") + " -Dhorizon.mountControlReference=true"
    subprocess.run([sys.executable, str(lab.ROOT / "tools/test_command_exec_parity.py"),
                    "--only", "scoreboard", "--reference-only", "--work-dir", str(work)],
                   cwd=lab.ROOT, env=env, check=True)
    reference = work / "mount-control-reference.json"
    rows = json.loads(reference.read_text())
    if (len(rows) != 32 or sum(r["controlled"] for r in rows) != 4
            or sum(not r["server_simulates"] for r in rows) != 4):
        raise SystemExit("vanilla mount-control fixture did not cover 32 cases / four controllers")
    env["HORIZON_MOUNT_CONTROL_REFERENCE"] = str(reference)
    env["HORIZON_TEST_RESOURCES"] = os.environ.get("HORIZON_TEST_RESOURCES", str(lab.RESOURCES))
    subprocess.run(["cargo", "test", "-p", "horizon-client", "--lib",
                    "game::remote_vehicles::tests::steering_mount_control_matches_vanilla",
                    "--", "--exact", "--ignored"], cwd=lab.ROOT, env=env, check=True)
    print("mount control: 32 vanilla cases match; four controllers predict forward movement and send MoveVehicle")


if __name__ == "__main__":
    main()
