import re,glob,os,sys
files=sorted(glob.glob('crates/horizon-client/src/mobs/*.rs'))
def block(s,i):
    # i at '{'
    d=0;j=i
    while j<len(s):
        c=s[j]
        if c=='{':d+=1
        elif c=='}':
            d-=1
            if d==0:return s[i:j+1],j
        j+=1
    return s[i:],len(s)
def skipstr(s,i):
    # simplistic: skip line comments/strings
    return False
out=[]
for f in files:
    s=open(f).read()
    for m in re.finditer(r'impl(?:<[^>]*>)?\s+Goal\s+for\s+(\w+)',s):
        name=m.group(1)
        # find block after 'for name' or after where clause
        b=s.find('{',m.end())
        if b<0:continue
        body,_=block(s,b)
        meths={}
        for mm in re.finditer(r'\bfn\s+(can_use|can_continue)\s*\(',body):
            bb=body.find('{',mm.end())
            if bb<0:continue
            mbody,_=block(body,bb)
            meths[mm.group(1)]=mbody
        if meths:
            out.append((f,name,meths,body))
with open('.analysis/rust_goals.txt','w') as o:
    for f,name,meths,body in out:
        o.write('### %s :: %s\n'%(f,name))
        for k,v in meths.items():
            o.write(' -- %s\n%s\n\n'%(k,v))
print(len(out))
