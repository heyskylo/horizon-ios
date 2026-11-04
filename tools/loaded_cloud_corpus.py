"""Loaded cloud runtime-model contract and an independent strict full diagnostic.

The loader contract excludes all post-cloud HurtTime values/combined predicates,
all post-cloud health measurements for zombie/husk/drowned (both effect kinds),
and all noninstant post-dose duration values/predicates. These are the separate
timer-phase and damage-source gaps H256/H257, not loader parity successes.
Initial actor state and command-effect controls remain exact. No tolerances.

Run the original strict diagnostic with:
    python3 tools/loaded_cloud_corpus.py --full --binary PATH
It reports every residual; --allow-gaps only changes the driver's exit status.
"""


def loaded_clouds(cmd, setup, contract='loader', summary=None):
    out = []
    serial = 0
    excluded = 0
    categories_seen = {}

    def observe(command, result=None, categories=()):
        nonlocal excluded
        for category in categories:
            categories_seen[category] = categories_seen.get(category, 0) + 1
        if contract == 'loader' and categories:
            excluded += 1
        else:
            out.append(cmd(command, expect_result=result))

    def check(command, result=1, categories=()):
        observe(command, result, categories)

    def read(target, field, categories=()):
        observe(f'/data get entity {target} {field}', categories=categories)

    def entity(kind, x=4.5):
        nonlocal serial
        serial += 1
        uid = f'00000000-0000-00fa-0000-{serial:012x}'
        tag = f'cloud_actor_{serial}'
        check(f'/summon {kind} {x} 200 4.5 '
              f'{{UUID:[I;0,250,0,{serial}],Tags:["cloud_subject","{tag}"],NoAI:1b,'
              'NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:20f,'
              'attributes:[{id:"minecraft:max_health",base:40d}]}')
        check(f'/execute as {uid} if entity @s[tag={tag},type={kind}]')
        for field in ('UUID', 'Pos', 'Health', 'HurtTime'):
            read(uid, field)
        check(f'/execute if entity @e[tag={tag},nbt={{NoAI:1b,NoGravity:1b,Health:20f}}]')
        return uid

    def cloud(effect='instant_health', extra='', defaults=False, x=4, amplifier=0,
              potion=None):
        nonlocal serial
        serial += 1
        uid = f'00000000-0000-00fa-0000-{serial:012x}'
        fields = '' if defaults else (
            'Duration:200,WaitTime:0,RadiusOnUse:0f,RadiusPerTick:0f,'
            'ReapplicationDelay:100,DurationOnUse:0,potion_duration_scale:1f,')
        # Extra fields replace the ordinary matrix's explicit settings.
        values = {'Radius': '3f'}
        values.update(part.split(':', 1) for part in fields.strip(',').split(',') if part)
        values.update(part.split(':', 1) for part in extra.strip(',').split(',') if part)
        fields = ''.join(f'{k}:{v},' for k, v in values.items())
        contents = (f'potion_contents:{{custom_effects:[{{id:"minecraft:{effect}",duration:40,'
                    f'amplifier:{amplifier}}}]}},'
                    if effect else '')
        if potion:
            contents = f'potion_contents:{{potion:"minecraft:{potion}"}},'
        check(f'/summon area_effect_cloud {x} 200 4 '
              f'{{UUID:[I;0,250,0,{serial}],Tags:["cloud_subject","cloud_listener"],'
              f'Age:0,{fields}{contents}NoGravity:1b}}')
        check(f'/execute as {uid} if entity @s[tag=cloud_listener,type=area_effect_cloud]')
        for field in ('UUID', 'Pos', 'Age', 'Radius', 'Duration', 'WaitTime',
                      'ReapplicationDelay', 'RadiusOnUse', 'RadiusPerTick',
                      'DurationOnUse', 'potion_duration_scale'):
            read(uid, field)
        if potion:
            check(f'/execute as {uid} if entity @s[nbt={{potion_contents:{{'
                  f'potion:"minecraft:{potion}"}}}}]')
        elif effect:
            check(f'/execute as {uid} if entity @s[nbt={{potion_contents:{{custom_effects:[{{'
                  f'id:"minecraft:{effect}",duration:40}}]}}}}]')
        return uid

    def step(n):
        out.extend(setup('/execute store result score #start audit run time query gametime'))
        out.extend(setup(f'/tick step {n}'))
        out.append({'op': 'tick', 'n': n})
        out.extend(setup('/execute store result score #elapsed audit run time query gametime'))
        check('/scoreboard players operation #elapsed audit -= #start audit', n)
        check(f'/execute if score #elapsed audit matches {n}')

    def health(uid, value, hurt, cloud=True, armored=False):
        health_category = ('armored_cloud_health',) if cloud and armored else ()
        timer_category = ('dose_timers',) if cloud else ()
        read(uid, 'Health', health_category)
        read(uid, 'HurtTime', timer_category)
        check(f'/execute as {uid} if entity @s[nbt={{Health:{value}f,HurtTime:{hurt}s}}]',
              categories=health_category + timer_category)

    def clean():
        out.extend(setup('/kill @e[tag=cloud_subject]', '/kill @e[type=item]'))

    for command in ('/tick freeze', '/gamerule spawn_mobs false', '/time set midnight',
                    '/scoreboard objectives add audit dummy', '/forceload add 0 0 15 15'):
        out.append(cmd(command))
    out.append({'op': 'settle'})
    check('/execute if loaded 0 200 0')
    check('/fill 0 199 0 15 199 15 stone', 256)
    for kind in ('pig', 'zombie', 'skeleton', 'husk', 'zoglin', 'drowned', 'phantom', 'spider'):
        undead = kind not in ('pig', 'spider')
        for effect in ('instant_health', 'instant_damage'):
            healing = (effect == 'instant_health') != undead
            for route in ('command', 'cloud'):
                clean()
                uid = entity(kind)
                if route == 'command':
                    check(f'/effect give {uid} {effect} 1 0 true')
                    read(uid, 'active_effects[0].duration')
                    step(1)
                    health(uid, 24 if healing else 14, 0 if healing else 10, cloud=False)
                    check(f'/execute unless entity @e[nbt={{UUID:[I;0,250,0,{serial}],'
                          'active_effects:[{id:"minecraft:' + effect + '"}]}]')
                else:
                    c = cloud(effect)
                    outside = delayed = None
                    if kind == 'pig' and effect == 'instant_health':
                        outside = entity('pig', 12.5)
                        delayed = cloud('instant_damage', 'WaitTime:11')
                    step(10)
                    health(uid, 22 if healing else 17, 0 if healing else 5,
                           armored=kind in ('zombie', 'husk', 'drowned'))
                    read(c, 'Age')
                    read(c, 'Radius')
                    check(f'/execute as {c} if entity @s[nbt={{Age:10,Radius:3f}}]')
                    if outside:
                        health(outside, 20, 0)
                        read(delayed, 'Age')
                        check(f'/execute as {delayed} if entity @s[nbt={{Age:10,WaitTime:11}}]')
                    check(f'/kill {c}')
                    check(f'/execute unless entity {c}')

    # Settings that the natural-cloud tick previously hardcoded. Each row uses
    # a fresh actor/listener, with checks at the dose/removal boundaries.
    cases = [
        ('', [(4, 20, True), (5, 22, True), (10, 22, True)]),
        ('WaitTime:6', [(5, 20, True), (6, 20, True), (10, 22, True)]),
        ('ReapplicationDelay:6', [(5, 22, True), (10, 22, True), (15, 24, True)]),
        ('ReapplicationDelay:5', [(5, 22, True), (10, 24, True)]),
        ('Radius:1f,RadiusOnUse:-0.5f,ReapplicationDelay:5',
         [(5, 22, True), (10, 24, False)]),
        ('Radius:1f,RadiusPerTick:-0.125f', [(4, 20, True), (5, 20, False)]),
        ('Duration:6,WaitTime:2', [(7, 22, True), (8, 22, False)]),
        ('Duration:20,DurationOnUse:-5,ReapplicationDelay:5',
         [(5, 22, True), (10, 24, True), (11, 24, False)]),
    ]
    for extra, checkpoints in cases:
        clean()
        uid = entity('pig')
        # Center the actor in the cloud for the shrinking-radius boundary.
        out.extend(setup(f'/tp {uid} 4 200 4'))
        c = cloud(extra=extra)
        elapsed = 0
        for age, value, alive in checkpoints:
            step(age - elapsed)
            elapsed = age
            health(uid, value, 0)
            check(f'/execute {"if" if alive else "unless"} entity {c}')
            if alive:
                for field in ('Age', 'Radius', 'Duration'):
                    read(c, field)
    for effect in ('instant_health', None):
        clean()
        uid = entity('pig')
        c = cloud(effect, defaults=True)
        elapsed = 0
        for age in (19, 20, 30):
            step(age - elapsed)
            elapsed = age
            health(uid, 22 if effect and age >= 20 else 20, 0)
            check(f'/execute as {c} if entity @s[nbt={{Age:{age},Duration:-1,WaitTime:20}}]')
    for scale, duration in ((1, 40), (0.5, 20)):
        clean()
        uid = entity('pig')
        cloud('glowing', f'potion_duration_scale:{scale}f')
        step(5)
        read(uid, 'active_effects[0].duration', ('dose_duration',))
        check(f'/execute as {uid} if entity @s[nbt={{active_effects:[{{'
              f'id:"minecraft:glowing",duration:{duration}}}]}}]', categories=('dose_duration',))
        step(5)
        read(uid, 'active_effects[0].duration', ('dose_duration',))
        check(f'/execute as {uid} if entity @s[nbt={{active_effects:[{{'
              f'id:"minecraft:glowing",duration:{duration-5}}}]}}]', categories=('dose_duration',))
    for options in ({'potion': 'healing'}, {'amplifier': 32}):
        clean()
        uid = entity('pig')
        cloud(**options)
        step(5)
        health(uid, 22, 0)
    clean()
    out.extend(setup('/tick unfreeze'))
    out.append({'op': 'tick', 'n': 25})
    out.extend(setup('/kill @e[tag=cloud_subject]', '/kill @e[type=item]',
                     '/fill 0 199 0 15 199 15 air', '/time set 1000',
                     '/scoreboard objectives remove audit', '/forceload remove 0 0 15 15'))
    if summary is not None:
        selected = sum(o['op'] == 'cmd' and o.get('record', True) for o in out)
        summary.update(selected=selected, excluded=excluded,
                       category_memberships=categories_seen,
                       overlap=sum(categories_seen.values()) - excluded if excluded else 0)
    return out


if __name__ == '__main__':
    import sys
    import command_exec_corpus as corpus
    import test_command_exec_parity as runner

    contract = 'full' if '--full' in sys.argv else 'loader'
    if '--full' in sys.argv:
        sys.argv.remove('--full')
    selection = {}
    operations = loaded_clouds(corpus.cmd, corpus.setup, contract, selection)
    print(f'{contract} contract: {selection}', flush=True)
    runner.corpus_module.groups = lambda: [
        ('setup', corpus.group_setup()), ('loaded potion clouds', operations)]
    runner.main()
