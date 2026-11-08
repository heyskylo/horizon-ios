import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from tools import mod_port_harness as harness


class VerdictLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        self.mods = ("first", "second", "untouched")
        self.old = json.dumps({"checks": {"items": {"passed": 7, "failed": 0}}})
        for mod in self.mods:
            folder = self.work / "mods" / mod
            (folder / "carry").mkdir(parents=True)
            (folder / "carry" / "keep.txt").write_text("content")
            (folder / "harness").mkdir()
            self.verdict(mod).write_text(self.old)
            (folder / "carry-report.json").write_text('{"items": 1}')
        facts = self.work / "oracle" / "facts.json"
        facts.parent.mkdir()
        facts.write_text(json.dumps({
            "blocks": {}, "tags": {"block": {}}, "recipes": {},
            "items": {f"{mod}:item": {"max_stack_size": 64} for mod in self.mods},
        }))

    def verdict(self, mod):
        return self.work / "mods" / mod / "harness" / "result.json"

    def attempt(self, *mods, failure=None, jars=False, missing_item=False):
        def run(command, **kwargs):
            if command[0] == "cargo":
                stage = "build"
            elif command[0] == sys.executable:
                stage = "export"
            else:
                stage = command[-1]
            if failure == stage:
                raise subprocess.CalledProcessError(1, command)
            if failure == "interrupt" and stage == "first":
                raise KeyboardInterrupt()
            if stage in self.mods:
                if failure == "parse":
                    Path(command[3]).write_text("not JSON")
                    return
                Path(command[3]).write_text(json.dumps({
                    "blocks": {}, "block_tags": {}, "recipes": {},
                    "items": {} if missing_item else {f"{stage}:item": {"max_stack_size": 64}},
                }))

        args = ["mod_port_harness.py", "--work", str(self.work)]
        if jars:
            args += ["--jars", str(self.work / "jars")]
        args += mods
        metadata = json.dumps({"target_directory": str(self.work / "target")})
        with patch.object(sys, "argv", args), \
                patch.object(harness.subprocess, "run", side_effect=run), \
                patch.object(harness.subprocess, "check_output", return_value=metadata), \
                contextlib.redirect_stdout(io.StringIO()):
            harness.main()

    def test_failed_or_interrupted_attempt_cannot_reuse_selected_verdicts(self):
        for failure in ("export", "build", "first", "second", "interrupt", "parse"):
            with self.subTest(failure=failure):
                for mod in ("first", "second"):
                    self.verdict(mod).write_text(self.old)
                with self.assertRaises((subprocess.CalledProcessError, KeyboardInterrupt, json.JSONDecodeError)):
                    self.attempt("first", "second", failure=failure, jars=failure == "export")
                self.assertFalse(self.verdict("first").exists())
                self.assertFalse(self.verdict("second").exists())
                self.assertEqual(self.verdict("untouched").read_text(), self.old)
                self.assertEqual((self.work / "mods/first/carry/keep.txt").read_text(), "content")

    def test_missing_facts_cannot_leave_a_selected_green_verdict(self):
        (self.work / "oracle/facts.json").unlink()
        with self.assertRaises(SystemExit):
            self.attempt("first")
        self.assertFalse(self.verdict("first").exists())
        self.assertEqual(self.verdict("second").read_text(), self.old)

    def test_complete_attempt_publishes_real_comparison_and_preserves_unselected_legacy(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.attempt("first", failure="first")
        self.assertFalse(self.verdict("first").exists())
        for missing_item in (False, True):
            with self.subTest(missing_item=missing_item):
                self.attempt("first", missing_item=missing_item)
                result = json.loads(self.verdict("first").read_text())
                self.assertEqual(result["passed"], not missing_item)
                self.assertEqual(result["checks"]["items"], {
                    "passed": 0 if missing_item else 1,
                    "failed": 1 if missing_item else 0,
                    "examples": ["first:item: missing"] if missing_item else [],
                })
                self.assertEqual(self.verdict("second").read_text(), self.old)
                self.assertEqual(self.verdict("untouched").read_text(), self.old)

    def test_default_selection_invalidates_all_carried_mods_before_build(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.attempt(failure="build")
        self.assertTrue(all(not self.verdict(mod).exists() for mod in self.mods))

    def test_failed_invalidation_stops_before_replacing_evidence(self):
        # A directory cannot be unlinked as a verdict on either Windows or Unix.
        self.verdict("first").unlink()
        self.verdict("first").mkdir()
        sentinel = self.verdict("first") / "keep.txt"
        sentinel.write_text("keep")
        with self.assertRaises(OSError):
            self.attempt("first")
        self.assertEqual(sentinel.read_text(), "keep")
        self.assertFalse((self.work / "mods/first/harness/horizon.json").exists())
        self.assertEqual(self.verdict("untouched").read_text(), self.old)


if __name__ == "__main__":
    unittest.main()
