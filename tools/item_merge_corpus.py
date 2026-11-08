"""Dropped-item neighbour merging through the real command/Game path."""


def stack_limit_merges(cmd, setup, diagnostic=False):
    cases = [
        dict(name='limit1', counts=(1, 1), limits=(1, 1), after=(1, 1)),
        dict(name='limit16', counts=(12, 8), limits=(16, 16), after=(12, 8)),
        dict(name='limit99', counts=(80, 10), limits=(99, 99), after=(64, 26), ages_after=(144, 145)),
        dict(name='finite_delay', counts=(5, 3), after=(8, None), delays=(0, 55), delays_after=(11, 0)),
        dict(name='pearl16', items=('ender_pearl', 'ender_pearl'), counts=(10, 6), after=(16, None)),
        dict(name='equal', counts=(4, 4), after=(None, 8)),
        dict(name='full64', counts=(64, 1), after=(64, 1)),
        dict(name='over64', counts=(70, 1), after=(70, 1)),
        dict(name='different_item', items=('stone', 'dirt'), counts=(3, 3), after=(3, 3)),
        dict(name='different_component', counts=(3, 3), limits=(16, 17), after=(3, 3)),
        dict(name='infinite_delay', counts=(3, 5), after=(3, 5), delays=(0, 32767), delays_after=(0, 32767)),
        dict(name='infinite_age', counts=(3, 5), after=(3, 5), ages=(-32768, 100), ages_after=(-32768, 145)),
    ]
    def observer():
        if diagnostic:
            return [cmd('/execute if entity @s[gamemode=spectator,nbt={Health:20f,Pos:[16.5d,110d,0.5d]}]', expect_result=1)]
        # Verify the effective mode and body independently of the separate
        # Target::Player gamemode-selector defect (raw diagnostic retained).
        return [cmd('/data get entity @s Health', expect_result=20),
                cmd('/data get entity @s Pos', expect_result=3),
                cmd('/data get entity @s playerGameType', expect_result=3),
                cmd('/execute if entity @s[nbt={Health:20f,Pos:[16.5d,110d,0.5d]}]', expect_result=1)]

    ops = [cmd('/tick freeze', expect_result=1),
           cmd('/gamemode spectator', expect_result=1),
           cmd('/tp @s 16.5 110 0.5', expect_result=1),
           cmd('/forceload add -16 -16 47 15', expect_result=8),
           {'op': 'settle'}]
    ops += [cmd(f'/execute if loaded {x} 100 {z}', expect_result=1)
            for x in (-16, 0, 16, 32) for z in (-16, 0)]
    ops += [cmd('/fill -16 99 -16 47 99 15 stone', expect_result=2048),
            cmd('/scoreboard objectives add merge_clock dummy', expect_result=1),
            *observer()]

    def target(number):
        return f'0000012d-0000-0000-0000-{number:012x}'

    def stack(item, count, limit):
        patch = '' if limit is None else f',components:{{"minecraft:max_stack_size":{limit}}}'
        return f'{{id:"minecraft:{item}",count:{count}{patch}}}'

    for batch in range(4):
        selection = '@e[tag=merge_subject]' if diagnostic else '@e[type=item]'
        ops += setup('/kill ' + selection)
        ops.append(cmd('/execute unless entity ' + selection, expect_result=1))
        ops += setup('/execute store result score before merge_clock run time query gametime')
        for lane, case in enumerate(cases[batch * 3:batch * 3 + 3]):
            x = 30.5 + lane * 6
            ops += [cmd(f'/execute if block {int(x)} 99 0 stone', expect_result=1),
                    cmd(f'/execute if block {int(x)} 100 0 air', expect_result=1)]
            for side in range(2):
                number = 2 * (batch * 3 + lane) + side + 1
                item = case.get('items', ('stone', 'stone'))[side]
                count = case['counts'][side]
                limit = case.get('limits', (None, None))[side]
                age = case.get('ages', (100, 100))[side]
                delay = case.get('delays', (0, 0))[side]
                tag = f'merge_{case["name"]}_{side}'
                uid = f'[I;301,0,0,{number}]'
                pos = f'[{x + side * .1}d,100d,0.5d]'
                tags = f'["merge_subject","{tag}"]' if diagnostic else f'["{tag}"]'
                data = (f'UUID:{uid},Tags:{tags},Item:{stack(item, count, limit)},'
                        f'Age:{age}s,PickupDelay:{delay}s,Motion:[0d,0d,0d]')
                ops.append(cmd(f'/summon item {x + side * .1} 100 0.5 {{{data}}}', expect_result=1))
                guard = (f'/execute as {target(number)} if entity @s[type=item,tag={tag},'
                         f'nbt={{UUID:{uid},Health:5s,Pos:{pos},Age:{age}s,PickupDelay:{delay}s,Item:{stack(item, count, limit)}}}]')
                ops.append(cmd(guard, expect_result=1))
                for field in ('UUID', 'Tags', 'Item', 'Item.count', 'Health', 'Pos', 'Motion', 'Age', 'PickupDelay'):
                    expected = {'Item.count': count, 'Health': 5, 'Age': age, 'PickupDelay': delay}.get(field)
                    ops.append(cmd(f'/data get entity {target(number)} {field}', expect_result=expected))
        ops.append(cmd('/tick step 45', expect_result=1))
        ops.append({'op': 'tick', 'n': 45})
        ops += setup('/execute store result score elapsed merge_clock run time query gametime',
                     '/scoreboard players operation elapsed merge_clock -= before merge_clock')
        ops.append(cmd('/scoreboard players get elapsed merge_clock', expect_result=45))
        for lane, case in enumerate(cases[batch * 3:batch * 3 + 3]):
            for side in range(2):
                number = 2 * (batch * 3 + lane) + side + 1
                count = case['after'][side]
                if count is None:
                    ops.append(cmd(f'/execute unless entity {target(number)}', expect_result=1))
                    continue
                item = case.get('items', ('stone', 'stone'))[side]
                limit = case.get('limits', (None, None))[side]
                age = case.get('ages_after', (145, 145))[side]
                delay = case.get('delays_after', (0, 0))[side]
                ops.append(cmd(f'/execute as {target(number)} if entity @s[type=item,tag=merge_{case["name"]}_{side},'
                               f'nbt={{UUID:[I;301,0,0,{number}],Health:5s,Item:{stack(item, count, limit)}}}]', expect_result=1))
                for field in ('UUID', 'Tags', 'Item', 'Item.count', 'Health', 'Pos', 'Age', 'PickupDelay'):
                    expected = {'Item.count': count, 'Health': 5, 'Age': age, 'PickupDelay': delay}.get(field)
                    ops.append(cmd(f'/data get entity {target(number)} {field}', expect_result=expected))
            # Missing stacks contribute zero through the normal command callback;
            # the explicit identity/absence guards above distinguish removal.
            a = target(2 * (batch * 3 + lane) + 1)
            b = target(2 * (batch * 3 + lane) + 2)
            ops += [cmd(f'/execute store result score left merge_clock run data get entity {a} Item.count'),
                    cmd(f'/execute store result score right merge_clock run data get entity {b} Item.count')]
            ops += setup('/scoreboard players operation left merge_clock += right merge_clock')
            ops.append(cmd('/scoreboard players get left merge_clock', expect_result=sum(case['counts'])))
        ops += observer()
    ops += setup('/kill ' + ('@e[tag=merge_subject]' if diagnostic else '@e[type=item]'), '/tick unfreeze')
    if not diagnostic:
        # Leave the shared corpus's observer and world ready for the next group.
        ops += setup('/scoreboard objectives remove merge_clock',
                     '/fill -16 99 -16 47 99 15 air',
                     '/forceload remove -16 -16 47 15',
                     '/gamemode creative', '/tp @s 8.5 230 8.5 0 0')
    return ops
