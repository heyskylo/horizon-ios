#!/usr/bin/env python3
"""On the wire, a Horizon client must look like vanilla 26.3's (its brand aside): a server's anticheat reads
the order and timing of what the client sends, and a difference from vanilla's client can flag an honest
player or keep them out. A real Horizon client joins a real vanilla 26.3 dedicated server through
tools/net_capture.py, which records every packet; each rule below is one thing vanilla's client does,
checked on that capture.

The proxy can hold the server's Login packet back (--login-delay, 1 s by default): a busy server, or a
Paper server with plugins, takes that long to place a new player, and the client's behaviour while it waits
is part of the wire.

Rules (vanilla's code in brackets):
- no-play-before-login: nothing in the play state before the server's Login. [Minecraft.tick sends
  ServerboundClientTickEndPacket only while it has a level, made by ClientPacketListener.handleLogin]
- brand-before-information: the configuration phase opens with the brand, then the client information.
  [ClientHandshakePacketListenerImpl.handleLoginFinished]
- no-play-information: no ClientInformation in the play state while no option changed. [the configuration
  phase sent it; ClientPacketListener.broadcastClientInformation runs only from Options.broadcastOptions]

  python3 tools/vanilla_wire_e2e.py
  python3 tools/vanilla_wire_e2e.py --rules no-play-before-login --login-delay 2

Every game and server here runs in fresh temporary folders (HORIZON_HOME, HORIZON_LAUNCHER_HOME, XDG_*), with no
audio and no self-update. Needs .local (the vanilla jars and prepared resources) and a built horizon-client and
horizon-ctl (--bin DIR, default target/fast).
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402
import net_capture  # noqa: E402
import test_rust_parity as base  # noqa: E402
from server_pack_e2e import Game  # noqa: E402
from test_dedicated_server_parity import Server, free_port  # noqa: E402

PLAYER = 'Steve'
PROPERTIES = {
    'online-mode': 'false', 'white-list': 'false', 'level-seed': '12345', 'level-type': 'minecraft\\:flat',
    'generate-structures': 'false', 'view-distance': '3', 'simulation-distance': '3', 'spawn-protection': '0',
    'pause-when-empty-seconds': '-1', 'gamemode': 'survival',
}


class Failed(Exception):
    pass


def hold_login(seconds):
    """Make the proxy hold each clientbound Login packet back for `seconds` before forwarding it."""
    record = net_capture.Session.record

    def delayed(self, direction, packet_id, payload):
        name = self.name(direction, packet_id)
        record(self, direction, packet_id, payload)
        if direction == 'clientbound' and self.state['clientbound'] == 'play' and name == 'login':
            time.sleep(seconds)
    net_capture.Session.record = delayed


def packets(capture, conn):
    return [e for e in net_capture.load(capture) if e.get('conn') == conn and 'name' in e]


def rule_no_play_before_login(entries):
    """Nothing serverbound in play before the clientbound Login (a client without a level sends nothing)."""
    early = []
    for e in entries:
        if e['dir'] == 's2c' and e['state'] == 'play' and e['name'] == 'login':
            break
        if e['dir'] == 'c2s' and e['state'] == 'play':
            early.append(e['name'])
    if early:
        counts = {n: early.count(n) for n in dict.fromkeys(early)}
        raise Failed(f'{len(early)} serverbound play packets before the server\'s Login: {counts}')
    return 'no serverbound play packet before Login'


def rule_brand_before_information(entries):
    """The first two serverbound configuration packets: the brand payload, then the client information."""
    sent = [e for e in entries if e['dir'] == 'c2s' and e['state'] == 'configuration'][:2]
    names = [e['name'] for e in sent]
    if names != ['custom_payload', 'client_information']:
        raise Failed(f'configuration opens with {names}, vanilla sends [custom_payload (brand), client_information]')
    payload = bytes.fromhex(sent[0].get('data', ''))
    channel = payload[1:1 + payload[0]] if payload else b''  # a short string: one length byte
    if channel != b'minecraft:brand':
        raise Failed(f'the first payload is {channel!r}, not minecraft:brand')
    return 'brand, then client_information'


def rule_no_play_information(entries):
    """No client_information in play: the options never change during the run."""
    sent = [e for e in entries if e['dir'] == 'c2s' and e['state'] == 'play' and e['name'] == 'client_information']
    if sent:
        raise Failed(f'{len(sent)} client_information in play with no option changed (first at {sent[0]["t"]} s)')
    return 'no client_information in play'


RULES = {
    'no-play-before-login': rule_no_play_before_login,
    'brand-before-information': rule_brand_before_information,
    'no-play-information': rule_no_play_information,
}


def run(args, out):
    lock, jars, libraries = base.verify_artifacts()
    deps = os.pathsep.join(map(str, [jars['server'], *libraries]))
    vanilla = ['java', '-Xmx1g', '-cp', deps, 'net.minecraft.server.Main', 'nogui']
    resources = lab.LOCAL / 'horizon-client' / lab.VERSION
    work = Path(tempfile.mkdtemp(prefix='hz-wire-'))
    print(f'scratch: {work}')
    for var, sub in (('HORIZON_HOME', 'home'), ('HORIZON_LAUNCHER_HOME', 'launcher'), ('XDG_DATA_HOME', 'data'),
                     ('XDG_CONFIG_HOME', 'config')):
        (work / 'servers' / sub).mkdir(parents=True)
        os.environ[var] = str(work / 'servers' / sub)
    os.environ.update(lab.game_env(HORIZON_TELEMETRY_URL='off'))
    directory = work / 'server'
    directory.mkdir()
    port = free_port()
    props = dict(PROPERTIES, **{'server-port': str(port), 'server-ip': '127.0.0.1'})
    (directory / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()),
                                                 encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    server = Server(vanilla, directory, 'server')
    capture = out / 'capture.jsonl'
    capture.touch()
    game = None
    try:
        server.wait_for(r'Done \(', timeout=300)
        if args.login_delay > 0:
            hold_login(args.login_delay)
        listen = free_port()
        stop = net_capture.start_background(listen, f'127.0.0.1:{port}', capture)
        game = Game(Path(args.bin), resources, work / 'game')
        game('connect', f'127.0.0.1:{listen}', PLAYER)
        game('wait-for', 'loaded', '--timeout', 180)
        game('wait', args.ticks)
        game('disconnect', ok=False)
        time.sleep(1)
        stop.set()
    finally:
        if game is not None:
            game.quit()
            if game.log.exists():
                shutil.copy(game.log, out / 'client.log')
        server.stop()
        shutil.copy(server.log_path, out / server.log_path.name)
    entries = packets(capture, 1)
    results = []
    for name in args.rules:
        try:
            results.append((name, 'pass', RULES[name](entries)))
        except Failed as e:
            results.append((name, 'FAIL', str(e)))
    shutil.rmtree(work, ignore_errors=True)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin', default=str(lab.cargo_target() / 'fast'), help='horizon-client and horizon-ctl')
    parser.add_argument('--out', default=str(lab.LOCAL / 'vanilla-wire-e2e'))
    parser.add_argument('--rules', default=','.join(RULES), type=lambda s: s.split(','),
                        help='comma-separated rule names (default: all)')
    parser.add_argument('--login-delay', type=float, default=1.0,
                        help='seconds the proxy holds the server\'s Login back (0: none)')
    parser.add_argument('--ticks', type=int, default=100, help='game ticks to stay after the world loaded')
    args = parser.parse_args()
    unknown = [r for r in args.rules if r not in RULES]
    if unknown:
        parser.error(f'unknown rules {unknown}; known: {", ".join(RULES)}')
    out = Path(args.out)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    results = run(args, out)
    print(f'\nvanilla 26.3 server, login held {args.login_delay} s; capture in {out / "capture.jsonl"}')
    for name, result, detail in results:
        print(f'  {result:4}  {name}: {detail}')
    if any(r != 'pass' for _, r, _ in results):
        sys.exit(1)


if __name__ == '__main__':
    main()
