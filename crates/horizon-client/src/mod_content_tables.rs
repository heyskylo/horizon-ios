//! The base's per-state tables, rewritten for the mods' states while
//! [`build`](super::build) reads the mods' files.
use super::{JSON_TABLES, NBT_TABLES, Overlay, Plan, Result, read_json};
use horizon_world::nbt::{self, Limits};
use std::fs;
use std::path::Path;

/// Threads rewriting the tables at once. Each holds a whole table as a tree
/// (gameplay-facts.json's is over a gigabyte with ATM10's mods), so more
/// would mostly add memory.
const TABLE_THREADS: usize = 3;

/// The base's per-state tables, the largest first, with their directories
/// made real in the overlay: [`tables`]' threads then only write files.
pub(super) fn table_jobs(base: &Path, overlay: &Overlay<'_>) -> Result<Vec<(&'static str, u64)>> {
    let mut jobs: Vec<(&str, u64)> = JSON_TABLES
        .iter()
        .chain(&NBT_TABLES)
        .filter_map(|name| Some((*name, fs::metadata(base.join(name)).ok()?.len())))
        .collect();
    jobs.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    for (name, _) in &jobs {
        if let Some(parent) = Path::new(name).parent() {
            overlay.dir(parent)?;
        }
    }
    Ok(jobs)
}

/// Rewrites the base's per-state tables (`jobs`, from [`table_jobs`]) for
/// the mods' states into the overlay. They are independent:
/// [`TABLE_THREADS`] threads take them, the largest first.
pub(super) fn tables(
    base: &Path,
    plan: &Plan,
    overlay: &Overlay<'_>,
    jobs: &[(&str, u64)],
) -> Result<()> {
    let table = |name: &str| -> Result<()> {
        let path = base.join(name);
        let bytes = if name == "entity-types.json" {
            plan.entity_types_json(&fs::read(&path)?)
                .map_err(|e| format!("{name}: {e}"))?
        } else if JSON_TABLES.contains(&name) {
            let mut doc = read_json(&path)?;
            plan.json(name, &mut doc)
                .map_err(|e| format!("{name}: {e}"))?;
            serde_json::to_vec(&doc)?
        } else {
            let (mut named, _) = nbt::read_named(&fs::read(&path)?, Limits::tables())
                .map_err(|e| format!("{name}: {e}"))?;
            plan.nbt(&mut named.tag);
            if name == "cached-collision.nbt" || name == "worldgen/blocks/cached-collision.nbt" {
                plan.collision_shapes(&mut named.tag)?;
            }
            nbt::write_named(&named, Limits::tables()).map_err(|e| format!("{name}: {e}"))?
        };
        overlay.write(name, &bytes)
    };
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = horizon_threads::worker_count();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads.min(TABLE_THREADS))
            .map(|_| {
                scope.spawn(|| -> std::result::Result<(), String> {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((name, _)) = jobs.get(i) else {
                            return Ok(());
                        };
                        table(name).map_err(|e| e.to_string())?;
                    }
                })
            })
            .collect();
        workers.into_iter().try_for_each(|w| {
            w.join()
                .unwrap_or_else(|_| Err("a table thread panicked".into()))
        })
    })?;
    Ok(())
}
