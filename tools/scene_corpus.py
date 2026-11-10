#!/usr/bin/env python3
"""Scene corpus for scene_replay: `generate` captures the recipes in benchmarks/scene-corpus/corpus.json, `list` shows them."""
import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

try:
    from . import graphics_bench, lab
except ImportError:
    import graphics_bench
    import lab

ROOT = Path(__file__).resolve().parent.parent
RECIPES = ROOT / "benchmarks/scene-corpus/corpus.json"
MAJOR = 1
WORLD = "SceneCorpus"
# Step name -> (min args, max args); settle and wait-far are the tool's own, the rest are horizon-ctl commands.
STEPS = {"tp": (3, 3), "look": (2, 2), "time": (1, 1), "wait-ready": (0, 1), "fly": (7, 8), "settle": (1, 1), "wait-far": (0, 1)}
# Acceptance key -> (measured value, which side must hold).
EXPECT = {
    "min_sections": ("sections", "min"), "min_tiles": ("tiles", "min"),
    "min_translucent_quads": ("translucent_quads", "min"),
    "min_pages": ("pages", "min"), "max_pages": ("pages", "max"),
    "min_id_scramble": ("id_scramble", "min"), "max_id_scramble": ("id_scramble", "max"),
    "min_culled_share": ("culled_share", "min"),
}
FIELDS = {"name", "render_distance", "lod_distance", "size", "steps", "views", "variants", "expect"}


def corpus_root():
    return Path(os.environ.get("HORIZON_SCENE_CORPUS") or ROOT / ".local/scene-corpus") / f"v{MAJOR}"


def recipe_hash(recipe):
    return hashlib.sha256(json.dumps(recipe, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def validate(recipe):
    """The reasons a recipe is unusable (empty when it is fine)."""
    name = recipe.get("name", "?")
    errors = [f"{name}: missing {key}" for key in sorted(FIELDS - recipe.keys())]
    errors += [f"{name}: unknown field {key}" for key in sorted(recipe.keys() - FIELDS)]
    for step in recipe.get("steps", []):
        bounds = STEPS.get(step[0]) if step else None
        if bounds is None:
            errors.append(f"{name}: unknown step {step[:1]}")
        elif not bounds[0] <= len(step) - 1 <= bounds[1] or not all(isinstance(a, (int, float)) for a in step[1:]):
            errors.append(f"{name}: step {step[0]} takes {bounds[0]}..{bounds[1]} numbers")
    if "expect" in recipe and not recipe["expect"]:
        errors.append(f"{name}: expect is empty")
    errors += [f"{name}: unknown expect {key}" for key in recipe.get("expect", {}) if key not in EXPECT]
    return errors


def load_recipes(path=RECIPES):
    recipes = json.loads(Path(path).read_text(encoding="utf-8"))
    errors = [e for recipe in recipes for e in validate(recipe)]
    names = [r.get("name") for r in recipes]
    errors += [f"duplicate scene {n}" for n in sorted({n for n in names if names.count(n) > 1})]
    if errors:
        raise ValueError("; ".join(errors))
    return recipes


def check_expect(expect, measured):
    """The acceptance rules a capture breaks, each with its measured value (empty when it passes)."""
    failures = []
    for key, bound in expect.items():
        field, side = EXPECT[key]
        value = measured.get(field)
        if value is None:
            failures.append(f"{key}: {field} not measured")
        elif (value < bound) if side == "min" else (value > bound):
            failures.append(f"{key}: {field} is {value:g}, wanted {'at least' if side == 'min' else 'at most'} {bound:g}")
    return failures


def measure(directory):
    """What acceptance reads from a capture's files; pages, id_scramble and culled_share are None when absent."""
    directory = Path(directory)
    scene = json.loads((directory / "scene.json").read_text(encoding="utf-8"))
    sections = json.loads((directory / "sections.json").read_text(encoding="utf-8"))
    layout = json.loads((directory / "layout.json").read_text(encoding="utf-8")) if (directory / "layout.json").exists() else None
    layout = layout or {}
    return {
        "sections": scene["sections"], "tiles": scene["tiles"],
        "translucent_quads": sum(m["translucent"] for m in sections) // 4,
        "pages": layout.get("arena_pages"), "id_scramble": layout.get("id_scramble"), "culled_share": None,
    }


def culled_share(report):
    """The share of solid indices the first view's culling hid, from a scene_replay report."""
    culled = report["views"][0].get("culled")
    return culled[1] if culled else None


def corpus_id(directory):
    directory = Path(directory)
    digest = hashlib.sha256()
    for path in sorted(p for p in directory.rglob("*") if p.is_file() and p.name != "corpus-id"):
        file = hashlib.sha256()
        with open(path, "rb") as handle:
            for chunk in iter(lambda: handle.read(1 << 20), b""):
                file.update(chunk)
        digest.update(f"{path.relative_to(directory).as_posix()}\0{file.hexdigest()}\n".encode())
    return digest.hexdigest()


def graphics_text(recipe):
    return f"gpuCulling:true\nlodDistance:{recipe['lod_distance']}\n"


def wait_far(game, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        lod = game.run("status").get("lod") or {}
        if lod.get("target_tiles") and lod.get("sampled_target_tiles") == lod["target_tiles"]:
            return
        time.sleep(1)
    raise RuntimeError(f"wait-far: far terrain not ready after {timeout:g}s")


def run_steps(game, recipe):
    for name, *args in recipe["steps"]:
        if name == "settle":
            time.sleep(args[0])
        elif name == "wait-far":
            wait_far(game, args[0] if args else 600)
        else:
            game.run(name, *args)


def replay_culled_share(replay_bin, directory, resources, env):
    report = Path(directory).with_name(Path(directory).name + "-replay.json")
    cmd = [str(replay_bin), str(directory), "--root", str(resources), "--frames", "10", "--warmup", "20", "--json", str(report)]
    run = subprocess.run(cmd, env=env, capture_output=True, text=True)
    if run.returncode:
        raise RuntimeError(f"scene_replay failed: {run.stderr.strip()[-400:]}")
    try:
        return culled_share(json.loads(report.read_text(encoding="utf-8")))
    finally:
        report.unlink(missing_ok=True)


def capture(recipe, args, ctl, game_bin, out):
    """Runs one game for the recipe and leaves a checked capture in `out`; raises with the reason otherwise."""
    cwd = Path(tempfile.mkdtemp(prefix="scene-corpus-game-"))
    env = lab.game_env(
        HORIZON_RENDER_CAPTURE="1", HORIZON_TELEMETRY_URL="off", HORIZON_HOME=cwd / "home", HORIZON_LAUNCHER_HOME=cwd / "launcher",
        XDG_DATA_HOME=cwd / "xdg-data", XDG_CONFIG_HOME=cwd / "xdg-config",
        HORIZON_NO_AUDIO="1", HORIZON_LAUNCHER_NO_SELF_UPDATE="1",
        HORIZON_LAUNCHER_TRASH=cwd / "trash", HORIZON_LAUNCHER_CLIPBOARD=cwd / "clipboard", HORIZON_LAUNCHER_DESKTOP=cwd / "desktop",
    )
    (cwd / "options.txt").write_text(graphics_bench.OPTIONS.format(distance=recipe["render_distance"]))
    (cwd / "horizon-graphics.txt").write_text(graphics_text(recipe))
    game = graphics_bench.Game(ctl, cwd / "control.sock", env, cwd)
    launch = ["launch", *(["--game", game_bin] if game_bin else []), "--world", WORLD, "--seed", graphics_bench.SEED, "--creative", "--render-distance", recipe["render_distance"],
              "--resources", args.resources, "--no-pause", "--weather", "clear", "--time", 6000, "--offscreen",
              "--width", recipe["size"][0], "--height", recipe["size"][1], "--game-dir", cwd]
    try:
        game.run(*launch)
        game.run("wait-ready", 300)
        run_steps(game, recipe)
        shutil.rmtree(out, ignore_errors=True)
        game.run("render-capture", out)
        measured = measure(out)
        if "min_culled_share" in recipe["expect"]:
            measured["culled_share"] = replay_culled_share(args.replay_bin, out, args.resources, env)
        failures = check_expect(recipe["expect"], measured)
        if failures:
            raise RuntimeError("acceptance: " + "; ".join(failures))
        return measured
    finally:
        game.run("quit", check=False)
        for _ in range(100):
            if not (cwd / "control.sock").exists():
                break
            time.sleep(0.2)
        log = cwd / "control.log"
        if sys.exc_info()[0] and log.exists():
            print("\n".join(log.read_text(errors="replace").splitlines()[-15:]), file=sys.stderr)
        shutil.rmtree(cwd, ignore_errors=True)


def stamp(directory, recipe, commit):
    path = directory / "scene.json"
    scene = json.loads(path.read_text(encoding="utf-8"))
    scene["corpus"] = {"recipe": recipe["name"], "recipe_hash": recipe_hash(recipe), "commit": commit,
                       "created": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}
    path.write_text(json.dumps(scene, indent=2), encoding="utf-8")
    (directory / "corpus-id").write_text(corpus_id(directory) + "\n", encoding="utf-8")


def chosen(recipes, names):
    if not names:
        return recipes
    wanted = names.split(",")
    unknown = [n for n in wanted if n not in {r["name"] for r in recipes}]
    if unknown:
        sys.exit(f"unknown scene {','.join(unknown)}; have {','.join(r['name'] for r in recipes)}")
    return [r for r in recipes if r["name"] in wanted]


def generate(args):
    recipes = chosen(load_recipes(), args.scenes)
    if args.game:
        game_bin = Path(args.game).resolve()
        ctl = game_bin.with_name("horizon-ctl")
    else:
        subprocess.run(["cargo", "build", "--profile", "fast", "-p", "horizon-client"], cwd=ROOT, check=True)
        game_bin, ctl = None, lab.cargo_target() / "fast/horizon-ctl"
    commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    root = corpus_root()
    root.mkdir(parents=True, exist_ok=True)
    unavailable = []
    for recipe in recipes:
        for attempt in range(args.retries + 1):
            work = root / (recipe["name"] + ".new")
            try:
                measured = capture(recipe, args, ctl, game_bin, work)
                stamp(work, recipe, commit)
                shutil.rmtree(root / recipe["name"], ignore_errors=True)
                work.rename(root / recipe["name"])
                print(f"{recipe['name']}: ok {json.dumps(measured)}", flush=True)
                break
            except Exception as error:
                print(f"{recipe['name']}: attempt {attempt + 1} failed: {error}", file=sys.stderr, flush=True)
            finally:
                shutil.rmtree(work, ignore_errors=True)
        else:
            unavailable.append(recipe["name"])
            print(f"{recipe['name']}: unavailable", flush=True)
    return 1 if unavailable else 0


def list_scenes(args):
    root = corpus_root()
    print(f"{'scene':<12} {'id':<12} {'format':<8} {'recipe':<12} {'state':<8} {'size':>9}  acceptance")
    for recipe in load_recipes():
        directory = root / recipe["name"]
        scene = json.loads((directory / "scene.json").read_text(encoding="utf-8")) if (directory / "scene.json").exists() else None
        accept = " ".join(f"{k}={v:g}" for k, v in recipe["expect"].items())
        if scene is None:
            print(f"{recipe['name']:<12} {'-':<12} {'-':<8} {recipe_hash(recipe)[:10]:<12} {'missing':<8} {'-':>9}  {accept}")
            continue
        made = (scene.get("corpus") or {}).get("recipe_hash")
        size = sum(p.stat().st_size for p in directory.rglob("*") if p.is_file())
        ident = (directory / "corpus-id").read_text().strip()[:10] if (directory / "corpus-id").exists() else "-"
        version = ".".join(map(str, scene["version"])) if scene.get("version") else "0"
        print(f"{recipe['name']:<12} {ident:<12} {version:<8} {(made or '-')[:10]:<12} "
              f"{'stale' if made != recipe_hash(recipe) else 'current':<8} {size / 2**20:>7.0f}MB  {accept}")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    gen = sub.add_parser("generate", help="capture the scenes with the real game")
    gen.add_argument("--scenes", help="comma-separated scene names (default all)")
    gen.add_argument("--game", help="a horizon-client binary, with horizon-ctl beside it (default: build --profile fast)")
    gen.add_argument("--resources", default=str(lab.RESOURCES))
    gen.add_argument("--replay-bin", default=str(lab.cargo_target() / "fast/examples/scene_replay"))
    gen.add_argument("--retries", type=int, default=2)
    gen.set_defaults(run=generate)
    sub.add_parser("list", help="the scenes, whether each is captured and current").set_defaults(run=list_scenes)
    args = parser.parse_args()
    sys.exit(args.run(args))


if __name__ == "__main__":
    main()
