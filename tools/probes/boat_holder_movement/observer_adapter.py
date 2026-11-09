"""Local transport adapter: existing Observer codec + LocatorBarClient list receipts.
/list establishes readiness/setup only. Original input-group ordering uses a subsequent
UseItem marker's EXACT sequence ACK, never this chat-command receipt alone.
"""
import struct,time
from collections import deque
from observer import Observer,string,read_varint
from world_compare import Nbt

class LocalObserver(Observer):
 def __init__(self,*args,**kwargs):
  self.native_ping=kwargs.pop("native_ping",False)
  self.protocol_receipts=[];self.list_pending=deque();self.exact_acks=[]
  outer=kwargs.pop('packet_hook',None)
  def hook(pid,payload):
   if pid in (1,4,11,50,73,124):
    row=dict(direction='clientbound',id=pid,payload=payload.hex(),monotonic_ns=time.monotonic_ns())
    self.protocol_receipts.append(row)
    if pid==4:
     import io
     sequence=read_varint(io.BytesIO(payload).read)
     with self.condition:
      self.exact_acks.append(sequence);self.condition.notify_all()
    if pid==124:
     r=Nbt(payload);tag=r.unpack('>B');content=r.payload(tag);overlay=r.unpack('>B');assert r.p==len(payload)
     row.update(content=content,overlay=overlay)
     if isinstance(content,dict) and content.get('translate')=='commands.list.players':
      with self.condition:
       assert self.list_pending,'unsolicited list reply'
       token=self.list_pending.popleft();row['list_token']=token
       if token>=1000000000:self.completed_barrier=token
       else:
        assert self.login_payload is not None and self.teleports>0 and self.loaded
        self.acknowledged=token;self.ready.set()
       self.condition.notify_all()
   if outer:outer(pid,payload)
  super().__init__(*args,packet_hook=hook,**kwargs)
 def send(self,pid,payload=b''):
  if pid==38 and self.login_payload is not None and not self.native_ping:
   token=struct.unpack('>q',payload)[0]
   with self.condition:self.list_pending.append(token)
   pid,payload=7,string('list')
   self.protocol_receipts.append(dict(direction='serverbound',id=pid,payload=payload.hex(),list_token=token,monotonic_ns=time.monotonic_ns()))
  elif self.login_payload is not None:
   self.protocol_receipts.append(dict(direction='serverbound',id=pid,payload=payload.hex(),monotonic_ns=time.monotonic_ns()))
  super().send(pid,payload)
 def wait_exact_ack(self,sequence):
  with self.condition:
   assert self.condition.wait_for(lambda:sequence in self.exact_acks or self.error,timeout=20),('missing exact action ACK',sequence)
  self.check()
