"""Real connected-owner trade extractions and crafted statistics, without world ticks."""
from command_exec_corpus import cmd

OWNER = [-1608632253, -450807400, -1786055591, -2146546415]


def cases():
    ops = []
    def c(text, expected=None, record=True):
        ops.append(cmd(text, expect_result=expected, record=record))
    def action(action, **kw):
        ops.append({'op': 'merchant_action', 'action': action, **kw})
    c('/tick freeze')
    c('/gamerule minecraft:spawn_mobs false')
    c('/forceload add 0 0 31 31')
    c('/tp @s 8.5 200 8.5 0 0')
    ops.append({'op': 'wait_chunks', 'chunks': [[0, 0], [0, 1], [1, 0], [1, 1]]})
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [0, 1], [1, 0], [1, 1]]})
    for x, z in [(0, 0), (0, 16), (16, 0), (16, 16)]:
        c(f'/execute if loaded {x} 200 {z}', 1)
    c('/fill 0 199 0 31 199 31 stone', 1024)
    c('/fill 0 200 0 31 204 31 air')
    c('/gamemode survival @s')
    for name, criterion in [('audit', 'dummy'), ('books', 'minecraft.crafted:minecraft.bookshelf'),
                            ('glass', 'minecraft.crafted:minecraft.glass'),
                            ('trades', 'minecraft.custom:minecraft.traded_with_villager')]:
        c(f'/scoreboard objectives add {name} {criterion}')
    for objective in ['books', 'glass', 'trades']:
        c(f'/scoreboard players set @s {objective} 0', 0)
    c('/data get entity @s UUID', 4)
    c('/execute if entity @s[nbt={UUID:[I;' + ','.join(map(str, OWNER)) + '],Health:20f}]', 1)
    c('/execute store result score #base audit run time query gametime', record=False)
    def clock():
        c('/execute store result score #now audit run time query gametime', record=False)
        c('/scoreboard players operation #now audit -= #base audit')
        c('/scoreboard players get #now audit', 0)
    def stats(books, glass, trades):
        for name, count in [('books', books), ('glass', glass), ('trades', trades)]:
            c(f'/scoreboard players get @s {name}', count)
    window = 0
    # The last two cases admit a real menu/offer but never an accepted take.
    for n, (item, output, payment, take, count) in enumerate([
        ('bookshelf', 1, 4, 'single', 1), ('glass', 3, 8, 'quick', 2),
        ('bookshelf', 1, 4, 'preview', 0), ('bookshelf', 1, 3, 'rejected', 0),
    ]):
        uuid = f'00000065-0000-0001-0000-{n+1:012x}'
        tag = f'trade_crafted_{n}'
        x = 6.5 + 4*n
        c('/clear @s')
        c(f'/tp @s {x} 200 3.5 0 0')
        c(f'/execute if entity @s[gamemode=survival,nbt={{Health:20f,Pos:[{x}d,200d,3.5d]}}]', 1)
        c(f'/item replace entity @s hotbar.0 with emerald {payment}', 1)
        c(f'/clear @s emerald 0', payment)
        nbt = ('{UUID:[I;101,1,0,' + str(n+1) + f'],Tags:["{tag}"],CustomName:"Trader{n}",'
               'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:20f,'
               'VillagerData:{profession:"minecraft:cleric",level:2,type:"minecraft:plains"},Xp:20,'
               'Offers:{Recipes:[{buy:{id:"minecraft:emerald",count:4},sell:{id:"minecraft:' + item +
               f'",count:{output}' + '},uses:0,maxUses:100,rewardExp:0b,xp:1,priceMultiplier:0f,demand:0,specialPrice:0}]}}')
        c(f'/summon villager {x} 200 5.5 {nbt}')
        c(f'/execute if entity @e[type=villager,tag={tag},nbt={{UUID:[I;101,1,0,{n+1}],Health:20f,Pos:[{x}d,200d,5.5d],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b}}]', 1)
        c(f'/data get entity {uuid} UUID', 4)
        c(f'/data get entity {uuid} Pos', 3)
        c(f'/execute if block {int(x)} 199 5 stone', 1)
        clock()
        action('open', uuid=uuid)
        window += 1
        def state(label, uses, pay, result, carried=None):
            ops.append({'op': 'merchant_state', 'label': f'{n}-{label}', 'offer_packets': True, 'expect_merchant': {
                'window': window, 'merchant': [101, 1, 0, n+1], 'player': OWNER,
                'slots': [['minecraft:emerald', pay] if pay else None, None,
                          [f'minecraft:{item}', output] if result else None],
                'carried': carried, 'xp': 20+uses,
                # With the world frozen, notifyTrade updates the offer but the
                # deferred AI reputation event has not sent another packet.
                'sent_offers': [{'uses': 0, 'special': 0, 'demand': 0, 'cost': 4}],
                'offers': [{'uses': uses, 'special': 0, 'demand': 0, 'cost': 4}]}})
        state('open', 0, 0, False)
        action('select', index=0)
        state('selected', 0, payment, payment >= 4)
        stats(1 if n else 0, 6 if n >= 2 else 0, 3 if n >= 2 else (1 if n else 0))
        if take != 'preview':
            action('click', slot=2, input=1 if take == 'quick' else 0)
        if take == 'single':
            state('taken', 1, 0, False, ['minecraft:bookshelf', 1])
            action('click', slot=31)
            state('stored', 1, 0, False)
        else:
            state('after', count, payment-4*count, take == 'preview')
        c(f'/clear @s {item} 0', output*count if count else None)
        if not count:
            ops[-1]['expect_callback'] = [False, 0]
        remaining = payment-4*count
        c('/clear @s emerald 0')
        ops[-1]['expect_callback'] = [False, 0]
        stats(1, 6 if n else 0, 3 if n else 1)
        clock()
        action('close')
        c('/clear @s emerald 0', remaining if remaining else None)
        if not remaining:
            ops[-1]['expect_callback'] = [False, 0]
        ops.append({'op':'merchant_saved_stats','label':str(n), 'expect_observation':{
            'uuid':OWNER,'books':1,'glass':6 if n else 0,'trades':3 if n else 1}})
        # Closing and a second read must not count the already extracted result again.
        stats(1, 6 if n else 0, 3 if n else 1)
        clock()
    return ops
