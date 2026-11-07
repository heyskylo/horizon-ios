//! Fluids of a block for native server mods (docs/native-mods.md, API 54):
//! the tanks a mod block declared through `native_block_entity_fluids`, and a
//! cauldron, read through a side, filled and drained in millibuckets. They act
//! where the world is simulated, never on a client's copy, and only on the
//! level in place, which the readers name by their `dimension`
//! (`minecraft:overworld`), as `native_set_block` does.
use super::Game;
use horizon_client::containers::ModFluidTanks;
use horizon_client::mod_api::{
    NativeFluidAction, NativeFluidDrain, NativeFluidStack, NativeFluidTankView,
};
use horizon_core::geometry::Direction;
use horizon_world::nbt::{Compound, JavaString, Tag};
use std::sync::Arc;

/// A tank as a side reaches it (`native_fluid_tanks`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeFluidTank {
    /// Its number, for the mod's own use.
    pub tank: usize,
    /// `minecraft:water`, `minecraft:lava` or a mod's own (`create:honey`);
    /// none when the tank is empty.
    pub fluid: Option<String>,
    /// In millibuckets: a bucket is a thousand.
    pub amount: i32,
    pub capacity: i32,
    /// Whether a fill may reach it through this side.
    pub can_fill: bool,
    /// Whether a drain may take it through this side.
    pub can_drain: bool,
}

/// A fluid a tank may hold: vanilla's water and lava, or a mod's own
/// (`create:chocolate`), any id in a namespace other than `minecraft`, of
/// vanilla's identifier characters. A mod's fluid lives in tanks only, the
/// world has vanilla's alone, and its mod draws it; a cauldron takes only
/// water and lava. Variants of one fluid (a potion's kinds) are fluids of
/// their own (`create:potion/healing`): equal ids merge, others don't.
fn tank_fluid(fluid: &str) -> bool {
    if matches!(fluid, "minecraft:water" | "minecraft:lava") {
        return true;
    }
    let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b);
    fluid.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && namespace != "minecraft"
            && namespace.bytes().all(ok)
            && !path.is_empty()
            && path.bytes().all(|b| ok(b) || b == b'/')
    })
}

fn plain_stack(stack: &NativeFluidStack, capacity: i32) -> bool {
    stack.components.is_empty()
        && (1..=capacity).contains(&stack.amount)
        && tank_fluid(&stack.fluid)
}

fn plain_tank(tank: NativeFluidTankView) -> Option<NativeFluidTank> {
    let (fluid, amount) = match tank.content {
        Some(stack) if plain_stack(&stack, tank.capacity) => (Some(stack.fluid), stack.amount),
        Some(_) => return None,
        None => (None, 0),
    };
    (tank.capacity >= 0).then_some(NativeFluidTank {
        tank: tank.tank,
        fluid,
        amount,
        capacity: tank.capacity,
        can_fill: tank.can_fill,
        can_drain: tank.can_drain,
    })
}

/// A cauldron level's contents: level `level` holds `level * 1000 / 3`
/// millibuckets (`LayeredCauldronBlock` has no finer unit than a bottle's
/// third of a bucket).
fn third(level: u8) -> i32 {
    i32::from(level) * 1000 / 3
}

/// The level a fill of `given` millibuckets raises `level` to: a level
/// rises only while its whole third fits what is left of the give, so no
/// sum ever grows past a bucket.
fn fill_level(level: u8, given: i32) -> u8 {
    let mut new = level;
    while new < 3 && third(new + 1) - third(level) <= given {
        new += 1;
    }
    new
}

/// What a drain of at most `amount` takes out of water level `level`, in
/// whole thirds — never more than was asked, as `IFluidHandler.drain`'s
/// `maxDrain`: less than the topmost whole third takes nothing. Answers
/// what it took and the level it left.
fn drain_take(level: u8, amount: i32) -> (i32, u8) {
    let mut taken = 0;
    let mut left = level;
    while left > 0 {
        let step = third(left) - third(left - 1);
        if taken + step > amount {
            break;
        }
        taken += step;
        left -= 1;
    }
    (taken, left)
}

/// What holds fluids at a position: a mod block's tanks, or a cauldron
/// (`CauldronFluids`). An empty cauldron is fillable; powder snow is not a
/// fluid (`FluidKind` has none), so a powder snow cauldron holds none.
enum FluidAt {
    Custom,
    /// A mod block's tanks, and its block id (`mymod:tank`).
    Mod {
        tanks: Arc<ModFluidTanks>,
        block: String,
    },
    Cauldron(CauldronFluids),
}
#[derive(Clone, Copy)]
enum CauldronFluids {
    Empty,
    Water(u8),
    Lava,
}

/// The saved state of one tank: its fluid and amount in millibuckets.
type Tank = (Option<String>, i32);

/// The tanks in `nbt`, each `n` long: missing entries read as empty, and a
/// save with more tanks than the block now declares keeps the ones that
/// still exist (a mod update that shrank its tanks loses nothing it kept).
fn read_tanks(nbt: &Compound, n: usize) -> Vec<Tank> {
    let list = match nbt.get(&JavaString::from("fluids")) {
        Some(Tag::List(list)) => list,
        _ => return vec![(None, 0); n],
    };
    let read: Vec<Tank> = list
        .iter()
        .take(n)
        .map(|tag| match tag {
            Tag::Compound(map) => (
                match map.get(&JavaString::from("Fluid")) {
                    Some(Tag::String(fluid)) if fluid.to_string_lossy() != "minecraft:empty" => {
                        Some(fluid.to_string_lossy())
                    }
                    _ => None,
                },
                match map.get(&JavaString::from("Amount")) {
                    Some(Tag::Int(amount)) => *amount,
                    _ => 0,
                },
            ),
            _ => (None, 0),
        })
        .collect();
    let mut tanks = read;
    tanks.resize(n, (None, 0));
    tanks
}

/// `read_tanks`'s writer: the list back into `nbt`, an empty tank as
/// `minecraft:empty` so the tank numbers stay in place.
fn write_tanks(nbt: &mut Compound, tanks: &[Tank]) {
    let list = tanks
        .iter()
        .map(|&(ref fluid, amount)| {
            let mut map = Compound::new();
            map.insert(
                JavaString::from("Fluid"),
                Tag::String(JavaString::from(
                    fluid.as_deref().unwrap_or("minecraft:empty"),
                )),
            );
            map.insert(JavaString::from("Amount"), Tag::Int(amount));
            Tag::Compound(map)
        })
        .collect();
    nbt.insert(JavaString::from("fluids"), Tag::List(list));
}

/// Fills `state` from `amount` of `fluid`: first the tanks holding it, then
/// empty ones, by the side `reached` and only where `accepts` lets it in
/// (`native_block_entity_accepts_fluid`); what did not fit. The choice is
/// made again in both passes, as `HopperBlockEntity.addItem` tries its slots.
fn fill_tanks(
    tanks: &ModFluidTanks,
    state: &mut [Tank],
    reached: &[usize],
    fluid: &str,
    amount: i32,
    accepts: impl Fn(usize) -> bool,
) -> i32 {
    let n = tanks.tanks.len();
    let mut left = amount.max(0);
    for pass in 0..2 {
        for &tank in reached {
            if left == 0 || !tanks.inputs.contains(&tank) || tank >= n {
                continue;
            }
            if !accepts(tank) {
                continue;
            }
            let entry = &mut state[tank];
            let fits = match pass {
                0 => entry.0.as_deref() == Some(fluid),
                _ => entry.0.is_none(),
            };
            if !fits {
                continue;
            }
            let room = tanks.tanks[tank] - entry.1;
            if room <= 0 {
                continue;
            }
            let add = room.min(left);
            entry.1 += add;
            left -= add;
            entry.0 = Some(fluid.to_owned());
        }
    }
    left
}

/// How much of `fluid` the side `tanks` names has room for: the fillable
/// tanks holding it or none, and only where `accepts` lets it in
/// (`native_block_entity_accepts_fluid`).
fn tank_room(tanks: &[NativeFluidTank], fluid: &str, accepts: impl Fn(usize) -> bool) -> i32 {
    tanks
        .iter()
        .filter(|t| t.can_fill && t.fluid.as_deref().is_none_or(|f| f == fluid) && accepts(t.tank))
        .fold(0_i32, |room, t| {
            room.saturating_add((t.capacity - t.amount).max(0))
        })
}

// Native mods use these; nothing in Horizon does.
impl Game<'_> {
    /// What holds fluids at `pos` in the level in place, `dimension`: a mod
    /// block's tanks (its block entity's, and not the placeholder standing in
    /// while it ticks), or a cauldron.
    fn native_fluid_at(&self, dimension: &str, pos: [i32; 3]) -> Option<FluidAt> {
        if !self.native_simulates() || dimension != self.dim.id() {
            return None;
        }
        let key = (pos[0], pos[1], pos[2]);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let info = self.pack.assets.registry.state(state)?;
        if !info.name.starts_with("minecraft:")
            && matches!(
                self.world.block_entities.get(&key),
                Some(horizon_client::containers::BlockEntity::Mod(m)) if !m.id.is_empty()
            )
        {
            let mut custom = false;
            self.native_custom_block_fluids(&info.name, &mut custom);
            if custom {
                return Some(FluidAt::Custom);
            }
        }
        if let Some(tanks) = self.world.block_entities.mod_fluids(state.0) {
            // The block entity is out of the world while it ticks.
            return matches!(
                self.world.block_entities.get(&key),
                Some(horizon_client::containers::BlockEntity::Mod(m)) if !m.id.is_empty()
            )
            .then(|| FluidAt::Mod {
                tanks: Arc::clone(tanks),
                block: info.name.clone(),
            });
        }
        match &*info.name {
            "minecraft:cauldron" => Some(FluidAt::Cauldron(CauldronFluids::Empty)),
            "minecraft:water_cauldron" => {
                let level = info
                    .properties
                    .get("level")
                    .and_then(|l| l.parse::<u8>().ok())?;
                Some(FluidAt::Cauldron(CauldronFluids::Water(level)))
            }
            "minecraft:lava_cauldron" => Some(FluidAt::Cauldron(CauldronFluids::Lava)),
            _ => None,
        }
    }
    /// The cauldron at `pos` set to `water` at `level` (0 is empty), or to
    /// lava. Whether it changed.
    fn cauldron_set(&mut self, pos: [i32; 3], water: Option<u8>, lava: bool) -> bool {
        let (name, level) = if lava {
            ("minecraft:lava_cauldron", None)
        } else {
            match water {
                Some(level) => ("minecraft:water_cauldron", Some(level)),
                None => ("minecraft:cauldron", None),
            }
        };
        let mut properties = std::collections::BTreeMap::new();
        if let Some(level) = level {
            properties.insert("level".to_owned(), level.to_string());
        }
        let state = horizon_world::chunk::BlockState {
            name: name.to_owned(),
            properties: properties.into(),
        };
        match self.pack.assets.registry.resolve(&state) {
            Ok(id) => self.level_set(pos, id).is_ok(),
            Err(_) => false,
        }
    }
    /// Whether tank `tank` of the mod block `block` takes `fluid`: a mod's
    /// answer to `native_block_entity_accepts_fluid`, true where none
    /// refuses it.
    fn native_fluid_accepts(&self, block: &str, tank: usize, fluid: &str) -> bool {
        let mut cancel = false;
        self.native_block_entity_accepts_fluid(block, tank, fluid, &mut cancel);
        !cancel
    }
    /// `native_fluid_insert`: up to `amount` millibuckets of `fluid`
    /// (`minecraft:water`, `minecraft:lava`, or since API
    /// @API:mod_fluids@ a mod's own, `create:chocolate`) into the block at
    /// `pos`, through its side `face`, in the level in place: a mod block's
    /// tanks first those with the same fluid, then empty ones, by the sides
    /// and tanks its table gave and every fillable tank that accepts the
    /// fluid (`native_block_entity_accepts_fluid`); a cauldron by thirds of a
    /// bucket, a lava one only whole, and none of a mod's fluid. What did not
    /// fit. API 54.
    pub(in crate::game) fn native_fluid_insert(
        &mut self,
        pos: [i32; 3],
        face: Direction,
        fluid: &str,
        amount: i32,
    ) -> i32 {
        if !tank_fluid(fluid) {
            return amount;
        }
        if !self.native_simulates() {
            return amount;
        }
        match self.native_fluid_at(self.dim.id(), pos) {
            Some(FluidAt::Custom) if amount > 0 => {
                let resource = NativeFluidStack {
                    fluid: fluid.to_owned(),
                    amount,
                    components: Compound::new(),
                };
                let mut accepted = None;
                self.native_custom_fluid_fill(
                    pos,
                    Some(face),
                    &resource,
                    NativeFluidAction::Execute,
                    &mut accepted,
                );
                amount - accepted.filter(|&n| (0..=amount).contains(&n)).unwrap_or(0)
            }
            Some(FluidAt::Mod { tanks, block }) => {
                let key = (pos[0], pos[1], pos[2]);
                let n = tanks.tanks.len();
                let reached = tanks.faces[face as usize].clone();
                let mut state = {
                    let Some(horizon_client::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get(&key)
                    else {
                        return amount;
                    };
                    read_tanks(&m.nbt, n)
                };
                let left = fill_tanks(&tanks, &mut state, &reached, fluid, amount, |tank| {
                    self.native_fluid_accepts(&block, tank, fluid)
                });
                if left < amount {
                    let Some(horizon_client::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get_mut(&key)
                    else {
                        return amount;
                    };
                    write_tanks(&mut m.nbt, &state);
                    self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                }
                left
            }
            Some(FluidAt::Cauldron(kind)) => match (kind, fluid) {
                (CauldronFluids::Empty, "minecraft:water")
                | (CauldronFluids::Water(_), "minecraft:water") => {
                    let level = match kind {
                        CauldronFluids::Water(level) => level,
                        _ => 0,
                    };
                    let given = amount.max(0);
                    let new_level = fill_level(level, given);
                    let consumed = third(new_level) - third(level);
                    if new_level > level && self.cauldron_set(pos, Some(new_level), false) {
                        given - consumed
                    } else {
                        amount
                    }
                }
                (CauldronFluids::Empty, "minecraft:lava") => {
                    if amount >= 1000 && self.cauldron_set(pos, None, true) {
                        amount - 1000
                    } else {
                        amount
                    }
                }
                // A full lava cauldron takes nothing, and water into lava
                // would turn it to stone in vanilla's world, not in a tank.
                _ => amount,
            },
            Some(FluidAt::Custom) | None => amount,
        }
    }
    /// The tanks of the block at `pos` in the level in place, `dimension`,
    /// that its side `face` reaches, with their fluids and amounts. None
    /// where no tanks are: another dimension, a chunk that isn't loaded, a
    /// block without tanks, or the mod block entity ticking now (it's out of
    /// the world). A cauldron reads as one tank of a bucket's capacity. API 54.
    pub(in crate::game) fn native_fluid_tanks(
        &self,
        dimension: &str,
        pos: [i32; 3],
        face: Direction,
    ) -> Option<Vec<NativeFluidTank>> {
        match self.native_fluid_at(dimension, pos)? {
            FluidAt::Custom => {
                let mut tanks = None;
                self.native_custom_fluid_tanks(pos, Some(face), &mut tanks);
                tanks?.into_iter().map(plain_tank).collect()
            }
            FluidAt::Mod { tanks, .. } => {
                let key = (pos[0], pos[1], pos[2]);
                let state = {
                    let Some(horizon_client::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get(&key)
                    else {
                        return None;
                    };
                    read_tanks(&m.nbt, tanks.tanks.len())
                };
                Some(
                    tanks.faces[face as usize]
                        .iter()
                        .map(|&tank| {
                            let (ref fluid, amount) = state[tank];
                            NativeFluidTank {
                                tank,
                                fluid: fluid.clone(),
                                amount,
                                capacity: tanks.tanks[tank],
                                can_fill: tanks.inputs.contains(&tank),
                                can_drain: tanks.outputs.contains(&tank),
                            }
                        })
                        .collect(),
                )
            }
            FluidAt::Cauldron(kind) => {
                let (fluid, amount, can_fill) = match kind {
                    CauldronFluids::Empty => (None, 0, true),
                    CauldronFluids::Water(level) => {
                        (Some("minecraft:water"), third(level), level < 3)
                    }
                    CauldronFluids::Lava => (Some("minecraft:lava"), 1000, false),
                };
                Some(vec![NativeFluidTank {
                    tank: 0,
                    fluid: fluid.map(str::to_owned),
                    amount,
                    capacity: 1000,
                    can_fill,
                    can_drain: amount > 0,
                }])
            }
        }
    }
    /// Takes up to `amount` millibuckets out of the block at `pos` in the
    /// level in place, `dimension`, through its side `face`: a mod block's
    /// drainable tanks, one fluid only (the first that's there), or a
    /// cauldron, which drains in thirds of a bucket and a lava one only
    /// whole. The fluid and how much; none when the side reaches nothing.
    /// API 54.
    pub(in crate::game) fn native_fluid_extract(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
        face: Direction,
        amount: i32,
    ) -> Option<(String, i32)> {
        if amount <= 0 {
            return None;
        }
        let storage = self.native_fluid_at(dimension, pos)?;
        if matches!(storage, FluidAt::Custom) {
            // The legacy interface cannot represent fluid components. Ask
            // without mutating storage, then request precisely what it saw.
            let mut preview = None;
            self.native_custom_fluid_drain(
                pos,
                Some(face),
                &NativeFluidDrain::Any(amount),
                NativeFluidAction::Simulate,
                &mut preview,
            );
            let preview = preview.filter(|s| plain_stack(s, amount))?;
            let mut result = None;
            self.native_custom_fluid_drain(
                pos,
                Some(face),
                &NativeFluidDrain::Exact(preview.clone()),
                NativeFluidAction::Execute,
                &mut result,
            );
            return result
                .filter(|s| plain_stack(s, preview.amount) && s.fluid == preview.fluid)
                .map(|s| (s.fluid, s.amount));
        }
        self.fluid_take(storage, pos, face, amount)
            .map(|(fluid, taken, _)| (fluid, taken))
    }
    /// `native_fluid_extract`'s drain, remembering what left which tank, so a
    /// move can put it back without the fill sides' leave: an empty list is a
    /// cauldron, whose own fill takes it back.
    fn fluid_take(
        &mut self,
        storage: FluidAt,
        pos: [i32; 3],
        face: Direction,
        amount: i32,
    ) -> Option<(String, i32, Vec<(usize, i32)>)> {
        if amount <= 0 {
            return None;
        }
        match storage {
            FluidAt::Custom => None,
            FluidAt::Mod { tanks, .. } => {
                let key = (pos[0], pos[1], pos[2]);
                let n = tanks.tanks.len();
                let mut state = {
                    let Some(horizon_client::containers::BlockEntity::Mod(m)) =
                        self.world.block_entities.get(&key)
                    else {
                        return None;
                    };
                    read_tanks(&m.nbt, n)
                };
                // The first drainable tank's fluid is all this call takes.
                let fluid: Option<String> = tanks.faces[face as usize]
                    .iter()
                    .filter(|&&tank| tanks.outputs.contains(&tank) && tank < n)
                    .find(|&&tank| state[tank].1 > 0)
                    .map(|&tank| state[tank].0.clone())?;
                let mut want = amount;
                let mut out = Vec::new();
                for &tank in &tanks.faces[face as usize] {
                    if want == 0 || !tanks.outputs.contains(&tank) || tank >= n {
                        continue;
                    }
                    if state[tank].0 != fluid {
                        continue;
                    }
                    let take = want.min(state[tank].1);
                    state[tank].1 -= take;
                    if state[tank].1 == 0 {
                        state[tank].0 = None;
                    }
                    want -= take;
                    out.push((tank, take));
                }
                let taken = amount - want;
                if taken == 0 {
                    return None;
                }
                let Some(horizon_client::containers::BlockEntity::Mod(m)) =
                    self.world.block_entities.get_mut(&key)
                else {
                    return None;
                };
                write_tanks(&mut m.nbt, &state);
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
                Some((fluid?, taken, out))
            }
            FluidAt::Cauldron(CauldronFluids::Water(level)) => {
                // A cauldron has no finer unit than its bottle's third, and
                // never hands out more than was asked: less than the topmost
                // whole third gives nothing.
                let (taken, left) = drain_take(level, amount);
                if taken > 0 && self.cauldron_set(pos, (left > 0).then_some(left), false) {
                    Some(("minecraft:water".to_owned(), taken, Vec::new()))
                } else {
                    None
                }
            }
            FluidAt::Cauldron(CauldronFluids::Lava) => (amount >= 1000
                && self.cauldron_set(pos, None, false))
            .then(|| ("minecraft:lava".to_owned(), 1000, Vec::new())),
            FluidAt::Cauldron(CauldronFluids::Empty) => None,
        }
    }
    /// Moves up to `max` millibuckets from the block at `from`, through its
    /// side `from_face`, into the one at `to`, through its side `to_face`, as
    /// `native_move_items` moves items: what the target won't take goes back
    /// into the source, so none is lost — a cauldron's whole thirds included.
    /// The fluid and how much moved; none where `native_fluid_tanks` is none
    /// at `from`, where nothing drainable is, where no block holds fluids at
    /// `to` or it has no room for that fluid (a tank that refuses it,
    /// `native_block_entity_accepts_fluid`, has none), and within one block.
    /// The source is asked for no more than the target's room. API 55.
    pub(in crate::game) fn native_move_fluids(
        &mut self,
        dimension: &str,
        from: [i32; 3],
        from_face: Direction,
        to: [i32; 3],
        to_face: Direction,
        max: i32,
    ) -> Option<(String, i32)> {
        if from == to {
            return None;
        }
        // Only built-in storage provides receipts that can restore the
        // exact drained tanks on failure.
        let source = self.native_fluid_at(dimension, from)?;
        if matches!(source, FluidAt::Custom) {
            return None;
        }
        // Ask the source for no more than the target has room for: a
        // cauldron drains in whole thirds and fills back only by them, so a
        // part the target refused couldn't always go back.
        let max = max.min(self.fluid_room(dimension, from, from_face, to, to_face)?);
        let (fluid, taken, receipt) = self.fluid_take(source, from, from_face, max)?;
        let left = self.native_fluid_insert(to, to_face, &fluid, taken);
        if left > 0 {
            // What the target wouldn't take goes back where it came from,
            // past the fill sides' leave: the tanks just gave it up, so they
            // have the room, and a cauldron levels back up by the thirds it
            // gave. What came back isn't moved: the answer is what the
            // target kept.
            let all_back = if receipt.is_empty() {
                self.native_fluid_insert(from, from_face, &fluid, left) == 0
            } else {
                self.fluid_return(dimension, from, &fluid, left, &receipt)
            };
            if !all_back {
                log::warn!("Horizon: a fluid move left {left} mB of {fluid} nowhere to go");
            }
        }
        Some((fluid, taken - left))
    }
    /// How much of the fluid `from`'s side `from_face` would give first fits
    /// into `to` through `to_face`: the room of its fillable tanks holding
    /// that fluid or none. None when `from` gives nothing or `to` holds no
    /// fluids.
    fn fluid_room(
        &self,
        dimension: &str,
        from: [i32; 3],
        from_face: Direction,
        to: [i32; 3],
        to_face: Direction,
    ) -> Option<i32> {
        let fluid = self
            .native_fluid_tanks(dimension, from, from_face)?
            .into_iter()
            .find(|t| t.can_drain && t.amount > 0)?
            .fluid?;
        let block = match self.native_fluid_at(dimension, to)? {
            FluidAt::Custom => return None,
            FluidAt::Mod { block, .. } => Some(block),
            FluidAt::Cauldron(_) => None,
        };
        // A cauldron reports itself fillable, but holds only vanilla's fluids:
        // no room for a mod's, so a move answers none rather than draining
        // and putting it all back.
        if block.is_none() && !matches!(fluid.as_str(), "minecraft:water" | "minecraft:lava") {
            return Some(0);
        }
        let room = tank_room(
            &self.native_fluid_tanks(dimension, to, to_face)?,
            &fluid,
            |tank| {
                block
                    .as_deref()
                    .is_none_or(|block| self.native_fluid_accepts(block, tank, &fluid))
            },
        );
        Some(room)
    }
    /// Puts `amount` of `fluid` back into the tanks `receipt` names, drained
    /// moments ago: their room is exactly what left them. Whether it all went
    /// back.
    fn fluid_return(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
        fluid: &str,
        mut amount: i32,
        receipt: &[(usize, i32)],
    ) -> bool {
        let Some(FluidAt::Mod { tanks, .. }) = self.native_fluid_at(dimension, pos) else {
            return false;
        };
        let key = (pos[0], pos[1], pos[2]);
        let Some(horizon_client::containers::BlockEntity::Mod(m)) =
            self.world.block_entities.get_mut(&key)
        else {
            return false;
        };
        // The declared count, as `fluid_take` read them: a shorter read
        // would write the other tanks away.
        let n = tanks.tanks.len();
        let mut state = read_tanks(&m.nbt, n);
        for &(tank, took) in receipt {
            let give = took.min(amount);
            if give <= 0 || tank >= n {
                continue;
            }
            state[tank].1 += give;
            state[tank].0 = Some(fluid.to_owned());
            amount -= give;
        }
        write_tanks(&mut m.nbt, &state);
        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
        amount == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "prepared resources in HORIZON_TEST_RESOURCES; run alone to isolate the mod registry"]
    fn the_standard_helpers_route_to_native_owned_storage() {
        use crate::game::{Audio, Options, Progress, Settings, World, load_resources};
        use crate::mod_api::{
            self, CustomBlockFluidsEvent, CustomFluidDrainEvent, CustomFluidFillEvent,
            CustomFluidTanksEvent, Hooks, Host,
        };
        use horizon_client::chunk::Chunk;
        use horizon_client::containers::{BlockEntity, ModBlockEntityFields};
        use horizon_client::mod_content::{ContentMod, build};
        use horizon_mods::content::{Block, Content};
        use std::sync::Mutex;

        const BLOCK: &str = "fixture:tank";
        const POS: [i32; 3] = [2, 64, 2];
        const DIM: &str = "minecraft:overworld";
        struct Tank(Mutex<NativeFluidStack>);
        impl Hooks for Arc<Tank> {
            fn custom_block_fluids(&self, _: &dyn Host, event: &mut CustomBlockFluidsEvent<'_>) {
                event.custom |= event.block == BLOCK;
            }
            fn custom_fluid_tanks(
                &self,
                _: &dyn Host,
                event: &mut CustomFluidTanksEvent,
            ) -> Option<Vec<NativeFluidTankView>> {
                assert_eq!((event.pos, event.side), (POS, Some(Direction::Up)));
                Some(vec![NativeFluidTankView {
                    tank: 0,
                    content: Some(self.0.lock().unwrap().clone()),
                    capacity: 1000,
                    can_fill: true,
                    can_drain: true,
                }])
            }
            fn custom_fluid_fill(
                &self,
                _: &mut dyn Host,
                event: &mut CustomFluidFillEvent<'_>,
            ) -> Option<i32> {
                assert_eq!((event.pos, event.side), (POS, Some(Direction::Up)));
                assert_eq!(event.action, NativeFluidAction::Execute);
                let mut tank = self.0.lock().unwrap();
                assert_eq!(event.resource.fluid, tank.fluid);
                let amount = event.resource.amount.min(1000 - tank.amount);
                tank.amount += amount;
                Some(amount)
            }
            fn custom_fluid_drain(
                &self,
                _: &mut dyn Host,
                event: &mut CustomFluidDrainEvent<'_>,
            ) -> Option<NativeFluidStack> {
                let mut tank = self.0.lock().unwrap();
                let mut drained = tank.clone();
                match (event.action, event.request) {
                    (NativeFluidAction::Simulate, NativeFluidDrain::Any(max)) => {
                        drained.amount = tank.amount.min(*max);
                    }
                    (NativeFluidAction::Execute, NativeFluidDrain::Exact(exact)) => {
                        assert_eq!(exact.fluid, tank.fluid);
                        drained.amount = tank.amount.min(exact.amount);
                        tank.amount -= drained.amount;
                    }
                    _ => panic!("unexpected drain mode"),
                }
                Some(drained)
            }
        }
        let base = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let scratch =
            std::env::temp_dir().join(format!("horizon-fluid-provider-{}", std::process::id()));
        let mut content = Content::declared("fixture");
        content
            .declare(
                &[Block {
                    id: BLOCK.into(),
                    template: "minecraft:stone".into(),
                    ..Default::default()
                }],
                &[],
            )
            .unwrap();
        build(
            &base,
            &[ContentMod {
                id: "fixture".into(),
                content,
            }],
            &scratch,
        )
        .unwrap();
        let settings = Settings::default();
        let loaded = load_resources(
            scratch.clone(),
            None,
            &scratch,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = scratch.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&scratch),
            None,
        )
        .unwrap();
        game.options.dedicated = Some(Vec::new());
        game.world = World::new(game.res.facts().air, 0);
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let state = game.pack.assets.registry.default_state(BLOCK).unwrap();
        game.world.set(POS[0], POS[1], POS[2], state).unwrap();
        game.world.block_entities.insert(
            (2, 64, 2),
            BlockEntity::Mod(Box::new(
                ModBlockEntityFields {
                    id: BLOCK.into(),
                    ..Default::default()
                }
                .into(),
            )),
        );

        let tank = Arc::new(Tank(Mutex::new(NativeFluidStack {
            fluid: "minecraft:water".into(),
            amount: 750,
            components: Compound::new(),
        })));
        assert!(
            mod_api::MODS
                .set(Arc::from(vec![Box::new(tank.clone()) as Box<dyn Hooks>]))
                .is_ok()
        );
        let top = Direction::Up;
        assert_eq!(
            game.native_fluid_tanks(DIM, POS, top).unwrap()[0].amount,
            750
        );
        assert_eq!(
            game.native_fluid_insert(POS, top, "minecraft:water", 400),
            150
        );
        assert_eq!(
            game.native_fluid_extract(DIM, POS, top, 300),
            Some(("minecraft:water".into(), 300))
        );
        assert_eq!(tank.0.lock().unwrap().amount, 700);
        assert!(
            game.native_move_fluids(DIM, POS, top, [3, 64, 2], top, 50)
                .is_none()
        );
        assert_eq!(tank.0.lock().unwrap().amount, 700);
        tank.0
            .lock()
            .unwrap()
            .components
            .insert("fixture:color".into(), Tag::Int(2));
        assert!(game.native_fluid_extract(DIM, POS, top, 50).is_none());
        assert_eq!(tank.0.lock().unwrap().amount, 700);
        drop(game);
        std::fs::remove_dir_all(scratch).unwrap();
    }

    /// Tanks hold vanilla's two fluids and any of a mod's own; not a
    /// made-up vanilla one, nor a name that isn't an identifier.
    #[test]
    fn a_tank_holds_vanilla_fluids_and_mods_own() {
        for fluid in [
            "minecraft:water",
            "minecraft:lava",
            "create:chocolate",
            "create:potion/healing",
        ] {
            assert!(tank_fluid(fluid), "{fluid}");
        }
        for fluid in [
            "minecraft:milk",
            "minecraft:empty",
            "chocolate",
            ":x",
            "create:",
            "Create:honey",
            "create:honey cake",
        ] {
            assert!(!tank_fluid(fluid), "{fluid}");
        }
    }

    #[test]
    fn a_cauldron_level_holds_a_third_of_a_bucket() {
        assert_eq!(third(1), 333);
        assert_eq!(third(2), 666);
        assert_eq!(third(3), 1000);
    }

    #[test]
    fn a_cauldron_never_hands_out_more_than_was_asked() {
        // The top third of a full cauldron is 334, then 333, 333.
        assert_eq!(drain_take(3, 1), (0, 3));
        assert_eq!(drain_take(3, 333), (0, 3));
        assert_eq!(drain_take(3, 334), (334, 2));
        assert_eq!(drain_take(3, 500), (334, 2));
        assert_eq!(drain_take(3, 666), (334, 2));
        assert_eq!(drain_take(3, 667), (667, 1));
        assert_eq!(drain_take(3, 1000), (1000, 0));
        assert_eq!(drain_take(1, 332), (0, 1));
        assert_eq!(drain_take(1, 333), (333, 0));
        assert_eq!(drain_take(1, i32::MAX), (333, 0));
    }

    #[test]
    fn a_cauldron_fill_stays_within_a_bucket_whatever_the_amount() {
        assert_eq!(fill_level(0, 332), 0);
        assert_eq!(fill_level(0, 333), 1);
        assert_eq!(fill_level(1, 332), 1);
        assert_eq!(fill_level(1, 333), 2);
        assert_eq!(fill_level(1, 666), 2);
        assert_eq!(fill_level(1, 667), 3);
        assert_eq!(fill_level(0, i32::MAX), 3);
        assert_eq!(fill_level(2, i32::MAX), 3);
    }

    fn tanks(list: &[(Option<&str>, i32)]) -> Vec<Tank> {
        list.iter()
            .map(|&(fluid, amount)| (fluid.map(str::to_owned), amount))
            .collect()
    }

    #[test]
    fn tank_state_round_trips_through_nbt_and_missing_entries_read_empty() {
        let mut nbt = Compound::new();
        let state = tanks(&[(Some("minecraft:water"), 3334), (None, 0)]);
        write_tanks(&mut nbt, &state);
        assert_eq!(read_tanks(&nbt, 2), state);
        // A tank the save never heard of reads as empty, not as gone.
        let longer = read_tanks(&nbt, 3);
        assert_eq!(longer[2], (None, 0));
        // Anything else under `fluids` is the mod's own, not a tank list.
        nbt.insert(JavaString::from("fluids"), Tag::Int(0));
        assert_eq!(read_tanks(&nbt, 2), vec![(None, 0), (None, 0)]);
    }

    #[test]
    fn a_shrunk_tank_table_keeps_the_tanks_that_still_exist() {
        // A mod update that declares one tank where the save has two keeps
        // the first tank's contents, instead of reading all as empty and
        // writing the wipe back.
        let mut nbt = Compound::new();
        let saved = tanks(&[
            (Some("minecraft:water"), 4000),
            (Some("minecraft:lava"), 1000),
        ]);
        write_tanks(&mut nbt, &saved);
        assert_eq!(
            read_tanks(&nbt, 1),
            tanks(&[(Some("minecraft:water"), 4000)])
        );
    }

    #[test]
    fn a_receipts_room_is_what_left_its_tanks() {
        // The bookkeeping a move's put-back trusts: a receipt never names
        // more than its tanks held, so returning it always fits.
        let receipt = vec![(1usize, 333i32), (2, 100)];
        let held = tanks(&[
            (None, 0),
            (Some("minecraft:water"), 333),
            (Some("minecraft:water"), 100),
        ]);
        assert!(receipt.iter().all(|&(tank, took)| took <= held[tank].1));
        let mut after = held.clone();
        for &(tank, took) in &receipt {
            after[tank].1 -= took;
            if after[tank].1 == 0 {
                after[tank].0 = None;
            }
        }
        assert_eq!(after, tanks(&[(None, 0), (None, 0), (None, 0)]));
        // And putting it back restores exactly what was there.
        let mut back = after;
        for &(tank, took) in &receipt {
            back[tank].1 += took;
            back[tank].0 = Some("minecraft:water".to_owned());
        }
        assert_eq!(back, held);
    }

    fn tank_table(capacities: &[i32], inputs: &[usize]) -> ModFluidTanks {
        ModFluidTanks {
            tanks: capacities.to_vec(),
            inputs: inputs.to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn a_refused_tank_takes_no_fluid_in_either_insert_pass() {
        let table = tank_table(&[4000, 4000], &[0, 1]);
        // Pass 0 tops up a tank already holding the fluid; refused, it is
        // left exactly as it was.
        let mut state = tanks(&[(Some("create:chocolate"), 1000), (None, 0)]);
        assert_eq!(
            fill_tanks(&table, &mut state, &[0], "create:chocolate", 500, |_| false),
            500
        );
        assert_eq!(state[0], (Some("create:chocolate".to_owned()), 1000));
        // Pass 1 fills an empty tank; refused, it stays empty.
        let mut state = tanks(&[(None, 0), (None, 0)]);
        assert_eq!(
            fill_tanks(&table, &mut state, &[1], "create:chocolate", 500, |_| false),
            500
        );
        assert_eq!(state[1], (None, 0));
        // Only the refusing tank is skipped: the other still takes the fluid.
        let mut state = tanks(&[(Some("create:chocolate"), 1000), (None, 0)]);
        assert_eq!(
            fill_tanks(
                &table,
                &mut state,
                &[0, 1],
                "create:chocolate",
                500,
                |tank| { tank == 1 }
            ),
            0
        );
        assert_eq!(
            state,
            tanks(&[
                (Some("create:chocolate"), 1000),
                (Some("create:chocolate"), 500),
            ])
        );
    }

    #[test]
    fn every_tank_takes_every_fluid_unless_it_refuses_it() {
        let table = tank_table(&[4000, 4000], &[0, 1]);
        let mut state = tanks(&[(None, 0), (None, 0)]);
        assert_eq!(
            fill_tanks(
                &table,
                &mut state,
                &[0, 1],
                "create:chocolate",
                1000,
                |_| true
            ),
            0
        );
        assert_eq!(state[0], (Some("create:chocolate".to_owned()), 1000));
        assert_eq!(state[1], (None, 0));
    }

    #[test]
    fn a_move_finds_room_only_in_tanks_that_accept_the_fluid() {
        let tank = NativeFluidTank {
            tank: 0,
            fluid: None,
            amount: 1000,
            capacity: 4000,
            can_fill: true,
            can_drain: true,
        };
        // A tank that refuses the fluid has no room, so `native_move_fluids`
        // asks its source for nothing rather than draining and putting it all
        // back.
        assert_eq!(
            tank_room(std::slice::from_ref(&tank), "create:chocolate", |_| false),
            0
        );
        assert_eq!(tank_room(&[tank], "create:chocolate", |_| true), 3000);
    }

    /// The hook the game asks (`native_block_entity_accepts_fluid`) through
    /// the same public dispatch `Game::native_fluid_accepts` calls: a water
    /// tank refuses a mod's fluid, so neither insert pass fills it and it
    /// offers no room to `native_move_fluids`; water still goes in, and with
    /// no mod every tank takes every fluid.
    #[test]
    fn a_hook_that_refuses_a_fluid_leaves_both_passes_empty_and_offers_no_room() {
        use horizon_client::mod_api::{
            BlockEntityAcceptsFluidEvent, Flow, Hooks, Host, dispatch_block_entity_accepts_fluid,
        };
        struct TestHost;
        impl Host for TestHost {}
        struct WaterTank;
        impl Hooks for WaterTank {
            fn block_entity_accepts_fluid(
                &self,
                _host: &dyn Host,
                event: &mut BlockEntityAcceptsFluidEvent<'_>,
            ) -> Flow {
                if event.block == "mymod:water_tank" && event.fluid != "minecraft:water" {
                    Flow::Cancel
                } else {
                    Flow::Continue
                }
            }
        }
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(WaterTank)];
        let accepts = |tank: usize| {
            let mut cancel = false;
            dispatch_block_entity_accepts_fluid(
                &TestHost,
                &mods,
                "mymod:water_tank",
                tank,
                "create:chocolate",
                &mut cancel,
            );
            !cancel
        };
        let table = tank_table(&[4000, 4000], &[0, 1]);
        // Pass 0 tops up a tank already holding the fluid: refused, it keeps
        // exactly what it had.
        let mut state = tanks(&[(Some("create:chocolate"), 1000), (None, 0)]);
        assert_eq!(
            fill_tanks(&table, &mut state, &[0], "create:chocolate", 500, accepts),
            500
        );
        assert_eq!(state[0], (Some("create:chocolate".to_owned()), 1000));
        // Pass 1 fills an empty tank: refused, it stays empty.
        let mut state = tanks(&[(None, 0), (None, 0)]);
        assert_eq!(
            fill_tanks(&table, &mut state, &[1], "create:chocolate", 500, accepts),
            500
        );
        assert_eq!(state[1], (None, 0));
        // `native_move_fluids` counts no room in the tank that refuses it, so
        // it asks its source for nothing.
        let tank = NativeFluidTank {
            tank: 0,
            fluid: None,
            amount: 1000,
            capacity: 4000,
            can_fill: true,
            can_drain: true,
        };
        assert_eq!(
            tank_room(std::slice::from_ref(&tank), "create:chocolate", accepts),
            0
        );
        // The same tank takes water (the hook lets it through) and, with no
        // mod at all, every fluid: existing mods are unchanged.
        let water = |tank: usize| {
            let mut cancel = false;
            dispatch_block_entity_accepts_fluid(
                &TestHost,
                &mods,
                "mymod:water_tank",
                tank,
                "minecraft:water",
                &mut cancel,
            );
            !cancel
        };
        assert_eq!(
            tank_room(std::slice::from_ref(&tank), "minecraft:water", water),
            3000
        );
        let none: [Box<dyn Hooks>; 0] = [];
        let no_mod = |tank: usize| {
            let mut cancel = false;
            dispatch_block_entity_accepts_fluid(
                &TestHost,
                &none,
                "mymod:water_tank",
                tank,
                "create:chocolate",
                &mut cancel,
            );
            !cancel
        };
        let mut state = tanks(&[(None, 0), (None, 0)]);
        assert_eq!(
            fill_tanks(
                &table,
                &mut state,
                &[0, 1],
                "create:chocolate",
                1000,
                no_mod
            ),
            0
        );
        assert_eq!(state[0], (Some("create:chocolate".to_owned()), 1000));
        assert_eq!(
            tank_room(std::slice::from_ref(&tank), "create:chocolate", no_mod),
            3000
        );
    }

    #[test]
    fn an_empty_tank_writes_the_empty_fluid_not_none() {
        let mut nbt = Compound::new();
        write_tanks(
            &mut nbt,
            &tanks(&[(Some("minecraft:lava"), 1000), (None, 0)]),
        );
        let Some(Tag::List(list)) = nbt.get(&JavaString::from("fluids")) else {
            panic!("no list");
        };
        let Some(Tag::Compound(empty)) = list.get(1) else {
            panic!("no compound");
        };
        assert_eq!(
            empty.get(&JavaString::from("Fluid")),
            Some(&Tag::String(JavaString::from("minecraft:empty")))
        );
    }
}
