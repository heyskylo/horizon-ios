"""Native imports cannot overlap or release the engine while a worker owns its files.

Runs the production request dispatcher and completion queue against a blocked
installation endpoint. Archive decoding is covered by the import crate tests.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = (ROOT / 'crates/horizon-client/src/game/launcher.rs').read_text()
    start = source.index('enum Waiting {')
    end = source.index('fn deliver()', start)
    state = source[start:end]
    methods = '\n'.join(method(source, name) for name in ('deliver', 'start_import'))
    if 'fn importing_archive(' in source:
        methods += '\n' + method(source, 'importing_archive')
    loading = source[source.index('impl Loading {'):]
    opening = loading.index('{', loading.index('"launcher-start" =>'))
    end, depth = opening + 1, 1
    while depth:
        depth += (loading[end] == '{') - (loading[end] == '}')
        end += 1
    launch = loading[opening + 1:end - 1]
    launcher = ROOT / 'crates/horizon-ios-launcher/src/lib.rs'
    return STUBS.replace('LAUNCHER_PATH', str(launcher)) + state + methods + '\nimpl Loading{fn launch(&mut self,request:horizon_ios_launcher::Request)->Result<()>{' + launch + 'Ok(())}}\nfn launch(request:horizon_ios_launcher::Request){Loading.launch(request).unwrap();}\n' + CASES


STUBS = r'''
#![allow(dead_code)]
use std::{cell::RefCell,path::Path,sync::{mpsc::{self,Receiver,TryRecvError},atomic::{AtomicUsize,Ordering},Mutex,Condvar,OnceLock},time::{Duration,Instant}};
extern crate self as horizon_client;
#[path="LAUNCHER_PATH"]mod real_launcher;
mod horizon_ios_launcher{
 pub use crate::real_launcher::Request;
 pub fn start(){crate::STARTS.fetch_add(1,crate::Ordering::SeqCst);}
 pub fn waiting()->bool{true}
}
mod import{pub fn home()->std::path::PathBuf{"fixture/home".into()}}
type Result<T>=std::result::Result<T,&'static str>;
struct Loading;impl Loading{fn refresh_import_root(&mut self,_:&Path)->Result<()>{Ok(())}}
#[derive(Clone,Copy,PartialEq)]enum Kind{Resources}
struct Installed{kind:Kind}
struct Json(String);impl std::fmt::Display for Json{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(&self.0)}}
macro_rules! json{({"error": $error:expr})=>{Json(format!("error:{}",$error))};}
fn installed_json(_: &Installed)->Json{Json("ok".into())}
static STARTS:AtomicUsize=AtomicUsize::new(0);
static RUNS:AtomicUsize=AtomicUsize::new(0);
fn gate()->&'static(Mutex<bool>,Condvar){static GATE:OnceLock<(Mutex<bool>,Condvar)>=OnceLock::new();GATE.get_or_init(||(Mutex::new(false),Condvar::new()))}
fn install_archive(path:&Path,_:&Path,_:bool)->std::result::Result<Installed,String>{
 RUNS.fetch_add(1,Ordering::SeqCst);
 let (lock,signal)=gate();let mut open=lock.lock().unwrap();while !*open{open=signal.wait(open).unwrap();}drop(open);
 if path==Path::new("failed.zip"){Err("fixture decode failure".into())}else{Ok(Installed{kind:Kind::Resources})}
}
fn request(line:&str)->(horizon_ios_launcher::Request,Receiver<String>){let(tx,rx)=mpsc::channel();(horizon_ios_launcher::Request::new(line,move|answer|{tx.send(answer.into()).unwrap();}),rx)}
fn finish(rx:&Receiver<String>)->String{let limit=Instant::now()+Duration::from_secs(3);loop{deliver();if let Ok(answer)=rx.try_recv(){return answer;}assert!(Instant::now()<limit,"worker did not finish");std::thread::yield_now();}}
'''

CASES = r'''
#[test]fn native_imports_hold_the_engine_until_completion_and_recover_after_failure(){
 let(first,first_reply)=request("import first.zip");start_import(first);
 let deadline=Instant::now()+Duration::from_secs(3);
 while RUNS.load(Ordering::SeqCst)==0{assert!(Instant::now()<deadline);std::thread::yield_now();}
 let(start,start_reply)=request("launcher-start");launch(start);
 let(second,second_reply)=request("import second.zip");start_import(second);
 let start_answer=start_reply.try_recv().ok();let second_answer=second_reply.try_recv().ok();
 // Always release blocked worker threads, even when checking the old broken code.
 *gate().0.lock().unwrap()=true;gate().1.notify_all();
 let first_answer=finish(&first_reply);
 // If a broken dispatcher admitted a second worker, drain its reply before failure.
 if second_answer.is_none(){let _=finish(&second_reply);}
 assert_eq!(STARTS.load(Ordering::SeqCst),0,"Play released the loader during an import");
 assert!(start_answer.unwrap().contains("error"));
 assert!(second_answer.is_some_and(|answer|answer.contains("error")),"second import was admitted instead of rejected");
 assert_eq!(RUNS.load(Ordering::SeqCst),1,"only one archive may own the destination");
 assert_eq!(first_answer,"ok");
 // An unrelated control reply must not block Play after the archive finishes.
 let(control_sender,control_receiver)=mpsc::channel();let(control,control_reply)=request("worlds");
 PENDING.with_borrow_mut(|pending|pending.push((Waiting::Control(control_receiver),control)));
 let(start,reply)=request("launcher-start");launch(start);assert_eq!(reply.recv().unwrap(),r#"{"ok":true}"#);assert_eq!(STARTS.load(Ordering::SeqCst),1);
 drop(control_sender);deliver();assert!(control_reply.recv().unwrap().contains("error"));
 let(failed,failed_reply)=request("import failed.zip");start_import(failed);assert!(finish(&failed_reply).contains("fixture decode failure"));
 let(retry,retry_reply)=request("import retry.zip");start_import(retry);assert_eq!(finish(&retry_reply),"ok");
 let(start,reply)=request("launcher-start");launch(start);assert_eq!(reply.recv().unwrap(),r#"{"ok":true}"#);assert_eq!(STARTS.load(Ordering::SeqCst),2);
}
'''


class MobileImportLifecycleTests(unittest.TestCase):
    def test_imports_are_serial_and_engine_start_waits_for_completion(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required for the production lifecycle probe')
        with tempfile.TemporaryDirectory(prefix='horizon-native-import-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', '--test', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary), 'native_imports_hold_the_engine', '--nocapture'], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn('1 passed', run.stdout)
