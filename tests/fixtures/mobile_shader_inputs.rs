fn verify(p: &Program) {
 for module in [&p.vertex,&p.fragment] {
  let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(module).unwrap();
  naga::back::msl::write_string(module,&info,&naga::back::msl::Options{lang_version:(2,4),fake_missing_bindings:true,..Default::default()},&Default::default()).unwrap();
 }
}
fn main(){
 let scratch=std::env::temp_dir().join(format!("shader-input-fixture-{}",std::process::id()));std::fs::create_dir_all(scratch.join("shaders")).unwrap();
 std::fs::write(scratch.join("shaders/terrain.vsh"),"#version 130\nattribute vec4 mc_Entity; attribute vec4 mc_midTexCoord; attribute vec4 at_tangent; varying vec4 colour; void main(){ gl_Position=ftransform(); colour=vec4(mc_midTexCoord.xy,at_tangent.x,mc_Entity.x); }").unwrap();
 std::fs::write(scratch.join("shaders/terrain.fsh"),"#version 130\nvarying vec4 colour; void main(){ gl_FragData[0]=colour; }").unwrap();
 let pack=Pack::open(&scratch).unwrap();let mut macros=Macros::standard();macros.define_renderer(4);
 let v=preprocess(&pack,"terrain.vsh",&macros);let f=preprocess(&pack,"terrain.fsh",&macros);
 let atlas=PackAtlas{columns:16,sprite:16,separate_ao:true};
 for flat in [false,true] {
  let inputs=atlas.terrain_inputs(flat);
  for shadow in [false,true] {let p=if shadow{translate_shadow(&v,&f,&inputs)}else{translate_gbuffers(&v,&f,Target::Buffers,&inputs)};verify(&p.unwrap_or_else(|e|panic!("flat={flat} shadow={shadow}: {e:?}")));}
 }
 println!("production terrain/shadow input variants: pass (Naga + Metal output)");
 if let Some(path)=std::env::args().nth(1){
  let pack=Pack::open(std::path::Path::new(&path)).unwrap();let _=read_properties(&pack,&mut macros);
  for dimension in ["world0", "world-1", "world1"] {
  for name in ["gbuffers_terrain_solid","gbuffers_terrain","gbuffers_textured","gbuffers_basic","gbuffers_water","shadow","shadow_water","gbuffers_line"] {
   if pack.read_text(&format!("{dimension}/{name}.vsh")).is_none() {continue;}
   let v=preprocess(&pack,&format!("{dimension}/{name}.vsh"),&macros);let f=preprocess(&pack,&format!("{dimension}/{name}.fsh"),&macros);
   let input=if name=="gbuffers_line"{line_inputs()}else{atlas.terrain_inputs(true)};
   let p=if name.starts_with("shadow"){translate_shadow(&v,&f,&input)}else{translate_gbuffers(&v,&f,Target::Buffers,&input)};
   match p{Ok(p)=>{verify(&p);println!("{dimension}/{name}: pass (Naga + Metal output)");},Err(e)=>panic!("{dimension}/{name}: {e:?}")}
  }
  }
 }
 std::fs::remove_dir_all(scratch).unwrap();
}
