#!/usr/bin/env python3
"""Compare the furnace block entity's tick with vanilla's AbstractFurnaceBlockEntity.

The Java oracle loads the vanilla data pack's real RecipeManager, puts a
furnace, blast furnace or smoker in a block-update level and calls the real
AbstractFurnaceBlockEntity.serverTick. The Rust probe runs FurnaceCooking::tick
on the chunk's Furnace. Cases cover the burn table (every cooking_fuel item),
the three kinds' 200/100/100 cook times, fuel stacks and the LIT state, the
RecipesUsed experience, full output slots, timer decay, and an input changed
mid-cook. `--no-change` leaves out the mid-cook family, reproducing the corpus
before it.
"""
import json
import sys
from pathlib import Path
try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab
    import oracle_harness as harness

KINDS = ['furnace', 'blast_furnace', 'smoker']
KIND_RECIPE = {'furnace': 'minecraft:smelting', 'blast_furnace': 'minecraft:blasting',
               'smoker': 'minecraft:smoking'}


def stack(item, count=1):
    return '{id:"%s",count:%d}' % (item, count)


def fcase(kind, ticks, inv, fuel, out='-', timer=0, total=0, lit=0, littotal=0,
          speed=1.0, change_at=-1, new='-'):
    return 'furnace %s %d %s %s %s %d %d %d %d %s %d %s' % (
        kind, ticks, inv, fuel, out, timer, total, lit, littotal, speed, change_at, new)


def recipes(resources):
    out = {}
    for kind, recipe_type in KIND_RECIPE.items():
        found = []
        for path in sorted((resources / 'data/minecraft/recipe').rglob('*.json')):
            data = json.loads(path.read_text())
            if data.get('type') != recipe_type:
                continue
            ing = data.get('ingredient')
            if isinstance(ing, dict):
                ing = ing.get('item') or ing.get('id')
            result = data.get('result')
            rid = result.get('id') if isinstance(result, dict) else result
            if not ing or not rid:
                continue
            found.append((ing, rid, int(data.get('cookingtime', 200)),
                          float(data.get('experience', 0.0))))
        out[kind] = found
    return out


def make_cases(build):
    resources = lab.LOCAL / 'horizon-client' / '26.3'
    catalog = json.loads((resources / 'items.json').read_text())
    all_recipes = recipes(resources)
    fuels = sorted(item for item, data in catalog.items()
                   if 'minecraft:cooking_fuel' in data)
    coal = stack('minecraft:coal')
    cases = []

    # One recipe per kind, at its cook-time boundaries.
    for kind in KINDS:
        ing, result, cook, _ = all_recipes[kind][0]
        for ticks in (0, 1, cook - 1, cook, cook + 1, 2 * cook):
            cases.append(fcase(kind, ticks, stack(ing), coal, total=cook))
        # XP accumulates per recipe and per item.
        cases.append(fcase(kind, 3 * cook + 1, stack(ing, 3), coal, total=cook))
        cases.append(fcase(kind, 3 * cook + 2, stack(ing, 3), coal, total=cook))

    # Every cooking_fuel item: burn time and speed multiplier after one/two ticks.
    for fuel in fuels:
        cases.append(fcase('furnace', 1, stack('minecraft:iron_ore'), stack(fuel), total=200))
        cases.append(fcase('furnace', 2, stack('minecraft:iron_ore'), stack(fuel), total=200))
    cases.append(fcase('furnace', 1, stack('minecraft:iron_ore'),
                       '{id:"minecraft:coal",count:2}', total=200))
    cases.append(fcase('furnace', 2, stack('minecraft:iron_ore'),
                       '{id:"minecraft:coal",count:2}', total=200))

    # LIT changes at the end of the burn, and fuel is consumed once.
    for ticks in (1599, 1600, 1601, 1602):
        cases.append(fcase('furnace', ticks, stack('minecraft:iron_ore'), coal, total=200))

    # A full output stack cannot take another result; the timer decays.
    cases.append(fcase('furnace', 1, stack('minecraft:iron_ore'),
                       stack('minecraft:coal'), stack('minecraft:iron_ingot', 63), total=200))
    cases.append(fcase('furnace', 200, stack('minecraft:iron_ore'),
                       stack('minecraft:coal'), stack('minecraft:iron_ingot', 63), total=200))
    for ticks in (1, 5, 10):
        cases.append(fcase('furnace', ticks, '-', '-', '-', timer=10, total=200))

    # The mid-cook family: a new input resets total and timer, the same input
    # does not, and an empty input stops the recipe.
    if '--no-change' not in sys.argv:
        for kind in KINDS:
            ing, result, cook, _ = all_recipes[kind][0]
            other = next((r for r in all_recipes[kind] if r[0] != ing), None)
            if not other:
                continue
            new = stack(other[0])
            for ticks in (50, 51, cook, cook + 1):
                cases.append(fcase(kind, ticks, stack(ing), coal, timer=50, total=cook,
                                   change_at=50, new=new))
            for ticks in (1, cook, cook + 1):
                cases.append(fcase(kind, ticks, stack(ing), coal, change_at=0, new=new))
            cases.append(fcase(kind, 51, stack(ing), coal, timer=50, total=cook,
                               change_at=50, new=stack(ing)))
            cases.append(fcase(kind, 51, stack(ing), coal, timer=50, total=cook,
                               change_at=50, new='-'))
        # A change exactly at the end of the run.
        ing, result, cook, _ = all_recipes['furnace'][0]
        cases.append(fcase('furnace', 1, stack(ing), coal, total=cook, change_at=1,
                           new=stack('minecraft:gold_ore')))

    path = build / 'cases.txt'
    path.write_text(f'#resources {resources}\n' + ''.join(c + '\n' for c in cases))
    return path, len(cases)


if __name__ == '__main__':
    harness.differential(
        'furnace-parity', 'VanillaFurnaceOracle', 'furnace_fixture', make_cases,
        'Furnace block entity tick: burn table for every cooking_fuel item, 200/100/100 '
        'cook times, fuel stack and LIT state, RecipesUsed experience, full output and '
        'timer decay, and input changed mid-cook. --no-change leaves out the change family.',
        sources=[lab.ROOT / 'crates/horizon-client-render/src/chunk/block_entity.rs',
                 lab.ROOT / 'crates/horizon-client/src/furnace.rs',
                 lab.ROOT / 'crates/horizon-client/src/crafting.rs'])
