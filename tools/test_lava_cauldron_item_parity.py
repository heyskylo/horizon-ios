#!/usr/bin/env python3
"""Dropped-item damage from lava cauldron contact against actual vanilla methods.

Uses a constructor-free stand-in level and a Rust environment probe. No game
window, server process, world bootstrap or network connection is involved.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


CASES = [
    ('basin', 'oak_planks', .5, .25, .5, 'lava'),
    ('side', 'oak_planks', -.1, .5, .5, 'lava'),
    ('above', 'oak_planks', .5, .9375, .5, 'lava'),
    ('near_top', 'oak_planks', .5, .937499, .5, 'lava'),
    ('within_top_epsilon', 'oak_planks', .5, .93749995, .5, 'lava'),
    ('above_top', 'oak_planks', .5, .93750001, .5, 'lava'),
    ('rim', 'oak_planks', .05, .95, .5, 'lava'),
    ('outside', 'oak_planks', -.2, .5, .5, 'lava'),
    ('near_side', 'oak_planks', -.12498, .5, .5, 'lava'),
    ('outside_side', 'oak_planks', -.12500001, .5, .5, 'lava'),
    ('empty', 'oak_planks', .5, .25, .5, 'empty'),
    ('netherite', 'netherite_ingot', .5, .25, .5, 'lava'),
    ('below', 'oak_planks', .5, -.3, .5, 'lava'),
    ('foot', 'oak_planks', .05, 0, .05, 'lava'),
    ('cross_gap', 'oak_planks', .5, -.2, .5, 'lava'),
    ('paired_cauldrons', 'oak_planks', .95, .25, .5, 'pair'),
    ('fluid_and_cauldron', 'oak_planks', 1.05, .25, .5, 'mixed'),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lab-root', type=Path)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--work-dir', type=Path, default=lab.LOCAL / 'lava-cauldron-item-parity')
    args = parser.parse_args()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    report = work / 'report.json'
    report.unlink(missing_ok=True)
    if not (args.resources / 'items.json').is_file():
        parser.error('prepared resources are required')
    if args.lab_root:
        lab.LOCAL = args.lab_root.resolve()
    lock, jars, libraries = base.verify_artifacts()
    java = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity'
    sources = [Path(__file__), java / 'VanillaLavaCauldronItemOracle.java', java / 'ParityData.java',
               lab.ROOT / 'crates/horizon-client/src/entities.rs',
               lab.ROOT / 'crates/horizon-client/examples/probe/main.rs',
               lab.ROOT / 'crates/horizon-client/examples/probe/lava_cauldron_item_fixture.rs',
               lab.ROOT / 'Cargo.lock']
    hashes = base.source_hashes(sources)
    inputs = work / 'inputs.tsv'
    inputs.write_text(''.join('\t'.join(map(str, [name, '{id:"minecraft:' + item + '",count:1}', x, y, z, block]))
                              + '\n' for name, item, x, y, z, block in CASES), encoding='utf-8')
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))

    def run(command, label, **kwargs):
        log = work / (label + '.log')
        with log.open('w', encoding='utf-8') as output:
            result = subprocess.run(list(map(str, command)), stdout=output, stderr=subprocess.STDOUT,
                                    timeout=1200, **kwargs)
        if result.returncode:
            raise RuntimeError(f'{label} failed: {log.read_text(encoding="utf-8")[-10000:]}')

    with tempfile.TemporaryDirectory(prefix='horizon-lava-cauldron-') as tmp:
        scratch = Path(tmp)
        env = lab.game_env(os.environ, **{key: scratch / key for key in
                           ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME')})
        run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', cp,
             '-d', classes, java / 'VanillaLavaCauldronItemOracle.java', java / 'ParityData.java'],
            'javac', cwd=scratch, env=env)
        expected, actual = work / 'vanilla.tsv', work / 'rust.tsv'
        expected.unlink(missing_ok=True)
        actual.unlink(missing_ok=True)
        run(['java', '-Xmx512M', '-Djava.awt.headless=true', '-Duser.home=' + tmp,
             '-cp', str(classes) + os.pathsep + cp, 'dev.horizonlab.parity.VanillaLavaCauldronItemOracle',
             inputs, expected], 'java', cwd=scratch, env=env)
        run(['cargo', 'build', '--locked', '-p', 'horizon-client-lib', '--example', 'client_probe',
             '--message-format=json'], 'compile', cwd=lab.ROOT, env=env)
        import json
        artifacts = [json.loads(line) for line in (work / 'compile.log').read_text(encoding='utf-8').splitlines()
                     if line.startswith('{')]
        binaries = [Path(a['executable']) for a in artifacts if a.get('reason') == 'compiler-artifact'
                    and a.get('target', {}).get('name') == 'client_probe' and a.get('executable')]
        assert len(binaries) == 1, 'expected one probe binary'
        binary_hash = lab.digest(binaries[0], 'sha256')
        run([binaries[0], 'lava_cauldron_item_fixture', args.resources.resolve(), inputs, actual],
            'rust', cwd=scratch, env=env)
        assert binary_hash == lab.digest(binaries[0], 'sha256'), 'probe binary changed'
    left, right = expected.read_text().splitlines(), actual.read_text().splitlines()
    assert len(left) == len(right) == len(CASES), 'case count changed'
    mismatches = [name for (name, *_), a, b in zip(CASES, left, right) if a != b]
    assert not mismatches, f'cauldron item mismatches: {mismatches}'
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(cases=len(CASES), matched=len(CASES), sources_sha256=hashes,
                               rust_binary_sha256=binary_hash, server_sha256=lab.digest(jars['server'], 'sha256')))
    print(f'{len(CASES)}/{len(CASES)} lava-cauldron item cases match vanilla')


if __name__ == '__main__':
    main()
