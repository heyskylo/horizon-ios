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
- every_screen_has_targets: the title's submenus and every options page give the focus something to land on.
- pause_icon: the pause menu's icons are targets, and A presses one.
- studio_rows: the D-pad walks Replay Studio's recordings to its buttons, and B leaves.
- erase_checkbox: the D-pad reaches Optimize World's Erase Cached Data box, and A ticks it.
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
import zipfile
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
EXE = '.exe' if os.name == 'nt' else ''
SETTLE = 0.5


WINDOW_ASPECT = 16 / 9
WINDOW_ASPECT_SLACK = 0.1


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
        self.launch = ['launch', '--game', args.game, '--offscreen', '--render-distance', 2,
                       '--resources', args.resources]
        if world:
            self.launch += ['--world', 'Menus', '--creative', '--no-pause', '--wait']
        self.ctl(*self.launch, timeout=500)
        self.warn_if_reshaped()

    def warn_if_reshaped(self):
        """Hidden or not, a tiling compositor sizes the window, and these cases assume 16:9."""
        width, height = self.ctl('ui')['gui_size']
        if abs(width / height - WINDOW_ASPECT) > WINDOW_ASPECT_SLACK:
            print(f'warning: the window is {width:g}x{height:g} GUI pixels, not 16:9, and cases that scroll will '
                  "fail. On a tiling compositor, float the app id 'horizon'.", file=sys.stderr, flush=True)

    def restart(self):
        """Quit and start again on the same files."""
        self.ctl('quit')
        time.sleep(3)
        self.ctl(*self.launch, timeout=500)

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

    def pad(self, button, action='tap'):
        self.ctl('pad', button, action)
        time.sleep(SETTLE)

    def stick(self, y=-1):
        """About one notch of the right stick; down scrolls towards the list's end."""
        self.ctl('pad', 'stick', 'right', 0, y)
        time.sleep(0.1)
        self.ctl('pad', 'stick', 'right', 0, 0)
        time.sleep(SETTLE)

    def widgets(self):
        return self.ctl('ui')['widgets']

    def rows(self):
        return [w for w in self.widgets() if w['kind'] == 'row']

    def focused_label(self):
        """What the focus is on, by what it says: a scrolled row moves."""
        return next((w.get('label') for w in self.widgets() if tuple(w['bounds']) == self.focus()), None)

    def focus_on(self, wanted):
        """The D-pad, down (right along its row, or at the bottom) until the focus reaches this widget id."""
        # A long list scrolls under the D-pad on the way past.
        for _ in range(60):
            was = self.focus()
            # Scrolled out of view, it isn't drawn yet.
            bounds = next((tuple(w['bounds']) for w in self.widgets() if w['id'] == wanted), None)
            if was == bounds:
                return
            # In the row of the focus, it's beside it.
            if bounds and was and was[1] == bounds[1] and was[0] < bounds[0]:
                self.pad('right')
                continue
            self.pad('down')
            if self.focus() == was:
                self.pad('right')
        raise RuntimeError(f'the D-pad never reached {wanted}: focus on {self.focus()}')

    def press_through(self, *presses, click=None):
        """From the title, by the pad and then one click of a widget id, to a screen."""
        self.wait_targets()
        self.title_buttons()
        for press in presses:
            self.pad(press)
        if click:
            self.ctl('click', click)
            self.pad('down')
        self.wait_targets()


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


OPTIMIZE_REGIONS = 60000


def open_optimize_confirm(game):
    """From the title, with one saved world, to the screen with the Erase Cached Data box."""
    game.pad('south')
    game.pad('south')
    game.wait_targets()
    game.pad('down')
    click_widget(game, game.rows()[0])
    click_widget(game, next(w for w in game.widgets() if w.get('label') == 'Edit'))
    click_widget(game, next(w for w in game.widgets() if w.get('label') == 'Optimize World'))


def optimize_world(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        click_widget(game, next(w for w in game.widgets() if w.get('label') == 'Save and Quit to Title'))
        game.wait_targets()
        game.title_buttons()
        # A small world is done in a moment, and a fast machine does thousands of regions in one:
        # enough to read that it takes a while. Links, as copies this many would take longer than the job.
        source = max((game.game_home / 'saves' / 'Menus').rglob('region/*.mca'), key=lambda f: f.stat().st_size)
        for i in range(OPTIMIZE_REGIONS):
            os.link(source, source.parent / f'r.{100 + i}.100.mca')
        open_optimize_confirm(game)
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
        # X is its Options, declared with a glyph like the others.
        check(declared(game).get('Options') is True, f'Options is not declared: {declared(game)}')
        check('West' in glyphs(game), f'no X on Options: {glyphs(game)}')
        game.pad('west')
        check(game.ctl('state')['screen'] == 'options', f"X opened {game.ctl('state')['screen']}")
    finally:
        game.quit()
    return check.failures


def seed_worlds(game_home):
    """Enough worlds to scroll: a level.dat that doesn't read still lists its folder."""
    for i in range(20):
        (game_home / 'saves' / f'World{i:02}').mkdir(parents=True)
        (game_home / 'saves' / f'World{i:02}' / 'level.dat').write_bytes(b'')


def seed_servers(game_home):
    """Enough servers to scroll, none answering, with the warning before the list accepted."""
    def text(name, value):
        return b'\x08' + struct.pack('>H', len(name)) + name.encode() + struct.pack('>H', len(value)) + value.encode()
    rows = b''.join(text('name', f'Server{i:02}') + text('ip', '127.0.0.1:1') + b'\0' for i in range(20))
    root = b'\x0a\0\0\x09\0\x07servers\x0a' + struct.pack('>i', 20) + rows + b'\0'
    (game_home / 'servers.dat').write_bytes(root)
    with (game_home / 'options.txt').open('a') as options:
        options.write('skipMultiplayerWarning:true\n')


def quick_tap(game, side):
    """A trigger down and up, well inside the book's tap (`TRIGGER_TAP`)."""
    game.ctl('pad', 'trigger', side, 1)
    time.sleep(0.05)
    game.ctl('pad', 'trigger', side, 0)
    time.sleep(SETTLE)


def selected_tab(game):
    tabs = [tuple(t['rect']) for t in game.menu()['targets'] if str(t['kind']).startswith('tab')]
    chosen = [tuple(t['rect']) for t in game.menu()['targets'] if t['kind'] == 'tab*']
    return tabs.index(chosen[0]) if chosen else None, len(tabs)


@title_case
def tabs_create_world(game, check, single, options, quit_game):
    open_create_world(game)
    check(selected_tab(game) == (0, 3), f'starts on {selected_tab(game)}')
    for want in (1, 2, 0):
        game.pad('rb')
        check(selected_tab(game) == (want, 3), f'RB to {want}: {selected_tab(game)}')
    game.pad('lb')
    check(selected_tab(game) == (2, 3), f'LB wraps: {selected_tab(game)}')


@title_case
def tab_release(game, check, single, options, quit_game):
    open_create_world(game)
    game.pad('rb')
    game.pad('right')
    game.pad('south', 'down')
    check(selected_tab(game)[0] == 1, f'A down pressed the tab: {selected_tab(game)}')
    game.pad('south', 'up')
    check(selected_tab(game)[0] == 2, f'A up did not press the tab: {selected_tab(game)}')


@title_case
def tab_held_a(game, check, single, options, quit_game):
    open_create_world(game)
    game.pad('south', 'down')
    game.pad('rb')
    check(selected_tab(game)[0] == 0, f'RB with A held switched tab: {selected_tab(game)}')
    game.ctl('pad', 'release')


def tabs_world(args, name):
    game = Game(args, name, world=True)
    check = Check()

    def steps(screen, count):
        time.sleep(1.5)
        game.pad('rb')
        check(selected_tab(game) == (1, count), f'{screen} RB: {selected_tab(game)}')
        game.pad('lb')
        check(selected_tab(game) == (0, count), f'{screen} LB: {selected_tab(game)}')

    try:
        game.ctl('command', 'advancement grant @s everything')
        game.ctl('command', 'give @s dirt 4')
        time.sleep(1)
        game.ctl('key', 'q', 'tap')
        time.sleep(1)
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        game.ctl('click', 'statistics')
        # No mob has been killed: Mobs can't be chosen.
        steps('Statistics', 2)
        game.ctl('key', 'escape', 'tap')
        time.sleep(1)
        game.wait_targets()
        game.ctl('click', 'advancements')
        steps('Advancements', 5)
        game.ctl('key', 'escape', 'tap')
        time.sleep(1)
        game.ctl('key', 'escape', 'tap')
        time.sleep(1)
        game.ctl('key', 'p', 'tap')
        steps('Player Social', 3)
    finally:
        game.quit()
    return check.failures


def recipe_book(args, name):
    game = Game(args, name, world=True)
    check = Check()

    pull = lambda side: quick_tap(game, side)

    def arrows():
        # The page arrows: past the first page there's one to go back, to the left of the other.
        return sorted(t[0] for t in game.ctl('screen')['recipe_book']['targets'] if t[2:] == [12.0, 17.0])

    try:
        game.ctl('command', 'gamemode survival')
        game.ctl('command', 'recipe give @s *')
        time.sleep(1)
        game.ctl('key', 'e', 'tap')
        time.sleep(SETTLE)
        game.wait_targets()
        x, y, _, _ = next(t for t in game.ctl('screen')['recipe_book']['targets'] if t[2:] == [20.0, 18.0])
        game.ctl('click-at', x + 10, y + 9)
        time.sleep(SETTLE)
        category = selected_tab(game)
        check(category[0] is not None, 'the book has no tabs')
        first = arrows()
        check(len(first) == 1, f'not on a first page of several: {first}')
        forward = first[0]
        game.pad('lb')
        check(arrows() == first, f'LB on the first page moved it: {arrows()}')
        game.pad('rb')
        check(min(arrows()) < forward, f'RB did not turn the page: {arrows()}')
        check(selected_tab(game) == category, 'RB changed the category')
        pull('right')
        check(selected_tab(game)[0] != category[0], 'RT did not change the category')
        pull('left')
        check(selected_tab(game) == category, 'LT did not come back')
        check(arrows() == first, f'a new category kept the page: {arrows()}')
        # To the last page, and no further.
        for _ in range(30):
            if forward not in arrows():
                break
            game.pad('rb')
        last = arrows()
        check(forward not in last, f'never reached the last page: {last}')
        game.pad('rb')
        check(arrows() == last, f'RB on the last page moved it: {arrows()}')
    finally:
        game.quit()
    return check.failures


def stick_lists(args, name):
    check = Check()
    for seed, presses in ((seed_worlds, ('south', 'south')), (seed_servers, ('south', 'down', 'south'))):
        game = Game(args, name, seed=seed)
        try:
            game.press_through(*presses)
            # A row that leaves hands the focus to the nearest one, not the first target.
            game.focus_on(game.rows()[0]['id'])
            first = game.focused_label()
            for _ in range(6):
                game.stick()
            top = game.rows()[0]['label']
            check(top != first, f'{first} is still in the list')
            check(game.focused_label() == top, f'{first} left: focus on {game.focused_label()}, not the top row {top}')
            # A row that stays keeps it, whichever way the list goes.
            game.focus_on(game.rows()[1]['id'])
            label, before = game.focused_label(), game.rows()
            for y in (-1, -1, 1):
                game.stick(y)
                check(game.focused_label() == label, f'stick {y}: focus on {game.focused_label()}, not {label}')
            check(game.rows() != before, f'{label}: the list did not scroll')
            # With the focus, and the hidden pointer, outside the list.
            # The D-pad went through every hidden row to get there, so the list is at its end.
            game.focus_on('back')
            back, before = game.focus(), game.rows()
            game.stick()
            check(game.rows() == before, f'{label}: the list scrolled past its end')
            game.stick(1)
            check(game.rows() != before, f'{label}: the stick did not scroll with the focus on Back')
            check(game.focus() == back, f'{label}: focus went from Back to {game.focus()}')
        finally:
            game.quit()
    return check.failures


def stick_changes_no_value(args, name):
    check = Check()
    for presses, click, wanted, in_list in (
        (('south', 'south'), None, 'difficulty', False),
        (('west',), 'chat-settings', 'chat', True),
    ):
        game = Game(args, name)
        try:
            game.press_through(*presses, click=click)
            game.focus_on(wanted)
            label = game.focused_label()
            game.stick()
            check(game.focused_label() == label, f'{label} is now {game.focused_label()}')
            # The mouse wheel keeps vanilla's way: it cycles a button of the screen
            # itself, and a list keeps it to scroll past its rows (#9633).
            x, y, w, h = game.focus()
            # Up in the list: the stick scrolled it down, and the row stays in view.
            game.ctl('scroll-at', x + w / 2, y + h / 2, 1 if in_list else -1)
            time.sleep(SETTLE)
            if in_list:
                # The row moved with the list, so it is found by its value, not its place.
                row = next((w for w in game.widgets() if w.get('label') == label), None)
                check(row is not None, f'the mouse wheel cycled the list row {label}')
                check(row is None or row['bounds'][1] > y, f'the mouse wheel did not scroll past {label}')
            else:
                check(game.focused_label() != label, f'the mouse wheel no longer cycles {label}')
        finally:
            game.quit()
    return check.failures


def dpad_edge_changes_no_value(args, name):
    check = Check()
    game = Game(args, name)
    try:
        game.press_through('west', click='chat-settings')
        game.focus_on('chat')
        label = game.focused_label()
        game.pad('up')
        check(game.focused_label() == label, f'{label} is now {game.focused_label()}')
    finally:
        game.quit()
    # Past the top visible row, the step still scrolls the list it's in.
    game = Game(args, name)
    try:
        game.press_through('west', click='accessibility-settings')
        game.focus_on('menu-background-blur')
        for _ in range(4):
            game.stick()
        moved = False
        for _ in range(12):
            before = {w['id']: w for w in game.widgets()}
            game.pad('up')
            after = {w['id']: w for w in game.widgets()}
            both = before.keys() & after.keys()
            moved = moved or any(before[i]['bounds'] != after[i]['bounds'] for i in both)
            for i in both:
                check(before[i].get('label') == after[i].get('label'), f"{before[i].get('label')} is now {after[i].get('label')}")
        check(moved, 'the D-pad did not scroll the options list')
    finally:
        game.quit()
    # A list the focus isn't in stays put.
    game = Game(args, name, seed=seed_worlds)
    try:
        game.press_through('south', 'south')
        game.focus_on('back')
        back, before = game.focus(), game.rows()
        game.pad('down')
        check(game.rows() == before, 'the D-pad scrolled the list with the focus on Back')
        check(game.focus() == back, f'focus went from Back to {game.focus()}')
    finally:
        game.quit()
    return check.failures


def crosshair_on_target(game):
    x, y = game.menu()['pointer']
    return any(abs(r[0] + r[2] / 2 - x) <= 0.5 and abs(r[1] + r[3] / 2 - y) <= 0.5 for r in game.rects())


def crosshair_walk(game, x, y):
    """The D-pad, towards a point, until the crosshair is on the target there."""
    game.pad('down')
    for _ in range(40):
        at = game.menu()['pointer']
        dx, dy = x - at[0], y - at[1]
        if max(abs(dx), abs(dy)) <= 10:
            return
        game.pad(('right' if dx > 0 else 'left') if abs(dx) > abs(dy) else ('down' if dy > 0 else 'up'))
    raise RuntimeError(f'the D-pad never reached {x}, {y}: on {at}')


def crosshair_spreads(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'gamemode survival')
        game.ctl('command', 'give @s dirt 16')
        time.sleep(1)
        game.ctl('key', 'e', 'tap')
        time.sleep(SETTLE)
        game.wait_targets()
        for _ in range(12):
            game.pad('down')
        for _ in range(12):
            game.pad('left')
        game.pad('south')
        check(game.ctl('inventory')['carried'], 'A did not pick up the stack in the hotbar')
        game.pad('up')
        was = game.menu()['pointer']
        game.pad('south', 'down')
        game.pad('right')
        at = game.menu()['pointer']
        check(at[0] - was[0] == 18 and at[1] == was[1], f'A + right went from {was} to {at}, not one slot')
        game.pad('right')
        game.pad('south', 'up')
        filled = [s for s in game.ctl('inventory')['main'] if s and s.get('count')]
        check(len(filled) == 3, f'the stack spread over {len(filled)} slots, not 3')
    finally:
        game.quit()
    return check.failures


def crosshair_recipe_book(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'gamemode survival')
        game.ctl('key', 'e', 'tap')
        time.sleep(SETTLE)
        game.wait_targets()
        x, y, w, h = next(t for t in game.ctl('screen')['recipe_book']['targets'] if t[2:] == [20.0, 18.0])
        crosshair_walk(game, x + w / 2, y + h / 2)
        was = game.menu()['pointer']
        game.pad('south')
        check(game.menu()['pointer'] != was, 'the recipe book did not move the crosshair with its button')
        check(crosshair_on_target(game), f"the book opened: crosshair off every target, at {game.menu()['pointer']}")
    finally:
        game.quit()
    return check.failures


def crosshair_creative_tabs(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'e', 'tap')
        time.sleep(SETTLE)
        game.wait_targets()
        game.pad('down')
        game.pad('down')
        for _ in range(14):
            game.pad('rb')
            check(crosshair_on_target(game), f"tab {selected_tab(game)}: crosshair off every target, at {game.menu()['pointer']}")
    finally:
        game.quit()
    return check.failures


def crosshair_station_rows(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'gamemode survival')
        game.ctl('command', 'clear @s')
        game.ctl('command', 'give @s minecraft:cobbled_deepslate 3')
        x, y, z = (int(v // 1) for v in game.ctl('state')['player']['position'])
        # Terrain can hide it from where the player stands.
        game.ctl('command', f'fill {x - 1} {y} {z} {x + 1} {y + 2} {z + 3} minecraft:air')
        game.ctl('command', f'setblock {x} {y} {z + 2} minecraft:stonecutter')
        time.sleep(1)
        game.ctl('interact', x, y, z + 2)
        game.ctl('wait-for', 'screen', 'station', '--timeout', 10)
        time.sleep(SETTLE)
        hotbar = game.ctl('inventory')['hotbar']
        slot = next(i for i, s in enumerate(hotbar) if s and 'deepslate' in s.get('item', ''))
        game.ctl('click-slot', slot, 'shift')
        time.sleep(SETTLE)
        recipes = game.ctl('menu')['info']['recipes']
        check(len(recipes) > 12, f'cobbled deepslate has {len(recipes)} recipes: nothing to scroll')
        cell = next(t['rect'] for t in game.menu()['targets'] if t['rect'][2:] == [16.0, 18.0])
        crosshair_walk(game, cell[0] + 8, cell[1] + 9)
        # Down through the three visible rows and on through the hidden ones.
        rows = (len(recipes) + 3) // 4
        for _ in range(rows - 1):
            game.pad('down')
        check(game.menu()['pointer'][1] < cell[1] + 54, 'the crosshair left the list before its end')
        game.pad('south')
        picked = game.ctl('menu')['info']['selected']
        check(picked == (rows - 1) * 4, f'A picked recipe {picked}, not the first of the last row, {(rows - 1) * 4}')
        # The list's end: the next step leaves it.
        game.pad('down')
        check(game.menu()['pointer'][1] >= cell[1] + 54, 'the D-pad did not leave the list at its end')
    finally:
        game.quit()
    return check.failures


def options_case(button):
    def wrap(body):
        def case(args, name):
            game = Game(args, name)
            check = Check()
            try:
                game.press_through('west', click=button)
                body(game, check)
            finally:
                game.quit()
            return check.failures
        return case
    return wrap


@options_case('accessibility-settings')
def stick_keeps_option(game, check):
    game.focus_on('menu-background-blur')
    label, y = game.focused_label(), game.focus()[1]
    game.stick()
    check(game.focused_label() == label, f'focus on {game.focused_label()}, not {label}')
    check(game.focus()[1] < y, 'the list did not scroll')


@options_case('controls')
def stick_under_keyboard(game, check):
    game.pad('south')
    check(game.menu()['keyboard'], 'A on the search field did not open the on-screen keyboard')
    before = game.widgets()
    game.stick()
    check(game.widgets() == before, 'the list scrolled under the keyboard')


@options_case('video-settings')
def stick_leaves_sidebar(game, check):
    # Video Settings... is the graphics screen.
    game.pad('down')
    game.wait_targets()
    game.focus_on('advanced')
    label = game.focused_label()
    game.stick()
    check(game.focused_label() == label, f'focus went from {label} to {game.focused_label()}')


def focused_is_row(game):
    return any(tuple(w['bounds']) == game.focus() for w in game.rows())


def flat_rows_select(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.press_through('south', 'south')
        game.pad('rb')
        for _ in range(6):
            if 'Superflat' in next(w['label'] for w in game.widgets() if w['id'] == 'world-type'):
                break
            game.ctl('click', 'world-type')
            time.sleep(SETTLE)
        game.ctl('click', 'customize')
        game.pad('down')
        game.wait_targets()
        remove = lambda: next(w for w in game.widgets() if w.get('label') == 'Remove Layer')
        game.focus_on(game.rows()[0]['id'])
        check(focused_is_row(game), f'the focus is not on a row: {game.focused_label()}')
        check(remove().get('enabled', True), 'the focus on a layer did not select it')
    finally:
        game.quit()
    return check.failures


def walk_rows(game, check, name, rows):
    """From the top row the D-pad goes through every row, past where the list shows them, out and back up."""
    game.focus_on(game.rows()[0]['id'])
    seen = [game.focused_label()]
    for _ in range(rows * 3):
        game.pad('down')
        if not focused_is_row(game):
            break
        seen.append(game.focused_label())
    check(len(set(seen)) >= rows, f'{name}: down went through {len(set(seen))} rows, not {rows}: {seen}')
    check(not focused_is_row(game), f'{name}: down never left the list: {seen}')
    outside = game.focused_label()
    game.pad('up')
    check(game.focused_label() == seen[-1], f'{name}: up from {outside} is on {game.focused_label()}, not {seen[-1]}')
    for _ in range(rows * 3):
        if game.focused_label() == seen[0]:
            break
        game.pad('up')
    check(game.focused_label() == seen[0], f'{name}: up never got back to {seen[0]}: on {game.focused_label()}')


def dpad_reaches_rows(args, name):
    check = Check()
    for list_name, seed, presses in (('worlds', seed_worlds, ('south', 'south')),
                                     ('servers', seed_servers, ('south', 'down', 'south'))):
        game = Game(args, name, seed=seed)
        try:
            game.press_through(*presses)
            walk_rows(game, check, list_name, 20)
        finally:
            game.quit()
    for list_name, world_type, presets, count in (('flat presets', 'Superflat', True, 9),
                                                  ('single biome', 'Single Biome', False, 40)):
        game = Game(args, name)
        try:
            game.press_through('south', 'south')
            game.pad('rb')
            for _ in range(6):
                if world_type in next(w['label'] for w in game.widgets() if w['id'] == 'world-type'):
                    break
                game.ctl('click', 'world-type')
                time.sleep(SETTLE)
            for button in ('customize', 'presets') if presets else ('customize',):
                game.ctl('click', button)
                game.pad('down')
                game.wait_targets()
            rows = game.rows()
            check(rows, f'{list_name}: no rows in {game.widgets()}')
            if rows:
                walk_rows(game, check, list_name, count)
        finally:
            game.quit()
    return check.failures


@options_case('accessibility-settings')
def dpad_options_reach(game, check):
    """The options list scrolls under the D-pad to its last row."""
    seen = []
    for _ in range(40):
        seen.append(game.focused_label())
        game.pad('down')
        if game.focused_label() in seen:
            break
    check(any(label.startswith('High Contrast') for label in seen), f'never reached the last row: {seen}')
    check(len(set(seen)) > 12, f'only {seen}: rows below the view were skipped')


def stick_steps_once(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.wait_targets()
        single, _, _ = game.title_buttons()
        game.pad('south')
        was = game.focus()
        check(was == single, f'focus is {was}, not Singleplayer')
        # Past the threshold, just under it, and past again, faster than a held step repeats.
        for y in (-0.7, -0.45, -0.7, -0.45, -0.7):
            game.ctl('pad', 'stick', 'left', 0, y)
            time.sleep(0.03)
        game.ctl('pad', 'stick', 'left', 0, 0)
        time.sleep(SETTLE)
        moved = game.focus()
        check(moved != was, 'the stick did not step')
        game.pad('up')
        check(game.focus() == was, f'the stick stepped more than once: on {moved}')
    finally:
        game.quit()
    return check.failures


@options_case('video-settings')
def gui_scale_keeps_focus(game, check):
    # The graphics screen's General page.
    game.focus_on('gui-scale')
    label = game.focused_label() or ''
    check(label.startswith('GUI Scale'), f'never reached GUI Scale: on {label}')
    before = game.focus()
    # A cycle button: A steps the scale, and every widget moves.
    game.pad('south')
    time.sleep(SETTLE)
    check(game.focus() != before, f'the scale did not change: {label}, {before}')
    now = game.focused_label() or ''
    check(now.startswith('GUI Scale'), f'focus went from {label} to {now}')


def held_dpad_stops_at_slots(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'gamemode survival')
        game.ctl('key', 'e', 'tap')
        time.sleep(SETTLE)
        game.wait_targets()
        game.pad('down')
        for _ in range(4):
            game.pad('down')
        # A column with a gap above the inventory: nothing near over its top row.
        game.pad('right')
        game.pad('right')
        game.ctl('pad', 'up', 'down')
        time.sleep(1.5)
        stopped = game.menu()['pointer']
        time.sleep(0.5)
        check(game.menu()['pointer'] == stopped, 'the held D-pad was still moving')
        game.ctl('pad', 'up', 'up')
        time.sleep(SETTLE)
        slots = [t['rect'] for t in game.menu()['targets'] if t['kind'] == 'slot']
        at = [r for r in slots if abs(r[0] + 8 - stopped[0]) <= 1 and abs(r[1] + 8 - stopped[1]) <= 1]
        check(at, f'the held D-pad stopped off the slots, at {stopped}')
        game.pad('up')
        check(game.menu()['pointer'] != stopped, f'a fresh press did not cross from {stopped}')
    finally:
        game.quit()
    return check.failures



def shot(game, name):
    """A screenshot, kept when PAD_E2E_SHOTS names a folder."""
    folder = Path(game.args.shots or os.environ.get('PAD_E2E_SHOTS') or game.home)
    folder.mkdir(parents=True, exist_ok=True)
    time.sleep(0.3)
    game.ctl('screenshot', folder / f'{name}.png', timeout=120)


def has_label(game, text):
    return any(text in label for label in labels(game))


def open_controller(game, page=None):
    """Options by X, Controls by a click, then the pad alone to the controller settings."""
    game.press_through('west', click='controls')
    game.focus_on('controller-settings')
    game.pad('south')
    game.wait_targets()
    if page:
        for _ in range(page):
            game.pad('rb')


def controller_saved(game):
    path = game.game_home / 'horizon-controller.txt'
    return path.read_text() if path.exists() else ''


def controller_settings_pad(args, name):
    game = Game(args, name)
    check = Check()
    try:
        open_controller(game)
        for page in ('General', 'Bindings', 'Device'):
            check(has_label(game, page), f'no page {page} in {labels(game)}')
        check(has_label(game, 'Editing:'), 'the screen does not say whose profile it edits')
        # Exactly: the General page has its Navigation Repeat rows.
        check('Navigation' not in labels(game), 'a navigation page contradicts menus always focusing')
        shot(game, 'controller-general')
        # The D-pad walks the whole list, scrolling it.
        game.pad('right')
        reached = {game.focused_label()}
        for _ in range(24):
            game.pad('down')
            reached.add(game.focused_label())
        for want in ('Controller', 'Look Sensitivity', 'Move Deadzone', 'Trigger Threshold', 'Repeat Interval',
                     'On-Screen Keyboard'):
            check(any(want.lower() in (r or '').lower() for r in reached), f'the D-pad never reached {want}: {reached}')
        # The bumpers change pages.
        game.pad('rb')
        check(has_label(game, 'key.') or has_label(game, 'Jump'), f'the bumper did not open Bindings: {labels(game)}')
        shot(game, 'controller-bindings')
        # Through every row of Bindings and its reset at the end.
        game.pad('right')
        reached = set()
        # Up to the row that leads the page, then down all of it.
        for _ in range(6):
            reached.add(game.focused_label())
            # Past it the focus leaves the list for the search box and the pages.
            if 'jump' in (game.focused_label() or '').lower():
                break
            game.pad('up')
        for _ in range(60):
            reached.add(game.focused_label())
            game.pad('down')
        for want in ('Sneak', 'Reset'):
            check(any(want.lower() in (r or '').lower() for r in reached), f'the D-pad never reached {want}: {reached}')
        shot(game, 'controller-bindings-end')
        game.pad('rb')
        game.pad('right')
        check(has_label(game, 'Player profile'), f'the bumper did not open Device: {labels(game)}')
        check(has_label(game, 'Detected glyphs'), 'no detected glyphs on Device')
        check(has_label(game, 'Button Glyphs'), 'no glyph override on Device')
        shot(game, 'controller-device')
        # Vibration changes and saves; Undo appears, X takes it back and it goes.
        check(not has_label(game, 'Undo'), 'Undo with nothing to undo')
        for _ in range(16):
            if 'vibration' in (game.focused_label() or '').lower():
                break
            game.pad('down')
        check('vibration' in (game.focused_label() or '').lower(), f'never reached Vibration: {game.focused_label()}')
        game.pad('south')
        check(has_label(game, 'Undo'), 'no Undo after a change')
        check('vibration:false' in controller_saved(game), f'not saved: {controller_saved(game)}')
        shot(game, 'controller-pending')
        game.pad('west')
        check(not has_label(game, 'Undo'), 'Undo stayed after undoing')
        check('vibration:false' not in controller_saved(game), 'the undo was not saved')
        game.pad('east')
        game.wait_targets()
        check(not has_label(game, 'Editing:'), 'B did not leave the screen')
    finally:
        game.quit()
    return check.failures


def controller_bind(args, name):
    game = Game(args, name)
    check = Check()
    try:
        open_controller(game, 1)
        # Right of the page's tab is the row level with it, the second.
        game.pad('right')
        for _ in range(8):
            if 'jump' in (game.focused_label() or '').lower():
                break
            game.pad('up')
        check('jump' in (game.focused_label() or '').lower(), f'never reached Jump: {game.focused_label()}')
        game.pad('south')
        check(has_label(game, '> '), f'A did not start the wait: {labels(game)}')
        game.pad('start')
        check(not has_label(game, '> '), 'Start did not cancel the wait')
        check(has_label(game, 'Editing:'), 'Start left the screen instead of cancelling')
        game.pad('south')
        game.pad('rs')
        check('key.jump' in controller_saved(game), f'the binding was not saved: {controller_saved(game)}')
        shot(game, 'controller-bound')
        game.pad('right')
        game.pad('south')
        check('key.jump' not in controller_saved(game), f'reset did not clear it: {controller_saved(game)}')
        check((game.menu()['focus'] or {}).get('kind') != 'field', f'reset sent the focus to the search: {game.menu()["focus"]}')
    finally:
        game.quit()
    return check.failures


def controller_settings_keys(args, name):
    # Keys go through `key`, which needs a world open.
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('options', 'controller')
        game.wait_targets()
        game.ctl('click', 'device')
        time.sleep(SETTLE)
        check(has_label(game, 'Player profile'), f'a click did not open Device: {labels(game)}')
        # Opened afresh, nothing has the focus: the first Tab lands on the search.
        game.ctl('key', 'esc', 'tap')
        time.sleep(SETTLE)
        game.ctl('options', 'controller')
        game.wait_targets()
        game.ctl('key', 'tab', 'tap')
        game.ctl('type', 'rumble')
        time.sleep(SETTLE)
        check(has_label(game, 'Vibration'), f'the search did not find Vibration by what it does: {labels(game)}')
        check(not has_label(game, 'Look Sensitivity'), 'the search kept other rows')
        # The focus off the field and back: a click at the field's end must not clear it.
        game.ctl('key', 'down', 'tap')
        game.ctl('key', 'up', 'tap')
        time.sleep(SETTLE)
        check(not has_label(game, 'Look Sensitivity'), 'coming back to the search cleared it')
        game.ctl('key', 'esc', 'tap')
        time.sleep(SETTLE)
        check(not has_label(game, 'Editing:'), 'Escape did not leave the screen')
    finally:
        game.quit()
    return check.failures


OPTIONS_PAGES = ('main', 'video', 'sound', 'controls', 'keybinds', 'controller', 'controller-binds', 'mouse', 'chat',
                 'accessibility', 'language', 'font', 'packs', 'mods', 'shader-packs', 'graphics', 'graphics-advanced',
                 'graphics-hdr', 'extras', 'pvp', 'credits', 'horizon', 'agent')
# Screens allowed to draw no target, and why. The pad falls back to the crosshair on these.
TARGETS_WAIT = 5
STUDIO_RECORDINGS, STUDIO_BUTTONS = 12, 3


def check_has_targets(game, check, screen):
    """A focus-mode screen draws a target soon after it opens; pointer screens (packs) use the crosshair by design."""
    end = time.time() + TARGETS_WAIT
    while True:
        menu = game.menu()
        if menu['mode'] != 'focus' or menu['targets']:
            return
        if time.time() > end:
            check(False, f'{screen} has no targets: a controller is left with the crosshair')
            return
        time.sleep(0.2)


@title_case
def every_screen_has_targets(game, check, single, options, quit_game):
    for screen, steps in (('create world', ['singleplayer']), ('multiplayer warning', ['multiplayer']),
                          ('multiplayer', ['multiplayer', 'proceed']), ('realms', ['realms'])):
        for step in steps:
            click_widget(game, widget(game, step))
        check(quit_game not in game.rects(), f'{screen} did not open')
        check_has_targets(game, check, screen)
        for _ in range(4):
            if quit_game in game.rects():
                break
            # Not the Escape key: horizon-ctl's needs a world open.
            game.pad('east')
            time.sleep(SETTLE)
        else:
            raise RuntimeError(f'B did not come back to the title from {screen}')
    game.pad('west')
    for page in OPTIONS_PAGES:
        game.ctl('options', page)
        time.sleep(SETTLE)
        check_has_targets(game, check, page)
    game.ctl('options', 'horizon')
    time.sleep(SETTLE)
    click_widget(game, widget(game, 'studio'))
    check_has_targets(game, check, 'studio')


def seed_recordings(game_home):
    """More saved recordings than the Studio page shows at once."""
    folder = game_home / 'recordings'
    folder.mkdir()
    for i in range(STUDIO_RECORDINGS):
        with zipfile.ZipFile(folder / f'run{i:02}.mcpr', 'w') as archive:
            archive.writestr('metaData.json', json.dumps({'serverName': f'Server{i:02}', 'date': i, 'duration': 1000}))


def studio_rows(args, name):
    """The D-pad walks Replay Studio's recordings, scrolling the hidden ones, to its buttons."""
    game = Game(args, name, seed=seed_recordings)
    check = Check()
    try:
        game.wait_targets()
        game.ctl('options', 'horizon')
        time.sleep(SETTLE)
        click_widget(game, widget(game, 'studio'))
        time.sleep(SETTLE)
        game.wait_targets()
        first = game.rows()[0]['label']
        for _ in range(4):
            game.stick()
        check(game.rows()[0]['label'] != first, f'the right stick did not scroll the list: {first} still on top')
        for _ in range(8):
            game.stick(1)
        check(game.rows()[0]['label'] == first, f'the right stick did not scroll back: {game.rows()[0]["label"]} on top')
        kinds = [t['kind'] for t in game.menu()['targets']]
        check(kinds.count('row') > 0 and kinds.count('row') < STUDIO_RECORDINGS, f'recordings in view: {kinds}')
        check(kinds.count('button') == STUDIO_BUTTONS, f'its buttons: {kinds}')
        game.pad('down')
        walked = set()
        for _ in range(STUDIO_RECORDINGS + STUDIO_BUTTONS):
            walked.add(game.focused_label())
            before = game.focus()
            kind = next((t['kind'] for t in game.menu()['targets'] if tuple(t['rect']) == before), None)
            if kind == 'button':
                break
            game.pad('down')
        check(kind == 'button', f'the D-pad never left the list for the buttons: on {kind}')
        check(len(walked) >= STUDIO_RECORDINGS, f'the D-pad skipped hidden recordings: {sorted(map(str, walked))}')
        check(game.menu()['actions'], 'Done declares no button')
        game.pad('east')
        time.sleep(SETTLE)
        check(not any(w['id'] == 'open-folder' for w in game.widgets()), 'B stayed on the Studio page')
    finally:
        game.quit()
    return check.failures


def pause_icon(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        # Report Bugs, Feedback and Player Reporting, and Friends when it is live.
        icons = sorted(r for r in game.rects() if r[2:] == (20, 20))
        check(len(icons) >= 3, f'pause icons with targets: {icons}')
        game.focus_on(next(w['id'] for w in game.widgets() if tuple(w['bounds']) == icons[-1]))
        before = game.rects()
        game.pad('south')
        check(game.rects() != before, 'A did not press Player Reporting')
    finally:
        game.quit()
    return check.failures


def controller_notices(args, name):
    game = Game(args, name, world=True)
    check = Check()
    notice = lambda: game.ctl('pad', 'status').get('notice')
    try:
        for _ in range(10):
            game.ctl('pad', 'connection', 'disconnected')
            game.ctl('pad', 'connection', 'connected')
        game.ctl('pad', 'connection', 'disconnected')
        time.sleep(1)
        check(notice() and 'disconnected' in notice()['title'], f'not the latest event: {notice()}')
        shot(game, 'notice-disconnected')
        game.ctl('pad', 'connection', 'connected')
        time.sleep(1)
        check(notice() and notice()['title'] == 'Controller connected', f'the reconnect did not replace it: {notice()}')
        shot(game, 'notice-connected')
        time.sleep(6.5)
        check(notice() is None, f'the notice outstayed its time: {notice()}')
    finally:
        game.quit()
    return check.failures


def erase_checkbox(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'escape', 'tap')
        game.wait_targets()
        click_widget(game, next(w for w in game.widgets() if w.get('label') == 'Save and Quit to Title'))
        game.wait_targets()
        game.title_buttons()
        open_optimize_confirm(game)
        box = next(w for w in game.widgets() if 'label' not in w)
        # The box is the screen's topmost target.
        for _ in range(4):
            if game.focus() == tuple(box['bounds']):
                break
            game.pad('up')
        check(game.focus() == tuple(box['bounds']), f"the D-pad stopped on {game.focus()}, not {box['bounds']}")
        path = game.home / 'box.png'

        def shot():
            game.ctl('screenshot', path)
            rows, width, channels = png_rows(path)
            scale = width / game.ctl('ui')['gui_size'][0]
            x, y, h = (int(box['bounds'][i] * scale) for i in (0, 1, 3))
            return [row[x * channels:(x + h) * channels] for row in rows[y:y + h]]

        # Ticked, its sprite changes under the same hover.
        unticked = shot()
        game.pad('south')
        check(shot() != unticked, 'A did not tick Erase Cached Data')
    finally:
        game.quit()
    return check.failures



def key_row_a_x(args, name):
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
        row = next(w for w in game.widgets() if w.get('label') in ('W', 'A', 'S', 'D'))
        click_widget(game, row)
        waiting = next(w for w in game.widgets() if w['bounds'] == row['bounds'])['label']
        check(waiting != row['label'], f"the row isn't waiting: {waiting}")
        for button in ('south', 'west', 'north', 'rb'):
            game.pad(button)
            after = next(w for w in game.widgets() if w['bounds'] == row['bounds'])['label']
            check(after == waiting, f'{button} left the row on {after}')
        game.pad('east')
        after = next(w for w in game.widgets() if w['bounds'] == row['bounds'])['label']
        check(after not in (waiting, 'Left Button', 'Right Button'), f'B left the row on {after}')
    finally:
        game.quit()
    return check.failures


def show_dialog(game, snbt):
    """A server's dialog over the world, once it is drawn."""
    game.ctl('command', f'dialog show @s {snbt}')
    time.sleep(1)
    check_open = game.ctl('state')['dialog']['open']
    if not check_open:
        raise RuntimeError(f'no dialog from {snbt}')
    game.wait_targets()


def dialog_open(game):
    return game.ctl('state')['dialog']['open'] is not None


def prompt_dialog(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        show_dialog(game, '{type:"minecraft:confirmation",title:"Sure?",yes:{label:"Yes"},no:{label:"No"}}')
        check(game.ctl('state')['screen'] == 'dialog', f"screen is {game.ctl('state')['screen']}")
        # The first A only lands the focus, on what answers no.
        game.pad('south')
        check(dialog_open(game), 'the first A answered the dialog')
        check(game.focused_label() == 'No', f'the focus starts on {game.focused_label()}')
        check(glyphs(game).get('East') is not None, f'no B on No: {glyphs(game)}')
        # The stick drives the focus: the player stays.
        before = game.ctl('state')['player']['position']
        game.ctl('pad', 'stick', 'left', -1, 0)
        time.sleep(0.6)
        game.ctl('pad', 'stick', 'left', 0, 0)
        time.sleep(SETTLE)
        check(game.ctl('state')['player']['position'] == before, 'the stick walked the player behind the dialog')
        check(game.focused_label() == 'Yes', f'the stick left the focus on {game.focused_label()}')
        game.pad('right')
        check(game.focused_label() == 'No', f'the D-pad left the focus on {game.focused_label()}')
        # A presses it on the way up.
        game.ctl('pad', 'south', 'down')
        time.sleep(SETTLE)
        check(dialog_open(game), 'A down answered the dialog')
        game.ctl('pad', 'south', 'up')
        time.sleep(SETTLE)
        check(not dialog_open(game), 'A up did not answer the dialog')
        check(game.ctl('state')['screen'] == 'playing', f"screen is {game.ctl('state')['screen']}")
        # B answers as Escape does, and doesn't open the pause menu as well.
        show_dialog(game, '{type:"minecraft:notice",title:"Hello"}')
        game.pad('east')
        check(not dialog_open(game), 'B did not close the notice')
        check(game.ctl('state')['screen'] == 'playing', f"B left the screen on {game.ctl('state')['screen']}")
        # One that Escape can't close stays, and doesn't pause either; A still answers it.
        show_dialog(game, '{type:"minecraft:notice",title:"Stay",can_close_with_escape:false}')
        game.pad('east')
        game.pad('start')
        check(dialog_open(game), 'B or Start closed a dialog that Escape cannot')
        game.pad('south')
        game.pad('south')
        check(not dialog_open(game), 'A did not answer it')
    finally:
        game.quit()
    return check.failures


def prompt_dialog_field(args, name):
    game = Game(args, name, world=True)
    check = Check()

    def typed():
        return game.ctl('state')['dialog']['open']['inputs'][0]['value']

    try:
        show_dialog(game, '{type:"minecraft:notice",title:"Name",inputs:[{type:"minecraft:text",key:"who",label:"Name"}]}')
        game.pad('south')
        field = next(t for t in game.menu()['targets'] if t['kind'] == 'field')
        check(game.focus() == tuple(field['rect']), f"the focus starts on {game.focus()}, not {field['rect']}")
        game.pad('south')
        check(game.menu()['keyboard'], 'A did not open the keyboard')
        game.pad('south')
        check(len(typed()) == 1, f'A on a key typed {typed()!r}')
        game.pad('south')
        check(len(typed()) == 2, f'A on a key again typed {typed()!r}')
        game.pad('west')
        check(len(typed()) == 1, f'X left {typed()!r}')
        check(game.menu()['keyboard'], 'typing closed the keyboard')
        game.pad('start')
        check(not game.menu()['keyboard'], 'Start did not end the typing')
        check(dialog_open(game) and len(typed()) == 1, 'Start answered the dialog')
        game.pad('east')
        check(not dialog_open(game), 'B did not close the dialog')
    finally:
        game.quit()
    return check.failures


def walk_to_field(game, check):
    """The D-pad, towards the screen's text field."""
    game.pad('down')
    field = tuple(next(t for t in game.menu()['targets'] if t['kind'] == 'field')['rect'])
    for _ in range(40):
        focus = game.focus()
        if focus == field:
            return
        if focus[1] != field[1]:
            game.pad('up' if focus[1] > field[1] else 'down')
        else:
            game.pad('left' if focus[0] > field[0] else 'right')
    check(game.focus() == field, f'the D-pad never reached the field {field}: on {game.focus()}')


def prompt_keeps_typing(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.pad('up')
        time.sleep(SETTLE)
        check(game.ctl('state')['screen'] == 'chat', f"D-pad up left the screen on {game.ctl('state')['screen']}")
        check(game.menu()['keyboard'], 'the keyboard did not open over chat')
        show_dialog(game, '{type:"minecraft:notice",title:"Hello"}')
        check(not game.menu()['keyboard'], 'the keyboard stayed over the dialog')
        game.pad('east')
        time.sleep(SETTLE)
        check(not dialog_open(game), 'B did not close the notice')
        check(game.menu()['keyboard'], 'the keyboard did not come back over chat')
        # Closed by B, it stays closed through the next prompt.
        game.pad('east')
        time.sleep(SETTLE)
        check(not game.menu()['keyboard'], 'B did not close the keyboard')
        show_dialog(game, '{type:"minecraft:notice",title:"Hello"}')
        game.pad('east')
        time.sleep(SETTLE)
        check(not dialog_open(game), 'B did not close the second notice')
        check(not game.menu()['keyboard'], 'a prompt reopened the keyboard B had closed')
    finally:
        game.quit()
    return check.failures


def prompt_keeps_crosshair(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'gamemode survival')
        game.ctl('key', 'e', 'tap')
        time.sleep(SETTLE)
        game.wait_targets()
        screen = game.ctl('state')['screen']
        game.pad('down')
        time.sleep(1)
        was = game.menu()['pointer']
        show_dialog(game, '{type:"minecraft:notice",title:"Hello"}')
        time.sleep(SETTLE)
        check(game.menu()['pointer'] != was, 'the focus on the notice left the pointer on the slot')
        game.pad('east')
        time.sleep(1)
        check(not dialog_open(game), 'B did not close the notice')
        check(game.ctl('state')['screen'] == screen, f"the notice left the screen on {game.ctl('state')['screen']}")
        at = game.menu()['pointer']
        check(max(abs(at[0] - was[0]), abs(at[1] - was[1])) <= 1, f'the crosshair went from {was} to {at}')
    finally:
        game.quit()
    return check.failures


def osk_stays_open(game, check, what):
    """A opens the keyboard on the focused field, and it types and deletes without closing."""
    game.pad('south')
    check(game.menu()['keyboard'], f'{what}: A did not open the keyboard')
    game.pad('south')
    check(game.menu()['keyboard'], f'{what}: typing closed the keyboard')
    game.pad('west')
    check(game.menu()['keyboard'], f'{what}: deleting closed the keyboard')


def osk_social(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'p', 'tap')
        time.sleep(1)
        game.wait_targets()
        walk_to_field(game, check)
        osk_stays_open(game, check, 'social')
        game.pad('start')
        check(not game.menu()['keyboard'], 'Start did not end the typing')
        check(game.ctl('state')['screen'] == 'social', f"Start left {game.ctl('state')['screen']}")
    finally:
        game.quit()
    return check.failures


def osk_debug_options(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 'f3', 'down')
        game.ctl('key', 'f6', 'tap')
        game.ctl('key', 'f3', 'up')
        time.sleep(1)
        check(game.ctl('state')['screen'] == 'debug-options', f"screen is {game.ctl('state')['screen']}")
        game.wait_targets()
        walk_to_field(game, check)
        rows = len(game.rects())
        game.pad('south')
        check(game.menu()['keyboard'], 'A did not open the keyboard')
        # Few of the options have the first key's letter in them.
        game.pad('south')
        check(game.menu()['keyboard'], 'typing closed the keyboard')
        check(len(game.rects()) < rows, f'the search kept {len(game.rects())} of {rows} targets')
        game.pad('west')
        check(len(game.rects()) == rows, f'deleting left {len(game.rects())} of {rows} targets')
    finally:
        game.quit()
    return check.failures


def osk_block(block, screen):
    def case(args, name):
        game = Game(args, name, world=True)
        check = Check()
        try:
            # In front of the eyes, looking east.
            game.ctl('command', f'setblock ~1 ~1 ~ {block}')
            game.ctl('look', -90, 0)
            time.sleep(1)
            game.ctl('mouse', 'right', 'click')
            time.sleep(1)
            check(game.ctl('state')['screen'] == screen, f"screen is {game.ctl('state')['screen']}")
            game.wait_targets()
            walk_to_field(game, check)
            osk_stays_open(game, check, screen)
            # Enter is the screen's Done, as on the keyboard.
            game.pad('start')
            check(game.ctl('state')['screen'] == 'playing', f"Start left {game.ctl('state')['screen']}")
            check(not game.menu()['keyboard'], 'the keyboard stayed after its screen closed')
        finally:
            game.quit()
        return check.failures
    return case


osk_command_block = osk_block('command_block', 'command-block')


osk_jigsaw = osk_block('jigsaw', 'block-editor')


def seed_controller(text):
    def seed(game_home):
        (game_home / 'horizon-controller.txt').write_text(text)
    return seed


def glyph_art(args, name):
    check = Check()
    for style, west, east in (('xbox', 'xb_x', 'xb_b'), ('playstation', 'psx_square', 'psx_circle'), ('nintendo', 'nx_y', 'nx_a')):
        game = Game(args, f'{name}-{style}', seed=seed_controller(f'glyphs:{style}\n'))
        try:
            game.wait_targets()
            _, options, quit_game = game.title_buttons()
            game.pad('down')
            shown = {g['button']: g for g in game.menu()['glyphs']}
            for button, art, widget in (('West', west, options), ('East', east, quit_game)):
                glyph = shown.get(button)
                check(glyph is not None, f'{style}: no {button} glyph in {shown}')
                if glyph:
                    sprite = art and f'horizon:gui/controller/{art}'
                    check(glyph['sprite'] == sprite, f"{style} {button}: {glyph['sprite']}, not {sprite}")
                    x, y, w, h = glyph['rect']
                    bx, by, bw, bh = widget
                    check(bx <= x and by <= y and x + w <= bx + bw and y + h <= by + bh,
                          f'{style} {button}: {glyph["rect"]} is not inside its button {widget}')
            shot(game, f'glyph_art_{style}_title')
        finally:
            game.quit()
    return check.failures


def glyph_art_world(args, name):
    check = Check()
    for style in ('xbox', 'playstation'):
        game = Game(args, f'{name}-{style}', world=True, seed=seed_controller(f'glyphs:{style}\nbindLayout:1\n'))
        try:
            game.ctl('key', 'e', 'tap')
            time.sleep(SETTLE)
            game.wait_targets()
            game.pad('down')
            check(game.menu()['pointer'] is not None, f'{style}: no crosshair in the inventory')
            shot(game, f'glyph_art_{style}_crosshair')
        finally:
            game.quit()
    return check.failures


def controller_trigger_threshold(args, name):
    game = Game(args, name)
    check = Check()
    try:
        open_controller(game)
        game.pad('right')
        for _ in range(24):
            if 'trigger threshold' in (game.focused_label() or '').lower():
                break
            game.pad('down')
        check('40%' in (game.focused_label() or ''), f'not on its default: {game.focused_label()}')
        # A step of the D-pad is a pixel of the slider, which can be less than a percent.
        for _ in range(6):
            game.pad('right')
            if '40%' not in (game.focused_label() or ''):
                break
        check('40%' not in (game.focused_label() or ''), f'right left it on {game.focused_label()}')
        saved = next((line for line in controller_saved(game).splitlines() if line.startswith('triggerThreshold:')), '')
        check(saved and float(saved.split(':')[1]) > 0.4, f'not saved: {saved}')
    finally:
        game.quit()
    return check.failures



CASES = {'idle_a': idle_a, 'idle_b': idle_b, 'idle_x': idle_x, 'release': release, 'move_cancels': move_cancels,
         'pad_release': pad_release, 'pause_y': pause_y, 'settings': settings, 'create_world': create_world,
         'world_options': world_options, 'key_row_b': key_row_b, 'lock_question': lock_question,
         'optimize_world': optimize_world, 'pointer_screens': pointer_screens, 'settings_pages': settings_pages,
         'split_setup': split_setup, 'actions_follow_screen': actions_follow_screen, 'glyphs_title': glyphs_title,
         'glyph_on_field': glyph_on_field, 'settings_apply': settings_apply, 'long_label': long_label,
         'tabs_create_world': tabs_create_world,
         'tab_release': tab_release,
         'tab_held_a': tab_held_a,
         'tabs_world': tabs_world,
         'recipe_book': recipe_book,
         'stick_lists': stick_lists,
         'stick_changes_no_value': stick_changes_no_value,
         'dpad_edge_changes_no_value': dpad_edge_changes_no_value,
         'stick_keeps_option': stick_keeps_option,
         'crosshair_spreads': crosshair_spreads,
         'crosshair_recipe_book': crosshair_recipe_book,
         'crosshair_creative_tabs': crosshair_creative_tabs,
         'crosshair_station_rows': crosshair_station_rows,
         'stick_under_keyboard': stick_under_keyboard,
         'stick_leaves_sidebar': stick_leaves_sidebar,
         'dpad_reaches_rows': dpad_reaches_rows,
         'dpad_options_reach': dpad_options_reach,
         'stick_steps_once': stick_steps_once,
         'gui_scale_keeps_focus': gui_scale_keeps_focus,
         'held_dpad_stops_at_slots': held_dpad_stops_at_slots,
         'flat_rows_select': flat_rows_select,
         'controller_settings_pad': controller_settings_pad,
         'controller_bind': controller_bind,
         'controller_settings_keys': controller_settings_keys,
         'controller_notices': controller_notices,
         'every_screen_has_targets': every_screen_has_targets,
         'studio_rows': studio_rows, 'pause_icon': pause_icon,
         'erase_checkbox': erase_checkbox,
         'glyph_art': glyph_art,
         'glyph_art_world': glyph_art_world,
         'key_row_a_x': key_row_a_x,
         'prompt_dialog': prompt_dialog,
         'prompt_dialog_field': prompt_dialog_field,
         'prompt_keeps_typing': prompt_keeps_typing,
         'prompt_keeps_crosshair': prompt_keeps_crosshair,
         'osk_social': osk_social,
         'osk_debug_options': osk_debug_options,
         'osk_command_block': osk_command_block,
         'osk_jigsaw': osk_jigsaw,
         'controller_trigger_threshold': controller_trigger_threshold}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--game', type=Path, default=lab.cargo_target() / f'fast/horizon-client{EXE}')
    parser.add_argument('--ctl', type=Path, default=lab.cargo_target() / f'fast/horizon-ctl{EXE}')
    parser.add_argument('--resources', type=Path, default=ROOT / '.local/horizon-client/26.3')
    parser.add_argument('--only', nargs='*', choices=sorted(CASES), help='run only these cases')
    parser.add_argument('--shots', type=Path, help='keep screenshots in this folder')
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
