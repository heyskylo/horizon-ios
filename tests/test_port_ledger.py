import json
import os
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest import mock

from tests.test_class_index import Pool
from tools import port_ledger

SCRIPT = Path(port_ledger.__file__).resolve()


def class_bytes(name: str, parent: str, fields: tuple[str, ...] = (), methods: tuple[str, ...] = ()) -> bytes:
    """A small class file with declared int fields and abstract methods."""
    pool = Pool()
    this, super_ = pool.cls(name), pool.cls(parent)
    encoded_fields = [struct.pack(">HHHH", 1, pool.utf8(field), pool.utf8("I"), 0) for field in fields]
    encoded_methods = [struct.pack(">HHHH", 0x0401, pool.utf8(method), pool.utf8("()V"), 0) for method in methods]
    return (b"\xca\xfe\xba\xbe" + struct.pack(">HH", 0, 69) + pool.bytes()
            + struct.pack(">HHHH", 0x0421, this, super_, 0)
            + struct.pack(">H", len(encoded_fields)) + b"".join(encoded_fields)
            + struct.pack(">H", len(encoded_methods)) + b"".join(encoded_methods) + struct.pack(">H", 0))


class PortLedgerJarOnlyTests(unittest.TestCase):
    def test_jar_only_and_partial_sources_preserve_field_and_stale_citations(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rust = root / "crates/example/src/lib.rs"
            rust.parent.mkdir(parents=True)
            rust.write_text("/// Child.present\nfn a() {}\n"
                            "/// Child.inherited\nfn b() {}\n"
                            "/// Child.missing\nfn c() {}\n"
                            "/// Child.tick\nfn d() {}\n"
                            "/// Child.sourceOnly\nfn e() {}\n", encoding="utf-8")
            jar = root / "game.jar"
            with zipfile.ZipFile(jar, "w") as archive:
                archive.writestr("net/minecraft/test/Parent.class", class_bytes(
                    "net/minecraft/test/Parent", "java/lang/Object", fields=("inherited",), methods=("tick",)))
                archive.writestr("net/minecraft/test/Child.class", class_bytes(
                    "net/minecraft/test/Child", "net/minecraft/test/Parent", fields=("present",)))
            sources = root / "sources"
            parent = sources / "net/minecraft/test/Parent.java"
            parent.parent.mkdir(parents=True)
            parent.write_text("package net.minecraft.test;\npublic class Parent {\n int inherited;\n void tick() {}\n}\n",
                              encoding="utf-8")
            out = root / "output"
            with mock.patch.multiple(port_ledger, ROOT=root, OUT=out, LEDGER=out / "ledger.json",
                                     MAP_DIR=root / "maps", ORACLE_DIR=root / "oracles",
                                     ProcessPoolExecutor=ThreadPoolExecutor), \
                    mock.patch.object(port_ledger, "rust_files", return_value=["crates/example/src/lib.rs"]):
                for source_roots in ([], [str(sources)]):
                    with self.subTest(source_roots=source_roots):
                        result = port_ledger.build(source_roots, [str(jar)], 1)
                        stale = {g["cite"] for g in result["gaps"] if g["kind"] == "stale"}
                        self.assertIn("Child.missing", stale)
                        self.assertNotIn("Child.present", stale)
                        self.assertNotIn("Child.inherited", stale)
                        self.assertNotIn("Child.tick", stale)
                        self.assertIn("Child.sourceOnly", stale)
                        self.assertEqual(json.loads((out / "ledger.json").read_text())["sources"], source_roots)
                child = sources / "net/minecraft/test/Child.java"
                child.write_text("package net.minecraft.test;\npublic class Child extends Parent {\n"
                                 " int present;\n int sourceOnly;\n void tick() {}\n}\n", encoding="utf-8")
                for jars in ([str(jar)], []):
                    with self.subTest(jars=jars):
                        result = port_ledger.build([str(sources)], jars, 1)
                        stale = {g["cite"] for g in result["gaps"] if g["kind"] == "stale"}
                        self.assertNotIn("Child.present", stale)
                        self.assertNotIn("Child.sourceOnly", stale)
                        self.assertNotIn("Child.tick", stale)
                        self.assertIn("Child.missing", stale)


class PortLedgerCliTests(unittest.TestCase):
    """--json is read the same before or after the query's name (port_ledger.py's docstring)."""

    # A ledger with one class, enough for `where` to answer without a built ledger.
    LEDGER = {
        "classes": {"net/minecraft/world/Foo": {"file": "net/minecraft/world/Foo.java", "name": "Foo"}},
        "methods": {"net/minecraft/world/Foo#tick": {
            "class": "net/minecraft/world/Foo", "name": "tick", "file": "net/minecraft/world/Foo.java",
            "lines": [[10, 20]], "callees": [], "client": False}},
        "class_cites": {}, "ported": {}, "mentioned": {}, "inferred": {},
    }

    def run_cli(self, out_dir, *args):
        env = dict(os.environ, PORT_LEDGER_OUT=str(out_dir))
        return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True, env=env)

    def test_json_flag_read_before_or_after_the_query(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "ledger.json").write_text(json.dumps(self.LEDGER), encoding="utf-8")
            for args, want_json in ((["where", "Foo", "--json"], True),
                                    (["--json", "where", "Foo"], True),
                                    (["where", "Foo"], False)):
                with self.subTest(args=args):
                    done = self.run_cli(tmp, *args)
                    self.assertNotIn("unrecognized arguments", done.stderr)
                    self.assertEqual(done.returncode, 0, done.stderr)
                    if want_json:
                        self.assertEqual(json.loads(done.stdout)[0]["path"], "net/minecraft/world/Foo")
                    else:
                        with self.assertRaises(json.JSONDecodeError):
                            json.loads(done.stdout)
                        self.assertIn("class: Foo", done.stdout)
            # The query's own help offers the flag, so it is where a reader looks for it.
            helped = self.run_cli(tmp, "where", "--help")
            self.assertEqual(helped.returncode, 0, helped.stderr)
            self.assertIn("--json", helped.stdout)


if __name__ == "__main__":
    unittest.main()
