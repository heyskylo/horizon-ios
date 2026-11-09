//! Text transport for the PalettedContainer comparison: replays scripted edits
//! and prints the same hashes as `VanillaPalettedContainerOracle`.
use horizon_core::paletted::{PalettedContainer, Strategy};
use std::{
    fs,
    io::{self, Write},
};
const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
fn fnv(h: u64, b: u8) -> u64 {
    (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
}
fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(BASIS, |h, &b| fnv(h, b))
}
fn varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}
/// `PalettedContainer.write`.
fn write(c: &PalettedContainer) -> Vec<u8> {
    let (bits, palette, words) = c.to_wire();
    let mut out = vec![bits];
    if bits == 0 {
        varint(&mut out, palette[0]);
    } else if !palette.is_empty() {
        varint(&mut out, palette.len() as u32);
        for p in palette {
            varint(&mut out, p);
        }
    }
    for w in words {
        out.extend(w.to_be_bytes());
    }
    out
}
/// `PalettedContainer.read` for the leading byte, palette and longs.
fn read(strategy: Strategy, bytes: &[u8]) -> PalettedContainer {
    let mut at = 1;
    let mut varint = || {
        let (mut v, mut shift) = (0u32, 0);
        loop {
            let b = bytes[at];
            at += 1;
            v |= u32::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    };
    let bits = bytes[0];
    let palette = if bits == 0 {
        vec![varint()]
    } else if strategy.wire_has_palette(bits) {
        let n = varint();
        (0..n).map(|_| varint()).collect()
    } else {
        Vec::new()
    };
    let words = bytes[at..]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_be_bytes(*c))
        .collect();
    PalettedContainer::from_wire(strategy, bits, palette, words).unwrap()
}
fn wire(c: &PalettedContainer) -> String {
    let bytes = write(c);
    format!("{}:{}", bytes.len(), hash_bytes(&bytes) as i64)
}
fn values(c: &PalettedContainer) -> i64 {
    let mut h = BASIS;
    for i in 0..c.strategy().entries() {
        for b in c.get(i).to_le_bytes() {
            h = fnv(h, b);
        }
    }
    h as i64
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut out = io::BufWriter::new(fs::File::create(&args[2])?);
    for line in fs::read_to_string(&args[1])?.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        let strategy = if t[1] == "blocks" {
            Strategy::blocks(16)
        } else {
            Strategy::biomes(7)
        };
        let mut c = PalettedContainer::new(strategy, t[2].parse()?);
        let every: usize = t[3].parse()?;
        let (mut olds, mut n) = (BASIS, 0usize);
        let mut checkpoints = Vec::new();
        for pair in t[4..].chunks(2) {
            let old = c.set(pair[0].parse()?, pair[1].parse()?);
            for b in old.to_le_bytes() {
                olds = fnv(olds, b);
            }
            n += 1;
            if n % every == 0 {
                checkpoints.push(wire(&c));
            }
        }
        let reread = read(strategy, &write(&c));
        let packed = c.pack();
        let storage = packed.data.as_ref().map_or(BASIS, |data| {
            data.iter().flat_map(|w| w.to_le_bytes()).fold(BASIS, fnv)
        });
        let palette: Vec<String> = packed.palette.iter().map(u32::to_string).collect();
        let loaded = PalettedContainer::from_packed(
            strategy,
            packed.palette.clone(),
            packed.data.as_deref(),
        )?;
        writeln!(
            out,
            "{} {} cp={} bits={} final={} values={} read={}/{} packed={}/{}/{} loaded={}/{}",
            t[0],
            olds as i64,
            checkpoints.join(","),
            c.bits(),
            wire(&c),
            values(&c),
            values(&reread),
            wire(&reread),
            packed.bits,
            storage as i64,
            palette.join(","),
            values(&loaded),
            wire(&loaded),
        )?;
    }
    Ok(())
}
