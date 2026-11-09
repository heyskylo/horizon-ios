#!/usr/bin/env python3
"""Check NBT boolean item components through the real stack-to-render bridge.

Uses actual DataComponents.ENCHANTMENT_GLINT_OVERRIDE codecs and ItemStack.hasFoil
on both pinned JARs. Rust decodes stack NBT, constructs the production render view
and resolves synthetic foil and custom-model flag layers. No game window, sprites or GPU are needed.
"""
import json
import os
import platform
from pathlib import Path
import subprocess
try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def corpus():
    for item, default in [('diamond', False), ('diamond_sword', False),
                          ('enchanted_golden_apple', True), ('nether_star', True)]:
        for nbt in [None, 'false', 'true', '-1b', '2b', '0s', '256s', '0', '1',
                    '0L', '1099511627776L', '0.0f', '-0.0f', '0.5f', '-0.5f',
                    '0.0d', '-0.0d', '0.5d', '-0.5d']:
            case = {'id': f'{item}:{nbt}', 'item': 'minecraft:' + item, 'default_glint': default}
            if nbt is not None:
                case['nbt'] = nbt
            yield case


def flags_corpus():
    forms = ['[B;1b,0b]', '[I;1,0]', '[L;1L,0L]', '[1b,0b]', '[1s,0s]',
             '[1,0]', '[1L,0L]', '[0.5f,-0.0f]', '[0.5d,-0.0d]']
    for form in forms:
        nbt = '{flags:' + form + ',floats:[1.25f],colors:[1193046],strings:["keep"]}'
        for index in range(3):
            yield {'id': f'{form}:{index}', 'kind': 'flags', 'nbt': nbt, 'index': index}


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'stack-foil-parity'
    build.mkdir(parents=True, exist_ok=True)
    report_path = build / 'report.json'
    report_path.unlink(missing_ok=True)
    inputs = build / 'cases.jsonl'
    inputs.write_text(''.join(json.dumps(c) + '\n' for c in corpus()), encoding='utf-8')
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaStackFoilOracle.java'
    flags_oracle = oracle.with_name('VanillaStackModelFlagsOracle.java')
    flags_inputs = build / 'flags-cases.jsonl'
    flags_inputs.write_text(''.join(json.dumps(c) + '\n' for c in flags_corpus()), encoding='utf-8')
    probe = lab.ROOT / 'crates/horizon-client/examples/probe/stack_foil_fixture.rs'
    sources = [Path(__file__), oracle, flags_oracle, oracle.with_name('ParityData.java'), probe,
               lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'Cargo.lock', lab.ROOT / 'crates/horizon-client/src/item_render.rs',
               lab.ROOT / 'crates/horizon-items/src/item_data.rs', lab.ROOT / 'crates/horizon-assets/src/item_model.rs']
    tested = base.source_hashes(sources)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    deps = os.pathsep.join(map(str, libraries))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-sourcepath', str(lab.ROOT / 'mods/horizon/src/test/java'),
                    '-cp', str(jars['client']) + os.pathsep + deps, '-d', str(classes), str(oracle), str(flags_oracle)], check=True)
    subprocess.run(['cargo', 'build', '--locked', '-p', 'horizon-client-lib', '--example', 'client_probe'],
                   cwd=lab.ROOT, check=True)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps',
                                                  '--format-version', '1'], cwd=lab.ROOT, text=True))
    binary = Path(metadata['target_directory']) / 'debug/examples' / ('client_probe.exe' if os.name == 'nt' else 'client_probe')
    subprocess.run([str(binary), 'stack_foil_fixture', str(inputs), str(build / 'rust.jsonl')], check=True)
    subprocess.run([str(binary), 'stack_foil_fixture', str(flags_inputs), str(build / 'rust-flags.jsonl')], check=True)
    native = []
    # Canonical JSON text keeps bool distinct from int (Python otherwise treats True == 1).
    def rows(path):
        return [json.dumps(json.loads(line), sort_keys=True) for line in path.read_text().splitlines()]
    actual = rows(build / 'rust.jsonl')
    for side, jar in jars.items():
        output = build / f'{side}.jsonl'
        subprocess.run(['java', '-Xmx512M', '-cp', os.pathsep.join([str(classes), str(jar), deps]),
                        'dev.horizonlab.parity.VanillaStackFoilOracle', str(inputs), str(output)], check=True)
        expected = rows(output)
        assert len(actual) == len(expected) == len(list(corpus())), f'{side}: output length differs'
        assert actual == expected, f'{side}: stack foil differs: {[(a,b) for a,b in zip(actual,expected) if a != b][:3]}'
        native.append({'side': side, 'sha256': lab.digest(jar, 'sha256'), 'matched': len(expected)})
    flags_output = build / 'client-flags.jsonl'
    subprocess.run(['java', '-Xmx512M', '-cp', os.pathsep.join([str(classes), str(jars['client']), deps]),
                    'dev.horizonlab.parity.VanillaStackModelFlagsOracle', str(flags_inputs), str(flags_output)], check=True)
    actual_flags, expected_flags = rows(build / 'rust-flags.jsonl'), rows(flags_output)
    assert len(actual_flags) == len(expected_flags) == len(list(flags_corpus())), 'Custom model flag output length differs'
    assert actual_flags == expected_flags, f'Custom model flags differ: {[(a,b) for a,b in zip(actual_flags,expected_flags) if a != b][:3]}'
    native.append({'side': 'client', 'component': 'custom_model_data.flags', 'matched': len(actual_flags)})
    base.ensure_sources_unchanged(sources, tested)
    lab.write_json(report_path, {'schema': 1, 'platform': platform.platform(),
                                'manifest_sha1': lock['minecraft']['manifest_sha1'], 'minecraft': lock['minecraft']['version'], 'cases': len(actual), 'custom_model_flag_cases': len(actual_flags),
                                'references': native, 'sources_sha256': tested,
                                'scope': 'NBT boolean decoding, JSON render bridge, standard model-layer foil and custom-model conditional selection. No GPU drawing or invalid command validation.'})
    print(f'Stack boolean components: {len(actual)} foil cases match each pinned JAR; {len(actual_flags)} model flag cases match the client; {report_path}')


if __name__ == '__main__':
    main()
