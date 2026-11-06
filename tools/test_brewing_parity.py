#!/usr/bin/env python3
"""Compare the brewing recipes' slot rules, matching and assembled output with vanilla.

The Java oracle loads the vanilla data pack's real `RecipeManager`, takes its
`brewing_input` and `brewing_reagent` property sets and the `BrewingRecipe`s,
checks all three brewing-slot filters and calls the first matching recipe's
`assemble`. The Rust probe calls `brewing::Brewing::is_bottle`/`is_reagent`,
`brewing_fuel` and `Brewing::find`. Cases cover every bundled brewing recipe,
every potion type and potion id against every reagent, candidates from the
whole item registry, and non-matching pairs.
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


def stack(item, potion=None):
    if potion:
        return '{id:"%s",count:1,components:{"minecraft:potion_contents":{potion:"%s"}}}' % (
            item, potion)
    return '{id:"%s",count:1}' % item


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
        'bundled recipe, potion type/id and item candidate. --baseline leaves out the cross-product '
        'and whole-registry slot candidates. The brewing-stand tick, brew time and fuel use remain pending.',
        sources=[lab.ROOT / 'crates/horizon-client-render/src/brewing.rs',
                 lab.ROOT / 'crates/horizon-items/src/item_data.rs',
                 lab.ROOT / 'crates/horizon-items/src/items.rs'])
