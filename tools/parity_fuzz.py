#!/usr/bin/env python3
"""Differential scenario fuzzer: generated command scripts run in vanilla and Horizon, compared exactly (#12498).

Each scenario comes from a seed: an area far from the player, forceloaded; a setup and an actor (summon with NBT,
a block entity with NBT, a dispenser, a drop, an explosion, a piston); time steps; optionally an unload and reload of
the area's chunk, which saves and loads it through each engine's chunk and entity storage; and reads (`/data get`).
Every scenario syncs both engines' randoms first (docs/parity-workflow.md, "Randoms that repeat"). A batch runs
through tools/test_command_exec_parity.py (`--groups-file`), each read is compared exactly (SNBT parsed, so a
difference names its field), failures are shrunk while the same field still differs, and grouped by family, subject
type and first differing field. docs/parity-workflow.md, "Fuzzing scenarios", says how to run it.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

try:
    from . import lab, test_command_exec_parity as harness
except ImportError:
    import lab
    import test_command_exec_parity as harness

ROOT = Path(__file__).resolve().parents[1]

# Far from the player (8.5 230 8.5) and its view distance, above any terrain; one chunk per scenario, three apart.
AREA_X, AREA_Z, FLOOR_Y = 4096, 4096, 300
RELOAD_TICKS = 60  # enough for both engines to save and drop a chunk whose forced ticket went (a guard checks it)

# Differences that are not parity bugs, by field path (a regular expression on the normalised path).
ALLOWED = [
    # CopperGolem.nextWeatheringTick is an absolute game time, and the two engines' worlds start the script at
    # different game times (they settle in a different number of ticks).
    (r'next_weather_age', 'absolute game time'),
]


def area(slot):
    return AREA_X + 48 * slot, FLOOR_Y, AREA_Z


# ---------------------------------------------------------------------------------------------------------------
# SNBT as /data get prints it (TextComponentTagVisitor): compounds become dicts, lists lists, scalars their text.

TOKEN = re.compile(r'\s*(?:("(?:[^"\\]|\\.)*"|\'(?:[^\'\\]|\\.)*\')|([{}\[\]:,;])|([^\s{}\[\]:,;"\']+))')


def parse_snbt(text):
    tokens = [m.group(1) or m.group(2) or m.group(3) for m in TOKEN.finditer(text)]
    pos = 0

    def value():
        nonlocal pos
        token = tokens[pos]
        pos += 1
        if token == '{':
            out = {}
            while tokens[pos] != '}':
                key = tokens[pos].strip('"\'')
                assert tokens[pos + 1] == ':', text
                pos += 2
                out[key] = value()
                if tokens[pos] == ',':
                    pos += 1
            pos += 1
            return out
        if token == '[':
            out = []
            if pos + 1 < len(tokens) and tokens[pos + 1] == ';':
                out.append(tokens[pos] + ';')
                pos += 2
            while tokens[pos] != ']':
                out.append(value())
                if tokens[pos] == ',':
                    pos += 1
            pos += 1
            return out
        return token

    result = value()
    assert pos == len(tokens), text
    return result


# Lists whose order vanilla doesn't fix, compared sorted by this key: AttributeMap keeps its instances in an
# Object2ObjectOpenHashMap keyed by Holder.Reference, which hashes by identity, so the saved order changes run to run.
UNORDERED = {'attributes': 'id'}


def diff(a, b, path=''):
    key = UNORDERED.get(path.rsplit('.', 1)[-1])
    if key and isinstance(a, list) and isinstance(b, list):
        a, b = (sorted(v, key=lambda e: str(e.get(key)) if isinstance(e, dict) else str(e)) for v in (a, b))
    if isinstance(a, dict) and isinstance(b, dict):
        return [d for key in sorted(set(a) | set(b))
                for d in (diff(a.get(key), b.get(key), f'{path}.{key}' if path else key))]
    if isinstance(a, list) and isinstance(b, list):
        out = [d for i, (x, y) in enumerate(zip(a, b)) for d in diff(x, y, f'{path}[{i}]')]
        return out + ([(f'{path}[#]', len(a), len(b))] if len(a) != len(b) else [])
    return [] if a == b else [(path, a, b)]


DATA = re.compile(r'^(.*) has the following (?:entity|block|storage) data: (.*)$', re.S)


def read_data(record):
    """A /data get record as (holder name, parsed data), or None when it reported no data."""
    text = ''.join(span[0] for message in record['messages'] for span in message)
    match = DATA.match(text)
    if not match:
        return None
    try:
        return match.group(1), parse_snbt(match.group(2))
    except (AssertionError, IndexError):
        return match.group(1), match.group(2)  # not SNBT this parser knows: compared as text


def normalise(path):
    return re.sub(r'\[\d+\]', '[]', path)


def allowed(path):
    return any(re.fullmatch(pattern, normalise(path)) for pattern, _ in ALLOWED)


def compare(vanilla, rust):
    """The differences between two records of one read, as [(field, vanilla, horizon)], allowlisted ones left out."""
    if vanilla['cmd'].startswith('/data get '):
        left, right = read_data(vanilla), read_data(rust)
        if left is None or right is None:
            if (left is None) != (right is None) or harness.compare(vanilla, rust):
                return [('<result>', harness.render(vanilla)[:300], harness.render(rust)[:300])]
            return []
        found = ([('<name>', left[0], right[0])] if left[0] != right[0] else []) + diff(left[1], right[1])
        return [d for d in found if not allowed(d[0])]
    kind = harness.compare(vanilla, rust)
    return [(f'<{kind}>', harness.render(vanilla)[:300], harness.render(rust)[:300])] if kind else []


# ---------------------------------------------------------------------------------------------------------------
# Scenarios: a seed, a family and steps. Offsets are relative to the scenario's area; NBT values are SNBT text.

def snbt(value):
    if isinstance(value, dict):
        return '{' + ','.join(f'{k}:{snbt(v)}' for k, v in value.items()) + '}'
    if isinstance(value, list):
        return '[' + ','.join(snbt(v) for v in value) + ']'
    return value


def uuid_words(slot, n):
    return [0x46555a5a, slot, n, 1]  # "FUZZ", then unique within a batch


def uuid_text(words):
    return '-'.join([f'{words[0] & 0xffffffff:08x}', f'{words[1] >> 16:04x}', f'{words[1] & 0xffff:04x}',
                     f'{words[2] >> 16:04x}', f'{words[2] & 0xffff:04x}{words[3]:08x}'])


def render(scenario, slot):
    """The scenario's harness operations in this slot, each command op tagged with its step index."""
    x0, y0, z0 = area(slot)

    def at(offset):
        return f'{x0 + offset[0]} {y0 + offset[1]} {z0 + offset[2]}'

    def cmd(text, step, record=True):
        op = {'op': 'cmd', 'cmd': text, 'step': step}
        if not record:
            op['record'] = False
        return op

    chunk = [[x0 >> 4, z0 >> 4]]
    seed = scenario['seed']
    ops = [cmd(f'/forceload add {x0} {z0}', -1, False), {'op': 'wait_chunks', 'chunks': chunk},
           cmd(f'/fill {x0} {y0 - 1} {z0} {x0 + 15} {y0 - 1} {z0 + 15} minecraft:stone', -1, False),
           # Entity ids: unique in a batch, the same modulo 4 in every slot (an item's resting tick depends on it).
           {'op': 'sync_randoms', 'uniquifier': seed, 'entity_id': 10_000_000 + 10_000 * slot,
            'level_seed': seed}]
    for i, step in enumerate(scenario['steps']):
        kind = step['k']
        if kind == 'summon':
            nbt = {'UUID': '[I;' + ','.join(map(str, uuid_words(slot, step['n']))) + ']', **step['nbt']}
            ops.append(cmd(f'/summon {step["type"]} {at(step["at"])} {snbt(nbt)}', i))
        elif kind == 'block':
            ops.append(cmd(f'/setblock {at(step["at"])} {step["block"]}{snbt(step.get("nbt") or {}) if step.get("nbt") else ""}', i))
        elif kind == 'cmd':
            ops.append(cmd(step['cmd'].format(*[at(o) for o in step.get('at', [])]), i))
        elif kind == 'tick':
            ops.append({'op': 'tick', 'n': step['n']})
        elif kind == 'dispense':
            ops.append({'op': 'crafter_tick', 'pos': [x0 + step['at'][0], y0 + step['at'][1], z0 + step['at'][2]]})
        elif kind == 'reload':
            ops += [cmd(f'/forceload remove {x0} {z0}', i, False), {'op': 'tick', 'n': RELOAD_TICKS},
                    {**cmd(f'/execute unless loaded {x0} {y0} {z0}', i), 'guard': True},
                    cmd(f'/forceload add {x0} {z0}', i, False), {'op': 'wait_chunks', 'chunks': chunk}]
        elif kind == 'read_entity':
            ops.append({**cmd(f'/data get entity {uuid_text(uuid_words(slot, step["n"]))}', i), 'read': True})
        elif kind == 'read_near':
            ox, oy, oz = (a + b for a, b in zip(area(slot), step['at']))
            ops.append({**cmd(f'/data get entity @e[type={step["type"]},x={ox}.5,y={oy},z={oz}.5,distance=..6,'
                              f'sort=nearest,limit=1]', i), 'read': True})
            ops.append({**cmd(f'/execute if entity @e[type={step["type"]},x={ox}.5,y={oy},z={oz}.5,distance=..12]', i),
                        'read': True})
        elif kind == 'read_block':
            ops.append({**cmd(f'/data get block {at(step["at"])}', i), 'read': True})
        else:
            raise ValueError(kind)
    ops += [cmd(f'/kill @e[type=!player,x={x0},y={y0 - 8},z={z0},dx=16,dy=48,dz=16]', -1, False),
            cmd(f'/fill {x0} {y0 - 1} {z0} {x0 + 15} {y0 + 12} {z0 + 15} minecraft:air', -1, False),
            cmd(f'/forceload remove {x0} {z0}', -1, False)]
    return ops


def subject(scenario, step):
    """The entity or block type a read looks at."""
    if step['k'] == 'read_entity':
        return next((s['type'] for s in scenario['steps'] if s['k'] == 'summon' and s['n'] == step['n']), '?')
    if step['k'] == 'read_near':
        return step['type']
    if step['k'] == 'read_block':
        placed = [s['block'] for s in scenario['steps'] if s['k'] == 'block' and s['at'] == step['at']]
        return re.sub(r'[\[{].*', '', placed[-1]) if placed else '?'
    return '?'


# ---------------------------------------------------------------------------------------------------------------
# Running batches through the command-exec harness.

class Runner:
    def __init__(self, work, binary, size):
        self.work, self.binary, self.size, self.batches = work, binary, size, 0

    def run(self, scenarios):
        """Per scenario: {'failures': [(signature, read, differences)], 'invalid': reason} or {'crash': log}."""
        if len(scenarios) > self.size:
            return [r for start in range(0, len(scenarios), self.size) for r in self.run(scenarios[start:start + self.size])]
        if not scenarios:
            return []
        self.batches += 1
        batch = self.work / f'batch-{self.batches:03d}'
        batch.mkdir(parents=True, exist_ok=True)
        rendered = [render(s, slot) for slot, s in enumerate(scenarios)]
        groups = [[f'fuzz {s["family"]} {s["seed"]}', [{k: v for k, v in op.items() if k not in ('step', 'read', 'guard')}
                                                       for op in ops]] for s, ops in zip(scenarios, rendered)]
        (batch / 'groups.json').write_text(json.dumps(groups), encoding='utf-8')
        print(f'batch {self.batches}: {len(scenarios)} scenarios', flush=True)
        command = [sys.executable, str(ROOT / 'tools/test_command_exec_parity.py'), '--allow-gaps',
                   '--groups-file', str(batch / 'groups.json'), '--work-dir', str(batch / 'run')]
        if self.binary:  # otherwise the harness builds the game (the first batch; later ones find it built)
            command += ['--binary', str(self.binary)]
        result = subprocess.run(command, cwd=ROOT, capture_output=True, encoding='utf-8', errors='replace',
                                # The game's script deadline (600 s by default) is for the fixed corpus.
                                env={**os.environ, 'HORIZON_HARNESS_TIMEOUT': str(30 * len(scenarios) + 600)})
        (batch / 'harness.log').write_text(result.stdout + result.stderr, encoding='utf-8')
        if result.returncode:
            if len(scenarios) == 1:
                return [{'crash': (result.stdout + result.stderr)[-2000:]}]
            half = len(scenarios) // 2  # find the scenario that stops the run
            return self.run(scenarios[:half]) + self.run(scenarios[half:])
        records = [[json.loads(line) for line in (batch / 'run' / f'{side}.jsonl').open(encoding='utf-8')]
                   for side in ('vanilla', 'rust')]
        for world in ('vanilla', 'rust', 'classes'):  # tens of MB a batch; the records and logs stay
            shutil.rmtree(batch / 'run' / world, ignore_errors=True)
        out, position = [], 0
        for scenario, ops in zip(scenarios, rendered):
            entry = {'failures': [], 'invalid': None, 'reads': []}
            for op in ops:
                if op['op'] != 'cmd' or op.get('record') is False:
                    continue
                vanilla, rust = records[0][position], records[1][position]
                position += 1
                step = scenario['steps'][op['step']]
                if op.get('guard'):
                    if vanilla['results'] != [[True, 1]]:
                        entry['invalid'] = f'vanilla kept the chunk loaded: {vanilla["results"]}'
                    if rust['results'] != vanilla['results']:
                        entry['failures'].append((f'{scenario["family"]}|chunk|<unload>', op['cmd'],
                                                  [('<unload>', vanilla['results'], rust['results'])]))
                    continue
                if op.get('read'):
                    entry['reads'].append((op['cmd'], vanilla, rust))
                differences = compare(vanilla, rust)
                if differences:
                    what = subject(scenario, step) if op.get('read') else op['cmd'].split()[0]
                    reloaded = any(s['k'] == 'reload' for s in scenario['steps'][:op['step']])
                    first = normalise(differences[0][0])
                    entry['failures'].append((f'{scenario["family"]}|{what}|{"reloaded" if reloaded else "live"}|{first}',
                                              op['cmd'], differences))
            out.append(entry)
        assert position == len(records[0]) == len(records[1]), (position, len(records[0]), len(records[1]))
        return out


def signatures(result):
    """A scenario's divergences: each failing read's signature once, or 'crash'. One read can't hide another's."""
    if 'crash' in result:
        return ['crash']
    return [] if result['invalid'] else list(dict.fromkeys(failure[0] for failure in result['failures']))


# ---------------------------------------------------------------------------------------------------------------
# Shrinking: drop steps, NBT keys and ticks while the same divergence stays.

def reduce(scenario, edits):
    """The scenario with these edits: ('drop', i) a step, ('half', i) a tick count, ('nbt', i, key) an NBT key."""
    steps = []
    for i, step in enumerate(scenario['steps']):
        if ('drop', i) in edits:
            continue
        if ('half', i) in edits:
            step = {**step, 'n': step['n'] // 2}
        gone = {e[2] for e in edits if e[:2] == ('nbt', i)}
        if gone:
            step = {**step, 'nbt': {k: v for k, v in step['nbt'].items() if k not in gone}}
        steps.append(step)
    return {**scenario, 'steps': steps}


def edits(scenario):
    for i, step in enumerate(scenario['steps']):
        if step['k'] == 'tick' and step['n'] > 1:
            yield ('half', i)
        yield ('drop', i)
        yield from (('nbt', i, key) for key in step.get('nbt') or {})


def runnable(scenario):
    """A candidate still reads something, and activates a dispenser only where one was placed."""
    steps = scenario['steps']
    placed = {tuple(s['at']) for s in steps if s['k'] == 'block' and s['block'].startswith('dispenser')}
    return (any(s['k'].startswith('read') for s in steps)
            and all(tuple(s['at']) in placed for s in steps if s['k'] == 'dispense'))


def shrink(runner, failing, rounds):
    """{signature: (scenario, result)} -> the same, each scenario as small as this many rounds could make it.

    A round runs every single edit of each scenario, then each scenario with all its edits that kept its signature
    applied together; if that loses the signature, the smallest single edit that kept it stands.
    """
    current = dict(failing)
    for _ in range(rounds):
        tried = [(sig, edit, reduce(s, {edit})) for sig, (s, _) in current.items() for edit in edits(s)]
        tried = [t for t in tried if runnable(t[2])]
        kept, best = {}, {}
        for (sig, edit, candidate), result in zip(tried, runner.run([t[2] for t in tried])):
            if sig in signatures(result):
                kept.setdefault(sig, set()).add(edit)
                if sig not in best or len(json.dumps(candidate)) < len(json.dumps(best[sig][0])):
                    best[sig] = (candidate, result)
        if not kept:
            break
        combined = [(sig, reduce(current[sig][0], e)) for sig, e in kept.items() if len(e) > 1]
        for (sig, candidate), result in zip(combined, runner.run([c for _, c in combined])):
            if sig in signatures(result):
                best[sig] = (candidate, result)
        current.update(best)
    return current


# ---------------------------------------------------------------------------------------------------------------
# Command line

# Fixed scenarios that match today: a chest with items and a bare armor stand, read after an unload and reload.
SMOKE = [{'family': 'smoke', 'seed': 1, 'steps': [
    {'k': 'block', 'block': 'chest', 'at': [7, 0, 7], 'nbt': {'Items': '[{Slot:0b,id:"minecraft:stone",count:3}]'}},
    {'k': 'summon', 'type': 'armor_stand', 'at': [3.5, 0, 3.5], 'n': 0, 'nbt': {}},
    {'k': 'reload'}, {'k': 'read_block', 'at': [7, 0, 7]}, {'k': 'read_entity', 'n': 0}]}]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--replay', type=Path, help='run the scenarios of this JSON file (a list of scenarios)')
    source.add_argument('--smoke', action='store_true', help='fixed scenarios that must match: the tool works')
    parser.add_argument('--batch', type=int, default=40, help='scenarios per harness run')
    parser.add_argument('--shrink-rounds', type=int, default=6)
    parser.add_argument('--binary', type=Path, help='an already built Horizon game')
    parser.add_argument('--work-dir', type=Path, default=lab.LOCAL / 'parity-fuzz')
    args = parser.parse_args()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    runner = Runner(work, args.binary and args.binary.resolve(strict=True), args.batch)
    scenarios = SMOKE if args.smoke else json.loads(args.replay.read_text(encoding='utf-8'))
    results = runner.run(scenarios)
    failing = [(s, r) for s, r in zip(scenarios, results) if signatures(r)]
    invalid = [(s['seed'], r['invalid']) for s, r in zip(scenarios, results) if r.get('invalid')]
    print(f'{len(scenarios)} scenarios: {len(failing)} diverge, {len(invalid)} invalid', flush=True)
    if args.smoke:
        empty = [cmd for r in results for cmd, vanilla, _ in r.get('reads', []) if vanilla['results'][:1] != [[True, 1]]]
        if failing or invalid or empty:
            sys.exit(f'smoke scenarios diverge: {[s["seed"] for s, _ in failing]} invalid: {invalid} '
                     f'empty reads: {empty}; see {work}')
        return
    groups = {}
    for scenario, result in failing:
        for sig in signatures(result):
            groups.setdefault(sig, []).append((scenario, result))
    # One bug, one shrink: each group's smallest scenario stands for it.
    shrunk = shrink(runner, {sig: min(members, key=lambda m: len(json.dumps(m[0]))) for sig, members in groups.items()},
                    args.shrink_rounds)
    report = {'scenarios': len(scenarios), 'invalid': invalid, 'diverging': len(failing), 'groups': []}
    for sig, members in sorted(groups.items(), key=lambda g: -len(g[1])):
        scenario, result = shrunk[sig]
        report['groups'].append({
            'signature': sig, 'seeds': [s['seed'] for s, _ in members], 'scenario': scenario,
            'commands': [op['cmd'] if op['op'] == 'cmd' else json.dumps({k: v for k, v in op.items() if k != 'step'})
                         for op in render(scenario, 0)],
            'differences': result.get('crash') or [[cmd, d[:8]] for s, cmd, d in result['failures'] if s == sig][:3]})
    lab.write_json(work / 'report.json', report)
    print(f'{len(failing)} diverging scenarios in {len(groups)} groups ({runner.batches} harness runs):')
    for group in report['groups']:
        print(f'  {len(group["seeds"]):3d}  {group["signature"]}')
    print(f'report: {work / "report.json"}')


if __name__ == '__main__':
    main()
