"""The detached prepared-file cache writer retains its generation's home lease.

Uses the production writer admission/shutdown code with a controlled slow
cache-write endpoint and the real home access API, without Minecraft files.
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
    source = Path(os.environ.get('HORIZON_PREPARED_WRITER_SOURCE', ROOT / 'crates/horizon-client/src/game/world_loading.rs')).read_text()
    imports = (ROOT / 'crates/horizon-client/src/import.rs').read_text()
    a = imports.rfind('#[derive', 0, imports.index('struct HomeGate {'))
    b = imports.rfind('#[derive', a, imports.index('pub enum Kind'))
    access = imports[a:b]
    a = source.index('static PREPARED_WRITERS:')
    b = source.index(';\n', a) + 2
    state = source[a:b]
    write = method(source, 'write_prepared_files')
    shutdown = method(source, 'shutdown_prepared_files')
    owned = 'home_access:' in write
    calls = 'write_prepared_files(root.clone(),Some(access));' if owned else 'let _access=access;write_prepared_files(root.clone());'
    cached = 'write_prepared_files(root.clone(),Some(access));' if owned else 'let _access=access;write_prepared_files(root.clone());'
    return STUBS.replace('HOME_ACCESS_API', access) + state + write + shutdown + CASES.replace('FIRST_WRITE_CALL', calls).replace('CACHED_WRITE_CALL', cached)


STUBS = r'''
#![allow(dead_code)]
extern crate self as horizon_client;
extern crate self as horizon_core;
extern crate self as horizon_threads;
extern crate self as log;
#[macro_export]macro_rules! info{($($t:tt)*)=>{{}}}
use std::{collections::BTreeMap,fs,path::{Path,PathBuf},sync::{Arc,Mutex,OnceLock,PoisonError,Weak,Condvar,mpsc,atomic::{AtomicBool,Ordering}},time::{Instant,Duration}};
type Result<T>=std::result::Result<T,Box<dyn std::error::Error+Send+Sync>>;
mod import{use super::*;HOME_ACCESS_API
 fn fixture_boundary(){}
}
pub fn idle_current(){}
static STARTED:AtomicBool=AtomicBool::new(false);
static READS:Mutex<Option<(String,String)>>=Mutex::new(None);
fn release()->&'static(Mutex<bool>,Condvar){static STATE:OnceLock<(Mutex<bool>,Condvar)>=OnceLock::new();STATE.get_or_init(||(Mutex::new(false),Condvar::new()))}
mod prepared_files{
 use super::*;
 pub const FILE_NAME:&str="fixture-cache";
 pub fn open(root:&Path)->Option<()>{root.join(FILE_NAME).exists().then_some(())}
 pub fn invalidate(_:&Path){}
 pub fn write(root:&Path)->std::io::Result<()>{
  let first=fs::read_to_string(root.join("generation"))?;
  STARTED.store(true,Ordering::SeqCst);
  let(lock,signal)=release();let mut go=lock.lock().unwrap();while !*go{go=signal.wait(go).unwrap();}drop(go);
  let second=fs::read_to_string(root.join("generation"))?;
  *READS.lock().unwrap()=Some((first,second));fs::write(root.join(FILE_NAME),b"prepared cache")
 }
}
'''

CASES = r'''
fn main(){
 let home=PathBuf::from(std::env::var_os("HORIZON_PREPARED_HOME_FIXTURE").unwrap());let root=home.join("resources");fs::create_dir_all(&root).unwrap();fs::write(root.join("horizon-resources.json"),b"{}").unwrap();fs::write(root.join("generation"),"before").unwrap();
 let producer_home=home.clone();let producer_root=root.clone();
 std::thread::spawn(move||{let root=producer_root;let access=import::acquire_home_access(&producer_home).unwrap();FIRST_WRITE_CALL}).join().unwrap();
 let deadline=Instant::now()+Duration::from_secs(3);while !STARTED.load(Ordering::SeqCst){assert!(Instant::now()<deadline);std::thread::yield_now();}
 assert!(!shutdown_prepared_files(),"The producer must not wait for the finite cache writer on the caller");
 let(tx,rx)=mpsc::channel();let(try_tx,try_rx)=mpsc::channel();let writer_home=home.clone();let writer_root=root.clone();
 let writer=std::thread::spawn(move||{try_tx.send(()).unwrap();import::with_home_access(&writer_home,||{fs::write(writer_root.join("generation"),"after")?;Ok(())}).unwrap();tx.send(()).unwrap();});
 try_rx.recv().unwrap();let escaped=rx.recv_timeout(Duration::from_millis(150)).is_ok();
 *release().0.lock().unwrap()=true;release().1.notify_all();
 let deadline=Instant::now()+Duration::from_secs(3);while !shutdown_prepared_files(){assert!(Instant::now()<deadline);std::thread::yield_now();}
 writer.join().unwrap();let reads=READS.lock().unwrap().clone().unwrap();
 eprintln!("cache read pair={reads:?}; writer_escaped={escaped}");
 assert!(!escaped,"The detached prepared-file writer lost its source generation lease");
 assert_eq!(reads,("before".into(),"before".into()),"The packed cache mixed resource generations");
 // The cached-file admission path must release its unused token immediately.
 let access=import::acquire_home_access(&home).unwrap();CACHED_WRITE_CALL
 let(check,checked)=mpsc::channel();std::thread::spawn(move||{let _access=import::acquire_home_access(&home).unwrap();check.send(()).unwrap();});
 checked.recv_timeout(Duration::from_secs(3)).unwrap();assert!(shutdown_prepared_files());
}
'''


class PreparedWriterHomeLifetimeTests(unittest.TestCase):
    def test_detached_cache_writer_and_cached_skip_release_finite_ownership(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required for the production cache-writer probe')
        with tempfile.TemporaryDirectory(prefix='horizon-cache-home-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            built = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            env = dict(os.environ, HORIZON_PREPARED_HOME_FIXTURE=str(Path(folder) / 'home'))
            ran = subprocess.run([str(binary)], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)
