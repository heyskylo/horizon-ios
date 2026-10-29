#!/usr/bin/env python3
"""Compare toggle sprint/sneak (and toggle attack/use) with the pinned client:
the real Options ToggleKeyMappings driven by the real static KeyMapping
functions as keyboard presses, SDL key repeats and releases, mouse buttons,
screens opening and closing over the game (releaseAll and
restoreToggleStatesOnScreenClosed), respawning (resetToggleKeys) and toggle
options changing mid-sequence, event by event."""
import json
import random

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness


def op(rng, screen):
    kind = rng.choice(['key', 'key', 'key', 'key', 'button', 'button', 'open', 'close', 'option', 'option', 'respawn'])
    if kind == 'key':
        return ['key', rng.choice([0, 1]), rng.choice([1, 1, 0, 0, -1]), screen]
    if kind == 'button':
        return ['button', rng.choice([2, 3]), rng.random() < 0.5, screen]
    if kind == 'option':
        return ['option', rng.randrange(4), rng.random() < 0.6]
    return [kind]


def cases(rng):
    # Every short sequence of the main events for sneak with toggle off and on.
    events = [['key', 0, 1, False], ['key', 0, 0, False], ['key', 0, -1, False], ['key', 0, 1, True], ['key', 0, 0, True],
              ['open'], ['close'], ['respawn'], ['option', 0, True], ['option', 0, False]]
    for a in events:
        for b in events:
            for c in events:
                for toggled in (False, True):
                    yield dict(ops=[['option', 0, toggled], a, b, c, ['key', 0, 1, False], ['open'], ['close']])
    for _ in range(6000):
        screen = False
        ops = [['option', i, rng.random() < 0.5] for i in range(4)]
        for _ in range(rng.randrange(1, 60)):
            o = op(rng, screen)
            if o[0] == 'open':
                screen = True
            elif o[0] == 'close':
                screen = False
            elif o[0] in ('key', 'button'):
                o[3] = screen
            ops.append(o)
        yield dict(ops=ops)


def make_cases(build):
    path = build / 'cases.jsonl'
    count = 0
    with path.open('w') as out:
        for case in cases(random.Random(0x544f474c)):
            out.write(json.dumps(case, separators=(',', ':')) + '\n')
            count += 1
    return path, count


def main():
    scope = ('The real Options keyShift/keySprint/keyAttack/keyUse ToggleKeyMappings with their real toggle options and '
             'shouldRestore flags, driven through the real static KeyMapping.set/click (every mapping bound to the key), '
             'releaseAll, restoreToggleStatesOnScreenClosed and resetToggleKeys, dispatched as KeyboardHandler.keyPress '
             '(release always; press and SDL repeat only without a screen), MouseHandler.onButton (only without a screen), '
             'Gui.setScreen and LocalPlayer.respawn call them. Compares down state and pending clicks after every event. '
             'The screen-consumed keyPressed/keyReleased branches and KeyMapping.setAll (an InputQuirks platform quirk) are '
             'not exercised; KeyboardInput then reads isDown as the sprint and local input harnesses compare.')
    harness.differential(
        'rust-toggle-keys-parity', 'VanillaToggleKeysOracle', 'toggle_keys_probe', make_cases, scope,
        sources=[lab.ROOT / 'crates/horizon-client-input/src/keymapping.rs'])


if __name__ == '__main__':
    main()
