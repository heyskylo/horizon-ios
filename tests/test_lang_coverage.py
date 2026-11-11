import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

from tools import lang_coverage


def write(folder: Path, name: str, body: str) -> None:
    (folder / name).write_text(body, encoding="utf-8")


def run(*arguments: str) -> tuple[int, str, str]:
    """The tool's exit code and its two streams."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = lang_coverage.main(list(arguments))
    return code, out.getvalue(), err.getvalue()


def by_code(rows: list[dict], code: str) -> dict:
    return next(row for row in rows if row["code"] == code)


class GameCoverageTests(unittest.TestCase):
    def test_a_gap_an_unknown_key_and_an_empty_value_are_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "en_us.json", '{\n  "a": "Apple",\n  "b": "Banana",\n  "c": "Cherry"\n}\n')
            write(folder, "xx.json", '{\n  "a": "Manzana",\n  "c": "",\n  "d": "Datil"\n}\n')
            reference, rows = lang_coverage.game_rows(folder, set())
        self.assertEqual(len(reference.entries), 3)
        row = by_code(rows, "xx")
        self.assertEqual(row["keys"], 3)
        self.assertEqual(row["missing"], ["b"])
        self.assertEqual(row["extra"], ["d"])
        self.assertEqual(row["empty"], ["c"])
        self.assertAlmostEqual(row["coverage"], 2 / 3)
        self.assertEqual(row["problem"], "")

    def test_a_value_equal_to_english_is_listed_but_never_for_the_reference(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "en_us.json", '{\n  "a": "Apple",\n  "b": "Banana"\n}\n')
            write(folder, "xx.json", '{\n  "a": "Apple",\n  "b": "Platano"\n}\n')
            _, rows = lang_coverage.game_rows(folder, set())
        self.assertEqual(by_code(rows, "xx")["same"], ["a"])
        self.assertEqual(by_code(rows, "en_us")["same"], [])

    def test_a_changed_placeholder_is_reported_and_plain_percent_text_is_not(self):
        self.assertEqual(lang_coverage.placeholders("%s chunks of %1$s, 100% sure, %.2f, %%"),
                         ["%%", "%.2f", "%1$s", "%s"])
        self.assertEqual(lang_coverage.placeholders("100% is plain text"), [])
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "en_us.json", '{\n  "z": "%s and %d"\n}\n')
            write(folder, "xx.json", '{\n  "z": "%s"\n}\n')
            _, rows = lang_coverage.game_rows(folder, set())
        changed = by_code(rows, "xx")["placeholders"]
        self.assertEqual([entry["key"] for entry in changed], ["z"])

    def test_a_repeated_key_or_a_missing_indent_is_a_problem(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "en_us.json", '{\n  "a": "Apple"\n}\n')
            write(folder, "repeated.json", '{\n  "a": "Apple",\n  "a": "Again"\n}\n')
            write(folder, "indented.json", '{\n    "a": "Apple"\n}\n')
            _, rows = lang_coverage.game_rows(folder, set())
        self.assertIn("repeats", by_code(rows, "repeated")["problem"])
        self.assertIn("indentation", by_code(rows, "indented")["problem"])

    def test_a_file_that_is_not_a_flat_map_is_a_problem(self):
        for name, body, wanted in [
            ("number.json", '{\n  "a": 1\n}\n', "flat map"),
            ("mark.json", '\ufeff{\n  "a": "Apple"\n}\n', "byte order mark"),
            ("broken.json", '{\n  "a": "Apple"\n', "not JSON"),
        ]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                folder = Path(tmp)
                write(folder, "en_us.json", '{\n  "a": "Apple"\n}\n')
                write(folder, name, body)
                _, rows = lang_coverage.game_rows(folder, set())
                self.assertIn(wanted, by_code(rows, Path(name).stem)["problem"])

    def test_a_broken_reference_stops_the_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            write(Path(tmp), "en_us.json", '{\n  "a": 1\n}\n')
            with self.assertRaises(ValueError):
                lang_coverage.game_rows(Path(tmp), set())

    def test_the_languages_come_out_most_complete_first(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "en_us.json", '{\n  "a": "Apple",\n  "b": "Banana"\n}\n')
            write(folder, "half.json", '{\n  "a": "Manzana"\n}\n')
            write(folder, "full.json", '{\n  "a": "Manzana",\n  "b": "Platano"\n}\n')
            _, rows = lang_coverage.game_rows(folder, set())
        self.assertEqual([row["code"] for row in rows], ["en_us", "full", "half"])


class LauncherCoverageTests(unittest.TestCase):
    def test_english_leftovers_and_dropped_holes_are_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "xx.json", '{\n  "Back": "Atras",\n  "Proxy": "Proxy",\n'
                                     '  "Open {file}": "Abrir",\n'
                                     '  "Save {count} files": "Guardar {archivo} archivos"\n}\n')
            rows = lang_coverage.launcher_rows(folder, set())
        row = by_code(rows, "xx")
        self.assertEqual(row["keys"], 4)
        self.assertEqual(row["same"], ["Proxy"])
        changed = {entry["key"]: entry for entry in row["holes"]}
        self.assertEqual(changed["Open {file}"]["missing"], ["file"])
        self.assertEqual(changed["Save {count} files"]["missing"], ["count"])
        self.assertEqual(changed["Save {count} files"]["extra"], ["archivo"])


class CommandLineTests(unittest.TestCase):
    def test_fail_under_and_an_unknown_language_set_the_exit_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "en_us.json", '{\n  "a": "Apple",\n  "b": "Banana"\n}\n')
            write(folder, "xx.json", '{\n  "a": "Manzana"\n}\n')
            code, _, err = run("--game-dir", tmp, "--game-only", "--fail-under", "90")
            self.assertEqual(code, 1)
            self.assertIn("xx: 50.0% < 90%", err)
            code, _, _ = run("--game-dir", tmp, "--game-only", "--fail-under", "10")
            self.assertEqual(code, 0)
            code, _, err = run("--game-dir", tmp, "--game-only", "--lang", "nosuch")
            self.assertEqual(code, 2)
            self.assertIn("no lang file is named nosuch", err)

    def test_json_carries_the_counts_a_pull_request_quotes(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            write(folder, "en_us.json", '{\n  "a": "Apple",\n  "b": "Banana"\n}\n')
            write(folder, "xx.json", '{\n  "a": "Manzana"\n}\n')
            code, out, _ = run("--game-dir", tmp, "--game-only", "--json")
        self.assertEqual(code, 0)
        payload = json.loads(out)
        self.assertEqual(payload["game"]["reference"], "en_us")
        self.assertEqual(payload["game"]["keys"], 2)
        self.assertEqual(by_code(payload["game"]["files"], "xx")["missing"], ["b"])

    def test_the_repository_folders_are_where_the_tool_looks(self):
        code, out, _ = run("--lang", "es_es", "es", "--summary")
        self.assertEqual(code, 0, out)
        self.assertIn("GAME es_es ", out)
        self.assertIn("LAUNCHER es ", out)


class ReviewTests(unittest.TestCase):
    """The record of what a person checked: anything else, or anything changed since, is unreviewed."""

    def test_only_entries_checked_in_their_present_text_count_as_reviewed(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder, review = Path(tmp) / "lang", Path(tmp) / "review"
            folder.mkdir()
            review.mkdir()
            write(folder, "en_us.json", '{\n  "a": "Apple",\n  "b": "Banana",\n  "c": "Cherry",\n'
                                        '  "d": "Date"\n}\n')
            write(folder, "xx.json", '{\n  "a": "Manzana",\n  "b": "Platano",\n  "c": "Cereza",\n'
                                     '  "d": ""\n}\n')
            # "b" was checked, then its text changed; "z" left the file.
            write(review, "xx.json", '{\n  "a": "Manzana",\n  "b": "Banano",\n  "z": "Zeta"\n}\n')
            _, rows = lang_coverage.game_rows(folder, set(), review)
        self.assertEqual(by_code(rows, "xx")["unreviewed"], ["b", "c"])
        self.assertEqual(by_code(rows, "en_us")["unreviewed"], [])

    def test_mark_reviewed_records_present_text_in_file_order_and_drops_gone_keys(self):
        with tempfile.TemporaryDirectory() as tmp:
            game, launcher, review = Path(tmp) / "game", Path(tmp) / "launcher", Path(tmp) / "review"
            game.mkdir()
            launcher.mkdir()
            write(game, "en_us.json", '{\n  "a": "Apple",\n  "b": "Banana",\n  "c": "Cherry"\n}\n')
            write(game, "xx_yy.json", '{\n  "a": "Manzana",\n  "b": "Platano",\n  "c": "Cereza"\n}\n')
            (review / "game").mkdir(parents=True)
            write(review / "game", "xx_yy.json", '{\n  "gone": "Ido"\n}\n')
            keys = Path(tmp) / "keys.txt"
            keys.write_text("a\n\n", encoding="utf-8")
            common = ("--game-dir", str(game), "--launcher-dir", str(launcher), "--review-dir", str(review))
            code, out, _ = run(*common, "--lang", "xx_yy", "--mark-reviewed", "c", "--keys-file", str(keys))
            self.assertEqual(code, 0, out)
            record = (review / "game" / "xx_yy.json").read_text(encoding="utf-8")
            self.assertEqual(record, '{\n  "a": "Manzana",\n  "c": "Cereza"\n}\n')
            code, out, _ = run(*common, "--game-only", "--lang", "xx_yy", "--unreviewed")
            self.assertEqual(code, 0)
            self.assertIn("xx_yy: 1 unreviewed", out)
            self.assertIn("en_us: Banana", out)
            code, _, err = run(*common, "--lang", "xx_yy", "--mark-reviewed", "nosuch")
            self.assertEqual(code, 1)
            self.assertIn("no key 'nosuch'", err)

    def test_mark_reviewed_refuses_what_it_cannot_tell_apart(self):
        with tempfile.TemporaryDirectory() as tmp:
            game, launcher = Path(tmp) / "game", Path(tmp) / "launcher"
            game.mkdir()
            launcher.mkdir()
            write(game, "en_us.json", '{\n  "a": "Apple"\n}\n')
            write(game, "xx.json", '{\n  "a": "Manzana"\n}\n')
            write(launcher, "xx.json", '{\n  "Apple": "Manzana"\n}\n')
            common = ("--game-dir", str(game), "--launcher-dir", str(launcher),
                      "--review-dir", str(Path(tmp) / "review"))
            for extra, wanted in [(("--lang", "xx"), "--game-only or --launcher-only"),
                                  (("--lang", "xx", "en_us"), "exactly one --lang"),
                                  (("--lang", "en_us"), "English reference"),
                                  (("--lang", "xx", "--game-only"), "needs keys")]:
                with self.subTest(extra=extra):
                    code, _, err = run(*common, *extra, "--mark-reviewed")
                    self.assertEqual(code, 2)
                    self.assertIn(wanted, err)
            code, _, _ = run(*common, "--lang", "xx", "--launcher-only", "--mark-reviewed", "Apple")
            self.assertEqual(code, 0)
            self.assertTrue((Path(tmp) / "review" / "launcher" / "xx.json").exists())

    def test_a_crates_own_folder_is_reported_and_its_reviews_kept_apart(self):
        with tempfile.TemporaryDirectory() as tmp:
            game, crates, review = Path(tmp) / "game", Path(tmp) / "crates", Path(tmp) / "review"
            friends = crates / "horizon-friends" / "assets" / "lang"
            game.mkdir()
            friends.mkdir(parents=True)
            (crates / "horizon-plain" / "src").mkdir(parents=True)  # no lang folder: not reported
            write(game, "en_us.json", '{\n  "a": "Apple"\n}\n')
            write(game, "xx_yy.json", '{\n  "a": "Manzana"\n}\n')
            write(friends, "en_us.json", '{\n  "f.one": "One",\n  "f.two": "Two"\n}\n')
            write(friends, "xx_yy.json", '{\n  "f.one": "Uno"\n}\n')
            common = ("--game-dir", str(game), "--crates-dir", str(crates), "--review-dir", str(review),
                      "--game-only")
            code, out, _ = run(*common, "--summary")
            self.assertEqual(code, 0)
            self.assertIn("GAME xx_yy 1/1 keys, missing 0", out)
            self.assertIn("GAME horizon-friends xx_yy 1/2 keys, missing 1", out)
            self.assertNotIn("horizon-plain", out)
            code, _, err = run(*common, "--fail-under", "90")
            self.assertEqual(code, 1)
            self.assertIn("xx_yy: 50.0%", err)
            # One command marks the keys of both folders; each lands in its folder's record.
            code, out, _ = run(*common, "--lang", "xx_yy", "--mark-reviewed", "a", "f.one")
            self.assertEqual(code, 0, out)
            self.assertEqual(json.loads((review / "game" / "xx_yy.json").read_text(encoding="utf-8")),
                             {"a": "Manzana"})
            self.assertEqual(json.loads((review / "horizon-friends" / "xx_yy.json").read_text(encoding="utf-8")),
                             {"f.one": "Uno"})
            # Marking the game's folder again keeps the crate's record.
            code, _, _ = run(*common, "--lang", "xx_yy", "--mark-reviewed", "a")
            self.assertEqual(code, 0)
            self.assertTrue((review / "horizon-friends" / "xx_yy.json").exists())
            code, _, err = run(*common, "--lang", "xx_yy", "--mark-reviewed", "nosuch")
            self.assertEqual(code, 1)
            self.assertIn("no key 'nosuch'", err)
            # A scratch --game-dir alone leaves the repository's crates out.
            code, out, _ = run("--game-dir", str(game), "--review-dir", str(review), "--game-only", "--summary")
            self.assertNotIn("horizon-friends", out)

    def test_every_repository_record_belongs_to_a_lang_file_and_reads(self):
        folders = [("game", lang_coverage.GAME), ("launcher", lang_coverage.LAUNCHER),
                   *lang_coverage.crate_dirs(lang_coverage.CRATES)]
        for kind, folder in folders:
            for record in sorted((lang_coverage.REVIEW / kind).glob("*.json")):
                with self.subTest(record=record.name, kind=kind):
                    self.assertTrue((folder / record.name).exists(), f"{record} names no lang file")
                    self.assertTrue(lang_coverage.reviewed(record))


if __name__ == "__main__":
    unittest.main()
