#!/usr/bin/env python3
"""Wind charges meeting a wither, on a dedicated server, against vanilla's, seen by a vanilla client.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator in survival. Three summoned withers (NoAI, so none heals, moves or
shoots) each take one charge from above:

- at half health (`isPowered`), a `wind_charge`: `WitherBoss.hurtServer` refuses a direct `WindCharge`, and the charge
  is spent all the same;
- at half health, a `breeze_wind_charge`: not a `WindCharge` (both only extend `AbstractWindCharge`), so it hurts;
- above half health, a `wind_charge`: it hurts, which shows the charge does reach a wither.

Whether each wither kept its health is compared (vanilla's answer is the oracle). The health a hurt wither is left
with is not asked: only that it changed.
"""
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'wither-wind-charge-parity'
# server_dir and generate_world write these server.properties.
locator.PROPERTIES = dict(locator.PROPERTIES, gamemode='survival')


def wither(tag, x, z, health):
    return f'cmd Alice summon minecraft:wither {x} -50 {z} {{NoAI:1b,Health:{health}f,Tags:["{tag}"]}}'


def charge(kind, x, z):
    # Six blocks above the wither's feet, flying straight down: it hits within a few ticks.
    return f'cmd Alice summon minecraft:{kind} {x} -44 {z} {{Motion:[0.0,-1.0,0.0]}}'


SCRIPT = [
    ('setup', ['chat on', wither('armored', 12.5, 0.5, 100), wither('breeze', -11.5, 0.5, 100),
               wither('unarmored', 0.5, 12.5, 200)]),
    ('hit', [charge('wind_charge', 12.5, 0.5), charge('breeze_wind_charge', -11.5, 0.5),
             charge('wind_charge', 0.5, 12.5)]),
    ('asked', ['cmd Alice execute unless entity @e[type=minecraft:wind_charge]',
               'cmd Alice execute unless entity @e[type=minecraft:breeze_wind_charge]',
               'cmd Alice execute if entity @e[tag=armored,nbt={Health:100.0f}]',
               'cmd Alice execute if entity @e[tag=breeze]',
               'cmd Alice execute unless entity @e[tag=breeze,nbt={Health:100.0f}]',
               'cmd Alice execute if entity @e[tag=unarmored]',
               'cmd Alice execute unless entity @e[tag=unarmored,nbt={Health:200.0f}]']),
]
ANSWERS = 7


def shown(lines):
    """The answers about the charges and the withers."""
    out = []
    for line in lines:
        parts = line.split(' ', 3)
        if len(parts) >= 3 and parts[1] == 'step=asked' and parts[2] == 'chat' and 'conditional' in line:
            out.append(f'{parts[1]} {parts[3]}')
    return out


def check(vanilla, vanilla_all):
    if sum('conditional.pass' in line for line in vanilla) != ANSWERS:
        return 'a charge did not hit its wither, or a wither did not take it as WitherBoss.hurtServer says'
    return None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, 'Wind charges flying into summoned withers on a dedicated '
                               'server: a powered wither refuses a wind_charge, and takes a breeze_wind_charge as '
                               'one above half health takes a wind_charge.', step_seconds=1.0)
    print(f'wither wind charge: {len(lines)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
