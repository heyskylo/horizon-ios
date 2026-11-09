//! Line-based differential probe. Not a game client or server.
use horizon_core::{
    position::{BlockPos, ChunkPos, SectionPos},
    random::{
        LegacyPositionalRandom, LegacyRandom, PositionalRandom, RandomSource,
        XoroshiroPositionalRandom, XoroshiroRandom, java_string_hash, position_seed,
        seed_from_hash_utf16,
    },
    storage::PackedStorage,
};
use std::io::{self, BufRead, BufWriter, Write};

fn random_probe<R: RandomSource>(out: &mut impl Write, mut rng: R, count: usize) -> io::Result<()> {
    let bounds = [1, 2, 3, 16, 31, 256, 65_537, 1_073_741_825, i32::MAX, 0, -1];
    for i in 0..count {
        let value = match i % 10 {
            0 => rng.next_int().to_string(),
            1 => rng.next_long().to_string(),
            2 => match rng.next_int_bound(bounds[(i / 10) % bounds.len()]) {
                Ok(value) => value.to_string(),
                Err(_) => "error".to_owned(),
            },
            3 => u8::from(rng.next_boolean()).to_string(),
            4 => rng.next_float().to_bits().to_string(),
            5 => rng.next_double().to_bits().to_string(),
            6 => rng.fork().next_long().to_string(),
            7 => {
                rng.consume_count((i % 7) as i32 - 2);
                rng.next_long().to_string()
            }
            8 => {
                if i % 101 == 8 {
                    let seed = rng.next_long().wrapping_sub(i as i64);
                    rng.set_seed(seed);
                }
                rng.next_int_bound(1_073_741_825).unwrap().to_string()
            }
            _ => rng.next_int().to_string(),
        };
        writeln!(out, "rng {i} {value}")?;
    }
    Ok(())
}

fn read<T: std::str::FromStr>(args: &mut std::str::SplitWhitespace<'_>) -> Result<T, String> {
    args.next()
        .ok_or_else(|| "missing argument".to_owned())?
        .parse()
        .map_err(|_| "invalid argument".to_owned())
}
fn helpers<R: RandomSource>(
    out: &mut impl Write,
    mut rng: R,
    bounds: [i32; 2],
    double: [f64; 2],
    float: [f32; 2],
) -> io::Result<()> {
    for i in 0..32 {
        let inclusive = rng
            .next_int_between_inclusive(bounds[0], bounds[1])
            .map_or_else(|_| "error".to_owned(), |v| v.to_string());
        let exclusive = rng
            .next_int_between(bounds[0], bounds[1])
            .map_or_else(|_| "error".to_owned(), |v| v.to_string());
        let d = rng.triangle(double[0], double[1]);
        let f = rng.triangle_float(float[0], float[1]);
        let d = if d.is_nan() {
            0x7ff8_0000_0000_0000
        } else {
            d.to_bits()
        };
        let f = if f.is_nan() { 0x7fc0_0000 } else { f.to_bits() };
        writeln!(
            out,
            "helpers {i} {inclusive} {exclusive} {d} {f} {}",
            rng.next_long()
        )?;
    }
    random_probe(out, rng, 24)
}
fn utf16(token: &str) -> Result<Vec<u16>, String> {
    if token == "-" {
        return Ok(Vec::new());
    }
    if !token.len().is_multiple_of(4) {
        return Err("invalid UTF-16 hex".to_owned());
    }
    token
        .as_bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|part| {
            u16::from_str_radix(
                std::str::from_utf8(part).map_err(|_| "invalid UTF-16 hex")?,
                16,
            )
            .map_err(|_| "invalid UTF-16 hex".to_owned())
        })
        .collect()
}
fn factory_probe<P: PositionalRandom>(
    out: &mut impl Write,
    factory: P,
    position: BlockPos,
    seed: i64,
    name: &[u16],
    count: usize,
) -> io::Result<()> {
    for (label, rng) in [
        ("at", factory.at(position)),
        ("seed", factory.from_seed(seed)),
        ("hash", factory.from_hash_of_utf16(name)),
    ] {
        writeln!(out, "factory {label}")?;
        random_probe(out, rng, count)?;
    }
    Ok(())
}

fn probe(line: &str, out: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = line.split_whitespace();
    match args.next().ok_or("missing operation")? {
        "helpers" => {
            let kind = args.next().ok_or("missing RNG kind")?;
            let seed: i64 = read(&mut args)?;
            let bounds = [read(&mut args)?, read(&mut args)?];
            let d = [
                f64::from_bits(u64::from_str_radix(
                    args.next().ok_or("missing double")?,
                    16,
                )?),
                f64::from_bits(u64::from_str_radix(
                    args.next().ok_or("missing double")?,
                    16,
                )?),
            ];
            let f = [
                f32::from_bits(u32::from_str_radix(
                    args.next().ok_or("missing float")?,
                    16,
                )?),
                f32::from_bits(u32::from_str_radix(
                    args.next().ok_or("missing float")?,
                    16,
                )?),
            ];
            match kind {
                "legacy" => helpers(out, LegacyRandom::new(seed), bounds, d, f)?,
                "xoroshiro" => helpers(out, XoroshiroRandom::new(seed), bounds, d, f)?,
                _ => return Err("unknown RNG kind".into()),
            }
        }
        "factory" => {
            let kind = args.next().ok_or("missing RNG kind")?;
            let mode: i32 = read(&mut args)?;
            let lo: i64 = read(&mut args)?;
            let hi: i64 = read(&mut args)?;
            let position = BlockPos::new(read(&mut args)?, read(&mut args)?, read(&mut args)?);
            let seed: i64 = read(&mut args)?;
            let name = utf16(args.next().ok_or("missing name")?)?;
            let count: usize = read(&mut args)?;
            let hash = seed_from_hash_utf16(&name);
            writeln!(
                out,
                "seeds {} {} {} {}",
                position_seed(position),
                java_string_hash(name.iter().copied()),
                hash[0] as i64,
                hash[1] as i64
            )?;
            if kind == "legacy" {
                let mut source = LegacyRandom::new(lo);
                let factory = if mode == 0 {
                    LegacyPositionalRandom::new(lo)
                } else {
                    source.fork_positional()
                };
                factory_probe(out, factory, position, seed, &name, count)?;
                if mode != 0 {
                    writeln!(out, "parent {}", source.next_long())?;
                }
            } else {
                let mut source = XoroshiroRandom::new(lo);
                let factory = if mode == 0 {
                    XoroshiroPositionalRandom::new(lo as u64, hi as u64)
                } else {
                    source.fork_positional()
                };
                factory_probe(out, factory, position, seed, &name, count)?;
                if mode != 0 {
                    writeln!(out, "parent {}", source.next_long())?;
                }
            }
        }
        "position" => {
            let p = BlockPos::new(read(&mut args)?, read(&mut args)?, read(&mut args)?);
            let offset = p.offset(read(&mut args)?, read(&mut args)?, read(&mut args)?);
            let packed = p.pack();
            let decoded = BlockPos::unpack(packed);
            let section = p.section();
            let sp = section.pack();
            let sd = SectionPos::unpack(sp);
            let chunk = p.chunk();
            let cp = chunk.pack();
            let cd = ChunkPos::unpack(cp);
            let region = chunk.region();
            let (rx, rz) = chunk.region_local();
            let relative = p.section_relative();
            let rebuilt = section.relative_to_block(relative);
            writeln!(
                out,
                "position {packed} {} {} {} {sp} {} {} {} {cp} {} {} {} {} {rx} {rz} {relative} {} {} {} {} {} {} {}",
                decoded.x,
                decoded.y,
                decoded.z,
                sd.x,
                sd.y,
                sd.z,
                cd.x,
                cd.z,
                region.x,
                region.z,
                p.section_index(),
                rebuilt.x,
                rebuilt.y,
                rebuilt.z,
                offset.x,
                offset.y,
                offset.z
            )?;
        }
        "legacy" => random_probe(out, LegacyRandom::new(read(&mut args)?), read(&mut args)?)?,
        "xoroshiro" => random_probe(
            out,
            XoroshiroRandom::new(read(&mut args)?),
            read(&mut args)?,
        )?,
        "xoroshiro_state" => random_probe(
            out,
            XoroshiroRandom::from_state(
                read::<i64>(&mut args)? as u64,
                read::<i64>(&mut args)? as u64,
            ),
            read(&mut args)?,
        )?,
        "storage" => {
            let bits: u8 = read(&mut args)?;
            let len: usize = read(&mut args)?;
            let seed: i64 = read(&mut args)?;
            let mut storage = PackedStorage::new(bits, len)?;
            let mut rng = LegacyRandom::new(seed);
            let mask = ((1_u64 << bits) - 1).min(i32::MAX as u64);
            for i in 0..len * 3 {
                let index = rng.next_int_bound(len as i32)? as usize;
                let value = (rng.next_int() as u32 as u64 & mask) as u32;
                let old = storage.set(index, value)?;
                writeln!(out, "set {i} {index} {old} {}", storage.get(index)?)?;
            }
            for (i, &word) in storage.words().iter().enumerate() {
                writeln!(out, "word {i} {}", word as i64)?;
            }
            for (i, value) in storage.unpack().iter().enumerate() {
                writeln!(out, "value {i} {}", *value as i32)?;
            }
            let mut imported =
                PackedStorage::from_words(bits, len, vec![u64::MAX; storage.words().len()])?;
            for i in 0..len {
                writeln!(out, "import {i} {}", imported.get(i)? as i32)?;
            }
            if len > 0 {
                imported.set(len - 1, 0)?;
            }
            for (i, &word) in imported.words().iter().enumerate() {
                writeln!(out, "import_word {i} {}", word as i64)?;
            }
            for value in [0, 1, mask as u32] {
                imported.fill(value)?;
                for (i, &word) in imported.words().iter().enumerate() {
                    writeln!(out, "fill {value} {i} {}", word as i64)?;
                }
            }
            let before = storage.clone();
            writeln!(
                out,
                "invalid_index {}",
                u8::from(storage.set(len, 0).is_err())
            )?;
            let invalid_value = if bits == 32 { u32::MAX } else { 1_u32 << bits };
            // An empty storage has no valid index at which to test value errors.
            if len > 0 {
                writeln!(
                    out,
                    "invalid_value {}",
                    u8::from(storage.set(0, invalid_value).is_err())
                )?;
            }
            writeln!(out, "unchanged {}", u8::from(storage == before))?;
        }
        _ => return Err("unknown operation".into()),
    }
    if args.next().is_some() {
        return Err("unexpected trailing argument".into());
    }
    Ok(())
}

fn main() {
    let mut out = BufWriter::new(io::stdout().lock());
    for (i, line) in io::stdin().lock().lines().enumerate() {
        let result = line
            .map_err(|e| e.to_string())
            .and_then(|line| probe(&line, &mut out).map_err(|e| e.to_string()));
        if let Err(error) = result {
            eprintln!("line {}: {error}", i + 1);
            std::process::exit(2);
        }
    }
    if let Err(error) = out.flush() {
        eprintln!("output failed: {error}");
        std::process::exit(2);
    }
}
