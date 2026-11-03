#!/usr/bin/env python3
"""A stale inventory-window click must not act while a chest menu is open.

Uses real protocol packets against a disposable dedicated server. A normal
out-of-range click is followed by a window-zero THROW, targeting hotbar slot 0
as an inventory menu would. The currently open chest must authorize neither.
The close cases send an old close ID, then a click for the formerly open menu;
Java closes the current menu regardless of the supplied close ID. The cursor
variant checks inventory counts before pickup, while carried, and after close.
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


def check(bins, scratch, out, case="inventory"):
    s, port = server(bins, scratch / 'server', out, 'server',
                     {'gamemode': 'survival', 'spawn-protection': '0'})
    bot = None
    try:
        # With no players, the spawn chunk may already have unloaded after
        # startup. Confirm chest creation before starting the timed bot.
        s.console('forceload add 0 0')
        deadline = time.monotonic() + 30
        while True:
            mark = s.count()
            s.console('setblock 2 -60 0 minecraft:chest')
            time.sleep(0.1)
            if any('Changed the block at 2, -60, 0' in line for line in s.since(mark)):
                break
            if time.monotonic() >= deadline:
                raise AssertionError('test chest was not created in its loaded chunk')
        s.console('setworldspawn 0 -60 0')
        s.console('gamerule respawn_radius 0')
        ids = serverbound_ids()
        window, slot = (0, 36) if case == 'inventory' else (1, 27)
        body = varint(window) + varint(0) + struct.pack('>hb', slot, 1) + varint(4) + b'\0\0'
        packet = ids['CONTAINER_CLICK']
        raw = f'100:{packet}:{body.hex()}'
        if case in ('close', 'cursor-close'):
            # Java closes the current menu even when this supplied ID is stale.
            raw = f'100:{ids["CONTAINER_CLOSE"]}:09,102:{packet}:{body.hex()}'
            if case == 'cursor-close':
                pickup = varint(1) + varint(0) + struct.pack('>hb', 27, 0) + varint(0) + b'\0\0'
                raw = (f'80:{packet}:{pickup.hex()},120:{ids["CONTAINER_CLOSE"]}:09,'
                       f'122:{packet}:{body.hex()}')
        capture = out / 'packets.bin'
        bot = subprocess.Popen([bins / f'horizon-bots{EXE}', '--server', f'127.0.0.1:{port}',
                                '--bots', '1', '--behaviour', 'idle', '--height', '0',
                                '--seconds', '15', '--name', 'Stale',
                                '--click', '2,-60,0:63:1:4',
                                '--raw', raw, '--capture', capture],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        s.wait_for(r'Stale0 joined the game', 60)
        s.console('give Stale0 minecraft:stone 640')
        time.sleep(1)
        before = entity_data(s, 'Stale0', 'Inventory')
        count = lambda value: sum(map(int, re.findall(r'count: (\d+)', value)))
        if case == 'cursor-close':
            time.sleep(3.5)
            middle = entity_data(s, 'Stale0', 'Inventory')
            assert count(middle) == 576, 'the cursor did not take the expected 64 stone'
            time.sleep(2)
        else:
            time.sleep(4.5)
        after = entity_data(s, 'Stale0', 'Inventory')
        assert opened_window(capture), 'the bot did not receive a chest menu'
        result = {'case': case, 'before': count(before), 'after': count(after), 'chest_opened': True}
        if case == 'cursor-close':
            result['while_carried'] = count(middle)
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
    parser.add_argument('--case', choices=['inventory', 'close', 'cursor-close'], default='inventory')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'server-stale-inventory')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    os.environ.update(lab.game_env())
    with tempfile.TemporaryDirectory(prefix='stale-inventory-') as scratch:
        check(args.bin_dir.resolve(), Path(scratch), args.out.resolve(), args.case)


if __name__ == '__main__':
    main()
