"""Accepted owner trades deliver reputation at the villager's actual AI phase.

The live session is the uses/XP witness. Optional live saved-trade observations
retain the separate H102 projection diagnostic; they are never loader claims.
All packet streams, including prior windows, are retained independently.
"""
from command_exec_corpus import cmd

OWNER = [-1608632253, -450807400, -1786055591, -2146546415]


def cases():
    ops = []
    def c(text, expected=None):
        ops.append(cmd(text, expect_result=expected))
    def action(kind, **kw):
        ops.append({'op': 'merchant_action', 'action': kind, **kw})
    c('/tick freeze')
    c('/forceload add 0 0 31 31')
    ops.append({'op': 'wait_chunks', 'chunks': [[0, 0], [0, 1], [1, 0], [1, 1]]})
    for x,z in [(0,0),(0,16),(16,0),(16,16)]:
        c(f'/execute if loaded {x} 200 {z}', 1)
    c('/fill 0 199 0 31 199 31 stone', 1024)
    c('/fill 0 200 0 31 204 31 air')
    c('/gamemode survival @s')
    c('/scoreboard objectives add merchant_gate dummy')
    c('/data get entity @s UUID', 4)
    c('/execute if entity @s[nbt={UUID:[I;' + ','.join(map(str, OWNER)) + '],Health:20f}]', 1)
    elapsed = 0
    def clock():
        c('/execute store result score #now merchant_gate run time query gametime')
        c('/scoreboard players operation #now merchant_gate -= #base merchant_gate')
        c('/scoreboard players get #now merchant_gate', elapsed)
    c('/execute store result score #base merchant_gate run time query gametime')
    window = 0
    # One trade, frozen/NoAI delay, two-trade coalescing, and no-trade control.
    for n, (no_ai, trades, close_first) in enumerate([
            (False, 1, False), (True, 1, True),
            (False, 2, False), (True, 2, True), (True, 0, True)]):
        uuid = f'00000067-0000-0001-0000-{n+1:012x}'
        x = 6.5 + n*4
        tag = f'merchant_gate_{n}'
        c('/clear @s')
        c(f'/tp @s {x} 200 3.5 0 0')
        c(f'/execute if entity @s[gamemode=survival,nbt={{Pos:[{x}d,200d,3.5d],Health:20f}}]', 1)
        if trades:
            c(f'/item replace entity @s hotbar.0 with emerald {trades*4}', 1)
            c(f'/clear @s emerald 0', trades*4)
        nbt = ('{UUID:[I;103,1,0,' + str(n+1) + f'],Tags:["{tag}"],CustomName:"Trader{n}",'
               f'NoAI:{int(no_ai)}b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:20f,'
               'VillagerData:{profession:"minecraft:cleric",level:2,type:"minecraft:plains"},Xp:20,'
               'Offers:{Recipes:[{buy:{id:"minecraft:emerald",count:4},sell:{id:"minecraft:glass",count:3},'
               'uses:0,maxUses:100,rewardExp:0b,xp:1,priceMultiplier:1f,demand:0,specialPrice:0}]}}')
        c(f'/summon villager {x} 200 5.5 {nbt}')
        def actor(ai):
            c(f'/execute if entity @e[type=villager,tag={tag},nbt={{UUID:[I;103,1,0,{n+1}],Health:20f,Pos:[{x}d,200d,5.5d],NoGravity:1b,Silent:1b,PersistenceRequired:1b}}]', 1)
            c(f'/data get entity {uuid} UUID', 4)
            c(f'/data get entity {uuid} Pos', 3)
            c(f'/execute {"if" if ai else "unless"} entity @e[tag={tag},nbt={{NoAI:1b}}]', 1)
            c(f'/execute if block {int(x)} 199 5 stone', 1)
        def gossip(value):
            c(f'/data get entity {uuid} Gossips', 1 if value else 0)
            if value:
                c(f'/execute if data entity {uuid} Gossips[{{Type:"trading",Value:{value},Target:[I;' + ','.join(map(str,OWNER)) + ']}]', 1)
            else:
                c(f'/execute unless data entity {uuid} Gossips[{{Type:"trading"}}]', 1)
        def state(label, count, price=0, payment=0, result=False, cursor=False):
            ops.append({'op':'merchant_state','label':f'{n}-{label}', 'expect_merchant':{
                'merchant':[103,1,0,n+1], 'player':OWNER, 'window':window,
                'xp':20+count, 'slots':[['minecraft:emerald',payment] if payment else None,None,
                    ['minecraft:glass',3] if result else None],
                'carried':['minecraft:glass',3] if cursor else None,
                'offers':[{'uses':count,'special':price,'cost':4+price}]}})
        def open_menu():
            nonlocal window
            action('open', uuid=uuid)
            window += 1
        def pump(real=0):
            nonlocal elapsed
            if real:
                c(f'/tick step {real}')
            ops.append({'op':'tick','n':real or 1})
            elapsed += real
            clock()
        actor(no_ai); gossip(0); clock()
        open_menu(); state('open',0)
        if trades:
            action('select',index=0)
            state('selected',0,payment=4*trades,result=True)
            for t in range(trades):
                action('click',slot=2)
                state(f'take-{t}',t+1,payment=4*(trades-t-1),result=t+1<trades,cursor=True)
                action('click',slot=31)
                state(f'stored-{t}',t+1,payment=4*(trades-t-1),result=t+1<trades)
                c('/clear @s glass 0',3*(t+1))
                gossip(0); clock()
        pump(); actor(no_ai); gossip(0)
        state('frozen-pump',trades)
        # Closing must not deliver. Reopening must not invent a second event.
        action('close'); gossip(0); clock()
        c(f'/data get entity {uuid} Xp',20+trades)
        c(f'/data get entity {uuid} Offers.Recipes[0].uses',trades if trades else None)
        if not trades:
            ops[-1]['expect_callback'] = [False, 0]
        open_menu(); state('reopened-before-ai',trades)
        if no_ai:
            pump(1); actor(True); gossip(0)
            state('noai-real-tick',trades)
            action('close'); gossip(0)
            c(f'/data merge entity {uuid} {{NoAI:0b}}')
            actor(False); gossip(0)
            pump(); gossip(0)
            open_menu(); state('ai-enabled-before-tick',trades)
        pump(1); actor(False); gossip(2 if trades else 0)
        state('eligible-ai',trades,price=-2 if trades else 0)
        pump(1); gossip(2 if trades else 0)
        state('no-repeat',trades,price=-2 if trades else 0)
        action('close')
        open_menu(); state('reopened-after-ai',trades,price=-2 if trades else 0)
        action('close'); clock()
    return ops
