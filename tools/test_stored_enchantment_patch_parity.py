#!/usr/bin/env python3
"""Actual ItemParser/ItemStack codecs versus saved Rust stacks; no server or world."""
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
    return [
        ('default', 'minecraft:enchanted_book', '', '{}'),
        ('empty', 'minecraft:enchanted_book', 'minecraft:stored_enchantments={}', '{"minecraft:stored_enchantments":{}}'),
        ('ordinary', 'minecraft:book', 'minecraft:stored_enchantments={}', '{"minecraft:stored_enchantments":{}}'),
        ('stone', 'minecraft:stone', 'minecraft:stored_enchantments={}', '{"minecraft:stored_enchantments":{}}'),
        ('filled', 'minecraft:enchanted_book', 'minecraft:stored_enchantments={"minecraft:sharpness":1}', '{"minecraft:stored_enchantments":{"minecraft:sharpness":1}}'),
        ('removed', 'minecraft:enchanted_book', '!minecraft:stored_enchantments', '{"!minecraft:stored_enchantments":{}}'),
        ('custom', 'minecraft:enchanted_book', 'minecraft:stored_enchantments={},minecraft:custom_data={keep:7}', '{"minecraft:stored_enchantments":{},"minecraft:custom_data":{keep:7}}'),
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lab-root', type=Path)
    parser.add_argument('--work-dir', type=Path, default=lab.LOCAL / 'stored-enchantment-patch-parity')
    args = parser.parse_args()
    build = args.work_dir.resolve()
    build.mkdir(parents=True, exist_ok=True)
    if args.lab_root:
        lab.LOCAL = args.lab_root.resolve()
        lab.GAME = lab.LOCAL / 'minecraft' / lab.VERSION
    lock, jars, libraries = base.verify_artifacts()
    inputs = build / 'inputs.tsv'
    inputs.write_text(''.join(label + '\t' + item + ('[' + arg + ']' if arg else '') + '\t'
                             + f'{{id:"{item}",count:1,components:{patch}}}\n'
                             for label, item, arg, patch in cases()), encoding='utf-8')
    source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaStoredEnchantmentPatchOracle.java'
    sources = [Path(__file__), source, source.parent / 'ParityData.java', inputs, lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-items/src/item_data.rs', lab.ROOT / 'crates/horizon-items/src/item_nbt.rs',
               lab.ROOT / 'crates/horizon-items/examples/stored_enchantment_patch_probe.rs']
    hashes = base.source_hashes(sources)
    report = build / 'report.json'
    report.unlink(missing_ok=True)

    def run(command, label, **kwargs):
        path = build / (label + '.log')
        with path.open('w', encoding='utf-8') as log:
            result = subprocess.run([str(c) for c in command], stdout=log, stderr=subprocess.STDOUT,
                                    timeout=600, **kwargs)
        if result.returncode:
            raise RuntimeError(f'{label} failed ({result.returncode}):\n{path.read_text(encoding="utf-8")[-12000:]}')
        return path

    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    with tempfile.TemporaryDirectory(prefix='horizon-stored-enchantments-') as tmp:
        scratch = Path(tmp)
        homes = {key: scratch / key for key in ('HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME')}
        env = lab.game_env(os.environ, **homes, CARGO_BUILD_JOBS='2')
        run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', cp,
             '-d', classes, source, source.parent / 'ParityData.java'], 'javac', cwd=scratch, env=env)
        expected, observed = build / 'vanilla.tsv', build / 'rust.tsv'
        expected.unlink(missing_ok=True)
        observed.unlink(missing_ok=True)
        run(['java', '-Xmx512M', '-Djava.awt.headless=true', '-Duser.home=' + tmp, '-cp',
             str(classes) + os.pathsep + cp, 'dev.horizonlab.parity.VanillaStoredEnchantmentPatchOracle',
             inputs, expected], 'java', cwd=scratch, env=env)
        compiled = run(['cargo', 'build', '--locked', '-p', 'horizon-items', '--example',
                        'stored_enchantment_patch_probe', '--message-format=json'], 'compile', cwd=lab.ROOT, env=env)
        binaries = []
        for line in compiled.read_text(encoding='utf-8').splitlines():
            if line.startswith('{'):
                row = json.loads(line)
                if (row.get('reason') == 'compiler-artifact' and row.get('executable')
                        and row.get('target', {}).get('name') == 'stored_enchantment_patch_probe'):
                    binaries.append(Path(row['executable']))
        assert len(binaries) == 1, 'expected one actual stack-codec probe'
        binary_hash = lab.digest(binaries[0], 'sha256')
        run([binaries[0], inputs, observed], 'rust', cwd=scratch, env=env)
        assert binary_hash == lab.digest(binaries[0], 'sha256'), 'probe changed'
    java = [line.split('\t') for line in expected.read_text(encoding='utf-8').splitlines()]
    rust = [line.split('\t') for line in observed.read_text(encoding='utf-8').splitlines()]
    assert len(java) == len(rust) == len(cases())
    assert [row[0] for row in java] == [row[0] for row in rust] == [case[0] for case in cases()]
    assert all(len(row) == 5 and row[1] == row[2] for row in java), 'command/NBT construction differs'
    assert all(len(row) == 2 for row in rust)
    mismatches = [a[0] for a, b in zip(java, rust) if a[2] != b[1]]
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(cases=len(java), matched=len(java) - len(mismatches), mismatches=mismatches,
                               sources_sha256=hashes, server_sha256=lab.digest(jars['server'], 'sha256'),
                               rust_binary_sha256=binary_hash, scope=__doc__))
    assert not mismatches, mismatches
    print(f'{len(java)} command/NBT codec cases match vanilla')


if __name__ == '__main__':
    main()
