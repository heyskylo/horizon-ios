//! Repeated region-record compression benchmark without game resources.
//!
//! ```text
//! cargo run --profile fast -p horizon-world --example compression_bench -- --json
//! ```
//!
//! The benchmark compares the production `Compression::Zlib::encode` path,
//! which reuses a thread-local deflater, with a fresh encoder per record.
//! Payloads are deterministic synthetic chunk, entity, and POI records; the
//! output checksum and compressed sizes are checked so the work cannot vanish
//! under optimization. This measures codec setup and deflation, not NBT
//! serialization or filesystem I/O.

use flate2::{Compression as Level, write::ZlibEncoder};
use horizon_world::compression::Compression;
use std::{
    env,
    hint::black_box,
    io::Write,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    bytes: usize,
    seed: u32,
}

fn payload(case: Case) -> Vec<u8> {
    // Mix long repeated runs with changing records, like palette-heavy chunk
    // data and small entity/POI NBT records, without depending on resources.
    (0..case.bytes)
        .map(|i| {
            let word = (i as u32 / 17).wrapping_mul(0x9e37_79b9) ^ case.seed;
            if i % 97 < 73 {
                (word >> ((i & 3) * 8)) as u8 & 0x1f
            } else {
                word.rotate_left((i & 31) as u32) as u8
            }
        })
        .collect()
}

fn fresh_encode(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Level::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn run(case: Case, input: &[u8], iterations: usize) -> (Duration, Duration, usize, u64) {
    // Warm the production thread-local state before timing either side.
    for _ in 0..16 {
        black_box(Compression::Zlib.encode(input).expect("reused zlib"));
        black_box(fresh_encode(input));
    }

    let start = Instant::now();
    let mut reused_bytes = 0usize;
    let mut reused_checksum = 0u64;
    for _ in 0..iterations {
        let encoded = Compression::Zlib.encode(input).expect("reused zlib");
        reused_bytes += encoded.len();
        reused_checksum = reused_checksum.wrapping_add(
            encoded
                .iter()
                .step_by((encoded.len() / 17).max(1))
                .map(|&b| u64::from(b))
                .sum::<u64>(),
        );
        black_box(encoded);
    }
    let reused = start.elapsed();

    let start = Instant::now();
    let mut fresh_bytes = 0usize;
    let mut fresh_checksum = 0u64;
    for _ in 0..iterations {
        let encoded = fresh_encode(input);
        fresh_bytes += encoded.len();
        fresh_checksum = fresh_checksum.wrapping_add(
            encoded
                .iter()
                .step_by((encoded.len() / 17).max(1))
                .map(|&b| u64::from(b))
                .sum::<u64>(),
        );
        black_box(encoded);
    }
    let fresh = start.elapsed();
    assert_eq!(
        reused_bytes, fresh_bytes,
        "compressed size changed for {}",
        case.name
    );
    assert_eq!(
        reused_checksum, fresh_checksum,
        "compressed bytes changed for {}",
        case.name
    );
    (reused, fresh, reused_bytes / iterations, reused_checksum)
}

fn main() {
    let mut iterations = 200;
    let mut json = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--iterations" => {
                iterations = args
                    .next()
                    .expect("--iterations needs a number")
                    .parse()
                    .expect("invalid --iterations");
            }
            other => panic!("unknown argument {other}"),
        }
    }
    assert!(iterations > 0, "--iterations must be positive");
    let cases = [
        Case {
            name: "chunk",
            bytes: 64 * 1024,
            seed: 0x1234_5678,
        },
        Case {
            name: "entity",
            bytes: 4 * 1024,
            seed: 0x9abc_def0,
        },
        Case {
            name: "poi",
            bytes: 12 * 1024,
            seed: 0x0bad_cafe,
        },
    ];
    if json {
        print!("{{\"iterations\":{iterations},\"cases\":[");
    } else {
        println!("iterations={iterations}");
    }
    for (i, case) in cases.into_iter().enumerate() {
        let input = payload(case);
        let (reused, fresh, compressed_bytes, checksum) = run(case, &input, iterations);
        let reused_ms = reused.as_secs_f64() * 1000.0;
        let fresh_ms = fresh.as_secs_f64() * 1000.0;
        if json {
            if i != 0 {
                print!(",");
            }
            print!(
                "{{\"name\":\"{}\",\"input_bytes\":{},\"compressed_bytes\":{},\"reused_ms\":{:.3},\"fresh_ms\":{:.3},\"speedup\":{:.3},\"checksum\":{checksum}}}",
                case.name,
                case.bytes,
                compressed_bytes,
                reused_ms,
                fresh_ms,
                fresh_ms / reused_ms,
            );
        } else {
            println!(
                "{}: {} -> {} bytes, reused {:.3} ms, fresh {:.3} ms, speedup {:.2}x (checksum {})",
                case.name,
                case.bytes,
                compressed_bytes,
                reused_ms,
                fresh_ms,
                fresh_ms / reused_ms,
                checksum,
            );
        }
    }
    if json {
        println!("]}}");
    }
}
