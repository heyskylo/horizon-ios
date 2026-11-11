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
- trigger_rules: over the crafting book RT changes the category as it goes down, LT as it comes up (however slowly) unless
  A was pressed meanwhile.
- craft_direct, craft_remainder, craft_full, craft_grid, craft_carrying: LT + A on a recipe, tapped and held: results,
  what a recipe leaves behind, a full inventory, a grid already in use, a stack in the hand.
- craft_tap_hold: LT + A tapped crafts one, whichever comes up first; held, several, stopping with the items.
- craft_release_a, craft_release_lt, craft_release_press: a held craft ends cleanly as A or LT comes up or another
  button goes down: no craft running, an empty grid, every item accounted for, and the next press starts fresh.
- craft_lan_guest, craft_dedicated: the same on a LAN host and a dedicated server, as a real server's client, and
  quick taps of A with LT held while a paced craft is in flight, each making one; joined by address, the LAN world
  refuses it (plain A) until Quick Craft on Servers is on.
- long_label: a label too wide for its button slides, beside its glyph.
- every_screen_has_targets: the title's submenus and every options page give the focus something to land on.
- pause_icon: the pause menu's icons are targets, and A presses one.
- studio_rows: the D-pad walks Replay Studio's recordings to its buttons, and B leaves.
- erase_checkbox: the D-pad reaches Optimize World's Erase Cached Data box, and A ticks it.
- controller_settings_pad: Controller Settings on the settings shell, by the pad alone: its pages, the D-pad through
  every page and its rows, the strip describing the focused row, A changing Vibration, X undoing it, B leaving.
- controller_bind: A on Bindings' Jump waits for a button (the row and the strip say so, Start cancels and leaves no
  Undo), the next button binds and saves it with an Undo, Right and A reset it and the Undo goes.
- controller_settings_keys: the keys: Tab to the search, typing finds a row by what it does, the focus off and back
  keeps the search, a pad press gives the keys back, Escape leaves.
- controller_nintendo_buttons: with Nintendo glyphs, A on Device's Nintendo Buttons picks By Letter and saves it, and
  again goes back to By Position.
- nintendo_letters: a Nintendo pad going by its letters shows the letter of the button that acts on the title's
  declared buttons (Options is X, Quit Game's B), and by places Y and A, and the guide's Jump is A or B; the guide
  and a connected pad's notice are shot in both modes.
- extras_shell: Horizon Extras on the settings shell: the D-pad walks its rows and scrolls through the hidden ones, past
  a greyed row, the right stick scrolls it, A changes and saves a toggle, the strip follows the row, B leaves.
- graphics_shell: the Graphics screen on the settings shell: the D-pad goes from its Quality page into the rows, A
  turns Fog off and saves it, X presses the Undo that appears, which puts it back, and B leaves.
- shell_choice_step: on Graphics, Left and Right on a focused choice (Preset) step it back and on, from the D-pad
  and the arrow keys alike, and the focus stays; up and down leave the row, and LB goes to the page list.
- shader_screens: Shaders, on the settings shell, with a pack of more screens than its page list has rows for: they
  collapse into one `‹ Name ›` strip, RB walks every one and round to the pack list, LB from the first goes back to
  it, and Undo shows once the pack is picked.
- extras_shell_edges: on Extras, which has no page list, down from the search field lands on the first row, not Done;
  a row the right stick leaves mostly hidden scrolls back into view when A presses it, and A changes it.
Needs built fast binaries (`cargo build --profile fast -p horizon-client -p horizon-client-lib --bins`)
and prepared resources in .local/horizon-client/26.3. Exit status 0 when every case passes.
- radial_commit: holding the opener shows the overlay and pauses nothing; the stick selects; releasing runs the action once.
- radial_no_selection: a release with nothing selected, after retention ran out, or on an empty slot runs nothing.
- radial_retention: a stick let go just before the release still commits.
- radial_pages: the bumpers change the page and clear the selection; no other action runs.
- radial_cancel: B, Start, the pad going away and focus loss cancel, during retention too; nothing stays held.
- radial_camera: the selecting stick leaves the camera alone, until it has been let go after the menu closes.
- radial_precedence: menus and the on-screen keyboard keep the opener's button.
- radial_editor: by the pad alone, the radial page steps a slot with A, Right and Left, adds, renames (Done, then Cancel
  keeps the name) and deletes a page, resets, Undo puts back what it found, and the opener's row binds Y in place.
- radial_held_sneak: a sneak held on the pad stays held while the menu opens, selects and commits.
- radial_focus_background: with Play in Background on, losing the window's focus still cancels the menu.
- radial_unavailable: on a single saved page, an action that can't run (Emote, cosmetics off) is dimmed and its commit does nothing.
- radial_old_file: a settings file from before the radial menu keeps D-pad down on the advancements.
- radial_flick_with_press: a stick flicked as the opener goes down selects; one leaning since well before waits.
- radial_held_drop: a drop held through the menu doesn't throw the rest of the stack as it closes.
- radial_death: dying with immediate respawn while it's open cancels it; the release runs nothing.
- radial_dpad_roll: a D-pad press just after a commit on the D-pad opener reaches nothing; a later one does.
- radial_fight_back: RT while it's open cancels it and swings; a key or a click cancels and goes on; the mouse moving
  doesn't cancel it.
- radial_toggle: in Toggle mode a tap opens it, A or the opener again commits, B cancels, nothing aimed runs nothing.
- radial_left_stick: selecting with the left stick (Y opens it, D-pad Right being under that thumb), it walks nowhere
  while the right stick still turns the view.
- radial_bumper_opener: LB as the opener: RB turns the pages on, round the end with a second tick.
- radial_haptics: no tick on an empty slot, a tick on a filled one, a buzz on a release that ran nothing.
- radial_narration: with the narrator on, the page's name, then the slot's caption once it settles.
- radial_actions: Social is dimmed in single player and its key says so on the action bar; Quick Actions is dimmed
  with no dialog; the debug screen runs; an unknown action is Missing and runs nothing.
- radial_commit_screen: committing Inventory gives the screen the view at once: the overlay neither flashes nor
  fades over it.
- radial_nintendo_glyphs: on a Nintendo pad the overlay's bumpers are L and R, and its Cancel is the button that
  cancels: B going by letters, A by places.
- radial_cancel_roll, radial_toggle_roll: after B cancels with the opener held, or toggle mode commits on its press,
  the thumb rolling off the D-pad opener onto Drop drops nothing.
- radial_either_takeover: with Either, the right stick taking over from a walking left one selects at once, and the
  walking thumb easing on its frozen stick never takes over.
- radial_stick_back: Selects With stepped from Left (Y the opener) back to Right gives D-pad Right again; Right and Left
  step the row as every shell choice does.
- radial_status_rows: survival with armour and absorption, shot at 427x240 and 320x180: the dial clear of the rows.
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
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
EXE = '.exe' if os.name == 'nt' else ''
SETTLE = 0.5
# Between quick taps of a button: a few of the game's polls, well inside a paced craft (`PLACE_GAP`).
TAP_APART = 0.05


WINDOW_ASPECT = 16 / 9
WINDOW_ASPECT_SLACK = 0.1


class Game:
    def __init__(self, args, name, world=False, seed=None, env=None, extra=()):
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
        self.env = {**lab.game_env(**homes), **(env or {})}
        self.socket = self.home / 'ctl.json'
        self.launch = ['launch', '--game', args.game, '--offscreen', '--render-distance', 2,
                       '--resources', args.resources]
        if world:
            self.launch += ['--world', 'Menus', '--creative', '--no-pause', '--wait']
        self.launch += list(extra)
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
    """A trigger down and up, quickly."""
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
        # Social opens only on a server: this world, open to the network.
        game.ctl('command', 'publish')
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


VIBRATION_ABOUT = 'Rumble when something happens to you.'


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
        check(has_label(game, VIBRATION_ABOUT), f'the strip does not describe the focused row: {labels(game)}')
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
        check(has_label(game, 'cancels'), f'the strip does not say what cancels the wait: {labels(game)}')
        shot(game, 'controller-waiting')
        game.pad('start')
        check(not has_label(game, '> '), 'Start did not cancel the wait')
        check(has_label(game, 'Editing:'), 'Start left the screen instead of cancelling')
        check(not has_label(game, 'Undo'), 'a cancelled wait left an Undo')
        game.pad('south')
        game.pad('rs')
        check('key.jump' in controller_saved(game), f'the binding was not saved: {controller_saved(game)}')
        check(has_label(game, 'Undo'), 'no Undo after a binding')
        check(has_label(game, 'This button also does'), f'the strip does not say the button is shared: {labels(game)}')
        shot(game, 'controller-bound')
        game.pad('right')
        game.pad('south')
        check('key.jump' not in controller_saved(game), f'reset did not clear it: {controller_saved(game)}')
        check(not has_label(game, 'Undo'), 'Undo stayed with the binding back on its default')
        check((game.menu()['focus'] or {}).get('kind') != 'field', f'reset sent the focus to the search: {game.menu()["focus"]}')
    finally:
        game.quit()
    return check.failures


def controller_nintendo_buttons(args, name):
    # The virtual pad poses as Nintendo through the glyphs, the way the screen shows it.
    game = Game(args, name, seed=seed_controller('glyphs:nintendo\n'))
    check = Check()
    try:
        open_controller(game, 2)
        game.pad('right')
        for _ in range(16):
            if 'nintendo buttons' in (game.focused_label() or '').lower():
                break
            game.pad('down')
        check('By Position' in (game.focused_label() or ''), f'never reached Nintendo Buttons: {game.focused_label()}')
        game.pad('south')
        check('By Letter' in (game.focused_label() or ''), f'A did not pick By Letter: {game.focused_label()}')
        check('nintendoLetters:true' in controller_saved(game), f'not saved: {controller_saved(game)}')
        shot(game, 'controller-nintendo-buttons')
        # The virtual pad joins after a real pad's letters are read, so it still confirms with South.
        game.pad('south')
        check('By Position' in (game.focused_label() or ''), f'A did not step back: {game.focused_label()}')
        check('nintendoLetters:false' in controller_saved(game), f'not saved: {controller_saved(game)}')
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
        # The pad moves the focus off the field (its first press lands it), which gives the keys back: typing
        # changes nothing.
        for _ in range(3):
            game.pad('down')
            if (game.menu()['focus'] or {}).get('kind') not in (None, 'field'):
                break
        check((game.menu()['focus'] or {}).get('kind') not in (None, 'field'), f"the pad stayed on the field: {game.menu()['focus']}")
        game.ctl('type', 'zzz')
        time.sleep(SETTLE)
        check(has_label(game, 'Vibration'), f'the search kept the keys after the pad left it: {labels(game)}')
        game.ctl('key', 'esc', 'tap')
        time.sleep(SETTLE)
        check(not has_label(game, 'Editing:'), 'Escape did not leave the screen')
    finally:
        game.quit()
    return check.failures


OPTIONS_PAGES = ('main', 'video', 'sound', 'controls', 'keybinds', 'controller', 'controller-binds', 'controller-radial', 'mouse', 'chat',
                 'accessibility', 'language', 'font', 'packs', 'mods', 'shader-packs', 'graphics', 'graphics-advanced',
                 'graphics-hdr', 'extras', 'pvp', 'credits', 'horizon', 'agent')
# Screens allowed to draw no target, and why. The pad falls back to the crosshair on these.
TARGETS_WAIT = 5
STUDIO_RECORDINGS, STUDIO_BUTTONS = 12, 3


EXTRAS_ABOUT = 'Optional additions, all off by default'
AUTO_RECONNECT = 'Auto-reconnect'
AUTO_RECONNECT_ABOUT = 'Rejoins a server after a lost connection or a restart'
FIRST_EXTRA = 'Own name tag'
LAST_MENU_EXTRA = 'Resource pack browser'
GREYED_EXTRA = 'Sort key'
ROW_WALK = 40


def label_of(game, text):
    """The widget whose label has this in it, or None while the list hides it."""
    return next((w for w in game.widgets() if text in w.get('label', '')), None)


def ring_on(game, rect):
    """Whether a white ring is drawn along the top of this rectangle, by the pixels: state can't show a stale one."""
    path = game.home / 'ring.png'
    game.ctl('screenshot', path, timeout=120)
    rows, width, channels = png_rows(path)
    scale = width / game.ctl('ui')['gui_size'][0]
    x, y, w, _ = rect
    pixel = rows[int((y + 0.5) * scale)][int((x + w / 2) * scale) * channels:][:3]
    return pixel == bytes((255, 255, 255))


def extras_saved(game):
    path = game.game_home / 'horizon-extras.txt'
    return path.read_text() if path.exists() else ''


def extras_shell(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.press_through('west')
        game.ctl('options', 'extras')
        time.sleep(SETTLE)
        game.wait_targets()
        check(declared(game).get('Done') is True, f'Extras declares {declared(game)}')
        check(game.menu()['undeclared'] == [], f"undeclared: {game.menu()['undeclared']}")
        check(has_label(game, EXTRAS_ABOUT), f'the strip does not say what the page holds: {labels(game)}')
        check(label_of(game, LAST_MENU_EXTRA) is None, 'the page fits: this case cannot see it scroll')
        shot(game, 'extras-open')

        # From the search field across to the first row, then down: a greyed row is stepped over, hidden rows scroll in.
        game.pad('right')
        check(FIRST_EXTRA in (game.focused_label() or ''), f'the D-pad never reached {FIRST_EXTRA}: {game.focused_label()}')
        # A presses with a click: the row it pressed doesn't stay lit once the focus moves on.
        game.pad('south')
        pressed = game.focus()
        game.pad('down')
        check(game.focus() != pressed, 'the D-pad did not leave the row A pressed')
        check(not ring_on(game, pressed), f'the row A pressed keeps a ring at {pressed} with the focus gone')
        reached = {game.focused_label()}
        for _ in range(ROW_WALK):
            game.pad('down')
            reached.add(game.focused_label())
            if LAST_MENU_EXTRA in (game.focused_label() or ''):
                break
        check(any(LAST_MENU_EXTRA in (r or '') for r in reached), f'the D-pad never scrolled to the last rows: {reached}')
        check(not any(GREYED_EXTRA in (r or '') for r in reached), f'the focus landed on a greyed row: {reached}')
        shot(game, 'extras-scrolled')

        # Back up the list to a row near its top; the right stick moves the list and the focus stays on it.
        for _ in range(ROW_WALK):
            if AUTO_RECONNECT in (game.focused_label() or ''):
                break
            game.pad('up')
        label = game.focused_label()
        check(AUTO_RECONNECT in (label or ''), f'the D-pad never came back to {AUTO_RECONNECT}: {label}')
        check(has_label(game, AUTO_RECONNECT_ABOUT), f'the strip does not follow the row: {labels(game)}')
        check(AUTO_RECONNECT in (label or '') and label.endswith('Off'), f'{AUTO_RECONNECT} reads {label}')
        shot(game, 'extras-described')
        y = game.focus()[1]
        game.stick()
        moved = game.focus()
        check(game.focused_label() == label, f'the stick moved the focus from {label} to {game.focused_label()}')
        check(moved and moved[1] < y, f'the stick did not scroll the list: {y} to {moved}')

        # A changes the toggle: what the row reads, and the file it is saved in.
        check('autoReconnect:true' not in extras_saved(game), 'Auto-reconnect began on')
        game.pad('south')
        label = game.focused_label()
        check(label and label.endswith('On'), f'A left {AUTO_RECONNECT} as {label}')
        check('autoReconnect:true' in extras_saved(game), 'the toggle was not saved to horizon-extras.txt')
        check('Apply' not in declared(game), f'Extras has no Undo, yet a change declared {declared(game)}')
        shot(game, 'extras-toggled')

        # B presses Done.
        game.pad('east')
        game.wait_targets()
        check(not has_label(game, AUTO_RECONNECT), f'B did not leave Extras: {labels(game)}')
    finally:
        game.quit()
    return check.failures


FOG = 'Fog'
GRAPHICS_PAGES = ('General', 'Quality', 'Performance', 'Shaders', 'Advanced', 'HDR')


def graphics_saved(game):
    path = game.game_home / 'horizon-graphics.txt'
    return path.read_text() if path.exists() else ''


def graphics_shell(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.press_through('west')
        game.ctl('options', 'graphics')
        time.sleep(SETTLE)
        game.wait_targets()
        check(declared(game).get('Done') is True, f'Graphics declares {declared(game)}')
        check('Apply' not in declared(game), f'Undo shows with nothing changed: {declared(game)}')
        # Down the page list from the search field to Quality, which opens it, and right into its rows.
        for _ in range(len(GRAPHICS_PAGES)):
            game.pad('down')
            if game.focused_label() == 'Quality':
                break
        game.pad('south')
        check(label_of(game, FOG) is not None, f'the Quality page has no {FOG} row: {labels(game)}')
        game.pad('right')
        for _ in range(ROW_WALK):
            if (game.focused_label() or '').startswith(FOG):
                break
            game.pad('down')
        label = game.focused_label() or ''
        check(label.startswith(FOG) and label.endswith('On'), f'the D-pad never reached {FOG} On: {label}')
        game.pad('south')
        label = game.focused_label() or ''
        check(label.endswith('Off'), f'A left {FOG} as {label}')
        check('fog:false' in graphics_saved(game), 'Fog off was not saved to horizon-graphics.txt')
        check(declared(game).get('Apply') is True, f'Undo did not appear: {declared(game)}')
        game.pad('west')
        check(label_of(game, FOG) and label_of(game, FOG)['label'].endswith('On'), f'X did not undo: {labels(game)}')
        check('Apply' not in declared(game), f'Undo stayed after undoing: {declared(game)}')
        game.pad('east')
        game.wait_targets()
        check(label_of(game, FOG) is None, f'B did not leave Graphics: {labels(game)}')
    finally:
        game.quit()
    return check.failures


PRESET = 'Preset'


def shell_choice_step(args, name):
    # In a world: the arrow keys go through `horizon-ctl key`, which needs one.
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('options', 'graphics')
        time.sleep(SETTLE)
        game.wait_targets()
        # Down to the General page (the first press lands the focus), and across into its first row.
        for _ in range(3):
            if game.focused_label() == 'General':
                break
            game.pad('down')
        check(game.focused_label() == 'General', f"down never reached General: {game.menu()['focus']}")
        game.pad('right')
        start = game.focused_label() or ''
        row = game.focus()
        check(start.startswith(PRESET), f'right from General went to {start}, not {PRESET}')
        if not start.startswith(PRESET):
            return check.failures

        def reads():
            return next((w.get('label') or '' for w in game.widgets() if tuple(w['bounds']) == row), '')
        game.pad('right')
        on = reads()
        check(on != start and on.startswith(PRESET), f'D-pad right left {PRESET} as {on}')
        check(game.focus() == row, f'D-pad right moved the focus off {PRESET} to {game.focus()}')
        check(declared(game).get('Apply') is True, f'a step showed no Undo: {declared(game)}')
        shot(game, 'shell-choice-right')
        game.pad('left')
        check(reads() == start, f'D-pad left did not step {PRESET} back: {reads()}, not {start}')
        check(game.focus() == row, f'D-pad left moved the focus off {PRESET} to {game.focus()}')
        # The arrow keys do what the D-pad does.
        game.ctl('key', 'right', 'tap')
        time.sleep(SETTLE)
        check(reads() == on, f'the Right key left {PRESET} as {reads()}, not {on}')
        game.ctl('key', 'left', 'tap')
        time.sleep(SETTLE)
        check(reads() == start, f'the Left key left {PRESET} as {reads()}, not {start}')
        check(game.focus() == row, f'the arrow keys moved the focus off {PRESET} to {game.focus()}')
        # Up and down leave the row; LB goes to the page list, on the page before.
        game.pad('down')
        check(game.focus() != row, 'down did not leave the choice')
        game.pad('up')
        check(game.focus() == row, f'up did not come back to {PRESET}: {game.focused_label()}')
        game.pad('rb')
        focus = game.menu()['focus'] or {}
        check(str(focus.get('kind')).startswith('tab') and game.focused_label() == 'Quality',
              f'RB from {PRESET} went to {game.focused_label()} ({focus})')
        check(label_of(game, FOG) is not None, f'RB did not open Quality: {labels(game)}')
        shot(game, 'shell-choice-pages')
    finally:
        game.quit()
    return check.failures


ZOOM_INSTALL = 'Zoom: install from Mods'
# How far a group's name sits over its first row (`graphics_style::GROUP_ROW`): the list's top from the Mods note.
GROUP_NAME_H = 14
STICK_NOTCHES = 8


def extras_shell_edges(args, name):
    game = Game(args, name)
    check = Check()
    try:
        game.press_through('west')
        game.ctl('options', 'extras')
        time.sleep(SETTLE)
        game.wait_targets()
        check((game.menu()['focus'] or {}).get('kind') == 'field', f"Extras opened on {game.menu()['focus']}")
        game.pad('down')
        check(FIRST_EXTRA in (game.focused_label() or ''),
              f'down from the search field went to {game.focused_label()}, not {FIRST_EXTRA}')

        # The right stick moves the list until the focused row's middle is above the list's top.
        note = label_of(game, ZOOM_INSTALL)
        check(note is not None, f'no Mods note to find the list by: {labels(game)}')
        if not note or FIRST_EXTRA not in (game.focused_label() or ''):
            return check.failures
        top = note['bounds'][1] - GROUP_NAME_H
        before = game.focused_label()
        for _ in range(STICK_NOTCHES):
            focus = game.focus()
            if not focus or focus[1] + focus[3] / 2 < top:
                break
            game.stick()
        focus = game.focus()
        check(focus and focus[1] + focus[3] > top > focus[1] + focus[3] / 2,
              f'the stick did not leave the row mostly hidden under {top}: {focus}')
        check(game.focused_label() == before, f'the stick moved the focus from {before} to {game.focused_label()}')
        shot(game, 'extras-clipped')
        game.pad('south')
        shown = game.focus()
        check(shown and shown[1] >= top, f'A did not scroll the row into view under {top}: {shown}')
        after = game.focused_label()
        check(after and after != before and after.split(':')[0] == before.split(':')[0],
              f'A on the half-hidden row left it {before} -> {after}')
        shot(game, 'extras-revealed')
    finally:
        game.quit()
    return check.failures


# A shader pack with more screens than Shaders' page list has rows for in a 427x240 GUI view.
MANY_SCREENS = [f'S{i:02}' for i in range(12)]


def seed_many_screens(game_home):
    pack = game_home / 'shaderpacks' / 'ManyScreens' / 'shaders'
    pack.mkdir(parents=True)
    options = [f'O_{name}' for name in MANY_SCREENS]
    (pack / 'final.fsh').write_text('#version 120\n' + ''.join(f'#define {o}\n#ifdef {o}\n#endif\n' for o in options)
                                    + 'void main() { gl_FragColor = vec4(1.0); }\n')
    (pack / 'shaders.properties').write_text(
        'screen = ' + ' '.join(f'[{name}]' for name in MANY_SCREENS) + '\n'
        + ''.join(f'screen.{name} = {o}\n' for name, o in zip(MANY_SCREENS, options)))


def open_tab(game):
    """The label of the page list's lit tab."""
    lit = [tuple(t['rect']) for t in game.menu()['targets'] if t['kind'] == 'tab*']
    return next((w.get('label') for w in game.widgets() if tuple(w['bounds']) in lit), None)


def shader_screens(args, name):
    game = Game(args, name, seed=seed_many_screens)
    check = Check()
    try:
        game.press_through('west')
        game.ctl('options', 'shader-packs')
        time.sleep(SETTLE)
        game.wait_targets()
        # On the settings shell a pack is a tick box: "ManyScreens: Off".
        pack = next((w for w in game.widgets() if (w.get('label') or '').startswith('ManyScreens')), None)
        check(pack is not None, f'no ManyScreens pack: {labels(game)}')
        game.ctl('click', pack['id'])
        time.sleep(SETTLE)
        # RB from the pack list through every screen, none left out, and round to the pack list.
        walked = []
        for _ in range(len(MANY_SCREENS) + 2):
            game.pad('rb')
            walked.append(open_tab(game))
        packs = walked[-1]
        check(walked[1:-1] == MANY_SCREENS, f'RB walked {walked}')
        # LB back from the first screen leaves the strip for the pack list.
        game.pad('rb')
        game.pad('lb')
        check(open_tab(game) == packs, f'LB from {walked[0]} went to {open_tab(game)}')
        # Picking the pack is a change the shell's Undo puts back.
        check(has_label(game, 'Undo'), f'no Undo after picking a pack: {labels(game)}')
    finally:
        game.quit()
    return check.failures


# A resource pack with Polytone config entries (#11486): two sections, presets, a slider, a switch and a choice.
PACK_ENTRIES = {
    'fog_density': {'default_value': 0.5, 'min': 0, 'max': 1, 'step': 0.25, 'section': 'sky', 'section_order': 1,
                    'performance_impact': 'medium', 'presets': {'fast': 0, 'pretty': 1}},
    'clouds': {'default_value': True, 'section': 'sky'},
    'rain_style': {'default_value': 'drops', 'allowed_values': ['drops', 'streaks', 'none'], 'section': 'weather',
                   'section_order': 2, 'presets': {'fast': 'none', 'pretty': 'streaks'}},
}
PACK_LANG = {
    'config.demo.section.sky': 'Sky', 'config.demo.section.weather': 'Weather',
    'config.demo.fog_density': 'Fog density', 'config.demo.fog_density.tooltip': 'How thick the distant fog is.',
    'config.demo.clouds': 'Clouds', 'config.demo.rain_style': 'Rain style',
    'config.demo.rain_style.tooltip': 'How rain is drawn.',
    'demo.presets.fast': 'Fast', 'demo.presets.pretty': 'Pretty',
}
# A card's options button (pack_screen's OPTIONS_SIZE), and the D-pad hops allowed to reach it.
PACK_OPTIONS_SIZE = 16
PACK_HOPS = 8


def seed_pack_options(game_home):
    pack = game_home / 'resourcepacks' / 'Configurable'
    entries = pack / 'assets' / 'demo' / 'polytone' / 'config_entries'
    entries.mkdir(parents=True)
    (pack / 'assets' / 'demo' / 'lang').mkdir()
    # Compatible with 26.3's resource pack format (97.1), so the card isn't marked incompatible.
    (pack / 'pack.mcmeta').write_text(json.dumps({'pack': {'description': 'Pack options', 'min_format': 97,
                                                           'max_format': 999}}))
    for name, entry in PACK_ENTRIES.items():
        (entries / f'{name}.json').write_text(json.dumps(entry))
    (pack / 'assets' / 'demo' / 'lang' / 'en_us.json').write_text(json.dumps(PACK_LANG))
    with (game_home / 'options.txt').open('a') as options:
        options.write('resourcePacks:["vanilla","file/Configurable"]\n')


def pack_options_button(game):
    """The card's options button among the pack screen's targets: the only square of its size."""
    return next((r for r in game.rects() if r[2] == r[3] == PACK_OPTIONS_SIZE), None)


def on_pack_options_button(game, hops=PACK_HOPS):
    """The D-pad hops the crosshair from control to control until it is on the options button."""
    button = pack_options_button(game)
    if button is None:
        return False
    for _ in range(hops):
        pointer = game.menu()['pointer']
        if pointer and button[0] <= pointer[0] < button[0] + button[2] and button[1] <= pointer[1] < button[1] + button[3]:
            return True
        game.pad('right')
    return False


def pack_card_options(args, name):
    """A pad player reaches a pack card's options button as a control, and A opens its Pack Options."""
    game = Game(args, name, seed=seed_pack_options)
    check = Check()
    try:
        game.press_through('west')
        game.ctl('options', 'packs')
        time.sleep(SETTLE)
        game.wait_targets()
        check(game.menu()['mode'] == 'pointer', f"the pack screen left the crosshair: {game.menu()['mode']}")
        check(pack_options_button(game) is not None, f'the options button is no target: {game.rects()}')
        check(on_pack_options_button(game), f'the D-pad never reached it: {game.menu()["pointer"]}')
        shot(game, 'pack_card_options_button')
        game.pad('south')
        game.wait_targets()
        check(label_of(game, 'Presets') is not None, f'A did not open Pack Options: {labels(game)}')
        game.pad('east')
        game.wait_targets()
        check(pack_options_button(game) is not None, f'B did not come back to the pack screen: {labels(game)}')
    finally:
        game.quit()
    return check.failures


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


def osk_book(args, name):
    """A on a book and quill's page opens the keyboard, a new line keeps it open, and B closes it."""
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'give @s writable_book')
        time.sleep(1)
        game.ctl('pad', 'trigger', 'left', 1)
        time.sleep(0.4)
        game.ctl('pad', 'trigger', 'left', 0)
        time.sleep(1.5)
        menu = game.menu()
        check(menu['mode'] == 'pointer', f"no book open: {menu['mode']}")
        field = next((t for t in menu['targets'] if t['kind'] == 'field'), None)
        check(field is not None, f"the page isn't a field: {menu['targets']}")
        x, y, w, h = field['rect']
        for _ in range(40):
            pointer = game.menu()['pointer']
            if pointer and x <= pointer[0] < x + w and y <= pointer[1] < y + h:
                break
            step = 'left' if pointer and pointer[0] >= x + w else 'right' if pointer and pointer[0] < x else 'up' if pointer and pointer[1] >= y + h else 'down'
            game.pad(step)
        game.pad('south')
        check(game.menu()['keyboard'], 'A on the page opened no keyboard')
    finally:
        game.quit()
    return check.failures


def osk_sign(args, name):
    """A keeps a sign's keyboard to hand: Enter goes to the next line, and B then A opens it again."""
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'give @s oak_sign')
        game.ctl('command', 'setblock ~ ~-1 ~ stone')
        time.sleep(1)
        game.ctl('turn', '0', '90')
        time.sleep(0.5)
        game.ctl('pad', 'trigger', 'left', 1)
        time.sleep(0.4)
        game.ctl('pad', 'trigger', 'left', 0)
        time.sleep(1.5)
        check(game.menu()['keyboard'], 'a sign the pad placed opened no keyboard')
        game.pad('east')
        check(not game.menu()['keyboard'], 'B did not close the keyboard')
        game.pad('south')
        check(game.menu()['keyboard'], 'A did not open the keyboard again')
    finally:
        game.quit()
    return check.failures


def recipe_search_keyboard(args, name):
    """A on the recipe book's search box opens the keyboard."""
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('command', 'gamemode survival')
        game.ctl('key', 'e', 'tap')
        time.sleep(SETTLE)
        game.wait_targets()
        x, y, w, h = next(t for t in game.ctl('screen')['recipe_book']['targets'] if t[2:] == [20.0, 18.0])
        crosshair_walk(game, x + w / 2, y + h / 2)
        game.pad('south')
        time.sleep(SETTLE)
        field = next((t for t in game.menu()['targets'] if t['kind'] == 'field'), None)
        check(field is not None, 'the search box is not a field')
        if field:
            x, y, w, h = field['rect']
            crosshair_walk(game, x + w / 2, y + h / 2)
            game.pad('south')
            check(game.menu()['keyboard'], 'A on the search box opened no keyboard')
    finally:
        game.quit()
    return check.failures


def chat_suggestion_rows(args, name):
    """A command's suggestions are rows the pad can reach."""
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.ctl('key', 't', 'tap')
        time.sleep(SETTLE)
        game.ctl('type', '/ga')
        time.sleep(SETTLE)
        check(any(t['kind'] == 'row' for t in game.menu()['targets']),
              f"no suggestion rows: {game.menu()['targets']}")
    finally:
        game.quit()
    return check.failures


def server_list_bumpers(args, name):
    """The bumpers move the selected server, and the list saves it."""
    game = Game(args, name, seed=seed_servers)
    check = Check()
    try:
        game.press_through('south', 'down', 'south')
        game.wait_targets()
        first = game.rows()[0]['id']
        game.focus_on(first)
        game.pad('south')
        before = [row.get('label') for row in game.rows()[:2]]
        check(before[0] and before[1], f'no rows: {before}')
        game.pad('rb')
        time.sleep(SETTLE)
        after = [row.get('label') for row in game.rows()[:2]]
        check(after == before[::-1], f'the first server did not move down: {before} then {after}')
        game.pad('lb')
        time.sleep(SETTLE)
        back = [row.get('label') for row in game.rows()[:2]]
        check(back == before, f'it did not move back up: {after} then {back}')
    finally:
        game.quit()
    return check.failures


def osk_social(args, name):
    game = Game(args, name, world=True)
    check = Check()
    try:
        # Social opens only on a server: this world, open to the network.
        game.ctl('command', 'publish')
        time.sleep(1)
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


def open_book(game, *recipes, player='@s', command=None):
    """Survival, the player knowing only these recipes, the inventory open on the book: where the first recipe is."""
    command = command or (lambda line: game.ctl('command', line))
    command(f'gamemode survival {player}')
    command(f'recipe take {player} *')
    for recipe in recipes:
        command(f'recipe give {player} {recipe}')
    time.sleep(1)
    game.ctl('key', 'e', 'tap')
    time.sleep(SETTLE)
    game.wait_targets()
    book = game.ctl('screen')['recipe_book']
    x, y, w, h = next(t for t in book['targets'] if t[2:] == [20.0, 18.0])
    if not book['open']:
        game.ctl('click-at', x + w / 2, y + h / 2)
        time.sleep(SETTLE)
    x, y, w, h = next(t for t in game.ctl('screen')['recipe_book']['targets'] if t[2:] == [25.0, 25.0])
    return x + w / 2, y + h / 2


def counts(game):
    return game.ctl('inventory')['counts']


def grid(game):
    return {s['slot']: s['count'] for s in game.ctl('screen')['slots'] if 100 <= s['slot'] < 109 and s['item']}


def craft_ended(game):
    """Until no craft is running."""
    end = time.time() + 60
    while game.menu()['crafting']:
        if time.time() > end:
            raise RuntimeError('the craft never ended')
        time.sleep(0.1)
    time.sleep(SETTLE)


def direct_craft(game):
    """LT + A tapped, and the craft to its end."""
    game.ctl('pad', 'trigger', 'left', 1)
    time.sleep(SETTLE)
    game.pad('south')
    game.ctl('pad', 'trigger', 'left', 0)
    time.sleep(SETTLE)
    craft_ended(game)


def craft_held(game):
    """LT + A held until the craft ends by itself (the items, the inventory), then let go."""
    game.ctl('pad', 'trigger', 'left', 1)
    time.sleep(SETTLE)
    game.pad('south', 'down')
    craft_ended(game)
    game.pad('south', 'up')
    game.ctl('pad', 'trigger', 'left', 0)
    time.sleep(SETTLE)


def world_case(run):
    def case(args, name):
        game = Game(args, name, world=True)
        check = Check()
        try:
            run(game, check)
        finally:
            game.quit()
        return check.failures
    case.__name__ = run.__name__
    return case


@world_case
def craft_direct(game, check):
    at = open_book(game, 'minecraft:oak_planks')
    game.ctl('command', 'clear @s')
    game.ctl('command', 'give @s oak_log 5')
    time.sleep(1)
    crosshair_walk(game, *at)
    direct_craft(game)
    have = counts(game)
    check(have.get('minecraft:oak_planks') == 4 and have.get('minecraft:oak_log') == 4, f'LT + A: {have}')
    check(not grid(game), f'LT + A left {grid(game)} in the grid')
    craft_held(game)
    have = counts(game)
    check(have.get('minecraft:oak_planks') == 20 and not have.get('minecraft:oak_log'), f'LT + A held: {have}')
    check(not grid(game), f'LT + A held left {grid(game)} in the grid')
    craft_held(game)
    check(counts(game).get('minecraft:oak_planks') == 20, f'with nothing left: {counts(game)}')


@world_case
def craft_remainder(game, check):
    at = open_book(game, 'minecraft:sugar_from_honey_bottle')
    game.ctl('command', 'clear @s')
    game.ctl('command', 'give @s honey_bottle 3')
    time.sleep(1)
    crosshair_walk(game, *at)
    craft_held(game)
    have = counts(game)
    check(have.get('minecraft:sugar') == 9, f'sugar: {have}')
    check(have.get('minecraft:glass_bottle') == 3, f'the bottles: {have}')
    check(not have.get('minecraft:honey_bottle'), f'honey left: {have}')
    check(not grid(game), f'left in the grid: {grid(game)}')


@world_case
def craft_full(game, check):
    at = open_book(game, 'minecraft:oak_planks')
    game.ctl('command', 'clear @s')
    # Thirty-six full slots: two logs, 62 planks (room for two, of a result of four), and dirt.
    game.ctl('command', 'give @s oak_log 2')
    game.ctl('command', 'give @s oak_planks 62')
    game.ctl('command', f'give @s dirt {34 * 64}')
    time.sleep(1)
    crosshair_walk(game, *at)
    craft_held(game)
    want = {'minecraft:oak_log': 2, 'minecraft:oak_planks': 62, 'minecraft:dirt': 34 * 64}
    check(counts(game) == want, f'{counts(game)}, not {want}')
    check(not grid(game), f'left in the grid: {grid(game)}')


@world_case
def craft_grid(game, check):
    at = open_book(game, 'minecraft:oak_planks')
    game.ctl('command', 'clear @s')
    game.ctl('command', 'give @s oak_log 2')
    game.ctl('command', 'give @s dirt 1')
    time.sleep(1)
    game.ctl('click-slot', 1)
    game.ctl('click-slot', 100)
    time.sleep(SETTLE)
    check(sum(grid(game).values()) == 1, f'the dirt is not in the grid: {grid(game)}')
    crosshair_walk(game, *at)
    direct_craft(game)
    have = counts(game)
    check(have == {'minecraft:oak_log': 1, 'minecraft:oak_planks': 4, 'minecraft:dirt': 1}, f'{have}')
    check(not grid(game), f'left in the grid: {grid(game)}')


@world_case
def craft_carrying(game, check):
    at = open_book(game, 'minecraft:oak_planks')
    game.ctl('command', 'clear @s')
    game.ctl('command', 'give @s oak_log 2')
    game.ctl('command', 'give @s dirt 1')
    time.sleep(1)
    game.ctl('click-slot', 1)
    time.sleep(SETTLE)
    check(game.ctl('inventory')['carried'], 'nothing in the hand')
    crosshair_walk(game, *at)
    check(not game.menu()['craft_hint'], 'the guide offers LT + A with a stack in the hand')
    direct_craft(game)
    check(not counts(game).get('minecraft:oak_planks'), f'crafted with a stack in the hand: {counts(game)}')


def trigger_rules(args, name):
    game = Game(args, name, world=True)
    check = Check()
    tab = lambda: selected_tab(game)[0]

    def settled(label, want):
        time.sleep(SETTLE)
        check(tab() == want, f'{label}: category {tab()}, not {want}')

    def chord(side, button):
        """A trigger with a press, as fast as they go."""
        game.ctl('pad', 'trigger', side, 1)
        game.ctl('pad', button, 'tap')
        game.ctl('pad', 'trigger', side, 0)
        time.sleep(SETTLE)
        craft_ended(game)

    try:
        at = open_book(game, 'minecraft:oak_planks')
        game.ctl('command', 'clear @s')
        game.ctl('command', 'give @s oak_log 4')
        game.ctl('command', 'give @s dirt 8')
        time.sleep(1)
        before = tab()
        # RT changes the category as it goes down, and nothing more as it comes up.
        game.ctl('pad', 'trigger', 'right', 1)
        time.sleep(SETTLE)
        after = tab()
        check(after != before, 'RT held did not change the category as it went down')
        game.ctl('pad', 'trigger', 'right', 0)
        settled('RT coming up', after)
        # LT on the crafting book: nothing as it goes down, the previous category as it comes up,
        # however slowly.
        game.ctl('pad', 'trigger', 'left', 1)
        time.sleep(1)
        check(tab() == after, 'LT changed the category as it went down on the crafting book')
        game.ctl('pad', 'trigger', 'left', 0)
        settled('a slow LT pull', before)
        quick_tap(game, 'left')
        check(tab() != before, 'a quick tap of LT did not change the category')
        quick_tap(game, 'right')
        settled('a quick tap of RT', before)

        crosshair_walk(game, *at)
        chord('left', 'south')
        settled('LT + A on a recipe', before)
        have = counts(game)
        check(have.get('minecraft:oak_planks') == 4 and have.get('minecraft:oak_log') == 3, f'LT + A crafted: {have}')
        # Y on a recipe is Y, with LT or without: as many as fit, no craft. Only A keeps LT from
        # changing the category.
        chord('left', 'north')
        check(sum(grid(game).values()) == 3, f'LT + Y did not place all, as Y does: {grid(game)}')
        check(counts(game).get('minecraft:oak_planks') == 4, f'LT + Y crafted: {counts(game)}')
        check(tab() != before, 'LT + Y did not change the category as LT came up')
        quick_tap(game, 'right')
        settled('RT back', before)

        slots = [t for t in game.menu()['targets'] if t['kind'] == 'slot']
        bottom = max(t['rect'][1] for t in slots)
        dirt = sorted((t['rect'] for t in slots if t['rect'][1] == bottom), key=lambda r: r[0])[1]
        crosshair_walk(game, dirt[0] + dirt[2] / 2, dirt[1] + dirt[3] / 2)
        check(not game.ctl('inventory')['carried'], 'something in the hand already')
        chord('left', 'south')
        settled('LT + A on a slot', before)
        check(game.ctl('inventory')['carried'], 'LT + A on a slot did not take it')
        game.pad('south')

        # Overlapping triggers, either coming up first: RT's change as it goes down, LT's as it
        # comes up, so back where it was.
        for first, second in (('right', 'left'), ('left', 'right')):
            game.ctl('pad', 'trigger', 'left', 1)
            game.ctl('pad', 'trigger', 'right', 1)
            game.ctl('pad', 'trigger', first, 0)
            game.ctl('pad', 'trigger', second, 0)
            settled(f'overlapping triggers, {first} up first', before)
    finally:
        game.quit()
    return check.failures


def craft_tap_hold(args, name):
    # A long delay, so the several commands of a tap can't reach it.
    game = Game(args, name, world=True, seed=seed_controller('repeatDelay:1\n'))
    check = Check()
    logs = 40

    def have():
        now = counts(game)
        return now.get('minecraft:oak_log', 0), now.get('minecraft:oak_planks', 0)

    try:
        at = open_book(game, 'minecraft:oak_planks')
        game.ctl('command', 'clear @s')
        game.ctl('command', f'give @s oak_log {logs}')
        time.sleep(1)
        crosshair_walk(game, *at)
        check(game.menu()['craft_hint'], 'no craft row with the pad in use')
        rows = game.ctl('pad', 'status')['guide']['left']
        check(any(r['with'] == 'lt' and r['button'] == 'south' for r in rows),
              f'no LT + A row at the bottom left: {rows}')
        shot(game, 'craft-guide')
        # A comes up first, then LT comes up first.
        for up in (('south', 'left'), ('left', 'south')):
            game.ctl('pad', 'trigger', 'left', 1)
            game.ctl('pad', 'south', 'down')
            for button in up:
                if button == 'left':
                    game.ctl('pad', 'trigger', 'left', 0)
                else:
                    game.ctl('pad', 'south', 'up')
            time.sleep(SETTLE)
            craft_ended(game)
            logs -= 1
            check(have() == (logs, (40 - logs) * 4), f'a tap with {up[0]} up first: {have()}')
            check(not grid(game), f'a tap left {grid(game)} in the grid')
        # Held: one, nothing more until the delay, then several, up to the end of the items.
        game.ctl('pad', 'trigger', 'left', 1)
        game.ctl('pad', 'south', 'down')
        time.sleep(0.5)
        check(have() == (logs - 1, (41 - logs) * 4), f'held inside the delay: {have()}')
        time.sleep(1.5)
        left, _ = have()
        check(left < logs - 2, f'held past the delay, {logs - left} crafts')
        craft_ended(game)
        game.ctl('pad', 'south', 'up')
        game.ctl('pad', 'trigger', 'left', 0)
        time.sleep(SETTLE)
        check(have() == (0, 160), f'held to the end of the items: {have()}')
        check(not grid(game), f'held to the end left {grid(game)} in the grid')
    finally:
        game.quit()
    return check.failures


def craft_release(release):
    """A held craft that `release` ends part way, then a fresh press."""
    def case(args, name):
        game = Game(args, name, world=True)
        check = Check()
        logs = 64
        try:
            at = open_book(game, 'minecraft:oak_planks')
            game.ctl('command', 'clear @s')
            game.ctl('command', f'give @s oak_log {logs}')
            time.sleep(1)
            crosshair_walk(game, *at)
            category = selected_tab(game)[0]
            game.ctl('pad', 'trigger', 'left', 1)
            time.sleep(SETTLE)
            game.ctl('pad', 'south', 'down')
            time.sleep(1.5)
            check(game.menu()['crafting'], 'the craft ended by itself')
            release(game)
            craft_ended(game)
            check(not game.menu()['crafting'], 'a craft runs after the release')
            check(not grid(game), f'left in the grid: {grid(game)}')

            def accounted(extra=0):
                have = counts(game)
                planks, left = have.get('minecraft:oak_planks', 0), have.get('minecraft:oak_log', 0)
                check(planks % 4 == 0 and left + planks // 4 == logs, f'{left} logs and {planks} planks of {logs}')
                check(not game.ctl('inventory')['carried'], 'something in the hand')
                return planks // 4
            made = accounted()
            check(1 < made < logs, f'{made} crafts')
            # Nothing goes on by itself, with whatever is still down.
            time.sleep(1)
            check(accounted() == made, f'crafts went on: {made} then {accounted()}')
            game.ctl('pad', 'south', 'up')
            game.ctl('pad', 'trigger', 'left', 0)
            time.sleep(SETTLE)
            check(accounted() == made, f'letting go of the rest crafted: {made} then {accounted()}')
            check(not grid(game), f'letting go of the rest left {grid(game)} in the grid')
            check(selected_tab(game)[0] == category, 'letting go of LT changed the category')
            # The next press starts fresh, and crafts one.
            direct_craft(game)
            check(accounted() == made + 1, f'the next press: {made} then {accounted()}')
            check(not grid(game), f'the next press left {grid(game)} in the grid')
        finally:
            game.quit()
        return check.failures
    return case


craft_release_a = craft_release(lambda game: game.ctl('pad', 'south', 'up'))
craft_release_lt = craft_release(lambda game: game.ctl('pad', 'trigger', 'left', 0))
craft_release_press = craft_release(lambda game: game.ctl('pad', 'rb', 'tap'))


def craft_option_on(game_home):
    (game_home / 'horizon-controller.txt').write_text('craftOnServers:true\n')


def taps_with_lt(game, taps):
    """LT held over `taps` quick taps of A, each up before the last craft could finish."""
    game.ctl('pad', 'trigger', 'left', 1)
    time.sleep(SETTLE)
    for _ in range(taps):
        game.ctl('pad', 'south', 'tap')
        # A tap comes up at the next poll: one before the next tap keeps them presses of their own.
        time.sleep(TAP_APART)
    running = game.menu()['crafting']
    game.ctl('pad', 'trigger', 'left', 0)
    time.sleep(SETTLE)
    craft_ended(game)
    return running


def guest_crafts(game, command, check, allowed):
    """The player `Crafter`, on another computer's server."""
    at = open_book(game, 'minecraft:oak_planks', player='Crafter', command=command)
    command('clear Crafter')
    command('give Crafter oak_log 6')
    time.sleep(1)
    crosshair_walk(game, *at)
    check(game.menu()['craft_hint'] == allowed, f"the guide's LT + A row with the option {'on' if allowed else 'off'}")
    direct_craft(game)
    if not allowed:
        # Refused, LT + A is plain A: the recipe placed once, nothing crafted.
        check(not counts(game).get('minecraft:oak_planks'), f'crafted with the option off: {counts(game)}')
        check(sum(grid(game).values()) == 1, f'LT + A refused did not place the recipe, as A does: {grid(game)}')
        return
    have = counts(game)
    check(have.get('minecraft:oak_planks') == 4 and have.get('minecraft:oak_log') == 5, f'LT + A: {have}')
    # A server's placements keep their distance, so these taps come while a craft is in flight:
    # each still makes one.
    check(taps_with_lt(game, 3), 'the craft had ended before the last tap')
    have = counts(game)
    check(have.get('minecraft:oak_planks') == 16 and have.get('minecraft:oak_log') == 2, f'three quick taps: {have}')
    craft_held(game)
    have = counts(game)
    check(have.get('minecraft:oak_planks') == 24 and not have.get('minecraft:oak_log'), f'LT + A held: {have}')
    check(not grid(game), f'left in the grid: {grid(game)}')
    game.ctl('key', 'esc', 'tap')
    time.sleep(SETTLE * 2)
    have = counts(game)
    check(have.get('minecraft:oak_planks') == 24 and not have.get('minecraft:oak_log'), f'after closing: {have}')


def craft_lan_guest(args, name):
    check = Check()
    host = Game(args, f'{name}-host', world=True)
    try:
        port = host.ctl('lan', 'open', 0, 'commands')['port']
        # Joined by address, not from the server list's LAN entries (`connect` has no discovery):
        # a server like any other, so it takes the option.
        for allowed in (False, True):
            label = 'on' if allowed else 'off'
            guest = Game(args, f'{name}-guest-{label}', seed=craft_option_on if allowed else None)
            try:
                guest.wait_targets()
                guest.ctl('connect', f'127.0.0.1:{port}', 'Crafter')
                guest.ctl('wait-for', 'loaded', '--timeout', 300, timeout=320)
                time.sleep(2)
                guest_crafts(guest, lambda line: host.ctl('command', line), check, allowed)
            finally:
                guest.quit()
    finally:
        host.quit()
    return check.failures


def craft_dedicated(args, name):
    from anticheat_e2e import PROPERTIES
    from server_bench import DONE, Server
    check = Check()
    folder = Path(tempfile.mkdtemp(prefix=f'pad-menu-{name}-server-'))
    port = lab.free_port()
    props = dict(PROPERTIES, gamemode='survival', difficulty='peaceful', **{'server-port': str(port)})
    (folder / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()))
    (folder / 'eula.txt').write_text('eula=true\n')
    server = Server([args.game, '--resources', args.resources, '--dedicated', '--nogui'], folder, folder / 'out.log')
    guest = None
    try:
        server.wait_for(DONE, 300)
        guest = Game(args, f'{name}-client', seed=craft_option_on)
        guest.wait_targets()
        guest.ctl('connect', f'127.0.0.1:{port}', 'Crafter')
        guest.ctl('wait-for', 'loaded', '--timeout', 300, timeout=320)
        time.sleep(2)

        def command(line):
            server.console(line)
            time.sleep(0.3)
        guest_crafts(guest, command, check, True)
    finally:
        if guest:
            guest.quit()
        server.stop()
    shutil.rmtree(folder, ignore_errors=True)
    return check.failures


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
            game.pad('lb')
            left, right = guide(game)
            check(left or right, f'{style}: no gameplay guide rows')
            shot(game, f'glyph_art_{style}_guide')
            game.ctl('key', 'e', 'tap')
            time.sleep(SETTLE)
            game.wait_targets()
            game.pad('down')
            check(game.menu()['pointer'] is not None, f'{style}: no crosshair in the inventory')
            shot(game, f'glyph_art_{style}_crosshair')
        finally:
            game.quit()
    return check.failures


def nintendo_letters(args, name):
    check = Check()
    # By letters the pad's X (on top) plays West and its B (at the bottom) East; by places Y and A.
    for letters, west, east in ((True, 'X', 'B'), (False, 'Y', 'A')):
        mode = 'letters' if letters else 'places'
        seed = seed_controller(f'nintendoLetters:{str(letters).lower()}\n')
        game = Game(args, f'{name}-{mode}', seed=seed)
        try:
            game.ctl('pad', 'family', 'nintendo')
            game.wait_targets()
            game.pad('down')
            shown = {g['button']: g for g in game.menu()['glyphs']}
            for part, label in (('West', west), ('East', east)):
                glyph = shown.get(part) or {}
                check(glyph.get('label') == label, f'{mode} {part}: {glyph.get("label")}, not {label}')
                sprite = f'horizon:gui/controller/nx_{label.lower()}'
                check(glyph.get('sprite') == sprite, f'{mode} {part}: {glyph.get("sprite")}, not {sprite}')
            shot(game, f'nintendo_{mode}_title')
        finally:
            game.quit()
        game = Game(args, f'{name}-{mode}-world', world=True, seed=seed)
        try:
            game.ctl('pad', 'family', 'nintendo')
            game.pad('lb')
            left, right = guide(game)
            check(left or right, f'{mode}: no gameplay guide rows')
            # Jump is South: the A on the right by letters, the B at the bottom by places.
            labels = {r['caption']: r['label'] for r in game.ctl('pad', 'status')['guide']['left']}
            jump = 'A' if letters else 'B'
            check(labels.get('key.jump') == jump, f'{mode}: jump shows {labels.get("key.jump")}, not {jump}')
            shot(game, f'nintendo_{mode}_guide')
            game.ctl('pad', 'connection', 'connected', 'nintendo')
            time.sleep(1)
            check(game.ctl('pad', 'status').get('notice'), f'{mode}: no notice for the connected pad')
            shot(game, f'nintendo_{mode}_notice')
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
        check(not game.menu()['holding_slider'], 'landing on the slider took hold of it')
        game.pad('south')
        check(game.menu()['holding_slider'], 'A did not take hold of the slider')
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



def guide(game):
    """Each column of the gameplay guide as last drawn, as (button, caption without its prefix)."""
    time.sleep(SETTLE)
    shown = game.ctl('pad', 'status')['guide']
    return tuple([(r['button'], r['caption'].replace('horizon.controller.guide.', '')) for r in shown[side]]
                 for side in ('left', 'right'))


def gameplay_guide(args, name):
    # Attack moved to Y, Pick Block unbound: the rows must follow.
    game = Game(args, name, world=True, seed=seed_controller('bind_key.attack:north\nbind_key.pickItem:none\n'))
    check = Check()
    try:
        check(guide(game) == ([], []), f'guide before the pad did anything: {guide(game)}')
        game.pad('lb')
        left, right = guide(game)
        check(('south', 'key.jump') in left and ('north', 'key.inventory') in left, f'left column: {left}')

        x, y, z = game.ctl('state')['player']['block_pos']
        game.ctl('command', 'gamerule randomTickSpeed 0')
        game.ctl('command', f'fill {x - 2} {y} {z + 1} {x + 2} {y + 3} {z + 4} air')
        at = (x, y + 1, z + 3)
        place = lambda block: game.ctl('command', f'setblock {at[0]} {at[1]} {at[2]} {block}')
        slot = game.ctl('state')['selected'] - 1
        give = lambda item: game.ctl('command', f'item replace entity @s hotbar.{slot} with {item}')
        place('stone')
        game.ctl('look-at', 'block', *at)
        give('stone')
        _, right = guide(game)
        check(right == [('north', 'break'), ('lt', 'place')], f'creative on stone with stone: {right}')

        place('chest')
        _, right = guide(game)
        check(('lt', 'open') in right and ('lt', 'place') not in right, f'on a chest: {right}')

        # Only what opens something says Open, and a block that keeps the click and does nothing
        # has no row, for it or for the item. Break shows the aim is on the block.
        def rows_on(block, item='air'):
            place(block)
            give(item)
            game.ctl('look-at', 'block', *at)
            _, rows = guide(game)
            check(('north', 'break') in rows, f'not aiming at {block}: {rows}')
            return rows
        for block, wanted in (('nether_portal', None), ('end_gateway', None), ('lectern', None),
                              ('flower_pot', None), ('chiseled_bookshelf', None),
                              ('stone_button[face=floor,powered=true]', None),
                              ('lectern[has_book=true]', 'open'), ('ender_chest', 'open'),
                              ('oak_door', 'use'), ('stone_button[face=floor]', 'use')):
            rows = rows_on(block)
            shown = [c for b, c in rows if b == 'lt']
            check(shown == ([wanted] if wanted else []), f'empty hand on {block}: {rows}')
        for block, item, wanted in (('lectern', 'stone', []), ('lectern', 'written_book', ['use']),
                                    ('flower_pot', 'stone', []), ('flower_pot', 'poppy', ['use']),
                                    ('nether_portal', 'stone', ['place'])):
            rows = rows_on(block, item)
            shown = [c for b, c in rows if b == 'lt']
            check(shown == wanted, f'{item} on {block}: {rows}')
        give('air')

        game.ctl('command', 'gamemode survival')
        place('bedrock')
        give('air')
        _, right = guide(game)
        check(right == [], f'survival with an empty hand on bedrock: {right}')
        place('stone')
        _, right = guide(game)
        check(right == [('north', 'break')], f'survival with an empty hand on stone: {right}')
        game.ctl('command', 'gamemode creative')

        # The item in use.
        give('bow')
        # Nothing in reach to break, whatever this world grew here.
        game.ctl('command', f'fill {x - 6} {y + 1} {z - 6} {x + 6} {y + 7} {z + 6} air')
        game.ctl('look-at', x + 40, y + 20, z)
        _, right = guide(game)
        check(right == [('lt', 'shoot')], f'creative bow at nothing: {right}')
        game.ctl('pad', 'trigger', 'left', 1)
        _, right = guide(game)
        check(right == [('lt', 'release_shoot')], f'drawing it: {right}')
        game.ctl('pad', 'trigger', 'left', 0)
        give('air')

        # A screen takes the input, and F1 hides it.
        game.pad('north')
        rows = guide(game)
        check(('south', 'key.jump') not in rows[0] and ('north', 'key.inventory') not in rows[0],
              f'the gameplay rows over the inventory: {rows}')
        game.pad('start')
        game.ctl('pad', 'release')
        time.sleep(SETTLE)
        game.pad('lb')
        check(guide(game) != ([], []), 'the guide did not come back')
        game.ctl('key', 'f1', 'tap')
        check(guide(game) == ([], []), 'the guide with the HUD hidden')
        game.ctl('key', 'f1', 'tap')
    finally:
        game.quit()
    return check.failures


def gameplay_guide_mobs(args, name):
    # An allay's answer to a click is the interaction itself: the guide showing it must not do it.
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.pad('lb')
        x, y, z = game.ctl('state')['player']['block_pos']
        game.ctl('command', 'gamemode survival')
        game.ctl('command', f'fill {x - 2} {y} {z + 1} {x + 2} {y + 3} {z + 4} air')
        slot = game.ctl('state')['selected'] - 1
        game.ctl('command', f'item replace entity @s hotbar.{slot} with diamond 8')
        game.ctl('command', f'summon allay {x} {y + 1} {z + 3} {{NoAI:1b}}')
        time.sleep(SETTLE)
        allay = game.ctl('entities', 8, 'allay')['entities'][0]
        game.ctl('look-at', 'entity', allay['id'])
        for _ in range(3):
            _, right = guide(game)
        check(any(button == 'lt' for button, _ in right), f'no use row on the allay: {right}')
        held = game.ctl('state')['held']
        check(held == {'item': 'minecraft:diamond', 'count': 8}, f'the guide gave the allay an item: {held}')
    finally:
        game.quit()
    return check.failures


def slider_needs_a(args, name):
    """Left and right only change a slider A took hold of; B and up or down let go of it."""
    game = Game(args, name)
    check = Check()
    try:
        open_controller(game)
        game.pad('right')
        for _ in range(24):
            if 'trigger threshold' in (game.focused_label() or '').lower():
                break
            game.pad('down')
        slider = game.focus()
        # Right is either nothing or the widget beside it, which left comes back from.
        game.pad('right')
        if game.focus() != slider:
            game.pad('left')
        check(game.focus() == slider, f'never came back to the slider: {game.focus()}')
        check('40%' in (game.focused_label() or ''), f'moved without A: {game.focused_label()}')
        check(not game.menu()['holding_slider'], 'moving the focus took hold of the slider')

        game.pad('south')
        check(game.menu()['holding_slider'], 'A did not take hold of the slider')
        for _ in range(6):
            game.pad('right')
            if '40%' not in (game.focused_label() or ''):
                break
        changed = game.focused_label()
        check('40%' not in (changed or ''), f'right in hand left it on {changed}')
        check(game.focus() == slider, 'left or right moved the focus off a slider in hand')

        screen = game.menu()['screen_key']
        game.pad('east')
        check(not game.menu()['holding_slider'], 'B did not let go of the slider')
        check(game.menu()['screen_key'] == screen, 'B left the screen as well as the slider')
        check(game.focused_label() == changed, f'letting go changed the value: {game.focused_label()}')

        game.pad('south')
        check(game.menu()['holding_slider'], 'A did not take hold again')
        game.pad('south')
        check(not game.menu()['holding_slider'], 'A again did not let go')

        game.pad('south')
        game.pad('down')
        check(not game.menu()['holding_slider'], 'down did not let go of the slider')
        check(game.focus() != slider, 'down did not move on from the slider')
        # Up can land on a row beside it, so find it again by name.
        for button in ['up'] * 8 + ['down'] * 16:
            if 'trigger threshold' in (game.focused_label() or '').lower():
                break
            game.pad(button)
        check('trigger threshold' in (game.focused_label() or '').lower(),
              f'never came back to the slider: {game.focused_label()}')
        check(not game.menu()['holding_slider'], 'the slider was still held on coming back')

        # The hold goes with the screen: Start, and back to where the focus is remembered.
        game.pad('south')
        check(game.menu()['holding_slider'], 'A did not take hold after coming back')
        game.pad('start')
        check(not game.menu()['holding_slider'], 'Start left a slider held')
    finally:
        game.quit()
    return check.failures


def gameplay_guide_basic(args, name):
    game = Game(args, name, world=True, seed=seed_controller('gameplayGuide:basic\n'))
    check = Check()
    try:
        game.pad('lb')
        check(game.ctl('pad', 'status')['in_use'], 'the pad is not in use')
        left, _ = guide(game)
        check(left == [], f'basic still shows the left column: {left}')
        status = game.ctl('pad', 'status')['guide']
        check('right' in status, f'no right column in {status}')
    finally:
        game.quit()
    return check.failures


def gameplay_guide_boat_and_keys(args, name):
    """A boat shows Ride only when it would take the player, and the keyboard hides the guide."""
    game = Game(args, name, world=True)
    check = Check()
    try:
        game.pad('lb')
        x, y, z = game.ctl('state')['player']['block_pos']
        game.ctl('command', f'fill {x - 2} {y} {z + 1} {x + 2} {y + 3} {z + 5} air')
        game.ctl('command', f'summon oak_boat {x} {y} {z + 3}')
        time.sleep(1)
        game.ctl('look-at', x, y + 0.2, z + 3)
        for _ in range(10):
            time.sleep(0.3)
            _, right = guide(game)
            if right:
                break
        check(('lt', 'ride') in right, f'no Ride on a free boat: {right}')
        game.ctl('key', 'lshift', 'down')
        game.pad('lb')
        _, sneaking = guide(game)
        check(not any(caption == 'ride' for _, caption in sneaking), f'Ride while sneaking: {sneaking}')
        game.ctl('key', 'lshift', 'up')

        game.ctl('key', 'j', 'tap')
        time.sleep(SETTLE)
        check(guide(game) == ([], []), f'the guide stayed up after a key: {guide(game)}')
        game.pad('lb')
        check(guide(game) != ([], []), 'the pad did not bring the guide back')

        # The pad's own click isn't the mouse: back to the game from the pause menu.
        game.pad('start')
        time.sleep(SETTLE)
        game.wait_targets()
        game.pad('south')
        game.pad('south')
        time.sleep(SETTLE)
        check(game.ctl('state')['screen'] == 'playing', f"still on {game.ctl('state')['screen']}")
        check(guide(game) != ([], []), "the pad's own click hid the guide")
    finally:
        game.quit()
    return check.failures


def gameplay_guide_off(args, name):
    game = Game(args, name, world=True, seed=seed_controller('gameplayGuide:false\n'))
    check = Check()
    try:
        game.pad('lb')
        check(game.ctl('pad', 'status')['in_use'], 'the pad is not in use')
        check(guide(game) == ([], []), f'the guide with its setting off: {guide(game)}')
    finally:
        game.quit()
    return check.failures



def radial(game):
    return game.ctl('pad', 'status')['radial']


def hold_opener(game):
    game.ctl('pad', 'right', 'down')
    time.sleep(0.2)


def aim(game, x, y, settle=0.15):
    game.ctl('pad', 'stick', 'right', x, y)
    time.sleep(settle)


def radial_case(body):
    def case(args, name):
        game = Game(args, name, world=True)
        check = Check()
        try:
            game.ctl('pad', 'focus', 'true')
            time.sleep(SETTLE)
            body(game, check)
        finally:
            game.quit()
        return check.failures
    return case


@radial_case
def radial_commit(game, check):
    ticks = game.ctl('status')['ticks']
    hold_opener(game)
    check(radial(game)['open'], 'holding the opener did not open it')
    check(radial(game)['shown'], 'held past the draw delay, it does not show')
    aim(game, 0, 1)
    check(radial(game)['selected'] == 0, f"up selected {radial(game)['selected']}, not the first slot")
    rumble = game.ctl('pad', 'status')['last_rumble']
    check(rumble and rumble['reason'] == 'radial', f'no tick on reaching a slot: {rumble}')
    shot(game, 'radial_overlay')
    time.sleep(1)
    check(game.ctl('status')['ticks'] > ticks, 'the world stopped behind the overlay')
    before = game.ctl('ui')['screen']
    game.ctl('pad', 'right', 'up')
    time.sleep(SETTLE)
    state = radial(game)
    check(not state['open'], 'still open after the release')
    check(state['runs'] == ['key.advancements'], f"ran {state['runs']}")
    check(state['run_count'] == 1, f"counted {state['run_count']} runs")
    check(game.ctl('ui')['screen'] != before, 'the advancements did not open')
    game.ctl('pad', 'stick', 'right', 0, 0)


@radial_case
def radial_no_selection(game, check):
    hold_opener(game)
    game.ctl('pad', 'right', 'up')
    time.sleep(SETTLE)
    hold_opener(game)
    aim(game, 0, 1)
    aim(game, 0, 0, settle=0.5)
    check(radial(game)['selected'] is None, 'the selection outlived the retention')
    game.ctl('pad', 'right', 'up')
    time.sleep(SETTLE)
    hold_opener(game)
    game.ctl('pad', 'rb', 'tap')
    time.sleep(0.2)
    aim(game, 0.7, 0.7)
    state = radial(game)
    check(state['selected'] == 1 and state['slots'][1] is None, f'not on an empty slot: {state}')
    game.ctl('pad', 'right', 'up')
    time.sleep(SETTLE)
    check(radial(game)['runs'] == [], f"ran {radial(game)['runs']}")


@radial_case
def radial_retention(game, check):
    hold_opener(game)
    aim(game, 0, 1)
    game.ctl('pad', 'stick', 'right', 0, 0)
    game.ctl('pad', 'right', 'up')
    time.sleep(SETTLE)
    check(radial(game)['runs'] == ['key.advancements'], f"ran {radial(game)['runs']}")


@radial_case
def radial_pages(game, check):
    hotbar = game.ctl('status')['selected']
    hold_opener(game)
    aim(game, 0, 1)
    check(radial(game)['selected'] == 0, 'the stick selected nothing')
    game.ctl('pad', 'rb', 'tap')
    time.sleep(0.2)
    state = radial(game)
    check(state['page'] == 1 and state['selected'] is None, f'after RB: {state}')
    check(state['waiting'], f'a stick held through the page turn is not waiting to centre: {state}')
    shot(game, 'radial_camera_page_waiting')
    game.ctl('pad', 'lb', 'tap')
    time.sleep(0.2)
    game.ctl('pad', 'lb', 'tap')
    time.sleep(0.2)
    check(radial(game)['page'] == 1, 'LB did not wrap')
    game.ctl('pad', 'right', 'up')
    time.sleep(SETTLE)
    check(radial(game)['runs'] == [], 'a stick held through the page turn ran something')
    check(game.ctl('status')['selected'] == hotbar, 'the bumpers moved the hotbar')
    game.ctl('pad', 'stick', 'right', 0, 0)
    # Every hold starts on the first page, so a flick aimed blind lands where it did.
    time.sleep(SETTLE)
    hold_opener(game)
    check(radial(game)['page'] == 0, f"opened on page {radial(game)['page']}")
    game.ctl('pad', 'right', 'up')


@radial_case
def radial_cancel(game, check):
    screen = game.ctl('ui')['screen']

    def selected_then_neutral():
        hold_opener(game)
        aim(game, 0, 1)
        game.ctl('pad', 'stick', 'right', 0, 0)

    def ends(what):
        time.sleep(0.05)
        check(not radial(game)['open'], f'{what} left it open')

    for button in ('east', 'start'):
        selected_then_neutral()
        game.ctl('pad', button, 'tap')
        ends(button)
        game.ctl('pad', 'right', 'up')
        time.sleep(SETTLE)
        check(game.ctl('ui')['screen'] == screen, f'{button} opened a screen')

    # The pad going away: `pad release` lets go of the opener too.
    selected_then_neutral()
    game.ctl('pad', 'release')
    ends('the pad going away')
    time.sleep(SETTLE)

    # Focus loss, with an action held since before the menu opened.
    game.ctl('pad', 'south', 'down')
    time.sleep(0.2)
    check('Space' in game.ctl('status')['keys'], 'A held no jump to begin with')
    selected_then_neutral()
    game.ctl('pad', 'focus', 'false')
    time.sleep(0.2)
    check(not radial(game)['open'], 'focus loss left it open')
    check(game.ctl('pad', 'status')['held'] == [], 'focus loss left buttons held')
    check('Space' not in game.ctl('status')['keys'], 'focus loss left the jump held')
    game.ctl('pad', 'right', 'up')
    game.ctl('pad', 'south', 'up')
    game.ctl('pad', 'focus', 'true')
    time.sleep(SETTLE)
    check(radial(game)['runs'] == [], f"cancelled but ran {radial(game)['runs']}")
    check(game.ctl('ui')['screen'] == screen, 'a cancelled menu opened a screen')

    # A commit leaves nothing held either: Hide HUD, on the camera page.
    hold_opener(game)
    game.ctl('pad', 'rb', 'tap')
    time.sleep(0.2)
    aim(game, 1, 0)
    game.ctl('pad', 'right', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(SETTLE)
    check(radial(game)['runs'] == ['key.toggleGui'], f"ran {radial(game)['runs']}")
    check(game.ctl('status')['keys'] == [], f"left {game.ctl('status')['keys']} held")


@radial_case
def radial_camera(game, check):
    yaw = lambda: game.ctl('status')['yaw']
    was = yaw()
    aim(game, 1, 0, settle=0.3)
    check(yaw() != was, 'the stick does not turn the view at all')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(SETTLE)
    was = yaw()
    hold_opener(game)
    aim(game, 1, 0, settle=0.4)
    check(yaw() == was, 'the selecting stick turned the view')
    game.ctl('pad', 'east', 'tap')
    time.sleep(0.3)
    check(not radial(game)['open'] and yaw() == was, 'a stick still leaning turned the view after the cancel')
    game.ctl('pad', 'right', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(SETTLE)
    was = yaw()
    aim(game, 1, 0, settle=0.3)
    check(yaw() != was, 'the stick stayed blocked after it was let go')
    game.ctl('pad', 'stick', 'right', 0, 0)


@radial_case
def radial_precedence(game, check):
    game.ctl('key', 'e', 'tap')
    time.sleep(SETTLE)
    game.wait_targets()
    game.pad('down')
    check(not radial(game)['open'], 'it opened over the inventory')
    game.ctl('key', 'e', 'tap')
    time.sleep(SETTLE)
    game.ctl('key', 't', 'tap')
    time.sleep(SETTLE)
    game.wait_targets()
    game.pad('south')
    check(game.menu()['keyboard'], 'A on the chat field opened no on-screen keyboard')
    game.ctl('pad', 'right', 'down')
    time.sleep(0.2)
    check(not radial(game)['open'], 'it opened over the on-screen keyboard')
    game.ctl('pad', 'right', 'up')


# Three saved pages, the third the player's own.
RADIAL_SEED = ('bindLayout:2\n'
               'radial_0_name:horizon.radial.page.general\nradial_0_slots:key.advancements,,key.command,,key.socialInteractions,,,\n'
               'radial_1_name:horizon.radial.page.camera\nradial_1_slots:key.screenshot,,key.toggleGui,,key.smoothCamera,,,\n'
               'radial_2_label:Mine\nradial_2_slots:key.chat,,,,,,,\n')
# The opener's row, D-pad Right as the Bindings page names it, and LB once bound to it.
OPENER_ROW = 'Opener: \u2192'
OPENER_LB = 'Opener: LB'


def pad_to(game, label, way, tries=30):
    """The D-pad that way until the focus is on what says `label`."""
    for _ in range(tries):
        if game.focused_label() == label:
            return True
        game.pad(way)
    return game.focused_label() == label


def radial_editor(args, name):
    game = Game(args, name, world=True, seed=lambda home: (home / 'horizon-controller.txt').write_text(RADIAL_SEED))
    check = Check()
    saved = lambda: (game.game_home / 'horizon-controller.txt').read_text()
    pages = lambda: radial(game)['pages']
    enabled = lambda label: next((w.get('enabled') for w in game.widgets() if w.get('label') == label), None)
    try:
        game.ctl('pad', 'focus', 'true')
        time.sleep(SETTLE)
        game.ctl('options', 'controller')
        time.sleep(SETTLE)
        game.wait_targets()
        # Bindings, then down its list to the page's row.
        game.pad('down')
        game.pad('rb')
        game.pad('right')
        pad_to(game, 'Radial Menu...', 'up', tries=6) or pad_to(game, 'Radial Menu...', 'down', tries=60)
        check(game.focused_label() == 'Radial Menu...', f'never reached the radial page: {game.focused_label()}')
        game.pad('south')
        game.wait_targets()
        shot(game, 'radial_editor')
        check(declared(game).get('Done') is True, f'the page declares {declared(game)}')
        check(game.menu()['undeclared'] == [], f"undeclared: {game.menu()['undeclared']}")
        check(all(has_label(game, page) for page in ('General', 'Camera', 'Mine')), f'the page list: {labels(game)}')
        check('Apply' not in declared(game), 'Undo before any change')

        # A slot: the focus opens on the first row (else it is across from the search field), and A steps it on.
        if not (game.focused_label() or '').startswith('Up:'):
            game.pad('right')
        check(game.focused_label() == 'Up: Advancements', f'the first row is {game.focused_label()}')
        # A opens the list of actions, on the slot's own, marked.
        game.pad('south')
        game.wait_targets()
        shot(game, 'radial_picker')
        check(has_label(game, 'Empty') and has_label(game, 'Inventory'), f'the list: {labels(game)}')
        check(game.focused_label() == 'Advancements: Current', f'the list opened on {game.focused_label()}')
        game.pad('down')
        check(game.focused_label() == 'Perspective', f'down went to {game.focused_label()}')
        game.pad('south')
        game.wait_targets()
        check(game.focused_label() == 'Up: Perspective', f'the pick left the slot as {game.focused_label()}')
        check(radial(game)['slots'][0] == 'key.togglePerspective', f"slots: {radial(game)['slots']}")
        check(declared(game).get('Apply') is True, f'a change shows no Undo: {declared(game)}')
        shot(game, 'radial_editor_undo')
        # Right steps it on and Left back, the focus staying on the slot.
        game.pad('right')
        check(game.focused_label() == 'Up: Swap Hands', f'Right left the slot as {game.focused_label()}')
        shot(game, 'radial_editor_stepped')
        game.pad('left')
        check(game.focused_label() == 'Up: Perspective', f'Left left the slot as {game.focused_label()}')

        # A new page, shown; then renamed on the keyboard the pad opens.
        check(pad_to(game, 'New Page', 'down'), f'never reached New Page: {game.focused_label()}')
        game.pad('south')
        check(pages() == 4, f'{pages()} pages after New Page')
        check(pad_to(game, 'Rename Page', 'up'), f'never reached Rename Page: {game.focused_label()}')
        game.pad('south')
        game.wait_targets()
        check(labels(game)[:2] == ['Done', 'Cancel'], f'Rename opened {labels(game)}')
        shot(game, 'radial_rename')
        field = (game.menu()['focus'] or {}).get('kind') == 'field' or pad_to(game, 'New Page', 'up', tries=3)
        check(field, f"the name page opened on {game.menu()['focus']}")
        game.pad('south')
        check(game.menu()['keyboard'], 'A on the name did not open the keyboard')
        game.pad('south')
        game.pad('start')
        game.wait_targets()
        renamed = [label for label in labels(game) if label.startswith('New Page') and label != 'New Page']
        check(not has_label(game, 'Cancel') and renamed, f'the name was not kept: {labels(game)}')
        check(game.focused_label() == 'Rename Page', f'Done left the focus on {game.focused_label()}')
        shot(game, 'radial_editor_renamed')

        # Rename again, then B: Cancel keeps the name, and the focus comes back to Rename Page.
        check(pad_to(game, 'Rename Page', 'up'), f'never reached Rename Page: {game.focused_label()}')
        game.pad('south')
        game.wait_targets()
        check(declared(game).get('Cancel') is True, f'the name page declares {declared(game)}')
        game.pad('south')
        game.pad('south')
        game.pad('east')
        game.pad('east')
        game.wait_targets()
        kept = [label for label in labels(game) if label.startswith('New Page') and label != 'New Page']
        check(kept == renamed, f'Cancel changed the name: {kept}, not {renamed}')
        check(game.focused_label() == 'Rename Page', f'Cancel left the focus on {game.focused_label()}')

        # Delete acts at once, on the page shown.
        check(pad_to(game, 'Delete Page', 'down'), f'never reached Delete Page: {game.focused_label()}')
        game.pad('south')
        check(pages() == 3 and not any(label in labels(game) for label in renamed), f'{pages()} pages after Delete: {labels(game)}')

        # Reset, at once too, then greyed.
        check(pad_to(game, 'Reset All Pages', 'down'), f'never reached Reset All Pages: {game.focused_label()}')
        game.pad('south')
        check(pages() == 2 and not has_label(game, 'Mine'), f'{pages()} pages after Reset: {labels(game)}')
        check(enabled('Reset All Pages') is False, 'Reset stays live at the defaults')
        shot(game, 'radial_editor_reset')

        # Undo, by the pad, puts back the three pages and the slot as the screen found them.
        game.pad('left')
        check(pad_to(game, 'Undo', 'down'), f'never reached Undo: {game.focused_label()}')
        game.pad('south')
        check(pages() == 3 and has_label(game, 'Mine'), f'{pages()} pages after Undo: {labels(game)}')
        check(radial(game)['slots'][0] == 'key.advancements', f"Undo left the slots {radial(game)['slots']}")
        check('Apply' not in declared(game), 'Undo stayed after undoing')
        check('radial_2_label:Mine' in saved(), f'saved after Undo: {saved()}')

        # Down to one page: Delete greys, and says why.
        game.pad('right')
        for _ in range(2):
            check(pad_to(game, 'Delete Page', 'down'), f'never reached Delete Page: {game.focused_label()}')
            game.pad('south')
        check(pages() == 1, f'{pages()} pages after two deletes')
        check(enabled('Delete Page') is False, 'the last page can be deleted')
        shot(game, 'radial_editor_one_page')

        # The opener's row waits on itself for a button: LB binds; Y is refused, Start cancels.
        check(pad_to(game, OPENER_ROW, 'down'), f'never reached the opener: {game.focused_label()}')
        shot(game, 'radial_editor_opener')
        game.pad('south')
        check(has_label(game, 'Press the button for this.'), f'the opener does not wait: {labels(game)}')
        check(not has_label(game, 'Radial Menu...'), 'the opener opened Bindings')
        shot(game, 'radial_editor_opener_waiting')
        game.pad('lb')
        check(game.focused_label() == OPENER_LB, f'LB left the opener as {game.focused_label()}')
        check('bind_pad.radial:lb' in saved(), f'the opener was not saved: {saved()}')
        shot(game, 'radial_editor_opener_lb')
        for refused in ('north', 'start'):
            game.pad('south')
            game.pad(refused)
            check(game.focused_label() == OPENER_LB, f'{refused} left the opener as {game.focused_label()}')
            check(has_label(game, OPENER_LB), f'{refused} left the page: {labels(game)}')
        game.pad('east')
        game.wait_targets()
        check(not has_label(game, OPENER_ROW), 'B did not leave the radial page')
        game.restart()
        check(pages() == 1, f'{pages()} pages after the restart')
    finally:
        game.quit()
    return check.failures


def radial_held_sneak(args, name):
    def seed(home):
        # Sneak held on its stick click, not toggled: what the menu must not let go of.
        (home / 'horizon-controller.txt').write_text('toggleSneak:false\nbindLayout:1\n')
    game = Game(args, name, world=True, seed=seed)
    check = Check()
    sneaking = lambda: 'ShiftLeft' in game.ctl('status')['keys']
    try:
        game.ctl('pad', 'focus', 'true')
        time.sleep(SETTLE)
        game.ctl('pad', 'rs', 'down')
        time.sleep(SETTLE)
        check(sneaking(), 'holding the stick click did not sneak')
        hold_opener(game)
        check(radial(game)['open'], 'holding the opener did not open it')
        time.sleep(SETTLE)
        check(sneaking(), 'opening the menu stood the player up')
        # A press while it's open reaches no mapping, down or up.
        game.ctl('pad', 'south', 'down')
        time.sleep(0.2)
        check('Space' not in game.ctl('status')['keys'], 'A jumped through the open menu')
        game.ctl('pad', 'south', 'up')
        time.sleep(0.2)
        check(radial(game)['open'], 'A closed the menu')
        # An action that opens no screen (a screen would let go of everything anyway): Hide HUD,
        # on the camera page.
        game.ctl('pad', 'rb', 'tap')
        time.sleep(0.2)
        aim(game, 1, 0, settle=SETTLE)
        check(sneaking(), 'aiming stood the player up')
        game.ctl('pad', 'right', 'up')
        game.ctl('pad', 'stick', 'right', 0, 0)
        time.sleep(SETTLE)
        check(radial(game)['runs'] == ['key.toggleGui'], f"ran {radial(game)['runs']}")
        check(sneaking(), 'the commit stood the player up')
        game.ctl('pad', 'rs', 'up')
        time.sleep(SETTLE)
        check(not sneaking(), 'still sneaking after the stick click came up')
    finally:
        game.quit()
    return check.failures


def radial_focus_background(args, name):
    def seed(home):
        # The pad keeps playing without the window's focus: the menu still ends with it.
        (home / 'horizon-controller.txt').write_text('playInBackground:true\nbindLayout:2\n')
    game = Game(args, name, world=True, seed=seed)
    check = Check()
    try:
        game.ctl('pad', 'focus', 'true')
        time.sleep(SETTLE)
        hold_opener(game)
        aim(game, 0, 1)
        check(radial(game)['selected'] == 0, f'nothing selected: {radial(game)}')
        game.ctl('pad', 'focus', 'false')
        time.sleep(0.2)
        check(not radial(game)['open'], 'focus loss left it open with Play in Background on')
        game.ctl('pad', 'right', 'up')
        game.ctl('pad', 'stick', 'right', 0, 0)
        time.sleep(SETTLE)
        check(radial(game)['runs'] == [], f"cancelled but ran {radial(game)['runs']}")
        # Still playing in the background: the opener opens it again, unfocused.
        hold_opener(game)
        check(radial(game)['open'], 'the opener did nothing in the background')
        game.ctl('pad', 'right', 'up')
    finally:
        game.quit()
    return check.failures


def radial_unavailable(args, name):
    def seed(home):
        # One page: Emote up, Commands right.
        (home / 'horizon-controller.txt').write_text(
            'bindLayout:2\nradial_0_name:horizon.radial.page.general\nradial_0_slots:key.emote,,key.command,,,,,\n')
    game = Game(args, name, world=True, seed=seed, env={'HORIZON_COSMETICS': '0'})
    check = Check()
    try:
        game.ctl('pad', 'focus', 'true')
        time.sleep(SETTLE)
        screen = game.ctl('ui')['screen']
        hold_opener(game)
        aim(game, 0, 1)
        state = radial(game)
        check(state['pages'] == 1 and state['slots'][0] == 'horizon:emote', f'not the saved page: {state}')
        check(state['selected'] == 0, f'up selected {state["selected"]}')
        check(state['available'][0] is False and state['available'][2] is True, f"available: {state['available']}")
        shot(game, 'radial_single_page_unavailable')
        game.ctl('pad', 'right', 'up')
        game.ctl('pad', 'stick', 'right', 0, 0)
        time.sleep(SETTLE)
        state = radial(game)
        check(state['runs'] == [] and state['run_count'] == 0, f"an unavailable action ran: {state['runs']}")
        check(game.ctl('ui')['screen'] == screen, 'an unavailable action opened a screen')
        # The one beside it still runs.
        hold_opener(game)
        aim(game, 1, 0)
        game.ctl('pad', 'right', 'up')
        game.ctl('pad', 'stick', 'right', 0, 0)
        time.sleep(SETTLE)
        check(radial(game)['runs'] == ['key.command'], f"ran {radial(game)['runs']}")
    finally:
        game.quit()
    return check.failures


def radial_old_file(args, name):
    def seed(home):
        (home / 'horizon-controller.txt').write_text('vibration:false\n')
    game = Game(args, name, world=True, seed=seed)
    check = Check()
    try:
        game.ctl('pad', 'focus', 'true')
        time.sleep(SETTLE)
        before = game.ctl('ui')['screen']
        game.ctl('pad', 'down', 'tap')
        time.sleep(SETTLE)
        check(radial(game)['runs'] == [], 'the old file ran a radial action')
        check(game.ctl('ui')['screen'] != before, 'D-pad down no longer opens the advancements')
    finally:
        game.quit()
    return check.failures


def seeded_radial(text, env=None):
    """A radial case whose game starts from this controller file."""
    def wrap(body):
        def case(args, name):
            game = Game(args, name, world=True, env=env,
                        seed=lambda home: (home / 'horizon-controller.txt').write_text(text))
            check = Check()
            try:
                game.ctl('pad', 'focus', 'true')
                time.sleep(SETTLE)
                body(game, check)
            finally:
                game.quit()
            return check.failures
        return case
    return wrap


@seeded_radial('bindLayout:2\nradial_0_name:horizon.radial.page.general\nradial_0_slots:key.inventory,,,,,,,\n')
def radial_commit_screen(game, check):
    hold_opener(game)
    aim(game, 0, 1)
    check(radial(game)['drawn'], 'the overlay is not drawn while it is held')
    game.ctl('pad', 'right', 'up')
    # Every poll for half a second: once the inventory is up, the overlay is gone.
    end = time.time() + 0.5
    over = []
    while time.time() < end:
        screen = game.ctl('ui')['screen']
        if screen != 'playing' and radial(game)['drawn']:
            over.append(screen)
    check(not over, f'the overlay drew over {over[:3]}')
    # Creative's inventory, in this creative world.
    check(game.ctl('ui')['screen'] != 'playing', 'no inventory opened')
    shot(game, 'radial_commit_screen')
    game.ctl('pad', 'stick', 'right', 0, 0)


def radial_nintendo_glyphs(args, name):
    check = Check()
    for letters, cancel in ((True, 'B'), (False, 'A')):
        seed = seed_controller(f'glyphs:nintendo\nnintendoLetters:{str(letters).lower()}\n')
        game = Game(args, f'{name}-{cancel}', world=True, seed=seed)
        try:
            game.ctl('pad', 'focus', 'true')
            game.ctl('pad', 'family', 'nintendo')
            time.sleep(SETTLE)
            hold_opener(game)
            glyphs = radial(game)['glyphs']
            check(glyphs['pages'] == ['L', 'R'], f'letters={letters}: bumpers {glyphs}')
            check(glyphs['cancel'] == cancel, f'letters={letters}: cancel {glyphs}')
            shot(game, f'radial_nintendo_{"letters" if letters else "places"}')
            game.ctl('pad', 'east', 'tap')
            game.ctl('pad', 'right', 'up')
        finally:
            game.quit()
    return check.failures


def let_go(game):
    game.ctl('pad', 'right', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(SETTLE)


def counts(game):
    return game.ctl('inventory').get('counts')


def stack_of(game, item):
    return sum(n for name, n in (counts(game) or {}).items() if name.endswith(item))


@radial_case
def radial_flick_with_press(game, check):
    # The thumb rolls onto the opener as the other flicks: both at once.
    game.ctl('pad', 'stick', 'right', 0, 1)
    game.ctl('pad', 'right', 'down')
    time.sleep(0.3)
    state = radial(game)
    check(state['selected'] == 0 and not state['waiting'], f'a flick with the press: {state}')
    let_go(game)
    check(radial(game)['runs'] == ['key.advancements'], f"ran {radial(game)['runs']}")
    game.ctl('key', 'escape', 'tap')
    time.sleep(SETTLE)
    # Leaning since well before: turning the camera, so it waits to be let go.
    game.ctl('pad', 'stick', 'right', 0, 1)
    time.sleep(0.4)
    game.ctl('pad', 'right', 'down')
    time.sleep(0.2)
    state = radial(game)
    check(state['waiting'] and state['selected'] is None, f'a lean from before: {state}')
    let_go(game)
    check(radial(game)['run_count'] == 1, f"ran {radial(game)['runs']}")


@radial_case
def radial_held_drop(game, check):
    game.ctl('command', 'clear @s')
    game.ctl('command', 'item replace entity @s hotbar.0 with minecraft:dirt 64')
    game.ctl('select', '1')
    time.sleep(SETTLE)
    game.ctl('pad', 'down', 'down')
    time.sleep(0.05)
    game.ctl('pad', 'right', 'down')
    # Held past the drop's stack hold, with the menu open.
    time.sleep(1.6)
    check(radial(game)['open'], 'the opener did not open it')
    during = stack_of(game, 'dirt')
    check(0 < during < 64, f'no drop before the open, or the stack went with it open: {counts(game)}')
    game.ctl('pad', 'right', 'up')
    time.sleep(0.15)
    after = stack_of(game, 'dirt')
    check(after == during, f'the menu closing threw {during - after} at once')
    # Still held, the drop starts its wait again and then goes on.
    time.sleep(1.0)
    check(stack_of(game, 'dirt') < after, f'the held drop never resumed: {counts(game)}')
    game.ctl('pad', 'down', 'up')


@seeded_radial('bindLayout:2\n')
def radial_death(game, check):
    for rule in ('immediate_respawn', 'doImmediateRespawn'):
        try:
            game.ctl('command', f'gamerule {rule} true')
            break
        except Exception:
            continue
    game.ctl('command', 'gamemode survival')
    time.sleep(SETTLE)
    hold_opener(game)
    aim(game, 0, 1)
    check(radial(game)['selected'] == 0, f'nothing selected: {radial(game)}')
    game.ctl('command', 'kill @s')
    time.sleep(1.0)
    # Respawned at once, no death screen: what closed it is the new body, not a screen.
    check(game.ctl('ui')['screen'] == 'playing', f"no immediate respawn: {game.ctl('ui')['screen']}")
    check(not radial(game)['open'], 'the menu outlived the death')
    let_go(game)
    check(radial(game)['runs'] == [], f"ran {radial(game)['runs']} on the new body")
    check(game.ctl('ui')['screen'] != 'advancements', 'the advancements opened after the death')



@radial_case
def radial_cancel_roll(game, check):
    # B cancels with the opener still held; the thumb lets it go later and rolls onto Drop.
    game.ctl('command', 'clear @s')
    game.ctl('command', 'item replace entity @s hotbar.0 with minecraft:stone 64')
    game.ctl('select', '1')
    time.sleep(SETTLE)
    hold_opener(game)
    aim(game, 0, 1)
    game.ctl('pad', 'east', 'tap')
    time.sleep(0.25)
    check(not radial(game)['open'], f'B did not cancel: {radial(game)}')
    game.ctl('pad', 'right', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(0.03)
    game.ctl('pad', 'down', 'down')
    time.sleep(0.3)
    game.ctl('pad', 'down', 'up')
    time.sleep(SETTLE)
    check(stack_of(game, 'stone') == 64, f'the rolled thumb dropped after a cancel: {counts(game)}')
    check(radial(game)['runs'] == [], f"ran {radial(game)['runs']}")
    # Past the settle a press is the player's own.
    game.ctl('pad', 'down', 'tap')
    time.sleep(SETTLE)
    check(stack_of(game, 'stone') == 63, f'a later D-pad Down dropped nothing: {counts(game)}')


@seeded_radial('bindLayout:2\nradialMode:toggle\n')
def radial_toggle_roll(game, check):
    # Toggle mode commits on the opener's second press: the thumb comes up after the close.
    game.ctl('command', 'clear @s')
    game.ctl('command', 'item replace entity @s hotbar.0 with minecraft:stone 64')
    game.ctl('select', '1')
    time.sleep(SETTLE)
    game.ctl('pad', 'right', 'tap')
    time.sleep(0.2)
    check(radial(game)['latched'], f'a tap did not latch it: {radial(game)}')
    aim(game, 0, 1)
    game.ctl('pad', 'right', 'down')
    time.sleep(0.25)
    check(not radial(game)['open'], f'the second press did not commit: {radial(game)}')
    game.ctl('pad', 'right', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(0.03)
    game.ctl('pad', 'down', 'down')
    time.sleep(0.3)
    game.ctl('pad', 'down', 'up')
    time.sleep(SETTLE)
    check(stack_of(game, 'stone') == 64, f'the rolled thumb dropped after a toggle commit: {counts(game)}')
    check(radial(game)['runs'] == ['key.advancements'], f"ran {radial(game)['runs']}")
    if game.ctl('ui')['screen'] != 'playing':
        game.ctl('key', 'escape', 'tap')


@seeded_radial('bindLayout:2\nradialStick:either\nbind_pad.radial:rb\n')
def radial_either_takeover(game, check):
    # Walking with the left stick, the opener down: waiting for the left; the right flicks and selects.
    game.ctl('pad', 'stick', 'left', 0, 1)
    time.sleep(0.4)
    game.ctl('pad', 'rb', 'down')
    time.sleep(0.2)
    check(radial(game)['waiting'], f'the walking stick does not wait: {radial(game)}')
    aim(game, 0, 1, settle=0.2)
    state = radial(game)
    check(state['selecting'] == 1 and state['selected'] == 0 and not state['waiting'],
          f'the right stick taking over: {state}')
    game.ctl('pad', 'rb', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    game.ctl('pad', 'stick', 'left', 0, 0)
    time.sleep(SETTLE)
    check(radial(game)['runs'] == ['key.advancements'], f"the first flick ran {radial(game)['runs']}")
    if game.ctl('ui')['screen'] != 'playing':
        game.ctl('key', 'escape', 'tap')
        time.sleep(SETTLE)
    # The walking thumb, frozen while it's open, eases off and on again: it never takes over.
    game.ctl('pad', 'stick', 'left', 0, 0.6)
    time.sleep(0.4)
    game.ctl('pad', 'rb', 'down')
    time.sleep(0.2)
    aim(game, 1, 0, settle=0.2)
    check(radial(game)['selected'] == 2, f'the right stick: {radial(game)}')
    game.ctl('pad', 'stick', 'right', 0, 0)
    for y in (0.4, 0.7, 0.4, 0.75):
        game.ctl('pad', 'stick', 'left', 0, y)
        time.sleep(0.04)
    state = radial(game)
    check(state['selecting'] == 1 and state['selected'] == 2, f'the walking thumb took over: {state}')
    game.ctl('pad', 'rb', 'up')
    game.ctl('pad', 'stick', 'left', 0, 0)
    time.sleep(SETTLE)
    check(radial(game)['runs'][-1:] == ['key.command'], f"ran {radial(game)['runs']}")
    if game.ctl('ui')['screen'] != 'playing':
        game.ctl('key', 'escape', 'tap')


def radial_stick_back(args, name):
    """Selects With stepped from Left (Y opening it) back to Right: D-pad Right opens it again. Right and Left
    step the row, as every shell choice."""
    seed = lambda home: (home / 'horizon-controller.txt').write_text(
        'bindLayout:2\nradialStick:left\nbind_pad.radial:north\n')
    game = Game(args, name, world=True, seed=seed)
    check = Check()
    row = 'Radial Menu Selects With'
    try:
        game.ctl('pad', 'focus', 'true')
        time.sleep(SETTLE)
        game.ctl('options', 'controller')
        time.sleep(SETTLE)
        game.wait_targets()
        # The first press lands the focus on the page list (General); then into its rows, and down
        # to the radial group.
        game.pad('down')
        game.pad('right')
        seen = []
        for _ in range(60):
            seen.append(game.focused_label())
            if (game.focused_label() or '').startswith(row):
                break
            game.pad('down')
        label = game.focused_label() or ''
        check(label.startswith(row) and 'Left' in label, f'never reached the row: {label}, passing {seen}')
        shot(game, 'radial_stick_left')
        # Right and Left step it as every shell choice does, and the focus stays on the row.
        game.pad('right')
        stepped = game.focused_label() or ''
        game.pad('left')
        back = game.focused_label() or ''
        check(stepped.startswith(row) and 'Either' in stepped and back.startswith(row) and 'Left' in back,
              f'Right and Left did not step the row: {stepped}, then {back}')
        # A steps it on: Either (no opener, Y being under the right thumb too), then Right.
        steps = []
        for _ in range(2):
            game.pad('south')
            steps.append(game.focused_label() or '')
        label = steps[-1]
        check('Either' in steps[0] and 'Right' in label, f'A did not step it round to the right stick: {steps}')
        shot(game, 'radial_stick_back_right')
        for _ in range(3):
            if game.ctl('ui')['screen'] == 'playing':
                break
            game.pad('east')
        time.sleep(SETTLE)
        check('radialStick:left' not in controller_saved(game), f'not saved: {controller_saved(game)}')
        game.ctl('pad', 'right', 'down')
        time.sleep(0.2)
        check(radial(game)['open'], f'D-pad Right does not open it on the right stick: {radial(game)}')
        game.ctl('pad', 'right', 'up')
    finally:
        game.quit()
    return check.failures


def radial_status_rows(args, name):
    """Survival with armour and absorption: the dial draws clear of the rows over the hotbar, at
    427x240 and 320x180 (shots only; the layout's own test checks the numbers)."""
    check = Check()
    for w, h in ((1280, 720), (320, 180)):
        game = Game(args, f'{name}-{w}', world=True, extra=('--width', w, '--height', h))
        try:
            game.ctl('pad', 'focus', 'true')
            time.sleep(SETTLE)
            game.ctl('command', 'gamemode survival')
            game.ctl('command', 'item replace entity @s armor.chest with minecraft:diamond_chestplate')
            game.ctl('command', 'effect give @s minecraft:absorption 60 1 true')
            time.sleep(SETTLE)
            hold_opener(game)
            aim(game, 0, -1)
            check(radial(game)['drawn'], f'{w}x{h}: not drawn')
            shot(game, f'radial_status_rows_{w}x{h}')
            game.ctl('pad', 'east', 'tap')
            let_go(game)
        finally:
            game.quit()
    return check.failures

# One page: Hide HUD up, Commands right, the rest empty.
HIDE_HUD_SEED = ('bindLayout:2\nradial_0_name:horizon.radial.page.general\n'
                 'radial_0_slots:key.toggleGui,,key.command,,,,,\n')


@seeded_radial(HIDE_HUD_SEED)
def radial_dpad_roll(game, check):
    game.ctl('command', 'clear @s')
    game.ctl('command', 'item replace entity @s hotbar.0 with minecraft:stone 10')
    game.ctl('select', '1')
    time.sleep(SETTLE)
    hold_opener(game)
    aim(game, 0, 1)
    # Off D-pad Right, then onto D-pad Down (Drop) a poll or two later, inside the settle.
    game.ctl('pad', 'right', 'up')
    time.sleep(0.03)
    game.ctl('pad', 'down', 'down')
    time.sleep(0.3)
    game.ctl('pad', 'down', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(SETTLE)
    check(radial(game)['runs'] == ['key.toggleGui'], f"ran {radial(game)['runs']}")
    check(stack_of(game, 'stone') == 10, f'the rolled thumb dropped: {counts(game)}')
    # Past the settle a press is the player's own.
    game.ctl('pad', 'down', 'tap')
    time.sleep(SETTLE)
    check(stack_of(game, 'stone') == 9, f'a later D-pad Down dropped nothing: {counts(game)}')
    # The opener again just after a close opens the menu.
    hold_opener(game)
    game.ctl('pad', 'right', 'up')
    time.sleep(0.03)
    game.ctl('pad', 'right', 'down')
    time.sleep(0.2)
    check(radial(game)['open'], 'the opener right after a close was eaten')
    game.ctl('pad', 'right', 'up')
    time.sleep(SETTLE)
    # Hide HUD back off.
    hold_opener(game)
    aim(game, 0, 1)
    let_go(game)


@seeded_radial(HIDE_HUD_SEED)
def radial_fight_back(game, check):
    game.ctl('command', 'clear @s')
    game.ctl('command', 'item replace entity @s hotbar.0 with minecraft:stone 10')
    game.ctl('select', '1')
    time.sleep(SETTLE)
    swings = lambda: game.ctl('state')['swings']
    # RT: the menu closes and the swing goes on, in the same poll.
    hold_opener(game)
    aim(game, 0, 1)
    was = swings()
    game.ctl('pad', 'trigger', 'right', 1)
    time.sleep(0.3)
    check(not radial(game)['open'], 'RT left it open')
    check(swings() > was, 'RT did not swing')
    game.ctl('pad', 'trigger', 'right', 0)
    let_go(game)
    check(radial(game)['runs'] == [], f"RT's cancel ran {radial(game)['runs']}")
    # A key: Q closes it and drops.
    hold_opener(game)
    aim(game, 0, 1)
    game.ctl('key', 'q', 'tap')
    time.sleep(SETTLE)
    check(not radial(game)['open'], 'Q left it open')
    check(stack_of(game, 'stone') == 9, f'Q was swallowed: {counts(game)}')
    let_go(game)
    # A click: closes it and swings.
    hold_opener(game)
    aim(game, 0, 1)
    was = swings()
    game.ctl('mouse', 'left', 'click')
    time.sleep(SETTLE)
    check(not radial(game)['open'], 'a click left it open')
    check(swings() > was, 'the click was swallowed')
    let_go(game)
    # An agent's mouse glide (its turn tool, not the window's raw motion, which
    # `device_event` drops while it's open) leaves it open.
    hold_opener(game)
    aim(game, 0, 1)
    game.ctl('mouse-move', 200, 0)
    time.sleep(SETTLE)
    check(radial(game)['open'], 'the mouse moving closed it')
    game.ctl('pad', 'east', 'tap')
    let_go(game)
    check(radial(game)['runs'] == [], f"cancelled but ran {radial(game)['runs']}")


@seeded_radial('bindLayout:2\nradialMode:toggle\n')
def radial_toggle(game, check):
    game.ctl('pad', 'right', 'tap')
    time.sleep(SETTLE)
    state = radial(game)
    check(state['open'] and state['latched'] and state['shown'], f'a tap did not open it: {state}')
    aim(game, 0, 1)
    # The right thumb lets go of the stick to press A: the selection waits for it.
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(0.5)
    check(radial(game)['selected'] == 0, f"letting go lost the selection: {radial(game)}")
    game.ctl('pad', 'south', 'tap')
    time.sleep(SETTLE)
    check(radial(game)['runs'] == ['key.advancements'], f"A ran {radial(game)['runs']}")
    game.ctl('key', 'escape', 'tap')
    time.sleep(SETTLE)
    # The opener again commits.
    game.ctl('pad', 'right', 'tap')
    time.sleep(SETTLE)
    aim(game, 1, 0)
    game.ctl('pad', 'right', 'tap')
    time.sleep(SETTLE)
    check(radial(game)['runs'][-1:] == ['key.command'], f"the opener again ran {radial(game)['runs']}")
    game.ctl('pad', 'stick', 'right', 0, 0)
    game.ctl('key', 'escape', 'tap')
    time.sleep(SETTLE)
    # B closes it; nothing aimed runs nothing.
    game.ctl('pad', 'right', 'tap')
    time.sleep(SETTLE)
    game.ctl('pad', 'east', 'tap')
    time.sleep(SETTLE)
    check(not radial(game)['open'], 'B left it open')
    game.ctl('pad', 'right', 'tap')
    time.sleep(SETTLE)
    game.ctl('pad', 'south', 'tap')
    time.sleep(SETTLE)
    state = radial(game)
    check(not state['open'] and state['run_count'] == 2, f'nothing aimed: {state}')


# Selecting with the left stick, D-pad Right is under that thumb: Y opens it instead.
@seeded_radial('bindLayout:2\nradialStick:left\nbind_pad.radial:north\n')
def radial_left_stick(game, check):
    position = lambda: game.ctl('status')['player']['position']
    yaw = lambda: game.ctl('status')['yaw']
    game.ctl('pad', 'north', 'down')
    time.sleep(0.2)
    check(radial(game)['open'], f'Y did not open it: {radial(game)}')
    before = position()
    game.ctl('pad', 'stick', 'left', 0, 1)
    time.sleep(0.4)
    check(radial(game)['selected'] == 0, f'the left stick selected {radial(game)}')
    check(position() == before, f'the selecting stick walked: {before} -> {position()}')
    was = yaw()
    aim(game, 1, 0, settle=0.3)
    check(yaw() != was, 'the right stick no longer turns the view')
    game.ctl('pad', 'stick', 'right', 0, 0)
    game.ctl('pad', 'north', 'up')
    game.ctl('pad', 'stick', 'left', 0, 0)
    time.sleep(SETTLE)
    check(radial(game)['runs'] == ['key.advancements'], f"ran {radial(game)['runs']}")


@seeded_radial('bindLayout:2\nbind_pad.radial:lb\n')
def radial_bumper_opener(game, check):
    rumble = lambda: (game.ctl('pad', 'status')['last_rumble'] or {}).get('reason')
    game.ctl('pad', 'lb', 'down')
    time.sleep(0.2)
    check(radial(game)['open'], 'LB did not open it')
    game.ctl('pad', 'rb', 'tap')
    time.sleep(0.2)
    check(radial(game)['page'] == 1 and rumble() == 'radial_page', f'RB: {radial(game)} {rumble()}')
    game.ctl('pad', 'rb', 'tap')
    time.sleep(0.3)
    check(radial(game)['page'] == 0, f'RB did not go round: {radial(game)}')
    check(rumble() == 'radial_wrap', f'no second tick going round: {rumble()}')
    aim(game, 0, 1)
    game.ctl('pad', 'lb', 'up')
    game.ctl('pad', 'stick', 'right', 0, 0)
    time.sleep(SETTLE)
    check(radial(game)['runs'] == ['key.advancements'], f"ran {radial(game)['runs']}")


@radial_case
def radial_haptics(game, check):
    rumble = lambda: (game.ctl('pad', 'status')['last_rumble'] or {}).get('reason')
    hold_opener(game)
    aim(game, 0.7, 0.7)
    check(radial(game)['selected'] == 1, f'not on the empty slot: {radial(game)}')
    check(rumble() != 'radial', f'an empty slot ticked: {rumble()}')
    aim(game, 0, 1)
    check(rumble() == 'radial', f'a filled slot did not tick: {rumble()}')
    aim(game, 0.7, 0.7)
    let_go(game)
    check(radial(game)['runs'] == [], f"ran {radial(game)['runs']}")
    check(rumble() == 'radial_nothing', f'a release that ran nothing: {rumble()}')


@radial_case
def radial_narration(game, check):
    game.ctl('options-set', 'narrator', '1')
    time.sleep(SETTLE)
    said = lambda: game.ctl('narration')['recent']
    hold_opener(game)
    time.sleep(0.2)
    check('General' in said(), f'the page was not said: {said()}')
    aim(game, 0, 1, settle=0.4)
    check('Advancements' in said(), f'the slot was not said: {said()}')
    game.ctl('pad', 'east', 'tap')
    let_go(game)


ACTIONS_SEED = ('bindLayout:2\nradial_0_label:Mine\n'
                'radial_0_slots:key.debug.overlay,mymod:dash,key.quickActions,,key.socialInteractions,,,\n'
                'radial_0_icon:minecraft:diamond\n')


@seeded_radial(ACTIONS_SEED)
def radial_actions(game, check):
    hold_opener(game)
    state = radial(game)
    check(state['slots'][1] == 'mymod:dash', f'the unknown action: {state}')
    check(state['available'] == [True, False, False, True, False, True, True, True],
          f"available: {state['available']}")
    shot(game, 'radial_actions')
    aim(game, 0.7, 0.7)
    let_go(game)
    check(radial(game)['runs'] == [], f"a missing action ran {radial(game)['runs']}")
    # The debug screen.
    hold_opener(game)
    aim(game, 0, 1)
    let_go(game)
    check(game.ctl('status')['debug']['overlay'], 'the debug screen did not show')
    hold_opener(game)
    aim(game, 0, 1)
    let_go(game)
    check(not game.ctl('status')['debug']['overlay'], 'the debug screen did not hide')
    # Social in single player, dimmed, goes through its key: the action bar says why, and
    # nothing opens.
    game.ctl('options-set', 'narrator', '1')
    hold_opener(game)
    aim(game, 0, -1)
    let_go(game)
    check(game.ctl('ui')['screen'] == 'playing', f"Social opened: {game.ctl('ui')['screen']}")
    said = game.ctl('narration')['recent']
    check(any('only available in multiplayer' in line.lower() for line in said), f'nothing said: {said}')


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
         'controller_nintendo_buttons': controller_nintendo_buttons,
         'controller_settings_keys': controller_settings_keys,
         'controller_notices': controller_notices,
         'every_screen_has_targets': every_screen_has_targets,
         'studio_rows': studio_rows, 'pause_icon': pause_icon,
         'erase_checkbox': erase_checkbox,
         'extras_shell': extras_shell,
         'extras_shell_edges': extras_shell_edges,
         'graphics_shell': graphics_shell,
         'shell_choice_step': shell_choice_step,
         'shader_screens': shader_screens,
         'pack_card_options': pack_card_options,
         'glyph_art': glyph_art,
         'glyph_art_world': glyph_art_world,
         'nintendo_letters': nintendo_letters,
         'key_row_a_x': key_row_a_x,
         'prompt_dialog': prompt_dialog,
         'prompt_dialog_field': prompt_dialog_field,
         'prompt_keeps_typing': prompt_keeps_typing,
         'prompt_keeps_crosshair': prompt_keeps_crosshair,
         'osk_social': osk_social,
         'osk_book': osk_book,
         'recipe_search_keyboard': recipe_search_keyboard,
         'chat_suggestion_rows': chat_suggestion_rows,
         'server_list_bumpers': server_list_bumpers,
         'osk_sign': osk_sign,
         'osk_debug_options': osk_debug_options,
         'osk_command_block': osk_command_block,
         'osk_jigsaw': osk_jigsaw,
         'controller_trigger_threshold': controller_trigger_threshold,
         'radial_commit': radial_commit, 'radial_no_selection': radial_no_selection,
         'radial_flick_with_press': radial_flick_with_press, 'radial_held_drop': radial_held_drop,
         'radial_death': radial_death, 'radial_dpad_roll': radial_dpad_roll,
         'radial_fight_back': radial_fight_back, 'radial_toggle': radial_toggle,
         'radial_left_stick': radial_left_stick, 'radial_bumper_opener': radial_bumper_opener,
         'radial_haptics': radial_haptics, 'radial_narration': radial_narration,
         'radial_actions': radial_actions,
         'radial_retention': radial_retention, 'radial_pages': radial_pages, 'radial_cancel': radial_cancel,
         'radial_camera': radial_camera, 'radial_precedence': radial_precedence, 'radial_editor': radial_editor,
         'radial_held_sneak': radial_held_sneak, 'radial_focus_background': radial_focus_background,
         'radial_unavailable': radial_unavailable, 'radial_old_file': radial_old_file,
         'radial_commit_screen': radial_commit_screen, 'radial_nintendo_glyphs': radial_nintendo_glyphs,
         'radial_cancel_roll': radial_cancel_roll, 'radial_toggle_roll': radial_toggle_roll,
         'radial_either_takeover': radial_either_takeover, 'radial_stick_back': radial_stick_back,
         'radial_status_rows': radial_status_rows,
         'slider_needs_a': slider_needs_a,
         'gameplay_guide': gameplay_guide,
         'gameplay_guide_basic': gameplay_guide_basic,
         'gameplay_guide_boat_and_keys': gameplay_guide_boat_and_keys,
         'gameplay_guide_off': gameplay_guide_off, 'gameplay_guide_mobs': gameplay_guide_mobs,
         'craft_direct': craft_direct, 'trigger_rules': trigger_rules, 'craft_tap_hold': craft_tap_hold,
         'craft_release_a': craft_release_a, 'craft_release_lt': craft_release_lt,
         'craft_release_press': craft_release_press, 'craft_remainder': craft_remainder,
         'craft_full': craft_full, 'craft_grid': craft_grid, 'craft_carrying': craft_carrying,
         'craft_lan_guest': craft_lan_guest, 'craft_dedicated': craft_dedicated}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--game', type=Path, default=lab.cargo_target() / f'fast/horizon-client{EXE}')
    parser.add_argument('--ctl', type=Path, default=lab.cargo_target() / f'fast/horizon-ctl{EXE}')
    parser.add_argument('--resources', type=Path, default=ROOT / '.local/horizon-client/26.3')
    parser.add_argument('--only', nargs='*', choices=sorted(CASES), help='run only these cases')
    parser.add_argument('--shots', type=Path, help='keep screenshots in this folder')
    parser.add_argument('--jobs', type=int, default=1, help='cases at once; each has its own game and home')
    args = parser.parse_args()
    args.game, args.ctl, args.resources = args.game.resolve(), args.ctl.resolve(), args.resources.resolve()
    def run(name):
        # A case that dies takes only itself down, and leaves no game behind it.
        try:
            return name, CASES[name](args, name)
        except Exception as error:
            return name, [f'{type(error).__name__}: {error}']

    failed = []
    with ThreadPoolExecutor(args.jobs) as pool:
        for name, failures in pool.map(run, args.only or CASES):
            for failure in failures:
                print(f'FAIL {name}: {failure}', flush=True)
            if failures:
                failed.append(name)
            else:
                print(f'ok   {name}', flush=True)
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
