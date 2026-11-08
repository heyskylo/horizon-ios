import re,glob,json
def block(s,i):
    d=0;j=i
    while j<len(s):
        c=s[j]
        if c=='{':d+=1
        elif c=='}':
            d-=1
            if d==0:return s[i:j+1]
        j+=1
    return s[i:]
out=[]
for f in sorted(glob.glob('crates/horizon-client/src/mobs/*.rs')):
    s=open(f).read()
    for m in re.finditer(r'impl(?:<[^>]*>)?\s+Goal\s+for\s+(\w+)',s):
        name=m.group(1)
        b=s.find('{',m.end())
        if b<0: continue
        body=block(s,b)
        fm=re.search(r'\bfn\s+flags\s*\(\s*&self\s*\)\s*->\s*u8\s*\{',body)
        if not fm: continue
        fb=body.find('{',fm.end()-1)
        fl=block(body,fb)
        out.append({'file':f,'class':name,'flags_src':fl})
json.dump(out,open('.analysis/rustflags.json','w'))
for e in out:
    one=' '.join(e['flags_src'].split())
    print(e['file'].split('/')[-1], e['class'], '::', one[:120])
