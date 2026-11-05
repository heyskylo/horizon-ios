"""Execute actual profile command submission/completion with delayed profile workers."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import test_profile_lookup_lifetime as lifetime
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def program():
    skins = (ROOT / 'crates/horizon-client-render/src/skins.rs').read_text()
    commands = (ROOT / 'crates/horizon-client/src/game/commands/extra.rs').read_text()
    a = skins.index('#[derive(Clone, Debug)]\npub struct ResolvedProfile')
    b = skins.index('impl ResolvedProfile', a)
    profile = skins[a:b] + 'impl ResolvedProfile {pub ' + method(skins, 'textures') + '}\n'
    a = skins.index('#[derive(Clone, Debug)]\npub enum Resolve')
    b = skins.index('/// `StringUtil.isValidPlayerName`', a)
    definitions = skins[a:b]
    resolver = skins[skins.index('impl Resolver {'):]
    methods = '\n'.join('pub ' + method(resolver, n) for n in ('new', 'sweep', 'poll', 'lookup', 'state'))
    a = commands.index('pub(in crate::game) struct ProfileFetch {')
    b = commands.index('\n}', a) + 2
    fetch = commands[a:b].replace('pub(in crate::game) ', '').replace('super::execute::Source', 'u32')
    methods_game = '\n'.join(method(commands, n) for n in ('cmd_fetchprofile', 'profile_fetches_pending', 'tick_profile_fetches'))
    return lifetime.CLOCK.replace("{collections,sync}", "{collections,mem,sync}") + lifetime.SKIN_ENDPOINTS + profile + definitions + method(skins, 'valid_player_name') + '\nimpl Resolver {' + methods + '}\n' + lifetime.CACHE_ENDPOINTS + '\n}\n' + STUBS + fetch + '\nimpl Game {' + methods_game + '}\n' + CASES


STUBS = r'''
use skins::{NameOrId,ProfileLookup,Resolve,ResolvedProfile};
use text::Text;
type R<T>=Result<T,String>;
#[derive(Clone)]enum Arg{Str(String)}
struct Context{literals:Vec<String>,arg:Arg}
impl Context{fn get(&self,_:&str)->Option<&Arg>{Some(&self.arg)}}
#[derive(PartialEq)]enum Target{Player,Other}
pub mod command{pub mod selector{pub fn java_uuid(s:&str)->Option<u128>{u128::from_str_radix(s,16).ok()}}}
pub mod crypto{pub fn uuid_string(id:u128)->String{format!("{id:032x}")}}
pub mod lan{pub mod playerdata{pub fn ints_uuid(_:u128)->u128{1}}}
mod text{
 #[derive(Clone)]pub struct Text(pub String);
 impl Text{pub fn literal(v:impl Into<String>)->Self{Self(v.into())}pub fn translate(key:&str,args:&[Self])->Self{Self(format!("{key}:{}",args.iter().map(|a|a.0.as_str()).collect::<Vec<_>>().join(",")))}}
}
fn tr_error(key:&str,_:&[Text])->String{key.into()}
#[derive(Default)]struct Cmds{profile_fetches:Vec<ProfileFetch>}
struct Game{skin_profiles:RefCell<skins::Resolver>,cmds:Cmds,command_source:Option<u32>,reports:Vec<(Option<u32>,String,bool)>}
impl Game{
 fn new()->Self{Self{skin_profiles:RefCell::new(skins::Resolver::new()),cmds:Cmds::default(),command_source:None,reports:Vec::new()}}
 fn one_entity(&self,_:&Context,_:&str,_:bool)->R<Target>{Ok(Target::Player)}
 fn target_name(&self,_:Target)->Text{Text::literal("Target")}
 fn acting_uuid(&self)->u128{1}fn acting_player_name(&self)->&str{"Local"}
 fn report_resolved_profile(&mut self,_:&str,arg:Text,_:&ResolvedProfile){self.reports.push((self.command_source,arg.0,true));}
 fn command_failure(&mut self,text:Text){self.reports.push((self.command_source,text.0,false));}
}
fn submit(g:&mut Game,id:u32,name:&str){g.command_source=Some(id);g.cmd_fetchprofile(&Context{literals:vec!["name".into()],arg:Arg::Str(name.into())}).unwrap();}
'''

CASES = r'''
fn main(){
 reset();let mut g=Game::new();
 for i in 0..500{submit(&mut g,i,&format!("NPC{i}"));}
 submit(&mut g,500,"VisibleSlim");submit(&mut g,501,"Missing");
 g.command_source=Some(9000);g.tick_profile_fetches();assert!(g.profile_fetches_pending());
 NOW.fetch_add(31_000_000,Ordering::SeqCst);skins::force_sweep(&mut g.skin_profiles.borrow_mut());
 // Reading readiness or ticking must not replace a command's original future.
 assert!(g.profile_fetches_pending());g.tick_profile_fetches();assert_eq!(g.command_source,Some(9000));
 NOW.fetch_add(15_000_000,Ordering::SeqCst);pump();
 assert!(!g.profile_fetches_pending(),"commands stayed pending after their original workers completed");
 g.tick_profile_fetches();assert!(g.cmds.profile_fetches.is_empty());
 assert_eq!(g.reports.len(),502);assert_eq!(LOADS.load(Ordering::SeqCst),502,"cache eviction caused duplicate command fetches");
 for i in 0..501{let r=&g.reports[i];assert_eq!(r.0,Some(i as u32));assert!(r.2);}
 assert_eq!(g.reports[500].1,"VisibleSlim");assert!(!g.reports[501].2);assert_eq!(g.reports[501].0,Some(501));
 assert_eq!(g.command_source,Some(9000));g.tick_profile_fetches();assert_eq!(g.reports.len(),502,"completion reported twice");
 // UUID submission captures a future too; immediate entity reporting is unchanged.
 reset();let mut g=Game::new();g.command_source=Some(42);
 g.cmd_fetchprofile(&Context{literals:vec!["id".into()],arg:Arg::Str("1234".into())}).unwrap();
 assert!(g.profile_fetches_pending());g.command_source=Some(9000);run_last();g.tick_profile_fetches();
 assert_eq!(g.reports[0],(Some(42),"00000000000000000000000000001234".into(),true));assert_eq!(g.command_source,Some(9000));
 g.cmd_fetchprofile(&Context{literals:vec!["entity".into()],arg:Arg::Str("Local".into())}).unwrap();
 assert_eq!(g.reports[1],(Some(9000),"Local".into(),true));assert_eq!(LOADS.load(Ordering::SeqCst),1);
 submit(&mut g,43,"invalid name");assert!(!g.profile_fetches_pending());g.command_source=Some(9000);g.tick_profile_fetches();
 assert_eq!(g.reports.len(),3);assert_eq!(g.reports[2].0,Some(43));assert!(!g.reports[2].2);assert_eq!(LOADS.load(Ordering::SeqCst),1);
}
'''


class CommandProfileLifetimeTests(unittest.TestCase):
    def test_each_command_keeps_its_result_and_reply_source_under_pressure(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-command-profile-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
