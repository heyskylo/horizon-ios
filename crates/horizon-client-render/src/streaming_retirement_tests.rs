//! Actual worker completion across terminal streaming cancellation.
use super::*;
use std::sync::mpsc;
use std::time::Duration;

struct HeldDecoration {
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    stone: u32,
    saved: Mutex<Vec<u64>>,
    release_timeout: Duration,
}
impl ChunkGenerator for HeldDecoration {
    type Worker = ();
    fn worker(&self) {}
    fn proto(&self, _: &mut (), _: i32, _: i32) -> Proto {
        Box::new(())
    }
    fn decorate(&self, _: &mut (), region: &mut [Proto; 9]) {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(self.release_timeout)
            .unwrap();
        for proto in region {
            *proto.downcast_mut::<u64>().unwrap() += 100;
        }
    }
    fn final_writes(&self, _: &Chunk, _: &Proto) -> Vec<([i32; 3], u32)> {
        vec![([1, 70, 1], self.stone)]
    }
    fn finish(&self, _: Proto) -> Chunk {
        unreachable!("the fixture only submits decoration and save work")
    }
    fn save_protos(&self, protos: &[&Proto]) {
        self.saved
            .lock()
            .unwrap()
            .extend(protos.iter().map(|p| *p.downcast_ref::<u64>().unwrap()));
    }
}

fn context() -> crate::Result<Arc<WorkerContext>> {
    use crate::{assets::Assets, biome::Biomes, blocks::BlockFacts};
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES required"),
    );
    let assets = Arc::new(Assets::load(&root)?);
    let render = Arc::new(assets.reload(&root)?);
    let facts = BlockFacts::load(&root, assets.registry)?;
    Ok(Arc::new(WorkerContext {
        tables: Arc::new(MeshTables::new(
            &assets,
            &render,
            &facts,
            crate::models::load_visibility(&root, assets.registry)?,
        )?),
        light: Arc::new(LightTables::load(&root, assets.registry, &facts)?),
        biomes: Arc::new(Biomes::load(&root)?),
        assets,
        render,
    }))
}

/// This deliberately crosses the production 30-second synchronous-save
/// deadline. A late decoration owns dirty proto chunks outside World.
#[test]
#[ignore = "requires HORIZON_TEST_RESOURCES; holds an actual worker for 32 seconds"]
fn synchronous_save_cannot_acknowledge_before_late_owned_decoration_is_saved() -> crate::Result<()>
{
    let context = context()?;
    let stone = context
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    let (entered, started) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let generator = Arc::new(HeldDecoration {
        entered,
        release: Mutex::new(held),
        stone: stone.0,
        saved: Mutex::new(Vec::new()),
        release_timeout: Duration::from_secs(45),
    });
    let mut streaming = Streaming::new(generator.clone(), context, 1);
    let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
    let final_chunk = Chunk::empty(0, 0);
    world.chunks.insert((0, 0), Arc::new(final_chunk.clone()));
    streaming.decorating = 1;
    streaming.decorating_finals.insert((0, 0));
    for z in -1..=1 {
        for x in -1..=1 {
            if (x, z) != (0, 0) {
                streaming.protos.insert((x, z), None);
            }
        }
    }
    streaming.submit(
        Some((0, 0)),
        1,
        Job::Decorate(
            0,
            0,
            Box::new(std::array::from_fn(|_| Box::new(7u64) as Proto)),
            vec![(4, final_chunk)],
        ),
    );
    started
        .recv_timeout(Duration::from_secs(10))
        .expect("worker did not start");
    streaming.abort_generation();
    let (ack, acknowledgement) = mpsc::channel();
    let saver = std::thread::spawn(move || {
        streaming.save(&mut world, true);
        ack.send(()).unwrap();
        (streaming, world)
    });
    let held_until = std::time::Instant::now() + Duration::from_secs(32);
    let early_ack = acknowledgement
        .recv_timeout(Duration::from_secs(32))
        .is_ok();
    // Keep the actual worker held for the full interval even when RED
    // acknowledges early; always release it before asserting below.
    std::thread::sleep(held_until.saturating_duration_since(std::time::Instant::now()));
    // Always release the real worker before an assertion, including on RED.
    release.send(()).unwrap();
    let (streaming, world) = saver.join().expect("save owner panicked");
    let saved = generator.saved.lock().unwrap().clone();
    drop(streaming);
    assert!(
        !early_ack,
        "save(true) falsely acknowledged while decoration still owned dirty protos"
    );
    assert_eq!(
        saved,
        vec![107; 8],
        "late owned proto writes must be persisted before ACK"
    );
    assert_eq!(
        world.block(1, 70, 1),
        stone,
        "late final writes must reach the authoritative world before its chunk save"
    );
    Ok(())
}

#[test]
#[ignore = "requires prepared models/textures in HORIZON_TEST_RESOURCES"]
fn closing_stream_preserves_owned_generation_before_ack() -> crate::Result<()> {
    let context = context()?;
    let stone = context
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    for mode in [
        "normal",
        "poll_save",
        "blocking_save",
        "poll_save_deferred",
        "blocking_save_deferred",
    ] {
        let (entered, started) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        let generator = Arc::new(HeldDecoration {
            entered,
            release: Mutex::new(held),
            stone: stone.0,
            saved: Mutex::new(Vec::new()),
            release_timeout: Duration::from_secs(10),
        });
        let mut streaming = Streaming::new(generator.clone(), context.clone(), 1);
        let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
        let final_chunk = Chunk::empty(0, 0);
        world.chunks.insert((0, 0), Arc::new(final_chunk.clone()));
        streaming.decorating = 1;
        streaming.decorating_finals.insert((0, 0));
        for z in -1..=1 {
            for x in -1..=1 {
                if (x, z) != (0, 0) {
                    streaming.protos.insert((x, z), None);
                }
            }
        }
        streaming.submit(
            Some((0, 0)),
            1,
            Job::Decorate(
                0,
                0,
                Box::new(std::array::from_fn(|_| Box::new(7u64) as Proto)),
                vec![(4, final_chunk)],
            ),
        );
        // A real pool worker has passed its abort check and is inside decorate.
        started
            .recv_timeout(Duration::from_secs(10))
            .expect("worker did not enter decorate");
        if mode != "normal" {
            streaming.abort_generation();
            let started = std::time::Instant::now();
            assert!(
                !streaming.poll_save(&mut world),
                "held worker must prevent closing ACK"
            );
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "UI poll must not join worker"
            );
        }
        release.send(()).unwrap();
        let done = streaming
            .shared
            .done
            .recv_timeout(Duration::from_secs(10))
            .expect("worker did not finish decoration");
        assert!(
            matches!(done, Done::Decorated(..)),
            "actual worker completion required"
        );
        if mode == "normal" {
            let Done::Decorated(x, z, region, finals, writes) = done else {
                unreachable!()
            };
            streaming.receive_decorated(x, z, *region, finals, writes, &mut world);
            assert_eq!(streaming.decorating, 0);
            assert!(streaming.decorating_finals.is_empty());
            assert_eq!(world.block(1, 70, 1), stone);
            assert_eq!(streaming.written.len(), 8);
            assert!(streaming.protos.values().all(Option::is_some));
            continue;
        }
        // Place completion before a genuine durable save. Blocking save must
        // accept the completed result while still draining the save owner.
        let prefix = if mode == "poll_save_deferred" {
            200_000
        } else if mode.ends_with("_deferred") {
            3
        } else {
            0
        };
        if prefix > 0 {
            for i in 0..prefix {
                streaming
                    .deferred
                    .push(Done::Proto(i, 80, Box::new(0u64), false));
            }
            streaming.deferred.push(done);
        } else {
            streaming.shared.done.send(done);
        }
        streaming.protos.insert((20, 20), Some(Box::new(999u64)));
        streaming.save_checked_out([(20, 20)]);
        assert_eq!(streaming.saving, 1);
        if mode.starts_with("blocking_save") {
            streaming.save(&mut world, true);
        } else {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !streaming.poll_save(&mut world) {
                assert!(std::time::Instant::now() < deadline, "save failed to drain");
                std::thread::yield_now();
            }
        }
        assert_eq!(
            streaming.save_deferred.len(),
            prefix as usize,
            "ordinary results preserved"
        );
        for (i, done) in streaming.save_deferred.iter().enumerate() {
            assert!(
                matches!(done, Done::Proto(x, 80, _, false) if *x == i as i32),
                "ordinary FIFO preserved"
            );
        }
        assert_eq!(streaming.saving, 0, "{mode}");
        let mut saved = generator.saved.lock().unwrap().clone();
        saved.sort_unstable();
        assert_eq!(
            saved,
            [vec![107; 8], vec![999]].concat(),
            "{mode}: accepted writes survive"
        );
        assert_eq!(streaming.decorating, 0, "{mode}");
        assert!(streaming.decorating_finals.is_empty(), "{mode}");
        assert_eq!(
            world.block(1, 70, 1),
            stone,
            "{mode}: completed final writes"
        );
        assert!(world.modified.contains(&(0, 0)), "{mode}");
        assert!(streaming.written.is_empty(), "{mode}: protos saved");
        assert!(streaming.decorated.contains(&(0, 0)), "{mode}");
        assert!(
            streaming.protos.values().all(Option::is_some),
            "{mode}: all owners returned"
        );
    }
    queued_checkouts_survive_abort(context);
    Ok(())
}

fn queued_checkouts_survive_abort(context: Arc<WorkerContext>) {
    for popped in [false, true] {
        let (entered, _) = mpsc::sync_channel(1);
        let (_, held) = mpsc::sync_channel(1);
        let generator = Arc::new(HeldDecoration {
            entered,
            release: Mutex::new(held),
            stone: 1,
            saved: Mutex::new(Vec::new()),
            release_timeout: Duration::from_secs(10),
        });
        let mut streaming = Streaming::new(generator.clone(), context.clone(), 1);
        let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
        let mut jobs = Vec::new();
        streaming.decorating = 1;
        for z in -1..=1 {
            for x in -1..=1 {
                streaming.protos.insert((x, z), None);
                streaming.written.insert((x, z));
            }
        }
        jobs.push((
            Some((0, 0)),
            Job::Decorate(
                0,
                0,
                Box::new(std::array::from_fn(|_| Box::new(7u64) as Proto)),
                Vec::new(),
            ),
        ));
        streaming.finishing.insert((20, 20));
        // Finish took this dirty key out of written at dispatch.
        jobs.push((Some((20, 20)), Job::Finish(Box::new(17u64))));
        streaming.viewing.insert((30, 30));
        streaming.protos.insert((30, 30), None);
        streaming.written.insert((30, 30));
        // LightView's scheduling column differs from the proto it owns.
        jobs.push((Some((31, 31)), Job::LightView(30, 30, Box::new(27u64))));
        streaming.generating.insert((40, 40));
        jobs.push((Some((40, 40)), Job::Generate(40, 40)));
        let jobs: Vec<_> = jobs
            .into_iter()
            .enumerate()
            .map(|(order, (at, job))| Queued {
                at,
                job,
                order: order as u64,
                bias: 0,
                priority: 1,
            })
            .collect();
        // An autosave while those protos are checked out must retain their dirty ownership.
        streaming.save(&mut world, false);
        if popped {
            // Exercise the actual worker's post-pop abort check: the queue
            // no longer owns these jobs when abort cancels what remains.
            streaming.abort_generation();
            for job in jobs {
                assert!(streaming.shared.accept_job(job).is_none());
            }
        } else {
            // Freeze dispatch at the same terminal flag used by abort so this
            // deterministically tests jobs still owned by the queue.
            let mut queue = streaming.shared.queue.lock().unwrap();
            queue.extend(jobs);
            streaming
                .shared
                .aborted
                .store(true, std::sync::atomic::Ordering::SeqCst);
            drop(queue);
            streaming.abort_generation();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !streaming.poll_save(&mut world) {
            assert!(
                std::time::Instant::now() < deadline,
                "canceled inputs never returned"
            );
            std::thread::yield_now();
        }
        assert_eq!(streaming.decorating, 0);
        assert!(streaming.finishing.is_empty());
        assert!(streaming.viewing.is_empty());
        assert_eq!(streaming.saving, 0);
        let mut saved = generator.saved.lock().unwrap().clone();
        saved.sort_unstable();
        assert_eq!(
            saved,
            [vec![7; 9], vec![17, 27]].concat(),
            "popped={popped}: dirty originals must survive"
        );
        assert_eq!(streaming.protos.len(), 11);
        assert!(streaming.protos.values().all(Option::is_some));
        assert!(streaming.written.is_empty());
    }
}
