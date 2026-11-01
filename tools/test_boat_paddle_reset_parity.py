#!/usr/bin/env python3
"""Boat paddle metadata transitions through actual vanilla/Horizon servers.

Drive both paddles, stop one, stop both, resume one, then dismount without
sending a final paddle packet. Clients must receive the false transitions.
"""
try:
    from . import test_locator_bar_parity as net
except ImportError:
    import test_locator_bar_parity as net

SCRIPT = [('setup', ['boats on', 'cmd Alice gamerule spawn_mobs false'])]
EXPECTED = []
for kind in ['oak_boat', 'birch_chest_boat', 'bamboo_raft']:
    SCRIPT.append((kind + '-setup', [
        'cmd Alice kill @e[type=!player]',
        'cmd Alice fill -5 79 -5 5 79 5 minecraft:stone',
        'cmd Alice fill -5 80 -5 5 80 5 minecraft:water',
        'cmd Alice tp @s 0.5 81 0.5',
        f'cmd Alice summon minecraft:{kind} 0.5 81 0.5',
        f'cmd Alice ride @s mount @e[type=minecraft:{kind},limit=1]',
    ]))
    for step, action, changes in [
        ('both', 'paddle Alice true true', [(11, True), (12, True)]),
        ('left', 'paddle Alice true false', [(12, False)]),
        ('stop', 'paddle Alice false false', [(11, False)]),
        ('resume', 'paddle Alice false true', [(12, True)]),
        ('dismount', 'cmd Alice ride @s dismount', [(12, False)]),
    ]:
        label = f'{kind}-{step}'
        SCRIPT.append((label, [action]))
        EXPECTED += [f'{label} {index} {str(value).lower()}' for index, value in changes]


def shown(lines):
    return [f'{p[1].removeprefix("step=")} {p[3]} {p[4]}'
            for line in lines if len(p := line.split()) == 5 and p[2] == 'paddle'
            and not p[1].endswith('-setup')]


def check(native, all_lines):
    if native != EXPECTED:
        return f'vanilla did not exercise the expected transitions: {native!r}'
    return None


def main():
    lines = net.one_player('boat-paddle-reset-parity', SCRIPT, shown, check, __doc__, step_seconds=0.75)
    print(f'boat paddle resets: {len(lines)} metadata transitions match vanilla')


if __name__ == '__main__':
    main()
