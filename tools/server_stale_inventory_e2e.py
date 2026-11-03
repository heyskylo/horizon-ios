#!/usr/bin/env python3
"""A stale inventory-window click must not act while a chest menu is open.

Uses real protocol packets against a disposable dedicated server. A normal
out-of-range click is followed by a window-zero THROW, targeting hotbar slot 0
as an inventory menu would. The currently open chest must authorize neither.
"""
import argparse
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import time

import lab
from server_container_click_e2e import entity_data
from server_horizon_only_e2e import EXE, server
from server_packet_rules_e2e import serverbound_ids, varint


def opened_window(capture):
    text = (lab.ROOT / 'crates/horizon-protocol/src/ids.rs').read_text()
    block = text[text.index('pub mod play {'):]
    block = block[block.index('pub mod clientbound {'):]
    block = block[:block.index('}')]
    packet = int(re.search(r'pub const OPEN_SCREEN: i32 = (\d+);', block)[1])
    close = int(re.search(r'pub const CONTAINER_CLOSE: i32 = (\d+);', block)[1])
    active = False
    data, offset = capture.read_bytes(), 0
    while offset + 12 <= len(data):
        _, kind, size = struct.unpack_from('<Iii', data, offset)
        offset += 12
        body = data[offset:offset + size]
        offset += size
        if kind == packet and body and body[0] > 0:
            active = True
        elif kind == close:
            active = False
    return active


def check(bins, scratch, out):
    s, port = server(bins, scratch / 'server', out, 'server',
                     {'gamemode': 'survival', 'spawn-protection': '0'})
    bot = None
    try:
        for command in ('setworldspawn 0 -60 0', 'gamerule respawn_radius 0',
                        'setblock 2 -60 0 minecraft:chest'):
            s.console(command)
        body = varint(0) + varint(0) + struct.pack('>hb', 36, 1) + varint(4) + b'\0\0'
        packet = serverbound_ids()['CONTAINER_CLICK']
        capture = out / 'packets.bin'
        bot = subprocess.Popen([bins / f'horizon-bots{EXE}', '--server', f'127.0.0.1:{port}',
                                '--bots', '1', '--behaviour', 'idle', '--height', '0',
                                '--seconds', '15', '--name', 'Stale',
                                '--click', '2,-60,0:63:1:4',
                                '--raw', f'100:{packet}:{body.hex()}', '--capture', capture],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        s.wait_for(r'Stale0 joined the game', 60)
        s.console('give Stale0 minecraft:stone 640')
        time.sleep(1)
        before = entity_data(s, 'Stale0', 'Inventory')
        time.sleep(4.5)
        after = entity_data(s, 'Stale0', 'Inventory')
        assert opened_window(capture), 'the bot did not receive a chest menu'
        count = lambda value: sum(map(int, re.findall(r'count: (\d+)', value)))
        result = {'before': count(before), 'after': count(after), 'chest_opened': True}
        (out / 'result.json').write_text(json.dumps(result, indent=2))
        assert result['before'] == 640, result
        assert result['after'] == 640, f'stale inventory click changed items: {result}'
        print(result)
    finally:
        if bot is not None:
            bot.kill()
            bot.wait()
        s.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, required=True)
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'server-stale-inventory')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    os.environ.update(lab.game_env())
    with tempfile.TemporaryDirectory(prefix='stale-inventory-') as scratch:
        check(args.bin_dir.resolve(), Path(scratch), args.out.resolve())


if __name__ == '__main__':
    main()
