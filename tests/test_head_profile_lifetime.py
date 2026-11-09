"""Production head rendering must retain delayed resolutions under shared cache pressure."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import test_profile_lookup_lifetime as lifetime
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    source = (ROOT / 'crates/horizon-client-render/src/skins.rs').read_text()
    resolver = source[source.index('impl Resolver {'):]
    extra = ''
    if 'fn render_state(' in resolver:
        extra += 'impl Resolver {pub ' + method(resolver, 'render_state') + '}\n'
        extra += 'pub fn render_cache_len(r:&Resolver)->usize{r.render_entries.len()}\n'
    else:
        extra += 'pub fn render_cache_len(_: &Resolver)->usize{0}\n'
    extra += STUBS + '\npub ' + method(source, 'head_skin') + '\n'
    # Run the real value/resolved-texture cache methods. Only the profile/texture
    # codec is an endpoint, as in the shared mannequin probe; its real URL and
    # signature checks remain covered by skins.rs's in-crate tests.
    cache = source[source.index('impl HeadProfiles {'):]
    head_start = source.index('#[derive(Debug, PartialEq)]\nstruct HeadProfile')
    head_end = source.index('\nfn prepare_head(', head_start)
    typed_cache = HEAD_CACHE + source[head_start:head_end]
    typed_cache += method(source, 'prepare_head') + method(source, 'prepare_head_info')
    typed_cache += 'impl HeadProfiles {' + method(cache, 'get') + method(cache, 'get_resolved') + '}\n'
    return lifetime.program().replace('#[derive(Default)]struct HeadProfiles;', typed_cache).replace(lifetime.CACHE_ENDPOINTS, lifetime.CACHE_ENDPOINTS + extra).replace(lifetime.CASES, CASES).replace('#[derive(Clone,Copy,PartialEq)]pub enum Signature', '#[derive(Clone,Copy,Debug,PartialEq)]pub enum Signature')


HEAD_CACHE = r'''
const MAX_HEAD_PROFILES:usize=4096;
#[derive(Default)]struct HeadProfiles {
 entries:std::collections::HashMap<crate::serde_json::Value, std::sync::Arc<HeadProfile>>,
 resolved:[std::collections::HashMap<String,std::sync::Arc<HeadProfile>>;2],
}
'''


STUBS = r'''
use crate::serde_json;
#[derive(Debug,Default,PartialEq)]pub struct ProfileInfo {pub textures:Option<(String,Signature)>,pub dynamic:Option<NameOrId>,pub id:u128}
fn parse_profile(p:&serde_json::Value)->ProfileInfo{ProfileInfo{textures:None,dynamic:Some(NameOrId::Name(p.0.clone())),id:0}}
pub struct Downloader;
#[derive(PartialEq)]enum Download{Ready}
enum Kind{Skin}
impl Downloader{fn state(&mut self,_:Kind,_:&str)->Download{Download::Ready}fn drawn(&mut self,_:&str){}fn again(&mut self,_:&str){}}
fn patch_skin(_: &str,_:&mut Downloader,_:impl Fn(&str)->bool)->Option<String>{None}
fn texture_hash(url:&str)->Option<String>{url.strip_prefix("skins/").map(str::to_owned)}
fn skin_location(hash:&str)->String{format!("skins/{hash}")}
'''

CASES = r'''
mod serde_json{#[derive(Clone,Hash,PartialEq,Eq)]pub struct Value(pub String);impl Value{pub fn get(&self,_:&str)->Option<&Self>{None}pub fn as_str(&self)->Option<&str>{Some(&self.0)}}}
mod tab_list{pub fn default_skin(_:u128)->&'static str{"fallback"}}
fn draw(resolver:&mut skins::Resolver,name:&str)->String{skins::head_skin(&serde_json::Value(name.into()),resolver,&mut skins::Downloader,|_|true)}
fn main(){
 reset();let mut resolver=skins::Resolver::new();
 // Five hundred off-screen jobs compete with the visible head. Each new
 // scene pass occurs before the original 45s network jobs have answered.
 for i in 0..500{assert_eq!(draw(&mut resolver,&format!("NPC{i}")),"fallback");}
 assert_eq!(draw(&mut resolver,"VisibleSlim"),"fallback");
 NOW.fetch_add(31_000_000,Ordering::SeqCst);skins::force_sweep(&mut resolver);
 for i in 0..500{draw(&mut resolver,&format!("NPC{i}"));}
 draw(&mut resolver,"VisibleSlim");
 NOW.fetch_add(15_000_000,Ordering::SeqCst);pump();
 for i in 0..500{assert_eq!(draw(&mut resolver,&format!("NPC{i}")),format!("skins/NPC{i}"),"head resolution was replaced by another pending job");}
 assert_eq!(draw(&mut resolver,"VisibleSlim"),"skins/VisibleSlim");
 assert_eq!(LOADS.load(Ordering::SeqCst),501,"repeated rendering must not restart pending profile requests");
 assert!(skins::cache_len(&resolver)<=256,"shared service cache stays bounded");
 // Continuously visible heads retain their render futures across shared
 // cache expiration. Unused render entries expire after five minutes.
 for _ in 0..4{NOW.fetch_add(240_000_000,Ordering::SeqCst);resolver.poll();assert_eq!(draw(&mut resolver,"VisibleSlim"),"skins/VisibleSlim");}
 assert_eq!(LOADS.load(Ordering::SeqCst),501);
 assert_eq!(skins::render_cache_len(&resolver),1,"unused head futures must expire while the visible one stays alive");
 NOW.fetch_add(601_000_000,Ordering::SeqCst);resolver.poll();assert_eq!(draw(&mut resolver,"VisibleSlim"),"fallback");assert_eq!(LOADS.load(Ordering::SeqCst),502);
 run_last();assert_eq!(draw(&mut resolver,"VisibleSlim"),"skins/VisibleSlim");
 // An empty service answer is retained too, without repeatedly fetching.
 assert_eq!(draw(&mut resolver,"Missing"),"fallback");run_last();
 for _ in 0..20{assert_eq!(draw(&mut resolver,"Missing"),"fallback");}assert_eq!(LOADS.load(Ordering::SeqCst),503);
}
'''


class HeadProfileLifetimeTests(unittest.TestCase):
    def test_rendered_heads_survive_shared_cache_eviction(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-head-profile-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
