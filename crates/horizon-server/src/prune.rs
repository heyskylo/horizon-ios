//! `[prune_chunks]` in server.toml: deletes the chunks players barely visited, so a world that players
//! crossed in every direction shrinks back (docs/hosting.md). A chunk whose `InhabitedTime` (vanilla's
//! count of the ticks a player spent near it) is under `min_inhabited_ticks` generates again from the
//! seed, as it was, when someone comes back. Kept whatever their time: the chunks within `spawn_radius`
//! of the world spawn (of 0 0 in the other dimensions), the chunks the files in `keep_files` name (the
//! SMP mod's claims and homes), every chunk that can't be read, and `ring` chunks around every kept
//! chunk, so no seam opens beside a build.
//!
//! `mode = "dry-run"` only writes the report, while the game runs; `mode = "delete"` prunes before the
//! game starts. Either sets `mode` back to "off". The chunk's entries in `entities/` and `poi/` go with
//! it; region files are rewritten compacted in vanilla's format (each kept chunk's bytes and timestamp
//! copied as they were) and a file left empty is deleted. Nothing else in the world is touched: not
//! level.dat, player data, stats or advancements.

use horizon_world::{
    nbt::{self, Limits, Tag},
    region::{ChunkRecord, RegionFile, RegionWriter, position_from_path},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

const SECTOR: u64 = 4096;
/// Where rewritten files wait, inside each folder, before they replace the old ones in one rename.
const STAGING: &str = ".horizon-prune";
/// The decoded bytes read first: `InhabitedTime` comes before the sections in vanilla's and Horizon's
/// chunks, so most chunks are never decoded in full.
const PREFIX: usize = 16 * 1024;
const KINDS: [&str; 3] = ["region", "entities", "poi"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Off,
    DryRun,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// "off", "dry-run" (a report, the game starts at once) or "delete" (before the game starts).
    pub mode: Mode,
    /// Chunks with less `InhabitedTime` than this go (1200 ticks: a minute).
    pub min_inhabited_ticks: i64,
    /// Chunks this close to the spawn (in chunks, on each axis) are kept.
    pub spawn_radius: u32,
    /// Chunks this close to a kept chunk are kept too.
    pub ring: u32,
    /// JSON files in the world folder naming chunks to keep: keys or strings `<dimension> <chunk x>
    /// <chunk z>` (claims), and objects with `dimension`, `x` and `z` (homes, warps). A file that is
    /// missing is skipped; one that can't be read stops the prune.
    pub keep_files: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            min_inhabited_ticks: 1200,
            spawn_radius: 32,
            ring: 1,
            keep_files: vec![
                "horizon-smp/claims.json".into(),
                "horizon-smp/places.json".into(),
            ],
        }
    }
}

impl Settings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn check(&self) -> Result<(), String> {
        if self.min_inhabited_ticks < 0 || self.ring > 8 {
            return Err("prune_chunks: min_inhabited_ticks must be 0 or more, ring 0 to 8".into());
        }
        if let Some(bad) = self
            .keep_files
            .iter()
            .find(|f| Path::new(f).is_absolute() || f.split(['/', '\\']).any(|p| p == ".."))
        {
            return Err(format!(
                "prune_chunks.keep_files: {bad:?} must be a path inside the world folder"
            ));
        }
        Ok(())
    }
}

type Chunk = (i32, i32);

#[derive(Clone, Copy, Default)]
struct Bits([u64; 16]);
impl Bits {
    fn set(&mut self, i: usize) {
        self.0[i / 64] |= 1 << (i % 64);
    }
    fn has(&self, i: usize) -> bool {
        self.0[i / 64] & (1 << (i % 64)) != 0
    }
    fn count(&self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }
}

fn slot(c: Chunk) -> usize {
    (c.0 & 31) as usize + (c.1 & 31) as usize * 32
}

/// A region file's header, read without its chunks.
struct Header {
    len: u64,
    /// Each slot's sector count (0: no chunk).
    sectors: [u8; 1024],
    /// A chunk too big for the file lives in its own `.mcc` file: such regions are left as they are.
    oversized: bool,
}

fn header(path: &Path) -> io::Result<Header> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    // As vanilla's RegionFile: a short file reads as far as it goes, and entries that overlap the header,
    // are empty or start past the end hold no chunk.
    let mut table = Vec::with_capacity(4096);
    (&mut file).take(4096).read_to_end(&mut table)?;
    table.resize(4096, 0);
    let mut sectors = [0; 1024];
    let mut oversized = false;
    for (i, entry) in table.as_chunks::<4>().0.iter().enumerate() {
        let entry = u32::from_be_bytes(*entry);
        let start = u64::from(entry >> 8);
        if start < 2 || entry & 255 == 0 || start * SECTOR > len {
            continue;
        }
        sectors[i] = (entry & 255) as u8;
        let mut head = [0; 5];
        file.seek(SeekFrom::Start(u64::from(entry >> 8) * SECTOR))?;
        if file.read_exact(&mut head).is_ok() && head[4] & 128 != 0 {
            oversized = true;
        }
    }
    Ok(Header {
        len,
        sectors,
        oversized,
    })
}

/// What a compacted file would hold without `drop`: its new length (0: deleted).
fn compacted_len(h: &Header, gone: &Bits) -> u64 {
    let kept: u64 = (0..1024)
        .filter(|&i| h.sectors[i] != 0 && !gone.has(i))
        .map(|i| u64::from(h.sectors[i]))
        .sum();
    if kept == 0 { 0 } else { (2 + kept) * SECTOR }
}

/// One region file of chunks after the scan.
enum Scan {
    Read {
        present: Bits,
        /// Inhabited long enough, or unreadable.
        busy: Bits,
        unreadable: u32,
        header: Header,
    },
    /// The file couldn't be read: every chunk in it counts as kept.
    Failed(String),
}

fn scan_region(path: &Path, min_ticks: i64) -> Scan {
    let result = (|| -> io::Result<Scan> {
        let header = header(path)?;
        let mut file = RegionFile::open(path)?;
        let (mut present, mut busy, mut unreadable) = (Bits::default(), Bits::default(), 0);
        let chunks: Vec<_> = file.chunks().map(|(c, _)| c).collect();
        for c in chunks {
            let i = slot((c.x, c.z));
            present.set(i);
            match file.read(c).ok().flatten().and_then(|r| inhabited(&r)) {
                Some(t) if t < min_ticks => {}
                Some(_) => busy.set(i),
                None => {
                    busy.set(i);
                    unreadable += 1;
                }
            }
        }
        Ok(Scan::Read {
            present,
            busy,
            unreadable,
            header,
        })
    })();
    result.unwrap_or_else(|e| Scan::Failed(e.to_string()))
}

/// A chunk's `InhabitedTime`, or None when it has none or can't be read.
fn inhabited(record: &ChunkRecord) -> Option<i64> {
    let mut bytes = Vec::new();
    let whole = record
        .compression
        .decode_into(&record.compressed, PREFIX, &mut bytes)
        .is_ok();
    match skim(&bytes, "InhabitedTime") {
        Ok(found) => found,
        Err(Skim::Short) if !whole => {
            let bytes = record
                .compression
                .decode(&record.compressed, Limits::saved().max_bytes)
                .ok()?;
            skim(&bytes, "InhabitedTime").ok().flatten()
        }
        Err(_) => None,
    }
}

#[derive(Debug, PartialEq)]
enum Skim {
    Short,
    Bad,
}

/// A long tag at the top of an NBT root, read without building the tree.
fn skim(bytes: &[u8], key: &str) -> Result<Option<i64>, Skim> {
    let mut c = Cursor { bytes, at: 0 };
    if c.take(1)?[0] != 10 {
        return Err(Skim::Bad);
    }
    let n = c.short()?;
    c.take(n)?;
    loop {
        let id = c.take(1)?[0];
        if id == 0 {
            return Ok(None);
        }
        let n = c.short()?;
        let name = c.take(n)?;
        if id == 4 && name == key.as_bytes() {
            return Ok(Some(i64::from_be_bytes(c.take(8)?.try_into().unwrap())));
        }
        c.skip(id, 0)?;
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Skim> {
        let end = self.at.checked_add(n).ok_or(Skim::Bad)?;
        let out = self.bytes.get(self.at..end).ok_or(Skim::Short)?;
        self.at = end;
        Ok(out)
    }
    fn short(&mut self) -> Result<usize, Skim> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as usize)
    }
    fn count(&mut self, width: usize) -> Result<usize, Skim> {
        let n = i32::from_be_bytes(self.take(4)?.try_into().unwrap());
        usize::try_from(n)
            .map_err(|_| Skim::Bad)?
            .checked_mul(width)
            .ok_or(Skim::Bad)
    }
    /// Skips a payload of type `id`.
    fn skip(&mut self, id: u8, depth: u32) -> Result<(), Skim> {
        if depth > 512 {
            return Err(Skim::Bad);
        }
        match id {
            1 => drop(self.take(1)?),
            2 => drop(self.take(2)?),
            3 | 5 => drop(self.take(4)?),
            4 | 6 => drop(self.take(8)?),
            7 => drop(self.count(1).and_then(|n| self.take(n))?),
            11 => drop(self.count(4).and_then(|n| self.take(n))?),
            12 => drop(self.count(8).and_then(|n| self.take(n))?),
            8 => drop(self.short().and_then(|n| self.take(n))?),
            9 => {
                let element = self.take(1)?[0];
                let n = self.count(1)?;
                for _ in 0..n {
                    self.skip(element, depth + 1)?;
                }
            }
            10 => loop {
                let id = self.take(1)?[0];
                if id == 0 {
                    break;
                }
                let n = self.short()?;
                self.take(n)?;
                self.skip(id, depth + 1)?;
            },
            _ => return Err(Skim::Bad),
        }
        Ok(())
    }
}

/// One dimension's folder and its scanned regions.
struct Dimension {
    id: String,
    folder: PathBuf,
    center: Chunk,
    regions: BTreeMap<Chunk, Scan>,
}

#[derive(Default, Clone)]
struct Tally {
    chunks: u64,
    pruned: u64,
    listed: u64,
    spawn: u64,
    busy: u64,
    ring: u64,
    unreadable: u64,
    files: u64,
    emptied: u64,
    skipped: u64,
    /// Bytes freed per kind (region, entities, poi).
    freed: [u64; 3],
    /// Bytes of every file of each kind before.
    before: [u64; 3],
}

/// The world's spawn chunk from level.dat (`spawn.pos` in 26.3, `SpawnX`/`SpawnZ` before), else 0 0.
fn spawn_chunk(world: &Path) -> Chunk {
    let read = || -> Option<Chunk> {
        let bytes = fs::read(world.join("level.dat")).ok()?;
        let bytes = horizon_world::compression::Compression::Gzip
            .decode(&bytes, Limits::saved().max_bytes)
            .ok()?;
        let (root, _) = nbt::read_named(&bytes, Limits::saved()).ok()?;
        let data = root.tag.get("Data")?;
        let int = |t: Option<&Tag>| match t {
            Some(Tag::Int(v)) => Some(*v),
            _ => None,
        };
        let (x, z) = match data.get("spawn").and_then(|s| s.get("pos")) {
            Some(Tag::IntArray(p)) if p.len() == 3 => (p[0], p[2]),
            _ => (int(data.get("SpawnX"))?, int(data.get("SpawnZ"))?),
        };
        Some((x >> 4, z >> 4))
    };
    read().unwrap_or((0, 0))
}

/// The chunks `keep_files` name, by dimension. Err when a file exists but can't be read.
fn listed(
    world: &Path,
    files: &[String],
    notes: &mut Vec<String>,
) -> Result<HashSet<(String, i32, i32)>, String> {
    let mut out = HashSet::new();
    for name in files {
        let path = world.join(name);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                notes.push(format!("{name}: not there, nothing kept from it"));
                continue;
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let before = out.len();
        collect(&value, &mut out);
        notes.push(format!("{name}: {} chunks kept", out.len() - before));
    }
    Ok(out)
}

fn collect(value: &serde_json::Value, out: &mut HashSet<(String, i32, i32)>) {
    let named = |s: &str, out: &mut HashSet<_>| {
        let mut w = s.split(' ');
        if let (Some(d), Some(Ok(x)), Some(Ok(z)), None) = (
            w.next(),
            w.next().map(str::parse),
            w.next().map(str::parse),
            w.next(),
        ) && d.contains(':')
        {
            out.insert((d.to_owned(), x, z));
        }
    };
    match value {
        serde_json::Value::String(s) => named(s, out),
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect(v, out)),
        serde_json::Value::Object(map) => {
            if let (Some(d), Some(x), Some(z)) = (
                map.get("dimension").and_then(|v| v.as_str()),
                map.get("x").and_then(|v| v.as_f64()),
                map.get("z").and_then(|v| v.as_f64()),
            ) {
                out.insert((
                    d.to_owned(),
                    (x.floor() as i32) >> 4,
                    (z.floor() as i32) >> 4,
                ));
            }
            for (k, v) in map {
                named(k, out);
                collect(v, out);
            }
        }
        _ => {}
    }
}

/// The dimension folders of a world: `dimensions/<namespace>/<path>` (26.3), and the older `region/`,
/// `DIM-1/`, `DIM1/` if a world still has them.
fn dimensions(world: &Path, spawn: Chunk) -> Vec<(String, PathBuf, Chunk)> {
    let mut out = Vec::new();
    let center = |id: &str| {
        if id == "minecraft:overworld" {
            spawn
        } else {
            (0, 0)
        }
    };
    for (id, folder) in [
        ("minecraft:overworld", world.to_owned()),
        ("minecraft:the_nether", world.join("DIM-1")),
        ("minecraft:the_end", world.join("DIM1")),
    ] {
        if folder.join("region").is_dir() {
            out.push((id.to_owned(), folder, center(id)));
        }
    }
    let mut namespaces: Vec<_> = fs::read_dir(world.join("dimensions"))
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    namespaces.sort_by_key(|e| e.file_name());
    for ns in namespaces {
        let mut paths: Vec<_> = fs::read_dir(ns.path())
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        paths.sort_by_key(|e| e.file_name());
        for p in paths {
            if p.path().join("region").is_dir() {
                let id = format!(
                    "{}:{}",
                    ns.file_name().to_string_lossy(),
                    p.file_name().to_string_lossy()
                );
                let c = center(&id);
                out.push((id, p.path(), c));
            }
        }
    }
    out
}

fn region_files(folder: &Path) -> Vec<(Chunk, PathBuf)> {
    let mut out: Vec<_> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let at = position_from_path(&p).ok()?;
            p.is_file().then_some(((at.x, at.z), p))
        })
        .collect();
    out.sort();
    out
}

/// `f` over `items` on `threads` threads, results in order.
fn parallel<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let mut out: Vec<(usize, R)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads.max(1))
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        mine.push((i, f(item)));
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

/// Rewrites a region file without the chunks in `drop`, compacted; deletes it if nothing is left.
/// The new file replaces the old one in one rename, so a stop halfway leaves the old file whole.
fn rewrite(path: &Path, gone: &Bits) -> io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    let staged = folder.join(STAGING).join(path.file_name().unwrap());
    match fs::remove_file(&staged) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let mut source = RegionFile::open(path)?;
    let chunks: Vec<_> = source.chunks().map(|(c, _)| c).collect();
    let mut writer = None;
    for c in chunks {
        if gone.has(slot((c.x, c.z))) {
            continue;
        }
        let record = source
            .read(c)?
            .ok_or_else(|| io::Error::other("a chunk vanished while copying"))?;
        if writer.is_none() {
            writer = Some(RegionWriter::create(&staged)?);
        }
        writer.as_mut().unwrap().write(c, &record)?;
    }
    drop(source);
    match writer {
        Some(w) => {
            w.finish()?;
            // The new file keeps the old one's permissions (a server folder readable by its owner only).
            fs::set_permissions(&staged, fs::metadata(path)?.permissions())?;
            fs::rename(&staged, path)
        }
        None => fs::remove_file(path),
    }
}

/// What a run found (and, deleting, did): the report's text.
pub struct Outcome {
    pub report: String,
    pub summary: String,
}

/// Scans the world and, with `delete`, prunes it. The game must not be running when `delete` is set.
pub fn run(
    world: &Path,
    settings: &Settings,
    delete: bool,
    threads: usize,
) -> Result<Outcome, String> {
    let started = std::time::Instant::now();
    let mut notes = Vec::new();
    let keep = listed(world, &settings.keep_files, &mut notes)?;
    let spawn = spawn_chunk(world);
    let mut dims: Vec<Dimension> = Vec::new();
    for (id, folder, center) in dimensions(world, spawn) {
        let files = region_files(&folder.join("region"));
        let scans = parallel(&files, threads, |(_, p)| {
            scan_region(p, settings.min_inhabited_ticks)
        });
        let regions = files.into_iter().map(|(at, _)| at).zip(scans).collect();
        dims.push(Dimension {
            id,
            folder,
            center,
            regions,
        });
    }
    let (r, ring) = (settings.spawn_radius as i32, settings.ring as i32);
    let mut tallies = Vec::new();
    let mut errors = Vec::new();
    for dim in &dims {
        let near_spawn =
            |c: Chunk| (c.0 - dim.center.0).abs() <= r && (c.1 - dim.center.1).abs() <= r;
        let mine: HashSet<Chunk> = keep
            .iter()
            .filter(|(d, _, _)| *d == dim.id)
            .map(|&(_, x, z)| (x, z))
            .collect();
        let is_listed = |c: Chunk| mine.contains(&c);
        let kept = |c: Chunk| {
            near_spawn(c)
                || is_listed(c)
                || match dim.regions.get(&(c.0 >> 5, c.1 >> 5)) {
                    Some(Scan::Read { busy, .. }) => busy.has(slot(c)),
                    Some(Scan::Failed(_)) => true,
                    None => false,
                }
        };
        let mut t = Tally::default();
        let mut work: Vec<(PathBuf, Bits)> = Vec::new();
        for (kind, k) in KINDS.iter().zip(0..) {
            t.before[k] = region_files(&dim.folder.join(kind))
                .iter()
                .filter_map(|(_, p)| fs::metadata(p).ok())
                .map(|m| m.len())
                .sum();
        }
        for (&(rx, rz), scan) in &dim.regions {
            t.files += 1;
            let (present, busy, unreadable, head) = match scan {
                Scan::Read {
                    present,
                    busy,
                    unreadable,
                    header,
                } => (present, busy, *unreadable, header),
                Scan::Failed(e) => {
                    errors.push(format!("{} r.{rx}.{rz}.mca: {e} (left as it is)", dim.id));
                    t.skipped += 1;
                    continue;
                }
            };
            t.unreadable += u64::from(unreadable);
            let mut gone = Bits::default();
            for i in (0..1024).filter(|&i| present.has(i)) {
                let c = (rx * 32 + (i % 32) as i32, rz * 32 + (i / 32) as i32);
                t.chunks += 1;
                if is_listed(c) {
                    t.listed += 1;
                } else if near_spawn(c) {
                    t.spawn += 1;
                } else if busy.has(i) {
                    t.busy += 1;
                } else if (-ring..=ring)
                    .any(|dx| (-ring..=ring).any(|dz| kept((c.0 + dx, c.1 + dz))))
                {
                    t.ring += 1;
                } else {
                    gone.set(i);
                }
            }
            if gone.count() == 0 {
                continue;
            }
            let name = format!("r.{rx}.{rz}.mca");
            let others: io::Result<Vec<_>> = KINDS[1..]
                .iter()
                .map(|kind| dim.folder.join(kind).join(&name))
                .filter(|p| p.is_file())
                .map(|p| header(&p).map(|h| (p, h)))
                .collect();
            let others = match others {
                Ok(o) => o,
                Err(e) => {
                    errors.push(format!(
                        "{} {name}'s entities or poi: {e} (left as it is)",
                        dim.id
                    ));
                    t.skipped += 1;
                    continue;
                }
            };
            if head.oversized || others.iter().any(|(_, h)| h.oversized) {
                t.skipped += 1;
                continue;
            }
            t.pruned += u64::from(gone.count());
            let new = compacted_len(head, &gone);
            t.freed[0] += head.len.saturating_sub(new);
            t.emptied += u64::from(new == 0);
            for (p, h) in others {
                let k = 1 + usize::from(p.parent().is_some_and(|f| f.ends_with("poi")));
                t.freed[k] += h.len.saturating_sub(compacted_len(&h, &gone));
                work.push((p, gone));
            }
            // Entities and POI first: a chunk generated again must never find the old one's entities.
            work.push((dim.folder.join("region").join(&name), gone));
        }
        if delete {
            for kind in KINDS {
                let _ = fs::create_dir_all(dim.folder.join(kind).join(STAGING));
            }
            let failed = parallel(&work, threads, |(p, gone)| {
                rewrite(p, gone)
                    .err()
                    .map(|e| format!("{}: {e}", p.display()))
            });
            errors.extend(failed.into_iter().flatten());
            for kind in KINDS {
                let _ = fs::remove_dir_all(dim.folder.join(kind).join(STAGING));
            }
        }
        tallies.push((dim.id.clone(), t));
    }
    let seconds = started.elapsed().as_secs();
    Ok(report(
        settings, delete, spawn, &tallies, &notes, &errors, seconds,
    ))
}

fn mib(b: u64) -> String {
    format!("{:.1}", b as f64 / 1_048_576.0)
}

fn report(
    settings: &Settings,
    delete: bool,
    spawn: Chunk,
    tallies: &[(String, Tally)],
    notes: &[String],
    errors: &[String],
    seconds: u64,
) -> Outcome {
    let mut text = String::new();
    let verb = if delete {
        "Deleted"
    } else {
        "Would delete (dry run)"
    };
    let _ = writeln!(
        text,
        "Chunk prune, {}: chunks with InhabitedTime < {} ticks, except within {} chunks of the spawn \
         (chunk {} {} in the overworld, 0 0 elsewhere), the listed chunks and a ring of {} around every kept chunk.",
        if delete { "deleted" } else { "dry run" },
        settings.min_inhabited_ticks,
        settings.spawn_radius,
        spawn.0,
        spawn.1,
        settings.ring
    );
    for n in notes {
        let _ = writeln!(text, "  keep_files {n}");
    }
    let _ = writeln!(
        text,
        "\n{:<24} {:>9} {:>9} {:>8} {:>7} {:>9} {:>7} {:>6} {:>7} {:>7} {:>7} {:>11} {:>11} {:>11}",
        "dimension",
        "chunks",
        "pruned",
        "listed",
        "spawn",
        "inhabited",
        "ring",
        "unread",
        "files",
        "emptied",
        "skipped",
        "region MiB",
        "entity MiB",
        "poi MiB"
    );
    let mut total = Tally::default();
    for (id, t) in tallies {
        let _ = writeln!(
            text,
            "{:<24} {:>9} {:>9} {:>8} {:>7} {:>9} {:>7} {:>6} {:>7} {:>7} {:>7} {:>11} {:>11} {:>11}",
            id,
            t.chunks,
            t.pruned,
            t.listed,
            t.spawn,
            t.busy,
            t.ring,
            t.unreadable,
            t.files,
            t.emptied,
            t.skipped,
            format!("-{} of {}", mib(t.freed[0]), mib(t.before[0])),
            format!("-{}", mib(t.freed[1])),
            format!("-{}", mib(t.freed[2])),
        );
        for (a, b) in [
            (&mut total.chunks, t.chunks),
            (&mut total.pruned, t.pruned),
            (&mut total.emptied, t.emptied),
            (&mut total.skipped, t.skipped),
        ] {
            *a += b;
        }
        for k in 0..3 {
            total.freed[k] += t.freed[k];
            total.before[k] += t.before[k];
        }
    }
    let freed: u64 = total.freed.iter().sum();
    let before: u64 = total.before.iter().sum();
    let summary = format!(
        "{verb} {} of {} chunks; {} MiB of {} MiB of region, entities and poi files freed, {} files emptied, {} left as they were, in {} s{}",
        total.pruned,
        total.chunks,
        mib(freed),
        mib(before),
        total.emptied,
        total.skipped,
        seconds,
        if errors.is_empty() {
            String::new()
        } else {
            format!(", {} errors", errors.len())
        }
    );
    let _ = writeln!(text, "\n{summary}");
    let _ = writeln!(
        text,
        "listed/spawn/inhabited/ring: kept for that reason (first that applies); unread: kept, can't be read; \
         skipped: files left whole (a chunk in its own .mcc file, or a file that can't be read)."
    );
    for e in errors {
        let _ = writeln!(text, "ERROR {e}");
    }
    Outcome {
        report: text,
        summary,
    }
}

/// Writes the report to `logs/prune-chunks-<time>.txt`; returns its path.
pub fn write_report(dir: &Path, outcome: &Outcome) -> io::Result<PathBuf> {
    let logs = dir.join("logs");
    fs::create_dir_all(&logs)?;
    let path = logs.join(format!(
        "prune-chunks-{}.txt",
        chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ")
    ));
    fs::write(&path, &outcome.report)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::position::ChunkPos;
    use horizon_world::{compression::Compression, nbt::NamedTag};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("horizon-prune-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A chunk as vanilla saves it, with `filler` bytes of sections before or after InhabitedTime.
    fn chunk(c: Chunk, ticks: Option<i64>, filler: usize, late: bool) -> ChunkRecord {
        let mut entries: Vec<(&str, Tag)> = vec![
            ("DataVersion", Tag::Int(4700)),
            ("xPos", Tag::Int(c.0)),
            ("zPos", Tag::Int(c.1)),
            ("Status", Tag::String("minecraft:full".into())),
        ];
        let sections = (
            "sections",
            Tag::List(vec![Tag::compound([(
                "data",
                // Values that don't compress away, so a big chunk takes many sectors.
                Tag::LongArray(
                    (0..filler as i64 / 8)
                        .map(|i| {
                            (i ^ i64::from(c.0)).wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64)
                        })
                        .collect(),
                ),
            )])]),
        );
        if late {
            entries.push(sections.clone());
        }
        if let Some(t) = ticks {
            entries.push(("InhabitedTime", Tag::Long(t)));
        }
        if !late {
            entries.push(sections);
        }
        let root = NamedTag {
            name: Default::default(),
            tag: Tag::compound(entries),
        };
        ChunkRecord::from_nbt(&root, Compression::Zlib, 1_700_000_000, Limits::saved()).unwrap()
    }

    fn write_region(path: &Path, chunks: &[(Chunk, ChunkRecord)]) {
        let mut w = RegionWriter::create(path).unwrap();
        for (c, r) in chunks {
            w.write(ChunkPos::new(c.0, c.1), r).unwrap();
        }
        w.finish().unwrap();
    }

    /// The chunks a region file holds, read back by horizon-world's port of vanilla's RegionFile.
    fn chunks_in(path: &Path) -> Vec<Chunk> {
        let mut f = RegionFile::open(path).unwrap();
        let mut out: Vec<_> = f.chunks().map(|(c, _)| (c.x, c.z)).collect();
        out.sort();
        for c in &out {
            f.read(ChunkPos::new(c.0, c.1))
                .unwrap()
                .unwrap()
                .nbt(Limits::saved())
                .unwrap();
        }
        out
    }

    fn settings(radius: u32) -> Settings {
        Settings {
            spawn_radius: radius,
            ..Settings::default()
        }
    }

    #[test]
    fn reads_inhabited_time_wherever_it_is() {
        assert_eq!(inhabited(&chunk((1, 2), Some(1234), 64, false)), Some(1234));
        // Past the decoded prefix: the whole chunk is decoded.
        assert_eq!(inhabited(&chunk((1, 2), Some(77), 200_000, true)), Some(77));
        assert_eq!(inhabited(&chunk((1, 2), None, 64, false)), None);
        let broken = ChunkRecord {
            compression: Compression::Zlib,
            compressed: vec![1, 2, 3],
            timestamp: 0,
        };
        assert_eq!(inhabited(&broken), None);
    }

    /// Overworld region r.2.0 (chunks 64..95, 0..31), far from the spawn at 0 0.
    fn world(name: &str) -> (PathBuf, PathBuf) {
        let dir = temp(name);
        let world = dir.join("world");
        let ow = world.join("dimensions/minecraft/overworld");
        let mut chunks = vec![
            // A build: kept, and its ring of 8 with it.
            ((70, 5), chunk((70, 5), Some(50_000), 64, false)),
            ((80, 5), chunk((80, 5), Some(0), 64, false)),
            ((85, 5), chunk((85, 5), Some(1199), 64, false)),
            ((90, 5), chunk((90, 5), Some(1200), 64, false)),
            // Two apart from the 1200 chunk: pruned, its neighbour (91) is ring.
            ((92, 5), chunk((92, 5), Some(3), 64, false)),
            ((91, 5), chunk((91, 5), Some(3), 64, false)),
            // A big chunk, after which compaction must still find every kept chunk.
            ((64, 25), chunk((64, 25), Some(0), 50_000, false)),
            ((65, 20), chunk((65, 20), Some(9_000), 64, false)),
        ];
        for dx in -1..=1 {
            for dz in -1..=1 {
                if (dx, dz) != (0, 0) {
                    let c = (70 + dx, 5 + dz);
                    chunks.push((c, chunk(c, Some(10), 64, false)));
                }
            }
        }
        write_region(&ow.join("region/r.2.0.mca"), &chunks);
        // The spawn region: never visited, kept by the spawn radius.
        write_region(
            &ow.join("region/r.0.0.mca"),
            &[
                ((0, 0), chunk((0, 0), Some(0), 64, false)),
                ((3, 3), chunk((3, 3), Some(0), 64, false)),
            ],
        );
        // A region nobody stayed in: emptied, so deleted.
        write_region(
            &ow.join("region/r.5.5.mca"),
            &[((170, 170), chunk((170, 170), Some(5), 64, false))],
        );
        // The same chunks' entities and POI.
        let entities = |c: Chunk| chunk(c, Some(0), 16, false);
        write_region(
            &ow.join("entities/r.2.0.mca"),
            &[((70, 5), entities((70, 5))), ((80, 5), entities((80, 5)))],
        );
        write_region(&ow.join("poi/r.2.0.mca"), &[((85, 5), entities((85, 5)))]);
        write_region(
            &ow.join("entities/r.5.5.mca"),
            &[((170, 170), entities((170, 170)))],
        );
        (dir, ow)
    }

    #[test]
    fn prunes_barely_visited_chunks_and_keeps_the_rest_readable() {
        let (dir, ow) = world("prune");
        let world = dir.join("world");
        let before = fs::read(ow.join("region/r.2.0.mca")).unwrap();

        let dry = run(&world, &settings(4), false, 2).unwrap();
        assert_eq!(
            fs::read(ow.join("region/r.2.0.mca")).unwrap(),
            before,
            "a dry run writes nothing"
        );
        assert!(ow.join("region/r.5.5.mca").is_file());
        assert!(
            dry.summary
                .contains("Would delete (dry run) 5 of 19 chunks"),
            "{}",
            dry.report
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                ow.join("region/r.2.0.mca"),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        let done = run(&world, &settings(4), true, 2).unwrap();
        assert!(
            done.summary.starts_with("Deleted 5 of 19 chunks"),
            "{}",
            done.report
        );
        let mut kept: Vec<Chunk> = vec![(65, 20), (90, 5), (91, 5)];
        for dx in -1..=1 {
            for dz in -1..=1 {
                kept.push((70 + dx, 5 + dz));
            }
        }
        kept.sort();
        assert_eq!(chunks_in(&ow.join("region/r.2.0.mca")), kept);
        assert_eq!(
            chunks_in(&ow.join("region/r.0.0.mca")),
            vec![(0, 0), (3, 3)]
        );
        assert!(
            !ow.join("region/r.5.5.mca").exists(),
            "an emptied region file is deleted"
        );
        assert!(!ow.join("entities/r.5.5.mca").exists());
        assert_eq!(chunks_in(&ow.join("entities/r.2.0.mca")), vec![(70, 5)]);
        assert!(!ow.join("poi/r.2.0.mca").exists());
        assert!(!ow.join("region").join(STAGING).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(ow.join("region/r.2.0.mca"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "a rewritten file keeps its permissions"
            );
        }

        // Compacted: 2 header sectors plus one per kept chunk (each fits one), and the kept bytes as they were.
        let after = fs::read(ow.join("region/r.2.0.mca")).unwrap();
        assert_eq!(after.len() as u64, (2 + kept.len() as u64) * SECTOR);
        assert!(after.len() < before.len());
        let mut f = RegionFile::open(ow.join("region/r.2.0.mca")).unwrap();
        let r = f.read(ChunkPos::new(70, 5)).unwrap().unwrap();
        assert_eq!(inhabited(&r), Some(50_000));
        assert_eq!(r.timestamp, 1_700_000_000);

        // A second run finds nothing more to do.
        let again = run(&world, &settings(4), true, 2).unwrap();
        assert!(
            again.summary.starts_with("Deleted 0 of 14 chunks"),
            "{}",
            again.report
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn claims_homes_and_the_spawn_radius_keep_chunks() {
        let (dir, ow) = world("claims");
        let world = dir.join("world");
        fs::create_dir_all(world.join("horizon-smp")).unwrap();
        fs::write(
            world.join("horizon-smp/claims.json"),
            r#"{"chunks": {"minecraft:overworld 80 5": "8a0c0b8e-0000-0000-0000-000000000001",
                           "minecraft:the_nether 85 5": "8a0c0b8e-0000-0000-0000-000000000001"}, "trust": {}}"#,
        )
        .unwrap();
        // A home at x 2720.5, z 2730: chunk 170 170.
        fs::write(
            world.join("horizon-smp/places.json"),
            r#"{"homes": {"p": {"base": {"dimension": "minecraft:overworld", "x": 2720.5, "y": 70.0, "z": 2730.0}}}}"#,
        )
        .unwrap();
        let mut s = settings(4);
        run(&world, &s, true, 1).unwrap();
        let left = chunks_in(&ow.join("region/r.2.0.mca"));
        assert!(left.contains(&(80, 5)), "a claimed chunk stays");
        assert!(
            !left.contains(&(85, 5)),
            "a claim in another dimension keeps nothing here"
        );
        assert!(
            ow.join("region/r.5.5.mca").is_file(),
            "a home's chunk stays"
        );
        assert_eq!(
            chunks_in(&ow.join("entities/r.2.0.mca")),
            vec![(70, 5), (80, 5)]
        );

        // The spawn radius reaches region r.2.0 (64 chunks out): everything there stays.
        s.spawn_radius = 100;
        let all = run(&world, &s, false, 1).unwrap();
        assert!(all.summary.contains(" 0 of "), "{}", all.report);

        // A claims file that can't be read stops the prune before anything goes.
        fs::write(world.join("horizon-smp/claims.json"), "{not json").unwrap();
        assert!(run(&world, &settings(4), true, 1).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn server_toml_turns_it_on_and_the_supervisor_turns_it_off() {
        let dir = temp("toml");
        let text = format!(
            "{}\n[prune_chunks]\nmode = \"delete\"\nring = 2\n",
            crate::config::TEMPLATE
        );
        fs::write(dir.join("server.toml"), text).unwrap();
        let config = crate::config::Config::load(&dir).unwrap();
        assert_eq!(
            (config.prune_chunks.mode, config.prune_chunks.ring),
            (Mode::Delete, 2)
        );
        crate::config::update(
            &dir,
            "prune_chunks",
            "mode",
            toml::Value::String("off".into()),
        )
        .unwrap();
        let config = crate::config::Config::load(&dir).unwrap();
        assert_eq!(
            (config.prune_chunks.mode, config.prune_chunks.ring),
            (Mode::Off, 2)
        );
        assert!(
            fs::read_to_string(dir.join("server.toml"))
                .unwrap()
                .contains("[discord]")
        );
        fs::write(
            dir.join("server.toml"),
            "[prune_chunks]\nkeep_files = [\"../ops.json\"]\n",
        )
        .unwrap();
        assert!(
            crate::config::Config::load(&dir).is_err(),
            "keep_files stay inside the world"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn spawn_comes_from_level_dat() {
        let dir = temp("spawn");
        let root = NamedTag {
            name: Default::default(),
            tag: Tag::compound([(
                "Data",
                Tag::compound([(
                    "spawn",
                    Tag::compound([("pos", Tag::IntArray(vec![1300, 64, -40]))]),
                )]),
            )]),
        };
        let bytes = nbt::write_named(&root, Limits::saved()).unwrap();
        fs::write(
            dir.join("level.dat"),
            Compression::Gzip.encode(&bytes).unwrap(),
        )
        .unwrap();
        assert_eq!(spawn_chunk(&dir), (81, -3));
        assert_eq!(spawn_chunk(&dir.join("missing")), (0, 0));
        let _ = fs::remove_dir_all(dir);
    }
}
