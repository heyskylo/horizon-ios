#!/usr/bin/env python3
"""Port a new Minecraft release to Horizon: detect it, prepare it, diff it against the pinned one
(docs/version-ports.md).

    version_port.py detect [--snapshots]          what Mojang released that Horizon doesn't pin
    version_port.py prepare VERSION               download, verify, run the data generator, read its facts,
                                                  index its classes (tools/class_index.py)
    version_port.py diff OLD NEW                  facts, packets, registries, blocks, items, commands,
                                                  data packs, assets, lang and classes, with the Horizon
                                                  code each changed class concerns: diff-OLD-NEW/diff.json
    version_port.py regen NEW [--base REF]        a port/NEW worktree pinned to NEW with every generated table
                                                  regenerated (tools/port_regen.py): diff-OLD-NEW/regen.json
    version_port.py harnesses NEW [--baseline S]  horizon-prepare and every harness in the candidate, each pass,
                                                  oracle (no longer compiles) or fail, and newly failing against
                                                  a run on the pinned version (tools/port_harness.py)
    version_port.py report OLD NEW [--results S]  the ranked port report: diff-OLD-NEW/report.md and .json
    version_port.py publish OLD NEW [--yes]       the report's work items as GitHub issues (tools/port_publish.py);
                                                  a dry run without --yes
    version_port.py port NEW [--old OLD]          prepare both, diff, regen, report
    version_port.py gc [--keep VERSION...]        keep the pinned, the newest two others and --keep

The downloads are for development only: they go to the version store (~/code/horizon-parity/versions,
HORIZON_VERSIONS overrides it), never into the repository, and nothing from them is shipped. Candidate
worktrees go to ~/code/horizon-parity/ports/NEW (HORIZON_PORTS overrides it). The pinned
version is not downloaded: its files are read where tools/lab.py prepared them (.local/).

detect exits 0 when nothing is new, 10 for a new release and 11 for a new snapshot (with --snapshots).
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

try:
    from . import class_index, lab, port_harness, port_publish, port_regen, port_report, vanilla_map
except ImportError:
    import class_index
    import lab
    import port_harness
    import port_publish
    import port_regen
    import port_report
    import vanilla_map

MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
STORE = Path(os.environ.get("HORIZON_VERSIONS", Path.home() / "code" / "horizon-parity" / "versions"))
PORTS = Path(os.environ.get("HORIZON_PORTS", STORE.parent / "ports"))
STATES = ["handshake", "status", "login", "configuration", "play"]
# How many changed JSON paths one changed file keeps in diff.json.
PATH_LIMIT = 50


def safe_version(version):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", version) or version in {".", ".."}:
        raise ValueError(f"Unsafe version identifier: {version!r}")
    return version


def fetch_json(url, sha1=None):
    request = urllib.request.Request(url, headers={"User-Agent": "horizon/0.1"})
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read()
    if sha1 is not None and hashlib.sha1(data).hexdigest() != sha1:
        raise ValueError(f"{url}: sha1 mismatch")
    return data


def pinned():
    return json.loads(lab.LOCK.read_text(encoding="utf-8"))["minecraft"]["version"]


# --- detect ------------------------------------------------------------------------------------------------

def news(manifest, pin, seen, snapshots=False):
    """[(kind, version)] Mojang lists that are neither pinned nor seen before, newest kind first."""
    found = []
    for kind in ["release"] + (["snapshot"] if snapshots else []):
        latest = manifest["latest"][kind]
        if latest != pin and latest != seen.get(kind):
            # The latest snapshot is the latest release right after a release: report it once.
            if kind == "snapshot" and latest == manifest["latest"]["release"]:
                continue
            found.append((kind, latest))
    return found


def detect(args):
    manifest = json.loads(fetch_json(MANIFEST_URL))
    state_path = STORE / "detect-state.json"
    seen = json.loads(state_path.read_text()) if state_path.exists() else {}
    found = news(manifest, pinned(), seen, args.snapshots)
    if not found:
        print(f"Nothing new: latest release {manifest['latest']['release']}, Horizon pins {pinned()}")
        return 0
    for kind, version in found:
        print(f"New {kind}: {version} (Horizon pins {pinned()})")
        seen[kind] = version
    if not args.dry_run:
        lab.write_json(state_path, seen)
    return 10 if found[0][0] == "release" else 11


# --- prepare -----------------------------------------------------------------------------------------------

def version_dir(version):
    return STORE / safe_version(version)


def paths(version):
    """The prepared files of a version: {client, server, server_game, reports, version_json}."""
    record = version_dir(version) / "prepared.json"
    if not record.exists():
        raise SystemExit(f"{version} is not prepared; run version_port.py prepare {version}")
    return {key: Path(value) for key, value in json.loads(record.read_text())["paths"].items()}


def jar_facts(client_jar):
    """The facts the game states about itself in the client jar's version.json."""
    with zipfile.ZipFile(client_jar) as archive:
        info = json.loads(archive.read("version.json"))
    pack = info.get("pack_version", {})
    return {
        "id": info["id"],
        "world_version": info["world_version"],
        "protocol_version": info["protocol_version"],
        "resource_pack": f"{pack.get('resource_major')}.{pack.get('resource_minor')}",
        "data_pack": f"{pack.get('data_major')}.{pack.get('data_minor')}",
        "java_version": info["java_version"],
        "stable": info.get("stable"),
    }


def unpack_game(server, destination, version):
    """The server download is a bundler: take its game jar only (the libraries stay inside it)."""
    with zipfile.ZipFile(server) as archive:
        if "META-INF/versions.list" not in archive.namelist():
            shutil.copyfile(server, destination)
            return destination
        entries = list(lab.bundle_entries(archive, "META-INF/versions.list"))
        if len(entries) != 1 or entries[0][1] != version:
            raise ValueError(f"Expected one bundled server for {version}")
        checksum, _, relative = entries[0]
        lab.extract_verified(archive, f"META-INF/versions/{relative}", destination, checksum)
    return destination


def java_command(major):
    java = Path(os.environ["JAVA_HOME"]) / "bin" / "java" if os.environ.get("JAVA_HOME") else shutil.which("java")
    if not java:
        raise SystemExit("Java not found: set JAVA_HOME or put java on the PATH")
    result = subprocess.run([str(java), "-version"], capture_output=True, text=True)
    found = re.search(r'version "(\d+)', result.stderr + result.stdout)
    if not found or int(found[1]) < major:
        raise SystemExit(f"This version needs Java {major}; {java} is {found[1] if found else 'unknown'}. "
                         f"Install JDK {major} (a fleet toolchain change) and rerun.")
    return str(java)


def run_datagen(server, reports, major):
    """vanilla's data generator, --reports only. The bundler unpacks its libraries into a scratch folder."""
    scratch = Path(tempfile.mkdtemp(prefix=".datagen-", dir=STORE))
    out = scratch / "out"
    try:
        log = scratch / "datagen.log"
        with log.open("w") as stream:
            result = subprocess.run([java_command(major), "-DbundlerMainClass=net.minecraft.data.Main", "-jar",
                                     str(server), "--reports", "--output", str(out)],
                                    cwd=scratch, stdout=stream, stderr=subprocess.STDOUT)
        if result.returncode:
            raise SystemExit(f"The data generator failed:\n{log.read_text()[-3000:]}")
        if reports.exists():
            shutil.rmtree(reports)
        shutil.move(str(out / "reports"), reports)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def prepare_pinned(version, directory):
    """The pinned version's files are already in .local: record where, don't copy them."""
    game = lab.LOCAL / "minecraft" / version
    found = {
        "client": game / "jars" / "client.jar",
        "server": game / "jars" / "server.jar",
        "server_game": game / "jars" / "server-game.jar",
        "reports": lab.LOCAL / "datagen" / "out" / "reports",
        "version_json": game / "manifest.json",
    }
    missing = [str(p) for p in found.values() if not p.exists()]
    if missing:
        raise SystemExit("The pinned version is not prepared in .local (tools/lab.py prepare, horizon-prepare): "
                         + ", ".join(missing))
    return {key: path.resolve() for key, path in found.items()}


def prepare_download(version, directory):
    manifest = json.loads(fetch_json(MANIFEST_URL))
    entry = next((v for v in manifest["versions"] if v["id"] == version), None)
    if entry is None:
        raise SystemExit(f"Mojang's manifest has no version {version}")
    version_json = directory / "version.json"
    if not (version_json.exists() and lab.digest(version_json, "sha1") == entry["sha1"]):
        version_json.write_bytes(fetch_json(entry["url"], entry["sha1"]))
    meta = json.loads(version_json.read_text())
    if "client_mappings" in meta["downloads"]:
        raise SystemExit(f"{version} is obfuscated; the pipeline handles unobfuscated releases (26.1 and later)")
    jars = directory / "jars"
    found = {"version_json": version_json}
    for side in ["client", "server"]:
        download = meta["downloads"][side]
        print(f"Fetching the {version} {side} jar ({download['size'] // 1_000_000} MB)", flush=True)
        found[side] = lab.download(download["url"], jars / f"{side}.jar", download["sha1"])
    found["server_game"] = unpack_game(found["server"], jars / "server-game.jar", version)
    found["reports"] = directory / "datagen" / "reports"
    if not found["reports"].exists():
        print(f"Running the {version} data generator", flush=True)
        run_datagen(found["server"], found["reports"], meta["javaVersion"]["majorVersion"])
    return found


def prepare(args):
    version = safe_version(args.version)
    directory = version_dir(version)
    directory.mkdir(parents=True, exist_ok=True)
    found = (prepare_pinned if version == pinned() and not args.download else prepare_download)(version, directory)
    facts = jar_facts(found["client"])
    if facts["id"] != version:
        raise ValueError(f"The client jar says it is {facts['id']}, not {version}")
    lab.write_json(directory / "facts.json", facts)
    classes = class_index.index_jars([found["client"], found["server_game"]], jobs=args.jobs)
    (directory / "classes.json").write_text(json.dumps(classes, separators=(",", ":")))
    print(f"Indexed {len(classes)} classes, {sum(len(c['methods']) for c in classes.values())} methods")
    lab.write_json(directory / "prepared.json", {
        "version": version, "pinned": version == pinned() and not args.download,
        "paths": {key: str(path) for key, path in found.items()},
    })
    print(json.dumps(facts, indent=2))
    return 0


# --- diff --------------------------------------------------------------------------------------------------

def clip(value, limit=200):
    text = json.dumps(value, sort_keys=True)
    return value if len(text) <= limit else text[:limit] + "..."


def json_equal(a, b):
    """Compare JSON values without treating booleans as numbers."""
    if a is b:
        return True
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(json_equal(a[key], b[key]) for key in a)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(json_equal(x, y) for x, y in zip(a, b))
    return isinstance(a, bool) == isinstance(b, bool) and a == b


def json_diff(old, new, path="", limit=PATH_LIMIT):
    """[(path, old, new)] where two JSON values differ; lists of different lengths count as one change."""
    changes = []

    def walk(a, b, at):
        if len(changes) >= limit or json_equal(a, b):
            return
        if isinstance(a, dict) and isinstance(b, dict):
            for key in sorted(a.keys() | b.keys()):
                walk(a.get(key), b.get(key), f"{at}/{key}")
        elif isinstance(a, list) and isinstance(b, list) and len(a) == len(b):
            for index, (x, y) in enumerate(zip(a, b)):
                walk(x, y, f"{at}[{index}]")
        else:
            changes.append((at or "/", clip(a), clip(b)))

    walk(old, new, path)
    return changes


def keyed_diff(old, new):
    """Added, removed and common keys of two mappings."""
    return sorted(new.keys() - old.keys()), sorted(old.keys() - new.keys()), sorted(old.keys() & new.keys())


def diff_facts(old, new):
    return {key: [old.get(key), new.get(key)] for key in sorted(old.keys() | new.keys())
            if key != "id" and not json_equal(old.get(key), new.get(key))}


def diff_packets(old, new):
    """Per state and direction: packets added, removed and renumbered (the ids are on the wire)."""
    result = {}
    for state in sorted(old.keys() | new.keys(), key=lambda s: (STATES.index(s) if s in STATES else 99, s)):
        for direction in sorted(old.get(state, {}).keys() | new.get(state, {}).keys()):
            a = {name: e["protocol_id"] for name, e in old.get(state, {}).get(direction, {}).items()}
            b = {name: e["protocol_id"] for name, e in new.get(state, {}).get(direction, {}).items()}
            added, removed, common = keyed_diff(a, b)
            renumbered = {name: [a[name], b[name]] for name in common if a[name] != b[name]}
            if added or removed or renumbered:
                result[f"{state}/{direction}"] = {
                    "added": {name: b[name] for name in added}, "removed": {name: a[name] for name in removed},
                    "renumbered": renumbered, "count": [len(a), len(b)]}
    return result


def diff_registries(old, new):
    """Registries added and removed; per registry its entries added and removed, and how many common
    entries changed protocol id (the network sends those ids)."""
    added, removed, common = keyed_diff(old, new)
    result = {"added": {name: sorted(new[name].get("entries", {})) for name in added}, "removed": removed,
              "changed": {}}
    for name in common:
        a, b = old[name].get("entries", {}), new[name].get("entries", {})
        entries_added, entries_removed, shared = keyed_diff(a, b)
        shifted = [e for e in shared if a[e].get("protocol_id") != b[e].get("protocol_id")]
        change = {}
        if entries_added:
            change["added"] = entries_added
        if entries_removed:
            change["removed"] = entries_removed
        if shifted:
            change["ids_shifted"] = len(shifted)
        if old[name].get("default") != new[name].get("default"):
            change["default"] = [old[name].get("default"), new[name].get("default")]
        if change:
            result["changed"][name] = change
    return result


def diff_blocks(old, new):
    """Blocks added and removed; per block property changes, state counts and default state; and the
    global block-state ids (the network and the chunk palette use them)."""
    added, removed, common = keyed_diff(old, new)
    # A field one report has for no block at all is a change of the report's format, not of every block.
    fields = [{key for block in side.values() for key in block} for side in (old, new)]
    format_change = sorted(fields[0] ^ fields[1])

    def default(block):
        return next((s.get("properties", {}) for s in block["states"] if s.get("default")), {})

    def first_id(block):
        return min(s["id"] for s in block["states"])

    changed = {}
    for name in common:
        a, b = old[name], new[name]
        change = {}
        if not json_equal(a.get("properties", {}), b.get("properties", {})):
            change["properties"] = json_diff(a.get("properties", {}), b.get("properties", {}))
        if len(a["states"]) != len(b["states"]):
            change["states"] = [len(a["states"]), len(b["states"])]
        if not json_equal(default(a), default(b)):
            change["default"] = [default(a), default(b)]
        if "definition" not in format_change and not json_equal(a.get("definition"), b.get("definition")):
            change["definition"] = json_diff(a.get("definition"), b.get("definition"))
        if change:
            changed[name] = change
    shifted = [name for name in common if first_id(old[name]) != first_id(new[name])]
    first = min(shifted, key=lambda n: first_id(new[n]), default=None)
    return {"added": added, "removed": removed, "changed": changed, "format": format_change,
            "state_count": [sum(len(b["states"]) for b in old.values()), sum(len(b["states"]) for b in new.values())],
            "state_ids_shifted": len(shifted), "first_shifted": first}


def diff_items(old, new):
    """Items added and removed; per item the default components added, removed and changed."""
    added, removed, common = keyed_diff(old, new)
    changed = {}
    for name in common:
        a, b = old[name], new[name]
        components_added, components_removed, shared = keyed_diff(a, b)
        values = {c: json_diff(a[c], b[c], limit=10) for c in shared if not json_equal(a[c], b[c])}
        if components_added or components_removed or values:
            changed[name] = {"added": components_added, "removed": components_removed, "changed": values}
    components = {}
    for change in changed.values():
        for label in ("added", "removed", "changed"):
            for component in change[label]:
                components.setdefault(component, {"added": 0, "removed": 0, "changed": 0})[label] += 1
    return {"added": added, "removed": removed, "changed": changed, "components": components}


def command_nodes(tree):
    """Every node of a commands report by its path ("execute as targets"), with what defines it."""
    nodes = {}

    def walk(node, path):
        for name, child in node.get("children", {}).items():
            at = f"{path} {name}".strip()
            nodes[at] = {k: child[k] for k in ("type", "parser", "properties", "executable", "redirect", "permissions")
                         if k in child}
            walk(child, at)

    walk(tree, "")
    return nodes


def diff_commands(old, new):
    a, b = command_nodes(old), command_nodes(new)
    added, removed, common = keyed_diff(a, b)
    return {"added": added, "removed": removed,
            "changed": {path: json_diff(a[path], b[path], limit=10) for path in common
                        if not json_equal(a[path], b[path])}}


def jar_entries(jar, prefix):
    """{path: crc} of a jar's files under prefix."""
    with zipfile.ZipFile(jar) as archive:
        return {i.filename: i.CRC for i in archive.infolist() if i.filename.startswith(prefix) and not i.is_dir()}


def content_kind(path):
    """data/minecraft/worldgen/biome/x.json -> worldgen/biome; assets/minecraft/models/block/x.json -> models/block."""
    parts = path.split("/")[2:-1]
    if parts and parts[0] in {"worldgen", "tags"} and len(parts) > 1:
        return "/".join(parts[:2])
    return parts[0] if parts else "(root)"


def diff_contents(old_jar, new_jar, prefix):
    """Files under prefix added, removed and changed (by CRC), by kind; changed JSON files with their paths."""
    a, b = jar_entries(old_jar, prefix), jar_entries(new_jar, prefix)
    added, removed, common = keyed_diff(a, b)
    changed = [p for p in common if a[p] != b[p]]
    kinds = {}
    for label, names in [("added", added), ("removed", removed), ("changed", changed)]:
        for name in names:
            kinds.setdefault(content_kind(name), {"added": [], "removed": [], "changed": []})[label].append(name)
    details = {}
    with zipfile.ZipFile(old_jar) as left, zipfile.ZipFile(new_jar) as right:
        for name in changed:
            if name.endswith(".json") and not name.endswith("/lang/en_us.json"):
                try:
                    details[name] = json_diff(json.loads(left.read(name)), json.loads(right.read(name)))
                except ValueError:
                    pass
    return {"kinds": dict(sorted(kinds.items())), "json": details,
            "counts": {"added": len(added), "removed": len(removed), "changed": len(changed)}}


def diff_lang(old_jar, new_jar, name="assets/minecraft/lang/en_us.json"):
    with zipfile.ZipFile(old_jar) as left, zipfile.ZipFile(new_jar) as right:
        a, b = json.loads(left.read(name)), json.loads(right.read(name))
    added, removed, common = keyed_diff(a, b)
    return {"added": added, "removed": removed, "changed": [k for k in common if a[k] != b[k]]}


def read_reports(reports):
    items = {}
    for path in sorted((reports / "minecraft" / "components" / "item").glob("*.json")):
        items[f"minecraft:{path.stem}"] = json.loads(path.read_text())["components"]
    return {
        "packets": json.loads((reports / "packets.json").read_text()),
        "registries": json.loads((reports / "registries.json").read_text()),
        "blocks": json.loads((reports / "blocks.json").read_text()),
        "commands": json.loads((reports / "commands.json").read_text()),
        "items": items,
    }


def class_names(old, new, moved):
    """Every class name of either version, but a moved class only under its new name, so that its simple
    name stays unambiguous."""
    return sorted(set(new) | (set(old) - set(moved)))


def diff_classes(result, mapping):
    """The class diff, each changed or removed class with the Horizon files and harnesses that concern it."""
    horizon = {}
    for name in result["changed"]:
        entry = vanilla_map.lookup(mapping, name)
        if entry["rust"] or entry["harnesses"]:
            horizon[name] = entry
    for name in result["removed"]:
        entry = vanilla_map.lookup(mapping, name)
        if entry["rust"] or entry["harnesses"]:
            horizon[name] = entry
    result["horizon"] = horizon
    return result


def diff_versions(old_paths, new_paths, old_facts, new_facts, classes=None, mapping=None):
    a, b = read_reports(old_paths["reports"]), read_reports(new_paths["reports"])
    classes = diff_classes(classes, mapping) if classes is not None else None
    return {
        "facts": diff_facts(old_facts, new_facts),
        "packets": diff_packets(a["packets"], b["packets"]),
        "registries": diff_registries(a["registries"], b["registries"]),
        "blocks": diff_blocks(a["blocks"], b["blocks"]),
        "items": diff_items(a["items"], b["items"]),
        "commands": diff_commands(a["commands"], b["commands"]),
        "data": diff_contents(old_paths["client"], new_paths["client"], "data/minecraft/"),
        "assets": diff_contents(old_paths["client"], new_paths["client"], "assets/minecraft/"),
        "lang": diff_lang(old_paths["client"], new_paths["client"]),
        "classes": classes,
    }


def summary(old, new, result):
    """A few lines a person reads first."""
    lines = [f"Minecraft {old} -> {new}"]
    for key, (a, b) in result["facts"].items():
        lines.append(f"  {key}: {a} -> {b}")
    for flow, change in result["packets"].items():
        lines.append(f"  packets {flow}: {change['count'][0]} -> {change['count'][1]}, +{len(change['added'])} "
                     f"-{len(change['removed'])}, {len(change['renumbered'])} renumbered")
    registries = result["registries"]
    lines.append(f"  registries: +{len(registries['added'])} -{len(registries['removed'])}, "
                 f"{len(registries['changed'])} changed")
    for name, change in registries["changed"].items():
        lines.append(f"    {name}: +{len(change.get('added', []))} -{len(change.get('removed', []))}"
                     + (f", {change['ids_shifted']} ids shifted" if change.get("ids_shifted") else ""))
    blocks = result["blocks"]
    lines.append(f"  blocks: +{len(blocks['added'])} -{len(blocks['removed'])}, {len(blocks['changed'])} changed, "
                 f"states {blocks['state_count'][0]} -> {blocks['state_count'][1]}, "
                 f"{blocks['state_ids_shifted']} blocks' state ids shifted")
    if blocks["format"]:
        lines.append(f"    report format: fields {', '.join(blocks['format'])} appear or disappear")
    items = result["items"]
    lines.append(f"  items: +{len(items['added'])} -{len(items['removed'])}, {len(items['changed'])} with "
                 f"changed default components")
    ranked = sorted(items["components"].items(), key=lambda kv: -sum(kv[1].values()))
    for component, counts in ranked[:10]:
        lines.append(f"    {component}: " + ", ".join(f"{k} on {n}" for k, n in counts.items() if n))
    commands = result["commands"]
    lines.append(f"  command nodes: +{len(commands['added'])} -{len(commands['removed'])}, "
                 f"{len(commands['changed'])} changed")
    for key in ["data", "assets"]:
        counts = result[key]["counts"]
        lines.append(f"  {key} files: +{counts['added']} -{counts['removed']} ~{counts['changed']}")
    lang = result["lang"]
    lines.append(f"  en_us keys: +{len(lang['added'])} -{len(lang['removed'])} ~{len(lang['changed'])}")
    classes = result.get("classes")
    if classes:
        methods = sum(class_index.changed_methods(c) for c in classes["changed"].values())
        lines.append(f"  classes: +{len(classes['added'])} -{len(classes['removed'])}, {len(classes['moved'])} moved, "
                     f"{len(classes['changed'])} changed ({methods} methods); {len(classes['horizon'])} of the "
                     f"changed or removed ones are ported or proved by Horizon")
    return "\n".join(lines)


def diff_dir(old, new):
    return STORE / f"diff-{safe_version(old)}-{safe_version(new)}"


def load(version, name):
    return json.loads((version_dir(version) / name).read_text())


def diff(args):
    directory = diff_dir(args.old, args.new)
    old_classes, new_classes = load(args.old, "classes.json"), load(args.new, "classes.json")
    classes = class_index.diff(old_classes, new_classes)
    mapping = vanilla_map.build(class_names(old_classes, new_classes, classes["moved"]),
                                dependencies=vanilla_map.recorded_dependencies(vanilla_map.default_shared()))
    lab.write_json(directory / "vanilla_map.json", mapping)
    result = diff_versions(paths(args.old), paths(args.new), load(args.old, "facts.json"), load(args.new, "facts.json"),
                           classes, mapping)
    lab.write_json(directory / "diff.json", result)
    print(summary(args.old, args.new, result))
    print(f"\n{directory / 'diff.json'}")
    return 0


# --- regen -------------------------------------------------------------------------------------------------

def regen(args):
    version = safe_version(args.new)
    result = port_regen.regen(version, paths(version), version_dir(version), args.base,
                              Path(args.worktree) if args.worktree else PORTS / version, args.only)
    lab.write_json(diff_dir(result["old"], version) / "regen.json", result)
    for name, entry in result["generators"].items():
        stat = ", ".join(f"{p} +{a} -{r}" for p, (a, r) in entry.get("stat", {}).items())
        print(f"{name}: {entry['status']}" + (f" ({stat})" if stat else ""))
    print(f"\n{result['candidate']} at {result['commit'][:10]}; {diff_dir(result['old'], version) / 'regen.json'}")
    return 1 if any(e["status"] == "failed" for e in result["generators"].values()) else 0


# --- harnesses ---------------------------------------------------------------------------------------------

def harnesses(args):
    version, old = safe_version(args.new), args.old or pinned()
    directory = diff_dir(old, version)
    previous = json.loads((directory / "harnesses.json").read_text()) if args.no_run else None
    result = port_harness.harnesses(version, version_dir(version),
                                    Path(args.worktree) if args.worktree else PORTS / version,
                                    args.baseline, args.names or None, args.timeout, previous)
    lab.write_json(directory / "harnesses.json", result)
    port_harness.keep_logs(result, directory / "harness-logs")
    counts = result["summary"]
    print(f"horizon-prepare: {result['prepare']['status']}")
    print(f"harnesses: {counts['pass']} pass, {counts['oracle']} oracle no longer compiles, {counts['fail']} fail; "
          f"{counts['new']} newly failing, {counts['known']} failing on the baseline too")
    print(f"{directory / 'harnesses.json'}")
    return 0


# --- report ------------------------------------------------------------------------------------------------

def harness_results(state_path):
    """{harness: "pass" | "fail"} from tools/parity.py's state; a harness fails if one of its shards does."""
    found = {}
    for name, record in json.loads(Path(state_path).read_text()).get("harnesses", {}).items():
        harness = name.split("#")[0]
        found[harness] = "fail" if record.get("status") != "pass" or found.get(harness) == "fail" else "pass"
    return found


def report(args):
    directory = diff_dir(args.old, args.new)
    if not (directory / "diff.json").exists():
        raise SystemExit(f"No diff yet; run version_port.py diff {args.old} {args.new}")
    result = json.loads((directory / "diff.json").read_text())
    mapping = json.loads((directory / "vanilla_map.json").read_text())
    results = harness_results(args.results) if args.results else None
    regenerated, ran = directory / "regen.json", directory / "harnesses.json"
    regenerated = json.loads(regenerated.read_text()) if regenerated.exists() else None
    ran = json.loads(ran.read_text()) if ran.exists() and not args.results else None
    built = port_report.build(result, mapping, args.old, args.new, load(pinned(), "facts.json"), results,
                              regen=regenerated, harnesses=ran)
    lab.write_json(directory / "report.json", built)
    (directory / "report.md").write_text(port_report.markdown(built))
    print("\n".join(port_report.markdown(built).splitlines()[:args.lines]))
    print(f"\n{directory / 'report.md'} ({len(built['items'])} items)")
    return 0


# --- publish -----------------------------------------------------------------------------------------------

def publish(args):
    directory = diff_dir(args.old, args.new)
    if not (directory / "report.json").exists():
        raise SystemExit(f"No report yet; run version_port.py report {args.old} {args.new}")
    built = json.loads((directory / "report.json").read_text())
    result = json.loads((directory / "diff.json").read_text())
    issues = port_publish.open_version_port_issues() if args.yes or args.check_github else {}
    actions = port_publish.plan(built, result, directory, issues)
    creates = sum(1 for a in actions if a[0] == "create")
    if not args.yes:
        for action, name, text, number in actions:
            print(f"would {action}{f' #{number}' if number else ''}: {name}")
        for action, name, text, number in actions[:args.show]:
            print(f"\n--- {name}\n{text}")
        missing = port_publish.missing_labels(args.new) if args.check_github else [port_publish.LABEL,
                                                                                    f"port-{args.new}"]
        print(f"\nDry run: {creates} issues to create, {len(actions) - creates} to update"
              f"{'' if args.check_github else ' (open issues not checked: --check-github)'}; labels to create: "
              f"{', '.join(missing) or 'none'}; then one Kira card for the tracking issue. Rerun with --yes to do it.")
        return 0
    urls = port_publish.apply(actions, built, issues)
    tracking = urls[actions[-1][1]]
    card = port_publish.kira_card(args.new, tracking)
    print(f"Tracking issue {tracking}" + (f"; Kira card {card}" if card else "; Kira card already there"))
    return 0


def port(args):
    """prepare both, diff, regen, report."""
    old = args.old or pinned()
    for version in (old, args.new):
        prepare(argparse.Namespace(version=version, download=False, jobs=args.jobs))
    diff(argparse.Namespace(old=old, new=args.new))
    if not args.no_regen:
        regen(argparse.Namespace(new=args.new, base=args.base, worktree=None, only=None))
    return report(argparse.Namespace(old=old, new=args.new, results=args.results, lines=args.lines))


# --- gc ----------------------------------------------------------------------------------------------------

def to_remove(entries, pin, keep):
    """Version store entries to delete: every version but the pinned one, --keep and the newest two others;
    diffs whose versions are gone; scratch folders. entries: [(name, mtime, is_version)]."""
    versions = sorted(((m, n) for n, m, is_version in entries if is_version and n not in keep | {pin}), reverse=True)
    kept = keep | {pin} | {n for _, n in versions[:2]}
    remove = []
    for name, _, is_version in entries:
        if name.startswith("."):
            remove.append(name)
        elif is_version and name not in kept:
            remove.append(name)
        elif name.startswith("diff-") and not any(name == f"diff-{a}-{b}" for a in kept for b in kept):
            remove.append(name)
    return sorted(remove)


def gc(args):
    if not STORE.exists():
        return 0
    entries = [(p.name, p.stat().st_mtime, (p / "prepared.json").exists()) for p in STORE.iterdir() if p.is_dir()]
    for name in to_remove(entries, pinned(), set(args.keep)):
        print(f"Removing {STORE / name}")
        if not args.dry_run:
            shutil.rmtree(STORE / name)
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    command = commands.add_parser("detect", help="what Mojang released that Horizon doesn't pin")
    command.add_argument("--snapshots", action="store_true")
    command.add_argument("--dry-run", action="store_true", help="don't remember what was found")
    command = commands.add_parser("prepare", help="download, verify and run the data generator")
    command.add_argument("version")
    command.add_argument("--download", action="store_true", help="download even the pinned version")
    command.add_argument("-j", "--jobs", type=int, default=int(os.environ.get("FLEET_LOAD_JOBS", 0)) or None,
                         help="class indexing processes (default: $FLEET_LOAD_JOBS, else the CPU count)")
    command = commands.add_parser("diff", help="diff two prepared versions")
    command.add_argument("old")
    command.add_argument("new")
    command = commands.add_parser("regen", help="a candidate worktree pinned to NEW, every table regenerated")
    command.add_argument("new")
    command.add_argument("--base", default="origin/main", help="what port/NEW starts from (default origin/main)")
    command.add_argument("--worktree", help="where (default: $HORIZON_PORTS/NEW)")
    command.add_argument("--only", nargs="*", help="these generators only (names in tools/port_regen.py)")
    command = commands.add_parser("harnesses", help="horizon-prepare and every harness in the candidate")
    command.add_argument("new")
    command.add_argument("names", nargs="*", help="these harnesses only (default: all)")
    command.add_argument("--old", help="the version the port starts from (default: the pinned one)")
    command.add_argument("--worktree", help="the candidate (default: $HORIZON_PORTS/NEW)")
    command.add_argument("--baseline", help="tools/parity.py's state.json from a run on the pinned version: what "
                                            "fails there too is not newly failing")
    command.add_argument("--timeout", type=int, help="per harness, seconds (tools/parity.py --timeout)")
    command.add_argument("--no-run", action="store_true", help="classify the last run again (another --baseline)")
    command = commands.add_parser("report", help="the ranked port report of a diff")
    command.add_argument("old")
    command.add_argument("new")
    command.add_argument("--results", help="tools/parity.py's state.json from a run against NEW (default: "
                                           "harnesses.json, when version_port.py harnesses ran)")
    command.add_argument("--lines", type=int, default=60, help="how much of report.md to print")
    command = commands.add_parser("publish", help="the report's work items as GitHub issues (dry run by default)")
    command.add_argument("old")
    command.add_argument("new")
    command.add_argument("--yes", action="store_true", help="create and update the issues, labels and Kira card")
    command.add_argument("--check-github", action="store_true",
                         help="dry run: read the open issues and labels to tell creates from updates")
    command.add_argument("--show", type=int, default=1, help="dry run: print this many issue bodies")
    command = commands.add_parser("port", help="prepare, diff and report in one go")
    command.add_argument("new")
    command.add_argument("--old", help="default: the pinned version")
    command.add_argument("--results", help="tools/parity.py's state.json from a run against NEW")
    command.add_argument("--lines", type=int, default=60)
    command.add_argument("--base", default="origin/main", help="what the candidate starts from")
    command.add_argument("--no-regen", action="store_true", help="diff and report only (snapshots)")
    command.add_argument("-j", "--jobs", type=int, default=int(os.environ.get("FLEET_LOAD_JOBS", 0)) or None)
    command = commands.add_parser("gc", help="delete versions and scratch the store doesn't need")
    command.add_argument("--keep", nargs="*", default=[])
    command.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    STORE.mkdir(parents=True, exist_ok=True)
    return {"detect": detect, "prepare": prepare, "diff": diff, "regen": regen, "harnesses": harnesses,
            "report": report, "publish": publish, "port": port, "gc": gc}[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
