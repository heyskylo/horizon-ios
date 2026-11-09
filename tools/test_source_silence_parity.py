#!/usr/bin/env python3
"""Compare source-silent explosions and potion impacts without starting a game/server.

The Java world records real ServerLevel.explode packets at radius zero and real
AbstractThrownPotion.onHit level events. Rust drives TNT fuse expiry and projectile
impacts in an isolated Game with silent audio. Only particle presence and sound
emission are compared, plus same-seed bottle XP/server-RNG silence invariance
on each implementation. Absolute bottle RNG ownership, terrain, damage,
potion durations and pitch are not compared.
"""
import os
from pathlib import Path
import tempfile

try:
    from . import lab, oracle_harness as h, test_model_baking_parity as baking, integration_env
except ImportError:
    import lab, oracle_harness as h, test_model_baking_parity as baking, integration_env


def main():
    lock, jars, _ = h.base.verify_artifacts()
    build = lab.LOCAL / 'source-silence-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    resources = Path(os.environ.get('HORIZON_TEST_RESOURCES',
        lab.LOCAL / 'horizon-client' / lock['minecraft']['version'])).resolve()
    if not (resources / 'movement-properties.nbt').is_file():
        raise RuntimeError('Prepared resources required: set HORIZON_TEST_RESOURCES')
    sources = [Path(__file__).resolve(),
        lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaSourceSilenceOracle.java',
        *[lab.ROOT / 'crates/horizon-client/src' / p for p in (
            'game.rs', 'mobs.rs', 'mobs/hostile.rs', 'mobs/wither.rs', 'mobs/sulfur_cube.rs',
            'game/explosion.rs', 'game/tnt.rs', 'game/potions.rs', 'game/persist.rs',
            'game/creatures.rs', 'game/wither.rs', 'game/dragon.rs', 'game/minecarts.rs')]]
    hashes = h.base.source_hashes(sources)
    classes, dependencies, _ = h.compile_oracle(build, 'VanillaSourceSilenceOracle',
        jars, baking.client_libraries(), lock['minecraft']['java_major'])
    with tempfile.TemporaryDirectory(prefix='source-silence-', dir=build) as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_TEST_RESOURCES=str(resources),
            HORIZON_SOURCE_SILENCE_OUTPUT=scratch,
            CARGO_BUILD_JOBS=os.environ.get('CARGO_BUILD_JOBS', '2'),
            RUST_TEST_THREADS='2')
        h.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
            'game::tnt::tests::silent_', '--', '--nocapture'], build, 'rust', env=env)
        rust = build / 'rust.txt'
        rust.write_text(''.join((Path(scratch) / f'{name}.txt').read_text()
            for name in ('explosion', 'potions', 'bottles')), encoding='utf-8')
        h.run_logged(['java', '-Xmx1G', '-Djava.awt.headless=true', '-cp',
            str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.VanillaSourceSilenceOracle',
            build / 'client.txt'], build, 'client', env=env)
    matched = h.compare(build / 'client.txt', rust, 21)
    h.base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        matched_output_lines=matched, sources_sha256=hashes,
        client_jar_sha256=lab.digest(jars['client'], 'sha256'),
        scope=__doc__, output_sha256=lab.digest(rust, 'sha256')))
    print(f'{matched} source-silence cases match vanilla: {report}')


if __name__ == '__main__':
    main()
