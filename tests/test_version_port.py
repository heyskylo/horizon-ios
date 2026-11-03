import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from tools import version_port as port


def jar(path, files):
    with zipfile.ZipFile(path, "w") as archive:
        for name, value in files.items():
            archive.writestr(name, value if isinstance(value, (str, bytes)) else json.dumps(value))
    return path


class DetectTests(unittest.TestCase):
    MANIFEST = {"latest": {"release": "26.4", "snapshot": "26.5-snapshot-1"}}

    def test_new_release_and_snapshot(self):
        self.assertEqual(port.news(self.MANIFEST, "26.3", {}), [("release", "26.4")])
        self.assertEqual(port.news(self.MANIFEST, "26.3", {}, snapshots=True),
                         [("release", "26.4"), ("snapshot", "26.5-snapshot-1")])

    def test_pinned_or_seen_is_not_news(self):
        self.assertEqual(port.news(self.MANIFEST, "26.4", {}), [])
        self.assertEqual(port.news(self.MANIFEST, "26.3", {"release": "26.4", "snapshot": "26.5-snapshot-1"},
                                   snapshots=True), [])

    def test_a_release_is_not_also_reported_as_the_latest_snapshot(self):
        manifest = {"latest": {"release": "26.4", "snapshot": "26.4"}}
        self.assertEqual(port.news(manifest, "26.3", {}, snapshots=True), [("release", "26.4")])

    def test_unsafe_versions_are_refused(self):
        for version in ["..", "26.4/../x", ""]:
            with self.assertRaises(ValueError):
                port.safe_version(version)


class ReportDiffTests(unittest.TestCase):
    def test_json_diff_paths(self):
        self.assertEqual(port.json_diff({"a": 1, "b": [1, 2]}, {"a": 2, "b": [1, 3], "c": True}),
                         [("/a", 1, 2), ("/b[1]", 2, 3), ("/c", None, True)])
        self.assertEqual(port.json_diff([1], [1, 2]), [("/", [1], [1, 2])])
        self.assertEqual(len(port.json_diff({str(i): i for i in range(9)}, {}, limit=3)), 3)

    def test_packets_added_removed_renumbered(self):
        old = {"play": {"serverbound": {"minecraft:swing": {"protocol_id": 0}, "minecraft:use": {"protocol_id": 1}}}}
        new = {"play": {"serverbound": {"minecraft:punch": {"protocol_id": 0}, "minecraft:use": {"protocol_id": 2},
                                        "minecraft:x": {"protocol_id": 1}}}}
        self.assertEqual(port.diff_packets(old, new), {"play/serverbound": {
            "added": {"minecraft:punch": 0, "minecraft:x": 1}, "removed": {"minecraft:swing": 0},
            "renumbered": {"minecraft:use": [1, 2]}, "count": [2, 3]}})
        self.assertEqual(port.diff_packets(old, old), {})

    def test_registries(self):
        def registry(*names, default=None):
            return {"default": default, "entries": {n: {"protocol_id": i} for i, n in enumerate(names)}}
        old = {"minecraft:item": registry("air", "stone"), "minecraft:gone": registry("a")}
        new = {"minecraft:item": registry("air", "dirt", "stone"), "minecraft:new": registry("b")}
        self.assertEqual(port.diff_registries(old, new), {
            "added": {"minecraft:new": ["b"]}, "removed": ["minecraft:gone"],
            "changed": {"minecraft:item": {"added": ["dirt"], "ids_shifted": 1}}})

    def test_blocks_state_ids_and_format(self):
        def block(first, *values, default=0, **extra):
            states = [{"id": first + i, "properties": {"p": v}} for i, v in enumerate(values)]
            states[default]["default"] = True
            return {"properties": {"p": list(values)}, "states": states, **extra}
        old = {"a": block(0, "x", "y"), "b": block(2, "x")}
        new = {"a": block(0, "x", "y", "z", default=1, definition={}),
               "b": block(3, "x", definition={}), "c": block(4, "x", definition={})}
        result = port.diff_blocks(old, new)
        self.assertEqual(result["added"], ["c"])
        self.assertEqual(result["format"], ["definition"])
        self.assertEqual(set(result["changed"]), {"a"})
        self.assertEqual(result["changed"]["a"]["states"], [2, 3])
        self.assertEqual(result["changed"]["a"]["default"], [{"p": "x"}, {"p": "y"}])
        self.assertEqual((result["state_ids_shifted"], result["first_shifted"]), (1, "b"))

    def test_items_by_component(self):
        old = {"stone": {"swing": {}, "name": "Stone"}, "dirt": {"swing": {}}}
        new = {"stone": {"attack": {}, "name": "Rock"}, "dirt": {"attack": {}}, "mud": {}}
        result = port.diff_items(old, new)
        self.assertEqual(result["added"], ["mud"])
        self.assertEqual(result["components"], {"attack": {"added": 2, "removed": 0, "changed": 0},
                                                "swing": {"added": 0, "removed": 2, "changed": 0},
                                                "name": {"added": 0, "removed": 0, "changed": 1}})

    def test_command_paths(self):
        def literal(**children):
            return {"type": "literal", "children": children} if children else {"type": "literal", "executable": True}
        old = {"type": "root", "children": {"data": literal(get=literal(block=literal(targetPos=literal())))}}
        new = {"type": "root", "children": {"data": literal(get=literal(block=literal(pos=literal()))),
                                            "compute": literal()}}
        result = port.diff_commands(old, new)
        self.assertEqual(result["added"], ["compute", "data get block pos"])
        self.assertEqual(result["removed"], ["data get block targetPos"])


class JarDiffTests(unittest.TestCase):
    def test_contents_lang_and_facts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            common = {"data/minecraft/tags/block/logs.json": {"values": ["oak"]}}
            old = jar(root / "old.jar", {**common,
                                         "data/minecraft/recipe/a.json": {"result": "a", "count": 1},
                                         "data/minecraft/recipe/gone.json": {},
                                         "assets/minecraft/lang/en_us.json": {"k": "K", "same": "S"}})
            new = jar(root / "new.jar", {**common,
                                         "data/minecraft/recipe/a.json": {"result": "a", "count": 2},
                                         "data/minecraft/worldgen/biome/new.json": {},
                                         "assets/minecraft/lang/en_us.json": {"k": "K2", "same": "S", "n": "N"},
                                         "version.json": {"id": "26.4", "world_version": 5100,
                                                          "protocol_version": 778, "java_version": 25,
                                                          "pack_version": {"resource_major": 98, "resource_minor": 0,
                                                                           "data_major": 122, "data_minor": 0}}})
            data = port.diff_contents(old, new, "data/minecraft/")
            self.assertEqual(data["counts"], {"added": 1, "removed": 1, "changed": 1})
            self.assertEqual(data["kinds"]["recipe"], {"added": [], "removed": ["data/minecraft/recipe/gone.json"],
                                                       "changed": ["data/minecraft/recipe/a.json"]})
            self.assertEqual(data["kinds"]["worldgen/biome"]["added"], ["data/minecraft/worldgen/biome/new.json"])
            self.assertEqual(data["json"], {"data/minecraft/recipe/a.json": [("/count", 1, 2)]})
            self.assertEqual(port.diff_lang(old, new), {"added": ["n"], "removed": [], "changed": ["k"]})
            facts = port.jar_facts(new)
            self.assertEqual((facts["world_version"], facts["protocol_version"], facts["data_pack"]),
                             (5100, 778, "122.0"))
            self.assertEqual(port.diff_facts({"id": "a", "protocol_version": 777}, {"id": "b", "protocol_version": 778}),
                             {"protocol_version": [777, 778]})


class GcTests(unittest.TestCase):
    def test_keeps_pinned_newest_two_and_their_diffs(self):
        entries = [("26.1", 1, True), ("26.2", 2, True), ("26.3", 3, True), ("26.4-snapshot-1", 4, True),
                   ("26.4-snapshot-2", 5, True), ("diff-26.2-26.3", 6, False),
                   ("diff-26.3-26.4-snapshot-2", 7, False), (".datagen-x", 8, False), ("detect-state.json", 9, False)]
        self.assertEqual(port.to_remove(entries, "26.3", set()),
                         [".datagen-x", "26.1", "26.2", "diff-26.2-26.3"])
        self.assertEqual(port.to_remove(entries, "26.3", {"26.1"}),
                         [".datagen-x", "26.2", "diff-26.2-26.3"])


if __name__ == "__main__":
    unittest.main()
