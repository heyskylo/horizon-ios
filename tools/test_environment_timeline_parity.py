#!/usr/bin/env python3
"""Compare timelines: every EasingType over 0-1 and beyond, and attribute tracks (the vanilla
day, moon and early game timelines plus synthetic ones) parsed by AttributeTrack's codec,
validated against their period and sampled at many ticks."""
import json
import random
import struct

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness

EASINGS = ['constant', 'linear', 'in_back', 'in_bounce', 'in_circ', 'in_cubic', 'in_elastic', 'in_expo', 'in_quad',
           'in_quart', 'in_quint', 'in_sine', 'in_out_back', 'in_out_bounce', 'in_out_circ', 'in_out_cubic',
           'in_out_elastic', 'in_out_expo', 'in_out_quad', 'in_out_quart', 'in_out_quint', 'in_out_sine', 'out_back',
           'out_bounce', 'out_circ', 'out_cubic', 'out_elastic', 'out_expo', 'out_quad', 'out_quart', 'out_quint',
           'out_sine']
BEZIERS = [[0.362, 0.241, 0.638, 0.759], [0.25, 0.1, 0.25, 1.0], [0, 0, 1, 1], [0.9, -0.5, 0.1, 1.5], [1, 0, 0, 1]]
BAD_EASINGS = ['"in_out_nope"', '{"cubic_bezier": [1.5, 0, 0, 1]}', '{"cubic_bezier": [0, 0, 1]}', '3']

# Bases for each attribute a track can change.
BASES = {'float': [1.0, 0.25], 'wide': [1.0, 15.0, 0.25, -3.0], 'rgb': ['#c0d8ff', '#000000', [1.5, 0.2, 0.9]],
         'argb': ['#ccffffff', '#00000000'], 'bool': [True, False]}
KINDS = {
    'minecraft:visual/sky_light_factor': 'float', 'minecraft:gameplay/sky_light_level': 'float',
    'minecraft:visual/star_brightness': 'float', 'minecraft:visual/sun_angle': 'wide',
    'minecraft:visual/moon_angle': 'wide', 'minecraft:visual/star_angle': 'wide',
    'minecraft:gameplay/cat_waking_up_gift_chance': 'float', 'minecraft:gameplay/turtle_egg_hatch_chance': 'float',
    'minecraft:gameplay/surface_slime_spawn_chance': 'float', 'minecraft:visual/cloud_height': 'wide',
    'minecraft:visual/fog_color': 'rgb', 'minecraft:visual/sky_color': 'rgb', 'minecraft:visual/sky_light_color': 'rgb',
    'minecraft:visual/cloud_color': 'argb', 'minecraft:visual/sunrise_sunset_color': 'argb',
    'minecraft:audio/firefly_bush_sounds': 'bool', 'minecraft:gameplay/bees_stay_in_hive': 'bool',
    'minecraft:gameplay/creaking_active': 'bool', 'minecraft:gameplay/monsters_burn': 'bool',
    'minecraft:gameplay/can_pillager_patrol_spawn': 'bool',
}


def f32(x):
    """A decimal that names exactly one float, so both sides read the same value."""
    return struct.unpack('f', struct.pack('f', x))[0]


def track_line(attribute, period, track, base, ticks):
    return '\t'.join(['track', attribute, str(period) if period else '-', json.dumps(track), json.dumps(base),
                      ','.join(map(str, ticks))])


def ticks_for(track, period, rng):
    keys = [k['ticks'] for k in track.get('keyframes', []) if isinstance(k, dict) and isinstance(k.get('ticks'), int)]
    out = {-1, 0, 1, 10 ** 12 + 7, -(10 ** 12) - 3}
    for k in keys:
        out |= {k - 1, k, k + 1}
    span = (period or max(keys + [100])) * 2
    out |= {rng.randrange(-span, span) for _ in range(40)}
    return sorted(out)


def cases(build):
    rng = random.Random(26_3_11486)
    lines = []
    xs = [0.0, -0.0, 1.0, 0.5, 0.25, 0.75, 1e-6, 0.999999, -0.2, 1.2, 0.36363637, 0.72727275, 0.9090909]
    xs += [f32(rng.random()) for _ in range(60)]
    for easing in [json.dumps(e) for e in EASINGS] + [json.dumps({'cubic_bezier': b}) for b in BEZIERS] + BAD_EASINGS:
        lines += [f'ease\t{easing}\t{x!r}' for x in xs]

    timelines = lab.LOCAL / 'horizon-client/26.3/data/minecraft/timeline'
    for name in ('day', 'moon', 'early_game'):
        data = json.loads((timelines / f'{name}.json').read_text())
        for attribute, track in data['tracks'].items():
            if attribute in KINDS:
                for base in BASES[KINDS[attribute]]:
                    period = data.get('period_ticks')
                    lines.append(track_line(attribute, period, track, base, ticks_for(track, period, rng)))

    float_keys = [{'ticks': 0, 'value': 0.0}, {'ticks': 300, 'value': 1.0}, {'ticks': 900, 'value': -2.5}]
    synthetic = [('minecraft:visual/cloud_height', {'keyframes': float_keys, 'ease': e}) for e in EASINGS]
    synthetic += [('minecraft:visual/cloud_height', {'keyframes': float_keys, 'ease': {'cubic_bezier': b}})
                  for b in BEZIERS]
    synthetic += [
        ('minecraft:visual/cloud_height', {'modifier': 'alpha_blend', 'keyframes': [
            {'ticks': 10, 'value': {'value': 100, 'alpha': 0.2}}, {'ticks': 500, 'value': 300}]}),
        ('minecraft:visual/cloud_height', {'modifier': 'add', 'ease': 'in_out_sine', 'keyframes': [
            {'ticks': 0, 'value': 5}, {'ticks': 0, 'value': 9}, {'ticks': 50, 'value': -5}]}),
        ('minecraft:visual/cloud_height', {'keyframes': [{'ticks': 77, 'value': 3.5}]}),
        ('minecraft:visual/fog_color', {'modifier': 'multiply', 'ease': {'cubic_bezier': BEZIERS[0]}, 'keyframes': [
            {'ticks': 100, 'value': '#ff8800'}, {'ticks': 700, 'value': [0.2, 0.4, 1.0]}]}),
        ('minecraft:visual/fog_color', {'modifier': 'blend_to_gray', 'keyframes': [
            {'ticks': 0, 'value': {'brightness': 0.1, 'factor': 0.0}},
            {'ticks': 600, 'value': {'brightness': 0.9, 'factor': 1.0}}]}),
        ('minecraft:visual/fog_color', {'modifier': 'alpha_blend', 'ease': 'out_bounce', 'keyframes': [
            {'ticks': 0, 'value': '#00ff0000'}, {'ticks': 400, 'value': '#ff0000ff'}]}),
        ('minecraft:visual/cloud_color', {'modifier': 'multiply', 'keyframes': [
            {'ticks': 0, 'value': '#80ffffff'}, {'ticks': 1000, 'value': '#ff102030'}]}),
        ('minecraft:audio/firefly_bush_sounds', {'modifier': 'xor', 'keyframes': [
            {'ticks': 0, 'value': True}, {'ticks': 100, 'value': False}, {'ticks': 900, 'value': True}]}),
        # Invalid tracks.
        ('minecraft:visual/cloud_height', {'keyframes': []}),
        ('minecraft:visual/cloud_height', {'keyframes': [{'ticks': 5, 'value': 1}, {'ticks': 2, 'value': 1}]}),
        ('minecraft:visual/cloud_height', {'keyframes': [{'ticks': 5, 'value': 1}] * 3}),
        ('minecraft:visual/cloud_height', {'keyframes': [{'ticks': 1, 'value': 1}, {'ticks': 5, 'value': 2},
                                                         {'ticks': 5, 'value': 3}, {'ticks': 5, 'value': 4}]}),
        ('minecraft:visual/cloud_height', {'keyframes': [{'ticks': -1, 'value': 1}]}),
        ('minecraft:visual/cloud_height', {'keyframes': [{'ticks': 1, 'value': 'x'}]}),
        ('minecraft:visual/cloud_height', {'modifier': 'blend_to_gray', 'keyframes': [{'ticks': 1, 'value': 1}]}),
        ('minecraft:visual/cloud_height', {'keyframes': [{'ticks': 1, 'value': 1}], 'ease': 'nope'}),
    ]
    for attribute, track in synthetic:
        for period in (None, 1000, 24000, 600):
            for base in BASES[KINDS.get(attribute, 'float')][:2]:
                lines.append(track_line(attribute, period, track, base, ticks_for(track, period, rng)))
    path = build / 'cases.txt'
    path.write_text('\n'.join(lines) + '\n')
    return path, len(lines)


if __name__ == '__main__':
    harness.differential(
        'environment-timeline-parity', 'VanillaEnvironmentTimelineOracle', 'environment_timeline_fixture', cases,
        'EasingType and Ease, AttributeTrack codec and validatePeriod, KeyframeTrack validation, '
        'KeyframeTrackSampler segments and sampling, argumentKeyframeLerp and the modifiers',
        sources=[lab.ROOT / 'crates/horizon-world/src/environment_attributes.rs',
                 lab.ROOT / 'crates/horizon-world/src/environment_timeline.rs'])
