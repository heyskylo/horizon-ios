#!/usr/bin/env python3
"""Headless integration contract for Horizon's native client particle helper.

This is a Horizon-only Host API surface, not a new vanilla particle provider.
The fixture calls the generated dyn Host bridge and uses the real particle
engine, pending/live queue, provider tick and CPU quad extraction. It supplies
its own sprite metadata/atlas rectangles; no GPU, window or external mod source
is needed. Vanilla provider physics remains covered by the particle harnesses.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
try:
    from . import lab
    from .integration_env import run_game
except ImportError:
    import lab
    from integration_env import run_game

SOURCES = [
    'crates/horizon-client/src/game.rs',
    'crates/horizon-client/src/game/native_client_particles.rs',
    'crates/horizon-client/src/game/native_client_particles_fixture.rs',
    'crates/horizon-client-render/src/particles.rs',
    'tools/native-hook-fragments/client_particles.json',
    'tools/test_native_client_particles_parity.py',
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='use an already built client')
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    args = parser.parse_args()
    work = lab.LOCAL / 'native-client-particles-parity'
    work.mkdir(parents=True, exist_ok=True)
    report = work / 'report.json'
    report.unlink(missing_ok=True)
    hashes = {name: lab.digest(lab.ROOT / name, 'sha256') for name in SOURCES}
    binary = args.binary
    if binary is None:
        with (work / 'build.log').open('w', encoding='utf-8') as log:
            subprocess.run(['cargo', 'build', '-p', 'horizon-client', '--bin', 'horizon-client'],
                           cwd=lab.ROOT, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=1800)
        binary = lab.cargo_target() / 'debug' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    with tempfile.TemporaryDirectory(prefix='hz-native-particles-') as scratch:
        with (work / 'client.log').open('w', encoding='utf-8') as log:
            run_game([str(binary.resolve()), '--headless', '--resources', str(args.resources.resolve()),
                      '--game-dir', scratch], cwd=scratch,
                     env=lab.game_env(HORIZON_NATIVE_CLIENT_PARTICLES_FIXTURE=str(work)),
                     stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    result = json.loads(report.read_text(encoding='utf-8'))
    for name in ('client_spawn', 'encoded_slots_unchanged', 'menu_and_servers_refused',
                 'nonfinite_refused', 'named_expired', 'normal_limiters_retained'):
        assert result[name] is True, name
    assert result['vanilla_vertices'] == result['named_vertices'] == 4
    assert hashes == {name: lab.digest(lab.ROOT / name, 'sha256') for name in SOURCES}, 'sources changed during test'
    result.update(source_hashes=hashes, binary_sha256=lab.digest(binary, 'sha256'), horizon_only=True)
    lab.write_json(report, result)
    print('native client particles: Host bridge, slot preservation, guards, real queues/ticks/CPU quads and normal limits pass')


if __name__ == '__main__':
    main()
