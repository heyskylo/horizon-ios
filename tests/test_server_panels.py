"""The hosting panel setups for horizon-server (docs/hosting.md, "Hosting panels"), the Pterodactyl egg and the AMP
template, name only things the server has: its HORIZON_* variables, its start script, its ready line and the `port`
key of server.toml."""
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
AMP = SERVER / "amp"
INIT_RS = (ROOT / "crates/horizon-server/src/init.rs").read_text()
CONFIG_RS = (ROOT / "crates/horizon-server/src/config.rs").read_text()
DEDICATED_RS = (ROOT / "crates/horizon-client/src/game/dedicated.rs").read_text()
LAN_RS = (ROOT / "crates/horizon-client/src/game/lan.rs").read_text()

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


def read_kvp(path):
    """AMP's `Key=Value` lines (the value may itself hold `=`)."""
    pairs = {}
    for line in path.read_text().splitlines():
        if line.strip():
            key, value = line.split("=", 1)
            pairs[key] = value
    return pairs


class AmpTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kvp = read_kvp(AMP / "horizon.kvp")
        cls.config = json.loads((AMP / "horizonconfig.json").read_text())
        cls.updates = json.loads((AMP / "horizonupdates.json").read_text())
        cls.ports = json.loads((AMP / "horizonports.json").read_text())
        cls.egg = json.loads(EGG.read_text())

    def test_every_file_the_template_names_is_there(self):
        kvp = self.kvp
        self.assertEqual(kvp["Meta.ConfigRoot"], "horizon.kvp")
        for key in ("Meta.ConfigManifest", "Meta.MetaConfigManifest"):
            self.assertTrue((AMP / kvp[key]).is_file(), kvp[key])
        for key in ("App.Ports", "App.UpdateSources"):
            name = re.fullmatch(r"@IncludeJson\[(.+)\]", kvp[key]).group(1)
            self.assertTrue((AMP / name).is_file(), name)
        for path in AMP.glob("*.json"):
            json.loads(path.read_text())

    def test_settings_are_the_eggs_variables(self):
        # Same names, defaults and kinds, so both panels set a server up the same way.
        settings = {s["FieldName"]: s for s in self.config}
        env = json.loads(self.kvp["App.EnvironmentVariables"])
        passed = {name: re.fullmatch(r"\{\{(\w+)\}\}", value).group(1) for name, value in env.items() if name.startswith("HORIZON_")}
        for name, field in passed.items():
            self.assertIn(field, settings, name)
        # HORIZON_VERSION is read at install only: the install stage passes it.
        install = [f for f in settings if "{{%s}}" % f in json.dumps(self.updates)]
        self.assertEqual(len(install), 1)
        fields = dict(passed, HORIZON_VERSION=install[0])
        self.assertEqual(len(self.config), len(fields))
        for var in self.egg["variables"]:
            name = var["env_variable"]
            setting = settings[fields[name]]
            self.assertEqual(setting["DefaultValue"], var["default_value"], name)
            boolean = "in:true,false" in var["rules"]
            self.assertEqual(setting["InputType"], "checkbox" if boolean else "text", name)
            if boolean:
                self.assertEqual(sorted(setting["EnumValues"].values()), ["false", "true"])
        # Every answer init reads reaches the server, and the port follows AMP's.
        self.assertEqual(set(passed), init_variables() - PANEL_OWNED)
        self.assertEqual(env["SERVER_PORT"], "{{$%s}}" % self.kvp["App.PrimaryApplicationPortRef"])
        self.assertIn(self.kvp["App.PrimaryApplicationPortRef"], {p["Ref"] for p in self.ports})

    def test_it_installs_with_the_eggs_script_and_starts_with_the_start_script(self):
        created = next(u for u in self.updates if u["UpdateSource"] == "CreateFile")
        self.assertEqual(created["UpdateSourceData"], self.egg["scripts"]["installation"]["script"])
        script = created["UpdateSourceArgs"].rsplit("}", 1)[1]
        run = next(u for u in self.updates if u["UpdateSource"] == "Executable")
        self.assertIn(script, run["UpdateSourceArgs"])
        self.assertIn("HORIZON_INSTALL_DIR='{{$FullBaseDir}}'", run["UpdateSourceArgs"])
        # The install script writes horizon-start.sh (panel-start.sh) beside the program; AMP runs it.
        self.assertIn('cat > "$DIR/horizon-start.sh"', created["UpdateSourceData"])
        self.assertTrue(self.kvp["App.CommandLineArgs"].endswith("horizon-start.sh"))
        self.assertEqual(self.kvp["App.ExitString"], "stop")

    def test_the_console_patterns_match_what_the_server_logs(self):
        done = re.search(r'"(Done \(\{:\.3\}s\)! For help, type \\"help\\")"', DEDICATED_RS).group(1)
        line = "[12:00:00] [Server thread/INFO]: " + done.replace("{:.3}", "4.217").replace('\\"', '"')
        self.assertRegex(line, self.kvp["Console.AppReadyRegex"])
        self.assertNotRegex("[12:00:00] [Server thread/INFO]: Preparing spawn area", self.kvp["Console.AppReadyRegex"])
        for key, said in (("Console.UserJoinRegex", "joined"), ("Console.UserLeaveRegex", "left")):
            self.assertIn(f'"System chat: {{}} {said} the game"', LAN_RS)
            found = re.search(self.kvp[key].replace("(?<", "(?P<"), f"[12:00:00] [Server thread/INFO]: System chat: Steve_2 {said} the game")
            self.assertEqual(found.group("username"), "Steve_2")


if __name__ == "__main__":
    unittest.main()
