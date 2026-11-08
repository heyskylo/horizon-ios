//! Baked far terrain: the generator's far samples (`LodSampler::grid`), kept
//! in the dimension's folder (`horizon_lod/`) so the next session draws them
//! without sampling the noise again. The detail chunks' columns are kept by
//! `lod::cache`.
//!
//! Only generator output is baked: saved chunks still override it
//! (`save::SavedLodSampler`), so edits show as before. A fingerprint of the
//! game build, the world's data packs, the block and biome registries and the
//! level's shape guards the folder; when any of them changes it is emptied.
//! Reduced far detail's approximate samples are kept apart from full ones.
use super::{Column, Span, Surface};
use crate::{chunk::LevelShape, fast_hash::FxHashMap};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use horizon_world::registry::BlockStateId;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const FORMAT: &str = "horizon-lod-bake 3";
const FOLDER: &str = "horizon_lod";
const FINGERPRINT: &str = "fingerprint.txt";
/// Tiles per bucket file side.
const GRID_BUCKET: i32 = 8;
/// The key written before each record: kind, x, z, stride, side.
const KEY_BYTES: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// `LodSampler::grid(x, z, stride, side)`, sampled approximately or not.
    Grid {
        approximate: bool,
        x: i32,
        z: i32,
        stride: i32,
        side: u16,
    },
}
impl Key {
    fn file(self) -> String {
        match self {
            Key::Grid {
                approximate,
                x,
                z,
                stride,
                side,
            } => {
                let span = stride * i32::from(side) * GRID_BUCKET;
                format!(
                    "{}{stride}x{side}.{}.{}.bin",
                    if approximate { "a" } else { "g" },
                    x.div_euclid(span),
                    z.div_euclid(span)
                )
            }
        }
    }
    fn bytes(self) -> [u8; KEY_BYTES] {
        let (kind, x, z, stride, side) = match self {
            Key::Grid {
                approximate,
                x,
                z,
                stride,
                side,
            } => (u8::from(approximate), x, z, stride, side),
        };
        let mut out = [0; KEY_BYTES];
        out[0] = kind;
        out[1..5].copy_from_slice(&x.to_le_bytes());
        out[5..9].copy_from_slice(&z.to_le_bytes());
        out[9..13].copy_from_slice(&stride.to_le_bytes());
        out[13..15].copy_from_slice(&side.to_le_bytes());
        out
    }
    fn from_bytes(b: &[u8; KEY_BYTES]) -> Option<Self> {
        let int = |at: usize| i32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        match b[0] {
            kind @ (0 | 1) => Some(Key::Grid {
                approximate: kind == 1,
                x: int(1),
                z: int(5),
                stride: int(9),
                side: u16::from_le_bytes([b[13], b[14]]),
            }),
            _ => None,
        }
    }
}

/// One append-only file of records: `[u32 length][key][zlib columns]`.
struct Bucket {
    file: fs::File,
    /// Each record's compressed columns: offset and length.
    index: FxHashMap<Key, (u64, u32)>,
    end: u64,
}
impl Bucket {
    fn open(path: &Path) -> std::io::Result<Self> {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let mut index = FxHashMap::default();
        let mut end = 0u64;
        let len = file.metadata()?.len();
        {
            let mut reader = BufReader::new(&mut file);
            loop {
                let mut length = [0; 4];
                if reader.read_exact(&mut length).is_err() {
                    break;
                }
                let length = u32::from_le_bytes(length);
                let mut key = [0; KEY_BYTES];
                if (length as usize) < KEY_BYTES || reader.read_exact(&mut key).is_err() {
                    break;
                }
                let Some(key) = Key::from_bytes(&key) else {
                    break;
                };
                let body = length - KEY_BYTES as u32;
                let start = end + 4 + KEY_BYTES as u64;
                // A record cut short by a crash ends the file there.
                if start + u64::from(body) > len || reader.seek_relative(i64::from(body)).is_err() {
                    break;
                }
                index.insert(key, (start, body));
                end = start + u64::from(body);
            }
        }
        if len != end {
            file.set_len(end)?;
        }
        Ok(Self { file, index, end })
    }
    fn read(&mut self, key: Key) -> Option<Vec<u8>> {
        let &(offset, length) = self.index.get(&key)?;
        let mut body = vec![0; length as usize];
        self.file.seek(SeekFrom::Start(offset)).ok()?;
        self.file.read_exact(&mut body).ok()?;
        Some(body)
    }
    fn append(&mut self, key: Key, body: &[u8]) -> std::io::Result<()> {
        let length = (KEY_BYTES + body.len()) as u32;
        let mut record = Vec::with_capacity(4 + length as usize);
        record.extend_from_slice(&length.to_le_bytes());
        record.extend_from_slice(&key.bytes());
        record.extend_from_slice(body);
        self.file.seek(SeekFrom::Start(self.end))?;
        self.file.write_all(&record)?;
        let start = self.end + 4 + KEY_BYTES as u64;
        self.index.insert(key, (start, body.len() as u32));
        self.end = start + body.len() as u64;
        Ok(())
    }
}

/// A dimension's baked far terrain, shared by the far sampling threads.
pub struct LodBake {
    dir: PathBuf,
    /// Open buckets by file name; `None` for one that failed to open.
    buckets: Mutex<FxHashMap<String, Option<Bucket>>>,
}
impl LodBake {
    /// Opens `dimension/horizon_lod`, emptying it when its fingerprint is not
    /// this build's, world's, level's and registries'. `None` (no baking) on
    /// an error.
    pub fn open(
        dimension: &Path,
        shape: LevelShape,
        block_states: usize,
        biomes: usize,
    ) -> Option<Self> {
        let dir = dimension.join(FOLDER);
        let fingerprint = fingerprint(dimension, shape, block_states, biomes);
        let current = fs::read_to_string(dir.join(FINGERPRINT)).ok();
        if current.as_deref() != Some(fingerprint.as_str()) {
            if current.is_some() {
                log::info!("Horizon: baked far terrain is out of date; baking it again");
            }
            let reset = || -> std::io::Result<()> {
                if dir.exists() {
                    fs::remove_dir_all(&dir)?;
                }
                fs::create_dir_all(&dir)?;
                fs::write(dir.join(FINGERPRINT), &fingerprint)
            };
            if let Err(e) = reset() {
                log::warn!(
                    "Horizon: far terrain will not be baked ({}): {e}",
                    dir.display()
                );
                return None;
            }
        }
        Some(Self {
            dir,
            buckets: Mutex::new(FxHashMap::default()),
        })
    }
    fn with_bucket<T>(&self, key: Key, f: impl FnOnce(&mut Bucket) -> Option<T>) -> Option<T> {
        let name = key.file();
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(name)
            .or_insert_with_key(|name| match Bucket::open(&self.dir.join(name)) {
                Ok(bucket) => Some(bucket),
                Err(e) => {
                    log::warn!("Horizon: baked far terrain {name}: {e}");
                    None
                }
            })
            .as_mut()?;
        f(bucket)
    }
    fn get(&self, key: Key, count: usize) -> Option<Vec<Column>> {
        let body = self.with_bucket(key, |bucket| bucket.read(key))?;
        let mut raw = Vec::new();
        ZlibDecoder::new(&body[..]).read_to_end(&mut raw).ok()?;
        decode(&raw, count)
    }
    fn put(&self, key: Key, columns: &[Column]) {
        if self.with_bucket(key, |bucket| Some(bucket.index.contains_key(&key))) != Some(false) {
            return;
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        let _ = encoder.write_all(&encode(columns));
        let Ok(body) = encoder.finish() else { return };
        self.with_bucket(key, |bucket| {
            if !bucket.index.contains_key(&key)
                && let Err(e) = bucket.append(key, &body)
            {
                log::warn!("Horizon: baking far terrain: {e}");
            }
            Some(())
        });
    }
    /// The baked `LodSampler::grid` result, if any.
    pub fn grid(
        &self,
        approximate: bool,
        [x, z]: [i32; 2],
        stride: i32,
        side: usize,
    ) -> Option<Vec<Column>> {
        let side16 = u16::try_from(side).ok()?;
        self.get(
            Key::Grid {
                approximate,
                x,
                z,
                stride,
                side: side16,
            },
            side * side,
        )
    }
    pub fn put_grid(
        &self,
        approximate: bool,
        [x, z]: [i32; 2],
        stride: i32,
        side: usize,
        columns: &[Column],
    ) {
        // A failed direct sample comes back as empty columns: never keep it.
        let Ok(side) = u16::try_from(side) else {
            return;
        };
        if columns.iter().all(|c| *c == Column::default()) {
            return;
        }
        self.put(
            Key::Grid {
                approximate,
                x,
                z,
                stride,
                side,
            },
            columns,
        );
    }
}

/// What baked columns depend on besides the world's seed (fixed per world).
fn fingerprint(dimension: &Path, shape: LevelShape, block_states: usize, biomes: usize) -> String {
    let build = super::cache::build_key(block_states, biomes);
    let packs = world_packs_digest(dimension);
    format!(
        "{FORMAT}\nbuild {build:016x}\nlevel {} {}\ndatapacks {packs}\n",
        shape.min_y, shape.height,
    )
}
/// `files_digest` of the data packs of the world `dimension` is a folder of.
pub fn world_packs_digest(dimension: &Path) -> String {
    // `dimensions/<namespace>/<path>` inside the world folder.
    dimension
        .ancestors()
        .nth(3)
        .map(|world| files_digest(&world.join("datapacks")))
        .unwrap_or_default()
}
/// Every file under `root`: its path, size and change time.
pub fn files_digest(root: &Path) -> String {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                let relative = path.strip_prefix(root).unwrap_or(&path);
                files.push(format!("{} {} {modified}", relative.display(), meta.len()));
            }
        }
    }
    files.sort_unstable();
    let mut hash = Sha256::new();
    for file in &files {
        hash.update(file.as_bytes());
        hash.update(b"\n");
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn encode(columns: &[Column]) -> Vec<u8> {
    let mut out = Vec::with_capacity(columns.len() * 24);
    out.extend_from_slice(&(columns.len() as u32).to_le_bytes());
    for column in columns {
        match column.surface {
            None => out.push(0),
            Some(s) => {
                out.push(if s.water.is_some() { 2 } else { 1 });
                out.extend_from_slice(&s.height.to_le_bytes());
                out.extend_from_slice(&s.state.0.to_le_bytes());
                out.extend_from_slice(&s.biome.to_le_bytes());
                if let Some((height, state)) = s.water {
                    out.extend_from_slice(&height.to_le_bytes());
                    out.extend_from_slice(&state.0.to_le_bytes());
                }
            }
        }
        out.extend_from_slice(&(column.spans.len() as u16).to_le_bytes());
        for span in &column.spans {
            out.extend_from_slice(&span.min.to_le_bytes());
            out.extend_from_slice(&span.max.to_le_bytes());
            out.extend_from_slice(&span.state.0.to_le_bytes());
            out.extend_from_slice(&span.biome.to_le_bytes());
            out.extend_from_slice(&span.light.to_le_bytes());
        }
    }
    out
}
fn decode(mut b: &[u8], count: usize) -> Option<Vec<Column>> {
    fn take<const N: usize>(b: &mut &[u8]) -> Option<[u8; N]> {
        let (head, rest) = b.split_first_chunk::<N>()?;
        *b = rest;
        Some(*head)
    }
    let i32_ = |b: &mut &[u8]| take::<4>(b).map(i32::from_le_bytes);
    let u32_ = |b: &mut &[u8]| take::<4>(b).map(u32::from_le_bytes);
    let u16_ = |b: &mut &[u8]| take::<2>(b).map(u16::from_le_bytes);
    let i16_ = |b: &mut &[u8]| take::<2>(b).map(i16::from_le_bytes);
    if u32_(&mut b)? as usize != count {
        return None;
    }
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = take::<1>(&mut b)?[0];
        let surface = match kind {
            0 => None,
            1 | 2 => {
                let height = i32_(&mut b)?;
                let state = BlockStateId(u32_(&mut b)?);
                let biome = u16_(&mut b)?;
                let water = if kind == 2 {
                    Some((i32_(&mut b)?, BlockStateId(u32_(&mut b)?)))
                } else {
                    None
                };
                Some(Surface {
                    height,
                    state,
                    water,
                    biome,
                })
            }
            _ => return None,
        };
        let spans = (0..u16_(&mut b)?)
            .map(|_| {
                Some(Span {
                    min: i16_(&mut b)?,
                    max: i16_(&mut b)?,
                    state: BlockStateId(u32_(&mut b)?),
                    biome: u16_(&mut b)?,
                    light: u16_(&mut b)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        columns.push(Column {
            surface,
            spans: spans.into(),
        });
    }
    b.is_empty().then_some(columns)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(seed: i32) -> Column {
        Column {
            surface: Some(Surface {
                height: 70 + seed,
                state: BlockStateId(9),
                water: (seed % 2 == 0).then_some((80, BlockStateId(86))),
                biome: 3,
            }),
            spans: vec![
                Span {
                    min: -64,
                    max: (70 + seed) as i16,
                    state: BlockStateId(1),
                    biome: 3,
                    light: 0,
                },
                Span {
                    min: (70 + seed) as i16,
                    max: 80,
                    state: BlockStateId(86),
                    biome: 3,
                    light: 240,
                },
            ]
            .into(),
        }
    }
    #[test]
    fn columns_round_trip_and_survive_reopening() {
        let world = std::env::temp_dir().join(format!("horizon-lod-bake-{}", std::process::id()));
        let _ = fs::remove_dir_all(&world);
        let dimension = world.join("dimensions").join("minecraft").join("overworld");
        fs::create_dir_all(&dimension).unwrap();
        let grid: Vec<Column> = (0..256).map(sample).collect();
        let other: Vec<Column> = (1..257).map(sample).collect();
        let shape = LevelShape::OVERWORLD;
        {
            let bake = LodBake::open(&dimension, shape, 100, 10).unwrap();
            assert_eq!(bake.grid(false, [-64, 32], 4, 16), None);
            bake.put_grid(false, [-64, 32], 4, 16, &grid);
            // A second put of the same key is ignored, not appended.
            bake.put_grid(false, [-64, 32], 4, 16, &[sample(-1)]);
            assert_eq!(
                bake.grid(false, [-64, 32], 4, 16).as_deref(),
                Some(&grid[..])
            );
            // Reduced far detail's samples are kept apart from full ones.
            assert_eq!(bake.grid(true, [-64, 32], 4, 16), None);
            bake.put_grid(true, [-64, 32], 4, 16, &other);
            assert_eq!(
                bake.grid(true, [-64, 32], 4, 16).as_deref(),
                Some(&other[..])
            );
        }
        let bake = LodBake::open(&dimension, shape, 100, 10).unwrap();
        assert_eq!(
            bake.grid(false, [-64, 32], 4, 16).as_deref(),
            Some(&grid[..])
        );
        assert_eq!(bake.grid(false, [-64, 48], 4, 16), None);
        drop(bake);
        // A cut-off record (a crash mid-write) is dropped, the rest is kept:
        // (-128, 32) shares (-64, 32)'s bucket file.
        let key = Key::Grid {
            approximate: false,
            x: -64,
            z: 32,
            stride: 4,
            side: 16,
        };
        let file = dimension.join(FOLDER).join(key.file());
        let full = fs::metadata(&file).unwrap().len();
        let bake = LodBake::open(&dimension, shape, 100, 10).unwrap();
        bake.put_grid(false, [-128, 32], 4, 16, &other);
        drop(bake);
        let f = fs::OpenOptions::new().write(true).open(&file).unwrap();
        f.set_len(fs::metadata(&file).unwrap().len() - 5).unwrap();
        drop(f);
        let bake = LodBake::open(&dimension, shape, 100, 10).unwrap();
        assert_eq!(bake.grid(false, [-128, 32], 4, 16), None);
        assert_eq!(
            bake.grid(false, [-64, 32], 4, 16).as_deref(),
            Some(&grid[..])
        );
        assert_eq!(fs::metadata(&file).unwrap().len(), full);
        drop(bake);
        // Another registry (or data pack, build or level shape) empties the folder.
        let bake = LodBake::open(&dimension, shape, 101, 10).unwrap();
        assert_eq!(bake.grid(false, [-64, 32], 4, 16), None);
        drop(bake);
        fs::remove_dir_all(&world).unwrap();
    }
    #[test]
    fn empty_samples_are_not_baked() {
        let world = std::env::temp_dir().join(format!("horizon-lod-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&world);
        let dimension = world.join("dimensions").join("minecraft").join("overworld");
        fs::create_dir_all(&dimension).unwrap();
        let bake = LodBake::open(&dimension, LevelShape::OVERWORLD, 1, 1).unwrap();
        bake.put_grid(false, [0, 0], 16, 16, &vec![Column::default(); 256]);
        assert_eq!(bake.grid(false, [0, 0], 16, 16), None);
        drop(bake);
        fs::remove_dir_all(&world).unwrap();
    }
}
