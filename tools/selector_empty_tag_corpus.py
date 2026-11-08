"""Fixed-UUID pigs; actual tag edits, no entity-order or observation-tick dependency."""

ACTORS = [('a', '0000010b-0000-0000-0000-000000000001', 1, 4.5),
          ('b', '0000010b-0000-0000-0000-000000000002', 2, 8.5)]


def empty_tags(full=False):
    ops = []

    def cmd(text, label, result=None, callback=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if callback is not None:
            op['expect_callback'] = callback
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('spawn_mobs', 'false'), ('advance_time', 'false'), ('random_tick_speed', '0')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule)
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/fill 0 99 0 15 99 15 stone', 'floor', 256)
    cmd('/tp @s 4.5 100 12.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h267 dummy', 'objective')
    cmd('/scoreboard objectives add h267_clock dummy', 'clock-objective')
    for actor, uuid, n, x in ACTORS:
        tags = ',Tags:["audit"]' if actor == 'b' else ''
        cmd(f'/summon pig {x} 100 4.5 {{UUID:[I;267,0,0,{n}],NoAI:1b,NoGravity:1b{tags}}}',
            actor + '-summon', 1)
    cmd('/execute store result score start h267_clock run time query gametime',
        'clock-start', diagnostic=True)

    for phase, tagged_a in [('initial', False), ('swapped', True), ('restored', False)]:
        if phase != 'initial':
            for actor, uuid, _, _ in ACTORS:
                tagged = tagged_a if actor == 'a' else not tagged_a
                cmd(f'/tag {uuid} {"add" if tagged else "remove"} audit',
                    phase + '-' + actor + '-edit', 1)
        cmd('/execute if entity @s[type=player,name=Player]', phase + '-owner', 1)
        for field, result in [('UUID', 4), ('Health', 20), ('Pos', 3), ('playerGameType', 1)]:
            cmd(f'/data get entity @s {field}', phase + '-owner-' + field, result)
        cmd('/execute if loaded 4 100 4', phase + '-loaded', 1)
        cmd('/execute if block 4 99 4 stone', phase + '-floor', 1)
        cmd('/execute store result score now h267_clock run time query gametime',
            phase + '-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h267_clock -= start h267_clock', phase + '-clock-delta-op', 0)
        cmd('/scoreboard players get now h267_clock', phase + '-clock-delta', 0)
        for actor, uuid, n, x in ACTORS:
            key = phase + '-' + actor
            tagged = tagged_a if actor == 'a' else not tagged_a
            cmd(f'/execute as {uuid} if entity @s[type=pig]', key + '-type', 1)
            for field, result in [('UUID', 4), ('Health', 10), ('Pos', 3), ('NoAI', 1), ('NoGravity', 1)]:
                cmd(f'/data get entity {uuid} {field}', key + '-' + field, result)
            cmd(f'/tag {uuid} list', key + '-live-tags', 1 if tagged else 0)
            cmd(f'/data get entity {uuid} Tags', key + '-Tags',
                result=1 if tagged else None, callback=None if tagged else [False, 0], diagnostic=True)
            # Keep the full body as raw diagnostics; typed scalar guards above are canonical.
            cmd(f'/data get entity {uuid}', key + '-body', diagnostic=True)
            for name, value, matches in [('empty', '', not tagged), ('nonempty', '!', tagged),
                                          ('audit', 'audit', tagged), ('not_audit', '!audit', not tagged)]:
                for sign in ['if', 'unless']:
                    label = key + '-' + name + '-' + sign
                    take = matches if sign == 'if' else not matches
                    for holder in ['result', 'success', 'branch']:
                        cmd(f'/scoreboard players set {holder} h267 -99', label + '-reset-' + holder, -99)
                    cmd(f'/execute as {uuid} store result score result h267 store success score success h267 '
                        f'{sign} entity @s[tag={value}] run scoreboard players set branch h267 1',
                        label + '-action', admission=name not in ('empty', 'nonempty'))
                    ops[-1]['expect_results'] = [[True, 1]] if take else []
                    for holder in ['result', 'success', 'branch']:
                        cmd(f'/scoreboard players get {holder} h267', label + '-' + holder,
                            1 if take else -99, admission=name not in ('empty', 'nonempty'))
    return ops
