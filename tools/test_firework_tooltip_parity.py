#!/usr/bin/env python3
"""Compare firework rocket and firework star tooltips with the pinned vanilla client.

Random rockets (flight durations 0 to 3 and higher, up to six explosions, runs of equal ones) and stars, with dye and
custom colours, fade colours, trails and twinkles, some without the component (a rocket's default) and some whose
tooltip_display hides it. A deterministic edge corpus adds every flight duration 0..255, runs of 0..256 equal
explosions, interleaved runs, colour lists up to 32 entries (including custom, negative and duplicate colours), both
components on one stack, firework components on non-rocket/non-star items and every combination of hidden components.
Vanilla prints what `Fireworks.addToTooltip` and `FireworkExplosion.addToTooltip` add
(VanillaFireworkTooltipOracle); Horizon what `item_tooltip::firework_lines` gives (examples/probe/firework_tooltip_fixture).
`--no-edges` leaves the edge corpus out and writes the random corpus exactly as before.
"""
import random
import sys
try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness

SHAPES = ['small_ball', 'large_ball', 'star', 'creeper', 'burst']
DYES = [0xF0F0F0, 0xEB8844, 0xC354CD, 0x6689D3, 0xDECF2A, 0x41CD34, 0xD88198, 0x434343, 0xABABAB, 0x287697,
        0x7B2FBE, 0x253192, 0x51301A, 0x3B511A, 0xB3312C, 0x1E1B1B]
COUNT = 3000


def explosion(rng):
    def colors():
        return [rng.choice(DYES) if rng.random() < 0.8 else rng.randrange(1 << 24) for _ in range(rng.choice([0, 1, 1, 2, 3]))]
    fields = [f'shape:"{rng.choice(SHAPES)}"']
    for key, values in (('colors', colors()), ('fade_colors', colors() if rng.random() < 0.4 else [])):
        if values:
            fields.append(f'{key}:[I;{",".join(map(str, values))}]')
    if rng.random() < 0.4:
        fields.append('has_trail:1b')
    if rng.random() < 0.4:
        fields.append('has_twinkle:1b')
    return '{' + ','.join(fields) + '}'


def stack(rng):
    components = []
    if rng.random() < 0.7:
        item = 'firework_rocket'
        if rng.random() < 0.9:
            explosions = []
            for _ in range(rng.choice([0, 0, 1, 1, 2, 3, 6])):
                # Runs of equal explosions are counted.
                explosions.append(explosions[-1] if explosions and rng.random() < 0.4 else explosion(rng))
            flight = rng.choice([0, 1, 1, 2, 3, 3, 7, 255])
            components.append(f'"minecraft:fireworks":{{flight_duration:{flight},explosions:[{",".join(explosions)}]}}')
    else:
        item = 'firework_star'
        if rng.random() < 0.9:
            components.append(f'"minecraft:firework_explosion":{explosion(rng)}')
    if rng.random() < 0.1:
        hidden = rng.choice(['minecraft:fireworks', 'minecraft:firework_explosion'])
        components.append(f'"minecraft:tooltip_display":{{hidden_components:["{hidden}"]}}')
    body = f',components:{{{",".join(components)}}}' if components else ''
    return f'{{id:"minecraft:{item}",count:1{body}}}'


def edge_stack(item, components):
    return '{id:"minecraft:%s",count:1,components:{%s}}' % (item, ','.join(components))


def edge_explosion(shape='star', colors=(), fade=(), trail=False, twinkle=False):
    fields = [f'shape:"{shape}"']
    if colors:
        fields.append('colors:[I;%s]' % ','.join(map(str, colors)))
    if fade:
        fields.append('fade_colors:[I;%s]' % ','.join(map(str, fade)))
    if trail:
        fields.append('has_trail:1b')
    if twinkle:
        fields.append('has_twinkle:1b')
    return '{' + ','.join(fields) + '}'


def edge_fireworks(flight, explosions):
    return '"minecraft:fireworks":{flight_duration:%d,explosions:[%s]}' % (
        flight, ','.join(explosions))


def edge_hidden(*components):
    return '"minecraft:tooltip_display":{hidden_components:[%s]}' % (
        ','.join('"%s"' % c for c in components))


def edge_cases():
    """The cases the random generator cannot produce: every flight duration,
    long runs, long colour lists, both components and non-rocket carriers."""
    out = []
    for flight in range(256):
        out.append(edge_stack('firework_rocket', [edge_fireworks(flight, [edge_explosion()])]))
    star = edge_explosion(shape='star')
    trail = edge_explosion(shape='small_ball', trail=True)
    burst = edge_explosion(shape='burst', colors=[0x41CD34], twinkle=True)
    for count in [0, 1, 2, 3, 4, 5, 6, 7, 8, 16, 32, 64, 128, 255, 256]:
        out.append(edge_stack('firework_rocket', [edge_fireworks(1, [star] * count)]))
    for shape in SHAPES:
        for count in (7, 8, 16):
            out.append(edge_stack('firework_rocket', [
                edge_fireworks(1, [edge_explosion(shape=shape)] * count)]))
    red = edge_explosion(shape='star', colors=[0xFF0000])
    blue = edge_explosion(shape='star', colors=[0x0000FF])
    faded = edge_explosion(shape='star', colors=[0xFF0000], fade=[0x00FF00])
    for sequence in ([star, trail, star], [star, star, trail, trail, star],
                     [star] * 7 + [trail], [star] * 6 + [trail] * 7,
                     [star, trail, burst, star, trail, burst],
                     [red, blue, red], [red, faded, red], [red, red, blue, blue]):
        out.append(edge_stack('firework_rocket', [edge_fireworks(3, sequence)]))
    dyes = [0xF0F0F0, 0xEB8844, 0xC354CD, 0x6689D3, 0xDECF2A, 0x41CD34, 0xD88198, 0x434343,
            0xABABAB, 0x287697, 0x7B2FBE, 0x253192, 0x51301A, 0x3B511A, 0xB3312C, 0x1E1B1B]
    for count in [0, 1, 2, 3, 4, 8, 16, 32]:
        colors = (dyes * 2)[:count]
        out.append(edge_stack('firework_star', [
            '"minecraft:firework_explosion":' + edge_explosion(colors=colors,
                                                               fade=colors[::-1])]))
    out.append(edge_stack('firework_star', [
        '"minecraft:firework_explosion":' + edge_explosion(
            colors=[0x123456, -1, 0xFFFFFF, 0x123456],
            fade=[-2147483648, 2147483647])]))
    for shape in SHAPES:
        for trail_flag in (False, True):
            for twinkle_flag in (False, True):
                out.append(edge_stack('firework_star', [
                    '"minecraft:firework_explosion":' + edge_explosion(
                        shape=shape, trail=trail_flag, twinkle=twinkle_flag)]))
    both = [edge_fireworks(2, [star, trail]),
            '"minecraft:firework_explosion":' + edge_explosion(shape='creeper',
                                                               colors=[0x41CD34], trail=True)]
    out.append(edge_stack('firework_rocket', both))
    out.append(edge_stack('stick', both))
    out.append(edge_stack('diamond_sword', [both[0]]))
    out.append(edge_stack('arrow', [both[1]]))
    out.append(edge_stack('firework_star', both))
    for hidden in [('minecraft:fireworks',), ('minecraft:firework_explosion',),
                   ('minecraft:fireworks', 'minecraft:firework_explosion')]:
        out.append(edge_stack('stick', both + [edge_hidden(*hidden)]))
    return out


def cases(build):
    rng = random.Random(0x46495245)
    path = build / 'cases.txt'
    resources = lab.LOCAL / 'horizon-client' / '26.3'
    lines = [stack(rng) for _ in range(COUNT)]
    if '--no-edges' not in sys.argv:
        lines += edge_cases()
    path.write_text(f'#resources {resources}\n' + ''.join(line + '\n' for line in lines))
    return path, len(lines)


if __name__ == '__main__':
    harness.differential(
        'firework-tooltip-parity', 'VanillaFireworkTooltipOracle', 'firework_tooltip_fixture', cases,
        'Firework rocket and star tooltip lines: flight duration (every 0..255), explosions (counted runs up to 256), '
        'colours and fade lists to 32 entries, trail, twinkle, defaults, tooltip_display, both components on one stack '
        'and firework components on non-rocket/non-star items',
        sources=[lab.ROOT / 'crates/horizon-items/src/enchant.rs',
                 lab.ROOT / 'crates/horizon-client/src/item_tooltip.rs', lab.ROOT / 'crates/horizon-items/src/item_data.rs',
                 lab.ROOT / 'crates/horizon-client-render/src/particles.rs'])
