"""Exercise production boat seat and outgoing-packet decisions for both clients."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def method(source, name):
    at = source.index('fn ' + name + '(')
    start = source.rfind('\n', 0, at) + 1
    brace = source.index('{', at)
    end, depth = brace + 1, 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end].replace('pub(super)', 'pub(crate)')


@unittest.skipUnless(shutil.which('rustc'), 'requires Rust')
class BoatPassengerControlTests(unittest.TestCase):
    def test_second_rider_never_claims_control_or_the_front_seat(self):
        remote = (ROOT / 'crates/horizon-client/src/game/remote_vehicles.rs').read_text()
        seats = (ROOT / 'crates/horizon-client/src/game/boat_passengers.rs').read_text()
        methods = method(remote, 'remote_vehicle_packets') + method(seats, 'player_boat_seat')
        for name in ['remote_controls_boat', 'remote_passenger_seat']:
            if 'fn ' + name + '(' in remote:
                methods += method(remote, name)
        source = r'''
#![allow(dead_code)]
use std::collections::HashMap;
#[derive(Clone,Copy,Debug)] struct Vec3 { x:f64,y:f64,z:f64 }
mod play {
 #[derive(Debug)] pub struct MoveVehicleRequest {pub position:[f64;3],pub yaw:f32,pub pitch:f32,pub on_ground:bool}
 #[derive(Debug)] pub struct PaddleBoat {pub left:bool,pub right:bool}
}
#[derive(Debug)] enum S {MoveVehicle(play::MoveVehicleRequest),PaddleBoat(play::PaddleBoat)}
fn mount_move(_:()) -> S {unreachable!()}
struct Remote {entity_id:i32,first_passenger:bool,passengers:HashMap<u32,Vec<i32>>}
struct Boat {id:u32,position:Vec3,yaw:f32,on_ground:bool,paddling:[bool;2]}
impl Boat {
 // The geometry service exposes its arguments; actual Boat::seat geometry is tested in Rust.
 fn seat(&self,index:usize,count:usize,_animal:bool,_offset:f64,_previous:bool)->Vec3 {
  Vec3{x:index as f64,y:count as f64,z:0.0}
 }
}
const PLAYER_RIDING_OFFSET:f64=0.6;
struct Game {riding:Option<u32>,remote:Option<Remote>,boats:Vec<Boat>}
impl Game {
 fn driven_mount(&self)->Option<()>{None}
 fn boat_players(&self,_:u32)->usize {1}
 fn boat_mobs(&self,id:u32)->Vec<usize>{let r=self.remote.as_ref().unwrap();r.passengers.get(&id).into_iter().flatten().filter(|p|**p!=r.entity_id).map(|p|*p as usize).collect()}
 METHODS
}
fn game(me:i32, order:Vec<i32>)->Game {
 Game{riding:Some(7),remote:Some(Remote{entity_id:me,first_passenger:order.first()==Some(&me),passengers:HashMap::from([(7,order)])}),
 boats:vec![Boat{id:7,position:Vec3{x:0.0,y:64.0,z:0.0},yaw:0.0,on_ground:false,paddling:[true,true]}]}
}
#[test] fn each_client_uses_the_server_seat_order_and_only_first_drives(){
 for order in [vec![10,20],vec![20,10]] {
  for (index,me) in order.iter().enumerate() {
   let g=game(*me,order.clone());
   assert_eq!(g.remote_vehicle_packets().len(),if index==0{2}else{0},"passenger {me} must not send vehicle or paddle commands");
   let at=g.player_boat_seat(0);
   assert_eq!((at.x,at.y),(index as f64,2.0),"passenger {me} must use its assigned seat");
  }
 }
}
#[test] fn remaining_rider_becomes_driver_and_returns_to_center(){
 let g=game(20,vec![20]);
 assert_eq!(g.remote_vehicle_packets().len(),2);
 let at=g.player_boat_seat(0);assert_eq!((at.x,at.y),(0.0,1.0));
 let mut g=game(20,vec![10,20]);g.riding=None;
 assert!(g.remote_vehicle_packets().is_empty());
}
'''.replace('METHODS', methods)
        with tempfile.TemporaryDirectory(prefix='boat-passenger-control-') as tmp:
            tmp = Path(tmp)
            (tmp / 'test.rs').write_text(source)
            built = subprocess.run(['rustc', '--edition=2024', '--test', str(tmp/'test.rs'), '-o', str(tmp/'test')], capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stderr)
            result = subprocess.run([str(tmp/'test')], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
