#!/usr/bin/env python3
"""A controller does what the keyboard does: the same action, sent both ways, checked from outside.

    python3 tools/pad_parity_e2e.py                 # every case
    python3 tools/pad_parity_e2e.py --only drop_stack

Each case starts its own isolated integrated-server game through horizon-ctl (fresh scratch homes from
`lab.game_env`), does one thing with the keyboard and the same thing with the virtual pad, and compares
what the player can observe (item counts, files written). The keyboard result is the reference; a case
whose keyboard side fails is reported as such, not as a controller failure.

Cases:
- drop_stack: Ctrl+Q over an inventory slot throws the whole stack; the pad's left stick click
  (its "throw stack" button in menus) must too.
- quick_move: Shift-click on an inventory slot moves the stack; so must the pad's Y.
- screenshot: key.screenshot bound to the pad takes a screenshot as F2 does.

Needs built fast binaries (`cargo build --profile fast -p horizon-client -p horizon-client-lib --bins`)
and prepared resources in .local/horizon-client/26.3. Exit status 0 when every case matches.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
EXE = '.exe' if os.name == 'nt' else ''


class Game:
    def __init__(self, args, name, controller=''):
        self.args = args
        self.home = Path(tempfile.mkdtemp(prefix=f'pad-parity-{name}-'))
        homes = {key: self.home / key for key in ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME',
                                                    'XDG_CONFIG_HOME')}
        for folder in homes.values():
            folder.mkdir()
        if controller:
            (homes['HORIZON_HOME'] / 'horizon-controller.txt').write_text(controller)
        self.game_home = homes['HORIZON_HOME']
        self.env = lab.game_env(**homes)
        self.socket = self.home / 'ctl.json'
        self.ctl('launch', '--game', args.game, '--world', 'Parity', '--creative', '--offscreen', '--no-pause',
                 '--wait', '--render-distance', 2, '--resources', args.resources, timeout=500)

    def ctl(self, *words, timeout=60):
        r = subprocess.run([str(self.args.ctl), '--socket', str(self.socket), *map(str, words)],
                           cwd=self.game_home, env=self.env, capture_output=True, text=True, timeout=timeout)
        data = json.loads(r.stdout or '{}')
        if r.returncode or data.get('error'):
            raise RuntimeError(f'{words}: {data}; {r.stderr}')
        return data

    def quit(self):
        try:
            self.ctl('pad', 'release')
            self.ctl('quit')
        except (RuntimeError, subprocess.TimeoutExpired, json.JSONDecodeError):
            pass
        shutil.rmtree(self.home, ignore_errors=True)


def hotbar_count(game, slot):
    stack = game.ctl('inventory')['hotbar'][slot]
    return stack.get('count', 0)


def planks_in_inventory(args, name):
    """A survival game with 16 planks in the held hotbar slot 0 and the inventory open."""
    game = Game(args, name)
    game.ctl('command', 'gamemode survival')
    game.ctl('command', 'item replace entity @s hotbar.0 with minecraft:oak_planks 16')
    game.ctl('select', 1)
    game.ctl('wait', 5)
    game.ctl('key', 'e', 'tap')
    game.ctl('wait', 2)
    return game


def hover_slot(game, slot):
    ui = game.ctl('ui')
    x, y, w, h = next(wi for wi in ui['widgets'] if wi['id'] == f'slot-{slot}')['bounds']
    game.ctl('scroll-at', x + w / 2, y + h / 2, 0)
    return x + w / 2, y + h / 2


def drop_stack(args, device):
    """Throw hotbar slot 0's stack from the inventory. Returns what's left in the slot."""
    game = planks_in_inventory(args, f'drop-{device}')
    try:
        if device == 'keyboard':
            hover_slot(game, 0)
            game.ctl('key', 'ctrl', 'down')
            game.ctl('key', 'q', 'tap')
            game.ctl('key', 'ctrl', 'up')
        else:
            # The pad's pointer lands on the held hotbar slot first.
            game.ctl('pad', 'ls', 'tap')
        game.ctl('wait', 5)
        return hotbar_count(game, 0)
    finally:
        game.quit()


def quick_move(args, device):
    """Shift-click hotbar slot 0 into the main inventory. Returns what's left in the slot."""
    game = planks_in_inventory(args, f'move-{device}')
    try:
        if device == 'keyboard':
            x, y = hover_slot(game, 0)
            game.ctl('key', 'shift', 'down')
            game.ctl('click-at', x, y)
            game.ctl('key', 'shift', 'up')
        else:
            game.ctl('pad', 'north', 'tap')
        game.ctl('wait', 5)
        return hotbar_count(game, 0)
    finally:
        game.quit()


def screenshot(args, device):
    """Press the screenshot binding in game. Returns how many screenshots were written."""
    controller = 'bind_key.screenshot:select\nbind_key.playerlist:none\n' if device == 'pad' else ''
    game = Game(args, f'shot-{device}', controller)
    try:
        if device == 'keyboard':
            game.ctl('key', 'f2', 'tap')
        else:
            game.ctl('pad', 'select', 'tap')
        game.ctl('wait', 10)
        folder = game.game_home / 'screenshots'
        return len(list(folder.glob('*.png'))) if folder.exists() else 0
    finally:
        game.quit()


CASES = {
    'drop_stack': (drop_stack, 0),
    'quick_move': (quick_move, 0),
    'screenshot': (screenshot, 1),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--game', type=Path, default=lab.cargo_target() / f'fast/horizon-client{EXE}')
    parser.add_argument('--ctl', type=Path, default=lab.cargo_target() / f'fast/horizon-ctl{EXE}')
    parser.add_argument('--resources', type=Path, default=ROOT / '.local/horizon-client/26.3')
    parser.add_argument('--only', nargs='*', choices=sorted(CASES), help='run only these cases')
    args = parser.parse_args()
    args.game, args.ctl, args.resources = args.game.resolve(), args.ctl.resolve(), args.resources.resolve()
    failed = []
    for name in args.only or CASES:
        case, want = CASES[name]
        keyboard = case(args, 'keyboard')
        pad = case(args, 'pad')
        if keyboard != want:
            print(f'FAIL {name}: the keyboard reference gave {keyboard}, expected {want}')
            failed.append(name)
        elif pad != keyboard:
            print(f'FAIL {name}: the pad gave {pad}, the keyboard {keyboard}')
            failed.append(name)
        else:
            print(f'ok   {name}: pad and keyboard both gave {pad}')
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
