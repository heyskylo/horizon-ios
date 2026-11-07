#!/usr/bin/env python3
"""A whole hand-over between two Horizon dedicated servers, over their real control sockets (#7484).

Two engines (`horizon-client --dedicated`) run with `[transfer]` on (`--transfers`) and Velocity forwarding
(`forwarding.secret`), as docs/server.md requires. This harness is the proxy: a small relay per engine answers
the engine's `velocity:player_info` request with a signed payload (docs/velocity-forwarding.md), then copies
bytes both ways, and the control requests are sent on each engine's control socket. The player is a
horizon-bots client; the hand-over's two connections are two bot processes with the same name, as a proxy
holds two connections for one client.

    python3 tools/server_transfer_e2e.py --bin-dir target/fast

Steps, in docs/server.md's order ("A hand-over, in order"), each checked:

1. A player joins server A through the relay (and one joining A directly is refused: forwarding is required).
   A's console gives it items and experience.
2. `player-expect` on B.
3. `player-export <uuid> freeze` on A: a record with a hand-over id. Asked again, the same bytes come back.
4. The second connection logs in to B: B holds it ("is held"), and announces nothing.
5. `player-import` on B with `"place": false`: the record is taken, the player stays held.
6. `player-confirm <uuid> <id>` on B: B's player file is on disk with the record's items when the reply comes,
   then the player is placed.
7. `player-release <uuid> <id> moved` on A: the player is out of A's world, without "left the game", and A
   writes no file for it, before or after it stops.

`transfer-status` is read on the way: after 2 (B expects one player), after 4 (B holds it, A has it placed and
frozen) and after 7 (the player is on B, A has nobody). B runs with `max-players=1`: a second player is refused
as full after 2 (the expectation holds the place) and after 7 (the player takes it).

Then the cancels: a hand-over from B back to A is aborted (`player-drop` on A, `player-release ... abort` on B)
and the player plays on on B; a late `moved` for it is refused.

The engines run in a fresh folder with fresh HORIZON_HOME and XDG folders (lab.game_env), on free ports, with no
window. Every request, reply (records shortened) and check goes to --out/steps.json, with both servers' logs.
Build first:
    cargo build --profile fast -p horizon-client --bin horizon-client
    cargo build --profile fast -p horizon-bots
"""
import argparse
import base64
import gzip
import hashlib
import hmac
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import uuid as uuidlib

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402
from server_bench import DONE, Server  # noqa: E402

EXE = '.exe' if os.name == 'nt' else ''
SECRET = b'server-transfer-e2e-secret'
NAME = 'Mover'
PLAYER = NAME + '0'  # horizon-bots names its bots PREFIX0, PREFIX1...
PROPERTIES = {
    'online-mode': 'false',
    'white-list': 'false',
    'level-type': 'minecraft\\:flat',
    'generate-structures': 'false',
    'gamemode': 'survival',
    'spawn-protection': '0',
    'view-distance': '3',
    'simulation-distance': '3',
}
CONTAINER_CLICK = 18  # ServerboundContainerClickPacket's id (horizon-protocol ids.rs)


def click_body(slot):
    """A pickup click on `slot` of the inventory menu (window 0, state 0, button 0), with no
    predicted slots and no predicted cursor: the server works the click out itself."""
    return '00' + '00' + f'{slot:04x}' + '00' + '00' + '00' + '00'


TRANSFERS = 'expect_seconds=20,max_expect_seconds=60,max_expected=8,max_freeze_seconds=30,max_hold_seconds=60'


class Failed(Exception):
    pass


# ---- the wire: what the relay reads of a login ---------------------------------------------------

def varint(n):
    out = b''
    n &= 0xFFFFFFFF
    while True:
        b, n = n & 0x7F, n >> 7
        out += bytes([b | 0x80]) if n else bytes([b])
        if not n:
            return out


def parse_varint(buf, i=0):
    n = shift = 0
    while True:
        b = buf[i]
        i += 1
        n |= (b & 0x7F) << shift
        if not b & 0x80:
            return n, i
        shift += 7


def string(s):
    b = s.encode()
    return varint(len(b)) + b


def parse_string(buf, i):
    n, i = parse_varint(buf, i)
    return buf[i:i + n].decode(), i + n


def recv_exact(sock, n):
    data = b''
    while len(data) < n:
        chunk = sock.recv(n - len(data))
        if not chunk:
            raise EOFError
        data += chunk
    return data


def recv_frame(sock):
    length = 0
    shift = 0
    while True:
        b = recv_exact(sock, 1)[0]
        length |= (b & 0x7F) << shift
        if not b & 0x80:
            break
        shift += 7
    return recv_exact(sock, length)


def send_frame(sock, body):
    sock.sendall(varint(len(body)) + body)


def offline_uuid(name):
    """UUID.nameUUIDFromBytes("OfflinePlayer:" + name), as an offline server and horizon-bots derive it."""
    md5 = bytearray(hashlib.md5(('OfflinePlayer:' + name).encode()).digest())
    md5[6] = (md5[6] & 0x0F) | 0x30
    md5[8] = (md5[8] & 0x3F) | 0x80
    return uuidlib.UUID(bytes=bytes(md5))


def forwarded(name, address='127.0.0.1'):
    """The `velocity:player_info` answer a proxy signs: version 1, the address, the profile, no properties."""
    data = varint(1) + string(address) + offline_uuid(name).bytes + string(name) + varint(0)
    return hmac.new(SECRET, data, hashlib.sha256).digest() + data


class Relay:
    """A stand-in for the proxy in front of one engine: it answers the engine's forwarding request for the
    client, then copies bytes both ways. Every connection is logged."""

    def __init__(self, engine_port, log):
        self.engine_port = engine_port
        self.log = log
        self.listener = socket.socket()
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(('127.0.0.1', 0))
        self.listener.listen(8)
        self.port = self.listener.getsockname()[1]
        self.sockets = []
        threading.Thread(target=self._accept, daemon=True).start()

    def _accept(self):
        while True:
            try:
                client, _ = self.listener.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(client,), daemon=True).start()

    def _serve(self, client):
        try:
            engine = socket.create_connection(('127.0.0.1', self.engine_port), timeout=30)
        except OSError as e:
            self.log(f'relay: no engine on {self.engine_port}: {e}')
            client.close()
            return
        self.sockets += [client, engine]
        try:
            handshake = recv_frame(client)
            send_frame(engine, handshake)
            _, i = parse_varint(handshake)  # packet id 0
            _, i = parse_varint(handshake, i)  # protocol
            _, i = parse_string(handshake, i)  # host
            next_state, _ = parse_varint(handshake, i + 2)
            if next_state != 2:
                self._pipe(client, engine)
                return
            login_start = recv_frame(client)
            send_frame(engine, login_start)
            _, i = parse_varint(login_start)
            name, _ = parse_string(login_start, i)
            request = recv_frame(engine)
            pid, i = parse_varint(request)
            transaction, i = parse_varint(request, i)
            channel, _ = parse_string(request, i) if pid == 4 else ('', i)
            if pid != 4 or channel != 'velocity:player_info':
                self.log(f'relay: the engine did not ask for forwarding (packet {pid})')
                send_frame(client, request)
            else:
                send_frame(engine, varint(2) + varint(transaction) + b'\x01' + forwarded(name))
                self.log(f'relay {self.port}: forwarded {name} to {self.engine_port}')
            self._pipe(client, engine)
        except (EOFError, OSError) as e:
            self.log(f'relay {self.port}: {type(e).__name__} {e}')
            client.close()
            engine.close()

    def _pipe(self, a, b):
        a.settimeout(None)
        b.settimeout(None)

        def copy(src, dst):
            try:
                while True:
                    chunk = src.recv(65536)
                    if not chunk:
                        break
                    dst.sendall(chunk)
            except OSError:
                pass
            for s in (src, dst):
                try:
                    s.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        threading.Thread(target=copy, args=(a, b), daemon=True).start()
        threading.Thread(target=copy, args=(b, a), daemon=True).start()

    def close(self):
        self.listener.close()
        for s in self.sockets:
            try:
                s.close()
            except OSError:
                pass


# ---- an engine and its control socket ------------------------------------------------------------

class Engine:
    def __init__(self, bins, resources, folder, out, label, **extra):
        self.label = label
        self.folder = folder
        folder.mkdir(parents=True)
        self.port = lab.free_port()
        props = dict(PROPERTIES, **extra, **{'server-port': str(self.port), 'motd': f'transfer e2e {label}'})
        (folder / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()))
        (folder / 'eula.txt').write_text('eula=true\n')
        (folder / 'forwarding.secret').write_bytes(SECRET + b'\n')
        # horizon-server makes the folder of the game's control endpoint; the game does not.
        self.endpoint = folder / '.horizon' / 'engine.json'
        self.endpoint.parent.mkdir()
        command = [bins / f'horizon-client{EXE}', '--resources', resources, '--control-socket', self.endpoint,
                   '--transfers', TRANSFERS, '--dedicated', '--nogui']
        self.server = Server(command, folder, out / f'server-{label}.log')
        self.control = None

    def ready(self):
        self.server.wait_for(DONE, 300)
        self.server.wait_for(r'Transfers on \(server\.toml \[transfer\]\)', 5)
        self.server.wait_for(r'Velocity player information forwarding is on', 5)
        info = json.loads(self.endpoint.read_text())
        sock = socket.create_connection(('127.0.0.1', info['port']), timeout=30)
        reader = sock.makefile('r', encoding='utf-8')
        sock.sendall(f'auth {info["token"]}\n'.encode())
        reply = reader.readline()
        if '"ok"' not in reply:
            raise Failed(f'{self.label}: the control socket refused the token: {reply.strip()}')
        self.control = (sock, reader)

    def ask(self, line):
        sock, reader = self.control
        sock.sendall((line + '\n').encode())
        return json.loads(reader.readline())

    def world_file(self, kind, player_uuid):
        """players/<kind>/<uuid>.<dat|json> of the world, as Horizon writes it."""
        ext = 'dat' if kind == 'data' else 'json'
        return self.folder / 'world' / 'players' / kind / f'{player_uuid}.{ext}'

    def stop(self):
        if self.control:
            try:
                self.control[0].close()
            except OSError:
                pass
        self.server.stop()


# ---- a player file -------------------------------------------------------------------------------

def read_nbt(data):
    """A named NBT root out of uncompressed bytes, as plain Python values."""
    pos = [0]

    def take(n):
        b = data[pos[0]:pos[0] + n]
        pos[0] += n
        return b

    def payload(tag):
        if tag == 1:
            return struct.unpack('>b', take(1))[0]
        if tag == 2:
            return struct.unpack('>h', take(2))[0]
        if tag == 3:
            return struct.unpack('>i', take(4))[0]
        if tag == 4:
            return struct.unpack('>q', take(8))[0]
        if tag == 5:
            return struct.unpack('>f', take(4))[0]
        if tag == 6:
            return struct.unpack('>d', take(8))[0]
        if tag == 7:
            n = struct.unpack('>i', take(4))[0]
            return take(n)
        if tag == 8:
            n = struct.unpack('>H', take(2))[0]
            return take(n).decode('utf-8', 'replace')
        if tag == 9:
            inner = take(1)[0]
            n = struct.unpack('>i', take(4))[0]
            return [payload(inner) for _ in range(n)]
        if tag == 10:
            out = {}
            while True:
                t = take(1)[0]
                if t == 0:
                    return out
                key = payload(8)
                out[key] = payload(t)
        if tag == 11:
            n = struct.unpack('>i', take(4))[0]
            return list(struct.unpack(f'>{n}i', take(4 * n)))
        if tag == 12:
            n = struct.unpack('>i', take(4))[0]
            return list(struct.unpack(f'>{n}q', take(8 * n)))
        raise ValueError(f'tag {tag}')

    root = take(1)[0]
    payload(8)
    return payload(root)


def items_in(player, item):
    return sum(s.get('count', 1) for s in player.get('Inventory', []) if s.get('id') == item)


# ---- the run -------------------------------------------------------------------------------------

class Steps:
    def __init__(self, out):
        self.out = out
        self.steps = []
        self.failed = []

    def note(self, step, ok, **what):
        entry = {'step': step, 'ok': ok, **what}
        self.steps.append(entry)
        print(('ok    ' if ok else 'FAIL  ') + step + ('' if ok else f': {what}'))
        if not ok:
            self.failed.append(step)
        (self.out / 'steps.json').write_text(json.dumps(self.steps, indent=1))

    def check(self, step, condition, **what):
        self.note(step, bool(condition), **what)
        return bool(condition)


def short(reply):
    """A reply for the log, its record shortened."""
    r = json.loads(json.dumps(reply))
    if isinstance(r.get('record'), dict) and isinstance(r['record'].get('player'), str):
        r['record']['player'] = f'<{len(r["record"]["player"])} base64 characters>'
        for key in ('advancements', 'stats'):
            if r['record'].get(key) is not None:
                r['record'][key] = '<...>'
    return r


class Bot:
    """One horizon-bots client standing still, through a relay."""

    def __init__(self, bins, port, out, label, seconds, *extra):
        self.report = out / f'bot-{label}.json'
        self.log = (out / f'bot-{label}.log').open('w')
        self.process = subprocess.Popen(
            [str(bins / f'horizon-bots{EXE}'), '--server', f'127.0.0.1:{port}', '--bots', '1',
             '--behaviour', 'stand', '--seconds', str(seconds), '--name', NAME, '--report', str(self.report),
             *extra],
            stdout=self.log, stderr=subprocess.STDOUT, env=lab.game_env())

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(10)
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.log.close()


def run(bins, resources, scratch, out):
    steps = Steps(out)
    player_uuid = str(offline_uuid(PLAYER))
    relay_log = (out / 'relay.log').open('w')

    def log(line):
        relay_log.write(line + '\n')
        relay_log.flush()

    a = Engine(bins, resources, scratch / 'a', out, 'a')
    # One place on B: the player moved there takes it.
    b = Engine(bins, resources, scratch / 'b', out, 'b', **{'max-players': '1'})
    stranger = str(offline_uuid('Stranger'))
    relays, bots = [], []
    try:
        a.ready()
        b.ready()
        relay_a, relay_b = Relay(a.port, log), Relay(b.port, log)
        relays += [relay_a, relay_b]
        steps.note('both engines up, transfers on, forwarding on', True, ports={'a': a.port, 'b': b.port})

        # 1. The player on A, through the proxy; a direct login is refused.
        direct = Bot(bins, a.port, out, 'direct', 20)
        bots.append(direct)
        direct.process.wait(60)
        steps.check('a login that skips the proxy is refused',
                    any('behind a proxy' in l or 'Refused' in l for l in a.server.since(0)))
        mark_a = a.server.count()
        # Three seconds after it joins, the client picks up its first hotbar slot (inventory menu
        # slot 36, where the diamonds go) and puts it in the 2x2 crafting grid (slot 1): two
        # ServerboundContainerClickPackets (window 0, a pickup each, no predicted slots).
        clicks = ','.join(f'{tick}:{CONTAINER_CLICK}:{click_body(slot)}' for tick, slot in ((60, 36), (64, 1)))
        first = Bot(bins, relay_a.port, out, 'first', 240, '--raw', clicks)
        bots.append(first)
        a.server.wait_for(rf'{PLAYER} joined the game', 60)
        a.server.console(f'give {PLAYER} minecraft:diamond 5')
        a.server.console(f'give {PLAYER} minecraft:oak_log 12')
        a.server.console(f'xp add {PLAYER} 7 levels')
        # The client's clicks put the diamonds in its 2x2 grid; the export must put them back in
        # the inventory, or they are in no record.
        time.sleep(5)
        steps.note('1. the player is on A through the proxy, with 5 diamonds, 12 logs and 7 levels', True)
        # The player compound has no crafting grid: diamonds that left `Inventory` are in it.
        mark = a.server.count()
        a.server.console(f'data get entity {PLAYER} Inventory')
        time.sleep(1)
        inventory = [l for l in a.server.since(mark) if 'entity data' in l or 'Inventory' in l]
        steps.check("1. its client moved the diamonds into its 2x2 crafting grid (they left its Inventory)",
                    inventory and not any('diamond' in l for l in inventory)
                    and any('oak_log' in l for l in inventory), console=inventory)
        a_file = a.world_file('data', player_uuid)
        a_before = a_file.stat().st_mtime_ns if a_file.exists() else None

        # 2. B expects it.
        reply = b.ask(f'player-expect {player_uuid}')
        steps.check('2. player-expect on B', reply.get('ok') is True, reply=reply)
        status = b.ask('transfer-status')
        steps.check('2. transfer-status on B counts the player as expected, and B is not full',
                    status.get('expected') == 1 and status.get('players') == 0 and status.get('full') is False,
                    status=status)
        reply = b.ask(f'player-expect {stranger}')
        steps.check("2. B's one place is the expected player's: a second player-expect is refused as full",
                    reply == {'error': 'the server is full'}, reply=reply)

        # 3. A exports it, frozen; a repeat answers the same bytes.
        reply = a.ask(f'player-export {player_uuid} freeze')
        record = reply.get('record') or {}
        handover = record.get('handover')
        steps.check('3. player-export freeze on A gives a record with a hand-over id',
                    reply.get('ok') is True and isinstance(handover, str) and len(handover) == 32,
                    reply=short(reply))
        again = a.ask(f'player-export {player_uuid} freeze')
        steps.check('3. a repeated export answers the same record, byte for byte',
                    json.dumps(again.get('record'), sort_keys=True) == json.dumps(record, sort_keys=True))
        steps.check('3. the export says how long the freeze still lasts',
                    0 < again.get('freeze_ticks_left', 0) <= 600, freeze_ticks_left=again.get('freeze_ticks_left'))

        # While frozen: no damage, no pickup.
        mark = a.server.count()
        a.server.console(f'damage {PLAYER} 5')
        a.server.console('summon minecraft:item 0.5 -60 0.5 {Item:{id:"minecraft:emerald",count:3},PickupDelay:0}')
        time.sleep(2)
        a.server.console('kill @e[type=minecraft:item]')
        time.sleep(1)
        console = a.server.since(mark)
        steps.check('3. a frozen player picks nothing up: the emeralds at its feet are still in the world',
                    any('Killed' in l for l in console), console=console)

        # 4. The second connection logs in to B, held.
        mark_b = b.server.count()
        second = Bot(bins, relay_b.port, out, 'second', 240)
        bots.append(second)
        b.server.wait_for(rf'{PLAYER} is held: it arrives from another server', 60)
        steps.check('4. the login on B is held, and announced to no one',
                    not any('joined the game' in l for l in b.server.since(mark_b)))
        status = b.ask('transfer-status')
        steps.check('4. transfer-status on B: held, no longer expected, and its place taken',
                    status.get('held') == 1 and status.get('expected') == 0 and status.get('online') == 1,
                    status=status)
        status = a.ask('transfer-status')
        steps.check('4. transfer-status on A: the player is placed and frozen',
                    status.get('players') == 1 and status.get('frozen') == 1, status=status)

        # 5. Import without placing.
        request = json.dumps({'record': record, 'place': False})
        reply = b.ask(f'player-import {player_uuid} {request}')
        steps.check('5. player-import on B, place false: taken, still held',
                    reply.get('ok') is True and reply.get('placed') is False and reply.get('handover') == handover,
                    reply=reply)
        b_file = b.world_file('data', player_uuid)
        steps.check('5. nothing is written before the confirmation', not b_file.exists())

        # 6. Confirm: the commit point.
        reply = b.ask(f'player-confirm {player_uuid} {handover}')
        written = b_file.exists()
        player = read_nbt(gzip.decompress(b_file.read_bytes())) if written else {}
        steps.check('6. player-confirm on B: placed, and its file is on disk when the reply comes',
                    reply.get('ok') is True and reply.get('placed') is True and written, reply=reply)
        steps.check("6. B's file holds the record's items and experience",
                    items_in(player, 'minecraft:diamond') == 5 and items_in(player, 'minecraft:oak_log') == 12
                    and player.get('XpLevel') == 7,
                    diamonds=items_in(player, 'minecraft:diamond'), logs=items_in(player, 'minecraft:oak_log'),
                    level=player.get('XpLevel'))
        exported = read_nbt(base64.b64decode(record['player'])) if record.get('player') else {}
        steps.check('3. the diamonds of the 2x2 grid are back in the exported inventory',
                    items_in(exported, 'minecraft:diamond') == 5, diamonds=items_in(exported, 'minecraft:diamond'))
        steps.check('6. the statistics went with it', b.world_file('stats', player_uuid).exists())
        steps.check('6. no emerald came: the record is the player at the export',
                    items_in(player, 'minecraft:emerald') == 0)
        # A proxy that lost the reply asks again: the same answer, nothing done again.
        retry = b.ask(f'player-confirm {player_uuid} {handover}')
        steps.check('6. a confirm retried after the commit is answered ok, placed',
                    retry.get('ok') is True and retry.get('placed') is True and retry.get('retried') is True,
                    reply=retry)
        retry = b.ask(f'player-import {player_uuid} {request}')
        steps.check('6. an import retried after the commit is answered ok, placed',
                    retry.get('ok') is True and retry.get('placed') is True and retry.get('retried') is True,
                    reply=retry)

        # 7. Release on A.
        reply = a.ask(f'player-release {player_uuid} {handover} moved')
        steps.check('7. player-release moved on A', reply.get('ok') is True and reply.get('released') == 'moved',
                    reply=reply)
        first.process.wait(30)
        time.sleep(2)
        report = json.loads(first.report.read_text())['per_bot'][0] if first.report.exists() else {}
        steps.check('3. the frozen player took no damage (its client was never sent a lower health)',
                    report.get('health') in (None, 20.0, 20), health=report.get('health'))
        lines_a = a.server.since(mark_a)
        steps.check('7. A logs no "left the game" for a player moved away',
                    not any(f'{PLAYER} left the game' in l for l in lines_a))
        steps.check("7. A's connection of the player is closed", first.process.poll() is not None)
        late = a.ask(f'player-release {player_uuid} {handover} moved')
        steps.check('7. a repeated release is refused', 'error' in late, reply=late)
        status_a, status_b = a.ask('transfer-status'), b.ask('transfer-status')
        steps.check('7. transfer-status: the player is on B now, and A has nobody',
                    status_b.get('players') == 1 and status_b.get('held') == 0
                    and status_a.get('players') == 0 and status_a.get('frozen') == 0,
                    a=status_a, b=status_b)
        reply = b.ask(f'player-expect {stranger}')
        steps.check('7. B is full with the player on it: player-expect is refused, and transfer-status says full',
                    reply == {'error': 'the server is full'} and status_b.get('full') is True
                    and status_b.get('stopping') is False, reply=reply)

        # The cancel: from B back to A, aborted.
        reply = a.ask(f'player-expect {player_uuid}')
        steps.check('cancel: player-expect on A', reply.get('ok') is True, reply=reply)
        reply = b.ask(f'player-export {player_uuid} freeze')
        back = (reply.get('record') or {}).get('handover')
        steps.check('cancel: player-export freeze on B', reply.get('ok') is True and back, reply=short(reply))
        third = Bot(bins, relay_a.port, out, 'third', 60)
        bots.append(third)
        a.server.wait_for(rf'{PLAYER} is held: it arrives from another server', 60)
        reply = a.ask(f'player-drop {player_uuid}')
        steps.check('cancel: player-drop on A lets the held login go', reply.get('held') is True, reply=reply)
        a.server.wait_for(rf'{PLAYER} was dropped while held', 30)
        reply = b.ask(f'player-release {player_uuid} {back} abort')
        steps.check('cancel: player-release abort on B', reply.get('released') == 'abort', reply=reply)
        reply = b.ask(f'player-release {player_uuid} {back} moved')
        steps.check('cancel: a moved after the abort is refused', 'error' in reply, reply=reply)
        status = b.ask('server-status')
        names = [p.get('name') for p in status.get('players', [])]
        steps.check('cancel: the player plays on on B', PLAYER in names and second.process.poll() is None,
                    players=names)

        # A wrote nothing for the player it let go, before or after it stops.
        a.stop()
        a_after = a_file.stat().st_mtime_ns if a_file.exists() else None
        steps.check('A wrote no file for the player after it moved away, even when it stopped',
                    a_after == a_before, before=a_before, after=a_after)
    finally:
        for bot in bots:
            bot.stop()
        for relay in relays:
            relay.close()
        for engine in (a, b):
            try:
                engine.stop()
            except Exception:  # noqa: BLE001 - stopping what is left
                pass
        relay_log.close()
    if steps.failed:
        raise Failed('failed: ' + '; '.join(steps.failed))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin-dir', default=str(lab.ROOT / 'target/fast'))
    parser.add_argument('--resources', default=str(lab.RESOURCES), help='the prepared game files')
    parser.add_argument('--out', default=str(lab.LOCAL / 'server-transfer-e2e'))
    args = parser.parse_args()
    bins, out = Path(args.bin_dir).resolve(), Path(args.out)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    os.environ.update(lab.game_env())
    with tempfile.TemporaryDirectory(prefix='server-transfer-e2e-') as scratch:
        try:
            run(bins, Path(args.resources), Path(scratch), out)
        except (Failed, RuntimeError) as e:
            sys.exit(f'server_transfer_e2e failed (steps and logs in {out}):\n{e}')
    print(f'server_transfer_e2e: the whole hand-over and its cancel went as docs/server.md says (steps in {out})')


if __name__ == '__main__':
    main()
