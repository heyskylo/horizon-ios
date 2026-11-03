import contextlib
import io
from pathlib import Path
import struct
import tempfile
import unittest

from tools import check_imports


def elf(interpreter, libraries, runpath=None):
    data = bytearray(4096)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<Q", data, 32, 64)
    struct.pack_into("<HH", data, 54, 56, 3 if interpreter else 2)
    struct.pack_into("<IIQQQQQQ", data, 64, 1, 4, 0, 0x400000, 0, len(data), len(data), 4096)
    strings = bytearray(b"\0")
    entries = [(5, 0x400400)]
    for tag, value in [(1, lib) for lib in libraries] + ([(29, runpath)] if runpath else []):
        entries.append((tag, len(strings)))
        strings.extend(value.encode() + b"\0")
    entries.append((0, 0))
    struct.pack_into("<IIQQQQQQ", data, 120, 2, 4, 512, 0x400200, 0,
                     len(entries) * 16, len(entries) * 16, 8)
    if interpreter:
        raw = interpreter.encode() + b"\0"
        data[256:256 + len(raw)] = raw
        struct.pack_into("<IIQQQQQQ", data, 176, 3, 4, 256, 0x400100, 0, len(raw), len(raw), 1)
    for i, entry in enumerate(entries):
        struct.pack_into("<qQ", data, 512 + i * 16, *entry)
    data[1024:1024 + len(strings)] = strings
    return data


class LinuxImportsTests(unittest.TestCase):
    def check_binary(self, interpreter, libraries, *, runpath=None, **options):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "program"
            path.write_bytes(elf(interpreter, libraries, runpath))
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                return check_imports.check(path, **options)

    def test_musl_desktop(self):
        self.assertTrue(self.check_binary("/lib/ld-musl-x86_64.so.1",
                        ["libc.musl-x86_64.so.1", "libgcc_s.so.1", "libasound.so.2", "libudev.so.1"],
                        musl=True))

    def test_musl_requires_its_dynamic_loader(self):
        for loader in (None, "/lib64/ld-linux-x86-64.so.2", "/nix/store/musl/lib/ld-musl-x86_64.so.1"):
            with self.subTest(loader=loader):
                self.assertFalse(self.check_binary(loader, [], musl=True))

    def test_musl_rejects_glibc_and_unexpected_libraries(self):
        for library in ("libc.so.6", "libpthread.so.0", "libm.so.6", "libcustom.so"):
            with self.subTest(library=library):
                self.assertFalse(self.check_binary("/lib/ld-musl-x86_64.so.1", [library], musl=True))

    def test_glibc_policy_still_rejects_musl(self):
        self.assertTrue(self.check_binary("/lib64/ld-linux-x86-64.so.2", ["libc.so.6", "libasound.so.2"]))
        self.assertFalse(self.check_binary("/lib/ld-musl-x86_64.so.1", ["libc.musl-x86_64.so.1"]))

    def test_server_and_runpath_checks_apply_to_both_abis(self):
        for musl, loader in ((False, "/lib64/ld-linux-x86-64.so.2"), (True, "/lib/ld-musl-x86_64.so.1")):
            with self.subTest(musl=musl):
                self.assertTrue(self.check_binary(loader, ["libgcc_s.so.1"], musl=musl, server=True))
                for lib in ("libasound.so.2", "libudev.so.1"):
                    self.assertFalse(self.check_binary(loader, [lib], musl=musl, server=True))
                self.assertFalse(self.check_binary(loader, [], musl=musl, runpath="/nix/store/libs"))
