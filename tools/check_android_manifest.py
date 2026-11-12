#!/usr/bin/env python3
"""Android compiles the shared gameplay sources in its own Cargo workspace.
Catch newly required direct dependencies before an expensive cross-build.
"""
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def missing_dependencies(game, android):
    missing = []
    for section in ("dependencies", "build-dependencies"):
        required = {name for name, spec in game.get(section, {}).items()
                    if not isinstance(spec, dict) or not spec.get("optional", False)}
        missing.extend(f"{section}: {name}" for name in sorted(required - android.get(section, {}).keys()))
    return missing

def main():
    def manifest(name):
        return tomllib.loads((ROOT / "crates" / name / "Cargo.toml").read_text())
    missing = missing_dependencies(manifest("horizon-client"), manifest("horizon-android"))
    if missing:
        print("Android is missing shared gameplay dependencies: " + ", ".join(missing), file=sys.stderr)
        return 1
    print("Android shared gameplay dependencies are present.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
