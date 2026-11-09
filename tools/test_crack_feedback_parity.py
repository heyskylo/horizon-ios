#!/usr/bin/env python3
"""Compare actual server crack packets with pinned game-mode stages and clear events.

The Rust fixture checks encoded packets, current tool changes, accepted STOP and
ABORT, and recipients inside, exactly at, and outside 32 blocks, the digger and
another dimension. Java calls incrementDestroyProgress and real STOP/ABORT with
controlled world/player providers. Observer distance is checked by the Rust
integration fixture against ServerLevel.destroyBlockProgress's strict radius;
Java networking and early-STOP delayed destruction are outside this harness.
"""
import argparse
import json
from pathlib import Path
import subprocess

try:
    from . import lab, oracle_harness as harness, test_rust_parity as base
    from . import test_model_baking_parity as baking
except ImportError:
    import lab
    import oracle_harness as harness
    import test_rust_parity as base
    import test_model_baking_parity as baking


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    args = parser.parse_args()
    lock, jars, _ = base.verify_artifacts()
    build = lab.LOCAL / 'crack-feedback-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    (build / 'rust.json').unlink(missing_ok=True)
    sources = [lab.ROOT / path for path in (
        'crates/horizon-client/src/game/lan_actions.rs',
        'crates/horizon-client/src/game/lan_actions/crack_feedback_tests.rs',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCrackFeedbackOracle.java',
        'tools/test_crack_feedback_parity.py')]
    hashes = base.source_hashes(sources)
    classes, dependencies, _ = harness.compile_oracle(
        build, 'VanillaCrackFeedbackOracle', jars, baking.client_libraries(),
        lock['minecraft']['java_major'])
    harness.java_run(build, classes, dependencies, 'VanillaCrackFeedbackOracle',
                     [build / 'java.json'], 'java')
    env = lab.game_env(HORIZON_TEST_RESOURCES=args.resources.resolve(),
                       HORIZON_CRACK_TRACE=build / 'rust.json')
    with (build / 'rust.log').open('w') as log:
        subprocess.run(
            ['cargo', 'test', '-p', 'horizon-client', '--lib',
             'crack_packets_follow_start_clock_and_strict_observer_radius',
             '--', '--ignored', '--nocapture'],
            cwd=lab.ROOT, env=env, stdout=log, stderr=subprocess.STDOUT,
            check=True, timeout=1800)
    java = json.loads((build / 'java.json').read_text())
    rust = json.loads((build / 'rust.json').read_text())
    if rust != java or len(java) != 7:
        raise AssertionError(f'Crack sequence differs: Java {java}; Rust {rust}')
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(minecraft=lock['minecraft']['version'],
                               stages=len(java), observers=5,
                               sources_sha256=hashes,
                               client_jar_sha256=lab.digest(jars['client'], 'sha256'),
                               scope=__doc__))
    print(f'7 crack stages match vanilla; 5 observer destinations verified: {report}')


if __name__ == '__main__':
    main()
