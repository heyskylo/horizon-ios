#!/usr/bin/env python3
import json
import math
import shutil
import statistics
import sys
import time
from argparse import Namespace
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import lab
import perf_bench as bench
import stress_scenes as stress

# Keep this run isolated from the normal benchmark workspace.
bench.WORK = Path('/tmp/horizon-entity-occlusion-release-quick')
shutil.rmtree(bench.WORK, ignore_errors=True)
bench.WORK.mkdir(parents=True)

FLOOR_Y = stress.FLOOR_Y
OX, OZ = stress.ORIGIN


def summon_grid(count, cols, ys, zs, spacing=1.4):
    commands = []
    i = 0
    for z in zs:
        for y in ys:
            for col in range(cols):
                if i >= count:
                    return commands
                x = OX + 0.5 + (col - (cols - 1) / 2) * spacing
                commands.append(
                    f'summon pig {stress.num(x)} {stress.num(y)} {stress.num(OZ + z)} '
                    '{NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b}'
                )
                i += 1
    return commands


def common(entities, wall=False, slit=False):
    out = list(stress.SETTINGS) + stress.floor(OX, OZ)
    if wall:
        out.append(f'fill {OX - 17} {FLOOR_Y - 1} {OZ + 7} {OX + 17} {FLOOR_Y + 22} {OZ + 7} stone')
        if slit:
            out.append(f'setblock {OX} {FLOOR_Y + 10} {OZ + 7} air')
    out.extend(entities)
    return out


def normal(ox, oz):
    return common(summon_grid(24, 4, [296, 298, 300], [10, 13], 1.8))


def wall_192(ox, oz):
    return common(summon_grid(192, 12, [293, 296, 299, 302], [11, 14, 17, 20], 1.4), True)


def wall_192_slit(ox, oz):
    return common(summon_grid(192, 12, [293, 296, 299, 302], [11, 14, 17, 20], 1.4), True, True)


def wall_512(ox, oz):
    return common(summon_grid(512, 16, [291 + 2 * i for i in range(8)], [11, 14, 17, 20], 1.8), True)


def wall_512_slit(ox, oz):
    return common(summon_grid(512, 16, [291 + 2 * i for i in range(8)], [11, 14, 17, 20], 1.8), True, True)

# Register only the five scenes used below; perf_bench will set up/clean them through control.
custom = [
    ('entity_normal', normal),
    ('entity_wall', wall_192),
    ('entity_wall_slit', wall_192_slit),
    ('entity_stress', wall_512),
    ('entity_stress_slit', wall_512_slit),
]
stress.STRESS.extend(custom)
stress.NAMES.extend(name for name, _ in custom)
stress.BOX = (-24, 24, 2, 40, FLOOR_Y - 9, FLOOR_Y + 26)
SCENE_NAMES = [name for name, _ in custom]

# The branch setting is written next to options.txt, as the game reads it at startup.
original_write_options = bench.write_options

def write_options(game_dir, distance, version=None):
    text = original_write_options(game_dir, distance, version)
    (game_dir / 'horizon-graphics.txt').write_text(
        f'entityCulling:{CURRENT_MODE}\n'
        'entityCullingPrecheck:64\n'
        'graphicsApi:opengl\n'
        'overlay:fps\n'
    )
    return text

bench.write_options = write_options

original_game_env = lab.game_env

def game_env(base=None, **extra):
    extra['HORIZON_OCCLUSION_CULL_COUNTS'] = '1'
    return original_game_env(base, **extra)

bench.lab.game_env = game_env

original_control_call = bench.Control.__call__
CURRENT_MODE = 'off'
RAW = []
SHOT_INDEX = 0

def capture(self, line, timeout=600):
    global SHOT_INDEX
    if line.startswith('fly ') and line.endswith(' raw'):
        shot_dir = bench.WORK / 'screenshots'
        shot_dir.mkdir(parents=True, exist_ok=True)
        scene = SCENE_NAMES[SHOT_INDEX] if SHOT_INDEX < len(SCENE_NAMES) else f'scene-{SHOT_INDEX}'
        shot_path = shot_dir / f'{CURRENT_MODE}-{scene}.png'
        # The current camera pose is already settled and the graphics file enables the FPS overlay.
        original_control_call(self, f'screenshot {shot_path}')
        print(f'SCREENSHOT {shot_path}', flush=True)
        SHOT_INDEX += 1
    result = original_control_call(self, line, timeout)
    if line.startswith('fly ') and line.endswith(' raw'):
        RAW.append((CURRENT_MODE, result))
    return result

bench.Control.__call__ = capture


def percentile(values, fraction):
    values = sorted(float(v) for v in values if math.isfinite(float(v)))
    if not values:
        return None
    return values[min(len(values) - 1, max(0, math.ceil(len(values) * fraction) - 1))]


def args_for():
    return Namespace(
        build=str(ROOT / 'target/release'),
        game='horizon',
        resources=str(ROOT / '.local/horizon-client/26.3'),
        width=1280,
        height=720,
        offscreen=True,
        server=False,
        server_address=None,
        port_work=None,
        warm=False,
        timeout=240.0,
        scenes=','.join(SCENE_NAMES),
        extra_scenes=[],
        settle=0.5,
        seconds=2.0,
        label='entity-occlusion-release-quick',
    )


if __name__ == '__main__':
    all_results = {}
    for mode in ('off', 'sync', 'async'):
        CURRENT_MODE = mode
        args = args_for()
        before = len(RAW)
        SHOT_INDEX = 0
        print(f'=== {mode} (release/OpenGL, 2 s per scene) ===', flush=True)
        result = bench.run_horizon(args, 8)
        mode_raw = [summary for run_mode, summary in RAW[before:] if run_mode == mode]
        mode_out = {}
        for name in SCENE_NAMES:
            scene = result['scenes'][name]
            frame = bench.frames(name, 0, scene['frame_ms'], scene.get('cpu_ms', ()), scene.get('gpu_ms', ())) or {}
            raw = mode_raw[len(mode_out)] if len(mode_raw) > len(mode_out) else {}
            occ = raw.get('async_occlusion_culling', {}) if isinstance(raw, dict) else {}
            mode_out[name] = {
                'frames': frame.get('frames'),
                'avg_fps': frame.get('avg_fps'),
                'p50_ms': frame.get('p50_ms'),
                'p95_ms': frame.get('p95_ms'),
                'cpu_avg_ms': (sum(scene.get('cpu_ms', ())) / len(scene['cpu_ms'])) if scene.get('cpu_ms') else None,
                'cpu_p50_ms': frame.get('cpu_p50_ms'),
                'async_occlusion': occ,
            }
            print(json.dumps({'mode': mode, 'scene': name, **mode_out[name]}, sort_keys=True), flush=True)
        all_results[mode] = mode_out
    (bench.WORK / 'results.json').write_text(json.dumps(all_results, indent=2) + '\n')
    print(f'RESULTS {bench.WORK / "results.json"}', flush=True)
