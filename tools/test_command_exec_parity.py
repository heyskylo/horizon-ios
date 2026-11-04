#!/usr/bin/env python3
"""Compare Horizon's command execution with vanilla's real command implementations.

Java (VanillaCommandExecOracle) starts a stand-in integrated server: a real
MinecraftServer with a generated world, Commands.CommandSelection.INTEGRATED
and one single-player owner named Player. Rust runs the same script through
Horizon's integrated server with a connected headless client (HORIZON_COMMAND_SCRIPT). Both run each command as
the player through Commands.performPrefixedCommand and record every system or
chat message the player receives (text and resolved style: colour, bold,
italic, underlined, strikethrough, obfuscated) together with the results the
command reported to the player's CommandResultCallback. World, entity and item
changes are observed through probe commands whose feedback reports them
(execute if, clear with a zero count, data get, scoreboard queries...).
Messages that depend on time or a random UUID are compared after masking.
Direct falling-block probes also compare the physical entities in each level,
independently of the command selector's support for those entity types.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import zipfile

try:
    from . import lab, test_rust_parity as base
    from .integration_env import run_game
    from . import command_exec_corpus as corpus_module
except ImportError:
    import lab
    from integration_env import run_game
    import test_rust_parity as base
    import command_exec_corpus as corpus_module

SEED = 1


def install_datapack_fixtures(world, predicates, functions, jar, advancements=None):
    if not predicates and not functions and not advancements:
        return
    with zipfile.ZipFile(jar) as archive:
        version = json.loads(archive.read('version.json'))['pack_version']
    fmt = [version['data_major'], version['data_minor']]
    pack = world / 'datapacks' / 'command-selector-fixture'
    pack.mkdir(parents=True, exist_ok=True)
    (pack / 'pack.mcmeta').write_text(json.dumps({'pack': {
        'description': 'Command execution fixtures', 'pack_format': fmt[0],
        'min_format': fmt, 'max_format': fmt}}), encoding='utf-8')
    for identifier, predicate in predicates.items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'predicate' / (path + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(predicate), encoding='utf-8')
    for identifier, contents in functions.items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'function' / (path + '.mcfunction')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(contents, encoding='utf-8')

    for identifier, advancement in (advancements or {}).items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'advancement' / (path + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(advancement), encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--pot-string-sherd-diagnostic', action='store_true',
                        help='use string sherd input in the pot data group (known separate codec gap)')
    parser.add_argument('--limit', type=int, default=40, help='print at most this many mismatches')
    parser.add_argument('--allow-gaps', action='store_true', help='report mismatches without failing')
    parser.add_argument('--only', help='run only corpus groups whose name contains this')
    parser.add_argument('--array-storage-diagnostic', action='store_true',
                        help='run all numeric-array edit cases, including the known wildcard/string storage side effects')
    parser.add_argument('--sulfur-power-load-diagnostic', action='store_true',
                        help='retain the immediate-load powered sulfur cube H249 diagnostic')
    parser.add_argument('--binary', type=Path, help='use an already built Horizon game instead of building it')
    parser.add_argument('--slot-codec-diagnostic', action='store_true',
                        help='retain the separate invalid inline slot-range codec diagnostic')
    args = parser.parse_args()
    build = lab.LOCAL / 'command-exec-parity'
    build.mkdir(parents=True, exist_ok=True)
    report_path = build / 'report.json'
    report_path.unlink(missing_ok=True)
    if args.prepare:
        lab.prepare()
    lock, jars, libraries = base.verify_artifacts()
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCommandExecOracle.java'
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependency_path = os.pathsep.join(map(str, libraries))

    def run(command, name, **kw):
        with (build / (name + '.log')).open('w', encoding='utf-8') as log:
            runner = run_game if kw.pop('game', False) else subprocess.run
            result = runner(list(map(str, command)), stdout=log, stderr=subprocess.STDOUT, **kw)
        if result.returncode:
            tail = (build / (name + '.log')).read_text(errors='replace')[-3000:]
            sys.exit(f'{name} failed ({result.returncode}):\n{tail}')

    groups = corpus_module.groups()
    if args.pot_string_sherd_diagnostic:
        groups = [(name, corpus_module.decorated_pot_data(string_sherd=True)
                   if name == 'decorated pot data' else ops) for name, ops in groups]
    if args.array_storage_diagnostic:
        groups = [('setup', corpus_module.group_setup()),
                  ('numeric array edits with storage side effects',
                   corpus_module.numeric_array_edits(include_storage_side_effects=True))]
    if args.sulfur_power_load_diagnostic:
        groups = [('setup', corpus_module.group_setup()),
                  ('sulfur powered-load diagnostic', corpus_module.sulfur_cube_fuse_ticks(True))]
    if args.only:
        groups = [g for g in groups if args.only in g[0] or g[0] == 'setup']
    if args.slot_codec_diagnostic:
        groups.append(('slot source codec diagnostic', [corpus_module.cmd(
            '/item fill block 4 200 4 {type:"minecraft:slot_range",slots:"container.99"} with diamond')]))
    script = []
    for _, lines in groups:
        script.extend(lines)
    script_path = build / 'script.jsonl'
    script_path.write_text(''.join(json.dumps({k: v for k, v in op.items() if k != 'expect_result'},
                                            ensure_ascii=False) + '\n' for op in script), encoding='utf-8')
    predicates = corpus_module.predicate_selector_fixtures([name for name, _ in groups])
    functions = corpus_module.function_return_fixtures([name for name, _ in groups])
    advancements = corpus_module.advancement_reward_fixtures([name for name, _ in groups])

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', str(jars['server']) + os.pathsep + dependency_path, '-d', classes, oracle], 'compile-java')
    vanilla_dir = build / 'vanilla'
    shutil.rmtree(vanilla_dir, ignore_errors=True)
    vanilla_dir.mkdir()
    install_datapack_fixtures(vanilla_dir / 'universe/harness', predicates, functions, jars['server'], advancements)
    run(['java', '-Xmx3G', '-Duser.timezone=UTC', '-cp', os.pathsep.join([str(classes), str(jars['server']), dependency_path]),
         'dev.horizonlab.parity.VanillaCommandExecOracle', vanilla_dir / 'universe', script_path, build / 'vanilla.jsonl', SEED], 'java', cwd=vanilla_dir)

    if args.binary:
        binary = args.binary.resolve(strict=True)
    else:
        run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'], 'compile-rust', cwd=lab.ROOT)
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True))
        binary = Path(metadata['target_directory']) / 'fast' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    rust_dir = build / 'rust'
    shutil.rmtree(rust_dir, ignore_errors=True)
    rust_dir.mkdir()
    install_datapack_fixtures(rust_dir / 'saves/harness', predicates, functions, jars['server'], advancements)
    # The oracle's Services resolve no profile (/fetchprofile fails on the next tick): the game gets no services
    # either, a discovery address that refuses at once, rather than Mojang's over the network.
    env = lab.game_env(HORIZON_PLAYER_NAME="Player", HORIZON_COMMAND_SCRIPT=script_path,
                       HORIZON_COMMAND_OUTPUT=build / 'rust.jsonl', HORIZON_API_DISCOVERY='http://127.0.0.1:9/')
    run([binary, '--resources', lab.RESOURCES, '--world', 'harness', '--seed', SEED, '--radius', 4], 'rust', game=True, cwd=rust_dir, env=env)

    vanilla = [json.loads(l) for l in (build / 'vanilla.jsonl').open(encoding='utf-8')]
    rust = [json.loads(l) for l in (build / 'rust.jsonl').open(encoding='utf-8')]
    if len(vanilla) != len(rust):
        sys.exit(f'record count differs: vanilla {len(vanilla)} rust {len(rust)}')
    recorded_ops = [op for op in script if op['op'] in ('falling', 'destroy_progress', 'weather_state') or
                    (op['op'] == 'cmd' and op.get('record', True)) or
                    (op['op'] == 'tick' and op.get('record'))]
    if len(recorded_ops) != len(vanilla):
        sys.exit(f'script record count differs: expected {len(recorded_ops)}, got {len(vanilla)}')
    for op, reference in zip(recorded_ops, vanilla):
        if 'expect_result' in op and reference.get('results') != [[True, op['expect_result']]]:
            sys.exit(f'Java fixture control failed: {op["cmd"]}: {reference.get("results")}')
        if 'expect_state' in op and reference.get('results') != [[True, v] for v in op['expect_state']]:
            sys.exit(f'Java weather guard failed: {op["label"]}: {reference.get("results")}')
    commands_by_group = []
    index = 0
    for name, lines in groups:
        n = sum(1 for op in lines if op['op'] in ('falling', 'destroy_progress', 'weather_state') or (op['op'] == 'cmd' and op.get('record', True)) or (op['op'] == 'tick' and op.get('record')))
        commands_by_group.append((name, index, index + n))
        index += n
    mismatches = []
    failed_positions = set()
    for position, (v, r) in enumerate(zip(vanilla, rust)):
        assert v['cmd'] == r['cmd'], (v['cmd'], r['cmd'])
        kind = compare(v, r, recorded_ops[position].get('comparison'))
        if kind:
            failed_positions.add(position)
            mismatches.append({'kind': kind, 'cmd': v['cmd'], 'vanilla': v, 'rust': r})
    (build / 'mismatches.jsonl').write_text(''.join(json.dumps(m, ensure_ascii=False) + '\n' for m in mismatches), encoding='utf-8')
    summary = []
    for name, lo, hi in commands_by_group:
        # By position: two groups that run the same probe command (the orb
        # groups' `data get` lines) must not be charged each other's failures.
        failed = sum(1 for position in failed_positions if lo <= position < hi)
        summary.append({'group': name, 'commands': hi - lo, 'mismatches': failed})
        print(f'{name}: {hi - lo - failed}/{hi - lo} commands match', flush=True)
    for m in mismatches[:args.limit]:
        print(m['kind'], repr(m['cmd']))
        print('  vanilla', render(m['vanilla'])[:400])
        print('  rust   ', render(m['rust'])[:400])
    total = len(vanilla)
    print(f'total: {total - len(mismatches)}/{total} commands match', flush=True)
    report = {'schema': 1, 'minecraft': lock['minecraft']['version'], 'platform': platform.platform(), 'seed': SEED,
              'commands': total, 'mismatches': len(mismatches), 'groups': summary,
              'comparison_overrides': [
                  {'cmd': op['cmd'], 'comparison': op['comparison'],
                   'vanilla_results': v['results'], 'rust_results': r['results']}
                  for op, v, r in zip(recorded_ops, vanilla, rust) if op.get('comparison')],
              'scope': 'Feedback and failure messages (text and resolved style), chat lines, and CommandResultCallback results of commands run by the single-player owner of an integrated server; world, entity and item effects through probe commands. Explicit positive_result operations compare presence only; their unmodified raw counts remain in the records and comparison_overrides.'}
    for override in report['comparison_overrides']:
        print(f'{override["comparison"]}: {override["cmd"]}: '
              f'Java {override["vanilla_results"]}, Horizon {override["rust_results"]}', flush=True)
    lab.write_json(report_path, report)
    if mismatches and not args.allow_gaps:
        sys.exit(f'{len(mismatches)} command execution mismatches; see {build}')
    print(f'Command execution parity report: {report_path}')


MASKS = [
    (re.compile(r'debug-trace-\d{4}-\d\d-\d\d_\d\d\.\d\d\.\d\d\.txt'), 'debug-trace-<time>.txt'),
    (re.compile(r'after [0-9.]+ second\(s\) and \d+ tick\(s\) \([0-9.,NaInfity]+ tick\(s\) per second\)'), 'after <time>'),
    (re.compile(r'Average time per tick: [0-9.]+ms'), 'Average time per tick: <ms>'),
    (re.compile(r'P50: [0-9.]+ms P95: [0-9.]+ms P99: [0-9.]+ms'), 'P50: <ms> P95: <ms> P99: <ms>'),
    (re.compile(r'debug/client-[0-9_.-]+\.(jfr|json)'), 'debug/client-<time>'),
]


GAMERULES = re.compile(r'/gamerule \(([^)]*)\)')


def masked(record):
    text = json.dumps(record['messages'], ensure_ascii=False)
    # GameRuleCommand registers the rules in identity-hash order, different
    # in every JVM: compare /help's gamerule alternatives as a set.
    text = GAMERULES.sub(lambda m: '/gamerule (' + '|'.join(sorted(m.group(1).split('|'))) + ')', text)
    for pattern, replacement in MASKS:
        text = pattern.sub(replacement, text)
    return text


def positive_result(record):
    """A counted operation's presence, retaining its unmodified raw record."""
    results = record.get('results')
    if not isinstance(results, list) or len(results) != 1 or not isinstance(results[0], list) or len(results[0]) != 2:
        return None
    accepted, count = results[0]
    if type(accepted) is not bool or type(count) is not int or count < 0 or not accepted and count != 0:
        return None
    return count > 0


def compare(v, r, comparison=None):
    # Explicit per-operation scope for random counts (for example explosion
    # fire). All raw feedback/results remain in the JSON and report; every
    # other command still compares exact feedback, styles and results.
    if comparison == 'positive_result':
        left, right = positive_result(v), positive_result(r)
        return None if left is not None and right is not None and left == right else 'positive_result'
    if comparison is not None:
        raise ValueError(f'unknown command comparison: {comparison}')
    if masked(v) != masked(r):
        return 'messages'
    if v['results'] != r['results']:
        if v['cmd'].startswith('/debug stop'):
            return None
        return 'results'
    return None


def render(record):
    lines = [''.join(span[0] for span in m) for m in record['messages']]
    return json.dumps({'messages': lines, 'styles': [[span[1] for span in m] for m in record['messages']], 'results': record['results']}, ensure_ascii=False)


if __name__ == '__main__':
    main()
