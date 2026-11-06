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
    return STUBS.replace('ROOT_FUNCTION', functions) + '\n' + method(launcher, 'start_owner') + '\nimpl Loading {\n' + methods + '\nfn launch(&mut self, request:Request)->Result<()>{' + launch + '\nOk(())}\n}\n' + CASES


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
mod horizon_ios_launcher{pub fn start(){}pub fn start_owned(owner:Option<&str>)->bool{assert_eq!(owner,Some("native-fixture"));true}pub fn waiting()->bool{true}}
fn importing_archive()->bool{false}
struct Request{line:String}impl Request{fn answer(self,_:&str){}}
struct Json(String);impl std::fmt::Display for Json{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(&self.0)}}
macro_rules! json{({"error": $error:expr})=>{Json(format!("error:{}",$error))};({"ok": $ok:expr, "phase": $phase:expr})=>{Json(format!(r#"{{"ok":{},"phase":"{}"}}"#,$ok,$phase))};}
struct Options{resources:PathBuf}
struct Progress;impl Progress{fn new()->Self{Self}}
enum LoaderMsg{Ui(usize,PathBuf),Done(std::result::Result<(),String>)}
fn panic_text(_:&(dyn std::any::Any+Send))->&str{"fixture panic"}
struct Loading{loaded:Option<std::result::Result<(),String>>,delivered:bool,ui_seen:Vec<usize>,handle:Option<JoinHandle<()>>,progress:Progress,options:Option<Options>,settings:(),vanilla_root:PathBuf,started:Instant,receiver:mpsc::Receiver<LoaderMsg>}
impl Loading{
 fn set_ui(&mut self,gui:usize,_:PathBuf){self.ui_seen.push(gui);}
 // Browser polling is a boundary endpoint here; its real ownership has separate Rust tests.
 fn drive_menu_mod_browser(&mut self){BROWSER_POLLS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);}
 fn restart_menu_resources(&mut self)->Result<()>{Ok(())}
 fn new(base:PathBuf,selected:PathBuf)->Self{Self{loaded:None,delivered:false,ui_seen:vec![],handle:None,progress:Progress,options:Some(Options{resources:selected}),settings:(),vanilla_root:base,started:Instant::now(),receiver:mpsc::channel().1}}
}
static CAPTURE:Mutex<Vec<(PathBuf,PathBuf)>>=Mutex::new(Vec::new());
static READ_BYTES:Mutex<Vec<Vec<u8>>>=Mutex::new(Vec::new());
static BROWSER_POLLS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
static FAIL_NEXT_SPAWN:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
mod startup_menu{
 use super::*;
 pub fn spawn_loader(options:&Options,_:&(),base:&Path,_:Instant,_:&Progress)->Result<(mpsc::Receiver<LoaderMsg>,JoinHandle<()>)>{
  CAPTURE.lock().unwrap().push((options.resources.clone(),base.to_owned()));
  if FAIL_NEXT_SPAWN.swap(false,std::sync::atomic::Ordering::SeqCst){return Err("fixture thread spawn failed".into());}
  // Read actual bytes from the exact root the production method hands over.
  READ_BYTES.lock().unwrap().push(std::fs::read(base.join("fixture-data"))?);
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
 assert!(BROWSER_POLLS.load(std::sync::atomic::Ordering::SeqCst)>=3,"resource polling omitted browser completion delivery");
 }

 // Native first-run download completes while waiting in the launcher, with
 // no import-screen object to invoke files_arrived. Play must refresh its base.
 let mut native=Loading::new(missing.clone(),missing.clone());native.launch(Request{line:"launcher-start native-fixture".into()}).unwrap();
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
 stale_overlay.load_the_files_that_arrived(home.clone()).unwrap();
 assert_eq!(stale_overlay.vanilla_root,arrived);assert_eq!(stale_overlay.options.as_ref().unwrap().resources,arrived);
 if let Some(handle)=stale_overlay.handle.take(){handle.join().unwrap();}

 // A failed OS thread spawn must retain the selected options for another
 // retry or for native cancellation to reconstruct the idle launcher.
 let mut spawn_retry=Loading::new(arrived.clone(),arrived.clone());
 FAIL_NEXT_SPAWN.store(true,std::sync::atomic::Ordering::SeqCst);
 assert!(spawn_retry.load_the_files_that_arrived(home.clone()).is_err());
 assert_eq!(spawn_retry.options.as_ref().expect("spawn failure consumed retry options").resources,arrived);
 assert!(spawn_retry.handle.is_none());
 spawn_retry.load_the_files_that_arrived(home).unwrap();
 if let Some(handle)=spawn_retry.handle.take(){handle.join().unwrap();}
 spawn_retry.poll_menu_resources().unwrap();
 assert!(matches!(spawn_retry.loaded,Some(Ok(()))),"retry after a spawn failure did not complete");
 assert_eq!(READ_BYTES.lock().unwrap().last().unwrap(),b"new resource tree");

 // Select the import base in the actual publication gap: the old valid tree
 // is already parked, and the new tree has not yet moved into resources/.
 let gap_home=root.join("publication-gap");let final_root=gap_home.join("resources");
 files(&gap_home.join("resources.previous"),b"previous generation");
 let staged=gap_home.join("resources.unpacking");files(&staged,b"published new generation");
 assert!(!final_root.exists());
 let explicit_gap_root=root.join("gap-explicit");files(&explicit_gap_root,b"valid explicit");
 let mut explicit_gap=Loading::new(explicit_gap_root.clone(),root.join("not-a-vanilla-overlay"));
 explicit_gap.refresh_import_root(&gap_home).unwrap();
 assert_eq!(explicit_gap.vanilla_root,explicit_gap_root,"publication gap displaced a valid explicit base");
 let vanished=root.join("missing-before-first-import");
 let mut gap=Loading::new(vanished.clone(),vanished);
 gap.refresh_import_root(&gap_home).unwrap();
 // Publish only after the main-thread selection, before the protected loader
 // reads its captured inputs. Do not refresh a second time and mask the bug.
 std::fs::rename(staged,&final_root).unwrap();
 let options=gap.options.as_ref().unwrap();
 let(receiver,worker)=startup_menu::spawn_loader(options,&gap.settings,&gap.vanilla_root,gap.started,&gap.progress)
  .expect("the publication-gap refresh captured its stale missing resource path");
 worker.join().unwrap();drop(receiver);
 assert_eq!(CAPTURE.lock().unwrap().last().unwrap(),&(final_root.clone(),final_root));
 assert_eq!(READ_BYTES.lock().unwrap().last().unwrap(),b"published new generation");
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
