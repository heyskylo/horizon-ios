"""Run the unified production reload coordinator with controlled worker results.

Only the worker and GPU/application callbacks are substituted. The method under
examination is extracted from game/packs.rs, so loader waits, supersession and
failure routing exercise the code used by both local and server pack reloads.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("rustc"), "requires Rust")
class LocalPackReloadTests(unittest.TestCase):
    def test_unified_reload_keeps_ui_responsive_and_routes_latest_results(self):
        source = (ROOT / "crates/horizon-client/src/game/packs.rs").read_text()
        def production_method(signature):
            start = source.index(signature)
            opening = source.index("{", start)
            end, depth = opening + 1, 1
            while depth:
                depth += (source[end] == "{") - (source[end] == "}")
                end += 1
            return source[start:end].replace("pub(super)", "")

        method = "\n".join(production_method(signature) for signature in (
            "    fn pack_context_loading(",
            "    pub(super) fn drive_pack_reload(",
        ))
        fixture = r'''
extern crate self as log;
#[macro_export] macro_rules! warn { ($($arg:tt)*) => {} }
#[derive(Clone,Copy,PartialEq,Eq)] enum ReloadKind {Selection,Server}
struct PackReload {
 kind: ReloadKind,
 worker: Option<std::thread::JoinHandle<Result<u32,String>>>,
}
impl PackReload {
 fn finished(&mut self)->Option<(ReloadKind,Result<u32,String>)> {
  if !self.worker.as_ref()?.is_finished() { return None; }
  Some((self.kind,self.worker.take()?.join().unwrap()))
 }
}
#[derive(Default)] struct Game {
 pack_reload: Option<PackReload>, pending_pack_reload: bool,
 rest_loader: Option<()>, pending_entry: Option<()>, integrated_starting: Option<()>,
 applied: Vec<u32>, selection_errors: Vec<String>, server_results: Vec<bool>,
}
impl Game {
 fn apply_staged_pack(&mut self,value:u32)->Result<(),String> {
  if value==0 {return Err("GPU application failed".into());}
  self.applied.push(value); Ok(())
 }
 fn selection_reload_failed(&mut self,error:&str) {self.selection_errors.push(error.into());}
 fn finish_pack_reload(&mut self,ok:bool) {self.server_results.push(ok);}
 METHOD
 fn complete(&mut self,kind:ReloadKind,result:Result<u32,String>) {
  self.pack_reload=Some(PackReload {kind,worker:Some(std::thread::spawn(move || result))});
  while !self.pack_reload.as_ref().unwrap().worker.as_ref().unwrap().is_finished() {
   std::thread::yield_now();
  }
 }
}
fn main() {
 let mut g=Game::default();
 let (send,recv)=std::sync::mpsc::channel();
 g.pack_reload=Some(PackReload {kind:ReloadKind::Selection,
  worker:Some(std::thread::spawn(move || {recv.recv().unwrap();Ok(1)}))});
 g.drive_pack_reload(); // Must return while the worker remains blocked.
 assert!(g.applied.is_empty()); assert!(g.pack_reload.is_some());
 g.pending_pack_reload=true;
 send.send(()).unwrap();
 while !g.pack_reload.as_ref().unwrap().worker.as_ref().unwrap().is_finished() {
  std::thread::yield_now();
 }
 g.drive_pack_reload();
 assert!(g.applied.is_empty()); assert!(g.pack_reload.is_none());
 assert!(g.pending_pack_reload,"new selection must remain queued");
 // The next generation applies, while failures from superseded work do not
 // reset the user's newer selection.
 g.complete(ReloadKind::Selection,Err("obsolete failure".into()));
 g.drive_pack_reload(); assert!(g.selection_errors.is_empty());
 g.pending_pack_reload=false;
 g.complete(ReloadKind::Selection,Ok(2)); g.drive_pack_reload();
 assert_eq!(g.applied,vec![2]);
 // Every individual loading state and their combinations must retain a
 // completed result, for both local selections and server resource packs.
 for loading in 1..8 {
  for kind in [ReloadKind::Selection,ReloadKind::Server] {
   g.complete(kind,Ok(3));
   g.rest_loader=(loading&1 != 0).then_some(());
   g.pending_entry=(loading&2 != 0).then_some(());
   g.integrated_starting=(loading&4 != 0).then_some(());
   let before=g.applied.len(); let callbacks=g.server_results.len();
   g.drive_pack_reload();
   assert!(g.pack_reload.is_some()); assert_eq!(g.applied.len(),before);
   assert_eq!(g.server_results.len(),callbacks,"no server callback before application");
   g.rest_loader=None; g.pending_entry=None; g.integrated_starting=None;
   g.drive_pack_reload();
   assert!(g.pack_reload.is_none()); assert_eq!(g.applied.len(),before+1);
  }
 }
 g.complete(ReloadKind::Selection,Err("invalid pack".into())); g.drive_pack_reload();
 g.complete(ReloadKind::Selection,Ok(0)); g.drive_pack_reload();
 assert_eq!(g.selection_errors,vec!["invalid pack","GPU application failed"]);
 // Server completions are never discarded on behalf of a queued local
 // selection: its state machine needs both success and failure callbacks.
 g.pending_pack_reload=true;
 g.complete(ReloadKind::Server,Ok(4)); g.drive_pack_reload();
 g.complete(ReloadKind::Server,Err("server pack failed".into())); g.drive_pack_reload();
 let mut expected_results=vec![true;8]; expected_results.push(false);
 assert_eq!(g.server_results,expected_results);
 let mut expected_applied=vec![2]; expected_applied.extend([3;14]); expected_applied.push(4);
 assert_eq!(g.applied,expected_applied); assert!(g.pending_pack_reload);
}
'''.replace("METHOD", method)
        with tempfile.TemporaryDirectory(prefix="horizon-pack-coordinator-") as directory:
            path = Path(directory)
            (path / "test.rs").write_text(fixture)
            build = subprocess.run(
                ["rustc", "--edition=2024", str(path / "test.rs"), "-o", str(path / "test")],
                capture_output=True, text=True, timeout=60,
            )
            self.assertEqual(build.returncode, 0, build.stderr)
            subprocess.run([str(path / "test")], check=True, timeout=5)
