import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from tools import test_rust_parity as parity


class ParityRunnerTests(unittest.TestCase):
    def test_report_rejects_sources_changed_during_a_run(self):
        with tempfile.TemporaryDirectory() as temp:
            # Resolved, as source_hashes resolves the sources: macOS's temp folder is a symlink
            # (/var -> /private/var).
            root = Path(temp).resolve()
            source = root / "source.rs"
            source.write_text("before")
            with mock.patch.object(parity.lab, "ROOT", root):
                before = parity.source_hashes([source])
                parity.ensure_sources_unchanged([source], before)
                source.write_text("after")
                with self.assertRaisesRegex(RuntimeError, "source.rs"):
                    parity.ensure_sources_unchanged([source], before)
    def test_sources_under_a_linked_local_folder_are_named_within_the_repository(self):
        # A worktree whose `.local` links to another checkout's: the harness's inputs live under
        # that link, and resolving them would leave the repository.
        with tempfile.TemporaryDirectory() as temp:
            root, elsewhere = Path(temp).resolve() / "worktree", Path(temp).resolve() / "shared"
            root.mkdir()
            (elsewhere / "rust-x-parity").mkdir(parents=True)
            try:
                (root / ".local").symlink_to(elsewhere, target_is_directory=True)
            except OSError as e:
                # Windows lets anyone make a junction, which is what a worktree there uses.
                try:
                    import _winapi
                    _winapi.CreateJunction(str(elsewhere), str(root / ".local"))
                except (ImportError, OSError):
                    self.skipTest(f"no directory links here: {e}")
            inputs = root / ".local" / "rust-x-parity" / "inputs.txt"
            inputs.write_text("1 2 3\n")
            with mock.patch.object(parity.lab, "ROOT", root):
                hashes = parity.source_hashes([inputs])
            self.assertEqual(list(hashes), [str(Path(".local", "rust-x-parity", "inputs.txt"))])
            with mock.patch.object(parity.lab, "ROOT", root), self.assertRaisesRegex(ValueError, "outside"):
                parity.source_hashes([Path(temp) / "stray.txt"])

    def test_comparison_rejects_changed_missing_extra_and_empty_results(self):
        for reference, candidate in [("rng 0 1\n", "rng 0 2\n"),
                                     ("rng 0 1\n", ""),
                                     ("rng 0 1\n", "rng 0 1\nrng 1 2\n"), ("", "")]:
            with self.subTest(reference=reference, candidate=candidate), tempfile.TemporaryDirectory() as temp:
                left, right = Path(temp) / "reference", Path(temp) / "candidate"
                left.write_text(reference)
                right.write_text(candidate)
                with self.assertRaises(AssertionError):
                    parity.compare(left, right)

    def test_comparison_counts_matching_lines(self):
        with tempfile.TemporaryDirectory() as temp:
            left, right = Path(temp) / "reference", Path(temp) / "candidate"
            left.write_text("rng 0 1\nrng 1 -1\n")
            right.write_text(left.read_text())
            self.assertEqual(parity.compare(left, right), 2)

    def test_chunk_comparison_preserves_newlines_unicode_and_unterminated_lines(self):
        cases = [
            ("one", "one", 1),
            ("\n\n", "\n\n", 2),
            ("one\r\ntwo\r", "one\ntwo\n", 2),
            ("α" * (1024 * 1024 + 1) + "\r\nlast", "α" * (1024 * 1024 + 1) + "\nlast", 2),
            ("same\n" * 220000 + "tail", "same\n" * 220000 + "tail", 220001),
        ]
        with tempfile.TemporaryDirectory() as temp:
            left, right = Path(temp) / "reference", Path(temp) / "candidate"
            for reference, candidate, count in cases:
                left.write_bytes(reference.encode("utf-8"))
                right.write_bytes(candidate.encode("utf-8"))
                self.assertEqual(parity.compare(left, right), count)

    def test_chunk_mismatch_retains_exact_first_line_diagnostic(self):
        prefix = "same\n" * 220000
        cases = [("expected\n", "actual\n", "'expected\\n'", "'actual\\n'"),
                 ("tail", "tail\n", "'tail'", "'tail\\n'"),
                 ("extra\n", "", "'extra\\n'", "None")]
        with tempfile.TemporaryDirectory() as temp:
            left, right = Path(temp) / "reference", Path(temp) / "candidate"
            for reference, candidate, expected, actual in cases:
                left.write_text(prefix + reference)
                right.write_text(prefix + candidate)
                with self.assertRaises(AssertionError) as caught:
                    parity.compare(left, right)
                self.assertEqual(str(caught.exception),
                                 f"Parity mismatch at output line 220001: vanilla={expected}, Rust={actual}")

    def test_failed_artifact_verification_removes_previous_success(self):
        with tempfile.TemporaryDirectory() as temp:
            local = Path(temp)
            report = local / "rust-parity/report.json"
            report.parent.mkdir()
            report.write_text(json.dumps({"previous_success": True}))
            with mock.patch.object(parity.lab, "LOCAL", local), mock.patch("sys.argv", ["parity"]), \
                 mock.patch.object(parity, "verify_artifacts", side_effect=ValueError("corrupt artifact")):
                with self.assertRaisesRegex(ValueError, "corrupt artifact"):
                    parity.main()
            self.assertFalse(report.exists())

    def test_corpus_is_repeatable_and_contains_rejection_and_word_boundaries(self):
        commands = list(parity.corpus())
        self.assertEqual(commands, list(parity.corpus()))
        self.assertIn("xoroshiro_state 0 0 2200", commands)
        self.assertIn("legacy -9223372036854775808 2200", commands)
        factories = [line.split() for line in commands if line.startswith("factory ")]
        self.assertEqual(len(factories), 4400)
        self.assertTrue({"-", "d800", "dc00", "d83dde00", "d8000061dc00"} <= {line[9] for line in factories})
        helpers = [line.split() for line in commands if line.startswith("helpers ")]
        self.assertEqual(len(helpers), 5000)
        for stream in ["legacy", "xoroshiro"]:
            bounds = {(int(line[3]), int(line[4])) for line in helpers if line[1] == stream}
            self.assertTrue({(-2147483648, 2147483647), (2147483647, -2147483648), (0, 0), (0, 1073741825)} <= bounds)
        for bits in range(1, 33):
            lengths = {int(line.split()[2]) for line in commands if line.startswith(f"storage {bits} ")}
            per_word = 64 // bits
            self.assertTrue({0, 1, per_word - 1, per_word, per_word + 1, 4096} <= lengths)


if __name__ == "__main__":
    unittest.main()
