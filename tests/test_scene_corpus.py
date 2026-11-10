import contextlib
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import scene_corpus as sc


def recipe(**changes):
    base = {"name": "t", "render_distance": 8, "lod_distance": 0, "size": [64, 64], "steps": [["wait-ready", 30]],
            "views": [0], "variants": {}, "expect": {"min_sections": 1}}
    base.update(changes)
    return base


def write_capture(directory, translucent=(8, 12), layout=None, recipe_hash=None):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    scene = {"format": "horizon-scene-capture", "version": [1, 0], "sections": len(translucent), "tiles": 3}
    if recipe_hash:
        scene["corpus"] = {"recipe": "spawn", "recipe_hash": recipe_hash}
    (directory / "scene.json").write_text(json.dumps(scene))
    (directory / "sections.json").write_text(json.dumps([{"translucent": t, "solid": 40} for t in translucent]))
    if layout is not None:
        (directory / "layout.json").write_text(json.dumps(layout))


class RecipeTests(unittest.TestCase):
    def test_committed_recipes_are_valid(self):
        names = [r["name"] for r in sc.load_recipes()]
        self.assertEqual(len(names), len(set(names)))
        self.assertIn("spawn", names)

    def test_valid_recipe_has_no_errors(self):
        self.assertEqual(sc.validate(recipe()), [])

    def test_unknown_step_is_named(self):
        errors = sc.validate(recipe(steps=[["teleport", 1, 2, 3]]))
        self.assertEqual(len(errors), 1)
        self.assertIn("teleport", errors[0])

    def test_step_with_wrong_arguments_is_refused(self):
        self.assertTrue(sc.validate(recipe(steps=[["tp", 1, 2]])))
        self.assertTrue(sc.validate(recipe(steps=[["tp", 1, 2, "x"]])))
        self.assertEqual(sc.validate(recipe(steps=[["wait-far"]])), [])

    def test_missing_or_empty_expect_is_refused(self):
        missing = recipe()
        del missing["expect"]
        self.assertTrue(any("expect" in e for e in sc.validate(missing)))
        self.assertTrue(any("empty" in e for e in sc.validate(recipe(expect={}))))

    def test_unknown_expect_key_is_refused(self):
        self.assertTrue(any("min_dragons" in e for e in sc.validate(recipe(expect={"min_dragons": 1}))))

    def test_load_reports_duplicates_and_invalid_recipes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "corpus.json"
            path.write_text(json.dumps([recipe(), recipe()]))
            with self.assertRaisesRegex(ValueError, "duplicate scene t"):
                sc.load_recipes(path)
            path.write_text(json.dumps([recipe(steps=[["fly"]])]))
            with self.assertRaisesRegex(ValueError, "fly"):
                sc.load_recipes(path)

    def test_hash_ignores_key_order_and_follows_content(self):
        a = recipe()
        b = dict(reversed(list(a.items())))
        self.assertEqual(sc.recipe_hash(a), sc.recipe_hash(b))
        self.assertNotEqual(sc.recipe_hash(a), sc.recipe_hash(recipe(render_distance=9)))

    def test_far_terrain_distance_reaches_the_graphics_file(self):
        self.assertIn("lodDistance:256\n", sc.graphics_text(recipe(lod_distance=256)))
        self.assertIn("gpuCulling:true\n", sc.graphics_text(recipe()))


class AcceptanceTests(unittest.TestCase):
    MEASURED = {"sections": 50000, "tiles": 0, "translucent_quads": 10, "pages": 6, "id_scramble": 0.4, "culled_share": 0.6}

    def test_passing_capture_has_no_failures(self):
        expect = {"min_sections": 40000, "min_pages": 5, "min_id_scramble": 0.25, "min_culled_share": 0.5, "max_pages": 6}
        self.assertEqual(sc.check_expect(expect, self.MEASURED), [])

    def test_failure_names_the_rule_and_the_measured_value(self):
        failures = sc.check_expect({"min_pages": 12, "max_id_scramble": 0.15, "min_sections": 1}, self.MEASURED)
        self.assertEqual(len(failures), 2)
        self.assertIn("min_pages: pages is 6, wanted at least 12", failures)
        self.assertIn("max_id_scramble: id_scramble is 0.4, wanted at most 0.15", failures)

    def test_bounds_are_inclusive(self):
        self.assertEqual(sc.check_expect({"min_pages": 6, "max_pages": 6}, self.MEASURED), [])

    def test_unmeasured_value_fails(self):
        failures = sc.check_expect({"min_id_scramble": 0.25}, dict(self.MEASURED, id_scramble=None))
        self.assertEqual(failures, ["min_id_scramble: id_scramble not measured"])

    def test_measure_reads_the_capture_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_capture(tmp, translucent=(8, 12, 4), layout={"arena_pages": 2, "id_scramble": 0.3})
            got = sc.measure(tmp)
        self.assertEqual((got["sections"], got["tiles"], got["translucent_quads"]), (3, 3, 6))
        self.assertEqual((got["pages"], got["id_scramble"]), (2, 0.3))

    def test_measure_without_gpu_culling_layout(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_capture(tmp, layout=None)
            self.assertIsNone(sc.measure(tmp)["pages"])
            write_capture(tmp, layout={})
            got = sc.measure(tmp)
        self.assertIsNone(got["pages"])
        self.assertIsNone(got["id_scramble"])

    def test_culled_share_comes_from_the_first_view(self):
        report = {"views": [{"culled": [[120, 80], 0.62]}, {"culled": [[1, 1], 0.1]}]}
        self.assertEqual(sc.culled_share(report), 0.62)
        self.assertIsNone(sc.culled_share({"views": [{"culled": None}]}))


class CorpusIdTests(unittest.TestCase):
    def test_id_follows_content_and_names_not_order_or_its_own_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            one, two = Path(tmp, "one"), Path(tmp, "two")
            for d, order in ((one, ("a.bin", "b.bin")), (two, ("b.bin", "a.bin"))):
                d.mkdir()
                for name in order:
                    (d / name).write_bytes(name.encode())
            self.assertEqual(sc.corpus_id(one), sc.corpus_id(two))
            (one / "corpus-id").write_text("whatever")
            self.assertEqual(sc.corpus_id(one), sc.corpus_id(two))
            (two / "a.bin").write_bytes(b"changed")
            self.assertNotEqual(sc.corpus_id(one), sc.corpus_id(two))

    def test_moving_bytes_between_files_changes_the_id(self):
        with tempfile.TemporaryDirectory() as tmp:
            one, two = Path(tmp, "one"), Path(tmp, "two")
            one.mkdir(), two.mkdir()
            (one / "a").write_bytes(b"xy"), (one / "b").write_bytes(b"z")
            (two / "a").write_bytes(b"x"), (two / "b").write_bytes(b"yz")
            self.assertNotEqual(sc.corpus_id(one), sc.corpus_id(two))


class ListTests(unittest.TestCase):
    def listing(self, root):
        out = io.StringIO()
        with mock.patch.dict(os.environ, {"HORIZON_SCENE_CORPUS": str(root)}), contextlib.redirect_stdout(out):
            self.assertEqual(sc.list_scenes(None), 0)
        return {line.split()[0]: line for line in out.getvalue().splitlines()[1:]}

    def test_missing_current_and_stale_scenes(self):
        spawn = next(r for r in sc.load_recipes() if r["name"] == "spawn")
        with tempfile.TemporaryDirectory() as tmp:
            write_capture(Path(tmp, "v1", "spawn"), recipe_hash=sc.recipe_hash(spawn))
            write_capture(Path(tmp, "v1", "travel"), recipe_hash="0" * 64)
            lines = self.listing(Path(tmp))
        self.assertIn("current", lines["spawn"])
        self.assertIn("1.0", lines["spawn"])
        self.assertIn("max_id_scramble=0.15", lines["spawn"])
        self.assertIn("stale", lines["travel"])
        self.assertIn("missing", lines["caves"])

    def test_scene_filter_rejects_unknown_names(self):
        with self.assertRaises(SystemExit) as raised:
            sc.chosen(sc.load_recipes(), "spawn,nowhere")
        self.assertIn("nowhere", str(raised.exception))
        self.assertEqual([r["name"] for r in sc.chosen(sc.load_recipes(), "travel")], ["travel"])


if __name__ == "__main__":
    unittest.main()
