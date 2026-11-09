#!/usr/bin/env python3
"""Compare mob flight-effect fall distance with the pinned server's actual aiStep.

The Java cow runs through real travel/movement, stopping before inside-block effects.
Its fluid query is fixed to dry air; active effects are controlled and NoAI is real.
Horizon runs its complete production mob tick in an empty resource-backed world.
Gravity is disabled. No effect, Levitation and Slow Falling are compared for both
NoAI states and zero/upward/downward movement. Positions, damage and landing are
outside this scoped comparison.
"""
import argparse
import os
from pathlib import Path
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path,
                        default=Path(os.environ.get('HORIZON_TEST_RESOURCES', lab.RESOURCES)))
    args = parser.parse_args()
    resources = args.resources.resolve()
    if not (resources / 'entity-types.json').is_file():
        raise SystemExit('prepared resources required; pass --resources PATH')
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'rust-mob-fall-reset-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    java_root = lab.ROOT / 'mods/horizon/src/test/java'
    oracle = java_root / 'dev/horizonlab/parity/VanillaMobFallResetOracle.java'
    sources = [Path(__file__), oracle, lab.ROOT / 'tools/lab.py',
               lab.ROOT / 'tools/oracle_harness.py', lab.ROOT / 'tools/test_rust_parity.py',
               lab.ROOT / 'tools/integration_env.py', lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/mobs.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/ai.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/living.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/world.rs',
               java_root / 'dev/horizonlab/parity/ParityData.java',
               java_root / 'dev/horizonlab/parity/VanillaBlockUpdateOracle.java']
    hashes = base.source_hashes(sources)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    harness.run_logged(['javac', '--release', lock['minecraft']['java_major'], '-proc:none',
                        '-sourcepath', java_root, '-cp', dependencies, '-d', classes, oracle],
                       build, 'compile-java')
    harness.run_logged(['java', '-Xmx512M', '-Djava.awt.headless=true', '-cp',
                        str(classes) + os.pathsep + dependencies,
                        'dev.horizonlab.parity.VanillaMobFallResetOracle', build / 'server.txt'],
                       build, 'server')
    with tempfile.TemporaryDirectory(prefix='horizon-mob-fall-reset-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_TEST_RESOURCES=str(resources),
                   HORIZON_MOB_FALL_RESET_OUTPUT=str(build / 'rust.txt'))
        env.setdefault('CARGO_BUILD_JOBS', '1')
        env.setdefault('RUST_TEST_THREADS', '2')
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client-lib', '--lib',
                            'mobs::ai::flight_effect_fall_tests::flight_effects_reset_accumulated_fall_distance_even_without_ai',
                            '--', '--exact', '--ignored'], build, 'rust', env=env)
    matched = harness.compare(build / 'server.txt', build / 'rust.txt', 18)
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        sources_sha256=hashes, server_jar_sha256=lab.digest(jars['server'], 'sha256'),
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope=__doc__))
    print(f'{matched} mob fall-distance cases match the pinned server: {report}', flush=True)


if __name__ == '__main__':
    main()
