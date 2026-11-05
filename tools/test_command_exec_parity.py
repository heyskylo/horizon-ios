#!/usr/bin/env python3
"""Compare Horizon's command execution with vanilla's real command implementations.

Java (VanillaCommandExecOracle) starts a stand-in integrated server: a real
MinecraftServer with a generated world, Commands.CommandSelection.INTEGRATED
and one single-player owner named Player. Rust runs the same script through
Horizon's integrated server with a connected headless client (HORIZON_COMMAND_SCRIPT). Both run each command as
the player through Commands.performPrefixedCommand and record every system or
chat message the player receives (text and resolved style: colour, bold,
italic, underlined, strikethrough, obfuscated) together with the results the
command reported to the player's CommandResultCallback. World, entity and item
changes are observed through probe commands whose feedback reports them
(execute if, clear with a zero count, data get, scoreboard queries...).
Messages that depend on time or a random UUID are compared after masking.
Direct falling-block probes also compare the physical entities in each level,
independently of the command selector's support for those entity types.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import zipfile

try:
    from . import lab, test_rust_parity as base
    from .integration_env import run_game
    from . import command_exec_corpus as corpus_module
except ImportError:
    import lab
    from integration_env import run_game
    import test_rust_parity as base
    import command_exec_corpus as corpus_module

SEED = 1


def install_datapack_fixtures(world, predicates, functions, jar, advancements=None, loot_tables=None, recipes=None):
    if not predicates and not functions and not advancements and not loot_tables and not recipes:
        return
    with zipfile.ZipFile(jar) as archive:
        version = json.loads(archive.read('version.json'))['pack_version']
    fmt = [version['data_major'], version['data_minor']]
    pack = world / 'datapacks' / 'command-selector-fixture'
    pack.mkdir(parents=True, exist_ok=True)
    (pack / 'pack.mcmeta').write_text(json.dumps({'pack': {
        'description': 'Command execution fixtures', 'pack_format': fmt[0],
        'min_format': fmt, 'max_format': fmt}}), encoding='utf-8')
    for identifier, predicate in predicates.items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'predicate' / (path + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(predicate), encoding='utf-8')
    for identifier, contents in functions.items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'function' / (path + '.mcfunction')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(contents, encoding='utf-8')

    for identifier, advancement in (advancements or {}).items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'advancement' / (path + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(advancement), encoding='utf-8')
    for identifier, table in (loot_tables or {}).items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'loot_table' / (path + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(table), encoding='utf-8')


    for identifier, recipe in (recipes or {}).items():
        namespace, path = identifier.split(':', 1)
        target = pack / 'data' / namespace / 'recipe' / (path + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(recipe), encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--advancement-loot-inline-parser-diagnostic', action='store_true',
                        help='retain all 100 inline observations, including the three missing-reference ACK differences')
    parser.add_argument('--advancement-loot-named-diagnostic', action='store_true',
                        help='retain the separate named world-loot registry-admission diagnostic')
    parser.add_argument('--pot-string-sherd-diagnostic', action='store_true',
                        help='use string sherd input in the pot data group (known separate codec gap)')
    parser.add_argument('--limit', type=int, default=40, help='print at most this many mismatches')
    parser.add_argument('--allow-gaps', action='store_true', help='report mismatches without failing')
    parser.add_argument('--wolf-anger-clock-diagnostic', action='store_true',
                        help='retain the separate loaded-NoAI absolute anger clock diagnostic')
    parser.add_argument('--only', help='run only corpus groups whose name contains this')
    parser.add_argument('--array-storage-diagnostic', action='store_true',
                        help='run all numeric-array edit cases, including the known wildcard/string storage side effects')
    parser.add_argument('--sulfur-power-load-diagnostic', action='store_true',
                        help='retain the immediate-load powered sulfur cube H249 diagnostic')
    parser.add_argument('--binary', type=Path, help='use an already built Horizon game instead of building it')
    parser.add_argument('--slot-codec-diagnostic', action='store_true',
                        help='retain the separate invalid inline slot-range codec diagnostic')
    parser.add_argument('--work-dir', type=Path, help='isolated scratch output directory')
    parser.add_argument('--loaded-fixture', action='store_true', help='pin actual view/simulation2 and use real player tickets')
    parser.add_argument('--loaded-block-diagnostic', action='store_true', help='retain separate FULL block-access observations')
    parser.add_argument('--mob-quad-fixture', action='store_true', help='guarded live mob quad-leash scene')
    parser.add_argument('--mob-quad-raw-diagnostic', action='store_true', help='retain full-precision leash diagnostic')
    parser.add_argument('--item-merge-diagnostic', action='store_true',
                        help='retain the original two-tag and spectator-selector item-merge diagnostic')
    parser.add_argument('--orb-water-fixture', action='store_true', help='guarded orb water/air scene')
    parser.add_argument('--orb-water-raw-diagnostic', action='store_true', help='add whole Pos/Motion records')
    parser.add_argument('--orb-lava-fixture', action='store_true', help='guarded server orb lava contact scene')
    parser.add_argument('--orb-lava-raw-diagnostic', action='store_true', help='retain random Pos/Motion as exact raw diagnostics')
    parser.add_argument('--mob-home-fixture', action='store_true')
    parser.add_argument('--mob-home-holder-removal-diagnostic', action='store_true')
    parser.add_argument('--mob-home-late-detach-control', action='store_true')
    parser.add_argument('--item-contents-fixture', action='store_true')
    parser.add_argument('--item-contents-provider-empty-control', action='store_true')
    parser.add_argument("--boat-item-hit-fixture", action="store_true")
    parser.add_argument("--reference-only", action="store_true")
    parser.add_argument("--boat-motion-diagnostic", action="store_true")
    parser.add_argument('--reinforcement-original-diagnostic', action='store_true', help='replay the original unisolated174/108 fixture, retaining its terrain prerequisite failure')
    parser.add_argument('--reinforcement-target-diagnostic', action='store_true', help='retain the separate creative-callee target relation record (full209)')
    parser.add_argument('--reinforcement-raw-diagnostic', action='store_true', help='add unnormalized full callee snapshots; not the typed gate')
    parser.add_argument("--pufferfish-contact-fixture", action="store_true")
    parser.add_argument("--pufferfish-body-diagnostic", action="store_true")
    parser.add_argument('--moving-piston-history-diagnostic', action='store_true',
                        help='retain the full piston presentation/default-origin controls, including H191 insertion-history gaps')
    parser.add_argument("--armor-stand-honey-fixture", action="store_true")
    parser.add_argument("--pufferfish-mob-fixture", action="store_true")
    parser.add_argument("--pufferfish-mob-duration-diagnostic", action="store_true")
    parser.add_argument("--pufferfish-mob-overlap-control", action="store_true")
    parser.add_argument("--map-decoration-namespace-fixture", action="store_true")
    parser.add_argument("--map-decoration-empty-diagnostic", action="store_true")
    parser.add_argument('--piglin-anger-memory-fixture', action='store_true')
    parser.add_argument('--piglin-anger-raw-diagnostic', action='store_true')
    parser.add_argument('--piglin-anger-memory-diagnostic', action='store_true')
    parser.add_argument('--map-decoration-patch-fixture', action='store_true')
    parser.add_argument('--map-decoration-removal-diagnostic', action='store_true')
    args = parser.parse_args()
    build = args.work_dir.resolve() if args.work_dir else lab.LOCAL / 'command-exec-parity'
    build.mkdir(parents=True, exist_ok=True)
    report_path = build / 'report.json'
    report_path.unlink(missing_ok=True)
    if args.loaded_fixture:
        for name in ['client_config.json', 'rust-client-config.json']:
            (build / name).unlink(missing_ok=True)
    if args.prepare:
        lab.prepare()
    lock, jars, libraries = base.verify_artifacts()
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaCommandExecOracle.java'
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependency_path = os.pathsep.join(map(str, libraries))

    def run(command, name, **kw):
        with (build / (name + '.log')).open('w', encoding='utf-8') as log:
            runner = run_game if kw.pop('game', False) else subprocess.run
            result = runner(list(map(str, command)), stdout=log, stderr=subprocess.STDOUT, **kw)
        if result.returncode:
            tail = (build / (name + '.log')).read_text(errors='replace')[-3000:]
            sys.exit(f'{name} failed ({result.returncode}):\n{tail}')

    groups = corpus_module.groups()
    if not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_FRAME_MAPS]
    if not args.only and not args.wolf_anger_clock_diagnostic:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_WOLF_ANGER]
    if args.wolf_anger_clock_diagnostic:
        try:
            from .wolf_anger_corpus import groups as anger_groups
        except ImportError:
            from wolf_anger_corpus import groups as anger_groups
        groups = [('setup', corpus_module.group_setup()),
                  *anger_groups(corpus_module.cmd, corpus_module.setup, clock_diagnostic=True)]
    isolated_reinforcements = corpus_module.ISOLATED_REINFORCEMENTS
    if not args.only:
        groups = [(name, ops) for name, ops in groups if name not in isolated_reinforcements]
    if args.reinforcement_raw_diagnostic or args.reinforcement_original_diagnostic:
        groups = [(name, corpus_module.command_zombie_reinforcements(
            once=name == 'command reinforcement once', raw=args.reinforcement_raw_diagnostic, legacy=args.reinforcement_original_diagnostic) if name in isolated_reinforcements else ops)
            for name, ops in groups]
    if not args.reinforcement_target_diagnostic and not args.reinforcement_raw_diagnostic and not args.reinforcement_original_diagnostic:
        # H300's declared208 gate excludes exactly this separate target-policy lead.
        # --reinforcement-target-diagnostic retains full209/all30 original records.
        groups = [(name, [op for op in ops if op.get('label') != 'h300 original/hard/callee target']
                   if name == 'command zombie reinforcements' else ops) for name, ops in groups]
    if args.advancement_loot_named_diagnostic and args.advancement_loot_inline_parser_diagnostic:
        parser.error('choose only one advancement-loot diagnostic')
    if args.advancement_loot_named_diagnostic or args.advancement_loot_inline_parser_diagnostic:
        groups = [(name, corpus_module.advancement_world_loot_rewards(
            named_table=args.advancement_loot_named_diagnostic, include_missing_ack=True)
                   if name == 'advancement world loot rewards' else ops) for name, ops in groups]
    # These overlays replace built-in recipes: run each in its own world, never
    # beside the generic corpus's fixed vanilla recipe-count controls.
    isolated_recipes = corpus_module.ISOLATED_RECIPES
    if not args.only:
        groups = [(name, ops) for name, ops in groups if name not in isolated_recipes]
    if args.loaded_fixture:
        groups = [('setup', corpus_module.group_setup()),
                  ('loaded ticket removal', corpus_module.loaded_ticket_removal(args.loaded_block_diagnostic))]
    if args.moving_piston_history_diagnostic:
        groups.append(('moving piston presentation diagnostic',
                       corpus_module.moving_piston_presentation_diagnostic()))
    if args.pot_string_sherd_diagnostic:
        groups = [(name, corpus_module.decorated_pot_data(string_sherd=True)
                   if name == 'decorated pot data' else ops) for name, ops in groups]
    if args.array_storage_diagnostic:
        groups = [('setup', corpus_module.group_setup()),
                  ('numeric array edits with storage side effects',
                   corpus_module.numeric_array_edits(include_storage_side_effects=True))]
    if args.sulfur_power_load_diagnostic:
        groups = [('setup', corpus_module.group_setup()),
                  ('sulfur powered-load diagnostic', corpus_module.sulfur_cube_fuse_ticks(True))]
    if args.map_decoration_namespace_fixture:
        try:
            from .map_decoration_namespace_corpus import map_decoration_namespaces
        except ImportError:
            from map_decoration_namespace_corpus import map_decoration_namespaces
        groups = [("setup", corpus_module.group_setup()), ("map decoration namespaces", map_decoration_namespaces(args.map_decoration_empty_diagnostic))]
    if args.armor_stand_honey_fixture:
        try:
            from .armor_stand_honey_corpus import armor_stand_honey
        except ImportError:
            from armor_stand_honey_corpus import armor_stand_honey
        groups = [("setup", corpus_module.group_setup()), ("armor stand honey slide", armor_stand_honey())]
    if args.mob_quad_fixture:
        try:
            from .mob_quad_leash_corpus import mob_quad_leash
        except ImportError:
            from mob_quad_leash_corpus import mob_quad_leash
        groups = [('setup', corpus_module.group_setup()), ('mob quad leash dispatch', mob_quad_leash(args.mob_quad_raw_diagnostic))]
    if args.item_merge_diagnostic:
        try:
            from .item_merge_corpus import stack_limit_merges
        except ImportError:
            from item_merge_corpus import stack_limit_merges
        groups = [('setup', corpus_module.group_setup()),
                  ('item stack-size override merges',
                   stack_limit_merges(corpus_module.cmd, corpus_module.setup, diagnostic=True))]
    if args.orb_water_fixture:
        try:
            from .orb_water_corpus import orb_water, arithmetic_inputs
        except ImportError:
            from orb_water_corpus import orb_water, arithmetic_inputs
        groups = [('setup', corpus_module.group_setup()), ('orb underwater arithmetic', orb_water(args.orb_water_raw_diagnostic))]
    if args.orb_lava_fixture:
        try:
            from .orb_lava_corpus import orb_lava
        except ImportError:
            from orb_lava_corpus import orb_lava
        groups = [('setup', corpus_module.group_setup()),
                  ('orb lava contact', orb_lava(args.orb_lava_raw_diagnostic))]
    if args.mob_home_fixture:
        try:
            from .mob_home_corpus import mob_home
        except ImportError:
            from mob_home_corpus import mob_home
        groups = [('setup', corpus_module.group_setup()), ('mob home NBT', mob_home(args.mob_home_holder_removal_diagnostic, args.mob_home_late_detach_control))]
    if args.item_contents_fixture:
        try:
            from .item_contents_corpus import item_contents
        except ImportError:
            from item_contents_corpus import item_contents
        groups = [('setup', corpus_module.group_setup()), ('item component contents', item_contents(args.item_contents_provider_empty_control))]
    if args.boat_item_hit_fixture:
        from boat_item_hit_corpus import boat_item_hits
        groups = [('boat item hit', boat_item_hits(args.boat_motion_diagnostic))]
    if args.pufferfish_mob_fixture:
        from pufferfish_mob_contact_corpus import mob_contacts, overlapping_cod_control
        groups = [("pufferfish mob contact", overlapping_cod_control() if args.pufferfish_mob_overlap_control
                   else mob_contacts(args.pufferfish_mob_duration_diagnostic))]
    if args.pufferfish_contact_fixture:
        from pufferfish_contact_corpus import contact_fixture
        groups = [("pufferfish player contact", contact_fixture(args.pufferfish_body_diagnostic))]
    if args.only:
        groups = [g for g in groups if args.only in g[0] or g[0] == 'setup']
    if args.slot_codec_diagnostic:
        groups.append(('slot source codec diagnostic', [corpus_module.cmd(
            '/item fill block 4 200 4 {type:"minecraft:slot_range",slots:"container.99"} with diamond')]))
    if any(name in isolated_reinforcements for name, _ in groups) and sum(name != 'setup' for name, _ in groups) != 1:
        parser.error('reinforcement fixtures require --only with one exact group')
    if any(name in isolated_recipes for name, _ in groups) and sum(name != 'setup' for name, _ in groups) != 1:
        parser.error('crafting recipe fixtures require --only with one exact group')
    if args.piglin_anger_memory_fixture:
        from piglin_anger_memory_corpus import piglin_anger_memory
        groups = [('setup', corpus_module.group_setup()), ('piglin live anger memory', piglin_anger_memory(args.piglin_anger_raw_diagnostic, args.piglin_anger_memory_diagnostic))]
    if args.map_decoration_patch_fixture:
        from map_decoration_patch_corpus import map_decoration_patches
        groups = [('setup', corpus_module.group_setup()), ('map decoration saved patch', map_decoration_patches(args.map_decoration_removal_diagnostic))]
    script = []
    for _, lines in groups:
        script.extend(lines)
    script_path = build / 'script.jsonl'
    script_path.write_text(''.join(json.dumps({k: v for k, v in op.items() if k != 'expect_result'},
                                            ensure_ascii=False) + '\n' for op in script), encoding='utf-8')
    predicates = corpus_module.predicate_selector_fixtures([name for name, _ in groups])
    functions = corpus_module.function_return_fixtures([name for name, _ in groups])
    recipes = corpus_module.crafting_priority_recipes([name for name, _ in groups])
    advancements = corpus_module.advancement_reward_fixtures([name for name, _ in groups])
    loot_advancements, loot_tables = corpus_module.advancement_loot_fixtures([name for name, _ in groups])
    advancements.update(loot_advancements)

    if args.pufferfish_contact_fixture:
        from pufferfish_contact_corpus import advancements as contact_advancements
        advancements.update(contact_advancements())

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', str(jars['server']) + os.pathsep + dependency_path, '-d', classes, oracle], 'compile-java')
    vanilla_dir = build / 'vanilla'
    shutil.rmtree(vanilla_dir, ignore_errors=True)
    vanilla_dir.mkdir()
    install_datapack_fixtures(vanilla_dir / 'universe/harness', predicates, functions, jars['server'], advancements, loot_tables, recipes=recipes)
    java_options = []
    if args.orb_water_fixture:
        (build / 'orb-water-reference.json').unlink(missing_ok=True)
        direct_input = build / 'orb-water-inputs.json'
        direct_input.write_text(json.dumps(arithmetic_inputs()), encoding='utf-8')
        java_options = ['-Dhorizon.orbWaterArithmetic=' + str(direct_input)]
    run(['java', '-Xmx3G', '-Duser.timezone=UTC', *(['-Dhorizon.loadedFixture=true'] if args.loaded_fixture else []), *java_options, '-cp', os.pathsep.join([str(classes), str(jars['server']), dependency_path]),
         'dev.horizonlab.parity.VanillaCommandExecOracle', vanilla_dir / 'universe', script_path, build / 'vanilla.jsonl', SEED], 'java', cwd=vanilla_dir)

    if args.orb_water_fixture:
        direct = json.loads((build / 'orb-water-reference.json').read_text())
        if ([r.get('input') for r in direct] != arithmetic_inputs() or
                any(len(r.get('bits', [])) != 3 or
                    any(re.fullmatch(r'[0-9a-f]{1,16}', b) is None for b in r['bits']) for r in direct)):
            sys.exit('Java direct underwater reference prerequisite failed')
    if args.reference_only:
        reference = [json.loads(line) for line in (build / 'vanilla.jsonl').read_text().splitlines()]
        recorded = [op for op in script if op['op'] in ('player_state', 'player_loop', 'map_state', 'frame_map_state', 'mob_anger', 'menu_state') or
                    (op['op'] == 'cmd' and op.get('record', True)) or
                    (op['op'] == 'tick' and op.get('record'))]
        if len(recorded) != len(reference):
            sys.exit('reference-only record count differs')
        for op, row in zip(recorded, reference):
            require_anger_reference(op, row)
            if "expect_observation" in op and row.get("state") != op["expect_observation"]:
                sys.exit(f"Java observation prerequisite failed: {op}: {row}")
            if 'expect_result' in op and row.get('results') != [[True, op['expect_result']]]:
                sys.exit(f'Java fixture guard failed: {op}: {row.get("results")}')
            if 'expect_callback' in op and row.get('results') != [op['expect_callback']]:
                sys.exit(f'Java callback guard failed: {op}: {row.get("results")}')
            if 'expect_map' in op and row.get('messages') != [[[op['expect_map'], [None, False, False, False, False, False]]]]:
                sys.exit(f'Java map prerequisite failed: {op}: {row}')
        print(f'Java reference: {len(reference)} records, fixture guards passed')
        return

    if args.binary:
        binary = args.binary.resolve(strict=True)
    else:
        run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'], 'compile-rust', cwd=lab.ROOT)
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True))
        binary = Path(metadata['target_directory']) / 'fast' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    rust_dir = build / 'rust'
    shutil.rmtree(rust_dir, ignore_errors=True)
    rust_dir.mkdir()
    install_datapack_fixtures(rust_dir / 'saves/harness', predicates, functions, jars['server'], advancements, loot_tables, recipes=recipes)
    if any(op['op'] == 'crafter_max_map_fixture' for op in script):
        source = vanilla_dir / 'universe/harness/data/minecraft/maps/31.dat'
        target = rust_dir / 'saves/harness/data/minecraft/maps/31.dat'
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    # The oracle's Services resolve no profile (/fetchprofile fails on the next tick): the game gets no services
    # either, a discovery address that refuses at once, rather than Mojang's over the network.
    env = lab.game_env(HORIZON_PLAYER_NAME="Player", HORIZON_COMMAND_SCRIPT=script_path,
                       HORIZON_COMMAND_OUTPUT=build / 'rust.jsonl', HORIZON_API_DISCOVERY='http://127.0.0.1:9/')
    if args.loaded_fixture:
        env['HORIZON_HARNESS_LOADED_FIXTURE'] = '1'
        env['HORIZON_HARNESS_CLIENT_CONFIG'] = str(build / 'rust-client-config.json')
    run([binary, '--resources', lab.RESOURCES, '--world', 'harness', '--seed', SEED, '--radius', 2 if args.loaded_fixture else 4], 'rust', game=True, cwd=rust_dir, env=env)

    if args.loaded_fixture:
        for name in ['client_config.json', 'rust-client-config.json']:
            config = json.loads((build / name).read_text())
            if config.get('view_distance') != 2 or config.get('simulation_distance') != 2:
                sys.exit(f'actual login distance prerequisite failed: {name}: {config}')
        java = json.loads((build / 'client_config.json').read_text())
        if java.get('server_view') != 2 or java.get('server_simulation') != 2 or not java.get('client_loaded'):
            sys.exit(f'Java server/client-loaded prerequisite failed: {java}')
        for name in ['java.log', 'rust.log']:
            waits = re.findall(r'harness paced wait: (\d+) ticks, (\d+) ms', (build / name).read_text())
            if len(waits) != 2 or any(int(n) != ticks or not ticks * 50 <= int(ms) <= upper
                    for (n, ms), ticks, upper in zip(waits, [40, 120], [4000, 10000])):
                sys.exit(f'bounded paced-wait prerequisite failed: {name}: {waits}')
        native = json.loads((build / 'rust-client-config.json').read_text())
        if native.get('render_setting') != 2 or native.get('simulation_setting') != 2 or not native.get('joined'):
            sys.exit(f'native settings/join prerequisite failed: {native}')
    vanilla = [json.loads(l) for l in (build / 'vanilla.jsonl').open(encoding='utf-8')]
    rust = [json.loads(l) for l in (build / 'rust.jsonl').open(encoding='utf-8')]
    if len(vanilla) != len(rust):
        sys.exit(f'record count differs: vanilla {len(vanilla)} rust {len(rust)}')
    recorded_ops = [op for op in script if op['op'] in ('falling', 'destroy_progress', 'weather_state', 'chunk_ready', 'server_distances', 'map_state', 'frame_map_state', 'light', 'player_state', 'player_loop', 'mob_anger', 'menu_state') or
                    (op['op'] == 'cmd' and op.get('record', True)) or
                    (op['op'] == 'tick' and op.get('record'))]
    if len(recorded_ops) != len(vanilla):
        sys.exit(f'script record count differs: expected {len(recorded_ops)}, got {len(vanilla)}')
    for op, reference in zip(recorded_ops, vanilla):
        require_anger_reference(op, reference)
        if "expect_observation" in op and reference.get("state") != op["expect_observation"]:
            sys.exit(f"Java observation prerequisite failed: {op}: {reference}")
        if 'expect_result' in op and reference.get('results') != [[True, op['expect_result']]]:
            sys.exit(f'Java fixture control failed: {op["cmd"]}: {reference.get("results")}')
        if 'expect_values' in op and reference.get('results') != [[True, n] for n in op['expect_values']]:
            sys.exit(f'Java readiness/distance prerequisite failed: {op}: {reference}')
        if 'expect_callback' in op and reference.get('results') != [op['expect_callback']]:
            sys.exit(f'Java loaded contract failed: {op}: {reference}')
        if 'expect_callback' in op and reference.get('results') != [op['expect_callback']]:
            sys.exit(f'Java callback guard failed: {op}: {reference.get("results")}')
        if 'expect_map' in op and reference.get('messages') != [[[op['expect_map'], [None, False, False, False, False, False]]]]:
            sys.exit(f'Java map fixture control failed: {op}: {reference}')
        if 'expect_state' in op and reference.get('results') != [[True, v] for v in op['expect_state']]:
            sys.exit(f'Java weather guard failed: {op["label"]}: {reference.get("results")}')
        if 'expect_light' in op and reference.get('results') != [[True, n] for n in op['expect_light']]:
            sys.exit(f'Java light prerequisite failed: {op}: {reference}')
    commands_by_group = []
    index = 0
    for name, lines in groups:
        n = sum(1 for op in lines if op['op'] in ('falling', 'destroy_progress', 'weather_state', 'chunk_ready', 'server_distances', 'map_state', 'frame_map_state', 'light', 'player_state', 'player_loop', 'mob_anger', 'menu_state') or (op['op'] == 'cmd' and op.get('record', True)) or (op['op'] == 'tick' and op.get('record')))
        commands_by_group.append((name, index, index + n))
        index += n
    mismatches = []
    failed_positions = set()
    for position, (v, r) in enumerate(zip(vanilla, rust)):
        assert v['cmd'] == r['cmd'], (v['cmd'], r['cmd'])
        kind = compare(v, r, recorded_ops[position].get('comparison'))
        if kind:
            failed_positions.add(position)
            mismatches.append({'kind': kind, 'cmd': v['cmd'], 'vanilla': v, 'rust': r})
    (build / 'mismatches.jsonl').write_text(''.join(json.dumps(m, ensure_ascii=False) + '\n' for m in mismatches), encoding='utf-8')
    summary = []
    for name, lo, hi in commands_by_group:
        # By position: two groups that run the same probe command (the orb
        # groups' `data get` lines) must not be charged each other's failures.
        failed = sum(1 for position in failed_positions if lo <= position < hi)
        summary.append({'group': name, 'commands': hi - lo, 'mismatches': failed})
        print(f'{name}: {hi - lo - failed}/{hi - lo} commands match', flush=True)
    for m in mismatches[:args.limit]:
        print(m['kind'], repr(m['cmd']))
        print('  vanilla', render(m['vanilla'])[:400])
        print('  rust   ', render(m['rust'])[:400])
    total = len(vanilla)
    print(f'total: {total - len(mismatches)}/{total} commands match', flush=True)
    report = {'schema': 1, 'minecraft': lock['minecraft']['version'], 'platform': platform.platform(), 'seed': SEED,
              'commands': total, 'mismatches': len(mismatches), 'groups': summary,
              'comparison_overrides': [
                  {'cmd': op['cmd'], 'comparison': op['comparison'],
                   'vanilla_results': v['results'], 'rust_results': r['results']}
                  for op, v, r in zip(recorded_ops, vanilla, rust) if op.get('comparison')],
              'scope': 'Feedback and failure messages (text and resolved style), chat lines, and CommandResultCallback results of commands run by the single-player owner of an integrated server; world, entity and item effects through probe commands. Explicit positive_result operations compare presence only; their unmodified raw counts remain in the records and comparison_overrides.'}
    for override in report['comparison_overrides']:
        print(f'{override["comparison"]}: {override["cmd"]}: '
              f'Java {override["vanilla_results"]}, Horizon {override["rust_results"]}', flush=True)
    lab.write_json(report_path, report)
    if mismatches and not args.allow_gaps:
        sys.exit(f'{len(mismatches)} command execution mismatches; see {build}')
    print(f'Command execution parity report: {report_path}')


def require_anger_reference(op, record):
    """Validate independent live-clock expectations, retaining raw absolute ends."""
    expected = op.get('expect_anger')
    if expected is None:
        return
    state = record.get('state', {})
    fields = ('uuid', 'kind', 'index')
    if (any(state.get(key) != expected[key] for key in fields) or
            state.get('serializer') != 2 or type(state.get('value')) is not int or
            type(state.get('game_time')) is not int):
        sys.exit(f'Java anger identity/type guard failed: {op}: {record}')
    value = -1 if expected['delta'] is None else state['game_time'] + expected['delta']
    if state['value'] != value:
        sys.exit(f'Java anger live-clock guard failed: {op}: {record}')


MASKS = [
    (re.compile(r'debug-trace-\d{4}-\d\d-\d\d_\d\d\.\d\d\.\d\d\.txt'), 'debug-trace-<time>.txt'),
    (re.compile(r'after [0-9.]+ second\(s\) and \d+ tick\(s\) \([0-9.,NaInfity]+ tick\(s\) per second\)'), 'after <time>'),
    (re.compile(r'Average time per tick: [0-9.]+ms'), 'Average time per tick: <ms>'),
    (re.compile(r'P50: [0-9.]+ms P95: [0-9.]+ms P99: [0-9.]+ms'), 'P50: <ms> P95: <ms> P99: <ms>'),
    (re.compile(r'debug/client-[0-9_.-]+\.(jfr|json)'), 'debug/client-<time>'),
]


GAMERULES = re.compile(r'/gamerule \(([^)]*)\)')


def masked(record):
    text = json.dumps(record['messages'], ensure_ascii=False)
    # GameRuleCommand registers the rules in identity-hash order, different
    # in every JVM: compare /help's gamerule alternatives as a set.
    text = GAMERULES.sub(lambda m: '/gamerule (' + '|'.join(sorted(m.group(1).split('|'))) + ')', text)
    for pattern, replacement in MASKS:
        text = pattern.sub(replacement, text)
    return text


def positive_result(record):
    """A counted operation's presence, retaining its unmodified raw record."""
    results = record.get('results')
    if not isinstance(results, list) or len(results) != 1 or not isinstance(results[0], list) or len(results[0]) != 2:
        return None
    accepted, count = results[0]
    if type(accepted) is not bool or type(count) is not int or count < 0 or not accepted and count != 0:
        return None
    return count > 0


def compare(v, r, comparison=None):
    # Explicit per-operation scope for random counts (for example explosion
    # fire). All raw feedback/results remain in the JSON and report; every
    # other command still compares exact feedback, styles and results.
    if comparison == 'positive_result':
        left, right = positive_result(v), positive_result(r)
        return None if left is not None and right is not None and left == right else 'positive_result'
    if comparison is not None:
        raise ValueError(f'unknown command comparison: {comparison}')
    if v.get("state") != r.get("state"):
        return "state"
    if masked(v) != masked(r):
        return 'messages'
    if v['results'] != r['results']:
        if v['cmd'].startswith('/debug stop'):
            return None
        return 'results'
    return None


def render(record):
    lines = [''.join(span[0] for span in m) for m in record['messages']]
    return json.dumps({'messages': lines, 'styles': [[span[1] for span in m] for m in record['messages']], 'results': record['results']}, ensure_ascii=False)


if __name__ == '__main__':
    main()
