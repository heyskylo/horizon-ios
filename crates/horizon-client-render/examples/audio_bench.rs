//! Audio occlusion, attenuation and mixing benchmark.
//! Run with: `cargo run --profile fast -p horizon-client-render --example audio_bench`
use horizon_client_render::{
    audio::Audio,
    sound_engine::{MobSound, SoundInstance, Sounds, TickContext},
};
use std::{path::Path, time::Instant};

fn main() {
    println!("=== Horizon Audio Attenuation & Mixing Benchmark ===");

    let mut audio = Audio::silent(Path::new("/nonexistent"));
    audio.engine.loaded = true;
    audio.backend_live_for_bench();
    audio.record_heard = false;

    // Register a sound event with attenuation distance 16
    audio.engine.registry.add_file(
        "minecraft",
        br#"{"bench.loop":{"attenuation_distance":16,"sounds":[{"name":"bench/loop","stream":false}]}}"#,
        &|_, _| true,
    );
    // Insert 1 second of audio into cache (48000 mono samples)
    let samples: Vec<f32> = (0..48_000)
        .map(|i| ((i as f32) * 440.0 * 2.0 * std::f32::consts::PI / 48000.0).sin() * 0.5)
        .collect();
    audio.insert_cached_sound("minecraft:bench/loop", samples, 48_000, 1);

    // 1. Benchmark set_listener (called every client render frame)
    let listener_iterations = 200_000;
    let start = Instant::now();
    for i in 0..listener_iterations {
        let angle = (i as f32) * 0.01;
        audio.set_listener(
            [
                100.0 + (angle.cos() as f64) * 5.0,
                64.0,
                200.0 + (angle.sin() as f64) * 5.0,
            ],
            angle,
            0.0,
        );
    }
    let listener_elapsed = start.elapsed();
    println!(
        "1. set_listener moving (render frame): {:?} for {} calls ({:.2} ns/call)",
        listener_elapsed,
        listener_iterations,
        (listener_elapsed.as_nanos() as f64) / (listener_iterations as f64)
    );

    // 1b. Benchmark set_listener stationary (lock elimination early-out)
    let stationary_iterations = 200_000;
    let start = Instant::now();
    for _ in 0..stationary_iterations {
        audio.set_listener([100.0, 64.0, 200.0], 0.0, 0.0);
    }
    let stationary_elapsed = start.elapsed();
    println!(
        "1b. set_listener stationary (lock early-out): {:?} for {} calls ({:.2} ns/call)",
        stationary_elapsed,
        stationary_iterations,
        (stationary_elapsed.as_nanos() as f64) / (stationary_iterations as f64)
    );

    // 2. Play 128 sounds in the world at various distances from listener at [100.0, 64.0, 200.0]
    // 64 sounds within attenuation distance (<16 blocks)
    // 64 sounds outside attenuation distance (>16 blocks)
    for i in 0..128 {
        let dist = if i < 64 {
            (i as f64) * 0.24 // 0 to 15.1 blocks (audible)
        } else {
            17.0 + ((i - 64) as f64) * 1.5 // 17 to 111.5 blocks (out of attenuation range)
        };
        let pos = [100.0 + dist, 64.0, 200.0];
        let instance = SoundInstance::new(
            "minecraft:bench.loop",
            crate_sound_source(i),
            1.0,
            1.0,
            horizon_core::random::LegacyRandom::new(i as i64),
            true, // looping
            0,
            horizon_client_render::sound_engine::Attenuation::Linear,
            pos,
            false,
        );
        audio.play_instance(instance, false);
    }

    // 3. Benchmark mix_buffer (5000 buffers of 1024 stereo frames @ 48kHz = ~106 seconds of audio mixing)
    let mut buffer = vec![0.0f32; 1024 * 2];
    let mix_iterations = 5_000;
    let start = Instant::now();
    for _ in 0..mix_iterations {
        audio.mix_buffer(&mut buffer, 2, 48_000);
    }
    let mix_elapsed = start.elapsed();
    println!(
        "2. mix_buffer (128 active channels, 1024 stereo frames): {:?} for {} buffers ({:.2} µs/buffer)",
        mix_elapsed,
        mix_iterations,
        (mix_elapsed.as_micros() as f64) / (mix_iterations as f64)
    );

    // 4. Benchmark SoundEngine::tick with ticking mob sounds
    let mut mobs = Vec::new();
    for i in 0..64 {
        mobs.push(MobSound {
            id: 2000 + i,
            position: [100.0 + (i as f64) * 0.5, 64.0, 200.0 + (i as f64) * 0.5],
            speed: 0.25,
            baby: false,
            angry: i % 2 == 0,
            silent: false,
            attack_scale: 0.5,
            sniffer_digging: false,
        });
    }

    let tick_ctx = TickContext {
        player_underwater: false,
        player_removed: false,
        vehicle: None,
        carts: Vec::new(),
        mobs: mobs.clone(),
        frozen: false,
        player_position: [100.0, 64.0, 200.0],
        player_speed_sqr: 0.1,
        player_fall_flying: false,
        ridden: None,
        camera_position: [100.0, 64.0, 200.0],
    };

    let tick_iterations = 10_000;
    let start = Instant::now();
    for _ in 0..tick_iterations {
        audio.tick(false, &tick_ctx);
    }
    let tick_elapsed = start.elapsed();
    println!(
        "3. SoundEngine::tick (64 channels + ticking): {:?} for {} ticks ({:.2} µs/tick)",
        tick_elapsed,
        tick_iterations,
        (tick_elapsed.as_micros() as f64) / (tick_iterations as f64)
    );
}

fn crate_sound_source(i: u32) -> usize {
    match i % 6 {
        0 => 0, // master
        1 => 1, // music
        2 => 2, // records
        3 => 3, // weather
        4 => 4, // blocks
        _ => 5, // hostile
    }
}
