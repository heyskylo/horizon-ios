#!/usr/bin/env python3
"""Compare EnvironmentAttributeSystem layer stacks: a dimension's attributes, the biome layer (one
biome, or a Gaussian mix of a grid of biomes) and the rain and thunder layers, for every float,
angle, color and boolean attribute, with the vanilla biome and dimension type data."""
import json
import random
import struct

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness

# Biomes with modifiers, so the mix and the weather blend modified values, not only plain ones.
SYNTHETIC = {
    'test:warm': {
        'visual/fog_color': {'modifier': 'multiply', 'argument': '#ffcc88'},
        'visual/fog_end_distance': {'modifier': 'multiply', 'argument': 0.5},
        'visual/sky_light_factor': {'modifier': 'alpha_blend', 'argument': {'value': 0.2, 'alpha': 0.4}},
        'visual/cloud_color': {'modifier': 'blend_to_gray', 'argument': {'brightness': 0.5, 'factor': 0.3}},
        'visual/sun_angle': {'modifier': 'add', 'argument': 30},
        'gameplay/water_evaporates': True,
    },
    'test:cold': {
        'visual/fog_color': '#203040',
        'visual/sky_light_factor': {'modifier': 'maximum', 'argument': 0.8},
        'visual/fog_start_distance': {'modifier': 'subtract', 'argument': 8},
        'visual/star_brightness': 0.9,
        'gameplay/water_evaporates': False,
    },
}


def f32(x):
    """A decimal that names exactly one float, so both sides read the same value."""
    return struct.unpack('f', struct.pack('f', x))[0]


def cases(build):
    data_root = lab.LOCAL / 'horizon-client/26.3/data/minecraft'
    data = {}
    for path in sorted((data_root / 'worldgen/biome').glob('*.json')):
        data['biome:' + path.stem] = json.loads(path.read_text()).get('attributes', {})
    for path in sorted((data_root / 'dimension_type').glob('*.json')):
        data['dimension:' + path.stem] = json.loads(path.read_text()).get('attributes', {})
    data.update(SYNTHETIC)
    biomes = [k for k in data if not k.startswith('dimension:')]
    dimensions = [k for k in data if k.startswith('dimension:')]
    rng = random.Random(11486)
    lines = [json.dumps(data, separators=(',', ':'))]
    for _ in range(900):
        rain = 0.0 if rng.random() < 0.3 else rng.random()
        thunder = 0.0 if rng.random() < 0.4 else rng.random() * (rain if rng.random() < 0.85 else 1.0)
        lines.append(json.dumps({
            'dimension': rng.choice(dimensions),
            'biomes': biomes,
            'grid': rng.sample(biomes, rng.randint(1, 4)),
            'position': [rng.uniform(-1000, 1000), rng.uniform(-64, 320), rng.uniform(-1000, 1000)],
            'blend': rng.random() < 0.8,
            'weather': rng.random() < 0.75,
            'rain': f32(rain),
            'thunder': f32(thunder),
        }, separators=(',', ':')))
    path = build / 'cases.txt'
    path.write_text('\n'.join(lines) + '\n')
    return path, len(lines) - 1


if __name__ == '__main__':
    harness.differential(
        'environment-system-parity', 'VanillaEnvironmentSystemOracle', 'environment_system_fixture', cases,
        'EnvironmentAttributeSystem (constant, biome and weather layers, bakeLayerSampler, sanitize), '
        'WeatherAttributes, SpatialAttributeInterpolator and GaussianSampler',
        sources=[lab.ROOT / 'crates/horizon-world/src/environment_attributes.rs',
                 lab.ROOT / 'crates/horizon-world/src/environment_system.rs'])
