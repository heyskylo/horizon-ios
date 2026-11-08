#!/usr/bin/env python3
"""Check that each crate depends only on its own layer or a lower one (docs/layers.md).

engine < platform < horizon; `tool` crates (dev tools) may use anything and nothing shipped uses them.
tools/layers.json gives every crate its layer and lists today's exceptions in `allowed`. The check fails on:
- a workspace crate with no layer;
- a normal, build or optional dependency on a higher layer that `allowed` doesn't list;
- with --base, an `allowed` entry the branch added from a crate that already had a layer on the base;
- a shipped crate depending on a tool.
An `allowed` entry with no dependency left is reported, not failed. Dev-dependencies are not checked.

    python3 tools/check_layers.py                     # the check
    python3 tools/check_layers.py --base origin/main  # and no new exceptions since the base
    python3 tools/check_layers.py --list              # each crate's layer and the upward dependencies left
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


def upward(config, graph):
    """The dependencies that go up a layer, as 'crate -> dependency'."""
    rank = {layer: i for i, layer in enumerate(config["order"])}
    layers = config["crates"]
    out = []
    for crate in sorted(graph):
        for dep in sorted(graph[crate]):
            if layers.get(crate) in rank and layers.get(dep) in rank and rank[layers[dep]] > rank[layers[crate]]:
                out.append(f"{crate} -> {dep}")
    return out


def check(config, graph, base=None):
    """(problems, notes): problems fail the check, notes are only printed."""
    rank = {layer: i for i, layer in enumerate(config["order"])}
    layers = config["crates"]
    allowed = set(config.get("allowed", []))
    problems = []
    for crate in sorted(graph):
        layer = layers.get(crate)
        if layer is None:
            problems.append(f"{crate}: no layer. Give it one in {CONFIG_PATH} ({DOC}{WHERE} says which)")
        elif layer != TOOL and layer not in rank:
            problems.append(f"{crate}: unknown layer {layer!r} in {CONFIG_PATH}")
    for crate in sorted(graph):
        layer = layers.get(crate)
        if layer is None or layer == TOOL or layer not in rank:
            continue
        for dep in sorted(graph[crate]):
            # A path dependency with no layer is a vendored crate (third_party/) or a fixture.
            if layers.get(dep) == TOOL:
                problems.append(f"{crate} ({layer}) -> {dep} (tool): shipped code may not depend on a dev tool "
                                f"({DOC}{SECTION[TOOL]})")
    found = set(upward(config, graph))
    for edge in sorted(found - allowed):
        crate, dep = edge.split(" -> ")
        problems.append(f"{edge}: {layers[crate]} may not depend on {layers[dep]}. "
                        f"See {DOC}{SECTION[layers[crate]]}: move the code up, or offer an extension point "
                        f"the {layers[dep]} side plugs into")
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


def workspace_graph(config):
    graph = {}
    for manifest in config["workspaces"]:
        for crate, deps in edges(metadata(ROOT / manifest)).items():
            graph.setdefault(crate, set()).update(deps)
    return graph


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--base", help="refuse 'allowed' entries added since this git ref")
    parser.add_argument("--list", action="store_true", help="print each crate's layer and the upward dependencies")
    args = parser.parse_args(argv)
    config = load_config()
    graph = workspace_graph(config)
    if args.list:
        for crate in sorted(graph):
            print(f"{config['crates'].get(crate, '?'):<9} {crate}")
        ups = upward(config, graph)
        print(f"\n{len(ups)} upward dependencies:")
        for edge in ups:
            print(f"  {edge}")
        return 0
    problems, notes = check(config, graph, base_config(args.base) if args.base else None)
    for note in notes:
        print(f"note: {note}")
    if problems:
        print(f"Layer check failed ({DOC}):")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(f"Layer check passed: {len(graph)} crates, {len(config.get('allowed', []))} upward dependencies left "
          f"to move ({DOC}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
