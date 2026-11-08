#!/usr/bin/env python3
"""Stack TOOL queries after actual Java item-command admission and save/reload."""
import json
import os
from pathlib import Path
import subprocess

try:
    from . import lab, oracle_harness as harness, test_rust_parity as base
    from . import test_model_baking_parity as baking
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base
    import test_model_baking_parity as baking


def corpus():
    tools = [
        '', '[!minecraft:tool]',
        '[minecraft:tool={rules:[],default_mining_speed:17.0f,damage_per_block:3}]',
        '[minecraft:tool={rules:[{blocks:"#minecraft:mineable/pickaxe",speed:50.0f,correct_for_drops:true}]}]',
        '[minecraft:tool={rules:[{blocks:"#mineable/pickaxe",speed:6.0f,correct_for_drops:true}]}]',
        '[minecraft:tool={rules:[{blocks:"stone",speed:4.0f,correct_for_drops:true}],default_mining_speed:-2.0f}]',
        '[minecraft:tool={rules:[{blocks:["minecraft:stone","minecraft:dirt"],correct_for_drops:false},{blocks:"minecraft:stone",speed:7.0f,correct_for_drops:true},{blocks:"minecraft:stone",speed:90.0f}],default_mining_speed:2.0f,damage_per_block:0}]',
    ]
    for item in ['stick', 'diamond_pickaxe', 'shears']:
        for tool in tools:
            for block in ['stone', 'dirt', 'cobweb']:
                yield dict(item='minecraft:' + item + tool, block='minecraft:' + block)
    # MAX_DAMAGE alone does not add DAMAGE. Non-durable prototypes with an
    # explicit zero DAMAGE need a separate item-aware component-codec fix.
    yield dict(item='minecraft:stick[minecraft:max_stack_size=1,minecraft:max_damage=20,minecraft:tool={rules:[],damage_per_block:3}]', block='minecraft:stone')
    for damage in ['', ',minecraft:damage=0', ',minecraft:damage=2', ',!minecraft:damage', ',minecraft:damage=0,minecraft:unbreakable={}']:
        yield dict(item='minecraft:diamond_pickaxe[minecraft:max_damage=20,minecraft:tool={rules:[],damage_per_block:3}' + damage + ']', block='minecraft:stone')
    for item in ['minecraft:diamond_pickaxe', 'minecraft:stick[minecraft:tool={rules:[],default_mining_speed:17.0f}]']:
        yield dict(item=item, count=0, block='minecraft:stone')


def main():
    lock, jars, _ = base.verify_artifacts()
    build = lab.LOCAL / 'rust-stack-mining-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    sources = [Path(__file__), lab.ROOT / 'tools/oracle_harness.py', lab.ROOT / 'tools/lab.py',
               lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'tools/test_model_baking_parity.py',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaStackMiningOracle.java',
               lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java',
               lab.ROOT / 'crates/horizon-client/examples/probe/stack_mining_probe.rs',
               lab.ROOT / 'crates/horizon-items/src/items.rs', lab.ROOT / 'crates/horizon-items/src/item_data.rs',
               lab.ROOT / 'crates/horizon-items/src/item_nbt.rs', lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock']
    tested = base.source_hashes(sources)
    inputs = build / 'inputs.jsonl'
    rows = list(corpus())
    inputs.write_text(''.join(json.dumps(row) + '\n' for row in rows), encoding='utf-8')
    input_hash = lab.digest(inputs, 'sha256')
    classes, dependencies, _ = harness.compile_oracle(build, 'VanillaStackMiningOracle', jars, baking.client_libraries(), lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaStackMiningOracle', [inputs, build / 'vanilla.txt'], 'java')
    harness.run_logged(['cargo', 'build', '--locked', '-p', 'horizon-client-lib', '--example', 'client_probe'], build, 'rust-build', env=dict(os.environ, CARGO_BUILD_JOBS='2'))
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=lab.ROOT))
    binary = Path(metadata['target_directory']) / 'debug/examples' / ('client_probe.exe' if os.name == 'nt' else 'client_probe')
    resources = Path(os.environ.get('HORIZON_TEST_RESOURCES', lab.RESOURCES))
    harness.run_logged([binary, 'stack_mining_probe', inputs, build / 'rust.txt', resources], build, 'rust', env=lab.game_env())
    harness.compare(build / 'vanilla.txt', build / 'rust.txt', len(rows))
    base.ensure_sources_unchanged(sources, tested)
    if input_hash != lab.digest(inputs, 'sha256'):
        raise RuntimeError('Input fixtures changed during validation')
    lab.write_json(report, dict(schema=1, minecraft=lab.VERSION, units=len(rows),
        sources_sha256=tested, inputs_sha256=input_hash,
        client_jar_sha256=lab.digest(jars['client'], 'sha256'),
        oracle_class_sha256=lab.digest(classes / 'dev/horizonlab/parity/VanillaStackMiningOracle.class', 'sha256'),
        rust_binary_sha256=lab.digest(binary, 'sha256'),
        output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Actual Java item-command admission, TOOL mining speed/drop/wear amount and durable-prototype '
              'capacity queries before/after saved-stack round trips. Custom durability on non-durable '
              'prototypes is deferred. Eye immersion is covered separately by source-derived Game regressions; '
              'this differential does not execute block destruction, durability mutation or statistics.'))
    print(f'{len(rows)} stack mining command/save cases match pinned vanilla {lab.VERSION}')


if __name__ == '__main__':
    main()
