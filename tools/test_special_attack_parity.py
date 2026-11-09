#!/usr/bin/env python3
"""Compare actual Player.attack's accepted special-target attacker tail.

Both sides run the full attack entry, not just the target hurt or a copied tail.
The scoped output first proves the special target accepted the hit, then compares
exhaustion, horizontal attacker velocity, sprint and cooldown.
Target movement, drops, sound delivery and weapon/enchantment effects are outside
this probe. Painting/frame controls exercise skipAttackInteraction and rejected
fixed-frame handling without the accepted tail. The zero-damage stand controls
compare only the attacker tail: the existing stand punch on a zero-damage attack
is deliberately not asserted equal. No real server, window, renderer or audio
device is started.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def cases():
    for target in ('boat', 'cart', 'stand', 'dragon', 'painting', 'frame', 'filled_frame', 'fixed_frame'):
        for creative in (False, True):
            for sprint in (False, True):
                for ticks in (0, 20):
                    for knockback in (False, True):
                        item = ('{id:"minecraft:stick",count:1,components:'
                                '{"minecraft:enchantments":{"minecraft:knockback":2}}}') if knockback else '-'
                        case = dict(seed=0, time=100, pos=[0, 0, 0], cells=[], items=[], entities=[],
                                   special_attack=dict(target=target, creative=creative, sprint=sprint,
                                                       ticks=ticks, item=item, **{'break': False},
                                                       player=[0.5, 0, -2], motion=[0.1, 0, 0.1],
                                                       fall=0, fall_flying=False, flying=False))
                        yield case
                        if target == 'stand':
                            yield {**case, 'special_attack': {**case['special_attack'], 'break': True}}
    # Player.attack tests base/magic damage BEFORE the mace's item bonus. The
    # old stand punch is outside this patch; none of these may pay the new tail.
    for control in (
        dict(attack_damage=0, item='{id:"minecraft:stick",count:1,components:{"minecraft:enchantments":{"minecraft:knockback":2}}}'),
        dict(weakness=True, item='-'),
        dict(attack_damage=0, item='{id:"minecraft:mace",count:1}', fall=5),
    ):
        for sprint in (False, True):
            for ticks in (0, 20):
                op = dict(target='stand', creative=False, sprint=sprint, ticks=ticks,
                          tail_only=True, **{'break': False}, player=[0.5, 0, -2],
                          motion=[0.1, 0, 0.1], fall=0, fall_flying=False, flying=False)
                op.update(control)
                yield dict(seed=0, time=100, pos=[0, 0, 0], cells=[], items=[], entities=[], special_attack=op)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path,
                        default=Path(os.environ.get('HORIZON_TEST_RESOURCES', lab.RESOURCES)))
    parser.add_argument('--test-binary', type=Path, required=True,
                        help='Previously built horizon-client --lib test artifact; avoids world startup')
    parser.add_argument('--work', type=Path, default=lab.LOCAL / 'special-attack-parity')
    args = parser.parse_args()
    lock, jars, libraries = base.verify_artifacts()
    source_paths = [lab.ROOT / path for path in (
        'crates/horizon-client/src/game.rs',
        'crates/horizon-client/src/game/melee.rs',
        'crates/horizon-client/src/game/armor_stands.rs',
        'crates/horizon-client/src/game/boats.rs',
        'crates/horizon-client/src/game/minecarts.rs',
        'crates/horizon-client/src/game/dragon.rs',
        'crates/horizon-client/src/game/special_attack_fixture.rs',
        'crates/horizon-client/src/game/dispense_fixture.rs',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaDispenserOracle.java',
        'tools/test_special_attack_parity.py')]
    source_hashes = base.source_hashes(source_paths)
    binary = args.test_binary.resolve()
    binary_hash = lab.digest(binary, 'sha256')
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    for name in ('native.txt', 'rust.txt', 'report.json', 'differences.json', 'tail-only-target-differences.json'):
        (work / name).unlink(missing_ok=True)
    fixtures = list(cases())
    input_path = work / 'cases.jsonl'
    input_path.write_text(''.join(json.dumps(case) + '\n' for case in fixtures), encoding='utf-8')
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    sources = lab.ROOT / 'mods/horizon/src/test/java'
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    with tempfile.TemporaryDirectory(prefix='special-attack-') as scratch:
        subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none', '-nowarn',
                        '-sourcepath', str(sources), '-cp', dependencies, '-d', str(classes),
                        str(sources / 'dev/horizonlab/parity/VanillaDispenserOracle.java')],
                       cwd=scratch, check=True)
        subprocess.run(['java', '-Xmx1G', '-cp', str(classes) + os.pathsep + dependencies,
                        'dev.horizonlab.parity.VanillaDispenserOracle', str(args.resources.resolve()),
                        str(input_path), str(work / 'native.txt')], cwd=scratch, check=True)
        subprocess.run(
            [str(binary), 'game::special_attack_fixture::tests::special_attack_cases_without_world_startup',
             '--exact', '--ignored', '--test-threads=1'], cwd=scratch,
            env=lab.game_env(HORIZON_TEST_RESOURCES=str(args.resources.resolve()),
                             HORIZON_SPECIAL_ATTACK_CASES=f'{input_path},{work / "rust.txt"}'),
            check=True, timeout=180)
    def scoped(path):
        rows = path.read_text(encoding='utf-8').splitlines()
        errors = [row for row in rows if row.startswith('error')]
        if errors:
            raise AssertionError(f'{path.name}: {errors[:3]}')
        return [row for row in rows if row.startswith('special ')]
    vanilla, rust = scoped(work / 'native.txt'), scoped(work / 'rust.txt')
    assert len(vanilla) == len(rust) == len(fixtures), (len(vanilla), len(rust), len(fixtures))
    for case, a, b in zip(fixtures, vanilla, rust):
        if (case['special_attack']['target'] in ('boat', 'cart', 'stand', 'dragon')
                and not case['special_attack'].get('tail_only')):
            assert a.startswith('special accepted '), (case['special_attack'], 'oracle did not accept', a)
            assert b.startswith('special accepted '), (case['special_attack'], 'production did not accept', b)
    def comparable(case, row):
        return row.split(' ', 2)[2] if case['special_attack'].get('tail_only') else row
    differences = [(case['special_attack'], a, b) for case, a, b in zip(fixtures, vanilla, rust)
                   if comparable(case, a) != comparable(case, b)]
    outcome_differences = [(case['special_attack'], a, b) for case, a, b in zip(fixtures, vanilla, rust)
                          if case['special_attack'].get('tail_only') and a.split()[1] != b.split()[1]]
    (work / 'tail-only-target-differences.json').write_text(json.dumps(outcome_differences, indent=2), encoding='utf-8')
    (work / 'differences.json').write_text(json.dumps(differences, indent=2), encoding='utf-8')
    base.ensure_sources_unchanged(source_paths, source_hashes)
    assert binary_hash == lab.digest(binary, 'sha256'), 'binary changed during parity run'
    lab.write_json(work / 'report.json', dict(minecraft=lock['minecraft']['version'],
        cases=len(fixtures), matched=len(fixtures)-len(differences), differences=len(differences),
        tail_only_controls=sum(bool(case['special_attack'].get('tail_only')) for case in fixtures),
        tail_only_target_outcome_differences=len(outcome_differences),
        sources_sha256=source_hashes, binary_sha256=binary_hash,
        input_sha256=lab.digest(input_path, 'sha256'),
        server_jar_sha256=lab.digest(jars['server'], 'sha256'), scope=__doc__))
    assert not differences, f'{len(differences)}/{len(fixtures)} differ: {differences[:3]}'
    print(f'{len(fixtures)}/{len(fixtures)} actual special-target attack tails match vanilla')


if __name__ == '__main__':
    main()
