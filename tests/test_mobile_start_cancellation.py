"""Native start cancellation must drain real workers before replacement and ACK.

Executes the production Loading drain, rest-worker finish, preload/prepared
shutdown helpers, supervisor cancellation arm, and launcher ACK helper. GPU/UI
objects are drop-recording endpoints; this is not a UIKit/device runtime test.
"""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program(game_source=None):
    game = game_source or '\n'.join((ROOT / path).read_text() for path in [
        'crates/horizon-client/src/game.rs',
        'crates/horizon-client/src/game/loading_screen.rs',
    ])
    world = (ROOT / 'crates/horizon-client/src/game/world_loading.rs').read_text()
    launcher = (ROOT / 'crates/horizon-client/src/game/launcher.rs').read_text()
    drain = method(game, 'drain_cancelled_start')
    resources = (ROOT / 'crates/horizon-client/src/game/resource_loader.rs').read_text()
    a = resources.index('pub(super) struct WorldRestLoader {')
    b = resources.index('pub(super) fn write_report', a)
    rest = resources[a:b].rsplit('///', 1)[0]
    a = resources.index('pub(super) fn retired_rest_loaders')
    rest += resources[a:]
    rest = rest.replace('pub(super) ', '')
    point = game.index('loading.drain_cancelled_start()')
    a = game.rfind('            App::Loading(loading) => {', 0, point)
    b = game.index('                if loading.error.is_some()', point)
    supervisor = game[a:b] + 'panic!("normal handoff reached during cancellation");}\nApp::Other=>panic!("wrong state"),'
    prepared = 'pub(super) ' + method(world, 'shutdown_prepared_files')
    preload = 'pub(super) ' + method(world, 'shutdown_preload')
    a = launcher.index('enum Waiting {')
    b = launcher.index('\n/// Answer what has come back', a)
    pending = launcher[a:b]
    ack = 'pub(super) ' + method(launcher, 'acknowledge_cancelled_start')
    broker = f'#[path="{ROOT / "crates/horizon-ios-launcher/src/lib.rs"}"]mod horizon_ios_launcher;\n'
    return (STUBS + broker + rest + '\nimpl Loading {\n' + drain + '\n}\n'
            + '\nmod world_loading {use super::*;\n' + WORLD_STUBS + preload + prepared + '\n}\n'
            + '\nmod launcher {use super::*;\n' + pending + ack + LAUNCHER_STUBS + '\n}\n'
            + '\nimpl App {fn tick(&mut self,event_loop:&EventLoop){match self {\n' + supervisor + '\n}}}\n' + CASES)


STUBS = r'''
#![allow(dead_code,unused_imports)]
extern crate self as horizon_client;
extern crate self as horizon_airplay;
extern crate self as horizon_hooks;
mod engine {pub fn shutdown(){}}
extern crate self as log;
#[macro_export]macro_rules! warn{($($t:tt)*)=>{{}}}
use std::{cell::{Cell,RefCell},fs,path::PathBuf,sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering},mpsc::{self,Receiver}},thread::{self,JoinHandle},time::{Duration,Instant}};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error+Send+Sync>>;
static TRACE:Mutex<Vec<&'static str>>=Mutex::new(Vec::new());
static REPLIES:Mutex<Vec<(String,String)>>=Mutex::new(Vec::new());
static PARENT_DONE:AtomicBool=AtomicBool::new(false);
static TAIL_DONE:AtomicBool=AtomicBool::new(false);
static PRELOAD_DONE:AtomicBool=AtomicBool::new(false);
static PREPARED_DONE:AtomicBool=AtomicBool::new(false);
static BROWSER_DONE:AtomicBool=AtomicBool::new(false);
static RETIRED_BROWSER_DONE:AtomicBool=AtomicBool::new(false);
static AUDIO_DONE:AtomicBool=AtomicBool::new(true);
static AUDIO_POLLS:AtomicUsize=AtomicUsize::new(0);
static COLLECTOR_DONE:AtomicBool=AtomicBool::new(true);
static COLLECTOR_CANCELS:AtomicUsize=AtomicUsize::new(0);
static NEW_COLLECTION_DONE:AtomicBool=AtomicBool::new(true);
static COLLECTIONS:AtomicUsize=AtomicUsize::new(0);
fn trace(s:&'static str){TRACE.lock().unwrap().push(s);}
#[derive(Clone)]struct DropProbe(Arc<AtomicUsize>);impl Drop for DropProbe{fn drop(&mut self){self.0.fetch_add(1,Ordering::SeqCst);}}
struct Progress;
struct Loaded{rest_loader:Option<WorldRestLoader>,_guard:DropProbe}
enum LoaderMsg{Ui(DropProbe,PathBuf),Done(std::result::Result<Loaded,String>)}
#[derive(Clone)]struct Options{world:Option<String>,server:Option<String>,resources:PathBuf}
struct Loading{handle:Option<JoinHandle<()>>,receiver:Receiver<LoaderMsg>,loaded:Option<std::result::Result<Loaded,String>>,delivered:bool,completed_without_game:bool,gui:Option<DropProbe>,options:Option<Options>,window:Option<usize>,settings:usize,vanilla_root:PathBuf,error:Option<Box<dyn std::error::Error+Send+Sync>>,name:&'static str}
impl Loading{
 fn idle(options:Options,settings:usize,root:PathBuf)->Self{let(_,receiver)=mpsc::channel();Self{handle:None,receiver,loaded:None,delivered:false,completed_without_game:false,gui:None,options:Some(options),window:None,settings,vanilla_root:root,error:None,name:"idle"}}
 // Controlled shutdown endpoints; their actual worker ownership is exercised by Rust Jobs tests.
 fn shutdown_mod_browser(&mut self)->bool{BROWSER_DONE.load(Ordering::SeqCst)}
 fn poll_launcher(&mut self)->Result<()>{Ok(())}
 fn about_to_wait(&mut self,_:&EventLoop){panic!("normal loading loop ran while canceling");}
 fn set_ui(&mut self,_:DropProbe,_:PathBuf){panic!("canceled UI was uploaded");}
}
impl Drop for Loading{fn drop(&mut self){if self.name=="old"{trace("old_loading_dropped");}}}
enum App{Loading(Box<Loading>),Other}
enum ControlFlow{WaitUntil(Instant)}
#[derive(Default)]struct EventLoop{waits:Cell<usize>,exited:Cell<bool>}
impl EventLoop{fn set_control_flow(&self,_:ControlFlow){self.waits.set(self.waits.get()+1);}fn exit(&self){self.exited.set(true);}}
fn launcher_loading(options:Options,settings:usize,root:PathBuf,_:Option<()>,_:Instant)->Result<Loading>{assert!(options.world.is_none()&&options.server.is_none());assert_eq!(options.resources,root);trace("replacement_constructed");Ok(Loading::idle(options,settings,root))}
mod heartbeat{pub fn stop(){super::trace("heartbeat_stopped");}}
mod mod_browser{pub fn shutdown_workers()->bool{super::RETIRED_BROWSER_DONE.load(super::Ordering::SeqCst)}}
mod audio{pub fn poll_retired_workers()->bool{super::AUDIO_POLLS.fetch_add(1,super::Ordering::SeqCst);super::AUDIO_DONE.load(super::Ordering::SeqCst)}}
mod mod_content{
 pub fn cancel_root_collectors(){super::COLLECTOR_CANCELS.fetch_add(1,super::Ordering::SeqCst);}
 pub fn poll_root_collectors()->bool{super::COLLECTOR_DONE.load(super::Ordering::SeqCst)&&super::NEW_COLLECTION_DONE.load(super::Ordering::SeqCst)}
 pub fn schedule_root_collection(home:super::PathBuf)->super::Result<()>{
  assert_eq!(home,std::env::current_dir().unwrap());assert!(super::COLLECTOR_DONE.load(super::Ordering::SeqCst));
  let trace=super::TRACE.lock().unwrap();assert!(trace.contains(&"old_loading_dropped")&&trace.contains(&"world_data_released"));drop(trace);
  super::trace("collection_scheduled");super::COLLECTIONS.fetch_add(1,super::Ordering::SeqCst);super::NEW_COLLECTION_DONE.store(false,super::Ordering::SeqCst);Ok(())
 }
}
mod discord_presence{pub enum Status{Off}pub fn set(_:Status){super::trace("presence_off");}}
pub fn screen(first:bool,second:bool){assert!(!first&&!second);trace("airplay_idle");}
struct Installed;
fn wait_for(mut ready:impl FnMut()->bool){let deadline=Instant::now()+Duration::from_secs(3);while !ready(){assert!(Instant::now()<deadline,"worker did not finish");thread::yield_now();}}
fn reply(label:&str)->horizon_ios_launcher::Request{let label=label.to_owned();horizon_ios_launcher::Request::new(label.clone(),move|json|{
 if json.contains("\"stopped\""){assert!(PARENT_DONE.load(Ordering::SeqCst)&&TAIL_DONE.load(Ordering::SeqCst)&&PRELOAD_DONE.load(Ordering::SeqCst)&&PREPARED_DONE.load(Ordering::SeqCst)&&BROWSER_DONE.load(Ordering::SeqCst)&&RETIRED_BROWSER_DONE.load(Ordering::SeqCst)&&AUDIO_DONE.load(Ordering::SeqCst)&&COLLECTOR_DONE.load(Ordering::SeqCst),"ACK preceded producer shutdown");let t=TRACE.lock().unwrap().clone();let drop=t.iter().position(|s|*s=="old_loading_dropped").expect("ACK preceded state replacement/drop");let release=t.iter().position(|s|*s=="world_data_released").expect("ACK preceded world data release");let collect=t.iter().position(|s|*s=="collection_scheduled").expect("ACK preceded retired-root scheduling");assert!(drop<release&&release<collect);}
 REPLIES.lock().unwrap().push((label,json.to_owned()));
})}
'''

WORLD_STUBS = r'''
enum Preload{Idle,Started{thread:JoinHandle<()>},Used(Option<JoinHandle<()>>)}
static PRELOAD:Mutex<Preload>=Mutex::new(Preload::Idle);
static PREPARED_WRITERS:Mutex<Vec<(PathBuf,JoinHandle<()>)>>=Mutex::new(Vec::new());
pub(super) fn install_preload(worker:JoinHandle<()>){*PRELOAD.lock().unwrap()=Preload::Used(Some(worker));}
pub(super) fn install_prepared(path:PathBuf,worker:JoinHandle<()>){PREPARED_WRITERS.lock().unwrap().push((path,worker));}
pub(super) fn prepared_finished()->bool{PREPARED_WRITERS.lock().unwrap().iter().all(|(_,w)|w.is_finished())}
pub(super) fn prepared_empty()->bool{PREPARED_WRITERS.lock().unwrap().is_empty()}
pub(super) fn release_world_data(){trace("world_data_released");}
'''

LAUNCHER_STUBS = r'''
pub(super) fn seed()->(mpsc::Sender<String>,mpsc::Sender<std::result::Result<Installed,String>>){
 let(control,control_rx)=mpsc::channel();let(import,import_rx)=mpsc::channel();
 PENDING.with_borrow_mut(|p|p.extend([
 (Waiting::CancelStart("A".into()),reply("cancel A first")),
 (Waiting::Control(control_rx),reply("control")),
 (Waiting::CancelStart("B".into()),reply("cancel B")),
 (Waiting::Import(import_rx),reply("import")),
 (Waiting::CancelStart("A".into()),reply("cancel A second")),
 ]));(control,import)
}
pub(super) fn check_untouched(){PENDING.with_borrow(|p|{assert_eq!(p.len(),3);assert_eq!(p.iter().map(|(_,r)|r.line.as_str()).collect::<Vec<_>>(),["control","cancel B","import"]);for(w,_)in p{match w{Waiting::Control(r)=>assert_eq!(r.try_recv().unwrap(),"control still alive"),Waiting::Import(r)=>assert!(r.try_recv().unwrap().is_ok()),Waiting::CancelStart(s)=>assert_eq!(s,"B")}}});}
pub(super) fn clear(){PENDING.with_borrow_mut(|p|p.clear());}
'''

CASES = r'''
fn main(){
 let root=PathBuf::from(std::env::var_os("HORIZON_CANCEL_FIXTURE_ROOT").unwrap());
 let ui_drops=Arc::new(AtomicUsize::new(0));let loaded_drops=Arc::new(AtomicUsize::new(0));let gui_drops=Arc::new(AtomicUsize::new(0));
 let(parent_tx,parent_rx)=mpsc::channel();let(release_parent,parent_release)=mpsc::channel();let(parent_started,parent_started_rx)=mpsc::channel();
 let(release_tail,tail_release)=mpsc::channel();let(tail_started,tail_started_rx)=mpsc::channel();
 let(tail_finished,tail_finished_rx)=mpsc::channel();
 let ui=DropProbe(ui_drops.clone());let loaded=DropProbe(loaded_drops.clone());
 let parent=thread::spawn(move||{let tail=thread::spawn(move||{tail_started.send(()).unwrap();tail_release.recv().unwrap();TAIL_DONE.store(true,Ordering::SeqCst);tail_finished.send(()).unwrap();Ok(())});
 parent_tx.send(LoaderMsg::Ui(ui,PathBuf::from("unused UI"))).unwrap();parent_started.send(()).unwrap();parent_release.recv().unwrap();parent_tx.send(LoaderMsg::Done(Ok(Loaded{rest_loader:Some(WorldRestLoader{progress:Progress,handle:Some(tail)}),_guard:loaded}))).unwrap();PARENT_DONE.store(true,Ordering::SeqCst);
 });
 parent_started_rx.recv().unwrap();tail_started_rx.recv().unwrap();
 let(release_preload,preload_release)=mpsc::channel();let(preload_finished,preload_finished_rx)=mpsc::channel();
 world_loading::install_preload(thread::spawn(move||{preload_release.recv().unwrap();PRELOAD_DONE.store(true,Ordering::SeqCst);preload_finished.send(()).unwrap();}));
 let(release_prepared,prepared_release)=mpsc::channel();let path=root.join("prepared-written");let target=path.clone();
 world_loading::install_prepared(path.clone(),thread::spawn(move||{prepared_release.recv().unwrap();fs::write(target,b"finished").unwrap();PREPARED_DONE.store(true,Ordering::SeqCst);}));
 horizon_ios_launcher::reset();horizon_ios_launcher::set_covered(true);assert!(horizon_ios_launcher::start_owned(Some("A")));assert_eq!(horizon_ios_launcher::request_cancel("A"),horizon_ios_launcher::CancelDecision::Accepted);
 let(control,import)=launcher::seed();launcher::acknowledge_cancelled_start("unrelated");assert!(REPLIES.lock().unwrap().is_empty());
 let options=Options{world:Some("do not open".into()),server:Some("do not connect".into()),resources:root.join("old-overlay")};
 let loading=Loading{handle:Some(parent),receiver:parent_rx,loaded:None,delivered:false,completed_without_game:false,gui:Some(DropProbe(gui_drops.clone())),options:Some(options),window:Some(99),settings:7,vanilla_root:root.clone(),error:None,name:"old"};
 let mut app=App::Loading(Box::new(loading));let events=EventLoop::default();
 app.tick(&events);assert!(REPLIES.lock().unwrap().is_empty());assert_eq!(ui_drops.load(Ordering::SeqCst),0);assert!(!path.exists());
 let App::Loading(l)=&app else{panic!()};assert!(!l.delivered&&l.handle.is_some());
 release_parent.send(()).unwrap();wait_for(||match &app{App::Loading(l)=>l.handle.as_ref().unwrap().is_finished(),_=>false});
 app.tick(&events);assert_eq!(ui_drops.load(Ordering::SeqCst),1);assert_eq!(loaded_drops.load(Ordering::SeqCst),0);assert_eq!(gui_drops.load(Ordering::SeqCst),0);assert!(REPLIES.lock().unwrap().is_empty());
 release_tail.send(()).unwrap();tail_finished_rx.recv().unwrap();wait_for(||match &app{App::Loading(l)=>l.loaded.as_ref().unwrap().as_ref().unwrap().rest_loader.as_ref().unwrap().ready().unwrap(),_=>false});
 AUDIO_DONE.store(false,Ordering::SeqCst);COLLECTOR_DONE.store(false,Ordering::SeqCst);
 app.tick(&events);let App::Loading(l)=&app else{panic!()};assert!(l.loaded.as_ref().unwrap().as_ref().unwrap().rest_loader.is_none());assert!(REPLIES.lock().unwrap().is_empty());assert_eq!(loaded_drops.load(Ordering::SeqCst),0);
 assert!(AUDIO_POLLS.load(Ordering::SeqCst)>0&&COLLECTOR_CANCELS.load(Ordering::SeqCst)>0);
 AUDIO_DONE.store(true,Ordering::SeqCst);app.tick(&events);assert!(REPLIES.lock().unwrap().is_empty());assert_eq!(loaded_drops.load(Ordering::SeqCst),0,"collector still owns the generation");
 COLLECTOR_DONE.store(true,Ordering::SeqCst);app.tick(&events);assert!(REPLIES.lock().unwrap().is_empty(),"preload still owns the generation");
 release_preload.send(()).unwrap();preload_finished_rx.recv().unwrap();app.tick(&events);assert!(REPLIES.lock().unwrap().is_empty());assert!(!world_loading::prepared_empty());assert!(!path.exists());
 release_prepared.send(()).unwrap();wait_for(world_loading::prepared_finished);app.tick(&events);
 assert!(REPLIES.lock().unwrap().is_empty());assert!(!TRACE.lock().unwrap().contains(&"old_loading_dropped"));
 BROWSER_DONE.store(true,Ordering::SeqCst);app.tick(&events);
 assert!(REPLIES.lock().unwrap().is_empty(),"retired browser producer still owns work");
 RETIRED_BROWSER_DONE.store(true,Ordering::SeqCst);app.tick(&events);
 assert_eq!(ui_drops.load(Ordering::SeqCst),1);assert_eq!(loaded_drops.load(Ordering::SeqCst),1);assert_eq!(gui_drops.load(Ordering::SeqCst),1);assert!(world_loading::prepared_empty());assert_eq!(fs::read(path).unwrap(),b"finished");
 assert!(!events.exited.get());assert!(events.waits.get()>=3);let App::Loading(l)=&app else{panic!()};assert_eq!(l.name,"idle");assert_eq!(l.window,Some(99));
 {let replies=REPLIES.lock().unwrap();assert_eq!(replies.len(),2);assert!(replies.iter().all(|(label,text)|label.starts_with("cancel A")&&text==r#"{"ok":true,"status":"stopped"}"#));}
 assert_eq!(COLLECTIONS.load(Ordering::SeqCst),1);let before=events.waits.get();app.tick(&events);assert!(events.waits.get()>before,"replacement loader must wait for newly scheduled finite collection");NEW_COLLECTION_DONE.store(true,Ordering::SeqCst);
 launcher::acknowledge_cancelled_start("A");assert_eq!(REPLIES.lock().unwrap().len(),2);control.send("control still alive".into()).unwrap();import.send(Ok(Installed)).unwrap();launcher::check_untouched();launcher::clear();assert_eq!(REPLIES.lock().unwrap().len(),5);
 // A failed Done still drains and drops the menu, without trying to surface
 // the canceled error or upload its UI. Repeated draining is idempotent.
 let(tx,rx)=mpsc::channel();let failed_ui=DropProbe(ui_drops.clone());let failed=thread::spawn(move||{tx.send(LoaderMsg::Ui(failed_ui,PathBuf::new())).unwrap();tx.send(LoaderMsg::Done(Err("resource parse failed".into()))).unwrap();});wait_for(||failed.is_finished());
 let mut l=Loading::idle(Options{world:None,server:None,resources:root.clone()},7,root);l.handle=Some(failed);l.receiver=rx;l.gui=Some(DropProbe(gui_drops.clone()));assert!(l.drain_cancelled_start());assert!(l.drain_cancelled_start());assert!(l.loaded.is_none()&&l.gui.is_none()&&l.delivered&&l.handle.is_none()&&l.error.is_none());assert_eq!(ui_drops.load(Ordering::SeqCst),2);assert_eq!(gui_drops.load(Ordering::SeqCst),2);
 println!("parent/tail/preload/prepared drained; UI discarded; replacement precedes matching ACK; stale/import/control requests retained; failed Done and repeated drain safe");
}
'''


class MobileStartCancellationTests(unittest.TestCase):
    def test_loading_cancel_waits_for_producers_and_acknowledges_after_replacement(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required for the native cancellation probe')
        with tempfile.TemporaryDirectory(prefix='horizon-cancel-start-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            built = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            env = os.environ.copy()
            env['HORIZON_CANCEL_FIXTURE_ROOT'] = folder
            ran = subprocess.run([str(binary)], cwd=folder, env=env, capture_output=True, text=True, timeout=15)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
            self.assertIn('replacement precedes matching ACK', ran.stdout)
