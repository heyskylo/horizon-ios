"""A resource generation's tail worker must retain its import/read ownership.

Executes the production startup worker and rest-worker spawn block against a
controlled slow file-read endpoint. No game assets or player data are used.
"""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    game = (ROOT / 'crates/horizon-client/src/game.rs').read_text()
    menu = (ROOT / 'crates/horizon-client/src/game/startup_menu.rs').read_text()
    imports = (ROOT / 'crates/horizon-client/src/import.rs').read_text()
    resource = (ROOT / 'crates/horizon-client/src/game/resource_load.rs').read_text()
    a = resource.index('    let rest_loader = if menu {', resource.index('fn load_resources_with_access('))
    b = resource.index('    // The world tables load (above)', a)
    rest = resource[a:b]
    a = game.index('struct WorldRestLoader {')
    b = game.index('\nimpl Game', a)
    loader = game[a:b]
    if 'struct HomeGate {' in imports:
        a = imports.rfind('#[derive', 0, imports.index('struct HomeGate {'))
        b = imports.rfind('#[derive', a, imports.index('pub enum Kind'))
        access = imports[a:b]
    else:
        access = 'pub ' + method(imports.replace('with_home_access<T>', 'with_home_access'), 'with_home_access').replace('fn with_home_access(', 'fn with_home_access<T>(', 1)
    startup = 'pub ' + method(menu, 'spawn_loader')
    mods = (ROOT / 'crates/horizon-client/src/game/mods.rs').read_text()
    prepare = 'pub ' + method(mods, 'prepare_content')
    prepare += '\n' + 'pub ' + method(mods, 'content_for_loaded_root')
    lease = f'#[path="{ROOT / "crates/horizon-assets/src/root_lease.rs"}"]mod root_lease;\n'
    return STUBS.replace('HOME_ACCESS_FUNCTION', access).replace('PREPARE_CONTENT_FUNCTION', prepare).replace('ROOT_LEASE_MODULE', lease) + loader + '\nmod production {use super::*;\n' + startup + '\n}\n' + LOAD_PREFIX + rest + LOAD_SUFFIX + CASES


STUBS = r'''
#![allow(dead_code)]
extern crate self as horizon_client;
extern crate self as horizon_assets;
extern crate self as log;
ROOT_LEASE_MODULE
#[macro_export]macro_rules! warn{($($t:tt)*)=>{{}}}
#[macro_export]macro_rules! info{($($t:tt)*)=>{{}}}
use std::{collections::{BTreeMap,BTreeSet},fs,path::{Path,PathBuf},sync::{Arc,Mutex,OnceLock,PoisonError,Weak,Condvar,mpsc::{self,Receiver},atomic::{AtomicBool,Ordering}},thread::JoinHandle,time::{Instant,Duration}};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error+Send+Sync>>;
mod import{use super::*;HOME_ACCESS_FUNCTION
 pub fn home()->PathBuf{PathBuf::from(std::env::var_os("HORIZON_HOME_LIFETIME_FIXTURE").unwrap())}
}
struct Game;impl Game{fn game_dir()->PathBuf{import::home()}}
static RECOVERY_OBSERVED:AtomicBool=AtomicBool::new(false);
static SNAPSHOT_HELD:AtomicBool=AtomicBool::new(false);
// The rollback implementation has real filesystem tests in mod_registry/storage.rs.
// Here its boundary records that production startup recovered the correct home before reading.
mod mod_registry{use super::*;pub fn with_content_snapshot<T>(home:&Path,read:impl FnOnce()->T)->Result<T>{assert_eq!(home,import::home());RECOVERY_OBSERVED.store(true,Ordering::SeqCst);SNAPSHOT_HELD.store(true,Ordering::SeqCst);let result=read();SNAPSHOT_HELD.store(false,Ordering::SeqCst);Ok(result)}}
#[derive(Clone)]struct Mods{enabled:BTreeSet<String>}
#[derive(Clone)]struct Settings{resource_packs:Vec<String>,mods:Mods}
struct Options{dev_mods:Vec<PathBuf>}
fn launch_world(_: &Options)->Option<&str>{None}
mod packs{use super::*;pub fn overlay_content(_:&Path,_:&Vec<String>,_:&[()],content:Arc<mods::PreparedContent>)->Result<Arc<mods::PreparedContent>>{Ok(content)}
pub fn startup_root_leased(root:&Path,_:&Vec<String>,_:&[()])->Result<(PathBuf,root_lease::RootLease)>{Ok((root.to_owned(),root_lease::RootLease::acquire(root)?))}}
mod mods{use super::*;
 pub struct PreparedContent{pub root:PathBuf,pub catalog:Arc<catalog::Catalog>,pub selection:Selection,pub lease:root_lease::RootLease,pub data_changed:bool}
 mod catalog{use super::*;
  pub struct Catalog{pub entries:BTreeMap<String,()>,pub invalid:BTreeSet<String>}
  #[derive(Clone,Default)]pub struct Selection{pub enabled:BTreeSet<String>,pub entries:BTreeMap<String,()>,pub order:Vec<String>}
  pub fn read(home:&Path,_:&[PathBuf],_:&BTreeSet<String>)->std::result::Result<Arc<Catalog>,String>{
   assert_eq!(home,import::home());assert!(SNAPSHOT_HELD.load(Ordering::SeqCst),"catalog read escaped snapshot gate");
   Ok(Arc::new(Catalog{entries:BTreeMap::from([("content.mod".into(),())]),invalid:BTreeSet::from(["broken.mod".into()])}))
  }
  impl Selection{pub fn wanted(enabled:&BTreeSet<String>,catalog:&Catalog)->Self{Self{enabled:enabled.clone(),entries:catalog.entries.clone(),order:vec!["content.mod".into()]}}}
 }
 use catalog::Selection;
 pub fn snapshot_has_no_content(content:&PreparedContent)->bool{content.selection.entries.is_empty()&&content.selection.order.is_empty()}
 mod content{use super::*;pub fn build(root:&Path,home:&Path,selection:&Selection,catalog:Arc<catalog::Catalog>,live:bool)->std::result::Result<Arc<PreparedContent>,String>{
  assert_eq!(home,import::home());assert!(!live);assert!(SNAPSHOT_HELD.load(Ordering::SeqCst),"content read escaped snapshot gate");
  let requested=Arc::new(PreparedContent{root:root.to_owned(),catalog,selection:selection.clone(),lease:root_lease::RootLease::acquire(root).map_err(|e|e.to_string())?,data_changed:true});
  *REQUESTED.lock().unwrap()=Some(Arc::downgrade(&requested));Ok(requested)
 }}
 PREPARE_CONTENT_FUNCTION
}
mod startup{pub fn step(_:&str){}}
mod telemetry{pub fn mark_rest_ready(){}}
mod world_loading{use super::*;pub fn write_prepared_files(_:PathBuf,_:Option<import::HomeAccess>){}}
#[derive(Clone)]struct Progress;impl Progress{fn set(&self,_:usize){}}
type UiResources=usize;
struct Resources{rest:OnceLock<String>}
struct Pack{content:Option<Arc<mods::PreparedContent>>}
struct Loaded{root:PathBuf,pack:Pack,home_access:Option<import::HomeAccess>,core:String,resources:Arc<Resources>,rest_loader:Option<WorldRestLoader>,pack_error:Option<String>}
enum LoaderMsg{Ui(usize,PathBuf),Done(std::result::Result<Loaded,String>)}
static TAIL_STARTED:AtomicBool=AtomicBool::new(false);
static FALLBACK_RESULT:AtomicBool=AtomicBool::new(false);
static REQUESTED:Mutex<Option<Weak<mods::PreparedContent>>>=Mutex::new(None);
fn tail_gate()->&'static(Mutex<bool>,Condvar){static GATE:OnceLock<(Mutex<bool>,Condvar)>=OnceLock::new();GATE.get_or_init(||(Mutex::new(false),Condvar::new()))}
fn load_world_rest(root:&Path,_:&Resources,_:&(),_:&(),_:&(),_:Option<&()>,_:&(),_:(),_:&Progress,_:Instant,_:bool)->Result<String>{
 TAIL_STARTED.store(true,Ordering::SeqCst);
 let(lock,ready)=tail_gate();let mut release=lock.lock().unwrap();while !*release{release=ready.wait(release).unwrap();}drop(release);
 Ok(fs::read_to_string(root.join("generation"))?)
}
'''

LOAD_PREFIX = r'''
fn load_resources_with_access(root:PathBuf,_:Option<&str>,vanilla_root:&Path,_:&Settings,started:Instant,menu:bool,headless:bool,progress:&Progress,home_access:Option<import::HomeAccess>,publish:&mut dyn FnMut(UiResources,&Path))->Result<Loaded>{
 assert!(RECOVERY_OBSERVED.load(Ordering::SeqCst),"core read preceded mod transaction recovery");
 let root=if FALLBACK_RESULT.load(Ordering::SeqCst){root.with_file_name("fedcba9876543210")}else{root};
 let core=fs::read_to_string(root.join("generation"))?;
 publish(0,&root);
 let resources=Arc::new(Resources{rest:OnceLock::new()});
 let entity_models:Option<Arc<()>>=None;let item_defaults=Arc::new(());let assets=();let behaviour_json=();let gameplay_projection=();let visibility=();
'''
LOAD_SUFFIX = r'''
 Ok(Loaded{root,pack:Pack{content:None},home_access,core,resources,rest_loader,pack_error:None})
}
'''

CASES = r'''
fn main(){
 let home=import::home();fs::create_dir_all(&home).unwrap();let root=home.join(".horizon-cache/mod-content/0123456789abcdef");fs::create_dir_all(&root).unwrap();fs::write(root.join("generation"),"before").unwrap();
 let settings=Settings{resource_packs:vec![],mods:Mods{enabled:BTreeSet::from(["content.mod".into(),"broken.mod".into(),"native.mod".into(),"future.mod".into()])}};let options=Options{dev_mods:vec![]};
 let(rx,parent)=production::spawn_loader(&options,&settings,&root,Instant::now(),&Progress).unwrap();
 let mut loaded=loop{match rx.recv_timeout(Duration::from_secs(3)).unwrap(){LoaderMsg::Ui(..)=>{},LoaderMsg::Done(result)=>break result.unwrap()}};
 parent.join().unwrap();
 assert_eq!(loaded.pack.content.as_ref().expect("matching loaded root must retain prepared content").root,loaded.root);
 assert!(Arc::ptr_eq(loaded.pack.content.as_ref().unwrap(),&REQUESTED.lock().unwrap().as_ref().unwrap().upgrade().unwrap()),"matching roots retain the requested immutable snapshot");
 let limit=Instant::now()+Duration::from_secs(3);while !TAIL_STARTED.load(Ordering::SeqCst){assert!(Instant::now()<limit);std::thread::yield_now();}
 let tail=loaded.rest_loader.take().unwrap();assert!(!tail.ready().unwrap());
 let core=loaded.core.clone();let resources=loaded.resources.clone();drop(loaded);
 assert!(root_lease::RootLease::try_exclusive(&root).unwrap().is_none(),"the detached tail must retain the generated root after Loaded drops");
 let (trying,attempted)=mpsc::channel();let(done,published)=mpsc::channel();let writer_home=home.clone();let writer_root=root.clone();
 let writer=std::thread::spawn(move||{trying.send(()).unwrap();import::with_home_access(&writer_home,||{fs::write(writer_root.join("generation"),"after")?;Ok(())}).unwrap();done.send(()).unwrap();});
 attempted.recv().unwrap();let escaped=published.recv_timeout(Duration::from_millis(150)).is_ok();
 // Release the finite reader even when observing the broken ownership path.
 *tail_gate().0.lock().unwrap()=true;tail_gate().1.notify_all();tail.finish().unwrap();writer.join().unwrap();
 assert!(root_lease::RootLease::try_exclusive(&root).unwrap().is_some(),"the completed tail must release its generated-root lease");
 let rest=resources.rest.get().unwrap();
 eprintln!("core={core}; rest={rest}; writer_escaped={escaped}");
 assert!(!escaped,"An importer published after the parent returned but while its rest worker still read the same generation");
 assert_eq!(&core,rest,"Core and rest tables came from different resource generations");
 assert_eq!(fs::read_to_string(root.join("generation")).unwrap(),"after","The finite reader must eventually release the writer");
 // A resource-loader fallback has a different effective root. Its Loaded
 // handoff must describe its actual root, without the rejected content selection.
 let fallback_root=root.with_file_name("fedcba9876543210");fs::create_dir_all(&fallback_root).unwrap();fs::write(fallback_root.join("generation"),"fallback").unwrap();
 FALLBACK_RESULT.store(true,Ordering::SeqCst);
 let(rx,parent)=production::spawn_loader(&options,&settings,&root,Instant::now(),&Progress).unwrap();
 let mut fallback=loop{match rx.recv_timeout(Duration::from_secs(3)).unwrap(){LoaderMsg::Ui(..)=>{},LoaderMsg::Done(result)=>break result.unwrap()}};
 parent.join().unwrap();assert_ne!(fallback.root,root);
 let snapshot=fallback.pack.content.as_ref().expect("fallback needs a truthful actual-root snapshot");assert_eq!(snapshot.root,fallback_root);assert_eq!(snapshot.root,fallback.root);
 assert!(mods::snapshot_has_no_content(snapshot));assert!(!snapshot.data_changed);
 assert_eq!(snapshot.selection.enabled,BTreeSet::from(["native.mod".into(),"future.mod".into()]));
 assert!(snapshot.catalog.entries.contains_key("content.mod")&&snapshot.catalog.invalid.contains("broken.mod"));
 assert_eq!(REQUESTED.lock().unwrap().as_ref().unwrap().strong_count(),0,"rejected requested snapshot must not be attached to fallback");
 assert!(snapshot.lease.is_active());assert!(root_lease::RootLease::try_exclusive(&fallback_root).unwrap().is_none());
 fallback.rest_loader.take().unwrap().finish().unwrap();drop(fallback);
 assert!(root_lease::RootLease::try_exclusive(&fallback_root).unwrap().is_some());
}
'''


class ResourceHomeLifetimeTests(unittest.TestCase):
    def test_parent_and_rest_worker_share_one_resource_generation(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required for the production worker-lifetime probe')
        with tempfile.TemporaryDirectory(prefix='horizon-home-lifetime-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            built = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            env = dict(os.environ, HORIZON_HOME_LIFETIME_FIXTURE=str(Path(folder) / 'home'))
            ran = subprocess.run([str(binary)], cwd=folder, env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
