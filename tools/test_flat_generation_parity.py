#!/usr/bin/env python3
"""Compare superflat chunks block-for-block with vanilla, decoration included.

A superflat world decorates its layers with what `FlatLevelGeneratorSettings.adjustGenerationSettings`
keeps of its biome: the lava lakes when `lakes` is on, the biome's placed features when `features` is
on (but those of the two structure steps, and its own lakes step when `lakes` is on), and one
`FillLayerFeature` per layer that is not opaque to MOTION_BLOCKING. Its structures come from its
`structure_overrides`.

Vanilla side: VanillaChunkGenerationOracle with HORIZON_FLAT=<preset>, which builds a real
FlatLevelSource from the `flat_level_generator_preset` registry and runs every generation stage
through WorldGenRegion instances. Horizon side: chunk_dump with the same variable, generating with
OverworldGenerator::flat. Every block state, quart biome and block entity of each target chunk is
compared. Presets that decorate must hold blocks beyond their layers in vanilla, so the comparison is
known to cover the decoration.

usage: test_flat_generation_parity.py [--preset ID] [--seed N] [--sites K]
"""
import argparse
import json
import os
import random
import subprocess
import sys

try:
    from . import lab, test_chunk_generation_parity as chunks
except ImportError:
    import lab
    import test_chunk_generation_parity as chunks

# The three presets that decorate (Overworld with lakes too), then three that only place their layers
# and structures: Snowy Kingdom's snow is a FillLayerFeature, the others check the structure steps.
PRESETS = [
    'minecraft:overworld',
    'minecraft:desert',
    'minecraft:tunnelers_dream',
    'minecraft:snowy_kingdom',
    'minecraft:bottomless_pit',
    'minecraft:water_world',
]
SEEDS = [12345, -7046029254386353131]


class Oracle(chunks.Oracle):
    """The chunk generation oracle, compiled into this harness's own directory (another harness may
    be compiling its own copy), on a smaller heap: a flat world needs no noise caches."""

    def __init__(self):
        self.lock, self.jars, self.libraries = chunks.base.verify_artifacts()
        self.dependencies = os.pathsep.join(map(str, [self.jars['server'], *self.libraries]))
        self.classes = build_dir() / 'classes'
        self.classes.mkdir(exist_ok=True)
        result = subprocess.run(['javac', '--release', str(self.lock['minecraft']['java_major']), '-proc:none',
                                 '-cp', self.dependencies, '-d', str(self.classes), str(chunks.ORACLE)],
                                capture_output=True, text=True)
        if result.returncode:
            raise SystemExit('javac failed:\n' + result.stderr[-4000:])

    def run(self, out, args):
        result = subprocess.run(['java', '-Xmx4G', '-XX:ActiveProcessorCount=2',
                                 '-cp', str(self.classes) + os.pathsep + self.dependencies,
                                 'dev.horizonlab.parity.VanillaChunkGenerationOracle', *map(str, [args[0], out, *args[1:]])],
                                capture_output=True, text=True, env=dict(os.environ))
        if result.returncode:
            output = (result.stdout + result.stderr).splitlines()
            lines = [l for l in output if 'Exception' in l or 'Caused' in l or '\tat ' in l]
            raise SystemExit(f'oracle failed (exit {result.returncode}): ' + ' | '.join(lines[:10] or output[-10:]))


def build_dir():
    path = lab.LOCAL / 'flat-generation-parity'
    path.mkdir(parents=True, exist_ok=True)
    return path


def preset_settings(preset):
    namespace, path = preset.split(':', 1)
    file = chunks.RESOURCES / f'data/{namespace}/worldgen/flat_level_generator_preset/{path}.json'
    return json.loads(file.read_text())['settings']


def beyond_layers(dump, settings, names):
    """How many blocks of the dump are neither air nor a block of the preset's layers."""
    layers = {layer['block'] for layer in settings['layers']}
    count = 0
    for chunk in dump.values():
        for row in chunk['y'].values():
            for state in row.split():
                name = names[int(state)].split('[', 1)[0]
                if state != chunks.AIR and name not in layers:
                    count += 1
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--preset', action='append', help='run only this preset (repeatable)')
    parser.add_argument('--seed', type=int, action='append', help='run only this seed (repeatable)')
    parser.add_argument('--sites', type=int, default=2, help='sites per seed besides 0,0')
    parser.add_argument('--radius', type=int, default=1, help='chunk radius around each site')
    args = parser.parse_args()
    os.environ['HORIZON_PHASE'] = 'full'
    oracle = Oracle()
    binary = chunks.rust_binary()
    names = chunks.state_names()
    build = build_dir()
    differing = 0
    undecorated = []
    for preset in args.preset or PRESETS:
        settings = preset_settings(preset)
        decorates = settings.get('features') or settings.get('lakes')
        os.environ['HORIZON_FLAT'] = preset
        for seed in args.seed or SEEDS:
            rng = random.Random(seed)
            r = args.radius
            sites = [(0, 0)] + [(rng.randint(-400, 400), rng.randint(-400, 400)) for _ in range(args.sites)]
            rects = [f'{x - r},{z - r},{x + r},{z + r}' for x, z in sites]
            tag = f'{preset.split(":", 1)[1]}-{seed}'
            vanilla_path, rust_path = build / f'vanilla-{tag}.txt', build / f'rust-{tag}.txt'
            oracle.run(vanilla_path, ['dump', seed, 'overworld', *rects])
            stderr = chunks.run_rust(binary, rust_path, seed, 'overworld', rects)
            for line in stderr.splitlines()[:5]:
                print('  horizon:', line)
            vanilla, rust = chunks.parse(vanilla_path), chunks.parse(rust_path)
            counts = chunks.compare(vanilla, rust, names, tag)
            differing += counts[0]
            extra = beyond_layers(vanilla, settings, names)
            if decorates and not extra:
                undecorated.append(tag)
            print(f'{tag}: {len(vanilla)} chunks, {extra} decorated blocks in vanilla, differing chunks {counts[0]} '
                  f'(blocks {counts[1]}, biome cells {counts[2]}, block entities {counts[3]})', flush=True)
    del os.environ['HORIZON_FLAT']
    if undecorated:
        sys.exit('vanilla placed nothing beyond the layers, so the decoration went unchecked: ' + ', '.join(undecorated))
    if differing:
        sys.exit(f'{differing} chunks differ from vanilla')
    print('Flat generation parity passed')


if __name__ == '__main__':
    main()
