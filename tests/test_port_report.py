import json
from pathlib import Path
import subprocess
import tempfile
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]

from tools import port_report as report
from tools import version_port as port


def mapping():
    return {
        "classes": {
            "net/minecraft/world/food/FoodData": {
                "rust": {"crates/w/src/food.rs": ["tick"], "crates/c/src/hud.rs": []}, "harnesses": ["food"]},
            "net/minecraft/world/entity/Entity": {
                "rust": {f"crates/c/src/f{i}.rs": [] for i in range(5)}, "harnesses": ["food", "move"]},
            "net/minecraft/client/Sky": {"rust": {"crates/c/src/sky.rs": []}, "harnesses": []},
            "net/minecraft/world/entity/NewMob": {"rust": {"crates/c/src/new_mob.rs": []}, "harnesses": []},
            "net/minecraft/util/datafix/DataFixers": {"rust": {"crates/w/src/datafix/mod.rs": []},
                                                      "harnesses": ["datafix"]},
        },
        "harnesses": {
            "food": {"rust": ["crates/w/src/food.rs"], "classes": ["net/minecraft/world/food/FoodData"]},
            "move": {"rust": [], "classes": ["net/minecraft/world/entity/Entity"]},
            "datafix": {"rust": [], "classes": []},
        },
    }


def diff():
    def methods(**kinds):
        return {"methods": {k: kinds.get(k, []) for k in ("added", "removed", "changed")}}
    changed = {
        "net/minecraft/world/food/FoodData": methods(changed=["tick()V", "eat(I)V"]),
        "net/minecraft/world/entity/Entity": methods(changed=["move()V"]),
        "net/minecraft/client/Sky": methods(added=["stars()V"]),
        "net/minecraft/util/datafix/DataFixers": methods(changed=["addFixers()V"]),
        "net/minecraft/server/Unported": methods(changed=["run()V"]),
    }
    m = mapping()
    return {
        "facts": {"protocol_version": [776, 777], "world_version": [4903, 5023]},
        "packets": {"play/serverbound": {"added": {"minecraft:punch": 46}, "removed": {"minecraft:swing": 63},
                                         "renumbered": {}, "count": [69, 69]}},
        "registries": {"added": {}, "removed": [], "changed": {
            "minecraft:entity_type": {"added": ["minecraft:new_mob"]},
            "minecraft:data_component_type": {"added": ["minecraft:attack_animation"],
                                              "removed": ["minecraft:swing_animation"]}}},
        "blocks": {"added": [], "removed": [], "changed": {}, "format": ["definition"]},
        "items": {"added": [], "removed": [], "changed": {}, "components": {}},
        "commands": {"added": [], "removed": [], "changed": {}},
        "classes": {
            "added": ["net/minecraft/world/entity/NewMob", "net/minecraft/util/datafix/schemas/V4996",
                      "net/minecraft/util/datafix/fixes/SwingSplitFix", "net/minecraft/network/protocol/game/Punch",
                      "net/minecraft/elsewhere/Thing"],
            "removed": ["net/minecraft/network/protocol/game/Swing"], "moved": {},
            "changed": changed,
            "horizon": {name: m["classes"][name] for name in changed if name in m["classes"]},
        },
    }


class PortReportTests(unittest.TestCase):
    def build(self, results=None, regen=None, harnesses=None):
        with mock.patch.object(report, "version_sites", return_value=["crates/p/src/lib.rs:20"]), \
                mock.patch.object(report, "files_naming", return_value=["crates/p/src/play.rs"]), \
                mock.patch.object(report, "files_containing", return_value=["crates/w/src/blocks.rs"]):
            return report.build(diff(), mapping(), "26.2", "26.3", {"id": "26.3"}, results, regen=regen,
                                harnesses=harnesses)

    def test_harness_results_count_only_new_failures(self):
        ran = {"prepare": {"status": "failed", "error": "javac exited 1",
                           "compile_errors": ["VanillaBlockExporter.java:9: cannot find symbol: class Foo"]},
               "summary": {"pass": 1, "oracle": 1, "fail": 2, "new": 2, "known": 1},
               "harnesses": {"food": {"status": "oracle", "baseline": "pass", "new": True, "compile_errors": ["X"]},
                             "move": {"status": "fail", "baseline": "fail", "new": False},
                             "light": {"status": "fail", "baseline": "pass", "new": True},
                             "sky": {"status": "pass", "baseline": "pass", "new": False}}}
        built = self.build(harnesses=ran)
        [prepare] = [i for i in built["items"] if i["kind"] == "prepare-failed"]
        self.assertEqual(prepare["rank"], 1)
        failed = {h: i for i in built["items"] for h in i.get("failed", [])}
        self.assertEqual(failed["food"]["oracle"], ["food"])
        self.assertNotIn("move", failed)  # It fails on the old version too.
        self.assertEqual(failed["light"]["title"], "Harness light: fails on 26.3")
        text = report.markdown(built)
        self.assertIn("1 pass, 1 whose oracle no longer compiles, 2 fail; 2 newly failing, 1 failing on 26.2 too", text)
        self.assertIn("- javac: `VanillaBlockExporter.java:9: cannot find symbol: class Foo`", text)

    def test_regen_results_replace_the_stale_tables(self):
        regen = {"new": "26.3", "candidate": "/ports/26.3", "commit": "0123456789abcdef", "generators": {
            "packet_ids": {"status": "regenerated", "outputs": ["crates/horizon-protocol/src/ids.rs"],
                           "stat": {"crates/horizon-protocol/src/ids.rs": [142, 134]}},
            "entity_tracking": {"status": "unchanged", "outputs": ["crates/horizon-client/src/lan/tracking.rs"],
                                "stat": {}},
            "lan_command_order": {"status": "failed", "outputs": ["crates/horizon-client/src/lan/command_order.rs"],
                                  "error": "javac exited 1",
                                  "compile_errors": ["ParityData.java:45: cannot find symbol: variable WORLD"]}}}
        built = self.build(regen=regen)
        self.assertFalse([i for i in built["items"] if i["kind"] == "regenerate"])
        [failed] = [i for i in built["items"] if i["kind"] == "regen-failed"]
        self.assertEqual(failed["rank"], 1)
        self.assertIn("no longer compiles", failed["title"])
        self.assertEqual(built["summary"]["generators"],
                         {"packet_ids": "regenerated", "entity_tracking": "unchanged", "lan_command_order": "failed"})
        text = report.markdown(built)
        self.assertIn("regenerated 1, unchanged 1, failed 1", text)
        self.assertIn("`crates/horizon-protocol/src/ids.rs` (+142 -134)", text)
        self.assertIn("- javac: `ParityData.java:45: cannot find symbol: variable WORLD`", text)

    def test_ranks_and_kinds(self):
        built = self.build()
        kinds = [(i["rank"], i["kind"]) for i in built["items"]]
        self.assertEqual(sorted(k for r, k in kinds if r == 1),
                         ["components", "facts", "protocol", "regenerate", "regenerate", "regenerate",
                          "report-format", "save-format"])
        self.assertEqual([i["id"] for i in built["items"]], list(range(1, len(kinds) + 1)))
        self.assertEqual(kinds, sorted(kinds, key=lambda k: k[0]))
        regenerated = {i["files"][0] for i in built["items"] if i["kind"] == "regenerate"}
        self.assertEqual(regenerated, {"crates/horizon-core/src/version.rs", "crates/horizon-protocol/src/ids.rs",
                                       "crates/horizon-client/src/lan/tracking.rs"})

    def test_save_format_lists_new_schemas_and_fixes(self):
        save = next(i for i in self.build()["items"] if i["kind"] == "save-format")
        self.assertEqual(save["details"]["schemas"], ["net/minecraft/util/datafix/schemas/V4996"])
        self.assertEqual(save["details"]["fixes"], ["net/minecraft/util/datafix/fixes/SwingSplitFix"])
        self.assertEqual(save["files"], ["crates/w/src/datafix/mod.rs"])

    def test_code_items_by_file(self):
        items = {i["module"]: i for i in self.build()["items"] if "module" in i}
        food = items["crates/w/src/food.rs"]
        self.assertEqual((food["rank"], food["kind"], food["harnesses"]), (2, "rerun", ["food"]))
        self.assertEqual(food["classes"]["net/minecraft/world/food/FoodData"]["named"], ["tick"])
        sky = items["crates/c/src/sky.rs"]
        self.assertEqual((sky["rank"], sky["kind"]), (3, "unproved"))
        # Named by many files and none names the changed method: grouped by package, harnesses by coverage.
        widely = items["widely named: world/entity"]
        self.assertEqual(widely["harnesses"], ["food", "move"])
        unported = next(i for i in self.build()["items"] if i["kind"] == "unported")
        self.assertEqual(unported["packages"], {"net/minecraft/server": 1})

    def test_failed_harness_ranks_first_in_rank_two(self):
        items = self.build({"food": "fail", "move": "pass"})["items"]
        first = next(i for i in items if i["rank"] == 2)
        self.assertEqual((first["kind"], first["failed"]), ("harness-failed", ["food"]))

    def test_new_content(self):
        items = self.build()["items"]
        self.assertIn("minecraft:new_mob", next(i for i in items if i["kind"] == "content")["entries"])
        new = next(i for i in items if i["kind"] == "new-classes")
        self.assertEqual((new["classes"], new["ported"]),
                         (["net/minecraft/world/entity/NewMob"], ["net/minecraft/world/entity/NewMob"]))
        self.assertEqual(next(i for i in items if i["kind"] == "new-unported")["packages"],
                         {"net/minecraft/elsewhere": 1})

    def test_markdown(self):
        text = report.markdown(self.build())
        self.assertTrue(text.startswith("# Port report: Minecraft 26.2 -> 26.3"))
        self.assertIn("## 1. Blocks the switch", text)
        self.assertIn("play/serverbound: added minecraft:punch; removed minecraft:swing", text)
        self.assertIn("FoodData (2 methods; tick)", text)

    def test_helpers(self):
        self.assertEqual(report.snake("LivingEntity"), "living_entity")
        entry = {"rust": {"a/x.rs": ["tick"], "a/living_entity.rs": [], "a/y.rs": ["tick", "eat"]}}
        self.assertEqual(report.primary_file(entry, {"tick", "eat"}, "n/LivingEntity"), "a/y.rs")
        self.assertEqual(report.primary_file(entry, {"other"}, "n/LivingEntity"), "a/living_entity.rs")
        self.assertEqual(report.primary_file(entry, set(), "n/Other"), "a/living_entity.rs")
        self.assertIsNone(report.primary_file({"rust": {f"{i}.rs": [] for i in range(4)}}, set(), "n/Other"))


class VersionPortReportTests(unittest.TestCase):
    def test_harness_results_merge_shards(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "state.json"
            state.write_text(json.dumps({"harnesses": {"food": {"status": "pass"}, "datafix#1": {"status": "pass"},
                                                       "datafix#2": {"status": "fail"}}}))
            self.assertEqual(port.harness_results(state), {"food": "pass", "datafix": "fail"})

    def test_class_names_drop_moved_old_names(self):
        self.assertEqual(port.class_names({"a/X": 0, "a/Y": 0}, {"b/X": 0, "a/Y": 0}, {"a/X": "b/X"}),
                         ["a/Y", "b/X"])


class UsedThroughVersionPortTests(unittest.TestCase):
    """The module is used through `tools/version_port.py`; run on its own it says so."""

    def test_it_says_how_it_is_used(self):
        result = subprocess.run([sys.executable, str(ROOT / "tools/port_report.py"), "--help"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("version_port.py", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
