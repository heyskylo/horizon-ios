"""Exercise production skin readiness, upload ordering and per-frame residency.

Atlas upload and disk/HTTP are endpoints. The production model-readiness,
visible-use, Downloader queue/draw scopes and fixed-budget SkinSlots run here.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = (ROOT / 'crates/horizon-client-render/src/skins.rs').read_text()
    remote = (ROOT / 'crates/horizon-client/src/game/remote.rs').read_text()
    a = source.index('pub struct Downloader {')
    b = source.index('/// `images` is bounded only', a)
    downloader = source[a:b]
    methods = '\n'.join(method(source, n) for n in (
        'new', 'image', 'remember_image', 'remember', 'queue', 'again', 'set_resource_root',
        'prioritize', 'take_ready', 'drawn', 'begin_draw', 'finish_draw', 'take_drawn'))
    # Resolver::new precedes Downloader::new; take this constructor from its
    # own impl instead of silently pulling the unrelated profile cache in.
    methods = methods.replace(method(source, 'new'), method(source[source.index('impl Downloader {'):], 'new'), 1)
    a = source.index('pub struct SkinSlots {')
    b = source.index('/// Decode before trusting', a)
    slots = source[a:b]
    game = '\n'.join(method(remote, n) for n in (
        'skin_available', 'ready_skin', 'request_skin', 'mark_skin_used',
        'again_skin', 'drawn_skin', 'face_skin'))
    return STUBS + downloader + '\nimpl Downloader {\n' + methods + '\n}\n' + slots + '\nimpl Game {\n' + game + '\n}\n' + CASES


STUBS = r'''
#![allow(dead_code)]
extern crate self as horizon_client;
mod tab_list{pub fn default_skin(_:u128)->&'static str{"entity/player/wide/steve"}}
use std::{cell::RefCell,collections::HashMap,path::PathBuf,sync::{Arc,atomic::{AtomicBool,Ordering}}};
// Atlas residency has no IO jobs; the real owned service/pressure lifecycle
// is exercised independently by skins/workers and blocked-HTTP tests.
mod workers{use super::*;pub struct Pool;impl Pool{
 pub fn new(_:&str,_:usize)->Arc<Self>{Arc::new(Self)}
 pub fn cancel_scope(&self,scope:&Arc<AtomicBool>){scope.store(true,Ordering::Release);}
 pub fn request_shutdown(&self){}
}}
#[derive(Clone,Copy,Hash,PartialEq,Eq)]enum Kind{Skin,Cape,Elytra}
#[derive(Clone,Copy)]enum Download{Ready,Pending,Failed}
const MAX_SKIN_IMAGES:usize=1024;
fn cached_texture(_: &std::path::Path,_:Kind,_:impl FnOnce()->Option<Vec<u8>>)->Option<Vec<u8>>{panic!("fixture images are already in memory; disk/HTTP must not run")}
struct Atlas{sprites:HashMap<String,usize>}struct Gui{atlas:Atlas}
struct Game{skin_slots:SkinSlots,skin_downloads:RefCell<Downloader>,gui:Gui}
impl Game{
 fn new()->Self{Self{skin_slots:SkinSlots::new(128),skin_downloads:RefCell::new(Downloader::new(PathBuf::from("unused-scratch-skin-cache"))),gui:Gui{atlas:Atlas{sprites:HashMap::new()}}}}
 fn remember(&self,name:&str){self.skin_downloads.borrow_mut().remember_image(name,&[1,2,3,4]);}
 // The atlas/GPU upload endpoint: use the real bounded slot assignment,
 // recording the exact sprite-to-slot map the renderer would receive.
 fn upload(&mut self){for(name,_)in self.skin_downloads.borrow_mut().take_ready(){if let Some((slot,evicted))=self.skin_slots.place(name.clone()){if let Some(old)=evicted{self.gui.atlas.sprites.remove(&old);}self.gui.atlas.sprites.insert(name,slot);}}}
 fn next_frame(&mut self){let drawn=self.skin_downloads.borrow_mut().take_drawn();self.skin_slots.next_frame(drawn);self.upload();}
}
'''

CASES = r'''
fn main(){
 let mut g=Game::new();
 let residents:Vec<_>=(0..128).map(|i|format!("skins/offscreen-{i}")).collect();
 for name in &residents{g.remember(name);let(slot,_)=g.skin_slots.place(name.clone()).unwrap();g.gui.atlas.sprites.insert(name.clone(),slot);}
 let visible="skins/new-visible-player";g.remember(visible);
 // Model updates for loaded entities do not imply a draw, even while all
 // atlas slots are occupied. Nor do unseen missing models refill the queue.
 for _ in 0..8{
  for name in &residents{assert_eq!(g.ready_skin(1,(name.clone(),false)).0,*name);}
  assert_eq!(g.ready_skin(2,(visible.into(),true)).0,"entity/player/wide/steve");
  assert!(g.skin_downloads.borrow_mut().take_drawn().is_empty());
  assert!(g.skin_downloads.borrow_mut().take_ready().is_empty());
 }
 // Unrelated downloads completed before the actual visible request. Its
 // request must be uploaded first, without expanding the 128-slot budget.
 for n in 0..160{let name=format!("skins/completed-offscreen-{n}");g.remember(&name);g.skin_downloads.borrow_mut().again(&name);}
 g.request_skin(visible);g.next_frame();
 assert!(g.skin_available(visible));assert_eq!(g.ready_skin(2,(visible.into(),true)),(visible.into(),true));
 let cached_slot=g.gui.atlas.sprites[visible];
 // An unchanged visible player uses cached model geometry. It still submits
 // the same texture reference each frame, while others replace unused slots.
 for frame in 0..8{
  g.skin_downloads.borrow_mut().begin_draw();g.mark_skin_used(visible);g.skin_downloads.borrow_mut().finish_draw(true);
  let next=format!("skins/background-{frame}");g.remember(&next);g.skin_downloads.borrow_mut().again(&next);g.next_frame();
  assert_eq!(g.gui.atlas.sprites[visible],cached_slot);
 }
 // Offline skin pixels are remembered independently of model readiness;
 // they upload only after an actual local body/arm/preview requests them.
 let mut offline=Game::new();let local="horizon/offline_skin";offline.remember(local);
 assert_eq!(offline.ready_skin(1,(local.into(),false)).0,"entity/player/wide/steve");assert!(offline.skin_downloads.borrow_mut().take_ready().is_empty());
 offline.request_skin(local);offline.next_frame();assert_eq!(offline.ready_skin(1,(local.into(),false)).0,local);offline.mark_skin_used(local);
 offline.next_frame();assert!(offline.skin_available(local));
 // A culled body/cape/head-item transaction cannot pin unused skin slots.
 let culled="capes/culled";g.remember(culled);g.skin_downloads.borrow_mut().again(culled);g.next_frame();assert!(g.skin_available(culled));
 g.mark_skin_used(visible);
 g.skin_downloads.borrow_mut().begin_draw();g.mark_skin_used(culled);g.mark_skin_used(visible);g.skin_downloads.borrow_mut().finish_draw(false);
 let drawn=g.skin_downloads.borrow_mut().take_drawn();assert!(drawn.contains(visible));assert!(!drawn.contains(culled));
 // Faces and special head draw calls still pin actual atlas uses, including
 // while the world body is not drawn. Duplicate marks are harmless.
 assert_eq!(g.face_skin(2,visible),visible);g.mark_skin_used(visible);
 let drawn=g.skin_downloads.borrow_mut().take_drawn();assert_eq!(drawn.len(),1);assert!(drawn.contains(visible));
 // A missing face queues its real wanted image and draws the default until
 // uploaded; querying readiness alone did not do that work above.
 let face="skins/new-face";g.remember(face);assert_eq!(g.face_skin(3,face),"entity/player/wide/steve");g.next_frame();assert_eq!(g.face_skin(3,face),face);
 // Changing resource packs invalidates queued patch pixels and references,
 // including an entity still being built, without touching URL/offline skins.
 let mut d=Downloader::new(PathBuf::from("scratch"));let patch="patch/server/entity/npc";let scoped="patch/server/entity/other";
 d.resources.insert(patch.into(),Download::Ready);d.resources.insert(scoped.into(),Download::Ready);
 d.queue(patch.into(),vec![255,0,0,255]);d.queue(scoped.into(),vec![255,0,0,255]);
 d.queue("skins/url".into(),vec![1,2,3,4]);d.queue("horizon/offline_skin".into(),vec![4,3,2,1]);
 d.drawn(patch);d.drawn("skins/url");d.drawn("horizon/offline_skin");d.begin_draw();d.drawn(scoped);
 d.set_resource_root(PathBuf::from("new-pack"));d.finish_draw(true);
 assert!(d.image(patch).is_none());assert!(d.image(scoped).is_none());
 let refs=d.take_drawn();assert!(!refs.contains(patch)&&!refs.contains(scoped));assert!(refs.contains("skins/url")&&refs.contains("horizon/offline_skin"));
 d.resources.insert(patch.into(),Download::Ready);d.queue(patch.into(),vec![0,0,255,255]);
 let ready=d.take_ready();let patches:Vec<_>=ready.iter().filter(|(name,_)|name==patch).collect();
 assert_eq!(patches.len(),1,"old red pixels survived the resource generation");assert_eq!(patches[0].1,vec![0,0,255,255]);assert!(!ready.iter().any(|(name,_)|name==scoped));
 assert!(ready.contains(&("skins/url".into(),vec![1,2,3,4])));assert!(ready.contains(&("horizon/offline_skin".into(),vec![4,3,2,1])));

}
'''


class SkinAtlasResidencyTests(unittest.TestCase):
    def test_visible_references_own_fixed_budget_atlas_slots(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-skin-residency-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
