"""Skin preparation must advance independently of optional HUD widgets/hooks.

Run the actual HUD dispatch/poll scheduling with controlled completed texture
and model DTOs. Rendering and network futures are deterministic endpoints.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method

ROOT = Path(__file__).resolve().parents[1]


def body_prefix(source, name, before):
    code = method(source, name)
    return code[code.index('{') + 1:code.index(before)]


def program():
    game = (ROOT / 'crates/horizon-client/src/game.rs').read_text()
    score = (ROOT / 'crates/horizon-client/src/game/scoreboard.rs').read_text()
    remote = (ROOT / 'crates/horizon-client/src/game/remote.rs').read_text()
    a = game.index('        // The chat screen draws the focused chat itself;')
    b = game.index('        // Screen.extractBlurredBackground', a)
    dispatch = game[a:b]
    advance = method(game, 'advance_skin_frame') if 'fn advance_skin_frame(' in game else ''
    return STUBS + '\nimpl Game {\n' + advance + '\nfn hud(&mut self,size:[u32;2])->HudBatch{' + body_prefix(game, 'hud', '        let mut batch =') + 'self.hud_layers(size)}\nfn hud_layers(&mut self,_:[u32;2])->HudBatch{let show_hud=!self.hide_gui;let mut batch=HudBatch::default();' + dispatch + 'batch}\nfn draw_tab_list(&mut self,_:&mut HudBatch){' + body_prefix(score, 'draw_tab_list', '        let key =') + 'self.remote_tab_players();self.tab_draws+=1;}\nfn remote_tab_players(&mut self){' + body_prefix(remote, 'remote_tab_players', '        let mut players:') + '}\n}\n' + CASES


STUBS = r'''
#![allow(dead_code,unused_imports)]
extern crate self as horizon_client;mod tab_list{pub struct TabInputs;}
#[derive(Clone,Copy,PartialEq)]enum Screen{Playing,Chat}
#[derive(Default)] struct HudBatch{width:f32,height:f32}
impl HudBatch{fn new(size:[u32;2])->Self{Self{width:size[0]as f32,height:size[1]as f32}}}
mod hud_kit{pub mod room{pub struct Room;impl Room{pub fn new(_:f32,_:f32)->Self{Self}}}}
struct Ui{room:hud_kit::room::Room}
#[derive(Clone,Debug,PartialEq)]struct SkinDto{texture:&'static str,slim:bool}
struct Game{ui:Ui,title:Option<()>,screen:Screen,hide_gui:bool,cancel_tab:bool,pending:Option<SkinDto>,available:bool,model:SkinDto,flushes:usize,polls:usize,tab_draws:usize}
impl Game{
 fn new(hide_gui:bool,cancel_tab:bool)->Self{Self{ui:Ui{room:hud_kit::room::Room},title:None,screen:Screen::Playing,hide_gui,cancel_tab,pending:Some(SkinDto{texture:"skins/custom-NPC",slim:true}),available:false,model:SkinDto{texture:"entity/player/wide/steve",slim:false},flushes:0,polls:0,tab_draws:0}}
 fn flush_skin_slots(&mut self){self.flushes+=1;self.available=self.pending.is_some();}
 fn update_remote_skins(&mut self){self.polls+=1;if self.available{if let Some(ready)=self.pending.take(){self.model=ready;}}}
 fn gui_layer(&mut self,name:&str,b:&mut HudBatch,draw:impl FnOnce(&mut Self,&mut HudBatch)){if name!="minecraft:tab_list"||!self.cancel_tab{draw(self,b);}}
 fn publish_tab_object_skins(&mut self){}
 fn draw_chat(&mut self,_:&mut HudBatch){}
}
'''

CASES = r'''
fn main(){
 for hide_gui in [false,true]{for cancel_tab in [false,true]{
  let mut g=Game::new(hide_gui,cancel_tab);g.hud([1024,768]);
  assert_eq!(g.model,SkinDto{texture:"skins/custom-NPC",slim:true},"HUD hidden={hide_gui}, tab hook cancelled={cancel_tab} stopped a completed NPC skin");
  assert_eq!((g.flushes,g.polls),(1,1),"one skin-preparation pass per frame");
 }}
 // The chat screen takes the direct HUD branch; preparation still runs once.
 let mut g=Game::new(false,true);g.screen=Screen::Chat;g.hud([1024,768]);assert_eq!(g.model.texture,"skins/custom-NPC");assert_eq!((g.flushes,g.polls),(1,1));
}
'''


class SkinProgressVisibilityTests(unittest.TestCase):
    def test_hud_visibility_and_native_tab_hook_do_not_stop_skin_progress(self):
        compiler = shutil.which('rustc')
        if compiler is None:
            self.skipTest('rustc is required')
        with tempfile.TemporaryDirectory(prefix='horizon-skin-progress-') as folder:
            source = Path(folder) / 'probe.rs'
            binary = Path(folder) / 'probe'
            source.write_text(program())
            build = subprocess.run([compiler, '--edition=2024', str(source), '-o', str(binary)], capture_output=True, text=True, timeout=60)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
