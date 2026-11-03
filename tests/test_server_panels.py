"""The Pterodactyl egg for horizon-server (docs/hosting.md, "Hosting panels") name only
things the server has: its HORIZON_* variables, its start script, its ready line and the `port` key of server.toml."""
import json
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERVER = ROOT / "packaging/server"
EGG = SERVER / "pterodactyl-egg.json"
START = SERVER / "panel-start.sh"
INIT_RS = (ROOT / "crates/horizon-server/src/init.rs").read_text()
CONFIG_RS = (ROOT / "crates/horizon-server/src/config.rs").read_text()
DEDICATED_RS = (ROOT / "crates/horizon-client/src/game/dedicated.rs").read_text()

# Read at install only, so init.rs does not know them.
INSTALL_ONLY = {"HORIZON_VERSION"}
# The panel allocates the port: the start script passes SERVER_PORT, so no variable for it.
PANEL_OWNED = {"HORIZON_PORT"}


def init_variables():
    """The HORIZON_* names `Answers::from_env` reads."""
    body = INIT_RS.split("pub fn from_env", 1)[1].split("\n    }\n", 1)[0]
    return set(re.findall(r'(?:var|flag)\("(HORIZON_[A-Z]+)"\)', body))


class EggTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.egg = json.loads(EGG.read_text())

    def test_fields_pterodactyl_requires_for_a_PTDL_v2_import(self):
        # EggParserService accepts meta.version PTDL_v1 or PTDL_v2; the Egg model requires these.
        egg = self.egg
        self.assertEqual(egg["meta"]["version"], "PTDL_v2")
        for key in ("name", "author", "description", "features", "docker_images", "file_denylist", "startup", "config", "scripts", "variables"):
            self.assertIn(key, egg)
        self.assertTrue(1 <= len(egg["name"]) <= 191)
        self.assertRegex(egg["author"], r"^[^@\s]+@[^@\s]+$")
        self.assertTrue(egg["docker_images"] and all(isinstance(k, str) and isinstance(v, str) for k, v in egg["docker_images"].items()))
        self.assertTrue(egg["startup"].strip())
        for key in ("files", "startup", "logs"):
            self.assertIsInstance(json.loads(egg["config"][key]), dict)  # JSON documents stored as strings
        self.assertTrue(egg["config"]["stop"])
        install = egg["scripts"]["installation"]
        for key in ("script", "container", "entrypoint"):
            self.assertTrue(install[key])
        seen = set()
        for var in egg["variables"]:
            for key in ("name", "description", "env_variable", "default_value", "user_viewable", "user_editable", "rules", "field_type"):
                self.assertIn(key, var)
            self.assertRegex(var["env_variable"], r"^\w{1,191}$")
            self.assertNotIn(var["env_variable"], {"STARTUP", "P_SERVER_LOCATION", "P_SERVER_UUID", "SERVER_PORT", "SERVER_IP", "SERVER_MEMORY"})
            self.assertIsInstance(var["user_viewable"], bool)
            self.assertIsInstance(var["user_editable"], bool)
            self.assertIsInstance(var["default_value"], str)
            self.assertTrue(var["rules"])
            self.assertNotIn(var["env_variable"], seen)
            seen.add(var["env_variable"])

    def test_every_variable_is_one_the_server_reads(self):
        known = init_variables() | INSTALL_ONLY
        self.assertGreaterEqual(len(init_variables()), 7)
        for var in self.egg["variables"]:
            self.assertIn(var["env_variable"], known)
        # ...and every answer init reads can be set in the panel.
        self.assertEqual({v["env_variable"] for v in self.egg["variables"]} - INSTALL_ONLY, init_variables() - PANEL_OWNED)

    def test_the_eula_is_never_assumed(self):
        eula = next(v for v in self.egg["variables"] if v["env_variable"] == "HORIZON_EULA")
        self.assertEqual(eula["default_value"], "false")

    def test_the_start_script_is_the_one_in_the_repository(self):
        script = self.egg["scripts"]["installation"]["script"]
        embedded = script.split("<<'START'\n", 1)[1].split("\nSTART\n", 1)[0] + "\n"
        self.assertEqual(embedded, START.read_text())
        self.assertIn("horizon-start.sh", self.egg["startup"])

    def test_the_ready_line_is_the_one_the_game_logs(self):
        done = json.loads(self.egg["config"]["startup"])["done"]
        self.assertIn(done, DEDICATED_RS)

    def test_the_script_rewrites_a_key_server_toml_has(self):
        self.assertIn('^port = ', START.read_text())
        self.assertRegex(CONFIG_RS + INIT_RS, r"\bport\b")

    def test_scripts_parse(self):
        shell = shutil.which("sh")
        if not shell:
            self.skipTest("no sh")
        install = self.egg["scripts"]["installation"]["script"]
        with tempfile.TemporaryDirectory() as tmp:
            for name, text in (("install.sh", install), ("start.sh", START.read_text())):
                path = Path(tmp) / name
                path.write_text(text)
                interpreter = shutil.which("bash") if name == "install.sh" else shell
                result = subprocess.run([interpreter, "-n", str(path)], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
