#!/usr/bin/env python3
"""The port report: a version diff (tools/version_port.py diff) turned into ranked, area-sized work items
(docs/version-ports.md, "The port report").

Ranks:
  1  blocks the switch: version facts, packets, the save format (DataFixers), data component types, report
     formats, generated tables to regenerate
  2  a harness newly fails against the new version, or its oracle no longer compiles (version_port.py
     harnesses, or --results), else changed vanilla code a harness proves: rerun it
  3  changed vanilla code Horizon ports and no harness proves
  4  new content: registry entries, and new classes in the vanilla packages Horizon ports
  5  changed and new vanilla code Horizon doesn't port (two summary items)

Used through `version_port.py report OLD NEW`; the functions here take plain data so tests can build it.
"""

from collections import Counter, defaultdict
from pathlib import Path
import re
import subprocess
import sys

try:
    from . import class_index, vanilla_map
except ImportError:
    import class_index
    import vanilla_map

ROOT = Path(__file__).resolve().parents[1]
PROTOCOL = "net/minecraft/network/protocol/"
DATAFIX = "net/minecraft/util/datafix/"
# Generated tables, what makes them stale, and how to regenerate them (docs/version-ports.md, "Regenerate").
GENERATORS = [
    ("crates/horizon-core/src/version.rs", "python3 tools/gen_version_consts.py", lambda d: bool(d["facts"])),
    ("crates/horizon-protocol/src/ids.rs", "python3 tools/generate_packet_ids.py",
     lambda d: bool(d["packets"])),
    ("crates/horizon-client/src/lan/tracking.rs", "python3 tools/generate_entity_tracking.py",
     lambda d: "minecraft:entity_type" in d["registries"]["changed"]
     or "net/minecraft/world/entity/EntityType" in d["classes"]["changed"]),
    ("crates/horizon-client/src/lan/command_suggestions.rs", "python3 tools/generate_command_suggestions.py",
     lambda d: bool(d["commands"]["added"] or d["commands"]["removed"] or d["commands"]["changed"])),
    ("crates/horizon-client/src/lan/command_order.rs", "python3 tools/generate_lan_command_order.py",
     lambda d: bool(d["commands"]["added"] or d["commands"]["removed"])),
    ("crates/horizon-command/src/vanilla_order.rs", "python3 tools/generate_command_order.py",
     lambda d: bool(d["commands"]["added"] or d["commands"]["removed"])),
]
# Registries whose new entries are new things to port.
CONTENT = ["minecraft:block", "minecraft:item", "minecraft:entity_type", "minecraft:block_entity_type",
           "minecraft:particle_type", "minecraft:menu", "minecraft:worldgen/feature", "minecraft:sound_event"]


def version_sites(facts, root=ROOT):
    """file:line of every place the code states these version facts (not tests)."""
    pattern = ["-e", str(facts["protocol_version"]), "-e", str(facts["world_version"])]
    found = []
    for command in (["git", "grep", "-n", "-w"] + pattern,
                    ["git", "grep", "-n", "-F", f'"{facts["id"]}"'],
                    ["git", "grep", "-n", "-F", f"'{facts['id']}'"]):
        result = subprocess.run(command + ["--", "crates/*.rs", "launcher/*.rs", "tools/*.py"],
                                cwd=root, capture_output=True, text=True)
        found += [line.split(":", 2)[0] + ":" + line.split(":", 2)[1] for line in result.stdout.splitlines()]
    return sorted({site for site in found if not re.search(r"(^|/)tests?(/|\.rs)|/test_|_tests?\.rs", site)})


def files_containing(text, root=ROOT):
    """Rust and Python files that contain this text (a report's file name)."""
    command = ["git", "grep", "-l", "-F", text, "--", "crates/*.rs", "launcher/*.rs", "tools/*.py"]
    return subprocess.run(command, cwd=root, capture_output=True, text=True).stdout.split()


def files_naming(words, root=ROOT):
    """Rust files that contain any of these words (component or packet names)."""
    if not words:
        return []
    command = ["git", "grep", "-l", "-w"] + [arg for word in words for arg in ("-e", word)] + ["--", "crates/*.rs"]
    return subprocess.run(command, cwd=root, capture_output=True, text=True).stdout.split()


def snake(name):
    """LivingEntity -> living_entity."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def primary_file(entry, members, cls):
    """The Horizon file that ports a changed class: the one whose comments name the most of its changed
    methods, else the one named after it, else one of the few files that name it. None for a class many
    files name in passing (Entity, BlockPos) when none names a changed method."""
    files = entry["rust"]
    scored = sorted((-len(set(named) & members), path) for path, named in files.items() if set(named) & members)
    if scored:
        return scored[0][1]
    stem = snake(cls.rsplit("/", 1)[-1].split("$")[0])
    named_after = sorted(path for path in files if Path(path).stem == stem)
    if named_after:
        return named_after[0]
    return sorted(files)[0] if 0 < len(files) <= 3 else None


def method_names(change):
    methods = change.get("methods", {})
    return {m.split("(")[0] for kind in ("added", "removed", "changed") for m in methods.get(kind, [])}


def item(rank, kind, title, **details):
    return {"rank": rank, "kind": kind, "title": title, **details}


def blocking_items(diff, mapping, pinned_facts, root, regen=None):
    items = []
    if diff["facts"]:
        changes = ", ".join(f"{k} {a} -> {b}" for k, (a, b) in diff["facts"].items())
        items.append(item(1, "facts", f"Version facts: {changes}", files=version_sites(pinned_facts, root),
                          details=diff["facts"]))
    if diff["packets"]:
        classes = diff["classes"]
        packets = {"added": [c for c in classes["added"] if c.startswith(PROTOCOL)],
                   "removed": [c for c in classes["removed"] if c.startswith(PROTOCOL)],
                   "changed": [c for c in classes["changed"] if c.startswith(PROTOCOL)]}
        names = sorted({n.split(":")[1] for flow in diff["packets"].values()
                        for n in list(flow["added"]) + list(flow["removed"])})
        constants = [n.upper().replace("/", "_") for n in names]
        counts = ", ".join(f"{flow} +{len(c['added'])} -{len(c['removed'])} ~{len(c['renumbered'])}"
                           for flow, c in diff["packets"].items())
        items.append(item(1, "protocol", f"Protocol: packets {counts}",
                          files=sorted(set(files_naming(constants, root)) | {"crates/horizon-protocol/src/ids.rs"}),
                          details={"packets": diff["packets"], "classes": packets}))
    if "world_version" in diff["facts"]:
        classes = diff["classes"]
        added = [c for c in classes["added"] if c.startswith(DATAFIX)]
        changed = [c for c in classes["changed"] if c.startswith(DATAFIX)]
        schemas = sorted(c for c in added if "/schemas/V" in c and "$" not in c)
        fixes = sorted(c for c in added if "/fixes/" in c and "$" not in c)
        horizon = vanilla_map.lookup(mapping, "net/minecraft/util/datafix/DataFixers")
        named = {f for c in schemas + fixes for f in mapping["classes"].get(c, {}).get("rust", {})}
        a, b = diff["facts"]["world_version"]
        items.append(item(1, "save-format", f"Save format: DataFixers {a} -> {b}: {len(schemas)} schemas, "
                          f"{len(fixes)} fixes added, {len(changed)} datafix classes changed",
                          files=sorted(set(horizon["rust"]) | named), harnesses=horizon["harnesses"],
                          details={"schemas": schemas, "fixes": fixes, "changed": changed}))
    registry = diff["registries"]["changed"].get("minecraft:data_component_type", {})
    if registry.get("added") or registry.get("removed"):
        names = [n.split(":")[1] for n in registry.get("added", []) + registry.get("removed", [])]
        items.append(item(1, "components", f"Data component types: +{len(registry.get('added', []))} "
                          f"-{len(registry.get('removed', []))} (sent on the wire and saved)",
                          files=files_naming(names, root),
                          details={"types": registry, "items": diff["items"]["components"]}))
    if diff["blocks"].get("format"):
        items.append(item(1, "report-format", f"blocks.json format: {', '.join(diff['blocks']['format'])}",
                          files=files_containing("blocks.json", root),
                          details={"fields": diff["blocks"]["format"]}))
    if regen is None:
        for output, command, stale in GENERATORS:
            if stale(diff):
                items.append(item(1, "regenerate", f"Regenerate {output}", files=[output], command=command))
    else:
        for name, entry in regen["generators"].items():
            if entry["status"] == "failed":
                errors = entry.get("compile_errors", [])
                what = "its vanilla exporter no longer compiles" if errors else "failed"
                items.append(item(1, "regen-failed", f"Generator {name}: {what} on {regen['new']}",
                                  files=entry["outputs"], error=entry["error"], compile_errors=errors))
    return items


def item_harnesses(path, classes, mapping, limit=8):
    """The harnesses for an item: those whose probes reach its file, else those covering the most of its
    classes, the most specific first."""
    harnesses = mapping["harnesses"]
    reaching = sorted(h for h, data in harnesses.items() if path in data["rust"])
    if reaching:
        return reaching
    covering = Counter(h for c in classes for h in vanilla_map.lookup(mapping, c)["harnesses"])
    return sorted(sorted(covering, key=lambda h: (-covering[h], len(harnesses[h]["classes"]), h))[:limit])


def code_items(diff, mapping, results=None):
    """Ranks 2, 3 and 5 from the changed and removed vanilla classes, one item per Horizon file that ports
    them (classes many files name in passing go by vanilla package)."""
    classes = diff["classes"]
    groups = defaultdict(lambda: {"classes": {}, "files": set(), "precise": 0, "methods": 0})
    unported = Counter()
    for name in list(classes["changed"]) + classes["removed"]:
        if name.startswith((PROTOCOL, DATAFIX)):
            continue  # The protocol and save-format items have them.
        entry = classes["horizon"].get(name)
        if not entry:
            unported[name.rsplit("/", 1)[0]] += 1
            continue
        removed = name not in classes["changed"]
        change = classes["changed"].get(name, {})
        members = method_names(change)
        path = primary_file(entry, members, name) if entry["rust"] else None
        key = path or "widely named: " + name.rsplit("/", 1)[0].removeprefix("net/minecraft/")
        group = groups[key]
        precise = sorted({m for named in entry["rust"].values() for m in named} & members)
        methods = 1 if removed else class_index.changed_methods(change)
        group["classes"][name] = {"methods": methods, "named": precise, "removed": removed}
        group["files"].update([path] if path else entry["rust"])
        group["precise"] += len(precise)
        group["methods"] += methods
    failing = {h for h, status in (results or {}).items() if status != "pass"}
    items = []
    for key, group in groups.items():
        harnesses = item_harnesses(key, group["classes"], mapping)
        failed = sorted(set(harnesses) & failing)
        rank, kind = (2, "harness-failed") if failed else (2, "rerun") if harnesses else (3, "unproved")
        oracle = sorted(h for h in failed if results[h] == "oracle")
        top = sorted(group["classes"], key=lambda c: (-group["classes"][c]["methods"], c))
        title = (f"{key}: {len(group['classes'])} vanilla classes changed "
                 f"({group['methods']} methods, {group['precise']} named in Horizon)")
        items.append(item(rank, kind, title, module=key, files=sorted(group["files"]), harnesses=harnesses,
                          failed=failed, oracle=oracle, classes={c: group["classes"][c] for c in top},
                          score=group["methods"] + 10 * group["precise"] + (1000 if failed else 0)))
    if unported:
        items.append(item(5, "unported", f"{sum(unported.values())} changed or removed vanilla classes Horizon "
                          f"doesn't port, in {len(unported)} packages", packages=dict(unported.most_common())))
    return items


def content_items(diff, mapping):
    """Rank 4: new registry entries, and new classes by vanilla package where Horizon ports other classes of
    that package (with those Horizon already names, after a port); new classes elsewhere go in one summary."""
    items = []
    for registry in CONTENT:
        entries = diff["registries"]["changed"].get(registry, {}).get("added", [])
        if entries:
            items.append(item(4, "content", f"New {registry.split(':')[1]}: {len(entries)} entries", entries=entries))
    ported_packages = Counter(c.rsplit("/", 1)[0] for c, e in mapping["classes"].items() if e["rust"])
    by_package = defaultdict(list)
    for name in diff["classes"]["added"]:
        if not re.search(r"\$\d", name) and not name.startswith((PROTOCOL, DATAFIX)):
            by_package[name.rsplit("/", 1)[0]].append(name)
    elsewhere = Counter()
    for package, names in sorted(by_package.items()):
        if package not in ported_packages:
            elsewhere[package] += len(names)
            continue
        ported = sorted(c for c in names if mapping["classes"].get(c, {}).get("rust"))
        nearby = Counter(f for c, e in mapping["classes"].items() if c.rsplit("/", 1)[0] == package for f in e["rust"])
        files = sorted({f for c in ported for f in mapping["classes"][c]["rust"]}) or [f for f, _ in nearby.most_common(5)]
        items.append(item(4, "new-classes", f"New in {package.removeprefix('net/minecraft/')}: {len(names)} classes"
                          f" ({len(ported)} already named in Horizon)", classes=sorted(names), ported=ported,
                          files=files, score=len(names)))
    if elsewhere:
        items.append(item(5, "new-unported", f"{sum(elsewhere.values())} new vanilla classes in {len(elsewhere)} "
                          f"packages Horizon doesn't port", packages=dict(elsewhere.most_common())))
    return items


def harness_items(ran, items, new):
    """Rank 1 when horizon-prepare fails on NEW (its exporters no longer compile, or a format broke); rank 2 for
    each newly failing harness that no code item names, so none is lost."""
    found = []
    prepared = ran["prepare"]
    if prepared["status"] != "ok":
        what = "its exporters no longer compile" if prepared.get("compile_errors") else "it fails"
        found.append(item(1, "prepare-failed", f"horizon-prepare on {new}: {what}", files=["crates/horizon-prepare"],
                          error=prepared["error"], compile_errors=prepared.get("compile_errors", [])))
    named = {h for i in items for h in i.get("failed", [])}
    for harness, entry in ran["harnesses"].items():
        if entry["new"] and harness not in named:
            what = "its oracle no longer compiles" if entry["status"] == "oracle" else "fails"
            found.append(item(2, "harness-failed", f"Harness {harness}: {what} on {new}", harnesses=[harness],
                              failed=[harness], oracle=[harness] if entry["status"] == "oracle" else [],
                              compile_errors=entry.get("compile_errors", []), score=1000))
    return found


def build(diff, mapping, old, new, pinned_facts, results=None, root=ROOT, regen=None, harnesses=None):
    """The report: summary, ranked items and what came out clean. pinned_facts: the facts of the version the
    code states today (to find where it states them); results: {harness: "pass" | "fail"} against NEW; regen:
    version_port.py regen's result (without it, the tables a diff makes stale are listed to regenerate);
    harnesses: version_port.py harnesses' result (only newly failing harnesses count)."""
    if harnesses:
        results = {h: e["status"] if e["new"] else "pass" for h, e in harnesses["harnesses"].items()}
    items = blocking_items(diff, mapping, pinned_facts, root, regen)
    items += code_items(diff, mapping, results) + content_items(diff, mapping)
    if harnesses:
        items += harness_items(harnesses, items, new)
    items.sort(key=lambda i: (i["rank"], -i.get("score", 0), i["title"]))
    for number, entry in enumerate(items, 1):
        entry["id"] = number
    clean = [kind for kind in ("packets", "commands") if not diff[kind]]
    clean += [kind for kind in ("blocks", "items") if not (diff[kind]["added"] or diff[kind]["removed"]
                                                            or diff[kind]["changed"])]
    classes = diff["classes"]
    return {
        "old": old, "new": new,
        "summary": {
            "facts": diff["facts"], "ranks": dict(sorted(Counter(i["rank"] for i in items).items())),
            "classes": {"added": len(classes["added"]), "removed": len(classes["removed"]),
                        "moved": len(classes["moved"]), "changed": len(classes["changed"]),
                        "horizon": len(classes["horizon"])},
            "generators": ({name: e["status"] for name, e in regen["generators"].items()} if regen
                           else {i["files"][0]: "stale" for i in items if i["kind"] == "regenerate"}),
        },
        "harnesses": harnesses["summary"] if harnesses else None,
        "regen": {"candidate": regen["candidate"], "commit": regen["commit"],
                  "stat": {p: s for e in regen["generators"].values() for p, s in e.get("stat", {}).items()}}
        if regen else None,
        "items": items, "clean": clean,
    }


def short(name):
    return name.rsplit("/", 1)[-1].replace("$", ".")


def markdown(report, limit=12):
    """report.md: what a person or an agent reads first."""
    lines = [f"# Port report: Minecraft {report['old']} -> {report['new']}", ""]
    summary = report["summary"]
    for key, (a, b) in summary["facts"].items():
        lines.append(f"- {key}: {a} -> {b}")
    c = summary["classes"]
    lines.append(f"- classes: +{c['added']} -{c['removed']}, {c['moved']} moved, {c['changed']} changed; "
                 f"{c['horizon']} changed or removed classes concern Horizon code or harnesses")
    lines.append(f"- work items by rank: " + ", ".join(f"{r}: {n}" for r, n in summary["ranks"].items()))
    if report["clean"]:
        lines.append(f"- unchanged: {', '.join(report['clean'])}")
    if report.get("regen"):
        generators = summary["generators"]
        lines.append(f"- generators in `{report['regen']['candidate']}` at {report['regen']['commit'][:10]}: "
                     + ", ".join(f"{s} {sum(1 for v in generators.values() if v == s)}"
                                 for s in ("regenerated", "unchanged", "failed")))
        for path, (added, removed) in report["regen"]["stat"].items():
            lines.append(f"  - regenerated `{path}` (+{added} -{removed})")
    if report.get("harnesses"):
        h = report["harnesses"]
        lines.append(f"- harnesses on {report['new']}: {h['pass']} pass, {h['oracle']} whose oracle no longer compiles, "
                     f"{h['fail']} fail; {h['new']} newly failing, {h['known']} failing on {report['old']} too")
    names = {1: "Blocks the switch", 2: "Proved by a harness: rerun it", 3: "Ported, no harness",
             4: "New content", 5: "Not ported"}
    for rank in sorted({i["rank"] for i in report["items"]}):
        lines += ["", f"## {rank}. {names[rank]}", ""]
        for entry in [i for i in report["items"] if i["rank"] == rank]:
            lines.append(f"### #{entry['id']} {entry['title']}")
            if entry.get("command"):
                lines.append(f"- run: `{entry['command']}`")
            if entry.get("compile_errors"):
                lines += [f"- javac: `{e}`" for e in entry["compile_errors"][:limit]]
            elif entry.get("error"):
                lines.append("- error: `" + entry["error"].strip().splitlines()[-1][:300] + "`")
            if entry.get("failed"):
                lines.append(f"- failing harnesses: {', '.join(entry['failed'])}"
                             + (f" (oracle no longer compiles: {', '.join(entry['oracle'])})" if entry.get("oracle") else ""))
            if entry.get("harnesses"):
                lines.append(f"- harnesses: {', '.join(entry['harnesses'][:limit])}"
                             + (f" (+{len(entry['harnesses']) - limit})" if len(entry["harnesses"]) > limit else ""))
            if entry.get("files"):
                lines.append(f"- Horizon: {', '.join(f'`{f}`' for f in entry['files'][:limit])}"
                             + (f" (+{len(entry['files']) - limit})" if len(entry["files"]) > limit else ""))
            if entry.get("classes") and isinstance(entry["classes"], dict):
                shown = list(entry["classes"].items())[:limit]
                lines.append("- vanilla: " + ", ".join(
                    f"{short(n)} ({'removed' if v['removed'] else str(v['methods']) + ' methods'}"
                    + (f"; {', '.join(v['named'])}" if v["named"] else "") + ")" for n, v in shown)
                    + (f" (+{len(entry['classes']) - limit})" if len(entry["classes"]) > limit else ""))
            elif entry.get("classes"):
                lines.append(f"- new classes: {', '.join(short(n) for n in entry['classes'][:limit])}"
                             + (f" (+{len(entry['classes']) - limit})" if len(entry["classes"]) > limit else ""))
            if entry.get("entries"):
                lines.append(f"- entries: {', '.join(n.split(':')[-1] for n in entry['entries'][:limit])}"
                             + (f" (+{len(entry['entries']) - limit})" if len(entry["entries"]) > limit else ""))
            if entry.get("ported"):
                lines.append(f"- already in Horizon: {', '.join(short(n) for n in entry['ported'][:limit])}")
            details = entry.get("details", {})
            for key in ("schemas", "fixes"):
                if details.get(key):
                    lines.append(f"- {key}: {', '.join(short(n) for n in details[key])}")
            if entry["kind"] == "protocol":
                for flow, change in details["packets"].items():
                    lines.append(f"- {flow}: added {', '.join(change['added']) or 'none'}; "
                                 f"removed {', '.join(change['removed']) or 'none'}")
            if entry["kind"] in ("unported", "new-unported"):
                lines.append("- " + ", ".join(f"{p.removeprefix('net/minecraft/')} {n}"
                                              for p, n in list(entry["packages"].items())[:limit]))
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


if __name__ == "__main__":
    # A module, not a command: say how it is used instead of doing nothing at all.
    print(__doc__, file=sys.stderr)
    raise SystemExit(2)
