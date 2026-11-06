#!/usr/bin/env python3
"""A spear's Lunge on the ground and in a jump, on a dedicated server, against vanilla's, seen by a vanilla client.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator in survival, holding a spear with Lunge III. LocatorBarClient (a
player made of vanilla's protocol code) jabs (ServerboundPlayerActionPacket STAB) standing, in the tick of a jump,
and a few ticks into one, sending one position a tick as LocalPlayer does. `ApplyEntityImpulse.apply` answers with a
ClientboundSetEntityMotionPacket, which replaces the client's velocity: the motions Alice is sent are compared. The
y keeps the jump because handlePlayerPositionChange makes the server's copy jump too (`jumpFromGround`).
"""
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'lunge-motion-parity'
# server_dir and generate_world write these server.properties.
locator.PROPERTIES = dict(locator.PROPERTIES, gamemode='survival', difficulty='peaceful')

# Exercise the lowest and an intermediate tier/level as well as the original maximum.
# The extra tiers jab standing: repeating every jump phase drifts Alice far enough
# from the spawn that vanilla's own real-time steps occasionally miss the jab, and
# the original maximum keeps all four phases (the same distance as before).
PHASES = [('ground', ['stab Alice']), ('same-tick', ['arc Alice 0']), ('rising', ['arc Alice 3']),
          ('falling', ['arc Alice 8'])]
GROUND = [PHASES[0]]
SPEARS = [('wooden_spear', 1, GROUND), ('iron_spear', 2, GROUND), ('netherite_spear', 3, PHASES)]


def script():
    # Peaceful: the flat world's slimes would otherwise hit Alice while the
    # script is idle before the first command (and kill her), adding motions.
    steps = [('setup', ['arc Alice -1'])]
    for spear, level, phases in SPEARS:
        steps.append(('setup', ['cmd Alice clear @s',
                                f'cmd Alice give @s minecraft:{spear}[enchantments={{lunge:{level}}}]']))
        for phase, actions in phases:
            steps.append((f'{spear}-{level}-{phase}', list(actions)))
    # jumpFromGround gives a sprinting server copy 0.2 forward too.
    steps.append(('sprinting', ['sprint Alice', 'arc Alice 3']))
    return steps


SCRIPT = script()


def shown(lines):
    """The motions Alice is sent from the first jab on."""
    return [line for line in lines if len(parts := line.split(' ', 3)) >= 3 and parts[1] != 'step=setup'
            and parts[2] == 'motion']


def check(vanilla, vanilla_all):
    steps = {line.split()[1] for line in vanilla}
    missing = {f'step={step}' for step, _ in SCRIPT if step != 'setup'} - steps
    return f'vanilla sent no motion in {sorted(missing)}' if missing else None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, "A spear's Lunge standing and at three points of a jump "
                               'on a dedicated server: motion for wooden Lunge I, iron Lunge II and netherite Lunge III.', step_seconds=2.0)
    print(f'lunge motion: {len(lines)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
