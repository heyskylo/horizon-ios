#!/usr/bin/env python3
"""Compare real folder-pack equipment discovery and reload replacement with vanilla 26.3.

Uses Minecraft's PathPackResources, MultiPackResourceManager and actual
EquipmentAssetManager prepare/apply, and Horizon's actual ArmorStandAssets.
No game, player data, network service, or graphics context is started.
"""
import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import zlib

try:
    from . import lab, test_rust_parity as base, test_model_baking_parity as baking
except ImportError:
    import lab, test_rust_parity as base, test_model_baking_parity as baking


def fixtures(root):
    def png(file, color):
        def chunk(kind, data):
            return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
        file.parent.mkdir(parents=True, exist_ok=True)
        scanlines = b''.join(b'\0' + bytes(color) * 64 for _ in range(32))
        file.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 64, 32, 8, 6, 0, 0, 0))
                         + chunk(b'IDAT', zlib.compress(scanlines)) + chunk(b'IEND', b''))

    def asset(pack, namespace, name, texture, *, player=False, dye=None):
        file = root / pack / 'assets' / namespace / 'equipment' / (name + '.json')
        file.parent.mkdir(parents=True, exist_ok=True)
        layer = {'texture': texture, 'use_player_texture': player}
        if dye is not None:
            layer['dyeable'] = dye
        file.write_text(json.dumps({'layers': {'wings': [layer]}}))
        texture_namespace, path = texture.split(':', 1)
        color = [*hashlib.sha256(texture.encode()).digest()[:3], 255]
        png(root / pack / 'assets' / texture_namespace / 'textures/entity/equipment/wings' / (path + '.png'), color)
    asset('base', 'minecraft', 'common', 'minecraft:old', player=False)
    asset('base', 'gems', 'nested/ruby', 'gems:ruby', player=True, dye={})
    asset('overlay', 'minecraft', 'common', 'minecraft:new', player=True, dye={'color_when_undyed': 0})
    asset('overlay', 'gems', 'nested/ruby', 'gems:new_ruby', player=False)
    asset('overlay', 'added', 'deep/only', 'added:only', player=True)
    (root / 'base/assets/gems/equipment/not_json.txt').write_text('{"layers":{"wings":[{"texture":"gems:ignored"}]}}')
    merged = root / 'merged'
    if merged.exists():
        shutil.rmtree(merged)
    shutil.copytree(root / 'base', merged)
    shutil.copytree(root / 'overlay', merged, dirs_exist_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=('fast', 'dev'), default='fast')
    args = parser.parse_args()
    lock, jars, _ = base.verify_artifacts()
    build = lab.LOCAL / 'rust-equipment-assets-parity'
    classes = build / 'classes'
    classes.mkdir(parents=True, exist_ok=True)
    roots = build / 'fixtures'
    fixtures(roots)
    dependencies = os.pathsep.join(map(str, [jars['client'], *baking.client_libraries()]))
    source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaEquipmentAssetsOracle.java'
    env = dict(os.environ, HORIZON_EQUIPMENT_FIXTURE_ROOT=str(roots),
               HORIZON_EQUIPMENT_FIXTURE_OUTPUT=str(build / 'rust.json'))
    commands = [
        ('compile-java', ['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                          '-nowarn', '-cp', dependencies, '-d', str(classes), str(source)]),
        ('vanilla', ['java', '-Djava.awt.headless=true', '-cp', str(classes) + os.pathsep + dependencies,
                     'dev.horizonlab.parity.VanillaEquipmentAssetsOracle', str(roots), str(build / 'vanilla.json')]),
        ('rust', ['cargo', 'test', '--locked', '--profile', args.profile, '-p', 'horizon-client',
                  '--lib', 'game::entity_model_loading_tests::equipment_assets_native_fixture',
                  '--', '--ignored', '--exact', '--nocapture']),
    ]
    for name, command in commands:
        with (build / (name + '.log')).open('w') as log:
            result = subprocess.run(command, cwd=lab.ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=1800)
        if result.returncode:
            raise RuntimeError(f'{name} failed: {(build / (name + ".log")).read_text()[-4000:]}')
    assert 'test result: ok. 1 passed; 0 failed;' in (build / 'rust.log').read_text(), 'native comparison did not execute the fixture'
    native = json.loads((build / 'vanilla.json').read_text())
    rust = json.loads((build / 'rust.json').read_text())
    assert len(native) == 7, native
    assert native == rust, f'Native equipment={native}; Horizon equipment={rust}'
    print('7/7 actual equipment records match across base, enabled override, and disabled override packs')


if __name__ == '__main__':
    main()
