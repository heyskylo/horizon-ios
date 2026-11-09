#!/usr/bin/env python3
"""A fishing bobber landing on a target block on a dedicated server, against vanilla's, seen by a vanilla client.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator in survival holding a fishing rod. The floor ahead of her is target
blocks. LocatorBarClient (a player made of vanilla's protocol code) casts the rod (ServerboundUseItemPacket); the
bobber comes down on a target's top face. `FishingHook.onHitBlock` calls `Projectile.onHitBlock`, so the block's
`onProjectileHit` runs: `TargetBlock.onProjectileHit` powers it and, the owner being a `ServerPlayer`, awards
`target_hit`. The statistic is read through a scoreboard objective before the cast, after it and after a second cast;
whether hits are counted, and whether the second cast adds more, must match.

A bobber falling onto a top face always meets it with `checkCollision`'s ray, whatever the throw's random spread: the
ray starts at the bobber's position, the bottom of its box, so the box cannot reach the face first. A side face can be
reached by the box (0.125 wider than the ray) before the ray, on some throws, so this harness casts only downwards.
"""
import json
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'fishing-hook-blocks-parity'
locator.PROPERTIES = dict(locator.PROPERTIES, gamemode='survival')
# Alice stands at the world spawn (0.5 -60 0.5) looking along +z: the grass she stands on is y=-61.
FLOOR = 'fill -4 -61 1 4 -61 24 minecraft:target'
QUERY = 'cmd Alice scoreboard players get Alice hits'
SCRIPT = [
    ('setup', ['hooks on', 'chat on', 'cmd Alice give @s minecraft:fishing_rod', f'cmd Alice {FLOOR}',
               'cmd Alice scoreboard objectives add hits minecraft.custom:minecraft.target_hit']),
    ('before', [QUERY]),
    ('cast', ['use Alice 30']),
    ('landed', [QUERY]),
    ('reel', ['use Alice 30']),
    ('recast', ['use Alice 30']),
    ('relanded', [QUERY]),
]


def shown(lines):
    """Per query: whether any target hit is counted, and at the last whether the second cast added more. A landing
    counts a hit each time the bobber meets the target while it bounces to rest, and how often depends on the
    throw's random spread, which the two servers do not share: the counts themselves are not compared."""
    out, last = [], None
    for line in lines:
        parts = line.split(' ', 3)
        if len(parts) < 4 or parts[2] != 'chat' or 'commands.scoreboard.players.get' not in line:
            continue
        if 'commands.scoreboard.players.get.null' in line:
            value = 0
        else:
            value = json.loads(parts[3])['with'][1]
        verdict = 'none' if value == 0 else 'some'
        if parts[1] == 'step=relanded' and last is not None:
            verdict += ' more' if value > last else ' same'
        out.append(f'{parts[0]} {parts[1]} {verdict}')
        last = value
    return out


def check(vanilla, vanilla_all):
    if [line.split(' ', 2)[2] for line in vanilla] != ['none', 'some', 'some more']:
        return f'vanilla did not count a hit on each cast: {vanilla}'
    return None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, 'A fishing bobber cast onto a floor of target blocks on '
                               'a dedicated server: the target_hit statistic its landing awards.', step_seconds=3.0)
    print(f'fishing hook blocks: {len(lines)} lines identical')


if __name__ == '__main__':
    sys.exit(main())
