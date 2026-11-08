"""Regeneration covers the shipped tables and check mode cannot rewrite them."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from tools import generate_registry_id_adapter as generator
from tools import regenerate_registry_id_adapters as regenerate


class RegistryRegeneration(unittest.TestCase):
    def test_every_shipped_release_has_a_regeneration_recipe(self):
        shipped = {p.stem[1:-4].replace("_", ".")
                   for p in (regenerate.ROOT / regenerate.ADAPTER).glob("v*_ids.rs")}
        self.assertEqual(shipped, set(regenerate.RELEASES))

    def test_check_reports_drift_without_writing_existing_or_missing_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / regenerate.ADAPTER
            target.mkdir(parents=True)
            generated = root / "generated.rs"
            generated.write_text("fresh\n")
            with patch.object(regenerate, "ROOT", root), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(regenerate.sync_generated([generated], check=True), 1)
                self.assertFalse((target / generated.name).exists())
                (target / generated.name).write_text("previous\n")
                self.assertEqual(regenerate.sync_generated([generated], check=True), 1)
                self.assertEqual((target / generated.name).read_text(), "previous\n")
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(regenerate.sync_generated([generated], check=False), 0)
                    self.assertEqual(regenerate.sync_generated([generated], check=True), 0)
                self.assertEqual((target / generated.name).read_text(), "fresh\n")

    def test_legacy_nbt_release_keeps_inherited_components_and_substitutes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for version in ("old", "new", "inherited"):
                prepared = root / version
                (prepared / "jars").mkdir(parents=True)
                with zipfile.ZipFile(prepared / "jars/client.jar", "w") as archive:
                    archive.writestr("version.json", json.dumps({"id": version, "name": version}))
                reports = prepared / "datagen/reports"
                reports.mkdir(parents=True)
                (reports / "registries.json").write_text(json.dumps({
                    "minecraft:item": {"entries": {"minecraft:stone": {"protocol_id": 0}}}}))
                (reports / "blocks.json").write_text(json.dumps({
                    "minecraft:stone": {"states": [{"id": 0, "default": True}]}}))
            output = root / "ids.rs"
            args = ["generate_registry_id_adapter.py", "--old", str(root / "old"),
                    "--new", str(root / "new"), "--inherit", str(root / "inherited"), "--out", str(output)]
            with patch("sys.argv", args), contextlib.redirect_stdout(io.StringIO()):
                generator.main()
            text = output.read_text()
            self.assertNotIn("DROPPED_COMPONENTS", text)
            self.assertNotIn("substitutes:", text)
            self.assertIn("..super::vinherited_ids::IDS", text)


if __name__ == "__main__":
    unittest.main()
