#!/usr/bin/env python3
"""How command replies name a mob, a team and a score holder, on a dedicated server, against vanilla's, seen by a
vanilla client.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator. She summons a wolf and a pig named Bob, puts the pig on a red team,
reads its Health with `/data get entity`, kills the wolf, and sets scores for the pig and for herself.
- `/summon`, `/data` and `/kill` name the mob with `Entity.getDisplayName`: the team's formatting of `getName()`,
  with a `ShowEntity` hover (type, UUID, name) and the UUID string to insert.
- `/team` names the team with `PlayerTeam.getFormattedDisplayName`: its name on hover and as insertion.
- `/team join` and `/scoreboard players set` name score holders with `ScoreHolder.getFeedbackDisplayName`: the
  display name with the scoreboard name (an entity's UUID, a player's name) on hover instead.
The replies' JSON is compared exactly, except the UUIDs, which are random on each run: each is replaced by `U`, after
checking that a reply's hover names the same UUID it inserts.
"""
import json
import re
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'mob-display-name-parity'
SCRIPT = [
    ('setup', ['chat on']),
    ('summon', ['cmd Alice summon minecraft:wolf 2.5 -60 4.5 {NoAI:1b}']),
    ('named', ['cmd Alice summon minecraft:pig 4.5 -60 4.5 {NoAI:1b,CustomName:"Bob"}']),
    # /team names the team with PlayerTeam.getFormattedDisplayName and its member with
    # ScoreHolder.getFeedbackDisplayName (the display name, its scoreboard name shown on hover).
    ('team', ['cmd Alice team add reds', 'cmd Alice team modify reds color red',
              'cmd Alice team join reds @e[type=minecraft:pig]']),
    ('data', ['cmd Alice data get entity @e[type=minecraft:pig,limit=1] Health']),
    ('kill', ['cmd Alice kill @e[type=minecraft:wolf]']),
    # /scoreboard names score holders with getFeedbackDisplayName too: an entity's UUID, a player's name on hover.
    ('scores', ['cmd Alice scoreboard objectives add points dummy',
                'cmd Alice scoreboard players set @e[type=minecraft:pig] points 3',
                'cmd Alice scoreboard players set Alice points 1']),
]
INTS = re.compile(r'"uuid":\[(-?\d+),(-?\d+),(-?\d+),(-?\d+)\]')
TEXT = re.compile(r'"uuid":"([0-9a-f-]{36})"')
# A ShowText hover of a UUID string (getFeedbackDisplayName's scoreboard name of an entity).
VALUE = re.compile(r'"value":"([0-9a-f-]{36})"')
INSERTION = re.compile(r'"insertion":"([0-9a-f-]{36})"')


def uuid_text(words):
    value = 0
    for word in words:
        value = (value << 32) | (int(word) & 0xFFFFFFFF)
    h = f'{value:032x}'
    return f'{h[:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:]}'


def shown(lines):
    """Each reply after setup, with its UUIDs replaced, or why its hover and insertion disagree."""
    out = []
    for line in lines:
        parts = line.split(' ', 3)
        if len(parts) < 4 or parts[2] != 'chat' or parts[1].startswith('step=setup'):
            continue
        text = parts[3]
        hovers = ([uuid_text(m.groups()) for m in INTS.finditer(text)] + [m.group(1) for m in TEXT.finditer(text)]
                  + [m.group(1) for m in VALUE.finditer(text)])
        insertions = [m.group(1) for m in INSERTION.finditer(text)]
        if sorted(hovers) != sorted(insertions):
            out.append(f'{parts[0]} {parts[1]} hover {hovers} but insertion {insertions}')
            continue
        text = INTS.sub('"uuid":"U"', text)
        text = VALUE.sub('"value":"U"', INSERTION.sub('"insertion":"U"', TEXT.sub('"uuid":"U"', text)))
        out.append(f'{parts[0]} {parts[1]} {json.dumps(json.loads(text), sort_keys=True)}')
    return out


def check(vanilla, vanilla_all):
    steps = {line.split()[1] for line in vanilla}
    if not {'step=summon', 'step=named', 'step=team', 'step=data', 'step=kill', 'step=scores'} <= steps:
        return f'vanilla answered only {sorted(steps)}'
    if not any('show_entity' in line for line in vanilla):
        return 'no vanilla reply names a mob with a hover'
    return None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, 'Command replies naming a summoned, a named and a '
                               'team-coloured mob on a dedicated server: their hover and insertion.',
                               step_seconds=2.0)
    print(f'mob display name: {len(lines)} lines identical')


if __name__ == '__main__':
    sys.exit(main())
