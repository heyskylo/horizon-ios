#!/usr/bin/env python3
"""Compare crafting, cooking, stonecutting and smithing with vanilla's RecipeManager.

The Java oracle (VanillaRecipeOracle) loads the data pack's recipe registry and
answers craft/cook/stonecut/smith commands; the Rust example (recipe_fixture)
answers the same commands from horizon_client::crafting and ::stations.
Crafting cases are built from every recipe's own ingredients (placed at random
offsets, mirrored, perturbed) plus focused generators for the special recipes
(banners, books, fireworks, dyeing, repair, pots, tipped arrows, maps) and
random grids. Results are compared as canonical ItemStack NBT, with the
matched recipe id and the crafting remainders.
"""
import argparse
import json
import random
try:
    from . import interaction_lab as lib, lab
except ImportError:
    import interaction_lab as lib
    import lab

DYES = ['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray', 'light_gray', 'cyan', 'purple',
        'blue', 'brown', 'green', 'red', 'black']
POTIONS = ['minecraft:healing', 'minecraft:swiftness', 'minecraft:water', 'minecraft:night_vision']
SHAPES = ['small_ball', 'large_ball', 'star', 'creeper', 'burst']


def quote(text):
    return '"' + text.replace('\\', '\\\\').replace('"', '\\"') + '"'


def stack(item, count=1, **components):
    """SNBT of an ItemStack.CODEC compound; component values are SNBT text."""
    body = ','.join(f'{quote("minecraft:" + key)}:{value}' for key, value in sorted(components.items()))
    comp = f',components:{{{body}}}' if body else ''
    return f'{{id:{quote(item)},count:{count}{comp}}}'


def colors(rng, n=None):
    n = rng.randrange(0, 4) if n is None else n
    return '[I;' + ','.join(str(rng.randrange(1 << 24)) for _ in range(n)) + ']'


def explosion(rng):
    return ('{shape:%s,colors:%s,fade_colors:%s,has_trail:%db,has_twinkle:%db}'
            % (quote(rng.choice(SHAPES)), colors(rng), colors(rng), rng.randrange(2), rng.randrange(2)))


class Gen:
    def __init__(self, rng, items, recipes, patterns):
        self.rng = rng
        self.items = items
        self.recipes = recipes
        self.patterns = patterns
        self.durable = [k for k, v in items.items() if v.get('minecraft:max_damage')]

    def layers(self, n=None):
        n = self.rng.choice([0, 0, 1, 2, 6, 7]) if n is None else n
        return '[' + ','.join('{pattern:%s,color:%s}' % (quote(self.rng.choice(self.patterns)), quote(self.rng.choice(DYES))) for _ in range(n)) + ']'

    def book(self, generation=None):
        g = self.rng.randrange(4) if generation is None else generation
        return stack('minecraft:written_book', written_book_content='{title:{raw:"T"},author:"A",generation:%d,pages:[{raw:"p1"},{raw:"p2"}]}' % g)

    def stack(self, item, count=None, rich=0.5):
        """A stack of the item, with the components that apply to it."""
        rng = self.rng
        info = self.items.get(item, {})
        count = rng.choice([1, 1, 1, 2, 3]) if count is None else count
        c = {}
        if rng.random() < rich:
            if info.get('minecraft:max_damage') and rng.random() < 0.7:
                c['damage'] = str(rng.randrange(0, info['minecraft:max_damage'] + 1))
            if info.get('minecraft:max_damage') and rng.random() < 0.3:
                c['enchantments'] = '{%s:1}' % quote(rng.choice(['minecraft:binding_curse', 'minecraft:vanishing_curse', 'minecraft:mending', 'minecraft:unbreaking']))
            if item.endswith('_banner'):
                c['banner_patterns'] = self.layers()
            if item == 'minecraft:shield':
                if rng.random() < 0.3:
                    c['banner_patterns'] = self.layers(rng.randrange(1, 3))
                    c['base_color'] = quote(rng.choice(DYES))
            if item == 'minecraft:written_book':
                return self.book()
            if item == 'minecraft:firework_star':
                c['firework_explosion'] = explosion(rng)
            if item == 'minecraft:firework_rocket':
                c['fireworks'] = '{flight_duration:%db,explosions:[%s]}' % (rng.randrange(1, 4), explosion(rng))
            if item in ('minecraft:lingering_potion', 'minecraft:potion', 'minecraft:splash_potion'):
                c['potion_contents'] = '{potion:%s}' % quote(rng.choice(POTIONS))
            if item.startswith('minecraft:leather_') or item in ('minecraft:wolf_armor',):
                if rng.random() < 0.6:
                    c['dyed_color'] = str(rng.randrange(1 << 24))
            if item.endswith('shulker_box'):
                c['container'] = '[{slot:0,item:{id:"minecraft:stone",count:%d}}]' % rng.randrange(1, 64)
            if item.endswith('_bundle') or item == 'minecraft:bundle':
                c['bundle_contents'] = '[{id:"minecraft:stone",count:%d}]' % rng.randrange(1, 64)
            if item == 'minecraft:filled_map':
                c['map_id'] = str(rng.randrange(1, 5))
        return stack(item, count, **c)

    def plain(self, item):
        return self.stack(item, rich=0.3)


def place(rng, cells, size):
    """Cells (stack list) at random free positions of a size x size grid."""
    grid = ['-'] * (size * size)
    spots = rng.sample(range(size * size), len(cells))
    for spot, cell in zip(spots, cells):
        grid[spot] = cell
    return grid


def place_box(rng, width, height, cells, size):
    """A width x height pattern at a random offset of the grid."""
    x0 = rng.randrange(size - width + 1)
    y0 = rng.randrange(size - height + 1)
    grid = ['-'] * (size * size)
    for i, cell in enumerate(cells):
        grid[(y0 + i // width) * size + x0 + i % width] = cell
    return grid


def craft_case(grid, size):
    return 'craft\t%d\t%s' % (size, '\t'.join(grid))


def recipe_cases(g, count_per):
    rng = g.rng
    for recipe in g.recipes:
        if 'slots' not in recipe or not recipe['ingredients']:
            continue
        ingredients = recipe['ingredients']
        slots = recipe['slots']
        shaped = recipe['kind'] == 'ShapedRecipe'
        if shaped:
            width = recipe['width']
            height = len(slots) // width
        for variant in range(count_per):
            pick = [g.plain(rng.choice(i)) if i else '-' for i in ingredients]
            if shaped:
                cells = ['-' if s < 0 else pick[s] for s in slots]
                if variant % 3 == 1:  # mirrored
                    cells = [cells[y * width + width - 1 - x] for y in range(height) for x in range(width)]
                size = rng.choice([s for s in (2, 3) if s >= max(width, height)])
                grid = place_box(rng, width, height, cells, size)
            else:
                size = 3 if len(pick) > 4 else rng.choice([2, 3])
                grid = place(rng, pick, size)
            yield craft_case(grid, size)
            # Perturbations: drop one, swap one for a stranger, add one, shift a count.
            if variant < 2:
                filled = [i for i, c in enumerate(grid) if c != '-']
                broken = list(grid)
                broken[rng.choice(filled)] = '-'
                yield craft_case(broken, size)
                broken = list(grid)
                broken[rng.choice(filled)] = g.plain(rng.choice([i for i in g.items if i != 'minecraft:air']))
                yield craft_case(broken, size)
                free = [i for i, c in enumerate(grid) if c == '-']
                if free:
                    broken = list(grid)
                    broken[rng.choice(free)] = g.plain(rng.choice([i for i in g.items if i != 'minecraft:air']))
                    yield craft_case(broken, size)


def special_cases(g, n):
    rng = g.rng
    items = g.items
    banners = [f'minecraft:{d}_banner' for d in DYES]
    dyes = [f'minecraft:{d}_dye' for d in DYES]
    armor = [k for k in items if k.startswith('minecraft:leather_') and k.count('_') == 2] + ['minecraft:wolf_armor']
    boxes = [k for k in items if k.endswith('shulker_box')]
    bundles = [k for k in items if k.endswith('bundle')]
    skulls = [k for k in items if k.endswith('_head') or k.endswith('_skull')]
    for _ in range(n):
        size = rng.choice([2, 3])
        # banner duplicate / shield decoration
        a, b = rng.choice(banners), rng.choice(banners)
        yield craft_case(place(rng, [g.stack(a, 1, 1.0), g.stack(b if rng.random() < 0.3 else a, 1, 0.5)], size), size)
        yield craft_case(place(rng, [g.stack(rng.choice(banners), 1, 1.0), g.stack('minecraft:shield', 1, 0.5)], size), size)
        # book cloning
        books = [g.book(rng.choice([0, 0, 1, 2, 3]))] + [stack('minecraft:writable_book') for _ in range(rng.randrange(0, 4))]
        if rng.random() < 0.2:
            books.append(g.book())
        yield craft_case(place(rng, books, 3), 3)
        # fireworks
        star = [stack(rng.choice(['minecraft:gunpowder'] + dyes[:2] + ['minecraft:gold_nugget', 'minecraft:feather', 'minecraft:fire_charge',
                                                                           'minecraft:diamond', 'minecraft:glowstone_dust', rng.choice(skulls)]))
                for _ in range(rng.randrange(1, 7))]
        star += [stack('minecraft:gunpowder'), stack(rng.choice(dyes))]
        yield craft_case(place(rng, star[:9], 3), 3)
        fade = [g.stack('minecraft:firework_star', 1, 1.0)] + [stack(rng.choice(dyes)) for _ in range(rng.randrange(1, 4))]
        yield craft_case(place(rng, fade, 3), 3)
        rocket = [stack('minecraft:paper')] + [stack('minecraft:gunpowder') for _ in range(rng.randrange(0, 5))] + \
                 [g.stack('minecraft:firework_star', 1, 1.0) for _ in range(rng.randrange(0, 4))]
        yield craft_case(place(rng, rocket[:9], 3), 3)
        # armour dyeing, with leftovers
        dye_cells = [g.stack(rng.choice(armor), 1, 0.7)] + [stack(rng.choice(dyes + ['minecraft:stick'] if rng.random() < 0.1 else dyes)) for _ in range(rng.randrange(1, 5))]
        yield craft_case(place(rng, dye_cells[:9], 3), 3)
        # repair
        tool = rng.choice(g.durable)
        pair = [g.stack(tool, 1, 1.0), g.stack(tool if rng.random() < 0.85 else rng.choice(g.durable), 1, 1.0)]
        yield craft_case(place(rng, pair, size), size)
        # shulker boxes and bundles
        yield craft_case(place(rng, [g.stack(rng.choice(boxes + bundles), 1, 1.0), stack(rng.choice(dyes))], size), size)
        multi = [g.stack(rng.choice(boxes + bundles), 1, 1.0)] + [stack(rng.choice(dyes)) for _ in range(rng.randrange(1, 4))]
        yield craft_case(place(rng, multi, 3), 3)
        # decorated pots
        sherds = ['minecraft:brick', 'minecraft:angler_pottery_sherd', 'minecraft:archer_pottery_sherd', 'minecraft:stick', 'minecraft:blade_pottery_sherd']
        pot = ['-', stack(rng.choice(sherds)), '-', stack(rng.choice(sherds)), '-', stack(rng.choice(sherds)), '-', stack(rng.choice(sherds)), '-']
        if rng.random() < 0.2:
            pot[rng.choice([0, 2, 6, 8, 4])] = stack('minecraft:brick')
        yield craft_case(pot, 3)
        yield craft_case([stack('minecraft:brick') if i in (1, 3, 5, 7) else '-' for i in range(9)], 3)
        # tipped arrows
        ring = [stack('minecraft:arrow', rng.choice([1, 2]))] * 9
        ring[4] = g.stack('minecraft:lingering_potion', 1, 1.0 if rng.random() < 0.8 else 0.0)
        yield craft_case(ring, 3)
        # map cloning
        maps = [g.stack('minecraft:filled_map', 1, 1.0)] + [stack('minecraft:map') for _ in range(rng.randrange(1, 9))]
        yield craft_case(place(rng, maps, 3), 3)
        # mixed junk of the interesting items
        pool = banners + dyes + armor + boxes + bundles + ['minecraft:written_book', 'minecraft:writable_book', 'minecraft:firework_star',
                                                           'minecraft:paper', 'minecraft:gunpowder', 'minecraft:shield', 'minecraft:glass_bottle',
                                                           'minecraft:milk_bucket', 'minecraft:honey_bottle', 'minecraft:bowl', 'minecraft:stick']
        junk = [g.stack(rng.choice(pool), None, 0.7) for _ in range(rng.randrange(1, 6))]
        yield craft_case(place(rng, junk, 3), 3)


def smithing_cases(g):
    rng = g.rng
    for recipe in g.recipes:
        if recipe['kind'].startswith('Smithing'):
            for _ in range(24):
                pick = lambda key: g.plain(rng.choice(recipe[key])) if recipe[key] else '-'
                yield 'smith\t%s\t%s\t%s' % (pick('template'), pick('base'), pick('addition'))
                yield 'smith\t%s\t%s\t%s' % (pick('template'), pick('base'), g.plain(rng.choice([i for i in g.items if i != 'minecraft:air'])))
    # Armour with a trim already on it.
    trims = ['{material:"minecraft:iron",pattern:"minecraft:coast"}', '{material:"minecraft:gold",pattern:"minecraft:sentry"}']
    for recipe in g.recipes:
        if recipe['kind'] == 'SmithingTrimRecipe':
            for trim in trims:
                base = rng.choice(recipe['base'])
                yield 'smith\t%s\t%s\t%s' % (stack(rng.choice(recipe['template'])), stack(base, 1, trim=trim), stack(rng.choice(recipe['addition'])))


def smithing_transform_cases():
    """Decode custom recipes on both sides; exercise actual optional matching and assembly."""
    base = stack('minecraft:stick', custom_name='"Original"', repair_cost='7', custom_data='{kept:1}')
    for template in [None, 'minecraft:diamond']:
        for addition in [None, 'minecraft:diamond']:
            recipe = {'type': 'minecraft:smithing_transform', 'base': 'minecraft:stick',
                      'result': {'id': 'minecraft:emerald', 'count': 3,
                                 'components': {'minecraft:custom_name': 'Upgraded', 'minecraft:repair_cost': 0}}}
            if template is not None:
                recipe['template'] = template
            if addition is not None:
                recipe['addition'] = addition
            for t in ['-', stack('minecraft:diamond')]:
                for a in ['-', stack('minecraft:diamond')]:
                    yield '\t'.join(['smith_transform', json.dumps(recipe, separators=(',', ':')), t, base, a])
    for result in ['minecraft:emerald', {'id': 'minecraft:emerald', 'count': 0},
                   {'id': 'minecraft:emerald', 'count': 100}, {'id': 'minecraft:diamond_sword', 'count': 2},
                   {'id': 'minecraft:emerald', 'components': {'!minecraft:custom_name': {}}},
                   {'id': 'minecraft:emerald', 'components': {'minecraft:max_damage': 10}},
                   {'id': 'minecraft:emerald', 'count': 65, 'components': {'minecraft:max_stack_size': 99}}]:
        recipe = {'type': 'minecraft:smithing_transform', 'base': 'minecraft:stick', 'result': result}
        yield '\t'.join(['smith_transform', json.dumps(recipe, separators=(',', ':')), '-', base, '-'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seed', type=int, default=7)
    parser.add_argument('--variants', type=int, default=6)
    parser.add_argument('--specials', type=int, default=1500)
    parser.add_argument('--random', type=int, default=4000)
    parser.add_argument('--limit', type=int, default=3)
    parser.add_argument('--smithing-transforms-only', action='store_true')
    args = parser.parse_args()
    work = lib.work_dir('recipe-parity')
    vanilla = lib.Vanilla(work)
    vanilla.compile('ParityData', 'VanillaRecipeOracle')
    if args.smithing_transforms_only:
        cases = list(smithing_transform_cases())
        (work / 'cases.txt').write_text(''.join(c + '\n' for c in cases))
        vanilla.run('VanillaRecipeOracle', work / 'cases.txt', work / 'native.txt')
        lib.cargo_example('horizon-client', 'recipe_fixture', lib.RESOURCES, work / 'cases.txt', work / 'rust.txt')
        raise SystemExit(bool(lib.compare(cases, work / 'native.txt', work / 'rust.txt', lambda *_: 'smithing transform', limit=args.limit)))
    vanilla.run('VanillaRecipeOracle', 'dump', work / 'recipes.jsonl')
    recipes = [json.loads(line) for line in (work / 'recipes.jsonl').read_text().splitlines()]
    # Shaped recipes: widths come from the data pack.
    for recipe in recipes:
        path = lib.RESOURCES / 'data/minecraft/recipe' / (recipe['id'] + '.json')
        data = json.loads(path.read_text())
        if recipe['kind'] == 'ShapedRecipe':
            rows = [row.rstrip() for row in data['pattern']]
            left = min(len(row) - len(row.lstrip()) for row in rows if row)
            recipe['width'] = max(len(row) for row in rows) - left
    items = json.loads((lib.RESOURCES / 'items.json').read_text())
    patterns = sorted('minecraft:' + p.stem for p in (lib.RESOURCES / 'data/minecraft/banner_pattern').glob('*.json'))
    rng = random.Random(args.seed)
    g = Gen(rng, items, recipes, patterns)
    cases = list(recipe_cases(g, args.variants))
    cases += list(special_cases(g, args.specials))
    pool = [k for k in items if not k.endswith('_spawn_egg') and k != 'minecraft:air']
    interesting = [f'minecraft:{d}_dye' for d in DYES] + ['minecraft:paper', 'minecraft:gunpowder', 'minecraft:bowl', 'minecraft:stick',
                                                          'minecraft:milk_bucket', 'minecraft:honey_bottle', 'minecraft:water_bucket']
    for _ in range(args.random):
        size = rng.choice([2, 3])
        cells = [g.stack(rng.choice(interesting if rng.random() < 0.6 else pool), None, 0.5) for _ in range(rng.randrange(1, size * size + 1))]
        cases.append(craft_case(place(rng, cells, size), size))
    for kind in ['smelting', 'blasting', 'smoking', 'campfire_cooking']:
        for item in [i for i in items if i != 'minecraft:air']:
            cases.append('cook\t%s\t%s' % (kind, stack(item)))
    for item in [i for i in items if i != 'minecraft:air']:
        cases.append('stonecut\t' + stack(item))
    cases += list(smithing_cases(g))
    cases += list(smithing_transform_cases())
    (work / 'cases.txt').write_text(''.join(c + '\n' for c in cases))
    vanilla.run('VanillaRecipeOracle', work / 'cases.txt', work / 'native.txt')
    lib.cargo_example('horizon-client', 'recipe_fixture', lib.RESOURCES, work / 'cases.txt', work / 'rust.txt')

    def key(case, native_line):
        kind = case.split('\t')[0]
        return kind if kind != 'craft' else 'craft ' + native_line.split('\t')[0]

    def describe(case):
        return case if len(case) < 1200 else case[:1200] + '...'
    total = lib.compare(cases, work / 'native.txt', work / 'rust.txt', key, describe, args.limit)
    raise SystemExit(1 if total else 0)


if __name__ == '__main__':
    main()
