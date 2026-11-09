"""OFFLINE V1: immutable packet/ACK + independent committed body arrival binding.
No engines. No runtime, observer, production or original report mutation.
Yaw/pitch are exclusively target facts, never candidate selection inputs.
"""
import argparse, copy, datetime, gzip, hashlib, io, json, math, re, struct, sys, uuid
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
PARSER = Path(__file__).with_name('spawnpoint_respawn_snbt_v1.py')
import spawnpoint_respawn_snbt_v1 as snbt
import anvil_dump

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def require(value, message):
    if not value: raise ValueError(message)
def fbits(value): return struct.pack('>f',value).hex()
def dbits(values): return [struct.pack('>d',v).hex() for v in values]

class Fields(snbt.Snbt):
    """Reuse complete original parser; retain root lexical spans for typed facts."""
    def __init__(self,text):
        super().__init__(text); self.pending=None; self.fields={}
    def compound_key(self,allow_resource_keys=False):
        key=super().compound_key(allow_resource_keys)
        if self.depth==1: self.pending=key
        return key
    def value(self,allow_resource_keys=False):
        key=self.pending if self.depth==1 else None
        if key is not None: self.pending=None
        start=self.position
        value=super().value(allow_resource_keys)
        if key is not None: self.fields[key]=self.text[start:self.position].strip()
        return value

def parsed(text):
    p=Fields(text); value=p.value(); p.whitespace()
    require(p.position==len(text),'incomplete full body SNBT consumption')
    return value,p.fields

def typed_list(raw,suffix,count):
    require(raw.startswith('[') and raw.endswith(']'),'typed list absent')
    tokens=raw[1:-1].split(',')
    pattern=r'[-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?'+suffix
    require(len(tokens)==count and all(re.fullmatch(pattern,t.strip()) for t in tokens),'typed list representation differs')
    values=[float(t.strip()[:-1]) for t in tokens]
    require(all(math.isfinite(v) for v in values),'nonfinite body list')
    return values

def vint(raw,at=0):
    val=0
    for shift in range(0,35,7):
        require(at<len(raw),'truncated VarInt'); b=raw[at];at+=1;val|=(b&127)<<shift
        if not b&128:return val-(1<<32) if val&(1<<31) else val,at
    raise ValueError('oversized VarInt')

def packet(frame,index):
    raw=bytes.fromhex(frame['data']); nonce,off=vint(raw)
    if frame['name']=='player_position':
        require(len(raw)-off==60,'position enclosing payload consumption')
        xyzdelta=struct.unpack('>dddddd',raw[off:off+48]); relative=struct.unpack('>i',raw[-4:])[0]
        require(relative==0 and all(math.isfinite(v) for v in xyzdelta),'unsupported relative/nonfinite position')
        angles=raw[off+48:off+56]
    else:
        require(len(raw)-off==32,'ACK enclosing payload consumption')
        xyzdelta=struct.unpack('>ddd',raw[off:off+24]);angles=raw[off+24:]
    return dict(index=index,id=nonce,pos=list(xyzdelta[:3]),pos_bits=dbits(xyzdelta[:3]),yaw_bits=angles[:4].hex(),pitch_bits=angles[4:].hex(),payload=frame['data'],t=frame['t'])

def body(commands,phase):
    markers=[i for i,r in enumerate(commands) if r['command'].startswith('data modify storage respawnproof:proof '+phase+'.identity set value ')]
    require(len(markers)==1,'missing/ambiguous phase marker '+phase)
    start=markers[0]
    candidates=[(i,r) for i,r in enumerate(commands) if i>start and r['command']=='data get entity Alice']
    require(candidates,'missing independent committed body '+phase)
    i,row=candidates[0]
    matches=[s.split('Alice has the following entity data: ',1)[1] for s in row['lines'] if 'Alice has the following entity data: ' in s]
    require(len(matches)==1,'missing/ambiguous full body reply '+phase)
    value,fields=parsed(matches[0])
    require(fields['UUID'].startswith('[I;') and len(value['UUID'])==4,'body UUID typed array')
    actual_uuid=str(uuid.UUID(bytes=b''.join(struct.pack('>i',x) for x in value['UUID'])))
    pos=typed_list(fields['Pos'],'[dD]',3)
    rotation=typed_list(fields['Rotation'],'[fF]',2)
    return dict(index=i,phase_start=start,receipt_at=row['receipt_at'],sent_at=row['sent_at'],uuid=actual_uuid,pos=pos,pos_bits=dbits(pos),rotation_bits=[fbits(x) for x in rotation],value=value,fields=fields,full_snbt_sha256=hashlib.sha256(matches[0].encode()).hexdigest())

def saved(path):
    raw=path.read_bytes();data=gzip.decompress(raw) if raw[:2]==b'\x1f\x8b' else raw
    f=io.BytesIO(data);require(f.read(1)==b'\x0a','saved root not compound'); n=struct.unpack('>H',f.read(2))[0]; require(len(f.read(n))==n,'root name truncated')
    value=anvil_dump.read_tag(f,10);require(f.read()==b'','saved enclosing payload consumption');return value

def load(directory,prefix):
    directory=Path(directory)
    paths={k:directory/(prefix+s) for k,s in [('frames','-packets.jsonl'),('commands','-commands.json'),('transcript','-client.txt'),('partial','-partial.json'),('saved','-configured-player.dat')]}
    return dict(frames=[json.loads(s) for s in paths['frames'].read_text().splitlines()],commands=json.loads(paths['commands'].read_text()),transcript=paths['transcript'].read_text(),partial=json.loads(paths['partial'].read_text()),saved=saved(paths['saved']),inputs={str(p):sha(p) for p in paths.values()})

def extract(data):
    frames=data['frames'];commands=data['commands'];text=data['transcript']
    require(not re.search(r'bytes left|larger than expected|\berror\b',text),'original official decoder failure')
    loginrows=[json.loads(s.split(' spawn-wire ',1)[1]) for s in text.splitlines() if ' spawn-wire ' in s]
    loginrows=[r for r in loginrows if r['kind']=='login'];require(len(loginrows)==1,'official login mapping absent/ambiguous')
    login=loginrows[0]
    profiles=[r for r in frames if r.get('name')=='login_finished' and r['dir']=='s2c'];require(len(profiles)==1,'login profile packet absent/ambiguous')
    profile=profiles[0];raw=bytes.fromhex(profile['data']);require(len(raw)>=17,'raw UUID truncated')
    actual_uuid=str(uuid.UUID(bytes=raw[:16]));n,off=vint(raw,16);require(n>=0 and off+n<=len(raw),'profile name truncated')
    actual_name=raw[off:off+n].decode();require(actual_uuid==login['profile_uuid'] and actual_name==login['profile_name']=='Alice','official/raw login identity differs')
    playlogins=[r for r in frames if r.get('name')=='login' and r['dir']=='s2c' and r['state']=='play'];require(len(playlogins)==1,'play login absent/ambiguous')
    playlogin=playlogins[0];encoded=bytes.fromhex(login['payload_hex']);_,offset=vint(encoded)
    require(encoded[offset:]==bytes.fromhex(playlogin['data']),'official full login payload not raw-bound')
    require(struct.unpack('>i',encoded[offset:offset+4])[0]==login['entity_id'],'official/raw entity id differs')
    conn=profile['conn'];require(playlogin['conn']==conn and {r['conn'] for r in frames if r['state']=='play'}=={conn},'wrong identity/connection stream')
    requests=[i for i,r in enumerate(frames) if r.get('name')=='client_command' and r['dir']=='c2s' and r['state']=='play' and r['data']=='00'];require(len(requests)==1,'real respawn request absent/ambiguous')
    request=requests[0]
    deaths=[i for i,r in enumerate(frames) if r.get('name')=='player_combat_kill' and i<request];require(deaths,'no actual death packet before request')
    respawns=[i for i,r in enumerate(frames) if r.get('name')=='respawn' and i>request];require(len(respawns)==1,'Respawn window absent/ambiguous');respawn=respawns[0]
    loaded=[i for i,r in enumerate(frames) if r.get('name')=='player_loaded' and r['dir']=='c2s' and i>respawn];require(len(loaded)==1 and frames[loaded[0]]['data']=='','bounded loaded receipt absent/ambiguous');end=loaded[0]
    require(0<=frames[end]['t']-frames[request]['t']<=30,'wire completion exceeds original bound')
    committed=body(commands,'revived');dead=body(commands,'dead');configured=body(commands,'configured')
    kills=[i for i,r in enumerate(commands) if r['command']=='kill Alice'];require(len(kills)==1 and kills[0]<dead['index']<committed['phase_start']<committed['index'],'independent body source phase order')
    # Frozen run_side calls spawn-loaded + sync BEFORE snapshot(revived); commands then query
    # the full body. No snapshot numeric values participate in this selection.
    elapsed=(datetime.datetime.fromisoformat(committed['receipt_at'])-datetime.datetime.fromisoformat(commands[kills[0]]['sent_at'])).total_seconds()
    require(0<=elapsed<=30,'committed command receipt outside bounded death/arrival window')
    for b in [dead,committed,configured]:require(b['uuid']==actual_uuid,'committed body UUID differs from actual login')
    require(dead['value']['Health']==0 and committed['value']['Health']==20 and committed['value']['playerGameType']==0 and committed['value']['Dimension']=='minecraft:overworld','independent death/revived body admission')
    allpositions=[packet(r,i) for i,r in enumerate(frames) if r.get('name')=='player_position' and r['dir']=='s2c']
    acknowledgements=[packet(r,i) for i,r in enumerate(frames) if r.get('name')=='accept_teleportation' and r['dir']=='c2s']
    oldids={p['id'] for p in allpositions if p['index']<request};require(oldids,'no pre-request teleport identity')
    positions=[p for p in allpositions if respawn<p['index']<end]
    chunks=[]
    for i,r in enumerate(frames):
        if respawn<i<end and r.get('name')=='level_chunk_with_light':
            raw=bytes.fromhex(r['data']);require(len(raw)>=8,'chunk coordinate payload truncated');chunks.append(dict(index=i,column=list(struct.unpack('>ii',raw[:8]))))
    center=[math.floor(committed['pos'][j])//16 for j in (0,2)]
    required={(center[0]+x,center[1]+z) for x in range(-2,3) for z in range(-2,3)}
    require(required<={tuple(c['column']) for c in chunks},'post-Respawn body-centered 25-column load witness missing')
    candidates=[]
    for p in positions:
        acks=[a for a in acknowledgements if p['index']<a['index']<end and a['id']==p['id'] and a['pos_bits']==p['pos_bits']]
        p['matching_ack_indices']=[a['index'] for a in acks]
        p['new_identity']=p['id'] not in oldids
        p['matches_actual_committed_body_position']=p['pos_bits']==committed['pos_bits']
        if p['new_identity'] and p['matches_actual_committed_body_position'] and len(acks)==1:candidates.append((p,acks[0]))
    require(len(candidates)==1,'no unique raw position/new identity/matching ACK/committed-body candidate')
    candidate,ack=candidates[0]
    # ONLY HERE do angle values enter target comparison. Corruption cannot pick another frame.
    config,configfields=parsed(configured['fields']['respawn'])
    pitch_literal=configfields.get('pitch','');config_pitch=(fbits(float(pitch_literal[:-1])) if re.fullmatch(r'[-+]?\d+(?:\.\d+)?[fF]',pitch_literal) else None)
    saved_value=data['saved'];require(saved_value['UUID']==('I',configured['value']['UUID']),'saved actual UUID binding')
    saved_pitch=saved_value['respawn'].get('pitch')
    targets=dict(configured_pitch=config_pitch==fbits(10),saved_config_pitch=saved_pitch==('f',10.0),committed_rotation=committed['rotation_bits']==[fbits(45),fbits(10)],client_rotation=[candidate['yaw_bits'],candidate['pitch_bits']]==[fbits(45),fbits(10)])
    return dict(admitted=True,identity=dict(uuid=actual_uuid,entity_id=login['entity_id'],connection=conn),window=dict(request_index=request,respawn_index=respawn,loaded_index=end,wire_elapsed=frames[end]['t']-frames[request]['t'],death_to_committed_receipt_seconds=elapsed),committed_body=dict(index=committed['index'],receipt_at=committed['receipt_at'],position=committed['pos'],position_bits=committed['pos_bits'],rotation_bits=committed['rotation_bits'],full_snbt_sha256=committed['full_snbt_sha256']),selected=candidate,ack=ack,ack_angles_match_selected=[ack['yaw_bits'],ack['pitch_bits']]==[candidate['yaw_bits'],candidate['pitch_bits']],targets=targets,all_position_frames=allpositions,post_respawn_positions=positions,provisional_extra_count=len(positions)-1,post_respawn_chunks=chunks,original_admission_receipt_count=len(data['partial']['admissions']),original_gates_unchanged=data['partial']['gates'])

def outcome(data):
    try:return extract(data)
    except (ValueError,KeyError,IndexError,struct.error) as e:return dict(admitted=False,error=str(e))

def main(argv=None):
    parser=argparse.ArgumentParser(description='V1 OFFLINE reporting only: existing complete captures; starts no engines.')
    parser.add_argument('--java-dir',type=Path,required=True)
    parser.add_argument('--pristine-dir',type=Path,required=True)
    parser.add_argument('--fixed-dir',type=Path,required=True)
    parser.add_argument('--output-dir',type=Path,required=True)
    parser.add_argument('--frozen-program',type=Path,required=True)
    args=parser.parse_args(argv)
    OUT=args.output_dir
    OUT.mkdir(parents=True,exist_ok=False)
    inputs={label:load(folder,prefix) for label,folder,prefix in [('java',args.java_dir,'java'),('pristine',args.pristine_dir,'native'),('fixed_v1',args.fixed_dir,'native')]}
    reports={label:outcome(data) for label,data in inputs.items()}
    controls={}
    base=inputs['fixed_v1'];baseline=reports['fixed_v1'];require(baseline['admitted'],'fixed offline prerequisite unavailable')
    selected=baseline['selected']['index'];ack_index=baseline['ack']['index']
    def check(name,mutate,predicate):
        data=copy.deepcopy(base);mutate(data);result=outcome(data);require(predicate(result),'synthetic control failed: '+name);controls[name]=dict(passed=True,result=result if not result['admitted'] else dict(admitted=True,selected_index=result['selected']['index'],selected_id=result['selected']['id'],targets=result['targets']))
    check('early_provisional_snapshot_cannot_select',lambda d:d.update(transcript=d['transcript']+'\nAlice step=synthetic spawn-wire '+json.dumps(dict(kind='arrival-pending',teleport_id=2,position=[12.5,100.1,12.5],yaw_bits=1110704128,pitch_bits=1092616192))),lambda r:r['admitted'] and r['selected']['index']==selected)
    check('missing_final_ACK',lambda d:d['frames'].pop(ack_index),lambda r:not r['admitted'])
    def wrong_ack(d):d['frames'][ack_index]['data']='7f'+d['frames'][ack_index]['data'][2:]
    check('wrong_final_ACK_identity',wrong_ack,lambda r:not r['admitted'])
    def missing_body(d):
        row=d['commands'][baseline['committed_body']['index']];row['lines']=[]
    check('missing_committed_body',missing_body,lambda r:not r['admitted'])
    def corrupt_pitch(d):
        frame=d['frames'][selected];raw=bytearray.fromhex(frame['data']);_,off=vint(raw);raw[off+52:off+56]=struct.pack('>f',0);frame['data']=raw.hex()
    check('raw_final_pitch_corruption_is_target_failure',corrupt_pitch,lambda r:r['admitted'] and r['selected']['index']==selected and not r['targets']['client_rotation'] and not r['ack_angles_match_selected'])
    def stale(d):
        f=d['frames'].pop(selected);i=baseline['window']['request_index'];f['t']=d['frames'][i-1]['t'];d['frames'].insert(i,f)
    check('stale_pre_request_matching_frame',stale,lambda r:not r['admitted'])
    check('wrong_identity_connection',lambda d:d['frames'][selected].update(conn=999),lambda r:not r['admitted'])
    def manufactured(d):
        d['frames'].pop(selected)
        d['transcript']+='\nAlice step=synthetic spawn-wire '+json.dumps(dict(kind='arrival-pending',teleport_id=3,position=[12.5,100.1,12.5],yaw_bits=1110704128,pitch_bits=1092616192))
    check('expected_like_torn_snapshot_cannot_manufacture_packet',manufactured,lambda r:not r['admitted'])
    provenance=dict(classification='RECOVERED OFFLINE EVIDENCE ONLY; original fixed-v1 STRICTRED remains unchanged; no automatic phase reclassification',measurement_change='Replace separately-read mutable observer snapshot/id completion with unique immutable raw absolute position + new teleport identity + matching raw ACK + post-Respawn 25-column load + independent later complete actual UUID-bound server-body position. Yaw/pitch excluded until target comparison. No engine or fixture/source/observer change.',source_order_binding='Original frozen run_side issues spawn-loaded receipt/sync then snapshot(revived); full body query follows revived phase marker. Command sent/receipt timestamps bound kill-to-body to 30s. Wire request-to-loaded bounded separately. No absolute proxy/console timestamp equivalence is asserted.',limits='Official original client codec supplies complete login/Respawn/chunk decoding; offline selected position and ACK payloads additionally consumed exactly. Initial/default metadata and other packet families are not parity targets. No new runtime capture or post-layout/byte-identical executable claim.',extractor_sha256=sha(Path(__file__)),parser_sha256=sha(PARSER),frozen_program_sha256=sha(args.frozen_program),inputs={k:d['inputs'] for k,d in inputs.items()})
    for label,report in reports.items():(OUT/(label+'-derived-report.json')).write_text(json.dumps(dict(provenance=provenance,result=report),indent=2)+'\n')
    (OUT/'synthetic-controls.json').write_text(json.dumps(controls,indent=2)+'\n')
    (OUT/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
    print(json.dumps({k:dict(admitted=r['admitted'],targets=r.get('targets'),selected=r.get('selected',{}).get('id'),extra=r.get('provisional_extra_count'),error=r.get('error')) for k,r in reports.items()},indent=2));print('synthetic controls',len(controls),'PASS')
if __name__=='__main__':main()
