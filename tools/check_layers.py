#!/usr/bin/env python3
"""Check that each crate depends only on the layers its own layer may use (docs/layers.md).

tools/layers.json's `may_depend` says which: engine on engine; platform on engine and platform; horizon on
platform and horizon only, so the Horizon layer builds against the platform's public API and can live in its own
repository. `tool` crates (dev tools) may use anything and nothing shipped uses them. `crates` gives every crate its
layer and `allowed` lists today's exceptions. The check fails on:
- a workspace crate with no layer;
- a normal, build or optional dependency on a layer its crate's layer may not use that `allowed` doesn't list;
- with --base, an `allowed` entry the branch added from a crate that already had a layer on the base;
- a shipped crate depending on a tool;
- a portable crate depending on a crate of the repository that isn't portable, or on a `forbidden` library.
An `allowed` entry with no dependency left is reported, not failed. Dev-dependencies are not checked.

    python3 tools/check_layers.py                     # the check
    python3 tools/check_layers.py --base origin/main  # and no new exceptions since the base
    python3 tools/check_layers.py --list              # each crate's layer and the exceptions left
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "tools" / "layers.json"
CONFIG_PATH = "tools/layers.json"
TOOL = "tool"
DOC = "docs/layers.md"
# The section of docs/layers.md that says what a layer may depend on.
SECTION = {"engine": "#engine", "platform": "#platform", "horizon": "#horizon", TOOL: "#tools"}
WHERE = "#where-a-new-feature-goes"


def load_config(path=CONFIG):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def base_config(base):
    """tools/layers.json as it is on `base`, or None when the base has none."""
    shown = subprocess.run(["git", "show", f"{base}:{CONFIG_PATH}"], capture_output=True, text=True, cwd=ROOT)
    return json.loads(shown.stdout) if shown.returncode == 0 else None


def metadata(manifest):
    """`cargo metadata` for one workspace, without resolving dependencies (no network, no lockfile)."""
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--offline", "--format-version", "1", "--manifest-path", str(manifest)],
        check=True, capture_output=True, text=True, cwd=ROOT,
    ).stdout
    return json.loads(out)["packages"]


def edges(packages):
    """{crate: {dependency, ...}} for normal and build dependencies on other local crates (path dependencies)."""
    graph = {}
    for package in packages:
        deps = graph.setdefault(package["name"], set())
        for dep in package["dependencies"]:
            if dep.get("path") and dep.get("kind") != "dev":
                deps.add(dep["name"])
    return graph


def crossings(config, graph):
    """The dependencies on a layer the crate's layer may not use, as 'crate -> dependency'."""
    may = config["may_depend"]
    layers = config["crates"]
    out = []
    for crate in sorted(graph):
        for dep in sorted(graph[crate]):
            if layers.get(crate) in may and layers.get(dep) in may and layers[dep] not in may[layers[crate]]:
                out.append(f"{crate} -> {dep}")
    return out


def check(config, graph, base=None):
    """(problems, notes): problems fail the check, notes are only printed."""
    may = config["may_depend"]
    layers = config["crates"]
    allowed = set(config.get("allowed", []))
    problems = []
    for crate in sorted(graph):
        layer = layers.get(crate)
        if layer is None:
            problems.append(f"{crate}: no layer. Give it one in {CONFIG_PATH} ({DOC}{WHERE} says which)")
        elif layer != TOOL and layer not in may:
            problems.append(f"{crate}: unknown layer {layer!r} in {CONFIG_PATH}")
    for crate in sorted(graph):
        layer = layers.get(crate)
        if layer is None or layer == TOOL or layer not in may:
            continue
        for dep in sorted(graph[crate]):
            # A path dependency with no layer is a vendored crate (third_party/) or a fixture.
            if layers.get(dep) == TOOL:
                problems.append(f"{crate} ({layer}) -> {dep} (tool): shipped code may not depend on a dev tool "
                                f"({DOC}{SECTION[TOOL]})")
    found = set(crossings(config, graph))
    for edge in sorted(found - allowed):
        crate, dep = edge.split(" -> ")
        problems.append(f"{edge}: {layers[crate]} may not depend on {layers[dep]}. "
                        f"See {DOC}{SECTION[layers[crate]]}: move the code to the right layer, or offer an "
                        f"extension point the other side plugs into")
    if base is not None:
        base_layers = base.get("crates", {})
        for edge in sorted(allowed - set(base.get("allowed", []))):
            crate, dep = edge.split(" -> ")
            # A crate new since the base (split out of another, or renamed) may carry its old crate's exceptions;
            # a new dependency target excuses nothing.
            if crate in base_layers:
                problems.append(f"{edge}: added to 'allowed' in {CONFIG_PATH}. The exceptions only shrink; "
                                f"see {DOC}{SECTION.get(layers.get(crate), '')}")
    notes = [f"{edge}: no longer a dependency; you can remove it from 'allowed' in {CONFIG_PATH}"
             for edge in sorted(allowed - found)]
    return problems, notes


def portable(config, packages):
    """Problems of the portable crates (docs/layers.md#engine): a path dependency on a crate that isn't portable,
    or a normal or build dependency on a library they never take (window, GPU, sound, gamepad, clipboard, HTTP)."""
    spec = config.get("portable", {})
    crates, forbidden = set(spec.get("crates", [])), set(spec.get("forbidden", []))
    problems = {f"{crate}: listed as portable in {CONFIG_PATH}, but not an engine crate there"
                for crate in crates if config["crates"].get(crate) != "engine"}
    for package in packages:
        if package["name"] not in crates:
            continue
        for dep in package["dependencies"]:
            if dep.get("kind") == "dev":
                continue
            edge = f"{package['name']} -> {dep['name']}"
            if dep.get("path") and dep["name"] not in crates:
                problems.add(f"{edge}: a portable crate depends only on the other portable crates "
                             f"({DOC}{SECTION['engine']})")
            elif not dep.get("path") and dep["name"] in forbidden:
                problems.add(f"{edge}: a portable crate takes no window, GPU, sound, gamepad, clipboard or HTTP "
                             f"library ({DOC}{SECTION['engine']})")
    return sorted(problems)


def workspace_packages(config):
    return [package for manifest in config["workspaces"] for package in metadata(ROOT / manifest)]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base", help="refuse 'allowed' entries added since this git ref")
    parser.add_argument("--list", action="store_true", help="print each crate's layer and the exceptions")
    args = parser.parse_args(argv)
    config = load_config()
    packages = workspace_packages(config)
    graph = edges(packages)
    if args.list:
        for crate in sorted(graph):
            print(f"{config['crates'].get(crate, '?'):<9} {crate}")
        ups = crossings(config, graph)
        print(f"\n{len(ups)} dependencies on a layer the crate's layer may not use:")
        for edge in ups:
            print(f"  {edge}")
        return 0
    problems, notes = check(config, graph, base_config(args.base) if args.base else None)
    problems += portable(config, packages)
    for note in notes:
        print(f"note: {note}")
    if problems:
        print(f"Layer check failed ({DOC}):")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(f"Layer check passed: {len(graph)} crates, {len(config.get('allowed', []))} exceptions left "
          f"to move ({DOC}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
