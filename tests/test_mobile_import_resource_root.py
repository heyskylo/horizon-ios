"""Newly imported game files must become the loader's base, not only options state.

The probe executes production retry/native-start methods and resource-root
selection with real fixture paths, capturing the loader's selected inputs.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    screen = (ROOT / 'crates/horizon-client/src/game/import_screen.rs').read_text()
    launcher = (ROOT / 'crates/horizon-client/src/game/launcher.rs').read_text()
    imports = (ROOT / 'crates/horizon-client/src/import.rs').read_text()
    startup = (ROOT / 'crates/horizon-client/src/game/startup_menu.rs').read_text()
    functions = 'pub ' + method(imports, 'resources_root')
    methods = method(screen, 'load_the_files_that_arrived') + '\n' + method(startup, 'poll_menu_resources')
    if 'fn refresh_import_root(' in screen:
        methods += '\n' + method(screen, 'refresh_import_root')
    loading = launcher[launcher.index('impl Loading {'):]
    opening = loading.index('{', loading.index('"launcher-start" =>'))
    end, depth = opening + 1, 1
    while depth:
        depth += (loading[end] == '{') - (loading[end] == '}')
        end += 1
    launch = loading[opening + 1:end - 1]
    return STUBS.replace('ROOT_FUNCTION', functions) + '\nimpl Loading {\n' + methods + '\nfn launch(&mut self, request:Request)->Result<()>{' + launch + '\nOk(())}\n}\n' + CASES


STUBS = r'''
#![allow(dead_code)]
extern crate self as horizon_client;
extern crate self as log;
#[macro_export]macro_rules! info{($($t:tt)*)=>{{}}}
use std::{path::{Path,PathBuf},sync::{Mutex,mpsc::{self,TryRecvError}},thread::JoinHandle,time::Instant};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error>>;
mod import{
 use super::*;
 enum Kind{Resources}impl Kind{fn marker(&self)->&str{"horizon-resources.json"}fn folder(&self)->&str{"resources"}}
 ROOT_FUNCTION
 pub fn home()->PathBuf{PathBuf::from(std::env::var_os("HORIZON_IMPORT_ROOT_FIXTURE").unwrap()).join("home")}
}
mod horizon_ios_launcher{pub fn start(){}pub fn waiting()->bool{true}}
fn importing_archive()->bool{false}
struct Request;impl Request{fn answer(self,_:&str){}}
struct Options{resources:PathBuf}
struct Progress;impl Progress{fn new()->Self{Self}}
enum LoaderMsg{Ui(usize,PathBuf),Done(std::result::Result<(),String>)}
fn panic_text(_:&(dyn std::any::Any+Send))->&str{"fixture panic"}
struct Loading{loaded:Option<std::result::Result<(),String>>,delivered:bool,ui_seen:Vec<usize>,handle:Option<JoinHandle<()>>,progress:Progress,options:Option<Options>,settings:(),vanilla_root:PathBuf,started:Instant,receiver:mpsc::Receiver<LoaderMsg>}
impl Loading{
 fn set_ui(&mut self,gui:usize,_:PathBuf){self.ui_seen.push(gui);}
 fn restart_menu_resources(&mut self)->Result<()>{Ok(())}
 fn new(base:PathBuf,selected:PathBuf)->Self{Self{loaded:None,delivered:false,ui_seen:vec![],handle:None,progress:Progress,options:Some(Options{resources:selected}),settings:(),vanilla_root:base,started:Instant::now(),receiver:mpsc::channel().1}}
}
static CAPTURE:Mutex<Vec<(PathBuf,PathBuf)>>=Mutex::new(Vec::new());
mod startup_menu{
 use super::*;
 pub fn spawn_loader(options:&Options,_:&(),base:&Path,_:Instant,_:&Progress)->Result<(mpsc::Receiver<LoaderMsg>,JoinHandle<()>)>{
  CAPTURE.lock().unwrap().push((options.resources.clone(),base.to_owned()));
  // Read actual bytes from the exact root the production method hands over.
  std::fs::read(base.join("fixture-data"))?;
  let(tx,rx)=mpsc::channel();let root=base.to_owned();
  let generation=CAPTURE.lock().unwrap().len();
  let worker=std::thread::spawn(move||{tx.send(LoaderMsg::Ui(generation,root)).unwrap();tx.send(LoaderMsg::Done(Ok(()))).unwrap();});
  Ok((rx,worker))
 }
}
fn files(root:&Path,bytes:&[u8]){std::fs::create_dir_all(root).unwrap();std::fs::write(root.join("horizon-resources.json"),b"{}").unwrap();std::fs::write(root.join("fixture-data"),bytes).unwrap();}
'''

CASES = r'''
fn main(){
 let root=PathBuf::from(std::env::var_os("HORIZON_IMPORT_ROOT_FIXTURE").unwrap());
 let home=root.join("home");let arrived=home.join("resources");files(&arrived,b"new resource tree");
 let missing=root.join("initially-missing");
 // The import-screen retry must send the newly installed tree as both the
 // selected resources and the unmodified vanilla base of any pack overlays.
 if std::env::var_os("HORIZON_IMPORT_NATIVE_ONLY").is_none(){
 let mut retry=Loading::new(missing.clone(),missing.clone());
 retry.loaded=Some(Err("old failed generation".into()));retry.delivered=true;
 retry.load_the_files_that_arrived(home.clone()).expect("The retry still loaded the pre-import missing directory");
 assert_eq!(retry.vanilla_root,arrived);assert_eq!(retry.options.as_ref().unwrap().resources,arrived);
 assert_eq!(CAPTURE.lock().unwrap().last().unwrap(),&(arrived.clone(),arrived.clone()));
 if let Some(handle)=retry.handle.take(){handle.join().unwrap();}
 retry.poll_menu_resources().unwrap();
 assert!(matches!(retry.loaded,Some(Ok(()))),"The new imported generation's Done was never consumed");
 assert_eq!(retry.ui_seen,vec![1],"The imported UI was never published");
 assert!(retry.delivered);
 retry.loaded.take();retry.poll_menu_resources().unwrap();
 assert!(retry.loaded.is_none());assert_eq!(retry.ui_seen,vec![1],"A completed channel was consumed twice");
 // A second retry resets only generation completion, so it can publish once too.
 retry.load_the_files_that_arrived(home.clone()).unwrap();
 if let Some(handle)=retry.handle.take(){handle.join().unwrap();}
 retry.poll_menu_resources().unwrap();
 assert!(matches!(retry.loaded,Some(Ok(()))));assert_eq!(retry.ui_seen,vec![1,2]);
 }

 // Native first-run download completes while waiting in the launcher, with
 // no import-screen object to invoke files_arrived. Play must refresh its base.
 let mut native=Loading::new(missing.clone(),missing.clone());native.launch(Request).unwrap();
 assert_eq!(native.vanilla_root,arrived,"Native Play retained its initially missing resource base");
 assert_eq!(native.options.as_ref().unwrap().resources,arrived);

 // A chosen valid custom base outranks the imported home tree. A staged
 // resource-pack overlay must never become the replacement vanilla base.
 let custom=root.join("explicit");files(&custom,b"custom base");
 let overlay=root.join("overlay");files(&overlay,b"staged overlay");
 let mut explicit=Loading::new(custom.clone(),overlay.clone());
 explicit.load_the_files_that_arrived(home.clone()).unwrap();
 assert_eq!(explicit.vanilla_root,custom);assert_eq!(explicit.options.as_ref().unwrap().resources,custom);
 assert_eq!(CAPTURE.lock().unwrap().last().unwrap(),&(custom.clone(),custom));
 if let Some(handle)=explicit.handle.take(){handle.join().unwrap();}

 let mut stale_overlay=Loading::new(missing,overlay);
 stale_overlay.load_the_files_that_arrived(home).unwrap();
 assert_eq!(stale_overlay.vanilla_root,arrived);assert_eq!(stale_overlay.options.as_ref().unwrap().resources,arrived);
 if let Some(handle)=stale_overlay.handle.take(){handle.join().unwrap();}
}
'''


class MobileImportResourceRootTests(unittest.TestCase):
    def test_retry_and_native_start_use_imported_base_preserving_custom_priority(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required for the production resource-root probe')
        with tempfile.TemporaryDirectory(prefix='horizon-import-root-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            import os
            for scenario in ("retry", "native"):
                with self.subTest(entry=scenario):
                    env = dict(os.environ, HORIZON_IMPORT_ROOT_FIXTURE=str(Path(folder) / scenario))
                    if scenario == "native":
                        env["HORIZON_IMPORT_NATIVE_ONLY"] = "1"
                    run = subprocess.run([str(binary)], env=env, capture_output=True, text=True, timeout=10)
                    self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
