"""Check every declared row, and separately report complete reply equality."""
import json,sys
from pathlib import Path
j,n=map(Path,sys.argv[1:]);a=json.loads((j/'strict-checks.json').read_text());b=json.loads((n/'strict-checks.json').read_text())
assert len(a)==len(b)==67
assert [(x['label'],x['expected']) for x in a]==[(x['label'],x['expected']) for x in b]
failed=[x for x in a+b if x['actual']!=x['expected']]
for folder in [j,n]:
 packets=json.loads((folder/'packets.json').read_text())
 for phase,want in [('first',[('probe:group',600)]),('same',[]),('failed',[]),('other',[('probe:other',600)])]:
  got=[(p['group'],p['duration']) for p in packets if p['id']==22 and p['phase']==phase]
  if got!=want:failed.append(dict(label='added-causal/'+phase,actual=got,expected=want))
 ar=json.loads((j/'cases.json').read_text());br=json.loads((n/'cases.json').read_text());assert [x['label'] for x in ar]==[x['label'] for x in br]
 print(folder.name, len(json.loads((folder/'strict-checks.json').read_text())), 'strict rows')
print(json.dumps(dict(failures=failed,complete_replies_equal=sum(x==y for x,y in zip(ar,br)),complete_replies=len(ar)),indent=2))
raise SystemExit(bool(failed))
