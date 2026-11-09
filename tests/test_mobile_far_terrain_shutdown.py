"""Actual supervisor readiness gate must include both far-terrain producers."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = '\n'.join((ROOT / path).read_text() for path in [
        'crates/horizon-client/src/game.rs',
        'crates/horizon-client/src/game/loading_screen.rs',
        'crates/horizon-client/src/game/world_update.rs',
    ])
    start = source.index('                    let mut stopped = game.audio.shutdown_workers();')
    start = source.rfind('                    horizon_client::mod_content::cancel_root_collectors();', 0, start)
    end = source.index('                    game.exiting(event_loop);', start)
    gate = source[start:end]
    a = source.index('struct FarTerrainWorkers {')
    b = source.index('\n}', a) + 2
    workers = source[a:b].replace('horizon_client::lod::Lod', 'Worker').replace('horizon_client::bluemap::BlueMap', 'Worker')
    workers += '\nimpl FarTerrainWorkers {' + method(source[source.index('impl FarTerrainWorkers {'):], 'shutdown') + '}\n'
    methods = ''.join(method(source, name) for name in ('shutdown_far_terrain', 'poll_retired_far_terrain', 'clear_lod', 'shutdown_skin_workers', 'poll_retired_skin_workers', 'retire_guest_skin_workers', 'poll_recording_workers', 'shutdown_recording_workers', 'retire_recording')).replace("Game<'_>", 'Game').replace('horizon_client::net::Session', 'Session')
    splitscreen = (ROOT / 'crates/horizon-client/src/game/splitscreen.rs').read_text()
    methods += method(splitscreen, 'stop_splitscreen')
    return STUBS + workers + '\nimpl Game {' + methods + '}\nfn poll(game:&mut Game,event_loop:&EventLoop){' + gate + 'READY.store(true,Ordering::SeqCst);}\n' + CASES


STUBS = r'''
#![allow(dead_code)]
extern crate self as horizon_client;
use std::{cell::RefCell,sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}},time::{Instant,Duration},thread::JoinHandle};
static READY:AtomicBool=AtomicBool::new(false);
static REST_READY:AtomicBool=AtomicBool::new(true);
// The real retirement registry has Rust barrier tests; this endpoint controls
// its readiness while executing the actual supervisor's all-producer gate.
fn poll_retired_rest_loaders()->bool{REST_READY.load(Ordering::SeqCst)}
struct Worker{ready:AtomicBool,stop:AtomicBool}
static RETIRED_AUDIO:Worker=Worker{ready:AtomicBool::new(true),stop:AtomicBool::new(false)};
static COLLECTOR:Worker=Worker{ready:AtomicBool::new(true),stop:AtomicBool::new(false)};
struct Audio(Worker);impl Audio{fn shutdown_workers(&mut self)->bool{self.0.shutdown()}}
mod audio{pub fn poll_retired_workers()->bool{super::RETIRED_AUDIO.shutdown()}}
mod mod_content{pub fn cancel_root_collectors(){super::COLLECTOR.stop.store(true,super::Ordering::SeqCst);}pub fn poll_root_collectors()->bool{super::COLLECTOR.ready.load(super::Ordering::SeqCst)}}
impl Worker{fn new()->Self{Self{ready:AtomicBool::new(false),stop:AtomicBool::new(false)}}fn shutdown(&self)->bool{self.stop.store(true,Ordering::SeqCst);self.ready.load(Ordering::SeqCst)}}
#[derive(Clone)] struct WorkHandle(Arc<Worker>);impl WorkHandle{fn shutdown(&self)->bool{self.0.shutdown()}}
struct SkinManager(Arc<Worker>);impl SkinManager{fn new(ready:bool)->Self{let worker=Worker::new();worker.ready.store(ready,Ordering::SeqCst);Self(Arc::new(worker))}fn shutdown(&mut self)->bool{self.0.shutdown()}fn retire_workers(&self)->WorkHandle{self.0.stop.store(true,Ordering::SeqCst);WorkHandle(self.0.clone())}}
struct Session{cancels:AtomicUsize,recording:Option<WorkHandle>}impl Session{fn recording_work(&self)->Option<WorkHandle>{self.recording.clone()}fn cancel_far_terrain(&self){self.cancels.fetch_add(1,Ordering::SeqCst);}fn disconnect(&mut self){}}
struct Remote{session:Session}struct Renderer;impl Renderer{fn shutdown_pack_compiler(&mut self)->bool{true}fn clear_lods(&mut self){}}
struct Rest{handle:JoinHandle<()>}
impl Rest{fn ready(&self)->std::result::Result<bool,()>{Ok(self.handle.is_finished())}}
struct Game{audio:Audio,remote:Option<Remote>,lod:Option<Worker>,lod_blue_map:Option<Arc<Worker>>,renderer:Option<Renderer>,rest_loader:Option<Rest>,retired_far_terrain:Vec<FarTerrainWorkers>,lod_source_attempted:bool,lod_held:Vec<()>,lod_observe:Vec<()>,far_beacons:Vec<()>,lod_covered_at:Option<()>,splitscreen_guest:Option<Box<Game>>,skin_profiles:RefCell<SkinManager>,skin_downloads:RefCell<SkinManager>,retired_skin_workers:Vec<WorkHandle>,retired_recording_workers:Vec<WorkHandle>,studio_recording_notice_pending:bool}
impl Game{fn retire_guest_culling_workers(&mut self,_:&mut Game){}fn shutdown_server_pack_worker(&mut self)->bool{true}fn shutdown_culling_workers(&mut self)->bool{true}fn shutdown_integrated_workers(&mut self)->bool{true}fn end_splitscreen(&mut self){}}
mod shader_packs{pub fn shutdown_parser()->bool{true}}
mod world_loading{pub fn shutdown_preload()->bool{true}pub fn shutdown_prepared_files()->bool{true}}
enum ControlFlow{WaitUntil(Instant)}struct EventLoop;impl EventLoop{fn set_control_flow(&self,_:ControlFlow){}}
'''

CASES = r'''
fn game_with_workers()->Game{let audio=Audio(Worker::new());audio.0.ready.store(true,Ordering::SeqCst);Game{audio,remote:Some(Remote{session:Session{cancels:AtomicUsize::new(0),recording:None}}),lod:Some(Worker::new()),lod_blue_map:Some(Arc::new(Worker::new())),renderer:None,rest_loader:None,retired_far_terrain:Vec::new(),lod_source_attempted:true,lod_held:vec![],lod_observe:vec![],far_beacons:vec![],lod_covered_at:None,splitscreen_guest:None,skin_profiles:RefCell::new(SkinManager::new(true)),skin_downloads:RefCell::new(SkinManager::new(true)),retired_skin_workers:vec![],retired_recording_workers:vec![],studio_recording_notice_pending:false}}
fn main(){
 let mut g=game_with_workers();
 poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));
 assert!(g.lod.as_ref().unwrap().stop.load(Ordering::SeqCst));assert!(g.lod_blue_map.as_ref().unwrap().stop.load(Ordering::SeqCst),"stop must reach both producers without short circuiting");
 g.lod.as_ref().unwrap().ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst),"BlueMap outlived the game readiness gate");
 g.lod_blue_map.as_ref().unwrap().ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));
 assert_eq!(g.remote.as_ref().unwrap().session.cancels.load(Ordering::SeqCst),3);
 REST_READY.store(false,Ordering::SeqCst);READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst),"retired resource tail outlived the engine gate");
 REST_READY.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));
 g.skin_profiles=RefCell::new(SkinManager::new(false));g.skin_downloads=RefCell::new(SkinManager::new(false));READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));assert!(g.skin_downloads.borrow().0.stop.load(Ordering::SeqCst));
 g.skin_profiles.borrow().0.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));g.skin_downloads.borrow().0.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));
 // An ordinary world/source change retires workers without dropping them
 // while unfinished, and later launcher shutdown also waits for that cohort.
 g.lod=Some(Worker::new());g.lod_blue_map=Some(Arc::new(Worker::new()));g.clear_lod();
 assert!(g.lod.is_none()&&g.lod_blue_map.is_none());assert_eq!(g.retired_far_terrain.len(),1);
 READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));
 g.retired_far_terrain[0].lod.as_ref().unwrap().ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));
 g.retired_far_terrain[0].blue_map.as_ref().unwrap().ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));assert!(g.retired_far_terrain.is_empty());
 // Recording finalization is independent of terrain/skin readiness.
 let recording=Arc::new(Worker::new());g.remote.as_mut().unwrap().session.recording=Some(WorkHandle(recording.clone()));
 READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));assert!(recording.stop.load(Ordering::SeqCst));
 recording.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));
 let retired=Arc::new(Worker::new());g.retired_recording_workers.push(WorkHandle(retired.clone()));
 READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));retired.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));assert!(g.retired_recording_workers.is_empty());
 g.remote=None;g.lod=None;g.lod_blue_map=None;READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));
 // Removing the AirPlay guest transfers its unfinished cohorts to the host.
 let mut child=game_with_workers();child.skin_profiles=RefCell::new(SkinManager::new(false));child.skin_downloads=RefCell::new(SkinManager::new(false));child.retired_skin_workers.push(WorkHandle(Arc::new(Worker::new())));
 let guest_recording=Arc::new(Worker::new());child.remote.as_mut().unwrap().session.recording=Some(WorkHandle(guest_recording.clone()));
 g.splitscreen_guest=Some(Box::new(child));g.stop_splitscreen();assert!(g.splitscreen_guest.is_none());assert_eq!(g.retired_far_terrain.len(),1);assert_eq!(g.retired_skin_workers.len(),3);assert_eq!(g.retired_recording_workers.len(),1);
 READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));
 g.retired_far_terrain[0].lod.as_ref().unwrap().ready.store(true,Ordering::SeqCst);g.retired_far_terrain[0].blue_map.as_ref().unwrap().ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));assert!(g.retired_far_terrain.is_empty());
 for worker in &g.retired_skin_workers{worker.0.ready.store(true,Ordering::SeqCst);}poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst),"guest recording outlived readiness gate");guest_recording.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));assert!(g.retired_skin_workers.is_empty());
 // Audio readers, retired readers and root collection independently retain
 // the engine, and all receive shutdown even while another one is blocked.
 g.audio.0.ready.store(false,Ordering::SeqCst);RETIRED_AUDIO.ready.store(false,Ordering::SeqCst);COLLECTOR.ready.store(false,Ordering::SeqCst);COLLECTOR.stop.store(false,Ordering::SeqCst);
 READY.store(false,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));
 assert!(g.audio.0.stop.load(Ordering::SeqCst)&&RETIRED_AUDIO.stop.load(Ordering::SeqCst)&&COLLECTOR.stop.load(Ordering::SeqCst));
 g.audio.0.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));
 RETIRED_AUDIO.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(!READY.load(Ordering::SeqCst));
 COLLECTOR.ready.store(true,Ordering::SeqCst);poll(&mut g,&EventLoop);assert!(READY.load(Ordering::SeqCst));
}
'''


class FarTerrainShutdownTests(unittest.TestCase):
    def test_supervisor_waits_for_both_far_terrain_producers(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-far-shutdown-') as folder:
            source, binary = Path(folder) / 'probe.rs', Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], cwd=folder, capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
