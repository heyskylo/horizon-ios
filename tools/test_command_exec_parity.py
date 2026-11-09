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
    from . import seed_agent
except ImportError:
    import lab
    import seed_agent
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
    parser.add_argument('--function-admission-fixture', action='store_true', help='compile initial Java functions at its actual GAMEMASTER permission, matching reload and native')
    parser.add_argument('--raid-village-fixture', action='store_true', help='match the existing development command trees in the isolated Java/native fixture processes')
    parser.add_argument('--raid-release-command-diagnostic', action='store_true', help='retain the unmatched native release-tree admission prerequisite diagnostic')
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
    parser.add_argument('--vehicle-kill-diagnostic', action='store_true',
                        help='retain random drop UUID/motion and whole vehicle bodies outside the explicit-content gate')
    parser.add_argument('--only', help='run only corpus groups whose name contains this')
    parser.add_argument('--vehicle-container-diagnostic', action='store_true',
                        help='retain all vehicle Tags-list and whole-body observations outside the saved-field gate')
    parser.add_argument('--array-storage-diagnostic', action='store_true',
                        help='run all numeric-array edit cases, including the known wildcard/string storage side effects')
    parser.add_argument('--sulfur-power-load-diagnostic', action='store_true',
                        help='retain the immediate-load powered sulfur cube H249 diagnostic')
    parser.add_argument('--merchant-gate-fixture', action='store_true')
    parser.add_argument('--binary', type=Path, help='use an already built Horizon game instead of building it')
    parser.add_argument('--trader-llama-discard-fixture', action='store_true',
                        help='an AI trader llama is gone after the tick its delay runs out (H182)')
    parser.add_argument('--hurt-cooldown-incoming-fixture', action='store_true',
                        help='a mob hurt cooldown compares the incoming damage before its reductions (H248)')
    parser.add_argument('--level-world-border-fixture', action='store_true',
                        help='each builtin level keeps its own world border (H260)')
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
    parser.add_argument("--stand-entity-collision-fixture", action="store_true")
    parser.add_argument("--reference-only", action="store_true")
    parser.add_argument("--boat-leash-view-fixture", action="store_true")
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
    parser.add_argument("--selector-bounds-fixture", action="store_true")
    parser.add_argument('--spectral-duration-fixture', action='store_true')
    parser.add_argument('--item-health-fixture', action='store_true')
    parser.add_argument("--macro-source-type-fixture", action="store_true")
    parser.add_argument("--stand-fall-fixture", action="store_true")
    parser.add_argument('--orb-uuid-fixture', action='store_true')
    parser.add_argument('--baby-modifier-fixture', action='store_true', help='/attribute add and remove of a zombie\'s transient minecraft:baby modifier')
    parser.add_argument('--baby-modifier-raw-diagnostic', action='store_true', help='add the whole attributes lists and entity data')
    parser.add_argument('--mob-attribute-save-fixture', action='store_true', help='which attribute instances a pig, allay and armor stand save before and after a tick')
    parser.add_argument('--mob-attribute-save-raw-diagnostic', action='store_true', help='add the whole attributes lists and entity data')
    parser.add_argument('--mob-inventory-merge-fixture', action='store_true', help='Piglin and Villager saved inventories loaded as SimpleContainer.addItem does')
    parser.add_argument('--mob-inventory-merge-raw-diagnostic', action='store_true', help='add each mob\'s whole entity data')
    parser.add_argument('--summon-passengers-fixture', action='store_true', help='summon trees with Passengers entries vanilla cannot load')
    parser.add_argument('--summon-passengers-raw-diagnostic', action='store_true', help='add each parent\'s whole entity data')
    parser.add_argument('--summon-uuid-fixture', action='store_true', help='summon trees whose UUIDs conflict with live entities or within the tree')
    parser.add_argument('--summon-uuid-internal-diagnostic', action='store_true', help='add the trees with duplicate UUIDs inside them (#11807)')
    parser.add_argument('--summon-uuid-raw-diagnostic', action='store_true', help='add each member\'s whole entity data')
    parser.add_argument('--mob-absorption-max-fixture', action='store_true', help='a mob\'s max_absorption with Absorption added, upgraded, cleared and run out')
    parser.add_argument('--mob-absorption-max-ops-fixture', action='store_true', help='the Absorption modifier among other max_absorption operations and the range clamp')
    parser.add_argument('--mob-absorption-max-raw-diagnostic', action='store_true', help='add each mob\'s whole entity data')
    parser.add_argument('--mob-health-boost-fixture', action='store_true', help='Health Boost on a mob: heal to its maximum, clamp on removal')
    parser.add_argument('--mob-health-boost-raw-diagnostic', action='store_true', help='add each mob\'s whole entity data')
    parser.add_argument('--sulfur-body-attributes-fixture', action='store_true', help='a sulfur cube\'s body archetype attribute modifiers through /attribute')
    parser.add_argument('--sulfur-body-replace-fixture', action='store_true', help='a sulfur cube\'s body replaced and removed through /data')
    parser.add_argument('--sulfur-body-attributes-raw-diagnostic', action='store_true', help='add the whole attributes lists and entity data')
    parser.add_argument('--mob-absorption-fixture', action='store_true', help='a mob\'s Absorption effect once the absorption is used up')
    parser.add_argument('--mob-absorption-infinite-fixture', action='store_true', help='infinite Absorption used up, kept, and beside another infinite effect')
    parser.add_argument('--mob-absorption-raw-diagnostic', action='store_true', help='add each mob\'s whole entity data')
    parser.add_argument("--mob-pickup-save-fixture", action="store_true")
    parser.add_argument('--merchant-crafted-fixture', action='store_true')
    parser.add_argument('--mob-name-presentation-diagnostic', action='store_true',
                        help='retain all 312 mob-name records, including ten separate name/saved-text presentation observations')
    parser.add_argument('--potion-effect-icon-fixture', action='store_true')
    parser.add_argument('--unsynced-randoms', action='store_true',
                        help='leave out the sync_randoms steps: the control in which their fields differ run to run')
    parser.add_argument('--groups-file', type=Path,
                        help='run these generated [name, ops] groups after the common setup instead of the corpus')
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
        groups = [g for g in groups if g[0] not in corpus_module.ISOLATED_BOAT_TREE]
    if not args.only:
        groups = [g for g in groups if g[0] not in corpus_module.ISOLATED_EFFECTIVE_PRESENCE]
    if not args.only:
        groups = [g for g in groups if g[0] not in corpus_module.ISOLATED_FUNCTION_FEEDBACK]
    if not args.only:
        groups = [g for g in groups if g[0] not in corpus_module.ISOLATED_JUKEBOX_CLOCK]
        groups = [g for g in groups if g[0] not in corpus_module.ISOLATED_GUEST_CLOSE]
        groups = [g for g in groups if g[0] not in corpus_module.ISOLATED_SHULKER_LOCK]
    if args.vehicle_container_diagnostic:
        try:
            from .vehicle_container_corpus import vehicle_containers
        except ImportError:
            from vehicle_container_corpus import vehicle_containers
        groups = [(name, vehicle_containers(corpus_module.cmd, diagnostic=True)
                   if name == 'vehicle container saved fields' else ops) for name, ops in groups]
    if args.macro_source_type_fixture or args.only == "macro source type error":
        groups = [(name, ops) for name, ops in groups if name == "macro source type error"]
    elif not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_MACRO_TYPE]
    if args.stand_fall_fixture or args.only == "stand saved fall distance":
        groups = [(name, ops) for name, ops in groups if name == "stand saved fall distance"]
    elif not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_STAND_FALL]
    if args.orb_uuid_fixture or args.only == 'orb saved UUID':
        groups = [(name, ops) for name, ops in groups if name == 'orb saved UUID']
    elif not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_ORB_UUID]
    if args.mob_pickup_save_fixture or args.only == "mob saved pickup loot":
        groups = [(name, ops) for name, ops in groups if name == "mob saved pickup loot"]
    elif not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_PICKUP_SAVE]
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
    isolated_recipes = corpus_module.ISOLATED_RECIPES | corpus_module.ISOLATED_MAP_CRAFTING
    if not args.only:
        groups = [(name, ops) for name, ops in groups if name not in isolated_recipes]
    if args.loaded_fixture:
        groups = [('setup', corpus_module.group_setup()),
                  ('loaded ticket removal', corpus_module.loaded_ticket_removal(args.loaded_block_diagnostic))]
    if args.spectral_duration_fixture or args.only == 'spectral saved duration':
        groups = [(name, ops) for name, ops in corpus_module.groups() if name == 'spectral saved duration']
    elif not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_SPECTRAL_DURATION]
    if args.item_health_fixture or args.only == 'item Health short load':
        groups = [(name, ops) for name, ops in corpus_module.groups() if name == 'item Health short load']
    elif not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_ITEM_HEALTH]
    if args.potion_effect_icon_fixture or args.only == 'potion custom effect icons':
        groups = [(name, ops) for name, ops in corpus_module.groups()
                  if name == 'potion custom effect icons']
    elif not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_POTION_ICONS]
    if not args.only:
        groups = [(name, ops) for name, ops in groups if name not in corpus_module.ISOLATED_SEED_SYNC]
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
    if args.hurt_cooldown_incoming_fixture:
        try:
            from .hurt_cooldown_incoming_corpus import hurt_cooldown_incoming
        except ImportError:
            from hurt_cooldown_incoming_corpus import hurt_cooldown_incoming
        groups = [('setup', corpus_module.group_setup()), ('hurt cooldown incoming', hurt_cooldown_incoming())]
    if args.level_world_border_fixture:
        try:
            from .level_world_border_corpus import level_world_border
        except ImportError:
            from level_world_border_corpus import level_world_border
        groups = [('setup', corpus_module.group_setup()), ('level world border', level_world_border())]
    if args.trader_llama_discard_fixture:
        try:
            from .trader_llama_discard_corpus import trader_llama_discard
        except ImportError:
            from trader_llama_discard_corpus import trader_llama_discard
        groups = [('setup', corpus_module.group_setup()), ('trader llama discard', trader_llama_discard())]
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
    if args.stand_entity_collision_fixture:
        from stand_entity_collision_corpus import stand_entity_collisions
        groups = [('stand entity collisions', stand_entity_collisions())]
    if args.pufferfish_mob_fixture:
        from pufferfish_mob_contact_corpus import mob_contacts, overlapping_cod_control
        groups = [("pufferfish mob contact", overlapping_cod_control() if args.pufferfish_mob_overlap_control
                   else mob_contacts(args.pufferfish_mob_duration_diagnostic))]
    if args.pufferfish_contact_fixture:
        from pufferfish_contact_corpus import contact_fixture
        groups = [("pufferfish player contact", contact_fixture(args.pufferfish_body_diagnostic))]
    if args.only == 'shulker external lock drop':
        groups = [g for g in groups if g[0] == args.only]
    elif args.only:
        groups = [g for g in groups if args.only in g[0] or g[0] == 'setup']
    if args.only in corpus_module.ISOLATED_BOAT_TREE:
        groups = [g for g in groups if g[0] == args.only]
    if any(n in corpus_module.ISOLATED_BOAT_TREE for n,_ in groups) and len(groups) != 1:
        parser.error('boat tree fixture requires exact --only group')
    if args.only in corpus_module.ISOLATED_EFFECTIVE_PRESENCE:
        groups = [g for g in groups if g[0] == args.only]
    if any(n in corpus_module.ISOLATED_EFFECTIVE_PRESENCE for n, _ in groups) and len(groups) != 1:
        parser.error('effective presence fixture requires exact --only group')
    if args.only in corpus_module.ISOLATED_FUNCTION_FEEDBACK:
        groups = [g for g in groups if g[0] == args.only]
    if args.only in corpus_module.ISOLATED_JUKEBOX_CLOCK:
        groups = [g for g in groups if g[0] == args.only]
    if args.vehicle_kill_diagnostic:
        from vehicle_kill_corpus import vehicle_kill_contents
        groups = [('setup', corpus_module.group_setup()), ('vehicle kill contents', vehicle_kill_contents(corpus_module.cmd, True))]
    if args.slot_codec_diagnostic:
        groups.append(('slot source codec diagnostic', [corpus_module.cmd(
            '/item fill block 4 200 4 {type:"minecraft:slot_range",slots:"container.99"} with diamond')]))
    if any(name in corpus_module.ISOLATED_JUKEBOX_CLOCK for name, _ in groups) and len(groups) != 1:
        parser.error('jukebox clock fixture requires --only with its exact group')
    if any(name in corpus_module.ISOLATED_SHULKER_LOCK for name, _ in groups) and len(groups) != 1:
        parser.error('shulker lock fixture requires --only with its exact group')
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
    if args.selector_bounds_fixture:
        from selector_bounds_corpus import selector_bounds
        groups = [("setup", corpus_module.group_setup()), ("selector entity bounds", selector_bounds())]
    if args.baby_modifier_fixture:
        from baby_modifier_corpus import baby_modifier
        groups = [('setup', corpus_module.group_setup()), ('baby modifier mutation', baby_modifier(args.baby_modifier_raw_diagnostic))]
    if args.mob_attribute_save_fixture:
        from mob_attribute_save_corpus import mob_attribute_save
        groups = [('setup', corpus_module.group_setup()), ('mob attribute save', mob_attribute_save(args.mob_attribute_save_raw_diagnostic))]
    if args.mob_inventory_merge_fixture:
        from mob_inventory_merge_corpus import mob_inventory_merge
        groups = [('setup', corpus_module.group_setup()), ('mob inventory merge', mob_inventory_merge(args.mob_inventory_merge_raw_diagnostic))]
    if args.summon_passengers_fixture:
        from summon_passengers_corpus import summon_passengers
        groups = [('setup', corpus_module.group_setup()), ('summon invalid passengers', summon_passengers(args.summon_passengers_raw_diagnostic))]
    if args.summon_uuid_fixture:
        from summon_uuid_corpus import summon_uuid
        groups = [('setup', corpus_module.group_setup()), ('summon UUID conflicts', summon_uuid(args.summon_uuid_raw_diagnostic, True))]
    if args.mob_absorption_max_fixture:
        from mob_absorption_max_corpus import mob_absorption_max
        groups = [('setup', corpus_module.group_setup()), ('mob absorption maximum', mob_absorption_max(args.mob_absorption_max_raw_diagnostic))]
    if args.mob_absorption_max_ops_fixture:
        from mob_absorption_max_corpus import mob_absorption_max_ops
        groups = [('setup', corpus_module.group_setup()), ('mob absorption maximum operations', mob_absorption_max_ops())]
    if args.mob_health_boost_fixture:
        from mob_health_boost_corpus import mob_health_boost
        groups = [('setup', corpus_module.group_setup()), ('mob health boost', mob_health_boost(args.mob_health_boost_raw_diagnostic))]
    if args.sulfur_body_attributes_fixture:
        from sulfur_body_attributes_corpus import sulfur_body_attributes
        groups = [('setup', corpus_module.group_setup()), ('sulfur body attributes', sulfur_body_attributes(args.sulfur_body_attributes_raw_diagnostic))]
    if args.sulfur_body_replace_fixture:
        from sulfur_body_attributes_corpus import sulfur_body_replace
        groups = [('setup', corpus_module.group_setup()), ('sulfur body replace', sulfur_body_replace())]
    if args.mob_absorption_fixture:
        from mob_absorption_corpus import mob_absorption
        groups = [('setup', corpus_module.group_setup()), ('mob depleted absorption', mob_absorption(args.mob_absorption_raw_diagnostic))]
    if args.mob_absorption_infinite_fixture:
        from mob_absorption_corpus import mob_absorption_infinite
        groups = [('setup', corpus_module.group_setup()), ('mob infinite absorption', mob_absorption_infinite())]
    if args.boat_leash_view_fixture:
        from boat_leash_view_corpus import cases as boat_leash_views
        groups = [('setup', corpus_module.group_setup()),
                  ('boat live leash view', boat_leash_views(corpus_module.cmd, corpus_module.setup))]
    if args.merchant_crafted_fixture:
        from merchant_crafted_corpus import cases
        groups = [('merchant crafted statistics', cases())]
    if args.mob_name_presentation_diagnostic:
        from mob_name_predicate_corpus import cases as mob_name_predicates
        groups = [('setup', corpus_module.group_setup()),
                  ('mob custom name components', mob_name_predicates(
                      corpus_module.cmd, corpus_module.setup, presentation_diagnostic=True))]
    if args.merchant_gate_fixture:
        from merchant_gate_corpus import cases
        groups = [('setup', [corpus_module.cmd('/tick freeze', False), *corpus_module.group_setup()]), ('merchant reputation AI gate', cases())]
    if args.groups_file:
        # Generated scripts (tools/parity_fuzz.py): the common setup, then the file's [name, ops] groups only.
        groups = [('setup', corpus_module.group_setup()),
                  *[(name, ops) for name, ops in json.loads(args.groups_file.read_text(encoding='utf-8'))]]
    script = []
    for _, lines in groups:
        script.extend(op for op in lines if not (args.unsynced_randoms and op['op'] == 'sync_randoms'))
    # docs/parity-workflow.md "Randoms that repeat": both engines' unseeded randoms repeat for a script that syncs them.
    synced = any(op['op'] == 'sync_randoms' for op in script)
    script_path = build / 'script.jsonl'
    script_path.write_text(''.join(json.dumps({k: v for k, v in op.items() if k != 'expect_result'},
                                            ensure_ascii=False) + '\n' for op in script), encoding='utf-8')
    predicates = corpus_module.predicate_selector_fixtures([name for name, _ in groups])
    functions = corpus_module.function_return_fixtures([name for name, _ in groups])
    recipes = corpus_module.crafting_priority_recipes([name for name, _ in groups])
    if any(name == 'transmute result patches' for name, _ in groups):
        try:
            from .transmute_patch_corpus import fixture_recipes
        except ImportError:
            from transmute_patch_corpus import fixture_recipes
        recipes.update(fixture_recipes())
    advancements = corpus_module.advancement_reward_fixtures([name for name, _ in groups])
    loot_advancements, loot_tables = corpus_module.advancement_loot_fixtures([name for name, _ in groups])
    advancements.update(loot_advancements)
    if os.environ.get('HORIZON_FISHING_FALLBACK_FIXTURE') == '1':
        # No random_sequence: FishingHook.retrieve must use the level stream.
        loot_tables['minecraft:gameplay/fishing'] = {
            'type': 'minecraft:fishing', 'pools': [{'rolls': 1, 'entries': [
                {'type': 'minecraft:item', 'name': 'minecraft:cod', 'weight': 1},
                {'type': 'minecraft:item', 'name': 'minecraft:salmon', 'weight': 1},
            ]}]}

    if args.pufferfish_contact_fixture:
        from pufferfish_contact_corpus import advancements as contact_advancements
        advancements.update(contact_advancements())

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-cp', str(jars['server']) + os.pathsep + dependency_path, '-d', classes, oracle], 'compile-java')
    vanilla_dir = build / 'vanilla'
    shutil.rmtree(vanilla_dir, ignore_errors=True)
    vanilla_dir.mkdir()
    install_datapack_fixtures(vanilla_dir / 'universe/harness', predicates, functions, jars['server'], advancements, loot_tables, recipes=recipes)
    java_options = ['-DMC_DEBUG_ENABLED=true', '-DMC_DEBUG_DEV_COMMANDS=true'] if args.raid_village_fixture else []
    if args.merchant_gate_fixture:
        java_options.append('-Dhorizon.merchantFixture=true')
    if args.merchant_crafted_fixture:
        java_options.append('-Dhorizon.merchantCraftedFixture=true')
    if args.orb_water_fixture:
        (build / 'orb-water-reference.json').unlink(missing_ok=True)
        direct_input = build / 'orb-water-inputs.json'
        direct_input.write_text(json.dumps(arithmetic_inputs()), encoding='utf-8')
        java_options = ['-Dhorizon.orbWaterArithmetic=' + str(direct_input)]
    if args.function_admission_fixture:
        java_options.append('-Dhorizon.functionAdmissionFixture=true')
    if synced:
        java_options.append(seed_agent.java_option(build, lock['minecraft']['java_major']))
    run(['java', '-Xmx3G', '-Duser.timezone=UTC', *(['-Dhorizon.loadedFixture=true'] if args.loaded_fixture else []), *java_options, '-cp', os.pathsep.join([str(classes), str(jars['server']), dependency_path]),
         'dev.horizonlab.parity.VanillaCommandExecOracle', vanilla_dir / 'universe', script_path, build / 'vanilla.jsonl', SEED], 'java', cwd=vanilla_dir)

    if any(op['op'] == 'raid_state' for op in script):
        from raid_village_corpus import validate_java_inputs
        validate_java_inputs(build, script)

    if args.orb_water_fixture:
        direct = json.loads((build / 'orb-water-reference.json').read_text())
        if ([r.get('input') for r in direct] != arithmetic_inputs() or
                any(len(r.get('bits', [])) != 3 or
                    any(re.fullmatch(r'[0-9a-f]{1,16}', b) is None for b in r['bits']) for r in direct)):
            sys.exit('Java direct underwater reference prerequisite failed')
    if args.reference_only:
        reference = [json.loads(line) for line in (build / 'vanilla.jsonl').read_text().splitlines()]
        recorded = [op for op in script if op['op'] in ('player_state', 'player_loop', 'map_state', 'frame_map_state', 'mob_anger', 'projectile_saved_type', 'raid_state', 'menu_state', 'craft_menu_state', 'map_files', 'merchant_state', 'merchant_saved_stats') or
                    (op['op'] == 'cmd' and op.get('record', True)) or
                    (op['op'] == 'tick' and op.get('record'))]
        if len(recorded) != len(reference):
            sys.exit('reference-only record count differs')
        for op, row in zip(recorded, reference):
            if "expect_merchant" in op and row.get("state") != op["expect_merchant"]:
                sys.exit(f"Java merchant admission failed: {op}: {row}")
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
    if synced:
        env.update(seed_agent.ENV)
    if args.raid_village_fixture and not args.raid_release_command_diagnostic:
        env['HORIZON_DEV_COMMANDS'] = '1'
    elif args.raid_release_command_diagnostic:
        env.pop('HORIZON_DEV_COMMANDS', None)
    if args.loaded_fixture:
        env['HORIZON_HARNESS_LOADED_FIXTURE'] = '1'
        env['HORIZON_HARNESS_CLIENT_CONFIG'] = str(build / 'rust-client-config.json')
    run([binary, '--resources', lab.RESOURCES, '--world', 'harness', '--seed', SEED, '--radius', 2 if args.loaded_fixture else 4], 'rust', game=True, cwd=rust_dir, env=env)

    if any(op['op'] == 'raid_state' for op in script):
        from raid_village_corpus import validate_native_inputs
        if args.raid_release_command_diagnostic:
            print('REJECTED admission prerequisite diagnostic: native release tree vs Java development tree; not H162 loss evidence')
        else:
            validate_native_inputs(build, script)

    if args.loaded_fixture:
        for name in ['client_config.json', 'rust-client-config.json']:
            config = json.loads((build / name).read_text())
            if config.get('view_distance') != 2 or config.get('simulation_distance') != 2:
                sys.exit(f'actual login distance prerequisite failed: {name}: {config}')
        java = json.loads((build / 'client_config.json').read_text())
        if java.get('server_view') != 2 or java.get('server_simulation') != 2 or not java.get('client_loaded'):
            sys.exit(f'Java server/client-loaded prerequisite failed: {java}')
        if not args.merchant_gate_fixture:
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
    recorded_ops = [op for op in script if op['op'] in ('falling', 'destroy_progress', 'weather_state', 'chunk_ready', 'server_distances', 'map_state', 'frame_map_state', 'light', 'player_state', 'player_loop', 'mob_anger', 'projectile_saved_type', 'raid_state', 'menu_state', 'craft_menu_state', 'map_files', 'merchant_state', 'merchant_saved_stats') or
                    (op['op'] == 'cmd' and op.get('record', True)) or
                    (op['op'] == 'tick' and op.get('record'))]
    if len(recorded_ops) != len(vanilla):
        sys.exit(f'script record count differs: expected {len(recorded_ops)}, got {len(vanilla)}')
    for op, reference in zip(recorded_ops, vanilla):
        if "expect_merchant" in op and reference.get("state") != op["expect_merchant"]:
            sys.exit(f"Java merchant admission failed: {op}: {reference}")
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
        n = sum(1 for op in lines if op['op'] in ('falling', 'destroy_progress', 'weather_state', 'chunk_ready', 'server_distances', 'map_state', 'frame_map_state', 'light', 'player_state', 'player_loop', 'mob_anger', 'projectile_saved_type', 'raid_state', 'menu_state', 'craft_menu_state', 'map_files', 'merchant_state', 'merchant_saved_stats') or (op['op'] == 'cmd' and op.get('record', True)) or (op['op'] == 'tick' and op.get('record')))
        commands_by_group.append((name, index, index + n))
        index += n
    mismatches = []
    failed_positions = set()
    for position, (v, r) in enumerate(zip(vanilla, rust)):
        assert v['cmd'] == r['cmd'], (v['cmd'], r['cmd'])
        kind = ('strict_record' if v != r else None) if args.merchant_gate_fixture or args.merchant_crafted_fixture else compare(v, r, recorded_ops[position].get('comparison'))
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
