#!/usr/bin/env python3
"""Check NBT glint overrides through the real stack-to-render bridge against vanilla.

Uses actual DataComponents.ENCHANTMENT_GLINT_OVERRIDE codecs and ItemStack.hasFoil
on both pinned JARs. Rust decodes stack NBT, constructs the production render view
and resolves a synthetic model layer. No game window, sprites or GPU are needed.
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


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'stack-foil-parity'
    build.mkdir(parents=True, exist_ok=True)
    report_path = build / 'report.json'
    report_path.unlink(missing_ok=True)
    inputs = build / 'cases.jsonl'
    inputs.write_text(''.join(json.dumps(c) + '\n' for c in corpus()), encoding='utf-8')
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaStackFoilOracle.java'
    probe = lab.ROOT / 'crates/horizon-client/examples/probe/stack_foil_fixture.rs'
    sources = [Path(__file__), oracle, oracle.with_name('ParityData.java'), probe,
               lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/test_rust_parity.py', lab.ROOT / 'Cargo.lock', lab.ROOT / 'crates/horizon-client/src/item_render.rs',
               lab.ROOT / 'crates/horizon-items/src/item_data.rs', lab.ROOT / 'crates/horizon-assets/src/item_model.rs']
    tested = base.source_hashes(sources)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    deps = os.pathsep.join(map(str, libraries))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-sourcepath', str(lab.ROOT / 'mods/horizon/src/test/java'),
                    '-cp', str(jars['client']) + os.pathsep + deps, '-d', str(classes), str(oracle)], check=True)
    subprocess.run(['cargo', 'build', '--locked', '-p', 'horizon-client-lib', '--example', 'client_probe'],
                   cwd=lab.ROOT, check=True)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps',
                                                  '--format-version', '1'], cwd=lab.ROOT, text=True))
    binary = Path(metadata['target_directory']) / 'debug/examples' / ('client_probe.exe' if os.name == 'nt' else 'client_probe')
    subprocess.run([str(binary), 'stack_foil_fixture', str(inputs), str(build / 'rust.jsonl')], check=True)
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
        assert len(actual) == len(expected), f'{side}: output length differs'
        assert actual == expected, f'{side}: stack foil differs: {[(a,b) for a,b in zip(actual,expected) if a != b][:3]}'
        native.append({'side': side, 'sha256': lab.digest(jar, 'sha256'), 'matched': len(expected)})
    base.ensure_sources_unchanged(sources, tested)
    lab.write_json(report_path, {'schema': 1, 'platform': platform.platform(),
                                'manifest_sha1': lock['minecraft']['manifest_sha1'], 'minecraft': lock['minecraft']['version'], 'cases': len(actual),
                                'references': native, 'sources_sha256': tested,
                                'scope': 'NBT boolean decoding, JSON render bridge and standard model-layer foil. No GPU drawing or invalid command validation.'})
    print(f'Stack foil: {len(actual)} cases match each pinned vanilla JAR; {report_path}')


if __name__ == '__main__':
    main()
