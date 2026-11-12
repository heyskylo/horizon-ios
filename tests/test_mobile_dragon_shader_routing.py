"""The dragon's final-buffer spans reach the real shader entity routing rules."""
from pathlib import Path
import subprocess,tempfile,unittest
from test_mobile_command_keyboard import method
ROOT=Path(__file__).resolve().parents[1]
class DragonShaderRouting(unittest.TestCase):
 def test_dragon_and_wrapped_beam_have_separate_final_buffer_spans(self):
  dragon=(ROOT/'crates/horizon-client/src/game/dragon.rs').read_text()
  pack=(ROOT/'crates/horizon-client-render/src/gfx/pack_entities.rs').read_text()
  orchestration=method(dragon,'dragon_frame_geometry')
  # Reuse the production beam-boundary operation, with geometry/animation as fixtures.
  boundary=dragon[dragon.index('self.mark_stage(out.len(), "NONE");',dragon.index('// Wrapped beam UVs')):]
  boundary=boundary[:boundary.index('\n        }')]
  source=STUB.replace('ORCHESTRATION',orchestration).replace('BEAM_BOUNDARY',boundary)
  source=source.replace('ROUTING','\n'.join(method(pack,n) for n in ['plain','program_of','runs']))
  with tempfile.TemporaryDirectory(prefix='horizon-dragon-routing-') as directory:
   p=Path(directory);(p/'main.rs').write_text(source)
   result=subprocess.run(['rustc','--edition=2024','--test',str(p/'main.rs'),'-o',str(p/'test')],capture_output=True,text=True)
   self.assertEqual(result.returncode,0,result.stderr)
   result=subprocess.run([str(p/'test')],capture_output=True,text=True)
   self.assertEqual(result.returncode,0,result.stdout+result.stderr)
STUB=r'''
#![allow(dead_code)]
use std::{cell::RefCell,ops::Range};
mod glam{#[derive(Clone,Copy)]pub struct DVec3;}
#[derive(Clone,Copy,PartialEq,Debug)]struct Vertex{layer:u32}
#[derive(Clone,Copy,Default,Debug)]struct EntitySpan{start:u32,stage:u32,id:i32,translucent:bool}
const ENTITY:usize=0;const TRANSLUCENT:usize=1;const BLOCK:usize=2;const PARTICLE:usize=3;const DESTROY:usize=4;
fn render_stage(s:&str)->u32{match s{"ENTITIES"=>1,"BLOCK_ENTITIES"=>2,"PARTICLES"=>3,"DESTROY"=>4,_=>0}}
pub struct Delta;impl std::ops::Sub for glam::DVec3{type Output=Delta;fn sub(self,_:Self)->Delta{Delta}}
struct Game{spans:RefCell<Vec<EntitySpan>>,pack:bool,dragon:bool}
impl Game{
 ORCHESTRATION
 fn mark_entity(&self,start:usize,name:&str){self.mark(start,"ENTITIES",if name=="minecraft:ender_dragon"{91}else{92});}
 fn mark_stage(&self,start:usize,stage:&str){self.mark(start,stage,0);}
 fn mark(&self,start:usize,stage:&str,id:i32){if !self.pack{return;}let s=EntitySpan{start:start as u32,stage:render_stage(stage),id,translucent:false};let mut spans=self.spans.borrow_mut();if spans.last().is_some_and(|p|p.start==s.start){*spans.last_mut().unwrap()=s;}else{spans.push(s);}}
 fn visual_ticks(&self)->u32{0}
 fn dragon_geometry(&self,camera:glam::DVec3,alpha:f64,out:&mut Vec<Vertex>,_:&mut Vec<Vertex>){
  if !self.dragon{return;}out.extend([Vertex{layer:0};4]);let pos=camera;let to=camera;let a=alpha as f32;
  BEAM_BOUNDARY
 }
 fn crystal_beam(&self,_:glam::DVec3,_:Delta,_:f32,_:glam::DVec3,out:&mut Vec<Vertex>){out.extend([Vertex{layer:6};4]);}
 fn dragon_fireball_geometry(&self,_:glam::DVec3,_:f64,out:&mut Vec<Vertex>){out.extend([Vertex{layer:0};4]);}
}
ROUTING
#[test]fn routing(){for dragon in [true,false]{
 let g=Game{spans:RefCell::new(vec![EntitySpan{start:0,stage:render_stage("PARTICLES"),..Default::default()}]),pack:true,dragon};
 let mut v=vec![Vertex{layer:0};8];let mut additive=vec![];g.dragon_frame_geometry(glam::DVec3,0.,&mut v,&mut additive);
 let spans=g.spans.borrow();let rows:Vec<_>=runs(v.len(),&spans).filter(|(_,r)|!r.is_empty()).collect();
 let fire=rows.last().unwrap();assert_eq!(fire.0.id,92);assert_eq!(program_of(&fire.0),Some(ENTITY));assert!(plain(&v[fire.1.clone()]));
 if dragon{let skin=&rows[1];assert_eq!(skin.1,8..12);assert_eq!(skin.0.id,91);assert_eq!(program_of(&skin.0),Some(ENTITY));assert!(plain(&v[skin.1.clone()]));assert_eq!(rows[2].1,12..16);assert_eq!(program_of(&rows[2].0),None);assert!(!plain(&v[8..16]));}
 assert_eq!(spans.last().unwrap().start as usize,v.len());assert_eq!(spans.last().unwrap().stage,0);
 let normal=Game{spans:RefCell::new(vec![]),pack:false,dragon};let mut unchanged=vec![Vertex{layer:0};8];normal.dragon_frame_geometry(glam::DVec3,0.,&mut unchanged,&mut vec![]);assert_eq!(v,unchanged);
}}
'''
