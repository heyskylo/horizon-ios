#!/usr/bin/env python3
"""A controller in menus: buttons press on release, and the title and pause shortcuts, checked from outside.

    python3 tools/pad_menu_e2e.py                 # every case
    python3 tools/pad_menu_e2e.py --only release

Each case starts its own isolated game through horizon-ctl (fresh scratch homes from `lab.game_env`) and drives
it with the virtual pad, reading what `pad status` reports: the focused widget and the screen's targets.

Cases:
- idle_a: A before the pad has touched anything lands the focus on Singleplayer and presses nothing.
- idle_b: B before the pad has touched anything focuses Quit Game.
- idle_x: X before the pad has touched anything opens Options; B comes back, not on Quit Game.
- release: A down leaves the button alone, A up presses it.
- move_cancels: moving the focus with A held presses nothing when A comes up.
- pad_release: the pad letting go of everything (`pad release`) presses nothing.
- pause_y: Y on a pause menu opened from the keyboard focuses its exit button; reopening starts elsewhere.
- settings: the options' Done declares its job, nothing there labelled Done, Back or Cancel is undeclared, and B
  presses it.
- create_world: Cancel declares its job, nothing there is undeclared, and B presses it.
- world_options: Apply is declared but off with nothing changed, so X does nothing; B presses Cancel.
- key_row_b: B on a key row waiting for its key unbinds it (Escape), not binding Left Click by pressing Done.
- lock_question: nothing is declared under World Options' lock question; B answers it, X does nothing.
- optimize_world: B doesn't stop Optimize World, which declares nothing.
- pointer_screens: Statistics', Advancements' and the packs' Done show a glyph and B presses them.
- settings_pages: nothing labelled Done, Back or Cancel is undeclared on any options page, Graphics included.
- split_setup: Splitscreen Setup's Done is declared, and off while a player is held.
- actions_follow_screen: right after a press opens a screen, the screen before it can't be pressed.
- glyphs_title: the title's X and B on their buttons, A once the focus is on the exit, gone at the mouse.
- glyph_on_field: A on a focused text field and B on Cancel; A opens one keyboard, which hides them; B closes it.
- settings_apply: X presses World Options' Apply once something changed.
- long_label: a label too wide for its button slides, beside its glyph.
Needs built fast binaries (`cargo build --profile fast -p horizon-client -p horizon-client-lib --bins`)
and prepared resources in .local/horizon-client/26.3. Exit status 0 when every case passes.
"""
import argparse
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
EXE = '.exe' if os.name == 'nt' else ''
SETTLE = 0.5


class Game:
    def __init__(self, args, name, world=False, seed=None):
        self.args = args
        self.home = Path(tempfile.mkdtemp(prefix=f'pad-menu-{name}-'))
        homes = {key: self.home / key for key in ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME',
                                                    'XDG_CONFIG_HOME')}
        for folder in homes.values():
            folder.mkdir()
        self.game_home = homes['HORIZON_HOME']
        # A first launch shows the accessibility onboarding before the title.
        (self.game_home / 'options.txt').write_text('onboardAccessibility:false\n')
        if seed:
            seed(self.game_home)
        self.env = lab.game_env(**homes)
        self.socket = self.home / 'ctl.json'
        launch = ['launch', '--game', args.game, '--offscreen', '--render-distance', 2, '--resources', args.resources]
        if world:
            launch += ['--world', 'Menus', '--creative', '--no-pause', '--wait']
        self.ctl(*launch, timeout=500)

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

    def menu(self):
        return self.ctl('pad', 'status')['menu']

    def rects(self):
        return [tuple(t['rect']) for t in self.menu()['targets']]

    def focus(self):
        focus = self.menu()['focus']
        return tuple(focus['rect']) if focus else None

    def wait_targets(self, timeout=120):
        """Until the screen has drawn its widgets (the title fades in first)."""
        end = time.time() + timeout
        while time.time() < end:
            if self.rects():
                # A pad press mustn't land mid-fade: the same widgets two polls running.
                first = self.rects()
                time.sleep(SETTLE)
                if self.rects() == first:
                    return
            time.sleep(0.2)
        raise RuntimeError('the screen never drew a target')

    def widgets(self):
        return self.ctl('ui')['widgets']

    def rows(self):
        return [w for w in self.widgets() if w['kind'] == 'row']

    def focus_on(self, wanted):
        """The D-pad, down (right along its row, or at the bottom) until the focus reaches this widget id."""
        # A long list scrolls under the D-pad on the way past.
        for _ in range(60):
            was = self.focus()
            bounds = next(tuple(w['bounds']) for w in self.widgets() if w['id'] == wanted)
            if was == bounds:
                return
            # In the row of the focus, it's beside it.
            if was and was[1] == bounds[1] and was[0] < bounds[0]:
                self.pad('right')
                continue
            self.pad('down')
            if self.focus() == was:
                self.pad('right')
        raise RuntimeError(f'the D-pad never reached {wanted}: focus on {self.focus()}')

    def pad(self, button, action='tap'):
        self.ctl('pad', button, action)
        time.sleep(SETTLE)

    def title_buttons(self):
        """Singleplayer (the widest, topmost button), and Options and Quit Game (the half-width pair)."""
        end = time.time() + 120
        while True:
            # The footer link draws before the buttons do.
            buttons = [r for r in self.rects() if r[3] == 20]
            widest = max((r[2] for r in buttons), default=0)
            pair = sorted((r for r in buttons if 40 < r[2] < widest), key=lambda r: r[0])
            if len(pair) >= 2:
                break
            if time.time() > end:
                raise RuntimeError(f'no title buttons in {self.rects()}')
            time.sleep(0.2)
        time.sleep(SETTLE)
        single = min((r for r in buttons if r[2] == widest), key=lambda r: r[1])
        return single, pair[0], pair[-1]


class Check:
    def __init__(self):
        self.failures = []

    def __call__(self, ok, what):
        if not ok:
            self.failures.append(what)


def title_case(body):
    def case(args, name):
        game = Game(args, name)
        check = Check()
        try:
            game.wait_targets()
            body(game, check, *game.title_buttons())
        finally:
            game.quit()
        return check.failures
    return case


@title_case
def idle_a(game, check, single, options, quit_game):
    check(game.focus() is None, 'something was focused before the pad was touched')
    game.pad('south')
    check(game.focus() == single, f'focus is {game.focus()}, not Singleplayer {single}')
    check(quit_game in game.rects(), 'A pressed something: the title buttons went away')


@title_case
def idle_b(game, check, single, options, quit_game):
    game.pad('east')
    check(game.focus() == quit_game, f'focus is {game.focus()}, not Quit Game {quit_game}')


@title_case
def idle_x(game, check, single, options, quit_game):
    game.pad('west')
    check(quit_game not in game.rects(), 'X did not leave the title for Options')
    game.pad('east')
    game.wait_targets()
    check(quit_game in game.rects(), 'B did not come back to the title')
    check(game.focus() != quit_game, 'came back on Quit Game')


@title_case
def release(game, check, single, options, quit_game):
    game.pad('south')
    game.pad('south', 'down')
    check(quit_game in game.rects(), 'A pressed the button on the way down')
    game.pad('south', 'up')
    check(quit_game not in game.rects(), 'A did not press the button on the way up')


@title_case
def move_cancels(game, check, single, options, quit_game):
    game.pad('south')
    game.pad('south', 'down')
    game.pad('down')
    moved = game.focus()
    check(moved not in (None, single), f'the D-pad did not move the focus: {moved}')
    game.pad('south', 'up')
    check(quit_game in game.rects(), 'A up pressed a button after the focus moved')
    check(game.focus() == moved, f'focus went to {game.focus()}')


@title_case
def pad_release(game, check, single, options, quit_game):
    game.pad('south')
    game.pad('south', 'down')
    game.ctl('pad', 'release')
    time.sleep(SETTLE)
    check(quit_game in game.rects(), '`pad release` pressed the button')


def open_create_world(game):
    game.pad('south')
    game.pad('south')
    game.wait_targets()


def labels(game):
    return [w.get('label') or '' for w in game.widgets()]


def declared(game):
    return {a['action']: a['enabled'] for a in game.menu()['actions']}


def glyphs(game):
    """The pad buttons whose glyph is drawn, each with the widget under it."""
    menu = game.menu()

    def under(at):
        return next((tuple(t['rect']) for t in menu['targets'] if t['rect'][0] <= at[0] < t['rect'][0] + t['rect'][2]
                     and t['rect'][1] <= at[1] < t['rect'][1] + t['rect'][3]), None)
    return {g['button']: under(g['at']) for g in menu['glyphs']}


def open_world_options(game):
    game.ctl('key', 'escape', 'tap')
    game.wait_targets()
    game.ctl('click', 'world-options')
    time.sleep(SETTLE)
    game.wait_targets()
    game.pad('down')


def widget(game, wanted):
    """The widget whose id has this in it."""
    widgets = game.widgets()
    found = next((w for w in widgets if wanted in w['id']), None)
    if not found:
        raise RuntimeError(f"no {wanted} in {[w['id'] for w in widgets]}")
    return found


def click_widget(game, found):
    x, y, w, h = found['bounds']
    game.ctl('click-at', x + w / 2, y + h / 2)
    time.sleep(SETTLE)


def key_row_b(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.wait_targets()
        game.title_buttons()
        game.pad('west')
        game.ctl('options', 'keybinds')
        time.sleep(SETTLE)
        game.wait_targets()
        game.pad('down')
        keys = ('Left Button', 'Right Button', 'Middle Button', 'W', 'A', 'S', 'D', 'E', 'Q', 'F', 'Left Shift')
        row = next(w for w in game.widgets() if w.get('label') in keys)
        click_widget(game, row)
        check(game.menu()['actions'] == [], f"declared while waiting: {game.menu()['actions']}")
        check(glyphs(game) == {}, f'glyphs while waiting: {glyphs(game)}')
        game.pad('east')
        after = next(w for w in game.widgets() if w['bounds'] == row['bounds'])
        check(after.get('label') not in (row['label'], 'Left Button'), f"B left the row on {after.get('label')}")
        check(declared(game).get('Done') is True, 'B left the key binds')
    finally:
        game.quit()
    return check.failures


def lock_question(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        open_world_options(game)
        # The lock's question comes with Apply, once the lock has changed. The lock is an icon beside Difficulty.
        game.focus_on('difficulty')
        difficulty = game.focus()
        game.pad('right')
        check(game.focus() != difficulty and game.focus()[2] == 20, f'right of Difficulty is {game.focus()}, not the lock')
        game.pad('south')
        check(declared(game).get('Apply') is True, f'the lock did not turn Apply on: {declared(game)}')
        click_widget(game, widget(game, 'apply'))
        check(game.menu()['actions'] == [], f"declared under the question: {game.menu()['actions']}")
        check(glyphs(game) == {}, f'glyphs under the question: {glyphs(game)}')
        before = game.rects()
        game.pad('west')
        check(game.rects() == before, 'X under the question did something')
        game.pad('east')
        time.sleep(SETTLE)
        check(declared(game).get('Cancel') is True, f'B did not answer the question: {declared(game)}')
    finally:
        game.quit()
    return check.failures


def optimize_world(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        click_widget(game, next(w for w in game.widgets() if w.get('label') == 'Save and Quit to Title'))
        game.wait_targets()
        game.title_buttons()
        # A small world is done in a moment: enough regions to read that it takes a while.
        source = max((game.game_home / 'saves' / 'Menus').rglob('region/*.mca'), key=lambda f: f.stat().st_size)
        for i in range(400):
            shutil.copy(source, source.parent / f'r.{100 + i}.100.mca')
        game.pad('south')
        game.pad('south')
        game.wait_targets()
        game.pad('down')
        click_widget(game, game.rows()[0])
        click_widget(game, next(w for w in game.widgets() if w.get('label') == 'Edit'))
        click_widget(game, next(w for w in game.widgets() if w.get('label') == 'Optimize World'))
        click_widget(game, next(w for w in game.widgets() if w.get('label') == "I Know What I'm Doing!"))
        check(game.widgets()[0].get('label') == 'Cancel', f'not on Optimize World: {labels(game)}')
        game.pad('down')
        check(game.menu()['actions'] == [], f"Optimize World declares {game.menu()['actions']}")
        check(game.menu()['undeclared'] == ['Cancel'], f"undeclared: {game.menu()['undeclared']}")
        before = game.rects()
        game.ctl('pad', 'east', 'tap')
        time.sleep(0.2)
        check(game.rects() == before, 'B stopped Optimize World')
    finally:
        game.quit()
    return check.failures


def pointer_screens(args, name):
    game = Game(args, name, world=True)
    check = Check()

    def done_by_b(screen, opened):
        opened()
        time.sleep(1.5)
        game.pad('left')
        menu = game.menu()
        check(menu['mode'] == 'pointer', f"{screen} is in {menu['mode']}")
        check(declared(game).get('Done') is True, f'{screen} declares {declared(game)}')
        check('East' in glyphs(game), f'{screen} shows {glyphs(game)}')
        shown = game.widgets()
        game.pad('east')
        time.sleep(SETTLE)
        check(game.widgets() != shown, f'B did not leave {screen}')

    try:
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        done_by_b('Statistics', lambda: game.ctl('click', 'statistics'))
        check('Exit' in declared(game), 'B did not come back to the pause menu from Statistics')
        done_by_b('Advancements', lambda: game.ctl('click', 'advancements'))
        check('Exit' in declared(game), 'B did not come back to the pause menu from Advancements')
        done_by_b('Packs', lambda: game.ctl('options', 'packs'))
    finally:
        game.quit()
    return check.failures


def settings_pages(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.wait_targets()
        game.title_buttons()
        game.pad('west')
        for page in ('video', 'sound', 'controls', 'keybinds', 'mouse', 'chat', 'accessibility', 'language', 'font',
                     'packs', 'mods', 'graphics', 'graphics-advanced'):
            game.ctl('options', page)
            time.sleep(SETTLE)
            game.wait_targets()
            menu = game.menu()
            check(menu['undeclared'] == [], f"undeclared on {page}: {menu['undeclared']}")
            check(declared(game).get('Done') is True, f'{page} declares {declared(game)}')
        # A Graphics page's Done, drawn its own way, is pressed by B.
        game.ctl('options', 'graphics')
        time.sleep(SETTLE)
        game.wait_targets()
        game.pad('down')
        before = game.rects()
        game.pad('east')
        game.wait_targets()
        check(game.rects() != before, 'B did not press Done on Graphics')
    finally:
        game.quit()
    return check.failures


def split_setup(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.wait_targets()
        game.title_buttons()
        game.pad('west')
        game.ctl('options', 'horizon')
        time.sleep(SETTLE)
        game.wait_targets()
        click_widget(game, widget(game, 'split'))
        game.wait_targets()
        game.pad('down')
        check(declared(game).get('Done') is True, f'Setup declares {declared(game)}')
        check(game.menu()['undeclared'] == [], f"undeclared: {game.menu()['undeclared']}")
        # A held player needs a second one, which one virtual pad can't be: `split_ui`'s
        # done_is_declared_off_while_a_player_is_held has that.
        game.pad('east')
        game.wait_targets()
        check(any('split' in w['id'] for w in game.widgets()), f'B did not press Done: {labels(game)}')
    finally:
        game.quit()
    return check.failures


@title_case
def actions_follow_screen(game, check, single, options, quit_game):
    # X, then B before Options may have been drawn: B leaves Options either way, never the title's Quit Game.
    # Whether a frame gets between them is up to horizon-ctl's round trip; `widgets`' own test is exact.
    for attempt in range(5):
        game.ctl('pad', 'west', 'tap')
        game.ctl('pad', 'east', 'tap')
        time.sleep(1)
        game.wait_targets()
        check(quit_game in game.rects(), f'try {attempt}: B did not come back from Options')
        check(game.focus() != quit_game, f'try {attempt}: B went to the title\'s Quit Game')


@title_case
def glyphs_title(game, check, single, options, quit_game):
    game.pad('down')
    shown = glyphs(game)
    check(shown.get('West') == options, f'X on {shown}, Options at {options}')
    check(shown.get('East') == quit_game, f'B on {shown}, Quit Game at {quit_game}')
    game.pad('east')
    shown = glyphs(game)
    check(game.focus() == quit_game, 'B did not focus Quit Game')
    check(shown.get('South') == quit_game, f'A not on the focused exit: {shown}')
    check('East' not in shown, f'B still on it: {shown}')
    # The mouse hides them, the next pad input brings them back.
    game.ctl('click-at', 5, 5)
    time.sleep(SETTLE)
    check(glyphs(game) == {}, f'glyphs under the mouse: {glyphs(game)}')
    game.pad('down')
    check(glyphs(game) != {}, 'the pad did not bring the glyphs back')


@title_case
def glyph_on_field(game, check, single, options, quit_game):
    open_create_world(game)
    menu = game.menu()
    field = next(t for t in menu['targets'] if t['kind'] == 'field')
    for _ in range(6):
        if game.focus() == tuple(field['rect']):
            break
        game.pad('up')
    check(game.focus() == tuple(field['rect']), f"focus on {game.focus()}, not the field {field['rect']}")
    check(glyphs(game).get('South') == tuple(field['rect']), f'A not on the field: {glyphs(game)}')
    check(glyphs(game).get('East') is not None, f'B not on Cancel: {glyphs(game)}')
    # One keyboard, and cancelling it keeps the screen.
    game.pad('south')
    check(game.menu()['keyboard'] is True, 'A did not open the keyboard')
    check(glyphs(game) == {}, f'glyphs under the keyboard: {glyphs(game)}')
    game.pad('east')
    game.pad('east')
    check(game.menu()['keyboard'] is False, 'B did not close the keyboard')


def settings_apply(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        open_world_options(game)
        check('West' not in glyphs(game), f'X on with nothing changed: {glyphs(game)}')
        click_widget(game, widget(game, 'difficulty'))
        check(declared(game).get('Apply') is True, f'the change left Apply off: {declared(game)}')
        game.pad('left')
        check('West' in glyphs(game), f'no X with a change: {glyphs(game)}')
        game.pad('west')
        game.wait_targets()
        check('Exit' in declared(game), f'X did not apply and leave: {declared(game)}')
    finally:
        game.quit()
    return check.failures


LONG_DONE = 'Done with everything on this page, and then some more words so that it cannot possibly fit on the button'


def seed_long_done(game_home):
    pack = game_home / 'resourcepacks' / 'long'
    (pack / 'assets' / 'minecraft' / 'lang').mkdir(parents=True)
    (pack / 'pack.mcmeta').write_text(json.dumps({'pack': {'description': 'x', 'min_format': 1, 'max_format': 999999,
                                                           'pack_format': 1}}))
    (pack / 'assets' / 'minecraft' / 'lang' / 'en_us.json').write_text(json.dumps({'gui.done': LONG_DONE}))
    with (game_home / 'options.txt').open('a') as options:
        options.write('resourcePacks:["vanilla","file/long"]\n')


def png_rows(path):
    """A PNG's rows of bytes, its width and its pixel size (8-bit colour, no interlace)."""
    data = Path(path).read_bytes()
    at, idat, head = 8, b'', None
    while at < len(data):
        size = struct.unpack('>I', data[at:at + 4])[0]
        kind, body = data[at + 4:at + 8], data[at + 8:at + 8 + size]
        if kind == b'IHDR':
            head = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat += body
        at += 12 + size
    width, height, _, colour, *_ = head
    channels = {2: 3, 6: 4}[colour]
    raw, stride, rows, last = zlib.decompress(idat), width * channels, [], bytearray(width * channels)
    for y in range(height):
        kind = raw[y * (stride + 1)]
        row = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = row[i - channels] if i >= channels else 0
            b = last[i]
            c = last[i - channels] if i >= channels else 0
            if kind == 1:
                row[i] = (row[i] + a) & 255
            elif kind == 2:
                row[i] = (row[i] + b) & 255
            elif kind == 3:
                row[i] = (row[i] + (a + b) // 2) & 255
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                row[i] = (row[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(row))
        last = row
    return rows, width, channels


def long_label(args, name):
    game = Game(args, name, seed=seed_long_done)
    check = Check()
    try:
        game.wait_targets()
        game.title_buttons()
        game.pad('west')
        time.sleep(1)
        game.wait_targets()
        done = next((w for w in game.widgets() if w.get('label') == LONG_DONE), None)
        check(done is not None, f"the pack's label is not on the page: {[w.get('label') for w in game.widgets()]}")
        if done:
            shots = []
            for i in range(2):
                path = game.home / f'shot{i}.png'
                game.ctl('screenshot', path)
                shots.append(png_rows(path))
                time.sleep(1.5)
            x, y, w, h = done['bounds']
            scale = shots[0][1] / game.ctl('ui').get('width', shots[0][1])
            def crop(shot):
                rows, _, channels = shot
                left, right = int((x + 16) * scale) * channels, int((x + w - 2) * scale) * channels
                return [rows[int((y + 4) * scale) + j][left:right] for j in range(int((h - 8) * scale))]
            check(crop(shots[0]) != crop(shots[1]), 'the long label did not slide')
    finally:
        game.quit()
    return check.failures


@title_case
def settings(game, check, single, options, quit_game):
    game.pad('west')
    game.wait_targets()
    check(declared(game).get('Done') is True, f'Options declares {declared(game)}')
    check(game.menu()['undeclared'] == [], f"undeclared in Options: {game.menu()['undeclared']}")
    check(quit_game not in game.rects(), 'X did not open Options')
    game.pad('east')
    game.wait_targets()
    check(quit_game in game.rects(), 'B did not press Done')


@title_case
def create_world(game, check, single, options, quit_game):
    game.pad('south')
    game.pad('south')
    game.wait_targets()
    check(declared(game).get('Cancel') is True, f'Create World declares {declared(game)}')
    check(game.menu()['undeclared'] == [], f"undeclared in Create World: {game.menu()['undeclared']}")
    game.pad('east')
    game.wait_targets()
    check(quit_game in game.rects(), 'B did not press Cancel')


def world_options(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        game.ctl('click', 'world-options')
        time.sleep(SETTLE)
        game.wait_targets()
        game.pad('down')
        check(declared(game) == {'Apply': False, 'Cancel': True}, f'World Options declares {declared(game)}')
        check(game.menu()['undeclared'] == [], f"undeclared: {game.menu()['undeclared']}")
        before = game.rects()
        game.pad('west')
        check(game.rects() == before, 'X with nothing changed left the screen')
        game.pad('east')
        game.wait_targets()
        check(game.rects() != before, 'B did not press Cancel')
        check('Exit' in declared(game), 'B did not come back to the pause menu')
    finally:
        game.quit()
    return check.failures


def pause_y(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        rects = game.rects()
        widest = max(r[2] for r in rects)
        exit_button = max((r for r in rects if r[2] == widest), key=lambda r: r[1])
        game.pad('north')
        check(game.focus() == exit_button, f'focus is {game.focus()}, not the exit button {exit_button}')
        check(exit_button in game.rects(), 'Y left the pause menu')
        game.pad('start')
        game.pad('start')
        game.wait_targets()
        check(game.focus() not in (None, exit_button), f'reopened on {game.focus()}')
    finally:
        game.quit()
    return check.failures


CASES = {'idle_a': idle_a, 'idle_b': idle_b, 'idle_x': idle_x, 'release': release, 'move_cancels': move_cancels,
         'pad_release': pad_release, 'pause_y': pause_y, 'settings': settings, 'create_world': create_world,
         'world_options': world_options, 'key_row_b': key_row_b, 'lock_question': lock_question,
         'optimize_world': optimize_world, 'pointer_screens': pointer_screens, 'settings_pages': settings_pages,
         'split_setup': split_setup, 'actions_follow_screen': actions_follow_screen, 'glyphs_title': glyphs_title,
         'glyph_on_field': glyph_on_field, 'settings_apply': settings_apply, 'long_label': long_label}


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
        # A case that dies takes only itself down, and leaves no game behind it.
        try:
            failures = CASES[name](args, name)
        except Exception as error:
            failures = [f'{type(error).__name__}: {error}']
        for failure in failures:
            print(f'FAIL {name}: {failure}')
        if failures:
            failed.append(name)
        else:
            print(f'ok   {name}')
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
