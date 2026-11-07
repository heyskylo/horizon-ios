#!/usr/bin/env python3
"""The fork's `/claim` bot: upstream's claim_command.py with this fork's owner and reviewers (docs/fork-workflow.md)."""
from __future__ import annotations

import os
import runpy
import sys
from pathlib import Path

CI = Path(__file__).resolve().parents[1] / "ci"
sys.path.insert(0, str(CI))
import nightly_set as ns  # noqa: E402

ns.OWNER = os.environ["FORK_OWNER"]
ns.OWNER_ID = int(os.environ["FORK_OWNER_ID"])
ns.REVIEWERS = ".github/fork-reviewers.txt"

if __name__ == "__main__":
    sys.argv[0] = str(CI / "claim_command.py")
    runpy.run_path(str(CI / "claim_command.py"), run_name="__main__")
