"""CPU reload ownership must survive until finite frame-thread application/discard.

Runs production spawn_pack_reload/PackReload and its actual staged destructure
against controlled filesystem read/apply endpoints; this is not a GPU test.
"""
from pathlib import Path
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = Path(os.environ.get('HORIZON_PACK_RELOAD_SOURCE', ROOT / 'crates/horizon-client/src/game/packs.rs')).read_text()
    imports = (ROOT / 'crates/horizon-client/src/import.rs').read_text()
    a = imports.rfind('#[derive', 0, imports.index('struct HomeGate {'))
    b = imports.rfind('#[derive', a, imports.index('pub enum Kind'))
    access = imports[a:b]
    a = source.rfind('#[derive', 0, source.index('pub(super) enum ReloadKind'))
    b = source.index('#[cfg(test)]', a)
    worker = source[a:b].replace('pub(super)', 'pub(crate)')
    spawn = 'pub(crate) ' + method(source, 'spawn_pack_reload')
    a = source.index('        let StagedPack {', source.index('fn apply_staged_pack('))
    b = source.index('} = staged;', a) + len('} = staged;')
    destructure = source[a:b]
    a = source.index('pub(super) struct StagedPack {')
    b = source.index('\n}', a)
    fields = re.findall(r'^\s*(?:pub\s+)?(\w+):', source[a:b], re.M)
    declaration = 'struct StagedPack{' + ','.join(f'{name}:' + ('PathBuf' if name == 'root' else 'Option<import::HomeAccess>' if name == 'home_access' else '()') for name in fields) + '}\n'
    values = ','.join(f'{name}:' + ('root.to_owned()' if name == 'root' else 'None' if name == 'home_access' else '()') for name in fields)
    stage = STAGE.replace('STAGED_FIELDS', values)
    return STUBS.replace('HOME_ACCESS_API', access) + declaration + stage + '\nmod production{use super::*;\n' + worker + '\nimpl Game{\n' + spawn + '\n}\n' + HELPERS.replace('STAGED_DESTRUCTURE', destructure) + '\n}\n' + CASES


STUBS = r'''
#![allow(dead_code,unused_variables)]
extern crate self as horizon_client;
use std::{collections::BTreeMap,fs,path::{Path,PathBuf},sync::{Arc,Mutex,OnceLock,PoisonError,Weak,Condvar,mpsc},time::{Duration,Instant}};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error+Send+Sync>>;
mod import{use super::*;HOME_ACCESS_API
 pub fn home()->PathBuf{PathBuf::from(std::env::var_os("HORIZON_PACK_HOME_FIXTURE").unwrap())}
}
#[derive(Clone)]struct Progress;impl Progress{fn with(_:&[&str])->Self{Self}}
const RELOAD_LINES:[&str;0]=[];
#[derive(Clone)]struct Mods{enabled:Vec<String>}
struct Settings{resource_packs:Vec<String>,mods:Mods,force_unicode:bool,japanese_glyphs:bool}
struct Ui{vanilla_root:PathBuf}
struct Options{dev_mods:Vec<PathBuf>}
struct ServerPacks{active:Vec<()>}
struct Pack{assets:Arc<()>,context:()}
struct Game{keys:Vec<()>,physical_keys:Vec<()>,left:bool,right:bool,ui:Ui,settings:Settings,server_packs:ServerPacks,options:Options,pack:Pack,res:Arc<()>,pack_reload:Option<production::PackReload>}
impl Game{
 fn vanilla_root(&self)->PathBuf{import::home().join("resources")}
 fn new()->Self{Self{keys:vec![],physical_keys:vec![],left:false,right:false,ui:Ui{vanilla_root:PathBuf::new()},settings:Settings{resource_packs:vec![],mods:Mods{enabled:vec![]},force_unicode:false,japanese_glyphs:false},server_packs:ServerPacks{active:vec![]},options:Options{dev_mods:vec![]},pack:Pack{assets:Arc::new(()),context:()},res:Arc::new(()),pack_reload:None}}
}
static CORE:Mutex<String>=Mutex::new(String::new());
'''

STAGE = r'''
fn stage_pack(root:&Path,_:&Vec<String>,_:&Vec<()>,_:&Vec<String>,_:&Vec<PathBuf>,_:&Arc<()>,_:&Arc<()>,_:&(),_:bool,_:bool,_:&Progress)->Result<StagedPack>{
 *CORE.lock().unwrap()=fs::read_to_string(root.join("generation"))?;
 Ok(StagedPack{STAGED_FIELDS})
}
'''
HELPERS = r'''
pub fn staged(game:&mut Game)->StagedPack{
 let deadline=Instant::now()+Duration::from_secs(3);loop{
  if let Some((_,result))=game.pack_reload.as_mut().unwrap().finished(){return result.unwrap();}
  assert!(Instant::now()<deadline);std::thread::yield_now();
 }
}
pub fn worker_done(game:&Game)->bool{game.pack_reload.as_ref().unwrap().handle.as_ref().is_none_or(|h|h.is_finished())}
pub fn discard(game:&mut Game){assert!(game.pack_reload.as_mut().unwrap().shutdown());}
pub fn apply(staged:StagedPack)->Result<String>{
 STAGED_DESTRUCTURE
 // A reader such as VillagerHats::load reads this same root after staging.
 Ok(fs::read_to_string(root.join("generation"))?)
}
'''
CASES = r'''
fn writer(home:PathBuf)->(std::thread::JoinHandle<()>,mpsc::Receiver<()>) {
 let(ready,started)=mpsc::channel();let(done,finished)=mpsc::channel();
 let thread=std::thread::spawn(move||{ready.send(()).unwrap();import::with_home_access(&home,||{fs::write(home.join("resources/generation"),"after")?;Ok(())}).unwrap();done.send(()).unwrap();});
 started.recv().unwrap();(thread,finished)
}
fn main(){
 let home=import::home();fs::create_dir_all(home.join("resources")).unwrap();fs::write(home.join("resources/generation"),"before").unwrap();
 let mut game=Game::new();game.spawn_pack_reload(production::ReloadKind::Selection).unwrap();
 let staged=production::staged(&mut game);let core=CORE.lock().unwrap().clone();
 let(thread,finished)=writer(home.clone());let escaped=finished.recv_timeout(Duration::from_millis(150)).is_ok();
 let applied=production::apply(staged).unwrap();thread.join().unwrap();
 eprintln!("staged={core}; applied={applied}; writer_escaped={escaped}");
 assert!(!escaped,"Importer replaced the staged generation before the frame-thread apply read it");
 assert_eq!(core,applied);assert_eq!(fs::read_to_string(home.join("resources/generation")).unwrap(),"after");
 // Discarding a completed worker also releases ownership; no UI apply is needed.
 game.spawn_pack_reload(production::ReloadKind::Selection).unwrap();
 let deadline=Instant::now()+Duration::from_secs(3);while !production::worker_done(&game){assert!(Instant::now()<deadline);std::thread::yield_now();}
 let(thread,finished)=writer(home);assert!(finished.recv_timeout(Duration::from_millis(150)).is_err());
 production::discard(&mut game);finished.recv_timeout(Duration::from_secs(3)).unwrap();thread.join().unwrap();
}
'''


class PackReloadHomeLifetimeTests(unittest.TestCase):
    def test_staged_generation_survives_until_apply_or_discard(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required for the production reload-lifetime probe')
        with tempfile.TemporaryDirectory(prefix='horizon-pack-home-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            built = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            env = dict(os.environ, HORIZON_PACK_HOME_FIXTURE=str(Path(folder) / 'home'))
            ran = subprocess.run([str(binary)], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
