#!/usr/bin/env python3
"""Run a Windows launcher beside unrelated injector DLL names, without opening a window."""

import argparse
import ctypes
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from check_imports import pe_imports


def check(binary):
    binary = Path(binary).resolve()
    # An imported DLL makes the bait discriminating: an unhardened launcher fails before main.
    imports = {name.lower() for name in pe_imports(binary.read_bytes())}
    if "version.dll" not in imports:
        raise RuntimeError("launcher no longer imports version.dll; update the DLL loading fixture")
    with tempfile.TemporaryDirectory(prefix="horizon-dll-search-") as scratch:
        root = Path(scratch)
        exe_dir, cwd = root / "bin", root / "cwd"
        exe_dir.mkdir()
        cwd.mkdir()
        env = os.environ.copy()
        for key in (
            "HORIZON_HOME", "HORIZON_LAUNCHER_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME",
            "HORIZON_LAUNCHER_TRASH", "HORIZON_LAUNCHER_CLIPBOARD", "HORIZON_LAUNCHER_DESKTOP",
        ):
            env[key] = str(root / key)
        env.update(HORIZON_LAUNCHER_NO_SELF_UPDATE="1", HORIZON_NO_AUDIO="1",
                   HORIZON_TELEMETRY_URL="off")

        def version(exe):
            result = subprocess.run([str(exe), "--version"], cwd=cwd, env=env,
                                    capture_output=True, text=True, timeout=30, check=True)
            if not result.stdout.startswith("horizon-launcher "):
                raise RuntimeError(f"unexpected launcher version: {result.stdout!r}")
            return result.stdout

        exe = exe_dir / "horizon-launcher.exe"
        shutil.copyfile(binary, exe)
        expected = version(exe)
        # Invalid DLLs fail immediately if Windows consults either untrusted folder.
        for directory in (exe_dir, cwd):
            for name in ("version.dll", "winmm.dll"):
                (directory / name).write_bytes(b"unrelated injector DLL fixture\n")
        for name in ("horizon-launcher.exe", "horizon-launcher-setup.exe"):
            target = exe_dir / name
            if target != exe:
                shutil.copyfile(binary, target)
            if version(target) != expected:
                raise RuntimeError(f"{name}: version changed beside unrelated DLLs")
        print("launcher and setup start beside unrelated version.dll and winmm.dll")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("this regression test requires Windows")
    # Child processes inherit this mode: loader failures must fail CI without a modal dialog.
    previous = ctypes.windll.kernel32.SetErrorMode(0x8003)
    try:
        check(args.binary)
    finally:
        ctypes.windll.kernel32.SetErrorMode(previous)


if __name__ == "__main__":
    main()
