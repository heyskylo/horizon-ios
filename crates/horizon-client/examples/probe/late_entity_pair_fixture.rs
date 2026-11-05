//! Late trident pairing and subsequent updates through the production tracker.
use horizon_client::{
    Result,
    lan::entity::{Entities, EntityState, Riding, Tracker, Viewer},
};
use horizon_protocol::{
    buf::Reader,
    entity_data::{Value, serializer},
    packets::play::Clientbound as C,
};
use std::io::Write;

fn dump(out: &mut impl Write, index: usize, phase: &str, packets: &mut Vec<Vec<u8>>) -> Result<()> {
    let mut encoded = Vec::new();
    for bytes in packets.drain(..) {
        let mut r = Reader::new(&bytes);
        let id = r.varint()?;
        let payload = r.rest();
        let name = match C::decode(id, payload)? {
            C::AddEntity(_) => "add",
            C::SetEntityMotion(_) => "motion",
            C::MoveEntityPosRot(_) => "posrot",
            C::SetEntityData(_) => "data",
            other => panic!("unexpected late-pair packet {other:?}"),
        };
        let hex: String = payload.iter().map(|b| format!("{b:02x}")).collect();
        encoded.push(format!("{name}:{hex}"));
    }
    writeln!(out, "{index} {phase} {}", encoded.join(","))?;
    Ok(())
}

pub fn main() -> Result<()> {
    let args: Vec<String> = crate::args().collect();
    let kind: i32 = args[1].parse()?;
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2])?);
    writeln!(out, "type {kind}")?;
    let mut index = 0;
    for position in [[0.5, 65.5, 0.5], [-0.123456789, 64.125, -0.5]] {
        for velocity in [[0.25, 0.0, 0.0], [0.0; 3], [0.25, -0.5, 0.125]] {
            for landed in [false, true] {
                let mut level = Entities::default();
                let mut tracker = Tracker::default();
                let mut packets = Vec::new();
                let chunks = [(0, 0), (-1, -1), (0, -1), (-1, 0)].into_iter().collect();
                let mut viewers = [Viewer {
                    entity_id: 1,
                    spectator: false,
                    permission: 0,
                    moved: false,
                    position: [0.5, 64.0, 0.5],
                    view_distance: 10,
                    chunks: &chunks,
                    tracker: &mut tracker,
                    out: &mut packets,
                    gone: None,
                }];
                let riding = Riding::default();
                level.tick(&[], &riding, &mut viewers, |_| Some(kind));
                let mut e = EntityState {
                    id: 37,
                    uuid: 37,
                    kind: "minecraft:trident".into(),
                    position,
                    velocity,
                    ..EntityState::default()
                };
                level.pair_after_tick(std::slice::from_ref(&e), &riding, &mut viewers, |_| {
                    Some(kind)
                });
                dump(&mut out, index, "pair", viewers[0].out)?;
                e.position[0] += 0.25;
                e.velocity = [0.0; 3];
                if landed {
                    e.meta.push((10, serializer::BOOLEAN, Value::Bool(true)));
                }
                level.tick(&[&e], &riding, &mut viewers, |_| Some(kind));
                dump(&mut out, index, "change", viewers[0].out)?;
                level.tick(&[&e], &riding, &mut viewers, |_| Some(kind));
                dump(&mut out, index, "again", viewers[0].out)?;
                index += 1;
            }
        }
    }
    Ok(())
}
