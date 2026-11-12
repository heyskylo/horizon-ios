"""A failed portal-blast build must leave actionable diagnostics in CI's log."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from tools import test_sulfur_portal_blast_parity as harness


class CompileDiagnosticsTests(unittest.TestCase):
    def run_build(self, code, stderr, records):
        def compile_result(command, **kwargs):
            kwargs["stderr"].write(stderr)
            kwargs["stdout"].write(records)
            return subprocess.CompletedProcess(command, code)

        captured = io.StringIO()
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            with mock.patch.object(harness.subprocess, "run", side_effect=compile_result), \
                    contextlib.redirect_stderr(captured):
                if code:
                    with self.assertRaises(subprocess.CalledProcessError) as failure:
                        harness.compile_tests(build, build / "scratch")
                    self.assertEqual(code, failure.exception.returncode)
                else:
                    harness.compile_tests(build, build / "scratch")
            self.assertEqual(stderr, (build / "compile.log").read_text(encoding="utf-8"))
            self.assertEqual(records, (build / "compile.jsonl").read_text(encoding="utf-8"))
        return captured.getvalue()

    def test_compiler_errors_are_visible_and_still_fail(self):
        diagnostic = "error[E0063]: missing fixture field\n"
        records = json.dumps({"reason": "compiler-message", "message": {"rendered": diagnostic}}) + "\n"
        self.assertEqual("build failed\n" + diagnostic, self.run_build(101, "build failed\n", records))

    def test_cargo_failure_without_compiler_json_is_visible(self):
        self.assertEqual("error: disk full\n", self.run_build(101, "error: disk full\n", "not JSON\n"))

    def test_success_keeps_artifacts_without_printing_build_noise(self):
        self.assertEqual("", self.run_build(0, "finished\n", '{"reason":"build-finished","success":true}\n'))


if __name__ == "__main__":
    unittest.main()
