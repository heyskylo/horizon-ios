#!/usr/bin/env python3
"""The packets a Horizon client sends, tick by tick and field by field, against a real vanilla 26.3 client's
for the same scripted play: an anticheat (Grim, Vulcan, Polar, Watchdog, Intave...) judges the serverbound
stream, also after ViaVersion translated it for an older server, so Horizon's must be vanilla's.

A vanilla 26.3 dedicated server (flat world, offline) runs behind tools/net_capture.py. Each client joins it in
turn as Steve and plays every scenario below: the harness resets the place and the player from the server's
console, then hands the client the scenario, which it plays tick by tick from its own tick loop (Horizon:
HORIZON_TICK_SCRIPT, game/tick_script.rs; vanilla: VanillaTickScript.java, the unchanged client with its
keys pressed through KeyMapping). Both send /wire-mark on the scenario's first tick. tools/wire_stream.py
cuts each scenario's stream into client ticks (ClientTickEnd ends one) and decodes the packets; every tick
must hold the same packets, in the same order, with the same fields (doubles and floats exactly). Packets
the server's timing decides (keep-alive, pong, chunk batch, teleport acceptance) are compared as a sequence
over the scenario, and must never fall between two packets of one tick.

  python3 tools/test_wire_stream_parity.py                      # every scenario
  python3 tools/test_wire_stream_parity.py --scenarios walk,bow --keep
  python3 tools/test_wire_stream_parity.py --clients horizon,horizon-legacy   # Legacy (1.8.9) combat feel

The second form plays the same scenarios with two Horizon clients, the second set to the Legacy combat feel with
every combat extra on (horizon-extras.txt: `LEGACY_EXTRAS`), and compares their streams the same way: the feel
changes what the player sees and hears, never what the client sends, and the 1.8 rules ("in worlds you host")
never reach a server that isn't the player's own.

Keyboard-only boat scripts explicitly use an uncaptured render mouse in vanilla too. Native boat
clampRotation can alternate one yaw bit on Entity.turn(0, 0), so comparing an uncaptured headless
client with a captured rendered client would depend on the latter's render-frame count. Mouse-action
scenarios retain captured input. This aligns capture state, without changing ticks or comparisons.

Horizon runs `--headless` (no window or GPU: the ticks are what it sends from); the vanilla client runs on the
virtual screen (`vscreen`) on Linux, from the logged-in session on macOS. Every game and server here runs in fresh temporary folders (HORIZON_HOME,
HORIZON_LAUNCHER_HOME, XDG_*), with no audio and no self-update. Needs .local (the vanilla jars and prepared
resources) and a built horizon-client and horizon-ctl (--bin DIR, default the parity target's fast build).
"""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402
import net_capture  # noqa: E402
import test_rust_parity as base  # noqa: E402
import wire_stream  # noqa: E402
from server_pack_e2e import Game  # noqa: E402
from test_dedicated_server_parity import Server, free_port  # noqa: E402

PLAYER = 'Steve'
BOAT_DRIVER = 'WireBoatDriver'
PROPERTIES = {
    'online-mode': 'false', 'white-list': 'false', 'level-seed': '12345', 'level-type': 'minecraft\\:flat',
    'generate-structures': 'false', 'view-distance': '4', 'simulation-distance': '4', 'spawn-protection': '0',
    'pause-when-empty-seconds': '-1', 'gamemode': 'survival', 'difficulty': 'easy', 'spawn-monsters': 'false',
    'spawn-animals': 'false', 'enable-command-block': 'false',
    # This isolated oracle server can run under -Xint on a loaded host.
    # Wall-clock watchdog stalls are not wire mismatches; scenario/settle
    # deadlines still fail a genuinely unresponsive run without accepting it.
    'max-tick-time': '-1',
}
# The flat world's grass is at y -61: the player stands at -60.
FLOOR = -61
RESET = [
    'gamerule spawn_mobs false', 'gamerule advance_time false', 'gamerule advance_weather false',
    f'fill -12 {FLOOR + 1} -12 12 {FLOOR + 12} 28 minecraft:air',
    f'fill -12 {FLOOR} -12 12 {FLOOR} 28 minecraft:grass_block',
    'kill @e[tag=wire_boat]',
    'kill @e[type=!minecraft:player]', f'clear {PLAYER}', f'effect clear {PLAYER}', f'gamemode survival {PLAYER}',
    f'effect give {PLAYER} minecraft:saturation 1 20 true', f'effect give {PLAYER} minecraft:instant_health 1 5 true',
    f'tp {PLAYER} 0.5 {FLOOR + 1} 0.5 0 0',
]
# A target that takes hits without moving: no AI, no knockback.
TARGET = ('summon minecraft:husk {x} {y} {z} {{NoAI:1b,Silent:1b,PersistenceRequired:1b,Health:1000f,'
          'attributes:[{{id:"minecraft:max_health",base:1000.0}},{{id:"minecraft:knockback_resistance",base:1.0}}]}}')
# Every scenario starts with a short step forward: it moves the player, which starts LocalPlayer.sendPosition's
# 20-tick reminder over on both clients alike, and selects the first slot.
PRELUDE = 10
# Client ticks between a reset's last teleport and the scenario: the player lands and stands still.
SETTLE = 40
# The Legacy client's horizon-extras.txt (crates/horizon-client-render/src/pvp.rs): the 1.8.9 feel, its
# indicator and sounds, and the 1.8 rules, which only a world the player hosts may use.
LEGACY_EXTRAS = 'combatFeel:legacy\ncombatIndicator:true\ncombatSounds:true\ncombatRules:true\n'
# The clients --clients may name: the first is the reference, the second must send what it sends.
CLIENTS = {'vanilla': 'vanilla', 'horizon': 'Horizon', 'horizon-legacy': 'Horizon Legacy'}


def taps(key, ticks):
    return {t: [f'key {key} tap'] for t in ticks}


def merge(*parts):
    out = {}
    for part in parts:
        for tick, lines in part.items():
            out.setdefault(tick, []).extend(lines)
    return out


def every(start, stop, step, *lines):
    return {t: list(lines) for t in range(start, stop, step)}


SCENARIOS = {
    'walk': dict(ticks=70, steps=merge(
        {0: ['key w down'], 20: ['look 30 0'], 30: ['look 30 15'], 40: ['key w up'], 50: ['look 0 0']})),
    'idle': dict(ticks=60, steps={}),
    'sprint_jump': dict(ticks=70, steps={
        0: ['key ctrl down', 'key w down'], 5: ['key space down'], 45: ['key space up'],
        50: ['key w up', 'key ctrl up']}),
    'strafe': dict(ticks=70, steps={
        0: ['key w down', 'key a down'], 12: ['key a up', 'key d down'], 24: ['key d up', 'key a down'],
        36: ['key a up', 'key ctrl down'], 48: ['key w up', 'key ctrl up', 'key s down'], 56: ['key s up']}),
    'w_tap': dict(ticks=70, steps=merge(
        {0: ['key ctrl down', 'key w down']},
        every(10, 50, 8, 'key w up'), every(12, 52, 8, 'key w down'),
        {55: ['key w up', 'key ctrl up']})),
    'sneak': dict(ticks=60, steps={
        0: ['key shift down'], 5: ['key w down'], 25: ['key shift up'], 30: ['key shift down'],
        40: ['key w up'], 45: ['key shift up']}),
    'hit_target': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.0), f'item replace entity {PLAYER} weapon.mainhand with '
               'minecraft:diamond_sword'],
        ticks=70, steps=merge({0: ['look 0 20']}, every(5, 65, 12, 'mouse left click'))),
    'sprint_combo': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=7.0), f'item replace entity {PLAYER} weapon.mainhand with '
               'minecraft:diamond_sword'],
        ticks=60, steps=merge(
            {0: ['look 0 10', 'key ctrl down', 'key w down']}, every(12, 52, 10, 'mouse left click'),
            every(14, 54, 10, 'key w up'), every(16, 56, 10, 'key w down'), {56: ['key w up', 'key ctrl up']})),
    'block_hit': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.0),
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:diamond_sword',
               f'item replace entity {PLAYER} weapon.offhand with minecraft:shield'],
        ticks=70, steps={0: ['look 0 20'], 5: ['mouse left click'], 8: ['mouse right down'], 20: ['mouse right up'],
                         22: ['mouse left click'], 25: ['mouse right down'], 30: ['mouse left click'],
                         40: ['mouse right up'], 50: ['mouse left click']}),
    'rod': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=6.0),
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:fishing_rod'],
        ticks=80, steps={0: ['look 0 -5'], 5: ['mouse right click'], 35: ['mouse right click'],
                         45: ['mouse right click'], 70: ['mouse right click']}),
    'bow': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:bow',
               f'give {PLAYER} minecraft:arrow 16'],
        ticks=80, steps={0: ['look 0 -10'], 5: ['mouse right down'], 30: ['mouse right up'],
                         40: ['mouse right down'], 45: ['mouse right up'], 55: ['mouse right down'],
                         75: ['mouse right up']}),
    'eat': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:golden_apple 4'],
        # Two bites of 10 ticks, a third of the 32 a golden apple takes: when a full one ends is the server's to
        # say (its entity event), and a loaded server that catches up runs its ticks in a burst.
        ticks=50, steps={5: ['mouse right down'], 15: ['mouse right up'], 25: ['mouse right down'],
                         35: ['mouse right up']}),
    'bridge': dict(
        setup=[f'fill -2 {FLOOR + 1} -2 2 {FLOOR + 2} 2 minecraft:stone', f'tp {PLAYER} 0.5 {FLOOR + 3} 2.2 180 78',
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:stone 64'],
        ticks=90, steps=merge({0: ['look 0 78', 'key shift down', 'key s down']}, every(4, 80, 4, 'mouse right click'),
                              {82: ['key s up', 'key shift up']})),
    'inventory': dict(
        setup=[f'item replace entity {PLAYER} hotbar.0 with minecraft:diamond_sword',
               f'item replace entity {PLAYER} hotbar.1 with minecraft:cooked_beef 16',
               f'item replace entity {PLAYER} inventory.0 with minecraft:golden_apple 8'],
        ticks=60, steps={5: ['key e tap'], 10: ['click-slot 36'], 14: ['click-slot 9'], 18: ['click-slot 36'],
                         22: ['click-slot 37 shift'], 26: ['click-slot 10 right'], 30: ['key e tap'],
                         40: ['key e tap'], 45: ['key esc tap']}),
    'hotbar_swap': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.0)] +
              [f'item replace entity {PLAYER} hotbar.{i} with minecraft:{item}' for i, item in
               enumerate(['diamond_sword', 'stone_axe', 'golden_apple', 'stone', 'bow'])],
        # 52-54: a swing at the sky with a new slot sends no slot before its Punch (no ensureHasSentCarriedItem
        # on a miss); the next tick's MultiPlayerGameMode.tick does.
        ticks=60, steps=merge({0: ['look 0 20']}, {t: [f'key {t % 5 + 1} tap'] for t in range(5, 15)},
                              {16: ['key 2 tap', 'mouse left click'], 20: ['key 1 tap'], 21: ['key 3 tap'],
                               30: ['key 5 tap', 'mouse left click'], 35: ['key q tap'], 40: ['key f tap'],
                               52: ['look 0 -80'], 54: ['key 3 tap', 'mouse left click']})),
    'dig_teleport': dict(
        setup=[f'setblock 0 {FLOOR + 1} 2 minecraft:stone'],
        ticks=60, anchor='accept_teleportation',
        steps={0: ['look 0 45'], 3: ['mouse left down'], 15: ['command tp @s ~ ~ ~'], 40: ['mouse left up']}),
    'boat_driver': dict(
        render_mouse=False,
        mount_setup=[f'summon minecraft:oak_boat 0.5 {FLOOR + 1} 0.5 {{Tags:["wire_boat"]}}',
                     f'ride {PLAYER} mount @e[tag=wire_boat,limit=1]'],
        ticks=50, steps={0: ['key w down', 'key ctrl down'], 12: ['look 30 10'],
                         25: ['key w up', 'key ctrl up']}),
    'boat_turning_driver': dict(
        render_mouse=False,
        mount_setup=[f'summon minecraft:oak_boat 0.5 {FLOOR + 1} 0.5 {{Tags:["wire_boat"]}}',
                     f'ride {PLAYER} mount @e[tag=wire_boat,limit=1]'],
        # A/D changes the boat's deltaRotation, while independent look
        # changes the rider. Release all movement to compare the coast too.
        ticks=70, steps={0: ['key w down', 'key a down'], 10: ['look 25 12'],
                         20: ['key a up', 'key d down'], 30: ['look -20 -8'],
                         40: ['key w up', 'key d up'], 55: ['look 15 0']}),
    'boat_second_passenger': dict(
        render_mouse=False,
        # A boat prioritizes players over animal passengers. Another actual
        # connected Player must occupy seat zero to exercise a guest's role.
        mount_setup=[f'tp {BOAT_DRIVER} 0.5 {FLOOR + 1} 0.5 0 0',
                     f'summon minecraft:oak_boat 0.5 {FLOOR + 1} 0.5 {{Tags:["wire_boat"]}}',
                     f'ride {BOAT_DRIVER} mount @e[tag=wire_boat,limit=1]',
                     f'ride {PLAYER} mount @e[tag=wire_boat,limit=1]'],
        ticks=50, steps={0: ['key w down', 'key ctrl down'], 12: ['look 30 10'],
                         25: ['key w up', 'key ctrl up']}),
}


def horizon_lines(lines):
    """A scenario's lines for horizon-ctl: menu slot ids become its inventory indices (hotbar 0-8, main 9-35)."""
    out = []
    for line in lines:
        w = line.split()
        if w[0] == 'click-slot':
            slot = int(w[1])
            w[1] = str(slot - 36 if 36 <= slot <= 44 else slot)
        out.append(' '.join(w))
    return out


def script(name, spec, horizon):
    steps = {0: ['key 1 tap', 'key w down'], 2: ['key w up']}
    for tick, lines in spec['steps'].items():
        steps.setdefault(tick + PRELUDE, []).extend(horizon_lines(lines) if horizon else lines)
    result = {'name': name, 'ticks': spec['ticks'] + PRELUDE,
              'steps': {str(k): v for k, v in sorted(steps.items())}}
    if 'render_mouse' in spec:
        if type(spec['render_mouse']) is not bool:
            raise ValueError('render_mouse must be a boolean')
        # A headless Horizon has no captured pointer. For keyboard-only
        # scenarios the rendered reference must not add idle per-frame turns:
        # native boat clamp math can toggle a yaw bit even with zero input.
        result['render_mouse'] = spec['render_mouse']
    return result


class Failed(Exception):
    pass


MESA_EGL = Path('/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json')


def screen_env(env):
    """Where the vanilla client's window goes. On Linux, the virtual screen (`vscreen`), never a real monitor,
    and Mesa's EGL: NVIDIA's crashed the client there (libEGL_nvidia). On macOS, the logged-in session."""
    if sys.platform == 'darwin' or not shutil.which('vscreen'):
        return env
    subprocess.run(['vscreen', 'up'], check=False, capture_output=True)
    text = subprocess.run(['vscreen', 'env'], capture_output=True, text=True, check=True).stdout
    env = dict(env)
    env.pop('DISPLAY', None)
    for match in re.finditer(r'(\w+)=(\S+)', text.split('unset')[0]):
        env[match.group(1)] = match.group(2).strip('"\'')
    if MESA_EGL.exists():
        env['__EGL_VENDOR_LIBRARY_FILENAMES'] = str(MESA_EGL)
    return env


class VanillaClient:
    """The vanilla 26.3 client, VanillaTickScript as its main class, joined to `address`."""

    def __init__(self, address, work, log):
        import vanilla_client
        classes = work / 'classes'
        classes.mkdir(parents=True)
        jars = os.pathsep.join(map(str, vanilla_client.classpath()))
        release = str(lab.config()[0]['minecraft']['java_major'])
        source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaTickScript.java'
        subprocess.run(['javac', '--release', release, '-proc:none', '-nowarn', '-cp', jars, '-d', classes, source],
                       check=True, capture_output=True)
        args = argparse.Namespace(name=PLAYER, join=address, world=None, game_dir=work / 'client', assets=None,
                                  heap='2G', width=854, height=480, lan_port=None, backend=None)
        args.game_dir.mkdir()
        # 20 frames a second at least: a tick's lines then act in their own frame, as Horizon's do.
        vanilla_client.write_options(args.game_dir, {'maxFps': '60'})
        command = vanilla_client.command(args)
        cp = command.index('-cp') + 1
        command[cp] = os.pathsep.join([str(classes), command[cp]])
        command[command.index('net.minecraft.client.main.Main')] = 'dev.horizonlab.parity.VanillaTickScript'
        self.log = open(log, 'w')
        self.ready, self.done = threading.Event(), {}
        # Its window opens without the focus (SDL's hint), so it takes neither the keyboard nor the pointer of
        # whoever is at the computer; VanillaTickScript keeps their input out once it runs.
        env = dict(screen_env(vanilla_client.environment()), SDL_WINDOW_ACTIVATE_WHEN_SHOWN='0')
        self.process = subprocess.Popen(command, cwd=args.game_dir, env=env, text=True,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        threading.Thread(target=self.pump, daemon=True).start()

    def pump(self):
        for line in self.process.stdout:
            self.log.write(line)
            self.log.flush()
            if '[VanillaTickScript] ready' in line:
                self.ready.set()
            elif '[VanillaTickScript] done ' in line:
                self.done[line.split('[VanillaTickScript] done ')[1].strip()] = time.time()

    def play(self, path, name, timeout):
        self.process.stdin.write(f'run {path}\n')
        self.process.stdin.flush()
        deadline = time.time() + timeout
        while name not in self.done:
            if time.time() > deadline or self.process.poll() is not None:
                raise Failed(f'vanilla: scenario {name} never finished (see vanilla-client.log)')
            time.sleep(0.2)

    def close(self):
        self.process.terminate()
        try:
            self.process.wait(20)
        except subprocess.TimeoutExpired:
            self.process.kill()
        self.log.close()


class HeadlessGame(Game):
    """server_pack_e2e's game, `--headless`: no display, window or GPU; the frame and the ticks run as a
    window's do (game/headless.rs), and only the ticks' packets matter here."""

    def __init__(self, bins, resources, work, scripts, extras=None):
        self.home = work / 'home'
        for d in ('home', 'launcher', 'data', 'config'):
            (work / d).mkdir(parents=True)
        if extras:
            # The game folder is the home here (Game runs horizon-ctl there): extras sit next to options.txt.
            (self.home / 'horizon-extras.txt').write_text(extras)
        self.env = lab.game_env(HORIZON_HOME=self.home, HORIZON_LAUNCHER_HOME=work / 'launcher',
                                XDG_DATA_HOME=work / 'data', XDG_CONFIG_HOME=work / 'config',
                                HORIZON_TELEMETRY_URL='off', HORIZON_TICK_SCRIPT=scripts / 'scenario.json')
        self.base = [str(bins / 'horizon-ctl'), '--socket', str(work / 'c.sock')]
        self.log = self.home / 'logs' / 'latest.log'
        reply = self('launch', '--game', bins / 'horizon-client', '--headless',
                     '--game-dir', self.home, '--resources', resources)
        self.pid = reply.get('pid')


class HorizonClient:
    def __init__(self, bins, resources, work, address, scripts, extras=None):
        self.scripts = scripts
        self.game = HeadlessGame(bins, resources, work, scripts, extras)
        self.game('connect', address, PLAYER)
        self.game('wait-for', 'loaded', '--timeout', 180)

    def play(self, path, name, timeout):
        target = self.scripts / 'scenario.json'
        done = target.with_suffix('.done')
        done.unlink(missing_ok=True)
        shutil.copy(path, target)
        target.with_suffix('.go').touch()
        deadline = time.time() + timeout
        while not done.exists():
            if time.time() > deadline:
                raise Failed(f'Horizon: scenario {name} never finished (see horizon-client.log)')
            time.sleep(0.2)

    def close(self):
        self.game('disconnect', ok=False)
        self.game.quit()


class ClientCounts:
    """How many teleports the client has accepted, and how many ticks it has ended, read from its capture as it
    grows (complete lines only)."""

    def __init__(self, capture):
        self.capture, self.offset, self.accepted, self.ticks = capture, 0, 0, 0

    def update(self):
        with open(self.capture, 'rb') as f:
            f.seek(self.offset)
            data = f.read()
        data = data[:data.rfind(b'\n') + 1]
        self.offset += len(data)
        for line in data.splitlines():
            if b'"c2s"' in line:
                e = json.loads(line)
                self.accepted += e.get('name') == 'accept_teleportation'
                self.ticks += e.get('name') == 'client_tick_end'
        return self


def settle(counts, accepted, timeout=120):
    """Wait until the client has accepted `accepted` teleports in all, then for SETTLE more of its ticks. The
    last command of a reset is a teleport, so the server has run them all by then; a busy server can take
    seconds to, and a teleport landing in a scenario's first ticks would change all of its moves."""
    deadline = time.time() + timeout
    while counts.update().accepted < accepted:
        if time.time() > deadline:
            raise Failed(f'{counts.capture.stem}: the client never accepted the reset\'s teleports')
        time.sleep(0.2)
    ticks = counts.ticks
    while counts.update().ticks < ticks + SETTLE:
        if time.time() > deadline:
            raise Failed(f'{counts.capture.stem}: the client stopped ticking')
        time.sleep(0.2)


def play_all(client, server, scenarios, scripts, horizon, capture, driver_present=False):
    counts = ClientCounts(capture)
    for name in scenarios:
        spec = SCENARIOS[name]
        # The place settles: blocks and items reach the client, the player stands still on the ground.
        # From the console `~` is the world spawn: the player teleports onto itself.
        # Teleporting can dismount a rider: the boat setup runs after the
        # reset's final teleport, and its real passenger packets are verified.
        # An idle fixture player is still a colliding entity. Remove it from
        # the measured area before every reset, including the next client's
        # driver scenario after the previous client's passenger scenario.
        park_driver = ([f'ride {BOAT_DRIVER} dismount',
                        f'tp {BOAT_DRIVER} 1000.5 {FLOOR + 1} 1000.5 0 0'] if driver_present else [])
        commands = park_driver + RESET + spec.get('setup', []) + [f'tp {PLAYER} {PLAYER}'] + spec.get('mount_setup', [])
        accepted = counts.update().accepted + sum(c.startswith(f'tp {PLAYER} ') for c in commands)
        for command in commands:
            server.console(command)
        settle(counts, accepted)
        path = scripts / f'{name}-{"horizon" if horizon else "vanilla"}.json'
        path.write_text(json.dumps(script(name, spec, horizon)))
        client.play(path, name, timeout=60)
        time.sleep(0.5)


def run(args, out):
    lock, jars, libraries = base.verify_artifacts()
    deps = os.pathsep.join(map(str, [jars['server'], *libraries]))
    vanilla = ['java', '-Xmx1g', '-cp', deps, 'net.minecraft.server.Main', 'nogui']
    resources = lab.LOCAL / 'horizon-client' / lab.VERSION
    work = Path(tempfile.mkdtemp(prefix='hz-wire-stream-'))
    print(f'scratch: {work}')
    for var, sub in (('HORIZON_HOME', 'home'), ('HORIZON_LAUNCHER_HOME', 'launcher'), ('XDG_DATA_HOME', 'data'),
                     ('XDG_CONFIG_HOME', 'config')):
        (work / 'env' / sub).mkdir(parents=True)
        os.environ[var] = str(work / 'env' / sub)
    os.environ.update(lab.game_env(HORIZON_TELEMETRY_URL='off'))
    directory = work / 'server'
    directory.mkdir()
    port = free_port()
    props = dict(PROPERTIES, **{'server-port': str(port), 'server-ip': '127.0.0.1'})
    (directory / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()),
                                                 encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    (directory / 'ops.json').write_text(json.dumps([{'uuid': str(lab.offline_uuid(PLAYER)), 'name': PLAYER,
                                                     'level': 4, 'bypassesPlayerLimit': False}]))
    scripts = work / 'scripts'
    scripts.mkdir()
    server = Server(vanilla, directory, 'server')
    driver = None
    captures = {}
    try:
        server.wait_for(r'Done \(', timeout=900)
        # Legacy spawn-monsters/spawn-animals properties do not prevent
        # 26.3 flat-world slime spawning. Set the native gamerule before any
        # player loads chunks, rather than waiting for the first RESET.
        server.console('gamerule spawn_mobs false')
        server.wait_for(r'Game rule spawn_mobs.*false', timeout=30)
        if 'boat_second_passenger' in args.scenarios:
            driver_scripts = work / 'driver-scripts'
            driver_scripts.mkdir()
            # A separate idle player joins the isolated offline server
            # directly, outside either tested client's capture proxy.
            driver = HeadlessGame(Path(args.bin).resolve(), resources, work / 'boat-driver', driver_scripts)
            driver('connect', f'127.0.0.1:{port}', BOAT_DRIVER)
            driver('wait-for', 'loaded', '--timeout', 180)
        for who in args.clients:
            capture = out / f'{who}.jsonl'
            capture.touch()
            listen = free_port()
            stop = net_capture.start_background(listen, f'127.0.0.1:{port}', capture, full=True)
            client = None
            try:
                if who != 'vanilla':
                    client = HorizonClient(Path(args.bin).resolve(), resources, work / who, f'127.0.0.1:{listen}',
                                           scripts, LEGACY_EXTRAS if who == 'horizon-legacy' else None)
                else:
                    client = VanillaClient(f'127.0.0.1:{listen}', work / 'vanilla', out / 'vanilla-client.log')
                    if not client.ready.wait(900):
                        raise Failed('the vanilla client never got into the world')
                time.sleep(3)
                play_all(client, server, args.scenarios, scripts, who != 'vanilla', capture, driver is not None)
            finally:
                if client:
                    if who != 'vanilla' and client.game.log.exists():
                        shutil.copy(client.game.log, out / f'{who}-client.log')
                    client.close()
                time.sleep(2)
                stop.set()
            captures[who] = capture
    finally:
        try:
            if driver:
                try:
                    if driver.log.exists():
                        shutil.copy(driver.log, out / 'boat-driver-client.log')
                    driver('disconnect', ok=False)
                finally:
                    driver.quit()
        finally:
            server.stop()
            shutil.copy(server.log_path, out / server.log_path.name)
            if not args.keep:
                shutil.rmtree(work, ignore_errors=True)
    return captures


def anchored_diffs(v, h, ticks, anchor, problems, names=('vanilla', 'Horizon')):
    """wire_stream.compare over the scenario's ticks. With an anchor (a frame packet the server's answer brings,
    as AcceptTeleportation), the two ticks that answer arrived in compare with each other
    (wire_stream.compare_answered). Numbers are vanilla's ticks."""
    if not anchor:
        return wire_stream.compare(v, h, ticks)
    av, ah = wire_stream.first_frame(v, anchor), wire_stream.first_frame(h, anchor)
    if av is None or ah is None:
        problems.append(f'  no {anchor}: {names[0]} at tick {av}, {names[1]} at tick {ah}')
        return wire_stream.compare(v, h, ticks)
    return wire_stream.compare_answered(v, h, ticks, av, ah)


def passenger_seats_at_marks(entries):
    """Observe the server's actual passenger order at each script marker.

    A successful /ride command alone proves neither receipt nor seat index.
    Requiring the actual SetPassengers packet prevents a dismounted fixture
    from passing the second-passenger no-control assertion vacuously.
    """
    me = wire_stream.self_id(entries)
    passengers, seats = {}, []
    for entry in entries:
        name = entry.get('name')
        if entry.get('state') != 'play':
            continue
        if entry.get('dir') == 's2c':
            reader = wire_stream.Reader(bytes.fromhex(entry.get('data', '')))
            if name == 'set_passengers':
                vehicle, count = reader.varint(), reader.varint()
                passengers[vehicle] = [reader.varint() for _ in range(count)]
            elif name == 'remove_entities':
                for _ in range(reader.varint()):
                    passengers.pop(reader.varint(), None)
            elif name in ('login', 'respawn'):
                passengers.clear()
        elif entry.get('dir') == 'c2s' and name == 'chat_command':
            fields = wire_stream.decode(name, bytes.fromhex(entry.get('data', '')))
            if fields.get('command') == 'wire-mark':
                seats.append([(order.index(me), len(order)) for order in passengers.values() if me in order])
    return seats


def compare(captures, scenarios):
    """Each scenario's problems, the second client's stream against the first's (`captures` in that order)."""
    (ref, ref_name), (new, new_name) = [(who, CLIENTS[who]) for who in captures]
    runs = {}
    seats = {}
    for who, capture in captures.items():
        entries = [e for e in net_capture.load(capture) if e.get('conn') == 1 and 'name' in e]
        found = wire_stream.scenarios(entries)
        if len(found) != len(scenarios):
            raise Failed(f'{who}: {len(found)} marked scenarios in the capture, expected {len(scenarios)}')
        runs[who] = dict(zip(scenarios, found))
        seats[who] = dict(zip(scenarios, passenger_seats_at_marks(entries)))
    results = []
    for name in scenarios:
        v, h = runs[ref][name], runs[new][name]
        ticks = SCENARIOS[name]['ticks'] + PRELUDE
        problems = []
        if name in ('boat_driver', 'boat_turning_driver', 'boat_second_passenger'):
            driving = name != 'boat_second_passenger'
            expected_seat = (0, 1) if driving else (1, 2)
            for who, run in ((ref, v), (new, h)):
                actual_seats = seats[who].get(name)
                if actual_seats != [expected_seat]:
                    problems.append(f'  {who}: fixture passenger seat {actual_seats}, expected {[expected_seat]}')
            if problems:
                # An invalid role is setup failure, never gameplay evidence.
                results.append((name, problems))
                continue
            for who, run in ((ref, v), (new, h)):
                sent = [packet for tick in run[:ticks + 1] for packet, _ in tick['packets']]
                for packet in ('paddle_boat', 'move_vehicle'):
                    present = packet in sent
                    if present != driving:
                        problems.append(f'  {who}: {packet} present={present}, contradicts the server passenger order')
        for i, vt, ht in anchored_diffs(v, h, ticks, SCENARIOS[name].get('anchor'), problems, (ref_name, new_name)):
            problems.append(f'  tick {i}:\n  {ref_name}:\n{wire_stream.describe(vt)}\n  {new_name}:\n'
                            f'{wire_stream.describe(ht)}')
            problems += wire_stream.field_diff(vt, ht, (ref_name, new_name))
        # The server decides when keep-alives and chunk batches come; the others answer what the scenario did.
        frame = lambda run: [n for t in run[:ticks + 1] for n, _ in t['frame']  # noqa: E731
                             if n not in ('keep_alive', 'chunk_batch_received')]
        if frame(v) != frame(h):
            problems.append(f'  frame packets: {ref_name} {frame(v)}, {new_name} {frame(h)}')
        misplaced = [(i, t['misplaced']) for i, t in enumerate(h[:ticks + 1]) if t['misplaced']]
        if misplaced:
            problems.append(f'  {new_name} sent the server\'s answers inside a tick: {misplaced}')
        results.append((name, problems))
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin', default=str(lab.cargo_target() / 'fast'), help='horizon-client and horizon-ctl')
    parser.add_argument('--out', default=str(lab.LOCAL / 'wire-stream-parity'))
    parser.add_argument('--scenarios', default=','.join(SCENARIOS), type=lambda s: s.split(','))
    parser.add_argument('--clients', default='vanilla,horizon', type=lambda s: s.split(','),
                        help='the reference client and the one compared with it, of vanilla, horizon and '
                             'horizon-legacy; one alone is only captured (a loaded machine whose vanilla client fails '
                             'to log in: rerun horizon alone, copy an earlier vanilla.jsonl into --out, then '
                             '--compare-only)')
    parser.add_argument('--compare-only', action='store_true', help='compare the captures already in --out')
    parser.add_argument('--keep', action='store_true', help='keep the scratch folders')
    args = parser.parse_args()
    unknown = [s for s in args.scenarios if s not in SCENARIOS]
    if unknown:
        parser.error(f'unknown scenarios {unknown}; known: {", ".join(SCENARIOS)}')
    if len(args.clients) not in (1, 2) or len(set(args.clients)) != len(args.clients) \
            or not set(args.clients) <= set(CLIENTS):
        parser.error(f'--clients takes one or two of {", ".join(CLIENTS)}')
    if args.compare_only and len(args.clients) != 2:
        parser.error('--compare-only compares two clients: name both in --clients')
    out = Path(args.out)
    if args.compare_only:
        captures = {who: out / f'{who}.jsonl' for who in args.clients}
    else:
        shutil.rmtree(out, ignore_errors=True)
        out.mkdir(parents=True)
        try:
            captures = run(args, out)
        except Failed as e:
            sys.exit(f'FAIL: {e}')
        if len(captures) < 2:
            print(f'captured {", ".join(captures)} in {out}; nothing to compare it with')
            return
    try:
        results = compare(captures, args.scenarios)
    except Failed as e:
        sys.exit(f'FAIL: {e}')
    print(f'\nserverbound play streams, {CLIENTS[args.clients[0]]} vs {CLIENTS[args.clients[1]]}; captures in {out}')
    for name, problems in results:
        print(f'  {"pass" if not problems else "FAIL"}  {name}')
        for p in problems:
            print(p)
    if any(p for _, p in results):
        sys.exit(1)


if __name__ == '__main__':
    main()
