#!/usr/bin/env python3
"""Compare environment attribute entries (a value or a {modifier, argument} object) parsed by
EnvironmentAttributeMap's codec and applied to a subject, per attribute type."""
import json
import random
import struct

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness


def f32(x):
    """A decimal that names exactly one float, so both sides read the same value."""
    return struct.unpack('f', struct.pack('f', x))[0]


def op(name, argument):
    return {'modifier': name, 'argument': argument}


FLOAT_OPS = ['override', 'add', 'subtract', 'multiply', 'minimum', 'maximum', 'alpha_blend', 'blend_to_gray', 'and',
             'ADD', 'divide']
COLOR_OPS = ['override', 'add', 'subtract', 'multiply', 'alpha_blend', 'blend_to_gray', 'minimum', 'xor']
BOOL_OPS = ['and', 'nand', 'or', 'nor', 'xor', 'xnor', 'override', 'add']

FLOAT_ARGS = [0.25, -3, 1e-7, 2.5, -0.0, {'value': 2, 'alpha': 0.5}, {'value': 2}, {'value': 2, 'alpha': 1.5},
              '#112233', [0.1, 0.2, 0.3], True]
COLOR_ARGS = ['#ff8800', '#AbCdEf', [0.5, 0.5, 0.5], [1.5, -0.5, 0.25], '#80ff0000', '#00ff0000', [1, 0, 0, 0.25],
              [1, 0, 0, 0], [1, 0, 0, 1], 16711680, -1, {'brightness': 0.24, 'factor': 0.94},
              {'brightness': 0.095, 'factor': 0.94}, {'brightness': 1.5, 'factor': 0.5}, {'brightness': 0.5}, 0.5]
BOOL_ARGS = [True, False, 1, 'true']

# Attribute, subjects valid for it, plain values to try, operations and their arguments.
ATTRIBUTES = [
    ('minecraft:visual/cloud_height', [-2.5, 0, 192.33, -0.0], [0, 1, -1, 2.5, 1e30, -0.0, '1', True],
     FLOAT_OPS, FLOAT_ARGS),
    ('minecraft:visual/fog_end_distance', [0, 7.25, 1024], [0, -1, 96, 'x'], FLOAT_OPS, FLOAT_ARGS),
    ('minecraft:visual/sky_light_factor', [0, 0.3, 1], [0.5, 1.5, -0.1], FLOAT_OPS, FLOAT_ARGS),
    ('minecraft:visual/sun_angle', [0, 90, -270.5], [45, 720], FLOAT_OPS, FLOAT_ARGS),
    ('visual/fog_color', ['#000000', '#c0d8ff', [0.2, 0.5, 0.9], [1.5, -0.5, 0.25]],
     ['#112233', '#AbCdEf', '112233', '#1122', '#80112233', [0.1, 0.2, 0.3], [0.1, 0.2], 16711680, -1, 'red'],
     COLOR_OPS, COLOR_ARGS),
    ('minecraft:visual/cloud_color', ['#ccffffff', '#00000000', [0.2, 0.5, 0.9, 0.5], [0.2, 0.5, 0.9, 1]],
     ['#80112233', '#112233', [0.1, 0.2, 0.3, 0.4], [0.1, 0.2, 0.3], -2147483648],
     COLOR_OPS, COLOR_ARGS),
    ('minecraft:gameplay/water_evaporates', [True, False], [True, False, 1, 'false'], BOOL_OPS, BOOL_ARGS),
]


def cases(build):
    lines = ['registry']
    for attribute, subjects, values, ops, args in ATTRIBUTES:
        entries = values + [op(o, a) for o in ops for a in args] + [{'modifier': 'add'}, {'argument': 1}]
        for subject in subjects:
            lines += [f'{attribute}\t{json.dumps(subject)}\t{json.dumps(e)}' for e in entries]
    rng = random.Random(26_3)
    for _ in range(600):
        subject, argument, alpha = (f32(rng.uniform(-1e4, 1e4)), f32(rng.uniform(-1e4, 1e4)), f32(rng.random()))
        name = rng.choice(['add', 'subtract', 'multiply', 'minimum', 'maximum'])
        lines.append(f'minecraft:visual/cloud_height\t{subject!r}\t{json.dumps(op(name, argument))}')
        blend = op('alpha_blend', {'value': argument, 'alpha': alpha})
        lines.append(f'minecraft:visual/cloud_height\t{subject!r}\t{json.dumps(blend)}')
        color = [f32(rng.random()) for _ in range(4)]
        source = [f32(rng.random()) for _ in range(4)]
        gray = op('blend_to_gray', {'brightness': f32(rng.random()), 'factor': f32(rng.random())})
        for entry in (op('alpha_blend', source), op('multiply', source), gray, op('add', source[:3])):
            lines.append(f'minecraft:visual/cloud_color\t{json.dumps(color)}\t{json.dumps(entry)}')
            lines.append(f'visual/fog_color\t{json.dumps(color[:3])}\t{json.dumps(entry)}')
    path = build / 'cases.txt'
    path.write_text('\n'.join(lines) + '\n')
    return path, len(lines)


if __name__ == '__main__':
    harness.differential(
        'environment-attribute-modifier-parity', 'VanillaEnvironmentAttributeModifierOracle',
        'environment_attribute_modifier_fixture', cases,
        'EnvironmentAttributeMap.Entry codec and AttributeModifier.apply for float, angle, RGB, ARGB and boolean '
        'attributes, with AttributeRange.sanitize',
        sources=[lab.ROOT / 'crates/horizon-world/src/environment_attributes.rs'])
