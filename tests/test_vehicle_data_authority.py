"""Entity NBT reads/loads must share the live vehicle-menu inventory.

Execute production /data boat/cart arms and inventory authority/close helpers.
The NBT codec, positions, loot engine and scoreboard storage are endpoints.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
from test_mobile_multiplayer_keyboard import block
ROOT=Path(__file__).resolve().parents[1]


def arm(source,name):
    at=source.index('Target::'+name+'(id) => {')
    return source[at:source.index('{',at)]+block(source,source.index('{',at))


def program():
    data=(ROOT/'crates/horizon-client/src/game/commands/data.rs').read_text()
    carts=(ROOT/'crates/horizon-client/src/game/minecarts.rs').read_text()
    persist=(ROOT/'crates/horizon-client/src/game/persist.rs').read_text()
    actions=(ROOT/'crates/horizon-client/src/game/lan_actions.rs').read_text()
    boats=(ROOT/'crates/horizon-client/src/game/boats.rs').read_text()
    getter=method(data,'entity_nbt_in_level');setter=method(data,'apply_entity_nbt')
    get='fn get(&mut self,t:Target)->Option<Tag>{Some(match t{'+','.join(arm(getter,n) for n in ('Boat','Cart'))+',_=>return None})}'
    tail=setter[setter.index('        if let Target::Mob(id)'):setter.rindex('        Ok(())')]
    # The codec endpoint uses String keys rather than JavaString keys.
    tail=tail.replace('&"Team".into()', '"Team"').replace('&"Passengers".into()', '"Passengers"')
    kept=setter[setter.index('        let kept_passengers ='):setter.index('        match t {\n            // EntityDataAccessor')]
    put='fn put(&mut self,t:Target,mut tag:Tag)->Result<(),String>{let stale=||"stale".to_owned();'+kept+'match t{'+','.join(arm(setter,n) for n in ('Boat','Cart'))+',_=>return Err(stale())}'+tail+'Ok(())}'
    methods='\n'.join(method(carts,n) for n in ('vehicle_slots','store_cart_items','close_cart_container'))
    for name in ('open_vehicle_items','sync_open_vehicle_items'):
        if 'fn '+name+'(' in carts:methods+='\n'+method(carts,name)
    methods+='\n'+method(actions,'vehicle_has_other_viewers')
    methods+='\n'+method(persist,'join_saved_team')
    free=method(persist[persist.index('fn join_saved_team(board:'):],'join_saved_team')
    real_boat='\nimpl Boat {'+method(boats,'to_nbt')+'}\n'
    return STUBS.replace('SAVED_FIRE', method(boats,'saved_fire').replace('fn saved_fire(', 'pub(crate) fn saved_fire('))+real_boat+free+'\nimpl Game{'+get+put+methods+'}\n'+CASES


STUBS=r'''
#![allow(dead_code)]
extern crate self as log;
extern crate self as horizon_client;
#[macro_export]macro_rules!warn{($($t:tt)*)=>{{}}}
use std::collections::{HashMap,HashSet};
type ItemStack=String;const CART_KEY_Y:i32=-100_000;
#[derive(Clone,Debug,PartialEq)]enum Tag{Short(i16),Byte(i8),Compound(HashMap<String,Tag>),Items(Vec<Option<ItemStack>>),IntArray(Vec<i32>),String(String)}
impl Tag{fn get(&self,k:&str)->Option<&Tag>{if let Self::Compound(c)=self{c.get(k)}else{None}}}
fn set_key(tag:&mut Tag,k:&str,v:Tag){if let Tag::Compound(c)=tag{c.insert(k.into(),v);}}
fn text(tag:Option<&Tag>)->Option<String>{if let Some(Tag::String(v))=tag{Some(v.clone())}else{None}}
fn uuid_text(id:[i32;4])->String{format!("entity-{}",id[0])}
mod save{use super::*;pub fn number(tag:Option<&Tag>)->Option<i64>{match tag{Some(Tag::Short(v))=>Some(i64::from(*v)),Some(Tag::Byte(v))=>Some(i64::from(*v)),_=>None}} pub fn decode_items(tag:Option<&Tag>,_:usize)->Vec<Option<ItemStack>>{if let Some(Tag::Items(v))=tag{v.clone()}else{vec![]}}}
pub mod entity_nbt{use super::*;pub fn boat_from_nbt(_:&Tag)->Option<([f64;3],[f64;3],f32)>{Some(([0.;3],[0.;3],0.))}pub fn rotation(_:&Tag)->[f32;2]{[0.;2]}pub fn flag(tag:&Tag,key:&str)->bool{save::number(tag.get(key)).unwrap_or(0)!=0} pub fn write_saved_flags(tag:&mut Tag,on_ground:bool,no_gravity:bool){set_key(tag,"OnGround",Tag::Byte(on_ground as i8));set_key(tag,"NoGravity",Tag::Byte(no_gravity as i8));}
 pub fn boat_to_nbt(_:&str,_:[f64;3],_:[f64;3],_:[f32;2],_:(bool,f64),_:[i32;4],raw:Option<&Tag>,items:Option<&[Option<ItemStack>]>)->Tag{let mut tag=raw.cloned().unwrap_or(Tag::Compound(HashMap::new()));if let Some(items)=items{set_key(&mut tag,"Items",Tag::Items(items.into()));}tag}}
mod game{pub mod boats{use crate::*;pub const CHEST_SLOTS:usize=27;SAVED_FIRE}pub mod minecarts{pub fn forget_unreadable_display(_: &mut crate::Cart,_:impl Fn(&crate::Tag)->bool){}}}
#[derive(Clone)]struct Cart{id:u32,items:Vec<Option<ItemStack>>,loot:Option<()>}
impl Cart{fn to_nbt(&self,u:[i32;4],raw:Option<&Tag>)->Tag{entity_nbt::boat_to_nbt("cart",[0.;3],[0.;3],[0.;2],(false,0.),u,raw,Some(&self.items))}fn load(&mut self,tag:&Tag)->Option<()>{self.items=save::decode_items(tag.get("Items"),27);Some(())}}
#[derive(Clone)]struct Boat{id:u32,chest:bool,items:Vec<Option<ItemStack>>,position:[f64;3],previous:[f64;3],velocity:[f64;3],yaw:f32,previous_yaw:f32,pitch:f32,no_gravity:bool,on_ground:bool,fire_ticks:i32,fall_distance:f64}impl Boat{fn entity_type(&self)->String{"boat".into()}}
enum BlockEntity{Chest(Vec<Option<ItemStack>>)}struct World{block_entities:HashMap<(i32,i32,i32),BlockEntity>}
#[derive(Clone,Copy)]enum Target{Boat(u32),Cart(u32),Mob(u32),Item(u32),Stand(u32),Frame(u32),Other(u32),Fangs(u32),Tnt(u32),Orb(u32),Cushion(u32),Trident(u32),Projectile(u32),Player}
struct Persist{raw:HashMap<u32,Tag>}
#[derive(Default)]struct Scoreboard{teams:HashSet<String>,members:HashMap<String,String>}impl Scoreboard{fn team(&self,t:&str)->Option<()>{self.teams.contains(t).then_some(())}fn add_to_team(&mut self,n:&str,t:&str){self.members.insert(n.into(),t.into());}}
#[derive(Clone,Copy,PartialEq)]pub enum Slots{Block((i32,i32,i32))}
pub struct Menu{pub slots:Slots}
pub mod lan{pub mod guest{pub struct Guest{pub conn:u32,pub placed:bool,pub dimension:String,pub menu:Option<crate::Menu>}}}
struct Hub{guests:Vec<lan::guest::Guest>}struct Host{hub:Hub}
struct Dim;impl Dim{fn id(&self)->&str{"minecraft:overworld"}}
struct Score{board:Scoreboard}struct Game{lan:Option<Host>,dim:Dim,carts:Vec<Cart>,boats:Vec<Boat>,world:World,persist:Persist,score:Score}
impl Game{fn put_mob_passengers(&self,_:&mut Tag,_:u32){} fn write_boat_leash(&self,_:&mut Tag,_:usize){} fn uuid_for(&mut self,id:u32)->[i32;4]{[id as i32,0,0,0]}fn state_from_nbt(&self,_:&Tag)->Option<()>{Some(())}fn roll_cart_loot(&self,_:&mut Cart,_:&Tag){}
 fn new()->Self{Self{lan:None,dim:Dim,carts:vec![Cart{id:7,items:vec![Some("diamond:3".into())],loot:Some(())}],boats:vec![Boat{id:8,chest:true,items:vec![Some("gold:3".into())],position:[0.;3],previous:[0.;3],velocity:[0.;3],yaw:0.,previous_yaw:0.,pitch:0.,no_gravity:false,on_ground:false,fire_ticks:0,fall_distance:0.}],world:World{block_entities:HashMap::new()},persist:Persist{raw:HashMap::new()},score:Score{board:Scoreboard::default()}}}
}
'''
CASES=r'''
fn main(){for(id,target)in[(7,Target::Cart(7)),(8,Target::Boat(8))]{let mut g=Game::new();let key=(i32::MAX-15,CART_KEY_Y,id as i32);g.world.block_entities.insert(key,BlockEntity::Chest(vec![Some("emerald:2".into())]));
 g.persist.raw.insert(id,Tag::Compound(HashMap::from([("Passengers".into(),Tag::String("kept-riders".into()))])));
 let saved=g.get(target).unwrap();assert_eq!(saved.get("Items"),Some(&Tag::Items(vec![Some("emerald:2".into())])),"NBT must read live menu slots");assert!(g.carts[0].loot.is_some(),"an NBT read must not unpack loot");
 let mut changed=saved.clone();set_key(&mut changed,"Items",Tag::Items(vec![Some("lapis:4".into())]));set_key(&mut changed,"Passengers",Tag::String("replacement-riders".into()));
 set_key(&mut changed,"OnGround",Tag::Byte(1));set_key(&mut changed,"NoGravity",Tag::Byte(1));set_key(&mut changed,"Fire",Tag::Short(67));g.put(target,changed).unwrap();
 assert_eq!(g.persist.raw[&id].get("Passengers"),Some(&Tag::String("kept-riders".into())),"Entity.load must not replace resident riders");
 if id==8{let b=&g.boats[0];assert!(b.on_ground&&b.no_gravity);assert_eq!(b.fire_ticks,67);assert_eq!(g.get(target).unwrap().get("Fire"),Some(&Tag::Short(67)));}
 assert_eq!(g.vehicle_slots(id).unwrap(),&vec![Some("lapis:4".into())],"first viewer missed data edit");assert_eq!(g.vehicle_slots(id).unwrap(),&vec![Some("lapis:4".into())],"second viewer missed data edit");
 // An unrelated merge reuses the actual live NBT snapshot, not stale backing slots.
 let mut named=g.get(target).unwrap();set_key(&mut named,"CustomName",Tag::String("Renamed".into()));g.put(target,named).unwrap();g.close_cart_container(key);assert!(g.world.block_entities.is_empty());assert_eq!(g.get(target).unwrap().get("Items"),Some(&Tag::Items(vec![Some("lapis:4".into())])),"close restored stale backing items");
 g.score.board.teams.insert("red".into());let mut team=g.get(target).unwrap();set_key(&mut team,"Team",Tag::String("red".into()));g.put(target,team).unwrap();assert_eq!(g.score.board.members.get(&format!("entity-{id}")).map(String::as_str),Some("red"));assert!(g.persist.raw[&id].get("Team").is_none(),"Team is read into scoreboard, never persisted as entity tag");
}}
'''


class VehicleDataAuthorityTests(unittest.TestCase):
    def test_live_vehicle_nbt_edits_and_common_team_load(self):
        if not shutil.which('rustc'):self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-vehicle-data-') as folder:
            src=Path(folder)/'probe.rs';binary=Path(folder)/'probe';src.write_text(program())
            built=subprocess.run(['rustc','--edition=2024',str(src),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
