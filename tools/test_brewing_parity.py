#!/usr/bin/env python3
"""Compare the brewing recipes' slot rules, matching and assembled output with vanilla.

The Java oracle loads the vanilla data pack's real `RecipeManager`, takes its
`brewing_input` and `brewing_reagent` property sets and the `BrewingRecipe`s,
checks all three brewing-slot filters and calls the first matching recipe's
`assemble`. The Rust probe calls `brewing::Brewing::is_bottle`/`is_reagent`,
`brewing_fuel` and `Brewing::find`. Cases cover every bundled brewing recipe,
every potion type and potion id against every reagent, candidates from the
whole item registry, and non-matching pairs. Tick cases run
`BrewingStandBlockEntity.serverTick` and `BrewingStand::tick` for 0..8001
ticks: the 400-tick brew time divided by `speed_multiplier` and its ceiling,
blaze powder's 20 fuel uses, fuel loaded with nothing to brew, splash and
lingering outputs, ingredient shrink and mismatch, the brew sound, and the
bottle bits. `--no-tick` leaves the tick corpus out, and `--baseline` keeps
only the exact recipe pairs.
"""
import json
from pathlib import Path
import sys
try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab
    import oracle_harness as harness

POTION_ITEMS = ['potion', 'splash_potion', 'lingering_potion']


def stack(item, potion=None, count=1):
    if potion:
        return '{id:"%s",count:%d,components:{"minecraft:potion_contents":{potion:"%s"}}}' % (
            item, count, potion)
    return '{id:"%s",count:%d}' % (item, count)


def recipes(resources):
    out = []
    for path in sorted((resources / 'data/minecraft/recipe/brewing').glob('*.json')):
        data = json.loads(path.read_text())
        if data.get('type') != 'minecraft:brewing':
            continue
        potion = data['input'].get('potion_contents', {}).get('potions')
        out.append({
            'input': stack(data['input']['item'], potion),
            'reagent': stack(data['reagent']['item']),
            'output': data['output'],
        })
    return out


def potion(kind='potion', name='water', count=1):
    return '{id:"minecraft:%s",count:%d,components:{"minecraft:potion_contents":{potion:"minecraft:%s"}}}' % (
        kind, count, name)


def tick_case(ticks, slots, brew_time=0, fuel=0, total=0, total_fuel=20, speed=1.0,
              ingredient='-'):
    filled = [slot if slot else '-' for slot in slots]
    return 'tick %d %s %d %d %d %d %s %s' % (
        ticks, ' '.join(filled), brew_time, fuel, total, total_fuel, speed, ingredient)


def tick_cases():
    water = potion()
    awkward = potion(name='awkward')
    splash = potion('splash_potion')
    lingering = potion('lingering_potion')
    wart = stack('minecraft:nether_wart')
    wart2 = stack('minecraft:nether_wart', count=2)
    wart5 = stack('minecraft:nether_wart', count=5)
    wart21 = stack('minecraft:nether_wart', count=21)
    blaze = stack('minecraft:blaze_powder')
    blaze2 = '{id:"minecraft:blaze_powder",count:2}'
    redstone = stack('minecraft:redstone')
    out = []
    # Brew-time countdown, with fuel already in the tank.
    for ticks in (0, 1, 2, 399, 400, 401, 402, 800):
        out.append(tick_case(ticks, [water, None, None, wart, None], fuel=20,
                             total=400, ingredient='minecraft:nether_wart'))
    # Blaze powder loads the tank (uses=20, speed=1), then the brew runs.
    for ticks in (0, 1, 2, 399, 400, 401):
        out.append(tick_case(ticks, [water, None, None, wart, blaze], total=0,
                             ingredient='minecraft:nether_wart'))
    out.append(tick_case(1, [water, None, None, wart, blaze2], total=0,
                         ingredient='minecraft:nether_wart'))
    # Fuel loads even with nothing to brew.
    out.append(tick_case(1, [None, None, None, wart, blaze], total=0,
                         ingredient='minecraft:nether_wart'))
    out.append(tick_case(1, [None, None, None, None, blaze], total=0))
    # Brew time is ceil(400 / speed) and speed 0 falls back to 1.
    for speed, finish in ((2.0, 200), (3.0, 134), (0.3, 1334), (0.0, 400)):
        for ticks in (1, finish, finish + 1, finish + 2):
            out.append(tick_case(ticks, [water, None, None, wart, None], fuel=5,
                                 total=0, speed=speed, ingredient='minecraft:nether_wart'))
    # Only the slots with a recipe change; the no-recipe bottle stays.
    for ticks in (1, 400, 401):
        out.append(tick_case(ticks, [water, awkward, water, wart, None], fuel=20,
                             total=400, ingredient='minecraft:nether_wart'))
    # The ingredient stack shrinks; the last one leaves an empty slot.
    for wart_stack, ticks in ((wart2, 400), (wart5, 400), (stack('minecraft:nether_wart'), 400)):
        out.append(tick_case(ticks, [water, None, None, wart_stack, None], fuel=20,
                             total=400, ingredient='minecraft:nether_wart'))
    # Changing the ingredient mid-brew clears the progress; finishing does not need fuel.
    out.append(tick_case(1, [water, None, None, redstone, None], brew_time=200, fuel=5,
                         total=400, ingredient='minecraft:nether_wart'))
    out.append(tick_case(1, [water, None, None, wart, None], brew_time=1, fuel=0,
                         total=400, ingredient='minecraft:nether_wart'))
    out.append(tick_case(1, [water, None, None, None, None], brew_time=1, fuel=5,
                         total=400, ingredient='minecraft:nether_wart'))
    # Splash and lingering potions keep their kind through the mix.
    out.append(tick_case(401, [splash, None, None, wart, None], fuel=20, total=400,
                         ingredient='minecraft:nether_wart'))
    out.append(tick_case(401, [lingering, None, None, wart, None], fuel=20, total=400,
                         ingredient='minecraft:nether_wart'))
    # Twenty brews from one blaze powder's twenty uses; the 21st ingredient stays.
    out.append(tick_case(8001, [water, None, None, wart21, blaze], total=0,
                         ingredient='minecraft:nether_wart'))
    out.append(tick_case(8002, [water, None, None, wart21, blaze], total=0,
                         ingredient='minecraft:nether_wart'))
    # Bottle bits are set from the slots on the first tick.
    for slots in ([None, None, None], [water, None, None], [water, None, water],
                  [water, awkward, water]):
        out.append(tick_case(1, slots + [None, None], fuel=20))
    return out


def make_cases(build):
    resources = lab.LOCAL / 'horizon-client' / '26.3'
    catalog = json.loads((resources / 'items.json').read_text())
    all_recipes = recipes(resources)
    potions = sorted({p for r in all_recipes
                      for p in (r['output'].get('components', {})
                                .get('minecraft:potion_contents', {}).get('potion', ''),)
                      if p})
    potion_ids = sorted({r['input'].split('"potion":"')[1].split('"')[0]
                         for r in all_recipes if '"potion":"' in r['input']})
    reagents = sorted({r['reagent'] for r in all_recipes})
    cases = []
    # The baseline corpus: each bundled recipe's exact pair with each of the
    # three slots probed. --baseline reproduces it without the cross-product.
    for r in all_recipes:
        for candidate in (r['input'], r['reagent'], stack('minecraft:blaze_powder'),
                          stack('minecraft:stick')):
            cases.append(f"{r['input']} {r['reagent']} {candidate}")
    if '--baseline' not in sys.argv:
        # Every potion item and potion id against every reagent, plus a few
        # mismatches and empty sides.
        for item in POTION_ITEMS:
            for potion in potion_ids + ['minecraft:water', 'minecraft:awkward']:
                for reagent in reagents:
                    inp = stack('minecraft:' + item, potion)
                    cases.append(f"{inp} {reagent} {inp}")
                cases.append(f"{stack('minecraft:' + item, potion)} {stack('minecraft:stick')} -")
                cases.append(f"- {stack('minecraft:stick')} {stack('minecraft:' + item, potion)}")
        # Every item as a filter candidate, so the slot rules are compared on
        # the whole registry and not only on the recipe inputs.
        for item in sorted(catalog):
            if item != 'minecraft:air':
                cases.append(f"- - {stack(item)}")
        for potion in potion_ids:
            for item in POTION_ITEMS:
                cases.append(f"- - {stack('minecraft:' + item, potion)}")
    if '--no-tick' not in sys.argv:
        cases += tick_cases()
    # Duplicate-free while keeping the first occurrence order.
    seen = set()
    unique = []
    for case in cases:
        if case not in seen:
            seen.add(case)
            unique.append(case)
    path = build / 'cases.txt'
    path.write_text(f'#resources {resources}\n' + ''.join(c + '\n' for c in unique))
    return path, len(unique)


if __name__ == '__main__':
    harness.differential(
        'brewing-parity', 'VanillaBrewingOracle', 'brewing_fixture', make_cases,
        'BrewingRecipe matching and assemble over the vanilla brewing data, the brewing_input and '
        'brewing_reagent property sets and the potion, reagent and fuel slot filters, for every '
        'bundled recipe, potion type/id and item candidate, plus the brewing-stand tick: brew time and '
        'speed multiplier, fuel uses and loading, ingredient shrink, outputs and bottle bits. --baseline '
        'leaves out the cross-product and whole-registry slot candidates; --no-tick leaves out the tick cases.',
        sources=[lab.ROOT / 'crates/horizon-client-render/src/brewing.rs',
                 lab.ROOT / 'crates/horizon-items/src/item_data.rs',
                 lab.ROOT / 'crates/horizon-items/src/items.rs'])
