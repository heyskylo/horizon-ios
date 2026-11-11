import importlib.util
import json
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "stamp_native_hooks", Path(__file__).resolve().parents[1] / "tools/stamp_native_hooks.py"
)
stamp = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stamp)


class StampTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.original = stamp.ROOT
        stamp.ROOT = self.root
        self.addCleanup(setattr, stamp, "ROOT", self.original)
        subprocess.run(["git", "init", "-q", "-b", "master", str(self.root)], check=True)
        for path, content in {
            stamp.SOURCE: "//! API 7 adds old.\n//!\npub(crate) const NATIVE_HOOKS_API: u32 = 7;\n",
            stamp.DOCS: "API 7 adds old.\n\n## What the launcher does\n",
            stamp.CATALOG: '{"hooks": []}\n',
        }.items():
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            (self.root / path).write_text(content)
        (self.root / stamp.FRAGMENTS).mkdir(parents=True)

    def git(self, *args):
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Test",
                        "-c", "user.email=test@example.invalid", *args],
                       check=True, capture_output=True)

    def fragment(self, name):
        path = self.root / stamp.FRAGMENTS / f"{name}.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({
            "hooks": [{"name": name, "target": f"target::{name}"}],
            "rust_summary": f"{name}.",
            "doc_summary": f'{name}; requires @API:{name}@.',
        }))
        return path

    def test_merge_order_and_idempotence(self):
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        self.git("checkout", "-qb", "first")
        self.fragment("z")
        self.git("add", ".")
        self.git("commit", "-qm", "first")
        self.git("checkout", "-q", "master")
        self.git("checkout", "-qb", "second")
        self.fragment("a")
        self.git("add", ".")
        self.git("commit", "-qm", "second")
        self.git("checkout", "-q", "master")
        self.git("merge", "--no-ff", "-qm", "merge first", "first")
        self.git("merge", "--no-ff", "-qm", "merge second", "second")
        self.assertTrue(stamp.stamp(True))
        self.assertFalse(stamp.stamp(False))
        source = (self.root / stamp.SOURCE).read_text()
        docs = (self.root / stamp.DOCS).read_text()
        self.assertIn("API 8 adds z.", source)
        self.assertIn("API 9 adds a.", source)
        self.assertIn("API 8 adds z; requires 8.", docs)
        self.assertIn("API 9 adds a; requires 9.", docs)
        self.assertIn("NATIVE_HOOKS_API: u32 = 9;", source)
        hooks = json.loads((self.root / stamp.CATALOG).read_text())["hooks"]
        self.assertEqual([(h["name"], h["version"]) for h in hooks], [("a", 1), ("z", 1)])
        self.assertFalse(stamp.stamp(False))

    def test_helper_only_gets_a_number(self):
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        path = self.fragment("helper")
        fragment = json.loads(path.read_text())
        fragment["hooks"] = []
        fragment["helpers"] = [{"name": "helper", "target": "target::helper"}]
        path.write_text(json.dumps(fragment))
        self.assertFalse(stamp.stamp(False))
        self.assertIn("NATIVE_HOOKS_API: u32 = 8;", (self.root / stamp.SOURCE).read_text())
        self.assertEqual(json.loads((self.root / stamp.CATALOG).read_text())["hooks"], [])
        self.assertEqual(json.loads((self.root / stamp.CATALOG).read_text())["helpers"], fragment["helpers"])

    def test_selected_fragments_keep_merge_order_and_others_stay_pending(self):
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        # An older, unfinished API must not stop two later, reviewed APIs being registered.
        for name in ("blocked", "z", "a"):
            self.fragment(name)
            self.git("add", ".")
            self.git("commit", "-qm", name)
        blocked = self.root / stamp.FRAGMENTS / "blocked.json"
        pending_comment = self.root / "pending.rs"
        pending_comment.write_text("/// Since API @API:blocked@.\n")
        fragment = json.loads(blocked.read_text())
        fragment["comments"] = ["pending.rs"]
        blocked.write_text(json.dumps(fragment))
        api = self.root / stamp.MOD_API
        api.write_text("/// APIs @API:blocked@, @API:z@ and @API:a@.\n")
        before = {path: path.read_bytes() for path in (blocked, pending_comment)}

        # CLI order must not override merge order; duplicate selections must not consume numbers.
        self.assertFalse(stamp.stamp(False, ["a", "z", "a"]))
        source = (self.root / stamp.SOURCE).read_text()
        self.assertIn("API 8 adds z.", source)
        self.assertIn("API 9 adds a.", source)
        self.assertIn("NATIVE_HOOKS_API: u32 = 9;", source)
        self.assertEqual(api.read_text(), "/// APIs @API:blocked@, 8 and 9.\n")
        docs = (self.root / stamp.DOCS).read_text()
        self.assertIn("API 8 adds z; requires 8.", docs)
        self.assertIn("API 9 adds a; requires 9.", docs)
        self.assertEqual({path: path.read_bytes() for path in before}, before)
        self.assertEqual(sorted(path.stem for path in blocked.parent.glob("*.json")), ["blocked"])
        hooks = json.loads((self.root / stamp.CATALOG).read_text())["hooks"]
        self.assertEqual([h["name"] for h in hooks], ["a", "z"])

        # Once ready, the skipped API gets the next free number, even though it merged first.
        self.assertTrue(stamp.stamp(True))
        self.assertFalse(stamp.stamp(False))
        self.assertIn("API 10 adds blocked.", (self.root / stamp.SOURCE).read_text())
        self.assertIn("API 10 adds blocked; requires 10.", (self.root / stamp.DOCS).read_text())
        self.assertEqual(api.read_text(), "/// APIs 10, 8 and 9.\n")
        self.assertEqual(pending_comment.read_text(), "/// Since API 10.\n")
        hooks = json.loads((self.root / stamp.CATALOG).read_text())["hooks"]
        self.assertEqual([h["name"] for h in hooks], ["a", "z", "blocked"])
        self.assertFalse(stamp.stamp(False))

    def test_selection_rejects_unknown_names_before_writing_even_in_check_mode(self):
        path = self.fragment("ready")
        before = {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        for check in (False, True):
            with self.subTest(check=check):
                with self.assertRaisesRegex(ValueError, "unknown pending fragment.*typo"):
                    stamp.stamp(check, ["ready", "typo"])
                self.assertEqual({p: p.read_bytes() for p in before}, before)
        path.unlink()
        with self.assertRaisesRegex(ValueError, "unknown pending fragment.*ready"):
            stamp.stamp(False, ["ready"])

    def test_selection_cli_check_is_read_only_and_can_stamp_helper_only(self):
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        path = self.fragment("ready")
        fragment = json.loads(path.read_text())
        fragment["hooks"] = []
        fragment["helpers"] = [{"name": "helper", "target": "target::helper"}]
        path.write_text(json.dumps(fragment))
        blocked = self.fragment("blocked")
        before = {p: p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        with patch("sys.argv", ["stamp_native_hooks.py", "--check", "--only", "ready"]):
            self.assertEqual(stamp.main(), 1)
        self.assertEqual({p: p.read_bytes() for p in before}, before)
        with patch("sys.argv", ["stamp_native_hooks.py", "--only", "ready"]):
            self.assertEqual(stamp.main(), 0)
        catalog = json.loads((self.root / stamp.CATALOG).read_text())
        self.assertEqual(catalog["hooks"], [])
        self.assertEqual(catalog["helpers"], fragment["helpers"])
        self.assertIn("NATIVE_HOOKS_API: u32 = 8;", (self.root / stamp.SOURCE).read_text())
        self.assertFalse(path.exists())
        self.assertEqual(blocked.read_bytes(), before[blocked])

    def test_catalog_facts_travel_with_the_hook(self):
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        path = self.fragment("served")
        fragment = json.loads(path.read_text())
        contract = {"kind": "listen", "receiver": "game"}
        fragment["hooks"][0].update(
            {"side": "server", "cancellable": True, "mod_api": contract, "other": 1})
        path.write_text(json.dumps(fragment))
        self.assertFalse(stamp.stamp(False))
        hooks = json.loads((self.root / stamp.CATALOG).read_text())["hooks"]
        self.assertEqual(hooks, [{"name": "served", "version": 1, "target": "target::served",
                                  "side": "server", "cancellable": True, "mod_api": contract}])

    def test_markers_are_stamped_in_hand_written_api_types(self):
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        self.fragment("drops")
        path = self.root / stamp.MOD_API
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("/// Since API @API:drops@.\n")
        self.assertFalse(stamp.stamp(False))
        self.assertEqual(path.read_text(), "/// Since API 8.\n")

    def test_stamped_files_are_read_and_written_as_utf8(self):
        # ” is UTF-8 that Windows' code page cannot decode (0x9d), as in files the repository's fragments stamp.
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        self.fragment("quote")
        path = self.root / stamp.MOD_API
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes("/// “Since” API @API:quote@.\n".encode())
        self.assertFalse(stamp.stamp(False))
        self.assertEqual(path.read_text(encoding="utf-8"), "/// “Since” API 8.\n")

    def test_stamping_leaves_stale_generated_copies_alone(self):
        # Old checkouts may still have these files; only the generator's inputs are stamped now.
        generated = (
            "crates/horizon-client/src/mod_api/generated.rs",
            "crates/horizon-client/src/mod_api/generated_host.rs",
            "crates/horizon-client/src/game/mod_api_host_generated.rs",
            "crates/horizon-client-render/src/gfx/native_render/generated.rs",
        )
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        self.fragment("drops")
        for name in generated:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("/// Since API @API:drops@.\n")
        self.assertFalse(stamp.stamp(False))
        self.assertIn("NATIVE_HOOKS_API: u32 = 8;", (self.root / stamp.SOURCE).read_text())
        for name in generated:
            self.assertEqual((self.root / name).read_text(), "/// Since API @API:drops@.\n")

    def test_duplicate_fails_before_writes(self):
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        self.fragment("same")
        (self.root / stamp.CATALOG).write_text('{"hooks": [{"name": "same"}]}')
        before = (self.root / stamp.SOURCE).read_text()
        with self.assertRaisesRegex(ValueError, "duplicate"):
            stamp.stamp(False)
        self.assertEqual((self.root / stamp.SOURCE).read_text(), before)

    def test_the_repository_stamps_every_marker(self):
        # A copy of this checkout's pending fragments and the files they stamp: stamping must
        # succeed and leave no marker, or the stamp workflow fails or ships unnumbered docs.
        repo = self.original
        fragments = [p.relative_to(repo) for p in (repo / stamp.FRAGMENTS).glob("*.json")]
        comments = {Path(c) for p in fragments
                    for c in json.loads((repo / p).read_text(encoding="utf-8")).get("comments", [])}
        stamped = [stamp.SOURCE, stamp.DOCS, stamp.MOD_API, *comments]
        for path in [stamp.CATALOG, *stamped, *fragments]:
            if (repo / path).exists():
                (self.root / path).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(repo / path, self.root / path)
        self.git("add", ".")
        self.git("commit", "-qm", "base")
        self.assertFalse(stamp.stamp(False))
        left = {str(p): re.findall(r"@API:\w+@", (self.root / p).read_text(encoding="utf-8"))
                for p in stamped if (self.root / p).exists()}
        self.assertEqual({p: m for p, m in left.items() if m}, {})


if __name__ == "__main__":
    unittest.main()
