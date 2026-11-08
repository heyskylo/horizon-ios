//! Captured level sounds through the production packet builder and protocol codec.
use horizon_client::{audio::CapturedSound, lan::containers::captured_sound_packet};
use horizon_protocol::{buf::Reader, packets::play::Clientbound as C};
use std::io::Write;

pub fn main() {
    let args: Vec<String> = crate::args().collect();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    for line in std::fs::read_to_string(&args[1]).unwrap().lines() {
        let c: serde_json::Value = serde_json::from_str(line).unwrap();
        let float =
            |key: &str| f32::from_bits(u32::from_str_radix(c[key].as_str().unwrap(), 16).unwrap());
        let position = std::array::from_fn(|i| {
            f64::from_bits(u64::from_str_radix(c["position"][i].as_str().unwrap(), 16).unwrap())
        });
        let sound = CapturedSound {
            event: "fixture:bound".into(),
            position,
            volume: float("volume"),
            pitch: float("pitch"),
            source: c["source"].as_u64().unwrap() as usize,
            entity: c["bound"]
                .as_bool()
                .unwrap()
                .then(|| c["entity"].as_i64().unwrap() as u32),
            except_entity: None,
        };
        let (packet, range) = captured_sound_packet(
            &sound,
            c["sound_id"].as_i64().map(|n| n as i32),
            c["seed"].as_i64().unwrap(),
        );
        let kind = match packet {
            C::SoundEntity(_) => "entity",
            C::Sound(_) => "position",
            _ => unreachable!(),
        };
        let bytes = packet.encode();
        let mut reader = Reader::new(&bytes);
        let id = reader.varint().unwrap();
        // The native StreamCodec writes the payload only; also require our own
        // decoder to consume this packet fully before comparing payload bytes.
        let prefix = bytes.len() - reader.remaining();
        C::decode(id, &bytes[prefix..]).unwrap();
        let payload: String = bytes[prefix..].iter().map(|b| format!("{b:02x}")).collect();
        writeln!(out, "{kind} {:016x} {payload}", range.to_bits()).unwrap();
    }
}
