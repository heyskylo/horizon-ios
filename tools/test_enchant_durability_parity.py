#!/usr/bin/env python3
"""Compare enchantment durability effects with vanilla.

Java (`VanillaEnchantDurabilityOracle`) loads the vanilla data pack, puts an
Unbreaking or Mending enchantment on a real `ItemStack` and calls
`EnchantmentHelper.processDurabilityChange` / `modifyDurabilityToRepairFromXp`
with a seeded level random. Rust (`enchant_durability_fixture`) evaluates the
same `minecraft:item_damage` / `minecraft:repair_with_xp` value effects through
`horizon_items::enchant::Enchantments`. Both print one integer per case.

The base corpus is Unbreaking: levels 0-3 on five damageable items, the
small/large removal paths and their seam, negative/zero/large amounts and
several random seeds. `--no-mending` leaves out Mending's XP repair and
reproduces the Unbreaking corpus exactly.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess

try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base

ITEMS = ['minecraft:diamond_pickaxe', 'minecraft:netherite_pickaxe', 'minecraft:shield',
         'minecraft:leather_boots', 'minecraft:elytra']
UNBREAKING_LEVELS = [0, 1, 2, 3]
UNBREAKING_AMOUNTS = [0, 1, 2, 3, 4, 7, 16, 63, 64, 127, 128, 129, 1000, -1]
SEEDS = [0, 1, 2, 146, 12345, -99]
MENDING_AMOUNTS = [0, 1, 2, 5, 10, 100]
MENDING_DAMAGE = [0, 1, 5, 50, 100]


def cases(mending):
    for item in ITEMS:
        for level in UNBREAKING_LEVELS:
            for amount in UNBREAKING_AMOUNTS:
                for seed in SEEDS:
                    yield dict(mode='unbreaking', item=item, damage=10, level=level,
                               amount=amount, seed=seed)
    if mending:
        for item in ITEMS:
            for level in (1, 2, 3):
                for damage in MENDING_DAMAGE:
                    for amount in MENDING_AMOUNTS:
                        yield dict(mode='mending', item=item, damage=damage, level=level,
                                   amount=amount, seed=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--no-mending', action='store_true',
                        help='leave out the Mending family, reproducing the Unbreaking corpus')
    args = parser.parse_args()
    build = lab.LOCAL / 'enchant-durability-parity'
    build.mkdir(parents=True, exist_ok=True)
    report_path = build / 'report.json'
    report_path.unlink(missing_ok=True)
    if args.prepare:
        lab.prepare()
    lock, jars, libraries = base.verify_artifacts()
    resources = lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    all_cases = list(cases(not args.no_mending))
    inputs = build / 'cases.jsonl'
    inputs.write_text(f'#resources {resources}\n' + ''.join(json.dumps(c) + '\n' for c in all_cases),
                      encoding='utf-8')
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaEnchantDurabilityOracle.java'
    sources = [oracle, Path(__file__), lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/lab.py',
               lab.ROOT / 'crates/horizon-client/examples/probe/enchant_durability_fixture.rs',
               lab.ROOT / 'crates/horizon-items/src/enchant.rs',
               lab.ROOT / 'crates/horizon-items/src/items.rs',
               lab.ROOT / 'crates/horizon-items/src/item_data.rs']
    tested = base.source_hashes(sources)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    java_root = lab.ROOT / 'mods/horizon/src/test/java'

    def run(command, name, timeout=900):
        with (build / (name + '.log')).open('w', encoding='utf-8') as log:
            subprocess.run(list(map(str, command)), cwd=lab.ROOT, check=True, timeout=timeout,
                           stdout=log, stderr=subprocess.STDOUT)

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-sourcepath', str(java_root),
         '-cp', dependencies, '-d', classes, oracle], 'compile-java')
    run(['java', '-Xmx2g', '-cp', str(classes) + os.pathsep + dependencies,
         'dev.horizonlab.parity.VanillaEnchantDurabilityOracle', inputs, build / 'client.txt'], 'client')
    run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client-lib', '--example', 'client_probe'],
        'compile-rust', timeout=3600)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                                                  cwd=lab.ROOT, text=True))
    binary = Path(metadata['target_directory']) / 'fast/examples/client_probe'
    run([binary, 'enchant_durability_fixture', inputs, build / 'rust.txt'], 'rust')
    matched = base.compare(build / 'client.txt', build / 'rust.txt')
    base.ensure_sources_unchanged(sources, tested)
    lab.write_json(report_path, {
        'schema': 1, 'minecraft': lock['minecraft']['version'], 'platform': platform.platform(),
        'cases': len(all_cases), 'matched': matched, 'sources_sha256': tested,
        'scope': 'EnchantmentHelper.processDurabilityChange (Unbreaking item_damage remove_binomial, '
                 'small/large paths, level scaling and seeded random draws) and '
                 'modifyDurabilityToRepairFromXp (Mending repair_with_xp multiply) over real ItemStacks. '
                 'Other enchantment effects, the equipment slots and the Game repair loop remain separate.'})
    print(f'{len(all_cases):,} enchant durability cases match vanilla ({matched} output lines): {report_path}',
          flush=True)


if __name__ == '__main__':
    main()
