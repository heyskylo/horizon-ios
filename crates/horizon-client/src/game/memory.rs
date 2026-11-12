//! `horizon-ctl memory`: what holds the game's memory, by category. The
//! process's resident set and the allocator's committed bytes say how much;
//! the census of the columns, the server's entities, players and chunk send
//! caches, the renderer's bookkeeping and the GPU allocator's resources by
//! label say where it goes. `unattributed_mb` is what the named parts leave.
use super::Game;
use horizon_client::chunk::ColumnCensus;
use horizon_client::fast_hash::FxHashSet;
use horizon_client::lan::hub::Hub;
use horizon_client::world::World;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::mem::size_of;
use std::sync::Arc;

const MB: f64 = 1048576.0;
/// Entity types and players listed apart; the rest only count in the totals.
const TOP: usize = 30;
const TOP_PLAYERS: usize = 50;

fn mb(bytes: impl TryInto<u64>) -> f64 {
    let bytes: u64 = bytes.try_into().unwrap_or(0);
    (bytes as f64 / MB * 10.0).round() / 10.0
}

fn mb_signed(bytes: i64) -> f64 {
    (bytes as f64 / MB * 10.0).round() / 10.0
}

fn vec_bytes<T>(v: &Vec<T>) -> usize {
    v.capacity() * size_of::<T>()
}

fn string_bytes(s: &Option<String>) -> usize {
    s.as_ref().map_or(0, String::capacity)
}

/// The columns of `worlds`, each section counted once however many columns
/// (or worlds) share it.
fn census<'a>(worlds: impl IntoIterator<Item = &'a World>) -> ColumnCensus {
    let mut census = ColumnCensus::default();
    let mut seen = FxHashSet::default();
    for world in worlds {
        for chunk in world.chunks.values() {
            chunk.census(&mut census, &mut seen);
        }
    }
    census
}

#[derive(Serialize)]
struct Columns {
    total_mb: f64,
    columns: usize,
    sections: usize,
    unique_sections: usize,
    uniform_block_sections: usize,
    light_arrays: usize,
    blocks_mb: f64,
    biomes_mb: f64,
    light_mb: f64,
    heightmaps_mb: f64,
    section_records_mb: f64,
    column_records_mb: f64,
    #[serde(skip)]
    bytes: usize,
}

impl Columns {
    fn new(c: &ColumnCensus) -> Self {
        Self {
            total_mb: mb(c.total_bytes()),
            columns: c.columns,
            sections: c.sections,
            unique_sections: c.unique_sections,
            uniform_block_sections: c.uniform_block_sections,
            light_arrays: c.light_arrays,
            blocks_mb: mb(c.block_bytes),
            biomes_mb: mb(c.biome_bytes),
            light_mb: mb(c.light_bytes),
            heightmaps_mb: mb(c.heightmap_bytes),
            section_records_mb: mb(c.section_record_bytes),
            column_records_mb: mb(c.column_bytes),
            bytes: c.total_bytes(),
        }
    }
}

/// The sections of `worlds` that are also in `others` (by address), and
/// their heap bytes: what one copy of them costs.
fn shared<'a>(
    worlds: impl IntoIterator<Item = &'a World>,
    others: &FxHashSet<usize>,
) -> (usize, usize) {
    let mut seen = FxHashSet::default();
    let (mut count, mut bytes) = (0, 0);
    for world in worlds {
        for chunk in world.chunks.values() {
            for section in &chunk.sections {
                let at = Arc::as_ptr(section) as usize;
                if others.contains(&at) && seen.insert(at) {
                    count += 1;
                    bytes += section.heap_bytes();
                }
            }
        }
    }
    (count, bytes)
}

#[derive(Serialize)]
struct SharedSections {
    sections: usize,
    mb: f64,
    #[serde(skip)]
    bytes: usize,
}

#[derive(Serialize)]
struct Generation {
    protos: usize,
    protos_mb: f64,
    proto_blocks_mb: f64,
    proto_section_lists_mb: f64,
    proto_heightmaps_mb: f64,
    proto_other_terrain_mb: f64,
    proto_biomes_mb: f64,
    proto_records_mb: f64,
    proto_sections_by_bits: [usize; 16],
    light_views: usize,
    light_views_mb: f64,
    /// Name, entries and megabytes where the cache can say.
    caches: Vec<(&'static str, usize, Option<f64>)>,
    stages: Vec<(&'static str, usize)>,
    #[serde(skip)]
    bytes: usize,
}

impl Generation {
    fn of(streaming: &horizon_client::streaming::Streaming) -> Self {
        let c = streaming.proto_census();
        let [blocks, lists, heightmaps, other, biomes, records] = c.bytes;
        let protos: usize = c.bytes.iter().sum();
        let cached: usize = c.caches.iter().filter_map(|(_, _, bytes)| *bytes).sum();
        Self {
            protos: c.protos,
            protos_mb: mb(protos),
            proto_blocks_mb: mb(blocks),
            proto_section_lists_mb: mb(lists),
            proto_heightmaps_mb: mb(heightmaps),
            proto_other_terrain_mb: mb(other),
            proto_biomes_mb: mb(biomes),
            proto_records_mb: mb(records),
            proto_sections_by_bits: c.sections_by_bits,
            light_views: c.light_views,
            light_views_mb: mb(c.light_view_bytes),
            caches: c
                .caches
                .iter()
                .map(|(name, n, bytes)| (*name, *n, bytes.map(mb)))
                .collect(),
            stages: streaming.stages().to_vec(),
            bytes: protos + c.light_view_bytes + cached,
        }
    }
}

/// Entities of every level by registry id: how many, and the bytes of their records.
#[derive(Serialize)]
struct Entities {
    count: usize,
    total_mb: f64,
    /// The `TOP` types holding the most bytes: id, count, megabytes.
    by_type: Vec<(String, usize, f64)>,
    #[serde(skip)]
    bytes: usize,
}

/// The connected players: queues and what each is owed or holds, the biggest first.
#[derive(Default, Serialize)]
struct Players {
    count: usize,
    total_mb: f64,
    /// Name, packets and megabytes waiting to be sent.
    send_queue: Vec<(String, usize, f64)>,
    chunks_waiting: Vec<(String, usize)>,
    chunks_sent: Vec<(String, usize)>,
    tracked_entities: Vec<(String, usize)>,
    #[serde(skip)]
    bytes: usize,
}

#[derive(Default, Serialize)]
struct Cached {
    entries: usize,
    size_mb: f64,
}

/// What the hub keeps to send chunks: encoded sections, heightmaps and compressed frames.
#[derive(Default, Serialize)]
struct ChunkSend {
    total_mb: f64,
    sections: Cached,
    heightmaps: Cached,
    frames: Cached,
    #[serde(skip)]
    bytes: usize,
}

#[derive(Serialize)]
struct Queue {
    len: usize,
    size_mb: f64,
}

#[derive(Serialize)]
struct Process {
    resident_mb: Option<f64>,
    allocator_commit_mb: f64,
    allocator_peak_commit_mb: f64,
}

#[derive(Serialize)]
struct Allocator {
    #[serde(skip_serializing_if = "Option::is_none")]
    live_heap_mb: Option<f64>,
    commit_mb: f64,
    peak_commit_mb: f64,
    /// Committed minus the pages in use.
    #[serde(skip_serializing_if = "Option::is_none")]
    overhead_mb: Option<f64>,
}

#[derive(Serialize)]
struct FarTiles {
    active: usize,
    cached: usize,
    kept: usize,
    kept_mb: f64,
}

#[derive(Serialize)]
struct Gpu {
    allocated_mb: f64,
    reserved_mb: f64,
    by_label_mb: Vec<(String, f64)>,
}

#[derive(Serialize)]
struct Renderer {
    section_arenas_mb: f64,
    section_vertices_mb: f64,
    cpu: BTreeMap<String, f64>,
    gpu: Option<Gpu>,
}

/// A serving game's own parts: the columns it holds, its generation, entities, players, chunk send caches and queues.
struct ServerSide {
    columns: Columns,
    addresses: FxHashSet<usize>,
    generation: Generation,
    entities: Entities,
    players: Players,
    chunk_send: ChunkSend,
    light_queue: Queue,
    generation_queue: Queue,
    queue_bytes: usize,
}

impl ServerSide {
    /// The bytes of every part this side names.
    fn bytes(&self) -> usize {
        self.columns.bytes
            + self.generation.bytes
            + self.entities.bytes
            + self.players.bytes
            + self.chunk_send.bytes
            + self.queue_bytes
    }
}

/// How often a running capture records what holds the memory.
pub(super) const MEMORY_EVERY_S: f64 = 10.0;

/// `horizon-ctl memory`'s report. Every `*_mb` is rounded to a tenth; the parts' bytes are summed unrounded.
#[derive(Serialize)]
pub(super) struct MemoryReport {
    process: Process,
    allocator: Allocator,
    render_distance: i32,
    /// The window's columns; a dedicated server has no window and names them `columns`.
    #[serde(skip_serializing_if = "Option::is_none")]
    client_columns: Option<Columns>,
    #[serde(skip_serializing_if = "Option::is_none")]
    columns: Option<Columns>,
    server_columns: Option<Columns>,
    shared_with_server: Option<SharedSections>,
    server_generation: Option<Generation>,
    entities: Option<Entities>,
    players: Option<Players>,
    chunk_send: Option<ChunkSend>,
    light_queue: Option<Queue>,
    generation_queue: Option<Queue>,
    far_tiles: Option<FarTiles>,
    renderer: Option<Renderer>,
    /// A build with the `alloc-census` feature: the live heap by the
    /// startup step, span or pool job that allocated it, else its thread.
    heap_by_tag_mb: Option<Vec<(String, f64)>>,
    /// The columns, generation, entities, players, chunk send caches, queues, renderer CPU data and far tiles together.
    named_mb: f64,
    /// Resident memory minus `named_mb`: what no census reaches (it can be negative where the parts overlap).
    unattributed_mb: Option<f64>,
}

/// `resident - named` in megabytes, None without a resident size.
fn unattributed(resident: Option<u64>, named: usize) -> Option<f64> {
    resident.map(|r| mb_signed(r as i64 - named as i64))
}

/// `horizon-ctl memory`'s report as a capture's rows: `a.b` paths, `key[name]` for `[name, value]` lists.
pub(super) fn flatten_report(report: &Value) -> Vec<(String, f64)> {
    let mut rows = Vec::new();
    flatten(String::new(), report, &mut rows);
    rows
}

fn flatten(path: String, value: &Value, rows: &mut Vec<(String, f64)>) {
    match value {
        Value::Number(n) => rows.extend(n.as_f64().map(|n| (path, n))),
        Value::Object(fields) => {
            for (key, inner) in fields {
                let next = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                flatten(next, inner, rows);
            }
        }
        Value::Array(items) => {
            for item in items {
                let Some([Value::String(name), count, rest @ ..]) =
                    item.as_array().map(Vec::as_slice)
                else {
                    continue;
                };
                rows.extend(count.as_f64().map(|n| (format!("{path}[{name}]"), n)));
                if let Some(mb) = rest.first().and_then(Value::as_f64) {
                    rows.push((format!("{path}_mb[{name}]"), mb));
                }
            }
        }
        _ => {}
    }
}

/// The `limit` rows with the biggest key first, ties by name.
fn biggest<K: Copy + Ord, V>(mut rows: Vec<(String, K, V)>, limit: usize) -> Vec<(String, K, V)> {
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(limit);
    rows
}

/// The hub's players: what waits in each one's send queue and what the hub tracks for it.
fn player_census(hub: &Hub) -> Players {
    let set = |len: usize| len * (size_of::<(i32, i32)>() + 1);
    let mut rows = Vec::new();
    let mut bytes = 0usize;
    for g in &hub.guests {
        let (packets, queued) = g.sender.queued();
        bytes += queued
            + set(g.sent_chunks.capacity())
            + set(g.far_sent_chunks.capacity())
            + vec_bytes(&g.chunk_queue)
            + vec_bytes(&g.far_queue)
            + g.tracker.heap_bytes();
        rows.push((
            g.name.clone(),
            [
                queued,
                packets,
                g.chunk_queue.len() + g.far_queue.len(),
                g.sent_chunks.len() + g.far_sent_chunks.len(),
                g.tracker.len(),
            ],
        ));
    }
    let top = |by: usize| {
        let ranked: Vec<_> = rows
            .iter()
            .map(|(name, v)| (name.clone(), v[by], v[0]))
            .collect();
        biggest(ranked, TOP_PLAYERS)
    };
    Players {
        count: hub.guests.len(),
        total_mb: mb(bytes),
        send_queue: top(0)
            .into_iter()
            .map(|(name, queued, _)| {
                let packets = rows.iter().find(|r| r.0 == name).map_or(0, |r| r.1[1]);
                (name, packets, mb(queued))
            })
            .collect(),
        chunks_waiting: top(2).into_iter().map(|(n, v, _)| (n, v)).collect(),
        chunks_sent: top(3).into_iter().map(|(n, v, _)| (n, v)).collect(),
        tracked_entities: top(4).into_iter().map(|(n, v, _)| (n, v)).collect(),
        bytes,
    }
}

fn chunk_send_census(hub: &Hub) -> ChunkSend {
    let [sections, heights] = hub.encoder.cache_census();
    let frames = horizon_client::lan::conn::shared_frame_census();
    let cached = |(entries, bytes): (usize, usize)| Cached {
        entries,
        size_mb: mb(bytes),
    };
    let bytes = sections.1 + heights.1 + frames.1;
    ChunkSend {
        total_mb: mb(bytes),
        sections: cached(sections),
        heightmaps: cached(heights),
        frames: cached(frames),
        bytes,
    }
}

impl Game<'_> {
    fn all_worlds(&self) -> impl Iterator<Item = &World> {
        std::iter::once(&self.world).chain(self.levels.parked_worlds())
    }
    /// The census of this game's levels: the one in place and the parked ones.
    fn level_census(&self) -> ColumnCensus {
        census(self.all_worlds())
    }
    /// The addresses of this game's sections.
    fn section_addresses(&self) -> FxHashSet<usize> {
        self.all_worlds()
            .flat_map(|w| w.chunks.values())
            .flat_map(|c| c.sections.iter().map(|s| Arc::as_ptr(s) as usize))
            .collect()
    }

    /// The entities of every level by type; a record's bytes are its struct and the buffers its fields own.
    fn entity_census(&self) -> Entities {
        fn size<T>(_: &[T]) -> usize {
            size_of::<T>()
        }
        let record = [
            size(self.dropped.as_slice()),
            size(self.orbs.as_slice()),
            size(self.creatures.projectiles.as_slice()),
            size(self.carts.as_slice()),
            size(self.boats.as_slice()),
        ];
        let mut by_type: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut level = |mobs: &[horizon_client::mobs::Mob], loose: [usize; 5]| {
            for m in mobs.iter().filter(|m| !m.removed) {
                let bytes = size_of::<horizon_client::mobs::Mob>()
                    + size_of_val(&*m.ai)
                    + vec_bytes(&m.outbox)
                    + string_bytes(&m.custom_name)
                    + string_bytes(&m.standing_on)
                    + string_bytes(&m.skin)
                    + string_bytes(&m.cape);
                let row = by_type.entry(m.kind.id.clone()).or_default();
                row.0 += 1;
                row.1 += bytes;
            }
            let loose = super::dedicated::report::LOOSE_ENTITIES
                .into_iter()
                .zip(loose)
                .zip(record);
            for ((id, n), size) in loose.filter(|((_, n), _)| *n > 0) {
                let row = by_type.entry(id.to_owned()).or_default();
                row.0 += n;
                row.1 += n * size;
            }
        };
        level(
            &self.mobs,
            [
                self.dropped.len(),
                self.orbs.len(),
                self.creatures.projectiles.len(),
                self.carts.len(),
                self.boats.len(),
            ],
        );
        for dim in self.levels.parked_dims() {
            if let Some(mobs) = self.levels.parked_mobs(dim) {
                let loose = self.levels.parked_entity_counts(dim).unwrap_or_default();
                level(mobs, loose);
            }
        }
        let count = by_type.values().map(|r| r.0).sum();
        let bytes = by_type.values().map(|r| r.1).sum();
        let rows = by_type.into_iter().map(|(id, (n, b))| (id, b, n)).collect();
        Entities {
            count,
            total_mb: mb(bytes),
            by_type: biggest(rows, TOP)
                .into_iter()
                .map(|(id, b, n)| (id, n, mb(b)))
                .collect(),
            bytes,
        }
    }

    /// This game's own server parts: it is a dedicated server, or the single-player server thread.
    fn server_side(&self) -> ServerSide {
        let (players, chunk_send) = match &self.lan {
            Some(host) => (player_census(&host.hub), chunk_send_census(&host.hub)),
            None => (Players::default(), ChunkSend::default()),
        };
        let (mut generation, mut light) = {
            let q = self.streaming.queue_census();
            (q.generation, q.light)
        };
        for streaming in self.levels.parked_streamings() {
            let q = streaming.queue_census();
            generation = (generation.0 + q.generation.0, generation.1 + q.generation.1);
            light = (light.0 + q.light.0, light.1 + q.light.1);
        }
        ServerSide {
            columns: Columns::new(&self.level_census()),
            addresses: self.section_addresses(),
            generation: Generation::of(&self.streaming),
            entities: self.entity_census(),
            players,
            chunk_send,
            light_queue: Queue {
                len: light.0,
                size_mb: mb(light.1),
            },
            generation_queue: Queue {
                len: generation.0,
                size_mb: mb(generation.1),
            },
            queue_bytes: generation.1 + light.1,
        }
    }

    pub(super) fn memory_report(&self) -> MemoryReport {
        let (commit, peak_commit) = horizon_threads::allocator_commit();
        let live = horizon_threads::allocator_stats::live_bytes();
        let resident = horizon_threads::process_memory::resident_bytes();
        let dedicated = self.options.dedicated.is_some();
        // The single-player server thread's own levels: a second copy of the
        // columns both hold, unless they share sections.
        let server = if dedicated {
            Some(self.server_side())
        } else {
            self.integrated
                .as_ref()
                .and_then(|i| i.call(|g| g.server_side()))
        };
        let window = (!dedicated).then(|| self.level_census());
        // The window's sections the server thread holds too (lan/handoff.rs).
        let shared = server.as_ref().filter(|_| !dedicated).map(|s| {
            let (sections, bytes) = shared(self.all_worlds(), &s.addresses);
            SharedSections {
                sections,
                mb: mb(bytes),
                bytes,
            }
        });
        let renderer = self.renderer.as_ref().map(|r| {
            let [arenas, vertices] = r.section_vertex_bytes();
            let cpu = r.cpu_census();
            let cpu_bytes: usize = cpu.iter().map(|(_, v)| *v).sum();
            let gpu = r
                .gpu_memory_by_label()
                .map(|(allocated, reserved, labels)| Gpu {
                    allocated_mb: mb(allocated),
                    reserved_mb: mb(reserved),
                    by_label_mb: labels
                        .iter()
                        .take(40)
                        .map(|(label, bytes)| (label.to_string(), mb(*bytes)))
                        .collect(),
                });
            let renderer = Renderer {
                section_arenas_mb: mb(arenas),
                section_vertices_mb: mb(vertices),
                cpu: cpu
                    .into_iter()
                    .map(|(k, v)| (format!("{k}_mb"), mb(v)))
                    .collect(),
                gpu,
            };
            (renderer, cpu_bytes)
        });
        let far = self.lod.as_ref().map(|lod| {
            let ((active, cached), (kept, kept_bytes)) = (lod.stats(), lod.kept());
            let tiles = FarTiles {
                active,
                cached,
                kept,
                kept_mb: mb(kept_bytes),
            };
            (tiles, kept_bytes)
        });
        let named = window.as_ref().map_or(0, ColumnCensus::total_bytes)
            + server.as_ref().map_or(0, ServerSide::bytes)
            + renderer.as_ref().map_or(0, |r| r.1)
            + far.as_ref().map_or(0, |f| f.1);
        let named = named.saturating_sub(shared.as_ref().map_or(0, |s| s.bytes));
        let mut report = MemoryReport {
            process: Process {
                resident_mb: resident.map(mb),
                allocator_commit_mb: mb(commit),
                allocator_peak_commit_mb: mb(peak_commit),
            },
            allocator: Allocator {
                live_heap_mb: live.map(mb),
                commit_mb: mb(commit),
                peak_commit_mb: mb(peak_commit),
                overhead_mb: live.map(|live| mb(commit.saturating_sub(live))),
            },
            render_distance: self.streaming.radius,
            client_columns: window.as_ref().map(Columns::new),
            columns: None,
            server_columns: None,
            shared_with_server: shared,
            server_generation: None,
            entities: None,
            players: None,
            chunk_send: None,
            light_queue: None,
            generation_queue: None,
            far_tiles: far.map(|f| f.0),
            renderer: renderer.map(|r| r.0),
            heap_by_tag_mb: horizon_threads::HEAP_CENSUS.then(|| {
                horizon_threads::heap_census::census()
                    .into_iter()
                    .take(60)
                    .map(|(tag, bytes)| (tag.to_string(), mb(bytes.max(0) as u64)))
                    .collect()
            }),
            named_mb: mb(named),
            unattributed_mb: unattributed(resident, named),
        };
        if let Some(s) = server {
            if dedicated {
                report.columns = Some(s.columns);
            } else {
                report.server_columns = Some(s.columns);
            }
            report.server_generation = Some(s.generation);
            report.entities = Some(s.entities);
            report.players = Some(s.players);
            report.chunk_send = Some(s.chunk_send);
            report.light_queue = Some(s.light_queue);
            report.generation_queue = Some(s.generation_queue);
        }
        report
    }

    /// The report as JSON, for the control socket and the capture's rows.
    pub(super) fn memory_value(&self) -> Value {
        serde_json::to_value(self.memory_report()).unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::{biggest, flatten_report, unattributed};
    use serde_json::json;

    #[test]
    fn unattributed_is_resident_minus_the_named_bytes_and_may_be_negative() {
        let mib = 1 << 20;
        assert_eq!(
            unattributed(Some(900 * mib), 600 * mib as usize),
            Some(300.0)
        );
        assert_eq!(unattributed(Some(10 * mib), 25 * mib as usize), Some(-15.0));
        assert_eq!(unattributed(None, 5), None);
    }

    #[test]
    fn the_biggest_rows_come_first_with_ties_by_name() {
        let rows = vec![
            ("b".to_string(), 2, ()),
            ("a".to_string(), 2, ()),
            ("c".to_string(), 9, ()),
        ];
        let names: Vec<_> = biggest(rows, 2).into_iter().map(|r| r.0).collect();
        assert_eq!(names, ["c", "a"]);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_servers_report_names_its_entities_and_players_and_leaves_the_rest_unattributed() {
        use crate::game::test_pos_guest_tests::{GUEST, game};
        use horizon_client::mobs::Mob;
        let mut game = game();
        game.options.dedicated = Some(Vec::new());
        for (id, kind) in [(1, "cow"), (2, "cow"), (3, "pig")] {
            let kind = game
                .res
                .mob_types()
                .get(&format!("minecraft:{kind}"))
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(
                id,
                kind,
                horizon_core::geometry::Vec3::new(0.5, 64.0, 0.5),
                0.0,
            ));
        }
        game.lan.as_ref().unwrap().hub.guests[0]
            .sender
            .send(vec![0; 3 << 20]);
        let report = game.memory_report();
        let by_type: std::collections::BTreeMap<_, _> = report
            .entities
            .as_ref()
            .unwrap()
            .by_type
            .iter()
            .map(|(id, n, _)| (id.as_str(), *n))
            .collect();
        assert_eq!((by_type["minecraft:cow"], by_type["minecraft:pig"]), (2, 1));
        let players = report.players.as_ref().unwrap();
        assert_eq!(players.count, 1);
        assert_eq!(players.send_queue[0].0, GUEST);
        assert_eq!(players.send_queue[0].2, 3.0);
        assert!(report.client_columns.is_none() && report.columns.is_some());
        let resident = report.process.resident_mb.unwrap();
        let rest = report.unattributed_mb.unwrap();
        assert!(
            (rest - (resident - report.named_mb)).abs() <= 0.15,
            "{rest} {resident} {}",
            report.named_mb
        );
    }

    #[test]
    fn a_report_flattens_to_paths_and_named_list_entries() {
        let report = json!({
            "process": { "resident_mb": 12.5, "note": "skipped" },
            "render_distance": 16,
            "missing": null,
            "by_label_mb": [["chunks", 3.0], ["bare"]],
            "caches": [["blocks", 7, 1.5]],
        });
        let mut rows = flatten_report(&report);
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        let expected = [
            ("by_label_mb[chunks]", 3.0),
            ("caches[blocks]", 7.0),
            ("caches_mb[blocks]", 1.5),
            ("process.resident_mb", 12.5),
            ("render_distance", 16.0),
        ];
        assert_eq!(
            rows.iter()
                .map(|(p, v)| (p.as_str(), *v))
                .collect::<Vec<_>>(),
            expected
        );
    }
}
