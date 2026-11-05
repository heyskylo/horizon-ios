#!/usr/bin/env python3
"""Exercise the actual Game world and preview geometry for player/mannequin equipment.

Real prepared model parts, mob types, equipment definitions and CPU renderers
run with no world, renderer or window. Controlled atlas regions identify body,
armor, cape and wing vertices. Native AvatarRenderer's layer/visibility rules
are the contract; no network, authentication or GPU-pixel parity is claimed.
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


def verify(rows):
    assert len(rows) == 80, len(rows)
    for row in rows:
        context = {key: value for key, value in row.items() if key not in ('world', 'reference', 'capeReference', 'previews')}
        world = row['world']
        if not row['invisible']:
            assert world['body'] > 0, f'missing real avatar body: {context}'
        else:
            assert world['body'] == 0, f'invisible body still drawn: {context}'
        if row['spectator']:
            for material in ('armor', 'cape', 'wings', 'default_wings'):
                assert world[material] == 0, f'spectator equipment still drawn: {material}, {context}'
            continue
        if row['gear'] == 'helmet':
            assert row['reference']['armor'] > 0, f'fixture has no real armor geometry: {context}'
            assert world['armor'] == row['reference']['armor'], f'world armor missing or duplicated: {context}, {world}'
        else:
            assert world['armor'] == 0, f'unworn armor drawn: {context}'
        if row['gear'] == 'elytra':
            selected = ('wings' if row['wingMode'] == 'dedicated' else
                        'cape' if row['wingMode'] == 'cape' and row['showCape'] else 'default_wings')
            assert row['reference'][selected] > 0, f'fixture has no real wing model: {context}'
            assert world[selected] == row['reference'][selected], f'world wings missing or duplicated: {context}, {world}'
            for other in {'wings', 'cape', 'default_wings'} - {selected}:
                assert world[other] == 0, f'wrong wing/cape texture: {other}, {context}'
        else:
            assert world['wings'] == world['default_wings'] == 0, f'unworn wings drawn: {context}'
            if row['showCape'] and not row['invisible']:
                expected = row['capeReference']['cape']
                assert expected > 0, f'fixture has no real cape model: {context}'
                assert world['cape'] == expected, f'cape geometry missing or duplicated: {context}, {world}'
            else:
                assert world['cape'] == 0, f'hidden cape drawn: {context}, {world}'
        for preview in row['previews']:
            proof = row['bodyPoseProof']
            assert proof['gpuVertices'] + proof['cpuVertices'] > 0 and proof['matchedExistingPath'], f'body pose comparison was not executed: {context}'
            materials = preview['materials']
            assert materials['body'] > 0, f'preview body missing: {context}, {preview}'
            if row['gear'] == 'helmet':
                assert materials['armor'] > 0, f'preview armor missing: {context}, {preview}'
            if row['gear'] == 'elytra':
                assert materials[selected] > 0, f'preview wings missing: {context}, {preview}'
            elif not row['showCape']:
                assert materials['cape'] == 0, f'hidden preview cape drawn: {context}, {preview}'



def verify_crouch(rows):
    expected = {
        ("minecraft:mannequin", 5, False, True),
        ("minecraft:mannequin", 0, True, False),
        ("minecraft:player", 0, False, False),
        ("minecraft:player", 5, True, True),
    }
    assert len(rows) == 4, len(rows)
    assert {(r["kind"], r["pose"], r["sneaking"], r["expectedCrouching"]) for r in rows} == expected
    for row in rows:
        for field in ("bodyVertices", "referenceBodyVertices", "wingVertices", "referenceWingVertices",
                      "previewBodyVertices", "previewWingVertices"):
            assert row[field] > 0, f"empty geometry cannot prove crouching: {row}"
        assert row["bodyMatches"], f"body used sneak flag instead of visual pose: {row}"
        assert row["wingMatches"], f"wing attachment used sneak flag instead of visual pose: {row}"
        assert row["previewMatches"], f"preview used sneak flag instead of visual pose: {row}"

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--output', type=Path, default=lab.LOCAL / 'avatar-equipment-geometry')
    parser.add_argument('--capture-only', action='store_true', help='record before-fix evidence without accepting it as a regression pass')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if args.binary:
        binary = args.binary.resolve()
    else:
        subprocess.run(['cargo', 'build', '--locked', '--profile', 'dev', '-p', 'horizon-client', '--bin', 'horizon-client'], cwd=lab.ROOT, check=True)
        binary = lab.cargo_target() / 'debug' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    # HORIZON_HOME holds logs; saves/options/caches belong to --game-dir.
    # Keep both the process CWD and that game directory private for this run.
    with tempfile.TemporaryDirectory(prefix='horizon-avatar-geometry-') as scratch:
        game_dir = Path(scratch).resolve()
        with (args.output / 'run.log').open('w') as log:
            run_game([str(binary), '--headless', '--game-dir', str(game_dir),
                      '--resources', str(lab.RESOURCES)], cwd=game_dir,
                     env=lab.game_env(HORIZON_AVATAR_EQUIPMENT_FIXTURE=args.output.resolve()),
                     stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    rows = json.loads((args.output / 'rust.json').read_text())
    if args.capture_only:
        print(f'Captured {len(rows)} actual Game geometry cases; this is not a regression pass')
    else:
        verify(rows)
        crouch = json.loads((args.output / 'crouch.json').read_text())
        verify_crouch(crouch)
        print(f'{len(rows)} actual Game world/preview avatar equipment geometry cases and 4 visual-crouch cases passed')


if __name__ == '__main__':
    main()
