#!/usr/bin/env python3
"""Run the Logical Redstone Archive's contraptions in vanilla and in Horizon, tick by tick.

The catalogue (`tools/lra-catalog/`, by maybeachromium, WTFPL) lists each archive entry's input and output
positions and what it was checked against. The archive's schematics have no licence and are never committed:
they are downloaded into `.local/lra-catalog/` and checked against the digest pinned here.

Each contraption is built from its schematic in `VanillaRedstoneCatalogOracle` (a real flat-world server)
and in `redstone_catalog_fixture` (horizon-world). Switches start off (except those the catalogue held on),
every block gets a neighbour update, and the level runs until quiet. Then each step sets the input levers
and presses buttons as a player's click would, and runs whole level ticks until quiet. Both sides print the
outputs that changed after every tick, the tick the step ended on, and a hash of every block in the box.

`verified` entries must match; `partial` and `unverified` ones are reported (expected unknown). Inputs the
fixture can't drive yet (analog levels, lecterns, bare wire) are listed as unsupported.
"""
import argparse
import hashlib
import json
import os
import random
import re
import subprocess
import time
import urllib.parse
import urllib.request
from pathlib import Path
try:
    from . import lab, lra_schem, parity_shard, test_rust_parity as base
except ImportError:
    import lab
    import lra_schem
    import parity_shard
    import test_rust_parity as base

ARCHIVE = 'mattbatwings/Logical-Redstone-Archive'
COMMIT = 'da4dd515559458140f78a555600e7e4607ac6b8c'
CATALOG = lab.ROOT / 'tools/lra-catalog/entries.jsonl'
# sha256 of the sorted "<path> <sha256>" lines of every entry's schematic at COMMIT.
SCHEMATICS_SHA256 = 'a02f086c05b4280605db0c992867632c4c5f0f91fc4648a4933a4850f70ed19c'
DATA = lab.LOCAL / 'lra-catalog'
DRIVEN = {'lever', 'stone_button', 'oak_button', 'birch_button'}
# tools/parity.py runs NAME#1..#3 in parallel (PARITY_SHARD=i/3), each with every third contraption.
PARITY_SHARDS = 3


def run(command, full, **kwargs):
    """test_rust_parity.run, without its 15-minute hang guard for --full."""
    if not full:
        return base.run(command, **kwargs)
    subprocess.run(command, cwd=lab.ROOT, check=True, **kwargs)


def load_catalog():
    return [json.loads(line) for line in CATALOG.read_text(encoding='utf-8').splitlines() if line]


def fetch_schematics(entries):
    """Each entry's schematic at COMMIT, under DATA/schematics; all of them checked against SCHEMATICS_SHA256."""
    root = DATA / 'schematics'
    sums = {}
    for entry in entries:
        rel = entry.get('schematic')
        if not rel or entry['status'] == 'no_schematic':
            continue
        if '..' in Path(rel).parts or rel.startswith('/'):
            raise ValueError(f'bad schematic path {rel}')
        path = root / rel
        if not path.exists():
            url = f'https://raw.githubusercontent.com/{ARCHIVE}/{COMMIT}/' + urllib.parse.quote(rel)
            path.parent.mkdir(parents=True, exist_ok=True)
            request = urllib.request.Request(url, headers={'User-Agent': 'horizon/0.1'})
            with urllib.request.urlopen(request, timeout=120) as response:
                data = response.read()
            # The shards download into the same folder at once: each writes its own part file.
            part = path.with_name(f'{path.name}.{os.getpid()}.part')
            part.write_bytes(data)
            part.replace(path)
        sums[rel] = lab.digest(path, 'sha256')
    listing = ''.join(f'{rel} {sums[rel]}\n' for rel in sorted(sums))
    combined = hashlib.sha256(listing.encode()).hexdigest()
    if combined != SCHEMATICS_SHA256:
        (DATA / 'schematics.sha256').write_text(listing)
        raise SystemExit(f'schematics digest {combined} is not the pinned {SCHEMATICS_SHA256}: see {DATA / "schematics.sha256"}')
    return root


class States:
    """Schematic state strings -> vanilla state ids (`facts.json`), missing properties at their defaults."""
    def __init__(self, resources):
        facts = json.loads((resources / 'facts.json').read_text())
        self.ids, self.props = {}, []
        for index, state in enumerate(facts['states']):
            name, props = lra_schem.parse_state(state[3].replace('Block{', '').replace('}', '', 1))
            self.ids[(name, tuple(sorted(props.items())))] = index
            self.props.append((name, props))
        self.defaults = facts['defaults']

    def id(self, text, **override):
        name, props = lra_schem.parse_state(text)
        full = dict(self.props[self.defaults[name]][1])
        full.update({k: v for k, v in props.items() if k in full})
        full.update(override)
        return self.ids[(name, tuple(sorted(full.items())))]


def held_on(entry):
    """Switches the catalogue's runs held on (`other_switches`), as positions; 'as saved' keeps saved ones."""
    text = ' '.join(str(v.get('other_switches', 'off')) for v in entry['verified'] or [])
    held = {tuple(map(int, m)) for m in re.findall(r'\[(\d+), (\d+), (\d+)', text)}
    saved = re.search(r'at x=(\d+), y=(\d+) on \(as saved\)', text)
    return held, (tuple(map(int, saved.groups())) if saved else None)


def steps(entry, bits, full):
    """Input vectors: every one when few enough (or random ones), random changes for stateful builds."""
    rng = random.Random(entry['code'])
    groups = [[tuple(p) for p in g['positions']] for g in entry['inputs']]
    sequence = any('sequence' in v['method'] for v in entry['verified'] or [])
    if sequence:
        out = []
        for _ in range(512 if full else 24):
            group = rng.choice(groups)
            out.append([[*p, rng.randrange(2)] for p in group])
        return out
    exhaustive = len(bits) <= (16 if full else 8)
    vectors = range(1 << len(bits)) if exhaustive else [0, (1 << len(bits)) - 1] + [rng.getrandbits(len(bits)) for _ in range(1024 if full else 24)]
    return [[[*p, (v >> i) & 1] for i, p in enumerate(bits)] for v in vectors]


def build(entry, schematics, states, full):
    """The case both sides run, or the reason it can't be run."""
    kinds = {b for g in entry['inputs'] for b in g['blocks']}
    if not kinds:
        return None, 'no inputs mapped'
    if not kinds <= DRIVEN or any(g['encoding'] != 'binary' for g in entry['inputs']):
        return None, 'unsupported inputs: ' + ','.join(sorted(kinds - DRIVEN) or ['analog levels'])
    schem = lra_schem.read(schematics / entry['schematic'])
    held, saved_row = held_on(entry)
    cells, comparators, containers, lecterns = [], [], [], []
    for x, y, z, text in schem['cells']:
        name, props = lra_schem.parse_state(text)
        override = {}
        if name == 'minecraft:lever' or name.endswith('_button'):
            keep = name == 'minecraft:lever' and saved_row and (x, y) == saved_row and props.get('powered') == 'true'
            override['powered'] = 'true' if (x, y, z) in held or keep else 'false'
        cells.append([x, y, z, states.id(text, **override)])
    for be in schem['block_entities']:
        x, y, z = be['pos']
        data = be['data']
        if 'OutputSignal' in data:
            comparators.append([x, y, z, data['OutputSignal']])
        if isinstance(data.get('Items'), list):
            items = [[i.get('Slot', 0), i.get('id', 'minecraft:air'), i.get('count', i.get('Count', 1))] for i in data['Items']]
            containers.append([x, y, z, items])
        if isinstance(data.get('Book'), dict):
            book = data['Book']
            pages = book.get('components', {}).get('minecraft:written_book_content', book.get('components', {}).get('minecraft:writable_book_content', {})).get('pages') or book.get('tag', {}).get('pages') or []
            lecterns.append([x, y, z, len(pages), data.get('Page', 0)])
    bits = [tuple(p) for g in entry['inputs'] for p in g['positions']]
    outputs = [p for g in entry['outputs'] for p in g['positions']]
    return {'code': entry['code'], 'size': schem['size'], 'cells': cells, 'comparators': comparators, 'containers': containers,
            'lecterns': lecterns, 'outputs': outputs, 'settle': 600, 'max': 300, 'budget': 60000 if full else 2500,
            'steps': steps(entry, bits, full)}, None


def fixed_cases(states):
    """Contraptions of our own, outside the archive, that must match.

    A repeater or comparator on a downward piston that pushes (or pulls) a target block, powered through a stone
    block by a lever: while the piston retracts, the base below the diode is a moving piston whose block entity
    holds the extended base, and `DiodeBlock.canSurviveOn` finds its top face rigid there (#12019).
    """
    for piston in ('piston', 'sticky_piston'):
        for diode in ('repeater', 'comparator'):
            cells = [[1, 1, 1, states.id(f'{piston}[facing=down]')], [1, 2, 1, states.id(f'{diode}[facing=south]')],
                     [1, 0, 1, states.id('target')], [2, 1, 1, states.id('stone')],
                     [3, 1, 1, states.id('lever[face=wall,facing=east]')]]
            # A long pulse, a 1-tick-off pulse and a 1-tick-on pulse (two flips in one step act in the same tick).
            lever = [3, 1, 1]
            steps = [[[*lever, 1]], [[*lever, 0]], [[*lever, 1]], [[*lever, 0], [*lever, 1]], [[*lever, 0]],
                     [[*lever, 1], [*lever, 0]]]
            yield {'code': f'horizon-{diode}-on-{piston}', 'size': [4, 3, 3], 'cells': cells, 'comparators': [],
                   'containers': [], 'lecterns': [], 'outputs': [[1, 1, 1], [1, 2, 1], [3, 1, 1]], 'settle': 600,
                   'max': 300, 'budget': 2500, 'steps': steps}


def split(text):
    """Output text -> {code: (lines compared, analog lines, other lines)}."""
    out, code = {}, None
    for line in text.splitlines():
        if line.startswith('case '):
            code = line[5:]
            out[code] = ([], [], [])
        elif line.startswith('# '):
            continue
        elif line.startswith('analog '):
            out[code][1].append(line[7:])
        elif line.startswith(('drift ', 'error ', 'time ')):
            out[code][2].append(line)
        else:
            out[code][0].append(line)
    return out


def first_difference(a, b):
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return i
    return min(len(a), len(b))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--full', action='store_true', help='every entry, larger sweeps (exhaustive up to 16 bits)')
    parser.add_argument('--only', help='comma separated codes')
    args = parser.parse_args()
    shard, shards = parity_shard.parse(os.environ.get('PARITY_SHARD', '1/1'))
    lock, jars, libraries = base.verify_artifacts()
    resources = lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    work = lab.LOCAL / 'redstone-catalog-parity' / (f'shard-{shard}' if shards > 1 else 'all')
    work.mkdir(parents=True, exist_ok=True)
    entries = load_catalog()
    schematics = fetch_schematics(entries)
    entries = [e for e in entries if not args.only or e['code'] in args.only.split(',')][shard - 1::shards]
    status = {}
    states = States(resources)
    cases, skipped = [], {}
    if shard == 1:
        for case in fixed_cases(states):
            if not args.only or case['code'] in args.only.split(','):
                cases.append(case)
                status[case['code']] = 'verified'
    for entry in entries:
        if entry['status'] == 'no_schematic':
            continue
        case, why = build(entry, schematics, states, args.full)
        if case:
            cases.append(case)
        else:
            skipped[entry['code']] = why
    (work / 'vanilla-cases.jsonl').write_text(''.join(json.dumps(c, separators=(',', ':')) + '\n' for c in cases))
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    deps = os.pathsep.join(map(str, [jars['server'], *libraries]))
    java_root = lab.ROOT / 'mods/horizon/src/test/java'
    oracle = java_root / 'dev/horizonlab/parity/VanillaRedstoneCatalogOracle.java'
    started = time.time()
    run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none', '-nowarn', '-sourcepath', str(java_root),
              '-cp', deps, '-d', str(classes), str(oracle)], args.full)
    run(['java', '-Xmx4G', '-cp', str(classes) + os.pathsep + deps, 'dev.horizonlab.parity.VanillaRedstoneCatalogOracle',
              str(work / 'vanilla-cases.jsonl'), str(work / 'vanilla.txt')], args.full, stdout=subprocess.DEVNULL)
    vanilla = split((work / 'vanilla.txt').read_text())
    for case in cases:
        notes = vanilla.get(case['code'], ([], [], []))[2]
        case['time'] = next((int(n[5:]) for n in notes if n.startswith('time ')), 0)
        case['analog'] = [[*map(int, a.split(' ')[0].split(',')), int(a.split(' ')[1])] for a in vanilla.get(case['code'], ([], [], []))[1]]
    (work / 'rust-cases.jsonl').write_text(''.join(json.dumps(c, separators=(',', ':')) + '\n' for c in cases))
    run(['cargo', 'build', '--release', '-p', 'horizon-world', '--example', 'world_probe'], args.full)
    probe = lab.cargo_target() / 'release/examples' / ('world_probe.exe' if os.name == 'nt' else 'world_probe')
    run([str(probe), 'redstone_catalog_fixture', str(resources), str(work / 'rust-cases.jsonl'), str(work / 'rust.txt')], args.full)
    rust = split((work / 'rust.txt').read_text())
    status.update({e['code']: e['status'] for e in entries})
    report, failures = {}, []
    for case in cases:
        code = case['code']
        a, _, notes = vanilla.get(code, ([], [], ['error missing']))
        b = rust.get(code, ([], [], []))[0]
        notes = [n for n in notes if not n.startswith('time ')]
        if any(n.startswith('error timeout') for n in notes):
            result = {'result': 'oracle timeout', 'notes': notes, 'why': 'vanilla took over 4 minutes: not compared'}
        elif any(n.startswith('error') for n in notes):
            result = {'result': 'oracle error', 'notes': notes}
        elif any(n.startswith('drift') for n in notes):
            result = {'result': 'not comparable', 'notes': notes[:5], 'why': 'vanilla moved items: container levels changed'}
        elif a == b:
            result = {'result': 'match', 'steps': sum(line.startswith('step') for line in a)}
        else:
            i = first_difference(a, b)
            step = next((line for line in reversed(a[:i + 1]) if line.startswith('step')), '?')
            result = {'result': 'mismatch', 'at': step, 'vanilla': a[i:i + 4], 'horizon': b[i:i + 4]}
        result['status'] = status[code]
        report[code] = result
        if status[code] == 'verified' and result['result'] in ('mismatch', 'oracle error'):
            failures.append(code)
    for code, why in skipped.items():
        report[code] = {'result': 'skipped', 'why': why, 'status': status[code]}
    lab.write_json(work / 'report.json', report)
    counts = {}
    for r in report.values():
        counts.setdefault((r['status'], r['result']), 0)
        counts[(r['status'], r['result'])] += 1
    for (st, res), n in sorted(counts.items()):
        print(f'{st:>10} {res}: {n}')
    for code in sorted(report):
        r = report[code]
        if r['result'] == 'mismatch':
            print(f'{code} ({r["status"]}): first difference in {r["at"]}\n  vanilla {r["vanilla"]}\n  horizon {r["horizon"]}')
    print(f'{len(cases)} contraptions run in {time.time() - started:.0f} s; report: {work / "report.json"}')
    raise SystemExit(1 if failures else 0)


if __name__ == '__main__':
    main()
