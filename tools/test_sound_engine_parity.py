#!/usr/bin/env python3
"""Compare the sound engine's bookkeeping with the vanilla 26.3 client.

The oracle runs the real `SoundManager`/`SoundEngine` on OpenAL Soft's "No
Output" device (see VanillaSoundRig) and replays scripts of plays (simple,
biome loop, underwater loop, sub-sound, minecart and riding-minecart
instances), stops (by instance, by sound and/or source, all), category gains,
source volume options and refreshes, channel ends, delayed and queued sounds,
pauses, minecart states and client ticks.
After every step it prints each instance's `isActive` and the AL source the
engine configured (state, gain, pitch, position, relative, looping, distance
model, max distance, rolloff, reference distance), the subtitle notifications
and the channel pool counts. Horizon's `SoundEngine` replays the scripts
against a backend that records the same AL source fields.
"""
import json
import math
import random

try:
    from . import audio_harness as h, lab
except ImportError:
    import audio_harness as h
    import lab


class Script:
    def __init__(self, events, streams):
        self.lines = []
        self.events, self.streams = events, streams

    def add(self, *parts):
        self.lines.append(' '.join(map(str, parts)))

    def play(self, name, event, source=4, volume=1.0, pitch=1.0, at=(0.0, 0.0, 0.0), looping=0, delay=0, linear=True, relative=0, seed=0, kind='simple', target=None):
        self.add('play', name, kind, event, source, h.bits(volume), h.bits(pitch), *map(lab.dbits, at), looping, delay,
                 'linear' if linear else 'none', relative, seed)
        if target is not None:
            self.lines[-1] += ' ' + target

    def cart(self, name, at, delta, removed=0, silent=None, new=False):
        self.add('cartnew' if new else 'cart', name, *map(lab.dbits, at), *map(lab.dbits, delta), removed)
        if silent is not None:
            self.lines[-1] += ' ' + str(silent)

    def tick(self, n=1, paused=0):
        for _ in range(n):
            self.add('tick', paused)

    def reset(self):
        self.add('stopmatch', '-', '-')
        for source in range(11):
            self.add('volume', source, h.bits(1.0))
        self.tick()


def entity_bound_cases(s):
    event = 'minecraft:block.stone.break'
    for seed in [42, -1, 9223372036854775807]:
        s.cart('bound_target', (.123456789, 64.123456789, -3.123456789), (0, 0, 0), new=True)
        s.play('bound', event, 6, .7, 1.2, seed=seed, kind='bound', target='bound_target')
        s.tick()
        s.cart('bound_target', (3.248456789, 65.25, -3.623456789), (0, 0, 0))
        s.tick(2)
        s.add('pause')
        s.cart('bound_target', (30000000.125, 65.3, -30000000.125), (0, 0, 0))
        s.tick(paused=1)
        s.add('resume')
        s.tick()
        s.add('freeze', 1)
        s.cart('bound_target', (.3, 63.7, -2.8), (0, 0, 0))
        s.tick()
        s.add('freeze', 0)
        s.cart('bound_target', (1, 2, 3), (0, 0, 0), silent=1)
        s.tick()
        s.cart('bound_target', (2, 3, 4), (0, 0, 0), silent=0)
        s.add('replay', 'bound')
        s.tick()
        s.cart('bound_target', (9, 9, 9), (0, 0, 0), removed=1)
        s.cart('bound_target', (100, 100, 100), (0, 0, 0), new=True)
        s.tick()
        s.play('replacement', event, 6, .7, 1.2, seed=seed, kind='bound', target='bound_target')
        s.tick()
        s.add('stopmatch', '-', '-')
        s.tick(25)
        s.add('forget', 'bound')
        s.add('forget', 'replacement')
    s.cart('bound_target', (1, 2, 3), (0, 0, 0), silent=1, new=True)
    s.play('initialsilent', event, 6, kind='bound', target='bound_target')
    s.tick()
    s.cart('bound_target', (2, 3, 4), (0, 0, 0), silent=0)
    s.add('replay', 'initialsilent')
    s.tick()
    s.add('stopmatch', '-', '-')
    s.tick(25)
    s.add('forget', 'initialsilent')


def scripted(s):
    stone, cave, click = 'minecraft:block.stone.break', 'minecraft:ambient.cave', 'minecraft:ui.button.click'
    music, loop, uloop = 'minecraft:music.game', 'minecraft:ambient.crimson_forest.loop', 'minecraft:ambient.underwater.loop'
    # Positional and relative sounds, lifetimes and channel ends.
    s.play('a', stone, 4, 1.0, 0.8, (10.5, 64.0, -3.25), seed=5)
    s.play('b', click, 10, 0.25, 1.0, linear=False, relative=1, seed=6)
    s.play('c', cave, 8, 4.0, 1.0, (100.0, 20.0, 100.0), seed=7)
    s.play('d', stone, 4, 1.5, 3.0, (0.5, 0.5, 0.5), seed=8)
    s.play('e', stone, 4, 0.5, 0.1, (1.0, 2.0, 3.0), seed=9)
    s.tick(3)
    s.add('finish', 'a')
    s.tick(25)
    s.add('finish', 'b')
    s.add('stop', 'c')
    s.tick(2)
    s.reset()
    # Volumes: options, category gains, refreshes, silent starts.
    s.add('volume', 4, h.bits(0.5))
    s.add('volume', 0, h.bits(0.75))
    s.play('a', stone, 4, 1.0, 1.0, (1.0, 1.0, 1.0), seed=1)
    s.add('volume', 4, h.bits(0.0))
    s.play('b', stone, 4, 1.0, 1.0, (1.0, 1.0, 1.0), seed=2)
    s.add('volume', 1, h.bits(0.0))
    s.play('m', music, 1, 1.0, 1.0, linear=False, relative=1, seed=3)
    s.add('refresh', 4)
    s.add('volume', 4, h.bits(0.3))
    s.add('refresh', 0)
    s.add('gain', 1, h.bits(0.5))
    s.add('gain', 1, h.bits(1.5))
    s.add('gain', 4, h.bits(-0.2))
    s.add('volume', 1, h.bits(1.0))
    s.add('refresh', 1)
    s.tick(2)
    s.add('volume', 0, h.bits(2.0))
    s.add('refresh', 0)
    s.reset()
    # Biome loops: fades, crossfade, stopping, removal.
    s.play('l1', loop, kind='loop', seed=11)
    s.add('fade', 'l1', 'in')
    s.tick(45)
    s.play('l2', 'minecraft:ambient.warped_forest.loop', kind='loop', seed=12)
    s.add('fade', 'l1', 'out')
    s.add('fade', 'l2', 'in')
    s.tick(50)
    s.add('fade', 'l2', 'out')
    s.tick(10)
    s.add('fade', 'l2', 'in')
    s.tick(60)
    s.add('volume', 8, h.bits(0.0))
    s.play('l3', loop, kind='loop', seed=13)
    s.tick(3)
    s.reset()
    # Underwater loop and sub-sounds.
    s.add('player', 1, 0)
    s.play('u', uloop, kind='uloop', seed=21)
    s.play('v', 'minecraft:ambient.underwater.loop.additions.rare', kind='usub', seed=22)
    s.tick(50)
    s.add('player', 0, 0)
    s.tick(5)
    s.add('player', 1, 0)
    s.tick(5)
    s.add('player', 0, 0)
    s.tick(30)
    s.add('player', 1, 1)
    s.play('u2', uloop, kind='uloop', seed=23)
    s.tick(3)
    s.add('player', 0, 0)
    s.reset()
    # Minecarts: the rolling sound and the two riding sounds, by speed,
    # underwater, frozen ticks, getting out and the cart's removal.
    s.cart('k', (10.3, 64.0, 5.7), (0.0, 0.0, 0.0))
    s.play('m', 'k', kind='cart', seed=61)
    s.tick(2)
    for i, v in enumerate([0.005, 0.0099, 0.01, 0.1, 0.3, 0.5, 0.51, 0.8, 1.0, 1.2, 2.0, 0.0]):
        s.cart('k', (10.3 + i * 0.37123, 64.0625, 5.7 - i * 0.1), (v * 0.6, -0.04, v * 0.8))
        s.tick(2)
        if i == 4:
            s.add('freeze', 1)
            s.tick(3)
            s.add('freeze', 0)
            s.add('ride', 'k')
            s.play('r', 'k', kind='rcartu', seed=62)
            s.play('i', 'k', kind='rcart', seed=63)
        if i == 8:
            s.add('player', 1, 0)
            s.tick(3)
            s.add('player', 0, 0)
    s.add('ride', '-')
    s.tick(3)
    s.cart('k', (1.0, 2.0, 3.0), (0.2, 0.0, 0.0), removed=1)
    s.tick(3)
    s.reset()
    # Queued tickables, delayed and manually looped sounds.
    s.play('q', loop, kind='loop', seed=31)
    s.add('stop', 'q')
    s.tick(25)
    s.add('queue', 'q')
    s.add('fade', 'q', 'in')
    s.tick(3)
    s.play('w', stone, 4, 1.0, 1.0, (2.0, 2.0, 2.0), seed=32)
    s.add('delayed', 'w', 5)
    s.tick(7)
    s.play('r', stone, 4, 1.0, 1.0, (3.0, 3.0, 3.0), looping=1, delay=4, seed=33)
    s.tick(22)
    s.add('finish', 'r')
    s.tick(8)
    s.add('finish', 'r')
    s.tick(6)
    s.play('n', stone, 4, 1.0, 1.0, (3.0, 3.0, 3.0), looping=1, seed=34)
    s.tick(2)
    s.reset()
    # Stopping by source and sound, pauses.
    s.play('a', stone, 4, seed=41)
    s.play('b', stone, 5, seed=42)
    s.play('c', cave, 8, seed=43)
    s.play('m', music, 1, linear=False, relative=1, seed=44)
    s.play('u', click, 10, linear=False, relative=1, seed=45)
    s.add('pause')
    s.tick(1, paused=1)
    s.add('stop', 'm')
    s.tick(1, paused=1)
    s.add('resume')
    s.add('stopmatch', stone, '-')
    s.tick(25)
    s.add('stopmatch', '-', 8)
    s.add('stopmatch', click, 4)
    s.add('stopmatch', click, 10)
    s.tick(2)
    s.reset()
    # Unknown, empty and intentionally empty events; replaying an instance.
    for i, event in enumerate(['minecraft:not.a.sound', 'minecraft:intentionally_empty', 'minecraft:empty', 'minecraft:entity.generic.eat']):
        s.play(f'x{i}', event, 6, seed=50 + i)
    s.add('replay', 'x3')
    s.tick(25)
    s.reset()
    # Channel pools.
    for i in range(252):
        s.play('p', stone, 4, 1.0, 1.0, (float(i), 0.0, 0.0), seed=i)
    for i in range(10):
        s.play('s', music, 1, linear=False, relative=1, seed=i)
    s.tick(2)
    s.add('stopmatch', '-', '-')
    s.tick(1)


def fuzz(s, rng, steps):
    names = ['f0', 'f1', 'f2', 'f3', 'f4', 'f5']
    live = set()
    for cart in ('k0', 'k1'):
        s.cart(cart, (0.0, 64.0, 0.0), (0.0, 0.0, 0.0))
    s.add('ride', '-')
    s.add('freeze', 0)
    for _ in range(steps):
        op = rng.random()
        name = rng.choice(names)
        if op < 0.25:
            kind = rng.choice(['simple', 'simple', 'simple', 'loop', 'uloop', 'usub'])
            event = rng.choice(s.streams if kind != 'simple' and rng.random() < 0.5 else s.events)
            at = tuple(rng.choice([0.0, rng.uniform(-200, 200)]) for _ in range(3))
            s.play(name, event, rng.randrange(11), rng.choice([1.0, 0.5, 2.0, 0.0, 4.0, rng.uniform(0, 3)]), rng.choice([1.0, 0.5, 2.5, rng.uniform(0.2, 3)]),
                   at, int(rng.random() < 0.2), rng.choice([0, 0, 0, 3]), rng.random() < 0.7, int(rng.random() < 0.3), rng.getrandbits(63), kind)
            live.add(name)
        elif not live:
            s.tick()
        elif op < 0.32:
            s.add('finish', rng.choice(sorted(live)))
        elif op < 0.36:
            s.add('stop', rng.choice(sorted(live)))
        elif op < 0.40:
            s.add('fade', rng.choice(sorted(live)), rng.choice(['in', 'out']))
        elif op < 0.42:
            s.add('stopmatch', rng.choice(['-', rng.choice(s.events)]), rng.choice(['-', rng.randrange(11)]))
        elif op < 0.45:
            s.add('volume', rng.randrange(11), h.bits(rng.choice([0.0, 1.0, 0.5, rng.random()])))
        elif op < 0.48:
            s.add('refresh', rng.randrange(11))
        elif op < 0.51:
            s.add('gain', rng.randrange(11), h.bits(rng.choice([0.0, 1.0, 0.3, 1.2])))
        elif op < 0.53:
            s.add('player', int(rng.random() < 0.6), int(rng.random() < 0.05))
        elif op < 0.55:
            s.add('replay', rng.choice(sorted(live)))
        elif op < 0.56:
            s.add('pause')
            s.tick(rng.randrange(1, 4), paused=1)
            s.add('resume')
        elif op < 0.57:
            s.add('forget', name)
            live.discard(name)
        elif op < 0.62:
            speed = rng.choice([0.0, 0.005, 0.01, 0.3, 0.5, 1.0, rng.uniform(0, 1.5)])
            angle = rng.uniform(0, 6.3)
            at = (rng.uniform(-50, 50), rng.uniform(0, 80), rng.uniform(-50, 50))
            s.cart(rng.choice(['k0', 'k1']), at, (speed * math.cos(angle), rng.uniform(-0.1, 0.1), speed * math.sin(angle)), int(rng.random() < 0.08))
        elif op < 0.65:
            s.add('ride', rng.choice(['k0', 'k1', '-']))
        elif op < 0.66:
            s.add('freeze', int(rng.random() < 0.5))
        elif op < 0.70:
            s.play(name, rng.choice(['k0', 'k1']), kind=rng.choice(['cart', 'rcart', 'rcartu']), seed=rng.getrandbits(63))
            live.add(name)
        else:
            s.tick(rng.randrange(1, 6))


def fixtures():
    sounds = json.loads((h.RESOURCES / 'assets/minecraft/sounds.json').read_text())
    names = sorted(sounds)
    streams = sorted('minecraft:' + n for n in names if any(isinstance(e, dict) and e.get('stream') for e in sounds[n].get('sounds', [])))
    rng = random.Random(0x454e47494e45)
    events = sorted(set(['minecraft:' + rng.choice(names) for _ in range(60)] + ['minecraft:ambient.cave', 'minecraft:block.stone.break']))
    s = Script(events, streams)
    entity_bound_cases(s)
    scripted(s)
    for _ in range(40):
        s.reset()
        fuzz(s, rng, 150)
    return s.lines


def main():
    lines = fixtures()
    matched, report = h.run_harness(
        'sound-engine', 'VanillaSoundEngineOracle', 'sound_engine_fixture', ''.join(l + '\n' for l in lines),
        extra_sources=[h.lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaSoundRig.java'],
        java_args=[h.RESOURCES], rust_args=[h.RESOURCES],
        scope='Real SoundEngine (OpenAL Soft null device, inline executor, silent static buffers, never-ready streams): play results, '
              'AL source parameters set by play/tickInGameSound/refreshCategoryVolume (gain from calculateVolume, clamped pitch, linear '
              'attenuation distance, looping, position, relative), isActive, the 20-tick minimum lifetime, channel ends, stop by instance/sound/source/all, '
              'pauseAllExcept/resume, tickMusicWhenPaused, queued tickables, playDelayed and manual looping, LoopSoundInstance, '
              'UnderLiquidAmbientSoundInstance and UnderLiquidSubSound ticks, MinecartSoundInstance and RidingMinecartSoundInstance ticks (speed, '
              'riding, underwater, frozen ticks, removal), subtitle notifications and ranges, and the 247/8 channel pools. '
              'Not compared: decoded sample output, OpenAL panning/HRTF and distance gain inside the mixer, asset loading timing.')
    print(f'{matched:,} sound engine lines match the pinned client ({len(lines):,} steps): {report}', flush=True)


if __name__ == '__main__':
    main()
