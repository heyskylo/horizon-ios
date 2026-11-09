#!/usr/bin/env python3
"""A multi-display recipe reaches a real client's recipe book (server-recipe-book-display-variants).

`ServerRecipeBook.addRecipes` sends one `ClientboundRecipeBookAddPacket.Entry` per display a recipe has
(`RecipeManager.listDisplaysForRecipe`), and `removeRecipes` a `ClientboundRecipeBookRemovePacket` of all of
them. `minecraft:map_cloning` is a `crafting_transmute` recipe whose `material_count` runs from 1 to 8:
`TransmuteRecipe.display()` gives it eight shapeless displays.

A real network player (LocatorBarClient) joins a dedicated server (vanilla, then Horizon) with the
menu recorder on: every recipe book Add and Remove packet is kept with its raw payload and its fields as
vanilla's own codec decodes them (the client refuses a payload with bytes left over). With the recipe book
emptied, the player is given `minecraft:crafting_table` (an ordinary shaped recipe, the positive), then
`minecraft:map_cloning`, then both are taken back, map cloning first. After each step the player's
`recipeBook` is read through command storage.

Primary slice, declared before any fixed capture: `target-` keys are the map cloning grant's Add entries
and its Remove ids. `diag-` keys are recorded diagnostics (the join packets, absolute ids). Every other key is
a setup admission or a control.
"""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

import anvil_dump as nbt
import test_locator_bar_parity as net
from test_disconnect_crafting_parity import generate, wait_new

PLAYER = 'Bob'
STEPS = [
    ('take-all', 'recipe take Bob *'),
    ('give-ordinary', 'recipe give Bob minecraft:crafting_table'),
    ('give-transmute', 'recipe give Bob minecraft:map_cloning'),
    ('take-transmute', 'recipe take Bob minecraft:map_cloning'),
    ('take-ordinary', 'recipe take Bob minecraft:crafting_table'),
]
TARGET = {'give-transmute', 'take-transmute'}


def run(label, command, client_command, build, world):
    directory = build / label
    port = net.server_dir(directory, world)
    server = net.Server(command, directory, label)
    wire = build / f'{label}-wire.txt'
    log = (build / f'{label}-client.log').open('w')
    client = None
    try:
        server.wait_for(r'Done \(')
        client = net.Client(client_command, port, wire, log)
        client.tell('chat on')
        client.tell('flat-chat on')
        client.tell('menus on')
        client.tell('step join')
        client.tell(f'join {PLAYER}')
        client.sync([PLAYER])

        def cmd(step, text):
            client.tell('step ' + step)
            client.tell(f'send-cmd {PLAYER} {text}')
            client.sync([PLAYER])

        cmd('admit', 'gamemode survival Bob')
        cmd('admit', 'data modify storage recipes:proof player set from entity Bob')
        for step, text in STEPS:
            cmd(step, text)
            cmd(step + '-book', f'data modify storage recipes:proof {step.replace("-", "_")} set from entity Bob recipeBook')
        client.tell('step save')
        client.sync([PLAYER])
        start = len(server.lines)
        server.console('save-all flush')
        wait_new(server, start, 'Saved the game')
        client.end()
    finally:
        if client:
            client.kill()
        server.stop()
        log.close()
    storage = [p for p in (directory / 'world').rglob('*.dat')
               if 'command_storage' in p.name and ('recipes' in p.parts or 'recipes' in p.name)]
    assert len(storage) == 1, storage
    proof = json.loads(json.dumps(nbt.read_dat(storage[0])))['data']['contents']['proof']
    steps = {}
    for line in wire.read_text().splitlines():
        if not line.startswith(PLAYER + ' step='):
            continue
        prefix, rest = line.split(' step=', 1)
        step, body = rest.split(' ', 1)
        record = steps.setdefault(step, {'packets': [], 'chat': []})
        if body.startswith('menu-wire '):
            p = json.loads(body[len('menu-wire '):])
            if p.get('kind') in ('recipes', 'recipes-removed'):
                record['packets'].append(p)
        elif body.startswith('chat-flat '):
            record['chat'].append(json.loads(body[len('chat-flat '):]))
    return proof, steps


def facts(proof, steps):
    out = {}
    player = proof['player']
    for key in ('UUID', 'Health', 'playerGameType'):
        out[f'admit/player/{key}'] = player.get(key)
    out['admit/initial-book/recipes'] = sorted(player.get('recipeBook', {}).get('recipes', []))
    out['diag-join/packets'] = steps.get('join', {}).get('packets')
    for step, _ in STEPS:
        record = steps.get(step, {'packets': [], 'chat': []})
        # An action's packets: its own step and its book read after it (a
        # server may send the book's packets after the command's reply).
        later = steps.get(step + '-book', {'packets': []})['packets']
        packets = record['packets'] + later
        prefix = 'target-' if step in TARGET else ''
        out[f'{step}/chat'] = record['chat']
        adds = [p for p in packets if p['kind'] == 'recipes']
        removes = [p for p in packets if p['kind'] == 'recipes-removed']
        out[f'diag-{step}/after-reply'] = [p['kind'] for p in later]
        out[f'{prefix}{step}/add/count'] = [len(p['entries']) for p in adds]
        out[f'{prefix}{step}/add/replace'] = [p['replace'] for p in adds]
        out[f'{prefix}{step}/add/entries'] = [
            {k: v for k, v in e.items() if k != 'id'} for p in adds for e in p['entries']]
        out[f'{prefix}{step}/add/ids'] = [e['id'] for p in adds for e in p['entries']]
        out[f'{prefix}{step}/remove/ids'] = [i for p in removes for i in p['ids']]
        out[f'diag-{step}/raw'] = [p['wire'] for p in packets]
        book = proof.get(step.replace('-', '_'), {})
        # ServerRecipeBook keeps sets: their saved order is not the book's.
        out[f'{step}/book/recipes'] = sorted(book.get('recipes', []))
        out[f'{step}/book/toBeDisplayed'] = sorted(book.get('toBeDisplayed', []))
    # The ids the transmute grant added are the ids its removal takes back.
    give = out['target-give-transmute/add/ids']
    out['target-take-transmute/remove-matches-add'] = sorted(give) == sorted(out['target-take-transmute/remove/ids'])
    return out


def recipe_bytes(jar, resources):
    with zipfile.ZipFile(jar) as archive:
        java = archive.read('data/minecraft/recipe/map_cloning.json')
    native = (resources / 'data/minecraft/recipe/map_cloning.json').read_bytes()
    return {'java': hashlib.sha256(java).hexdigest(), 'native': hashlib.sha256(native).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--side', choices=['java', 'native', 'both'], default='both')
    parser.add_argument('--label')
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--reference', type=Path, help='a Java facts file to compare with')
    args = parser.parse_args()
    net.PLAYERS = [PLAYER]
    net.PROPERTIES['force-gamemode'] = 'false'
    lock, build, java, native, client = net.sides('recipe-book-variants-parity',
                                                 args.binary.resolve() if args.binary else None)
    import test_rust_parity as base
    _, jars, _ = base.verify_artifacts()
    admitted = recipe_bytes(jars['server'], net.lab.LOCAL / 'horizon-client/26.3')
    (build / 'recipe-bytes.json').write_text(json.dumps(admitted, indent=2) + '\n')
    assert admitted['java'] == admitted['native'], admitted
    world = build / 'generate/world'
    if not world.exists():
        world = generate(java, build)
    sides = ['java', 'native'] if args.side == 'both' else [args.side]
    reference = json.loads(args.reference.read_text()) if args.reference else None
    for side in sides:
        label = args.label or side
        proof, steps = run(label, java if side == 'java' else native, client, build, world)
        actual = facts(proof, steps)
        (build / f'{label}-facts.json').write_text(json.dumps(actual, indent=2, sort_keys=True) + '\n')
        if reference is None:
            reference = actual
            print(label, len(actual), 'facts captured')
            continue
        assert actual.keys() == reference.keys(), 'canonical keys changed'
        all_differences = [{'label': k, 'expected': reference[k], 'actual': actual[k]}
                           for k in reference if reference[k] != actual[k]]
        differences = [d for d in all_differences if not d['label'].startswith('diag-')]
        strict = [k for k in reference if not k.startswith('diag-')]
        report = {'records': len(strict), 'passed': len(strict) - len(differences), 'differences': differences,
                  'diagnostic_differences': [d['label'] for d in all_differences if d['label'].startswith('diag-')]}
        (build / f'{label}-report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(label, report['passed'], '/', report['records'])


if __name__ == '__main__':
    main()
