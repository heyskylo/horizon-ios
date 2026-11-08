"""Host-only advancement criteria before grant/after grant/after revoke; zero ticks."""
ROOT = 'minecraft:story/root'
MISSING = 'h266_missing'
UUID = [-1608632253, -450807400, -1786055591, -2146546415]


def criterion_selectors(full=False):
    ops=[]
    def cmd(text,label,result=None,callback=None,admit=False,diagnostic=False):
        op={'op':'cmd','cmd':text,'label':label,'admission':admit}
        if result is not None: op['expect_result']=result
        if callback is not None: op['expect_callback']=callback
        if diagnostic and not full: op.update(record=False,trace=True)
        ops.append(op)
    cmd('/tick freeze','freeze',admit=True)
    for rule,value in [('spawn_mobs','false'),('advance_time','false'),('random_tick_speed','0'),('announce_advancements','false')]:
        cmd(f'/gamerule {rule} {value}','rule-'+rule,admit=True)
    cmd('/forceload add 0 0','force',1,admit=True)
    ops.append({'op':'settle','required_chunks':[[0,0]]})
    cmd('/fill 0 99 0 15 99 15 stone','floor',256,admit=True)
    cmd('/tp @s 4.5 100 4.5 0 0','owner-position',1,admit=True)
    cmd('/gamemode creative @s','owner-mode',admit=True)
    cmd('/clear @s','initial-clear',diagnostic=True)
    cmd(f'/advancement revoke @s only {ROOT}','initial-revoke',diagnostic=True)
    cmd('/summon pig 8.5 100 8.5 {UUID:[I;266,0,0,1],Tags:["h266_nonplayer"],NoAI:1b,NoGravity:1b}','pig',1,admit=True)
    for objective in ['h266','h266_clock']:
        cmd(f'/scoreboard objectives add {objective} dummy','objective-'+objective,admit=True)
    cmd('/execute store result score start h266_clock run time query gametime','clock-start',diagnostic=True)
    queries=[
        ('known_true',f'{ROOT}={{crafting_table=true}}',lambda done:done),
        ('known_false',f'{ROOT}={{crafting_table=false}}',lambda done:not done),
        ('unknown_true',f'{ROOT}={{{MISSING}=true}}',lambda done:False),
        ('unknown_false',f'{ROOT}={{{MISSING}=false}}',lambda done:False),
        ('done_true',f'{ROOT}=true',lambda done:done),
        ('done_false',f'{ROOT}=false',lambda done:not done),
        ('empty_criteria',f'{ROOT}={{}}',lambda done:True),
        ('missing_advancement','h266:missing=false',lambda done:False),
    ]
    def action(selector,key,match,admit=False):
        for sign in ['if','unless']:
            label=key+'-'+sign;take=match if sign=='if' else not match
            for holder in ['result','success','branch']:
                cmd(f'/scoreboard players set {holder} h266 -99',label+'-reset-'+holder,-99,admit=True)
            cmd(f'/execute store result score result h266 store success score success h266 {sign} entity {selector} run scoreboard players set branch h266 1',label+'-action',callback=[True,1] if take else None,admit=admit)
            ops[-1]['expect_results']=[[True,1]] if take else []
            for holder in ['result','success','branch']:
                cmd(f'/scoreboard players get {holder} h266',label+'-'+holder,1 if take else -99,admit=admit)
    for phase,done in [('before',False),('granted',True),('revoked',False)]:
        if phase=='granted':
            cmd(f'/advancement grant @s only {ROOT} crafting_table','actual-grant',1,admit=True)
            cmd(f'/advancement grant @s only {ROOT} crafting_table','grant-noop',callback=[False,0],admit=True)
        elif phase=='revoked':
            cmd(f'/advancement revoke @s only {ROOT} crafting_table','actual-revoke',1,admit=True)
        cmd(f'/advancement {"grant" if done else "revoke"} @s only {ROOT} crafting_table',phase+'-progress-noop',callback=[False,0],admit=True)
        cmd('/execute if entity @s[type=minecraft:player,name=Player]',phase+'-owner',1,admit=True)
        for field,result in [('UUID',4),('Health',20),('Pos',3),('playerGameType',1),('Inventory',0)]:
            cmd(f'/data get entity @s {field}',phase+'-'+field,result,admit=True)
        cmd('/execute if loaded 4 100 4',phase+'-loaded',1,admit=True)
        cmd('/execute if block 4 99 4 stone',phase+'-floor',1,admit=True)
        cmd('/execute if entity @e[type=pig,tag=h266_nonplayer]',phase+'-pig-positive',1,admit=True)
        for field,result in [('UUID',4),('Health',10),('NoAI',1),('NoGravity',1),('Pos',3)]:
            cmd(f'/data get entity @e[type=pig,tag=h266_nonplayer,limit=1] {field}',phase+'-pig-'+field,result,admit=True)
        cmd('/execute store result score now h266_clock run time query gametime',phase+'-clock-raw',diagnostic=True)
        cmd('/scoreboard players operation now h266_clock -= start h266_clock',phase+'-clock-delta-op',0,admit=True)
        cmd('/scoreboard players get now h266_clock',phase+'-clock-delta',0,admit=True)
        for name,condition,expected in queries:
            action('@s[advancements={'+condition+'}]',phase+'-'+name,expected(done),admit=name!='unknown_false')
        if phase=='before':
            for name,condition in [('missing_true','h266:missing=true'),('missing_empty','h266:missing={}')]:
                action('@s[advancements={'+condition+'}]',phase+'-'+name,False,admit=True)
            action('@e[type=pig,tag=h266_nonplayer,advancements={'+ROOT+'={}}]',phase+'-nonplayer',False,admit=True)
    return ops
