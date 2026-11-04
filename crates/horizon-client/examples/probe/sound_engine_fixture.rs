//! Replays `tools/test_sound_engine_parity.py` scripts through Horizon's
//! `SoundEngine` with a backend recording what an OpenAL source would hold:
//! `sound_engine_fixture ROOT INPUT OUTPUT`.
use crate::audio_common as common;
use common::{Fake, d, describe, f, flush, result_name};
use horizon_client::sound_engine::{
    Attenuation, Backend, CartSound, InstanceId, SoundEngine, SoundEntity, SoundEntityState,
    SoundInstance, TickContext,
};
use horizon_client::sound_events::SoundRegistry;
use horizon_core::random::LegacyRandom;
use std::collections::BTreeMap;
use std::io::Write;

pub fn main() {
    let args: Vec<String> = crate::args().collect();
    let mut engine = SoundEngine::new(SoundRegistry::load(std::path::Path::new(&args[1])));
    engine.listening = true;
    let mut backend = Fake::default();
    let mut ctx = TickContext::default();
    let mut instances: BTreeMap<String, InstanceId> = BTreeMap::new();
    // Script cart names to entity ids (a removed cart leaves the level).
    let mut cart_ids: BTreeMap<String, u32> = BTreeMap::new();
    let mut removed: BTreeMap<u32, CartSound> = BTreeMap::new();
    let mut bound_entities: BTreeMap<String, SoundEntity> = BTreeMap::new();
    let input = std::fs::read_to_string(&args[2]).expect("input");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[3]).expect("output"));
    for line in input.lines() {
        writeln!(out, "{line}").expect("write");
        let a: Vec<&str> = line.split(' ').collect();
        let source = |i: usize| a[i].parse::<usize>().expect("source");
        match a[0] {
            "play" => {
                let event = horizon_client::sound_events::normalize(a[3]);
                let random = LegacyRandom::new(a[14].parse().expect("seed"));
                let instance = match a[2] {
                    "simple" => SoundInstance::new(
                        &event,
                        source(4),
                        f(a[5]),
                        f(a[6]),
                        random,
                        a[10] == "1",
                        a[11].parse().expect("delay"),
                        if a[12] == "linear" {
                            Attenuation::Linear
                        } else {
                            Attenuation::None
                        },
                        [d(a[7]), d(a[8]), d(a[9])],
                        a[13] == "1",
                    ),
                    "loop" => SoundInstance::biome_loop(&event, random),
                    "uloop" => SoundInstance::under_liquid_loop(&event, random),
                    "usub" => SoundInstance::under_liquid_sub(&event, random),
                    "bound" => SoundInstance::entity_bound(
                        &event,
                        source(4),
                        f(a[5]),
                        f(a[6]),
                        random,
                        bound_entities[a[15]].clone(),
                    ),
                    "cart" | "rcart" | "rcartu" => {
                        let id = cart_ids[a[3]];
                        let cart = ctx
                            .carts
                            .iter()
                            .find(|c| c.id == id)
                            .or_else(|| removed.get(&id))
                            .copied()
                            .expect("cart");
                        if a[2] == "cart" {
                            SoundInstance::minecart(&cart, random)
                        } else {
                            SoundInstance::riding_minecart(&cart, a[2] == "rcartu", random)
                        }
                    }
                    _ => panic!("{line}"),
                };
                if let Some(old) = instances.remove(a[1]) {
                    engine.release(old);
                }
                let (id, result) = engine.play_new(instance, true, &mut backend);
                instances.insert(a[1].to_owned(), id);
                writeln!(out, "  result {}", result_name(result)).expect("write");
            }
            "replay" => {
                let result = engine.play(instances[a[1]], &mut backend);
                writeln!(out, "  result {}", result_name(result)).expect("write");
            }
            "queue" => engine.queue_ticking(instances[a[1]]),
            "delayed" => engine.play_delayed(instances[a[1]], a[2].parse().expect("delay")),
            "fade" => {
                let i = engine.instance_mut(instances[a[1]]).expect("instance");
                if a[2] == "in" {
                    i.fade_in();
                } else {
                    i.fade_out();
                }
            }
            "stop" => engine.stop(instances[a[1]], &mut backend),
            "stopmatch" => {
                let sound = (a[1] != "-").then(|| horizon_client::sound_events::normalize(a[1]));
                let src = (a[2] != "-").then(|| source(2));
                engine.stop_matching(sound.as_deref(), src, &mut backend);
            }
            "finish" => {
                if let Some(c) = engine.channel(instances[a[1]]) {
                    backend.stop(c);
                }
            }
            "gain" => engine.update_category_volume(source(1), f(a[2]), &mut backend),
            "volume" => engine.source_volumes[source(1)] = f(a[2]),
            "refresh" => engine.refresh_category_volume(source(1), &mut backend),
            "pause" => engine.pause_all_except(&[1, 10], &mut backend),
            "resume" => engine.resume(&mut backend),
            "player" => {
                ctx.player_underwater = a[1] == "1";
                ctx.player_removed = a[2] == "1";
            }
            "cart" | "cartnew" => {
                let next = cart_ids.len() as u32 + 1;
                let id = *cart_ids.entry(a[1].to_owned()).or_insert(next);
                ctx.carts.retain(|c| c.id != id);
                let cart = CartSound {
                    id,
                    position: [d(a[2]), d(a[3]), d(a[4])],
                    // Vec3.horizontalDistance.
                    speed: (d(a[5]) * d(a[5]) + d(a[7]) * d(a[7])).sqrt(),
                };
                let silent = a.get(9) == Some(&"1");
                if a[0] == "cartnew"
                    && let Some(old) = bound_entities.remove(a[1])
                {
                    let mut state = old.state();
                    state.removed = true;
                    old.update(state);
                }
                let entity = bound_entities
                    .entry(a[1].to_owned())
                    .or_insert_with(|| SoundEntity::new(id, cart.position, silent));
                entity.update(SoundEntityState {
                    position: cart.position,
                    silent,
                    removed: a[8] == "1",
                });
                if a[8] == "1" {
                    // Out of the level, still there for a later play.
                    removed.insert(id, cart);
                } else {
                    removed.remove(&id);
                    ctx.carts.push(cart);
                }
            }
            "ride" => ctx.vehicle = cart_ids.get(a[1]).copied(),
            "freeze" => ctx.frozen = a[1] == "1",
            "tick" => engine.tick(a[1] == "1", &ctx, &mut backend),
            "forget" => {
                if let Some(id) = instances.remove(a[1]) {
                    engine.release(id);
                }
            }
            _ => panic!("{line}"),
        }
        for (name, id) in &instances {
            writeln!(out, "{}", describe(&engine, &backend, name, *id)).expect("write");
        }
        flush(&mut engine, &mut out);
    }
}
