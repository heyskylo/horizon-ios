//! The dropped items' own moves, worked out ahead of the item loop on the tick's helper threads
//! while the parallel tick is on (`horizon_client::parallel`).
//!
//! An item's move (`ItemEntity.tick` up to its merge) reads only itself, the level's blocks, the
//! obstacle boxes and the border, and the item loop changes none of them: so an item's move worked
//! out on a copy before the loop is the move its turn would make. Its escape from inside a block
//! draws the item's own random, which the copy carries. A merge before its turn can change its
//! count, age and pickup delay: such an item moves at its turn instead. With a native mod loaded
//! (its hooks see every item) or a mod listening to the round, every item moves at its turn.
use super::super::{boats::fall_reset, entity::Entity as _, native_ticks};
use super::*;
use horizon_client::{
    blocks::BlockFacts,
    entities::{ItemEntity, ItemSplash, ItemStep, ObstacleGrid},
    motion::MotionResources,
    world::World,
};
use horizon_core::geometry::{Aabb, Vec3, border::BorderGeometry};
use horizon_world::{block_collision::BlockCollisionRegistry, registry::BlockRegistry};

/// From this many items their moves go to the helper threads.
const MIN_ITEMS: usize = 256;

/// What an item's own move reads.
pub(in crate::game) struct ItemTables<'a> {
    world: &'a World,
    facts: &'a BlockFacts,
    collision: &'a BlockCollisionRegistry<'a>,
    motion: &'a MotionResources,
    registry: &'a BlockRegistry,
    border: &'a BorderGeometry,
}

impl<'w> Game<'w> {
    pub(in crate::game) fn item_tables(&self) -> ItemTables<'_> {
        ItemTables {
            world: &self.world,
            facts: self.res.facts(),
            collision: self.collision,
            motion: self.res.motion(),
            registry: self.pack.assets.registry,
            border: self.border.geometry(),
        }
    }
}

impl ItemTables<'_> {
    /// `ItemEntity.tick` up to its move's vertical contact: the fluids, the water splash, the move
    /// (pushed out at `escape_speed` when inside a block) and its fall.
    pub(in crate::game) fn move_item(
        &self,
        item: &mut ItemEntity,
        escape_speed: Option<f32>,
        entity_boxes: &[Aabb],
        splash: &mut impl FnMut(ItemSplash),
    ) -> ItemStep {
        // Entity.baseTick splashes before moving, except on the first tick, at
        // the motion before the currents push it.
        let before = item.velocity;
        let heights = item.push_by_fluid(self.world, &self.motion.fluids);
        item.water_entry(heights.0 > 0.0, item.ticks == 0, before, splash);
        item.lava_fall(heights.1 > 0.0);
        let step = item.tick_move_before_block_effects(
            self.world,
            self.facts,
            self.collision,
            escape_speed.is_some(),
            escape_speed,
            heights,
            entity_boxes,
            Some(self.border),
            Some(&self.motion.blocks),
        );
        item.check_fall(&step, |fall, from, moved| {
            fall_reset(self.world, self.motion, self.facts, fall, from, moved)
        });
        step
    }
    /// The rest of the move after its vertical contact: block effects, bubble columns, settling
    /// and the water entry. Whether the item lives on.
    pub(in crate::game) fn settle_item(
        &self,
        item: &mut ItemEntity,
        no_physics: bool,
        step: ItemStep,
        splash: &mut impl FnMut(ItemSplash),
    ) -> bool {
        item.finish_move_block_effects(
            self.world,
            self.facts,
            no_physics,
            Some(&self.motion.blocks),
            step,
        );
        item.push_by_bubble_column(self.world, self.registry, self.facts, self.collision);
        let alive = item.tick_settle(self.world, self.facts, no_physics, step);
        // ItemEntity.tick refreshes immersion and splashes on water entry.
        let before = item.velocity;
        let (water, _) = item.push_by_fluid(self.world, &self.motion.fluids);
        item.water_entry(water > 0.0, false, before, splash);
        alive
    }
}

/// `getEntityCollisions(entity, bounds.expandTowards(movement))`: the box around an item that its
/// move reads others' boxes in (a block of slack for the next velocity).
pub(super) fn reach(item: &ItemEntity) -> Aabb {
    let v = item.velocity;
    item.bounds()
        .inflate(Vec3::new(v.x.abs() + 1.0, v.y.abs() + 1.0, v.z.abs() + 1.0))
}

/// An item's move as its turn makes it: without physics (inside a block), alive, discarded, and
/// its splashes.
pub(super) type Moved = (bool, bool, bool, Vec<ItemSplash>);

/// One item's move, worked out on a copy.
pub(super) struct Ahead {
    moved: ItemEntity,
    result: Moved,
    /// What a merge before its turn can change: its count, age and pickup delay.
    started: (u32, i32, i64),
}

fn started(item: &ItemEntity) -> (u32, i32, i64) {
    (item.stack.count, item.age, item.pickup_delay)
}

/// `item`'s whole move on a server with no native mod, from the tables alone.
fn move_alone(
    tables: &ItemTables,
    obstacles: &ObstacleGrid,
    item: &mut ItemEntity,
    found: &mut Vec<u32>,
    near: &mut Vec<Aabb>,
) -> Moved {
    obstacles.near_shared(reach(item), found, near);
    let no_physics = item.inside_block(tables.world, tables.collision, near, Some(tables.border));
    // `moveTowardsClosestSpace` draws the speed from the item's own random.
    let escape_speed = no_physics.then(|| item.random.next_float() * 0.2 + 0.1);
    let mut splashes = Vec::new();
    let step = tables.move_item(item, escape_speed, near, &mut |s| splashes.push(s));
    if step.vertical_contact.is_some() && item.stack.count == 0 {
        return (no_physics, false, true, splashes);
    }
    let alive = tables.settle_item(item, no_physics, step, &mut |s| splashes.push(s));
    (no_physics, alive, false, splashes)
}

#[cfg(test)]
thread_local! {
    /// A test's choice of the path, over the parallel tick's switch.
    static AHEAD: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// The moves a test's tick put in place, and those a merge made it refuse.
    static TAKEN: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// Whether the moves are worked out ahead: while the parallel tick is on.
fn ahead() -> bool {
    #[cfg(test)]
    if let Some(ahead) = AHEAD.with(std::cell::Cell::get) {
        return ahead;
    }
    horizon_client::parallel::lanes() > 1
}

/// The moves worked out ahead, by the item's place in the loop.
pub(super) struct Moves(Vec<Option<Ahead>>);

impl Moves {
    /// The moves of the items that will tick (`ticks`), on the helper threads; none while the
    /// parallel tick is off, for few items, or with a mod that could see an item's move.
    pub(super) fn work_out(
        game: &Game,
        items: &[ItemEntity],
        obstacles: &ObstacleGrid,
        round: &native_ticks::TickRound,
        ticks: impl Fn(&ItemEntity) -> bool,
    ) -> Self {
        if !ahead()
            || items.len() < MIN_ITEMS
            || game.remote.is_some()
            || round.listening()
            || !horizon_client::mod_api::mods().is_empty()
        {
            return Self(Vec::new());
        }
        let todo: Vec<usize> = (0..items.len()).filter(|&i| ticks(&items[i])).collect();
        let tables = game.item_tables();
        let done = horizon_client::parallel::par_map(&todo, MIN_ITEMS / 4, |part, out| {
            let (mut found, mut near) = (Vec::new(), Vec::new());
            for &i in part {
                let mut moved = items[i].clone();
                let result = move_alone(&tables, obstacles, &mut moved, &mut found, &mut near);
                let started = started(&items[i]);
                out.push((
                    i,
                    Ahead {
                        moved,
                        result,
                        started,
                    },
                ));
            }
        });
        let mut moves: Vec<Option<Ahead>> = (0..items.len()).map(|_| None).collect();
        for (i, a) in done {
            moves[i] = Some(a);
        }
        Self(moves)
    }
    /// Item `i`'s move, put in place, when it was worked out and no merge changed the item since.
    /// Under `HORIZON_PARALLEL_TICK=verify` the move is made again at its turn, on a copy, and a
    /// difference is logged.
    pub(super) fn take(
        &mut self,
        i: usize,
        item: &mut ItemEntity,
        again: impl FnOnce(&mut ItemEntity) -> Moved,
    ) -> Option<Moved> {
        let a = self.0.get_mut(i)?.take()?;
        let fits = a.started == started(item);
        #[cfg(test)]
        TAKEN.with(|t| {
            let (taken, refused) = t.get();
            t.set((taken + usize::from(fits), refused + usize::from(!fits)));
        });
        if !fits {
            return None;
        }
        if horizon_client::parallel::parallel_tick() == 2 {
            let mut serial = item.clone();
            let expected = again(&mut serial);
            check(item.id, &serial, &expected, &a.moved, &a.result);
        }
        *item = a.moved;
        Some(a.result)
    }
}

/// `HORIZON_PARALLEL_TICK=verify`: an item's move worked out ahead against the one at its turn.
fn check(id: u32, serial: &ItemEntity, expected: &Moved, ahead: &ItemEntity, got: &Moved) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CHECKED: AtomicU64 = AtomicU64::new(0);
    static DIFFERED: AtomicU64 = AtomicU64::new(0);
    if format!("{serial:?} {expected:?}") != format!("{ahead:?} {got:?}") {
        DIFFERED.fetch_add(1, Ordering::Relaxed);
        log::error!("parallel tick: item {id}'s move worked out ahead differs from its turn's");
    }
    let n = CHECKED.fetch_add(1, Ordering::Relaxed) + 1;
    if n == 1 || n.is_multiple_of(10_000) {
        log::info!(
            "parallel tick: {n} item moves worked out ahead checked, {} differed",
            DIFFERED.load(Ordering::Relaxed)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::native_machines::tests::server_game;
    use super::{AHEAD, TAKEN};
    use horizon_client::{chunk::Chunk, items::ItemStack};
    use horizon_core::{geometry::Vec3, random::RandomSource};
    use std::sync::Arc;

    /// One item tick with the moves worked out ahead leaves every item, and the level's random,
    /// as the tick that moves each item at its turn: items escaping a block (their own random),
    /// floating in water, falling, and merging into a later item before its turn (whose move
    /// is then made at its turn).
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn moves_worked_out_ahead_match_those_made_at_each_turn() {
        let mut game = server_game();
        let feet = horizon_core::geometry::Vec3::new(8.5, 70.0, 8.5);
        game.player = Some(horizon_client::motion::PlayerMotion::new(game.motion, feet).unwrap());
        // `ChunkMap.prepareEntityTickingChunk`: the chunks within 2 are loaded too.
        for x in -2..=2 {
            for z in -2..=2 {
                game.world
                    .chunks
                    .insert((x, z), Arc::new(Chunk::empty(x, z)));
            }
        }
        game.update_chunk_tickets();
        assert!(game.entity_ticking_area()(8, 8));
        game.border.set_center([8.0, 8.0]);
        game.border.set_size(16.0);
        let registry = game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let water = registry.default_state("minecraft:water").unwrap();
        let mut escaping = Vec::new();
        for x in 0..16 {
            for z in 0..16 {
                let (fx, fz) = (f64::from(x) + 0.5, f64::from(z) + 0.5);
                let stack = |count| ItemStack::new("minecraft:stone", count);
                match (x + z) % 5 {
                    0 => {
                        game.world.set(x, 65, z, stone).unwrap();
                        game.spawn_item(stack(1), [fx, 65.3, fz], [0.0; 3], 10);
                        escaping.push(game.dropped.last().unwrap().id);
                    }
                    1 => {
                        game.world.set(x, 65, z, water).unwrap();
                        game.spawn_item(stack(1), [fx, 65.5, fz], [0.0; 3], 10);
                    }
                    2 => {
                        game.spawn_item(stack(1), [fx, 66.0, fz], [0.0; 3], 0);
                        game.spawn_item(stack(5), [fx, 66.0, fz], [0.0; 3], 0);
                    }
                    _ => game.spawn_item(stack(1), [fx, 67.0, fz], [0.1, 0.2, -0.1], 10),
                }
            }
        }
        // Real solid-entity boxes, not a stubbed obstacle query. One item is
        // caught inside the boat; another falls onto its top this tick.
        game.next_entity += 1;
        let boat = super::super::super::boats::Boat::new(
            game.next_entity,
            Arc::from("oak"),
            Vec3::new(8.5, 80.0, 8.5),
            0.0,
        );
        let boat_top = boat.bounds().max_position().y;
        game.boats.push(boat);
        game.spawn_item(
            ItemStack::new("minecraft:iron_ingot", 1),
            [8.5, 80.2, 8.5],
            [0.0; 3],
            10,
        );
        let caught = game.dropped.last().unwrap().id;
        escaping.push(caught);
        game.spawn_item(
            ItemStack::new("minecraft:gold_ingot", 1),
            [8.5, 81.0, 8.5],
            [0.0, -1.0, 0.0],
            10,
        );
        let landing = game.dropped.last().unwrap().id;
        // The border cuts the box even though the centre is still inside it.
        game.spawn_item(
            ItemStack::new("minecraft:diamond", 1),
            [15.99, 80.0, 8.5],
            [0.0; 3],
            10,
        );
        let border = game.dropped.last().unwrap().id;
        escaping.push(border);
        let boxes = game.item_entity_collisions();
        for id in [caught, border] {
            let item = game.dropped.iter().find(|i| i.id == id).unwrap();
            assert!(item.inside_block(
                &game.world,
                game.collision,
                &boxes,
                Some(game.border.geometry())
            ));
            assert!(!item.inside_block(&game.world, game.collision, &[], None));
        }
        for item in game.dropped.iter_mut() {
            item.ticks = 39;
        }
        game.ticked_entities_through = u32::MAX;
        let (items, random) = (game.dropped.clone(), game.world.random.clone());
        // Each merge-written field independently invalidates a prepared move.
        // Rejecting it must not overwrite the merge or advance the item's RNG.
        AHEAD.with(|a| a.set(Some(true)));
        let obstacles = super::ObstacleGrid::new(boxes);
        for changed in 0..3 {
            let mut moves = super::Moves::work_out(
                &game,
                &items,
                &obstacles,
                &super::native_ticks::TickRound::default(),
                |_| true,
            );
            let mut item = items[0].clone();
            match changed {
                0 => item.stack.count += 1,
                1 => item.age += 1,
                _ => item.pickup_delay += 1,
            }
            let before = format!("{item:?}");
            assert!(
                moves
                    .take(0, &mut item, |_| panic!("invalidated move was verified"))
                    .is_none()
            );
            assert_eq!(format!("{item:?}"), before);
        }
        let mut runs = Vec::new();
        // First use the real environment switch, then serial/ahead/serial
        // overrides. This also checks switch-off behavior above the threshold.
        for ahead in [None, Some(false), Some(true), Some(false)] {
            game.dropped = items.clone();
            game.world.random = random.clone();
            AHEAD.with(|a| a.set(ahead));
            TAKEN.with(|t| t.set((0, 0)));
            game.tick_items();
            // Independently advance just the escaping items' starting randoms.
            // The helper copy must consume exactly one float and return that
            // state at the turn; ordinary moves must consume none of it.
            for item in game.dropped.iter() {
                let before = items.iter().find(|i| i.id == item.id).unwrap();
                let mut expected = before.random.clone();
                if escaping.contains(&item.id) {
                    expected.next_float();
                }
                assert_eq!(
                    item.random.clone().next_int(),
                    expected.next_int(),
                    "item {}",
                    item.id
                );
            }
            let landed = game.dropped.iter().find(|i| i.id == landing).unwrap();
            assert_eq!(landed.position.y, boat_top);
            assert!(landed.on_ground);
            assert_eq!(format!("{:?}", game.world.random), format!("{random:?}"));
            let left: Vec<String> = game.dropped.iter().map(|i| format!("{i:?}")).collect();
            runs.push((
                left,
                format!("{:?}", game.world.random),
                TAKEN.with(|t| t.get()),
            ));
        }
        AHEAD.with(|a| a.set(None));
        assert!(runs[0].0.len() < items.len(), "some items merged");
        assert_eq!(runs[0].0, runs[1].0);
        assert_eq!(runs[0].1, runs[1].1);
        assert_eq!(runs[0].0, runs[2].0);
        assert_eq!(runs[0].1, runs[2].1);
        assert_eq!(runs[0].0, runs[3].0);
        assert_eq!(runs[0].1, runs[3].1);
        assert_eq!(runs[1].2, (0, 0));
        assert_eq!(runs[3].2, (0, 0));
        if horizon_client::parallel::lanes() > 1 {
            assert_eq!(runs[0].2, runs[2].2);
        } else {
            assert_eq!(runs[0].2, (0, 0));
        }
        // The path under test ran: most moves were put in place, and a merge refused some.
        let (taken, refused) = runs[2].2;
        assert!(
            taken >= 200 && refused > 0,
            "{taken} put in place, {refused} refused"
        );
        eprintln!(
            "item work-ahead proof: {} inputs, {} escaping; {taken} taken, {refused} invalidated",
            items.len(),
            escaping.len()
        );
    }
}
