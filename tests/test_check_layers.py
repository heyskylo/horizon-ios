import unittest

from tools import check_layers

CONFIG = {
    "order": ["engine", "platform", "horizon"],
    "crates": {
        "core": "engine",
        "game": "engine",
        "mod-api": "platform",
        "cosmetics": "horizon",
        "launcher": "horizon",
        "xtask": "tool",
    },
    "allowed": [],
}


def package(name, *deps):
    """A `cargo metadata` package; a dep is a name, or (name, kind) for dev/build."""
    out = []
    for dep in deps:
        dep_name, kind = dep if isinstance(dep, tuple) else (dep, None)
        out.append({"name": dep_name, "kind": kind, "path": f"/x/{dep_name}"})
    return {"name": name, "dependencies": out}


def run(*packages, allowed=(), base=None, crates=None):
    config = dict(CONFIG, allowed=list(allowed), crates=dict(CONFIG["crates"], **(crates or {})))
    return check_layers.check(config, check_layers.edges(packages), base)


class CheckLayersTests(unittest.TestCase):
    def test_downward_and_same_layer_dependencies_pass(self):
        problems, notes = run(package("mod-api", "core"), package("launcher", "game", "mod-api", "cosmetics"))
        self.assertEqual((problems, notes), ([], []))

    def test_engine_may_not_depend_on_platform_or_horizon(self):
        problems, _ = run(package("game", "mod-api", "cosmetics"))
        self.assertEqual(len(problems), 2)
        self.assertTrue(any("game -> mod-api: engine may not depend on platform" in p for p in problems))
        self.assertTrue(all("docs/layers.md#engine" in p for p in problems))

    def test_platform_may_not_depend_on_horizon(self):
        problems, _ = run(package("mod-api", "cosmetics"))
        self.assertIn("docs/layers.md#platform", problems[0])

    def test_allowed_upward_dependency_passes(self):
        self.assertEqual(run(package("game", "cosmetics"), allowed=["game -> cosmetics"]), ([], []))

    def test_build_dependencies_count_and_dev_dependencies_do_not(self):
        self.assertEqual(len(run(package("core", ("mod-api", "build")))[0]), 1)
        self.assertEqual(run(package("core", ("mod-api", "dev"))), ([], []))

    def test_an_exception_with_no_dependency_left_is_only_a_note(self):
        problems, notes = run(package("game", "core"), allowed=["game -> cosmetics"])
        self.assertEqual(problems, [])
        self.assertIn("game -> cosmetics", notes[0])

    def test_a_branch_may_not_add_exceptions_between_known_crates(self):
        base = {"crates": CONFIG["crates"], "allowed": []}
        problems, _ = run(package("game", "cosmetics"), allowed=["game -> cosmetics"], base=base)
        self.assertIn("added to 'allowed'", problems[0])
        kept = {"crates": CONFIG["crates"], "allowed": ["game -> cosmetics"]}
        self.assertEqual(run(package("game", "cosmetics"), allowed=["game -> cosmetics"], base=kept), ([], []))

    def test_a_renamed_crate_carries_its_exception(self):
        base = {"crates": CONFIG["crates"], "allowed": ["game -> cosmetics"]}
        problems, _ = run(package("gameplay", "cosmetics"), allowed=["gameplay -> cosmetics"], base=base,
                          crates={"gameplay": "engine"})
        self.assertEqual(problems, [])

    def test_a_new_dependency_target_excuses_nothing(self):
        base = {"crates": CONFIG["crates"], "allowed": []}
        problems, _ = run(package("game", "shop"), allowed=["game -> shop"], base=base, crates={"shop": "horizon"})
        self.assertIn("added to 'allowed'", problems[0])

    def test_unknown_crate_fails(self):
        self.assertIn("no layer", run(package("newcrate", "core"))[0][0])

    def test_tools_may_use_anything_but_nothing_shipped_uses_tools(self):
        self.assertEqual(run(package("xtask", "cosmetics", "game")), ([], []))
        self.assertIn("dev tool", run(package("game", "xtask"))[0][0])

    def test_dependencies_outside_the_layers_are_ignored(self):
        # Vendored crates (third_party/) and test fixtures have no layer.
        self.assertEqual(run(package("game", "naga")), ([], []))

    def test_portable_crates_depend_only_on_each_other_and_no_desktop_library(self):
        config = dict(CONFIG, portable={"crates": ["core"], "forbidden": ["wgpu"]})
        clean = {"name": "core", "dependencies": [
            {"name": "serde", "kind": None, "path": None},
            {"name": "wgpu", "kind": "dev", "path": None},
            {"name": "game", "kind": "dev", "path": "/x/game"},
        ]}
        self.assertEqual(check_layers.portable(config, [clean]), [])
        dirty = {"name": "core", "dependencies": [
            {"name": "game", "kind": None, "path": "/x/game"},
            {"name": "wgpu", "kind": "build", "path": None},
            {"name": "wgpu", "kind": None, "path": None, "target": "cfg(unix)"},
        ]}
        problems = check_layers.portable(config, [dirty])
        self.assertEqual(len(problems), 2, problems)
        self.assertIn("core -> game: a portable crate depends only on the other portable crates", problems[0])
        self.assertIn("core -> wgpu: a portable crate takes no window, GPU", problems[1])

    def test_a_portable_crate_is_an_engine_crate(self):
        config = dict(CONFIG, portable={"crates": ["mod-api", "gone"], "forbidden": []})
        self.assertEqual(check_layers.portable(config, []), [
            f"gone: listed as portable in {check_layers.CONFIG_PATH}, but not an engine crate there",
            f"mod-api: listed as portable in {check_layers.CONFIG_PATH}, but not an engine crate there",
        ])

    def test_registry_dependencies_are_not_edges(self):
        pkg = {"name": "game", "dependencies": [{"name": "cosmetics", "kind": None, "path": None}]}
        self.assertEqual(check_layers.edges([pkg]), {"game": set()})


if __name__ == "__main__":
    unittest.main()
