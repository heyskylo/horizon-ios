"""Completed shader jobs quarantine failed packs, without disabling pending/geometry jobs.

Run the production completion handler and quarantine method with a renderer
completion queue and settings persistence recorder; no shader assets or GPU.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = (ROOT / 'crates/horizon-client/src/game/shader_packs.rs').read_text()
    methods = '\n'.join(method(source, name) for name in ('poll_shader_workers', 'shader_pack_failed'))
    return STUBS + '\nimpl Game {\n' + methods + '\n}\n' + CASES


STUBS = r'''
#![allow(dead_code)]
extern crate self as log;
#[macro_export]macro_rules! warn{($($t:tt)*)=>{{}}}
#[macro_export]macro_rules! error{($($t:tt)*)=>{{}}}
use std::{cell::{Cell,RefCell},path::PathBuf,sync::mpsc};
type Report=(i32,Vec<String>,Vec<String>);
#[derive(Clone,Copy,Debug,PartialEq)]enum Api{V0_1}
thread_local!{static REMEMBERED:RefCell<Vec<(PathBuf,Option<Api>)>>=const{RefCell::new(Vec::new())};}
fn fingerprint(_:&PathBuf)->u64{42}
fn remember(path:&PathBuf,fingerprint:u64,api:Option<Api>){assert_eq!(fingerprint,42);REMEMBERED.with_borrow_mut(|r|r.push((path.clone(),api)));}

thread_local!{
 static OPTIONS_REVISION:Cell<u64>=const{Cell::new(0)};
 static PENDING_LOAD:RefCell<Option<PendingLoad>>=const{RefCell::new(None)};
 static REPORT:RefCell<Option<(String,Report)>>=const{RefCell::new(None)};
}
#[derive(PartialEq)]struct LoadKey{path:PathBuf,resources:PathBuf,options_revision:u64,dimension:i32,dimension_name:String,context:(u64,u64)}
struct PendingLoad{key:LoadKey,worker:Option<std::thread::JoinHandle<()>>,result:mpsc::Receiver<()>}
#[derive(Clone,Copy)]enum Dim{Overworld,Nether,End}
impl Dim{fn id(self)->&'static str{match self{Self::Overworld=>"minecraft:overworld",Self::Nether=>"minecraft:the_nether",Self::End=>"minecraft:the_end"}}}
struct Renderer{dimension:Option<i32>,report:Option<Report>,dropped:bool,disabled:bool}
impl Renderer{
 fn shader_pack_dimension(&self)->Option<i32>{self.dimension}
 fn drop_shader_packs(&mut self){self.report=None;self.dropped=true;}
 fn take_shader_report(&mut self)->Option<Report>{self.report.take()}
 fn shader_context_key(&self)->(u64,u64){(0,0)}
 fn set_shader_pack(&mut self,dimension:i32,_:Option<()>,_:&[i32]){self.disabled=true;self.dimension=Some(dimension);self.report=None;}
}
struct Horizon{shader_pack:String}
struct Settings{horizon:Horizon,saves:Cell<usize>}
impl Settings{fn save(&self,_:&PathBuf){self.saves.set(self.saves.get()+1);}}
fn picked_pack(s:&Settings)->&str{&s.horizon.shader_pack}
struct NativeShaders{picked:String}struct Options{resources:PathBuf}
struct Text;impl Text{fn colored(s:String,_:u32)->String{s}}
struct Game{dim:Dim,renderer:Option<Renderer>,pack_ids:Option<()>,native_shaders:NativeShaders,settings:Settings,options:Options,messages:Vec<String>}
impl Game{
 fn shader_packs_dir()->PathBuf{PathBuf::from("shaderpacks")}
 fn options_path()->PathBuf{PathBuf::from("scratch/options.txt")}
 fn system_message(&mut self,text:String){self.messages.push(text);}
 fn new(report:Option<Report>)->Self{Self{dim:Dim::Overworld,renderer:Some(Renderer{dimension:Some(0),report,dropped:false,disabled:false}),pack_ids:Some(()),native_shaders:NativeShaders{picked:"example.zip".into()},settings:Settings{horizon:Horizon{shader_pack:"example.zip".into()},saves:Cell::new(0)},options:Options{resources:PathBuf::from("scratch/resources")},messages:vec![]}}
}
fn record_report(name:&str,dimension:i32,programs:&[String],notes:&[String]){REPORT.with_borrow_mut(|r|*r=Some((name.into(),(dimension,programs.into(),notes.into()))));}
'''

CASES = r'''
#[test]fn failed_completed_gpu_job_is_disabled_saved_and_reported(){
 let notes=vec!["Device attachment limit exceeded".to_owned(),"Using normal rendering".to_owned()];
 let mut g=Game::new(Some((0,vec![],notes.clone())));g.poll_shader_workers();
 assert!(g.pack_ids.is_none());assert!(g.settings.horizon.shader_pack.is_empty());assert_eq!(g.settings.saves.get(),1);
 assert!(g.renderer.as_ref().unwrap().disabled);assert_eq!(g.messages.len(),1);
 REMEMBERED.with_borrow(|r|assert!(r.iter().all(|(p,api)|p==&PathBuf::from("shaderpacks/example.zip")&&api.is_none())));
 REPORT.with_borrow(|r|assert_eq!(r.as_ref().unwrap(),&("example.zip".into(),(0,vec![],notes))));
 g.poll_shader_workers();assert_eq!(g.settings.saves.get(),1);assert_eq!(g.messages.len(),1);
}
#[test]fn pending_gpu_job_keeps_selected_pack(){let mut g=Game::new(None);g.poll_shader_workers();assert_eq!(g.settings.horizon.shader_pack,"example.zip");assert_eq!(g.settings.saves.get(),0);assert!(g.messages.is_empty());}
#[test]fn geometry_only_completed_pack_is_not_quarantined(){let mut g=Game::new(Some((0,vec!["gbuffers_terrain".into()],vec![])));g.poll_shader_workers();assert!(g.pack_ids.is_some());assert_eq!(g.settings.horizon.shader_pack,"example.zip");assert_eq!(g.settings.saves.get(),0);assert!(!g.renderer.as_ref().unwrap().disabled);REMEMBERED.with_borrow(|r|assert_eq!(r.last().unwrap().1,Some(Api::V0_1)));}
#[test]fn obsolete_dimension_failure_cannot_disable_current_pack(){let mut g=Game::new(Some((-1,vec![],vec!["old failure".into()])));g.renderer.as_mut().unwrap().dimension=Some(-1);g.poll_shader_workers();assert!(g.renderer.as_ref().unwrap().dropped);assert_eq!(g.settings.horizon.shader_pack,"example.zip");assert_eq!(g.settings.saves.get(),0);assert!(g.messages.is_empty());}
'''


class MobileShaderCompletionTests(unittest.TestCase):
    def test_production_gpu_completion_and_quarantine(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-shader-completion-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', '--test', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn('4 passed', run.stdout)
