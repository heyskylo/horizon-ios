#!/usr/bin/env python3
"""Fail when a Windows release executable's PE machine doesn't match its named architecture."""

import argparse
import struct
import sys
from pathlib import Path

MACHINES = {"x64": 0x8664, "arm64": 0xAA64}


def pe_machine(data):
    """The COFF machine value in a PE image, after checking its DOS and PE signatures."""
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("not a PE executable (missing MZ header)")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if pe + 6 > len(data) or data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE executable (missing PE signature)")
    return struct.unpack_from("<H", data, pe + 4)[0]


def check(path, architecture):
    expected = MACHINES[architecture]
    try:
        actual = pe_machine(Path(path).read_bytes())
    except (OSError, ValueError, struct.error) as error:
        print(f"{path}: {error}", file=sys.stderr)
        return False
    if actual != expected:
        print(f"{path}: PE machine 0x{actual:04x}, expected {architecture} (0x{expected:04x})", file=sys.stderr)
        return False
    print(f"{path}: {architecture} (PE machine 0x{actual:04x})")
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("architecture", choices=MACHINES)
    parser.add_argument("binaries", nargs="+")
    args = parser.parse_args()
    sys.exit(0 if all(check(path, args.architecture) for path in args.binaries) else 1)


if __name__ == "__main__":
    main()
