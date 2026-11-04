#!/usr/bin/env python3
"""Compare immediate discarded-entity lookup/save visibility with real 26.3 JARs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile
import lab


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-dir', type=Path, default=lab.GAME)
    parser.add_argument('--resources', type=Path, required=True)
    parser.add_argument('--java-home', type=Path)
    args = parser.parse_args()
    build = lab.LOCAL / 'rust-mob-discard-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    source = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaMobDiscardOracle.java'
    sources = [source, Path(__file__).resolve(),
               lab.ROOT / 'crates/horizon-client/src/game/mob_discard_tests.rs',
               lab.ROOT / 'crates/horizon-client/src/game/commands.rs',
               lab.ROOT / 'crates/horizon-client/src/game/entity.rs',
               lab.ROOT / 'crates/horizon-client/src/game/native_mobs.rs',
               lab.ROOT / 'crates/horizon-client/src/game/persist.rs']
    def hashes():
        return {str(p.relative_to(lab.ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    tested = hashes()
    jars = {side: args.game_dir / 'jars' / name for side, name in
            [('client', 'client.jar'), ('server', 'server-game.jar')]}
    for jar in jars.values():
        with zipfile.ZipFile(jar) as archive:
            assert json.loads(archive.read('version.json'))['id'] == lab.VERSION
    libraries = sorted((args.game_dir / 'libraries/server').rglob('*.jar'))
    assert libraries, 'prepare the pinned game dependencies first'
    java = str(args.java_home / 'bin/java') if args.java_home else 'java'
    javac = str(args.java_home / 'bin/javac') if args.java_home else 'javac'
    env = os.environ.copy()
    scratch = Path(tempfile.mkdtemp(prefix='homes-', dir=build))
    for key in ['HORIZON_HOME', 'HORIZON_LAUNCHER_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME']:
        path = scratch / key
        path.mkdir()
        env[key] = str(path)
    rust = build / 'rust.json'
    rust.unlink(missing_ok=True)
    env.update(HORIZON_TEST_RESOURCES=str(args.resources.resolve()),
               HORIZON_NO_AUDIO='1', HORIZON_LAUNCHER_NO_SELF_UPDATE='1',
               HORIZON_DISCARD_REPORT=str(rust))
    def run(name, command):
        with (build / (name + '.log')).open('w') as log:
            subprocess.run(list(map(str, command)), cwd=lab.ROOT, env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    run('compile-java', [javac, '-proc:none', '-cp', cp, '-d', build, source])
    run('rust', ['cargo', 'test', '--offline', '--locked', '-p', 'horizon-client', '--lib',
                 'discarded_mob_leaves_selectors_and_saves_while_frozen', '--', '--ignored', '--nocapture'])
    actual = json.loads(rust.read_text())
    for side, jar in jars.items():
        output = build / (side + '.json')
        output.unlink(missing_ok=True)
        cp = os.pathsep.join(map(str, [build, jar, *libraries]))
        run(side, [java, '-cp', cp, 'dev.horizonlab.parity.VanillaMobDiscardOracle', output])
        expected = json.loads(output.read_text())
        assert expected == actual, (side, expected, actual)
    assert hashes() == tested, 'tested source changed during the comparison'
    report.write_text(json.dumps({'pass': True, 'minecraft': lab.VERSION,
        'sources_sha256': tested,
        'jars_sha256': {side: hashlib.sha256(p.read_bytes()).hexdigest() for side, p in jars.items()},
        'matched_fields_per_side': len(actual),
        'scope': 'Immediate Entity.discard/manager enumeration, ID/UUID lookup and save assembly, no entity ticks. Java uses actual Marker removal and manager with controlled storage/initial fields; Rust uses actual Game/API37/commands/chunk-save assembly. Dying-but-notremoved behavior and UUID reuse are also asserted; no full-server/pixel/network proof.'}, indent=2) + '\n')
    print(f'Mob discard visibility parity passed: {report}')


if __name__ == '__main__':
    main()
