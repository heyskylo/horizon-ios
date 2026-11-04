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
    a = game.index('    let rest_loader = if menu {', game.index('fn load_resources('))
    b = game.index('    // The world tables load (above)', a)
    rest = game[a:b]
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
    return STUBS.replace('HOME_ACCESS_FUNCTION', access) + loader + '\nmod production {use super::*;\n' + startup + '\n}\n' + LOAD_PREFIX + rest + LOAD_SUFFIX + CASES


STUBS = r'''
#![allow(dead_code)]
extern crate self as horizon_client;
extern crate self as log;
#[macro_export]macro_rules! warn{($($t:tt)*)=>{{}}}
#[macro_export]macro_rules! info{($($t:tt)*)=>{{}}}
use std::{collections::BTreeMap,fs,path::{Path,PathBuf},sync::{Arc,Mutex,OnceLock,PoisonError,Weak,Condvar,mpsc::{self,Receiver},atomic::{AtomicBool,Ordering}},thread::JoinHandle,time::{Instant,Duration}};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error+Send+Sync>>;
mod import{use super::*;HOME_ACCESS_FUNCTION
 pub fn home()->PathBuf{PathBuf::from(std::env::var_os("HORIZON_HOME_LIFETIME_FIXTURE").unwrap())}
}
#[derive(Clone)]struct Mods{enabled:Vec<String>}
#[derive(Clone)]struct Settings{resource_packs:Vec<String>,mods:Mods}
struct Options{dev_mods:Vec<PathBuf>}
fn launch_world(_: &Options)->Option<&str>{None}
mod packs{use super::*;pub fn startup_root(root:&Path,_:&Vec<String>,_:&[()])->Result<PathBuf>{Ok(root.to_owned())}}
mod mods{use super::*;pub fn content_root(root:&Path,_:&Vec<String>,_:&Vec<PathBuf>)->PathBuf{root.to_owned()}}
mod startup{pub fn step(_:&str){}}
mod telemetry{pub fn mark_rest_ready(){}}
mod world_loading{use super::*;pub fn write_prepared_files(_:PathBuf,_:Option<import::HomeAccess>){}}
#[derive(Clone)]struct Progress;impl Progress{fn set(&self,_:usize){}}
type UiResources=usize;
struct Resources{rest:OnceLock<String>}
struct Loaded{home_access:Option<import::HomeAccess>,core:String,resources:Arc<Resources>,rest_loader:Option<WorldRestLoader>,pack_error:Option<String>}
enum LoaderMsg{Ui(usize,PathBuf),Done(std::result::Result<Loaded,String>)}
static TAIL_STARTED:AtomicBool=AtomicBool::new(false);
fn tail_gate()->&'static(Mutex<bool>,Condvar){static GATE:OnceLock<(Mutex<bool>,Condvar)>=OnceLock::new();GATE.get_or_init(||(Mutex::new(false),Condvar::new()))}
fn load_world_rest(root:&Path,_:&Resources,_:&(),_:&(),_:&(),_:(),_:&Progress,_:Instant,_:bool)->Result<String>{
 TAIL_STARTED.store(true,Ordering::SeqCst);
 let(lock,ready)=tail_gate();let mut release=lock.lock().unwrap();while !*release{release=ready.wait(release).unwrap();}drop(release);
 Ok(fs::read_to_string(root.join("generation"))?)
}
'''

LOAD_PREFIX = r'''
fn load_resources_with_access(root:PathBuf,_:Option<&str>,vanilla_root:&Path,_:&Settings,started:Instant,menu:bool,headless:bool,progress:&Progress,home_access:Option<import::HomeAccess>,publish:&mut dyn FnMut(UiResources,&Path))->Result<Loaded>{
 let core=fs::read_to_string(root.join("generation"))?;
 publish(0,&root);
 let resources=Arc::new(Resources{rest:OnceLock::new()});
 let item_defaults=();let assets=();let behaviour_json=();let gameplay_projection=();let visibility=();
'''
LOAD_SUFFIX = r'''
 Ok(Loaded{home_access,core,resources,rest_loader,pack_error:None})
}
'''

CASES = r'''
fn main(){
 let home=import::home();fs::create_dir_all(&home).unwrap();let root=home.join("resources");fs::create_dir_all(&root).unwrap();fs::write(root.join("generation"),"before").unwrap();
 let settings=Settings{resource_packs:vec![],mods:Mods{enabled:vec![]}};let options=Options{dev_mods:vec![]};
 let(rx,parent)=production::spawn_loader(&options,&settings,&root,Instant::now(),&Progress).unwrap();
 let mut loaded=loop{match rx.recv_timeout(Duration::from_secs(3)).unwrap(){LoaderMsg::Ui(..)=>{},LoaderMsg::Done(result)=>break result.unwrap()}};
 parent.join().unwrap();
 let limit=Instant::now()+Duration::from_secs(3);while !TAIL_STARTED.load(Ordering::SeqCst){assert!(Instant::now()<limit);std::thread::yield_now();}
 let tail=loaded.rest_loader.take().unwrap();assert!(!tail.handle.is_finished());
 let core=loaded.core.clone();let resources=loaded.resources.clone();drop(loaded);
 let (trying,attempted)=mpsc::channel();let(done,published)=mpsc::channel();let writer_home=home.clone();let writer_root=root.clone();
 let writer=std::thread::spawn(move||{trying.send(()).unwrap();import::with_home_access(&writer_home,||{fs::write(writer_root.join("generation"),"after")?;Ok(())}).unwrap();done.send(()).unwrap();});
 attempted.recv().unwrap();let escaped=published.recv_timeout(Duration::from_millis(150)).is_ok();
 // Release the finite reader even when observing the broken ownership path.
 *tail_gate().0.lock().unwrap()=true;tail_gate().1.notify_all();tail.finish().unwrap();writer.join().unwrap();
 let rest=resources.rest.get().unwrap();
 eprintln!("core={core}; rest={rest}; writer_escaped={escaped}");
 assert!(!escaped,"An importer published after the parent returned but while its rest worker still read the same generation");
 assert_eq!(&core,rest,"Core and rest tables came from different resource generations");
 assert_eq!(fs::read_to_string(root.join("generation")).unwrap(),"after","The finite reader must eventually release the writer");
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
            ran = subprocess.run([str(binary)], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
