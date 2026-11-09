"""Live mannequin futures must survive bounded shared profile-cache eviction.

Production resolver/request ownership and mannequin lifetime methods execute
with deterministic time and deferred fetch workers. Profile decoding is an
endpoint here; worker pressure/shutdown is covered separately by the actual
owned-queue tests, and ordinary skin tests cover the property/texture codec.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    skins = (ROOT / 'crates/horizon-client-render/src/skins.rs').read_text()
    remote = (ROOT / 'crates/horizon-client/src/game/remote.rs').read_text()
    a = skins.index('#[derive(Clone, Debug)]\npub struct ResolvedProfile')
    b = skins.index('impl ResolvedProfile', a)
    profile = skins[a:b] + 'impl ResolvedProfile {pub ' + method(skins, 'textures') + '}\n'
    a = skins.index('#[derive(Clone, Debug)]\npub enum Resolve')
    b = skins.index('/// `StringUtil.isValidPlayerName`', a)
    definitions = skins[a:b]
    resolver = skins[skins.index('impl Resolver {'):]
    methods = '\n'.join('pub ' + method(resolver, n) for n in ('new', 'sweep', 'poll', 'lookup', 'state'))
    a = remote.index('struct MannequinProfile {')
    b = remote.index('\n}\n', a) + 3
    mannequin = remote[a:b]
    impl_at = remote.index('impl MannequinProfile {', b)
    mannequin += 'impl MannequinProfile {\n' + method(remote[impl_at:], 'new') + method(remote[impl_at:], 'textures') + '}\n'
    mannequin += method(remote, 'prune_mannequin_profiles')
    return CLOCK + SKIN_ENDPOINTS + method(skins, 'io_active') + '\n' + profile + definitions + method(skins, 'valid_player_name') + '\nimpl Resolver {' + methods + '}\n' + CACHE_ENDPOINTS + '\n}\n' + ENTITIES + mannequin + '\nimpl Game {' + method(remote, 'mannequin_textures') + method(remote, 'mannequin_profile_id') + '}\n' + CASES


CLOCK = r'''
#![allow(dead_code)]
extern crate std as real_std;
extern crate self as horizon_client;
extern crate self as horizon_protocol;
use real_std::{cell::RefCell,collections::HashMap,sync::{Mutex,atomic::{AtomicU64,Ordering}}};
static NOW:AtomicU64=AtomicU64::new(1_000_000_000);
static LOADS:AtomicU64=AtomicU64::new(0);
type Task=Box<dyn FnOnce()+Send>;
static TASKS:Mutex<Vec<(u64,Task)>>=Mutex::new(Vec::new());
mod std {
 pub use crate::real_std::{collections,sync};
 pub mod time {
  pub use crate::real_std::time::Duration;
  #[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)]pub struct Instant(pub u64);
  impl Instant{pub fn now()->Self{Self(crate::NOW.fetch_add(1,crate::Ordering::SeqCst))}pub fn elapsed(self)->Duration{Duration::from_micros(crate::NOW.load(crate::Ordering::SeqCst).saturating_sub(self.0))}}
 }
 pub mod thread{pub fn spawn(f:impl FnOnce()+Send+'static){crate::LOADS.fetch_add(1,crate::Ordering::SeqCst);crate::TASKS.lock().unwrap().push((crate::NOW.load(crate::Ordering::SeqCst)+45_000_000,Box::new(f)));}}
}
mod auth{pub fn discovery()->Result<(),()>{Ok(())}}
fn pump(){let mut tasks=TASKS.lock().unwrap();let mut pending=Vec::new();let mut ready=Vec::new();for(due,task)in tasks.drain(..){if due<=NOW.load(Ordering::SeqCst){ready.push(task)}else{pending.push((due,task))}}*tasks=pending;drop(tasks);for task in ready{task();}}
fn run_last(){let(_,task)=TASKS.lock().unwrap().pop().unwrap();task();}
fn reset(){TASKS.lock().unwrap().clear();NOW.store(1_000_000_000,Ordering::SeqCst);LOADS.store(0,Ordering::SeqCst);}
'''

SKIN_ENDPOINTS = r'''
mod skins {
use crate::std;
use crate::real_std::sync::{Arc,atomic::{AtomicBool,Ordering}};
// Head/NBT parsing is outside this mannequin-future probe. The head probe
// replaces this endpoint with the production cache and its codec endpoints.
#[derive(Default)]struct HeadProfiles;
mod workers {
 use super::*;
 pub struct Pool;
 struct Deferred {scope:Arc<AtomicBool>,run:Option<Box<dyn FnOnce(&AtomicBool,&AtomicBool)+Send>>,cancel:Option<Box<dyn FnOnce()+Send>>}
 impl Deferred{fn execute(mut self){let stopping=AtomicBool::new(false);if !self.scope.load(Ordering::Acquire){self.run.take().unwrap()(&stopping,&self.scope);self.cancel.take();}}}
 impl Drop for Deferred{fn drop(&mut self){if let Some(cancel)=self.cancel.take(){cancel();}}}
 impl Pool{
  pub fn new(_:&str,_:usize)->Arc<Self>{Arc::new(Self)}
  pub fn stopped(&self)->bool{false}
  pub fn submit(&self,scope:Arc<AtomicBool>,run:impl FnOnce(&AtomicBool,&AtomicBool)+Send+'static,cancel:impl FnOnce()+Send+'static){
   let job=Deferred{scope,run:Some(Box::new(run)),cancel:Some(Box::new(cancel))};
   crate::LOADS.fetch_add(1,Ordering::SeqCst);crate::TASKS.lock().unwrap().push((crate::NOW.load(Ordering::SeqCst)+45_000_000,Box::new(move||job.execute())));
  }
 }
}
#[derive(Clone,Debug,Hash,PartialEq,Eq)]pub enum NameOrId{Name(String),Id(u128)}
#[derive(Clone,Copy,PartialEq)]pub enum Signature{Signed,Unsigned}
#[derive(Clone,Debug,Default,PartialEq)]pub struct Textures{pub skin:Option<String>,pub slim:bool}
pub const TEXTURE_URIS:[&str;2]=["https://example.invalid/texture/{textureId}","http://example.invalid/texture/{textureId}"];
pub fn unpack(value:&str,_:Signature,_:&[&str])->Textures{Textures{skin:Some(format!("skins/{value}")),slim:value.contains("Slim")}}
fn resolve_profile_cancellable(key:&NameOrId,doc:&(),active:&impl Fn()->bool)->Option<ResolvedProfile>{if !active(){return None;}resolve_profile(key,doc)}
fn resolve_profile(key:&NameOrId,_:&())->Option<ResolvedProfile>{let name=match key{NameOrId::Name(name)=>name.clone(),NameOrId::Id(id)=>format!("NPC{id}")};if name=="Missing"{return None;}Some(ResolvedProfile{id:1,name:name.clone(),properties:vec![("textures".into(),name,None)]})}
'''

CACHE_ENDPOINTS = r'''
pub fn force_sweep(r:&mut Resolver){r.last_sweep=std::time::Instant(crate::NOW.load(crate::Ordering::SeqCst)-31_000_000);r.sweep();}
pub fn cache_len(r:&Resolver)->usize{r.entries.len()}
pub fn cached(r:&Resolver,key:&NameOrId)->bool{r.entries.contains_key(key)}
'''

ENTITIES = r'''
pub mod crypto{pub fn offline_uuid(name:&str)->u128{assert_eq!(name,"VisibleSlim","unexpected identity endpoint");0x624452b8de833ade80cebc2542610c15}}
pub mod packets{pub mod login{#[derive(Clone,Debug,Default,PartialEq)]pub struct Property{pub name:String,pub value:String,pub signature:Option<String>}}}
pub mod entity_data{#[derive(Clone,Debug,Default,PartialEq)]pub struct Profile{pub name:Option<String>,pub uuid:Option<u128>,pub properties:Vec<crate::packets::login::Property>}}
struct Remote{mannequin_profiles:RefCell<HashMap<u32,MannequinProfile>>}
struct Game{remote:Option<Remote>,skin_profiles:RefCell<skins::Resolver>}
impl Game{fn new()->Self{Self{remote:Some(Remote{mannequin_profiles:RefCell::default()}),skin_profiles:RefCell::new(skins::Resolver::new())}}}
fn profile(name:&str)->entity_data::Profile{entity_data::Profile{name:Some(name.into()),..Default::default()}}
'''

CASES = r'''
fn main(){
 reset();let g=Game::new();let profiles:Vec<_>=(0..500).map(|i|profile(&format!("NPC{i}"))).collect();let visible=profile("VisibleSlim");
 assert!(g.mannequin_textures(9999,Some(&visible)).skin.is_none());
 assert_eq!(g.mannequin_profile_id(9999,&visible),0x624452b8de833ade80cebc2542610c15,"pending name uses the partial profile UUID");
 for(i,p)in profiles.iter().enumerate(){assert!(g.mannequin_textures(i as u32,Some(p)).skin.is_none());}
 assert_eq!(LOADS.load(Ordering::SeqCst),501);
 skins::force_sweep(&mut g.skin_profiles.borrow_mut());assert_eq!(skins::cache_len(&g.skin_profiles.borrow()),256);assert!(!skins::cached(&g.skin_profiles.borrow(),&skins::NameOrId::Name("VisibleSlim".into())));
 // The 45s-deferred original workers settle their captured futures even
 // though the shared cache evicted nearly half the keys.
 NOW.fetch_add(45_000_000,Ordering::SeqCst);pump();g.skin_profiles.borrow_mut().poll();
 for round in 0..4{
  for(i,p)in profiles.iter().enumerate(){assert_eq!(g.mannequin_textures(i as u32,Some(p)).skin.as_deref(),Some(format!("skins/NPC{i}").as_str()));}
  let skin=g.mannequin_textures(9999,Some(&visible));assert_eq!(skin.skin.as_deref(),Some("skins/VisibleSlim"));assert!(skin.slim);assert_eq!(g.mannequin_profile_id(9999,&visible),1,"completed default body uses the resolved GameProfile UUID");
  NOW.fetch_add(700_000_000,Ordering::SeqCst);skins::force_sweep(&mut g.skin_profiles.borrow_mut());assert!(skins::cache_len(&g.skin_profiles.borrow())<=256);
  assert_eq!(LOADS.load(Ordering::SeqCst),501,"round{round}: live callers restarted profile requests");
 }
 // A changed profile captures its own future. The older worker may finish
 // afterwards, but its reply cannot overwrite this entity's new appearance.
 reset();let g=Game::new();let old=profile("Old");let new=profile("NewSlim");g.mannequin_textures(7,Some(&old));g.mannequin_textures(7,Some(&new));run_last();
 assert_eq!(g.mannequin_textures(7,Some(&new)).skin.as_deref(),Some("skins/NewSlim"));run_last();g.skin_profiles.borrow_mut().poll();
 assert_eq!(g.mannequin_textures(7,Some(&new)).skin.as_deref(),Some("skins/NewSlim"));
 // Removal/profile mismatch prunes entity ownership, without a global
 // unbounded map retaining every entity that has ever been seen.
 let unchanged=HashMap::from([(7,&new)]);prune_mannequin_profiles(&mut g.remote.as_ref().unwrap().mannequin_profiles.borrow_mut(),&unchanged);assert_eq!(g.mannequin_textures(7,Some(&new)).skin.as_deref(),Some("skins/NewSlim"));
 let other=profile("Other");let current=HashMap::from([(7,&other)]);prune_mannequin_profiles(&mut g.remote.as_ref().unwrap().mannequin_profiles.borrow_mut(),&current);
 assert!(g.remote.as_ref().unwrap().mannequin_profiles.borrow().is_empty());
 let removed=profile("PendingRemoved");g.mannequin_textures(8,Some(&removed));prune_mannequin_profiles(&mut g.remote.as_ref().unwrap().mannequin_profiles.borrow_mut(),&HashMap::new());assert!(g.remote.as_ref().unwrap().mannequin_profiles.borrow().is_empty());
 let after=profile("AfterRemoval");g.mannequin_textures(8,Some(&after));run_last();run_last();g.skin_profiles.borrow_mut().poll();assert_eq!(g.mannequin_textures(8,Some(&after)).skin.as_deref(),Some("skins/AfterRemoval"));
 // Replacing the entire remote connection drops its per-entity futures.
 // An old worker can settle only its old captured result, never a new world.
 reset();let mut world=Game::new();world.mannequin_textures(9,Some(&old));world.remote.take();world.remote=Some(Remote{mannequin_profiles:RefCell::default()});world.mannequin_textures(9,Some(&new));run_last();run_last();world.skin_profiles.borrow_mut().poll();assert_eq!(world.mannequin_textures(9,Some(&new)).skin.as_deref(),Some("skins/NewSlim"));
 // Genuine negative results remain consumed for this profile, as Java's
 // completed Optional.empty skin future does, with no extra retry policy.
 reset();let mut g=Game::new();let missing=profile("Missing");g.mannequin_textures(1,Some(&missing));run_last();
 for _ in 0..20{assert!(g.mannequin_textures(1,Some(&missing)).skin.is_none());}assert_eq!(LOADS.load(Ordering::SeqCst),1);
 g.remote.take();assert!(g.remote.is_none());
}
'''


class ProfileLookupLifetimeTests(unittest.TestCase):
    def test_live_profile_lookups_survive_pressure_and_follow_entity_lifetime(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-profile-lifetime-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
