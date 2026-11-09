//! Sculk: the sensor's phases, the shrieker and catalyst blocks, and the
//! level `SculkSpreader` with the sculk, sculk vein and default charge-cursor
//! behaviours (`SculkBlock`, `SculkVeinBlock`, `SculkBehaviour`).
use super::{
    Behaviours, LevelAccess, LevelEvent, Sim, Ticks,
    classes::Class,
    update::{above, below, rel},
};
use crate::{fluid::FluidState, registry::BlockStateId};
use horizon_core::{geometry::Direction, position::BlockPos, random::RandomSource};

/// `SculkSensorBlock.ACTIVE_TICKS` (calibrated sensors use 10).
pub const SENSOR_ACTIVE_TICKS: i32 = 30;
pub const CALIBRATED_ACTIVE_TICKS: i32 = 10;
/// `SculkSensorBlock.COOLDOWN_TICKS`.
pub const SENSOR_COOLDOWN_TICKS: i32 = 10;
/// `SculkShriekerBlockEntity` shrieking time.
pub const SHRIEKING_TICKS: i32 = 90;
/// `SculkCatalystBlockEntity.CatalystListener.PULSE_TICKS`.
pub const PULSE_TICKS: i32 = 8;

// SculkSpreader.createLevelSpreader.
const GROWTH_SPAWN_COST: i32 = 10;
const NO_GROWTH_RADIUS: i32 = 4;
const CHARGE_DECAY_RATE: i32 = 10;
const ADDITIONAL_DECAY_RATE: i32 = 5;
const MAX_CURSORS: usize = 32;
const MAX_CHARGE: i32 = 1000;
const MAX_CURSOR_DISTANCE: i32 = 1024;

/// One `SculkSpreader.ChargeCursor`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChargeCursor {
    pub pos: BlockPos,
    pub charge: i32,
    pub update_delay: i32,
    pub decay_delay: i32,
    /// The faces of the block the cursor last moved onto, if any.
    pub facings: Option<Vec<Direction>>,
}
impl ChargeCursor {
    pub fn new(pos: BlockPos, charge: i32) -> Self {
        Self {
            pos,
            charge,
            update_delay: 0,
            decay_delay: 1,
            facings: None,
        }
    }
}
/// A catalyst's `SculkSpreader` (the level configuration).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SculkSpreader {
    pub cursors: Vec<ChargeCursor>,
}
impl SculkSpreader {
    /// `addCursors`: charge in chunks of at most 1000, at most 32 cursors.
    pub fn add_cursors(&mut self, start: BlockPos, mut charge: i32) {
        while charge > 0 {
            let current = charge.min(MAX_CHARGE);
            self.add_cursor(ChargeCursor::new(start, current));
            charge -= current;
        }
    }
    pub fn add_cursor(&mut self, cursor: ChargeCursor) {
        if self.cursors.len() < MAX_CURSORS {
            self.cursors.push(cursor);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    Default,
    Sculk,
    Vein,
}
#[derive(Clone, Copy)]
enum Spread {
    SamePosition,
    SamePlane,
    WrapAround,
}
const ALL_SPREADS: [Spread; 3] = [Spread::SamePosition, Spread::SamePlane, Spread::WrapAround];

/// `MultifaceBlock.pack`: the faces as bits of their ordinals.
pub fn pack_faces(faces: &[Direction]) -> i32 {
    faces.iter().fold(0, |code, d| code | 1 << *d as i32)
}
/// `MultifaceBlock.unpack`.
pub fn unpack_faces(data: i32) -> Vec<Direction> {
    Direction::ALL
        .into_iter()
        .filter(|d| data & (1 << *d as i32) != 0)
        .collect()
}

impl<L: LevelAccess + ?Sized> Sim<'_, L> {
    fn sculk_rand(&mut self, bound: i32) -> i32 {
        self.random().next_int_bound(bound).unwrap_or(0)
    }
    fn sculk_in_tag(&self, state: BlockStateId, tag: &str) -> bool {
        self.b.tag(tag).is_some_and(|set| self.b.in_set(state, set))
    }
    fn sculk_sound(&mut self, pos: BlockPos, sound: &str, volume: f32, pitch: f32) {
        self.emit(LevelEvent::Sound {
            pos,
            sound: sound.into(),
            volume,
            pitch,
        });
    }

    // ---- sensors ----

    /// `SculkSensorBlock.canActivate`.
    pub(crate) fn sensor_phase(&self, state: BlockStateId) -> &'static str {
        match self.b.table.text(state, self.b.props.sculk_sensor_phase) {
            "active" => "active",
            "cooldown" => "cooldown",
            _ => "inactive",
        }
    }
    fn sensor_update_neighbours(&mut self, pos: BlockPos, state: BlockStateId) {
        let block = self.b.block(state);
        self.update_neighbors_at(pos, block);
        self.update_neighbors_at(below(pos), block);
    }
    /// `SculkSensorBlock.tick`.
    pub(crate) fn sculk_sensor_tick(&mut self, pos: BlockPos, state: BlockStateId) {
        let b = self.b;
        match self.sensor_phase(state) {
            "active" => self.sculk_sensor_deactivate(pos, state),
            "cooldown" => {
                let next = b
                    .table
                    .set_text(state, b.props.sculk_sensor_phase, "inactive");
                self.set_block_and_update(pos, next);
                if !b.table.bool(state, b.props.waterlogged) {
                    let pitch = self.random().next_float() * 0.2 + 0.8;
                    self.sculk_sound(
                        pos,
                        "minecraft:block.sculk_sensor.clicking_stop",
                        1.0,
                        pitch,
                    );
                }
            }
            _ => {}
        }
    }
    /// `SculkSensorBlock.deactivate`.
    pub(crate) fn sculk_sensor_deactivate(&mut self, pos: BlockPos, state: BlockStateId) {
        let b = self.b;
        let next = b.table.set_int(
            b.table
                .set_text(state, b.props.sculk_sensor_phase, "cooldown"),
            b.props.power,
            0,
        );
        self.set_block_and_update(pos, next);
        let block = b.block(state);
        self.schedule_tick(pos, block, SENSOR_COOLDOWN_TICKS);
        self.sensor_update_neighbours(pos, state);
    }
    /// `SculkSensorBlock.activate` without the resonance and the tendril
    /// event, which need the source entity.
    pub(crate) fn sculk_sensor_activate(
        &mut self,
        pos: BlockPos,
        state: BlockStateId,
        power: i32,
        active_ticks: i32,
    ) {
        let b = self.b;
        let next = b.table.set_int(
            b.table
                .set_text(state, b.props.sculk_sensor_phase, "active"),
            b.props.power,
            power,
        );
        self.set_block_and_update(pos, next);
        let block = b.block(state);
        self.schedule_tick(pos, block, active_ticks);
        self.sensor_update_neighbours(pos, state);
    }
    /// The activation sound, played after the tendril event.
    pub(crate) fn sculk_sensor_click(&mut self, pos: BlockPos, state: BlockStateId) {
        if !self.b.table.bool(state, self.b.props.waterlogged) {
            let pitch = self.random().next_float() * 0.2 + 0.8;
            self.sculk_sound(pos, "minecraft:block.sculk_sensor.clicking", 1.0, pitch);
        }
    }

    // ---- shrieker and catalyst ----

    /// `SculkShriekerBlock.tick`: the shriek ends and the block responds.
    pub(crate) fn sculk_shrieker_tick(&mut self, pos: BlockPos, state: BlockStateId) {
        let b = self.b;
        if b.table.bool(state, b.props.shrieking) {
            let next = b.table.set_bool(state, b.props.shrieking, false);
            self.set_block_and_update(pos, next);
            self.emit(LevelEvent::BlockEvent {
                pos,
                state: next,
                kind: "shrieker_respond",
            });
        }
    }
    /// `SculkCatalystBlock.tick`.
    pub(crate) fn sculk_catalyst_tick(&mut self, pos: BlockPos, state: BlockStateId) {
        let b = self.b;
        if b.table.bool(state, b.props.bloom) {
            let next = b.table.set_bool(state, b.props.bloom, false);
            self.set_block_and_update(pos, next);
        }
    }

    // ---- the spreader ----

    fn sculk_behaviour(&self, state: BlockStateId) -> Behaviour {
        let block = self.b.block(state);
        if block == self.b.named.sculk {
            Behaviour::Sculk
        } else if block == self.b.named.sculk_vein {
            Behaviour::Vein
        } else {
            Behaviour::Default
        }
    }
    fn is_vein(&self, state: BlockStateId) -> bool {
        self.b.block(state) == self.b.named.sculk_vein
    }
    fn vein_has_face(&self, state: BlockStateId, face: Direction) -> bool {
        self.is_vein(state) && self.multiface_has_face(state, face)
    }
    fn vein_with_face(&self, state: BlockStateId, face: Direction, value: bool) -> BlockStateId {
        self.b
            .table
            .set_bool(state, self.direction_prop(face), value)
    }
    fn available_faces(&self, state: BlockStateId) -> Vec<Direction> {
        if !self.b.instance_of(state, Class::Multiface) {
            return Vec::new();
        }
        Direction::ALL
            .into_iter()
            .filter(|d| self.multiface_has_face(state, *d))
            .collect()
    }
    fn is_sculk_source(&self, state: BlockStateId) -> bool {
        self.sculk_behaviour(state) != Behaviour::Default
    }

    /// `SculkSpreader.updateCursors`.
    pub(crate) fn sculk_update_cursors(
        &mut self,
        spreader: &mut SculkSpreader,
        origin: BlockPos,
        spread_veins: bool,
    ) {
        if spreader.cursors.is_empty() {
            return;
        }
        let cursors = std::mem::take(&mut spreader.cursors);
        let mut processed: Vec<ChargeCursor> = Vec::new();
        // The cursor each position merges into (index into `processed`).
        let mut mergeable: Vec<(BlockPos, usize)> = Vec::new();
        let mut charge_map: Vec<(BlockPos, i32)> = Vec::new();
        for mut cursor in cursors {
            let distance = (cursor.pos.x.wrapping_sub(origin.x).wrapping_abs())
                .max(cursor.pos.y.wrapping_sub(origin.y).wrapping_abs())
                .max(cursor.pos.z.wrapping_sub(origin.z).wrapping_abs());
            if distance > MAX_CURSOR_DISTANCE {
                continue;
            }
            self.sculk_cursor_update(&mut cursor, origin, spread_veins);
            if cursor.charge <= 0 {
                self.level_event(3006, cursor.pos, 0);
                continue;
            }
            let pos = cursor.pos;
            match charge_map.iter_mut().find(|(p, _)| *p == pos) {
                Some(entry) => entry.1 += cursor.charge,
                None => charge_map.push((pos, cursor.charge)),
            }
            match mergeable.iter().position(|(p, _)| *p == pos) {
                None => {
                    mergeable.push((pos, processed.len()));
                    processed.push(cursor);
                }
                Some(slot) => {
                    let index = mergeable[slot].1;
                    if cursor.charge + processed[index].charge <= MAX_CHARGE {
                        // mergeWith
                        processed[index].charge += cursor.charge;
                        processed[index].update_delay =
                            processed[index].update_delay.min(cursor.update_delay);
                    } else {
                        let smaller = cursor.charge < processed[index].charge;
                        processed.push(cursor);
                        if smaller {
                            mergeable[slot].1 = processed.len() - 1;
                        }
                    }
                }
            }
        }
        for (pos, charge) in charge_map {
            let faces = mergeable
                .iter()
                .find(|(p, _)| *p == pos)
                .and_then(|(_, i)| processed[*i].facings.clone());
            if charge > 0
                && let Some(faces) = faces
            {
                let particles =
                    (horizon_core::jmath::log(f64::from(charge) + 1.0) as f32 / 2.3) as i32 + 1;
                let data = (particles << 6) + pack_faces(&faces);
                self.level_event(3006, pos, data);
            }
        }
        spreader.cursors = processed;
    }

    /// `ChargeCursor.update`.
    fn sculk_cursor_update(&mut self, cursor: &mut ChargeCursor, origin: BlockPos, veins: bool) {
        // shouldUpdate: charge left and the origin's chunk is ticking.
        if cursor.charge <= 0 || !self.level.is_ticking_chunk(origin.chunk()) {
            return;
        }
        if cursor.update_delay > 0 {
            cursor.update_delay -= 1;
            return;
        }
        let mut state = self.state(cursor.pos);
        let mut behaviour = self.sculk_behaviour(state);
        if veins && self.sculk_attempt_spread_vein(behaviour, cursor.pos, state, &cursor.facings) {
            // SculkBlock.canChangeBlockStateOnSpread is false.
            if behaviour != Behaviour::Sculk {
                state = self.state(cursor.pos);
                behaviour = self.sculk_behaviour(state);
            }
            self.sculk_sound(cursor.pos, "minecraft:block.sculk.spread", 1.0, 1.0);
        }
        cursor.charge = self.sculk_attempt_use_charge(behaviour, cursor, origin, veins);
        if cursor.charge <= 0 {
            self.sculk_on_discharged(behaviour, state, cursor.pos);
            return;
        }
        let transfer = self.sculk_valid_movement(cursor.pos);
        if let Some(target) = transfer {
            self.sculk_on_discharged(behaviour, state, cursor.pos);
            cursor.pos = target;
            state = self.state(target);
        }
        if self.is_sculk_source(state) {
            cursor.facings = Some(self.available_faces(state));
        }
        cursor.decay_delay = match behaviour {
            Behaviour::Default => (cursor.decay_delay - 1).max(0),
            _ => 1,
        };
        cursor.update_delay = 1;
    }

    /// `SculkBehaviour.attemptSpreadVein` (the default behaviour's override
    /// regrows veins or spreads in the same space).
    fn sculk_attempt_spread_vein(
        &mut self,
        behaviour: Behaviour,
        pos: BlockPos,
        state: BlockStateId,
        facings: &Option<Vec<Direction>>,
    ) -> bool {
        if behaviour == Behaviour::Default {
            match facings {
                None => {
                    let current = self.state(pos);
                    return self.sculk_spread_all(current, pos, &[Spread::SamePosition]) > 0;
                }
                Some(faces) if !faces.is_empty() => {
                    let b = self.b;
                    let water = b.is_block(state, b.named.water) && b.fluid(state).is_source();
                    if !b.is_air(state) && !water {
                        return false;
                    }
                    return self.sculk_regrow(pos, state, faces);
                }
                Some(_) => {}
            }
        }
        self.sculk_spread_all(state, pos, &ALL_SPREADS) > 0
    }
    /// `SculkVeinBlock.regrow`.
    fn sculk_regrow(&mut self, pos: BlockPos, existing: BlockStateId, faces: &[Direction]) -> bool {
        let b = self.b;
        let mut state = b.default_of(b.named.sculk_vein);
        let mut any = false;
        for &face in faces {
            if b.multiface_can_attach(self.state(rel(pos, face)), face) {
                state = self.vein_with_face(state, face, true);
                any = true;
            }
        }
        if !any {
            return false;
        }
        if b.fluid(existing) != FluidState::EMPTY {
            state = b.table.set_bool(state, b.props.waterlogged, true);
        }
        self.set_block_and_update(pos, state);
        true
    }
    /// `MultifaceSpreader.spreadAll` with the sculk vein configuration.
    fn sculk_spread_all(&mut self, state: BlockStateId, pos: BlockPos, types: &[Spread]) -> i64 {
        let other_source = !self.is_vein(state);
        let mut count = 0;
        for face in Direction::ALL {
            if !(other_source || self.vein_has_face(state, face)) {
                continue;
            }
            for direction in Direction::ALL {
                if crate::level::query::axis_of(direction) == crate::level::query::axis_of(face) {
                    continue;
                }
                if !(other_source
                    || (self.vein_has_face(state, face) && !self.vein_has_face(state, direction)))
                {
                    continue;
                }
                let mut target = None;
                for kind in types {
                    let candidate = match kind {
                        Spread::SamePosition => (pos, direction),
                        Spread::SamePlane => (rel(pos, direction), face),
                        Spread::WrapAround => {
                            (rel(rel(pos, direction), face), direction.opposite())
                        }
                    };
                    if self.sculk_can_spread_into(pos, candidate.0, candidate.1) {
                        target = Some(candidate);
                        break;
                    }
                }
                let Some((target, face_to)) = target else {
                    continue;
                };
                let old = self.state(target);
                let Some(new) = self.sculk_state_for_placement(old, target, face_to) else {
                    continue;
                };
                if self.set_block(target, new, 2) {
                    count += 1;
                }
            }
        }
        count
    }
    fn sculk_valid_for_placement(&self, old: BlockStateId, pos: BlockPos, face: Direction) -> bool {
        if self.vein_has_face(old, face) {
            return false;
        }
        self.b
            .multiface_can_attach(self.state(rel(pos, face)), face)
    }
    /// `MultifaceBlock.getStateForPlacement` for the sculk vein.
    fn sculk_state_for_placement(
        &self,
        old: BlockStateId,
        pos: BlockPos,
        face: Direction,
    ) -> Option<BlockStateId> {
        if !self.sculk_valid_for_placement(old, pos, face) {
            return None;
        }
        let b = self.b;
        let base = if self.is_vein(old) {
            old
        } else if b.fluid(old) == FluidState::WATER {
            b.table
                .set_bool(b.default_of(b.named.sculk_vein), b.props.waterlogged, true)
        } else {
            b.default_of(b.named.sculk_vein)
        };
        Some(self.vein_with_face(base, face, true))
    }
    /// `SculkVeinSpreaderConfig.canSpreadInto` (with the default config).
    fn sculk_can_spread_into(&self, source: BlockPos, target: BlockPos, face: Direction) -> bool {
        let b = self.b;
        let existing = self.state(target);
        let against = self.state(rel(target, face));
        let against_block = b.block(against);
        if against_block == b.named.sculk
            || against_block == b.named.sculk_catalyst
            || against_block == b.named.moving_piston
        {
            return false;
        }
        let manhattan =
            (source.x - target.x).abs() + (source.y - target.y).abs() + (source.z - target.z).abs();
        if manhattan == 2 {
            let neighbour = rel(source, face.opposite());
            if b.is_face_sturdy(self.state(neighbour), face) {
                return false;
            }
        }
        let fluid = b.fluid(existing);
        if fluid != FluidState::EMPTY && !(b.is_block(existing, b.named.water) && fluid.is_source())
        {
            return false;
        }
        if self.sculk_in_tag(existing, "minecraft:fire") {
            return false;
        }
        let replaceable = b.is_replaceable(existing)
            || b.is_air(existing)
            || self.is_vein(existing)
            || (b.is_block(existing, b.named.water) && fluid.is_source());
        replaceable && self.sculk_valid_for_placement(existing, target, face)
    }

    /// `SculkBehaviour.attemptUseCharge` per behaviour.
    fn sculk_attempt_use_charge(
        &mut self,
        behaviour: Behaviour,
        cursor: &ChargeCursor,
        origin: BlockPos,
        veins: bool,
    ) -> i32 {
        let charge = cursor.charge;
        match behaviour {
            Behaviour::Default => {
                if cursor.decay_delay > 0 {
                    charge
                } else {
                    0
                }
            }
            Behaviour::Vein => {
                if veins && self.sculk_place_sculk(cursor.pos) {
                    charge - 1
                } else if self.sculk_rand(CHARGE_DECAY_RATE) == 0 {
                    (charge as f32 * 0.5).floor() as i32
                } else {
                    charge
                }
            }
            Behaviour::Sculk => {
                if charge == 0 || self.sculk_rand(CHARGE_DECAY_RATE) != 0 {
                    return charge;
                }
                let pos = cursor.pos;
                let dist_sqr = f64::from(
                    (pos.x - origin.x).pow(2)
                        + (pos.y - origin.y).pow(2)
                        + (pos.z - origin.z).pow(2),
                );
                let close = dist_sqr < f64::from(NO_GROWTH_RADIUS * NO_GROWTH_RADIUS);
                if !close && self.sculk_can_place_growth(pos) {
                    if self.sculk_rand(GROWTH_SPAWN_COST) < charge {
                        let growth_pos = above(pos);
                        let growth = self.sculk_random_growth(growth_pos);
                        self.set_block_and_update(growth_pos, growth);
                        let sounds = self.b.data(growth).sounds.clone();
                        self.emit(LevelEvent::Sound {
                            pos,
                            sound: sounds.place.clone(),
                            volume: 1.0,
                            pitch: 1.0,
                        });
                    }
                    return (charge - GROWTH_SPAWN_COST).max(0);
                }
                if self.sculk_rand(ADDITIONAL_DECAY_RATE) != 0 {
                    return charge;
                }
                let penalty = if close {
                    1
                } else {
                    // getDecayPenalty
                    let outer = (dist_sqr.sqrt() as f32 - NO_GROWTH_RADIUS as f32).powi(2);
                    let reach = (24 - NO_GROWTH_RADIUS) * (24 - NO_GROWTH_RADIUS);
                    let factor = 1.0f32.min(outer / reach as f32);
                    1.max((charge as f32 * factor * 0.5) as i32)
                };
                charge - penalty
            }
        }
    }
    /// `SculkBlock.getRandomGrowthState` (not for world generation).
    fn sculk_random_growth(&mut self, pos: BlockPos) -> BlockStateId {
        let b = self.b;
        let state = if self.sculk_rand(11) == 0 {
            b.default_of(b.named.sculk_shrieker)
        } else {
            b.default_of(b.named.sculk_sensor)
        };
        if self.fluid_at(pos) != FluidState::EMPTY {
            b.table.set_bool(state, b.props.waterlogged, true)
        } else {
            state
        }
    }
    /// `SculkBlock.canPlaceGrowth`.
    fn sculk_can_place_growth(&self, pos: BlockPos) -> bool {
        let b = self.b;
        let state_above = self.state(above(pos));
        let open = b.is_air(state_above)
            || (b.is_block(state_above, b.named.water) && b.fluid(state_above).is_source());
        if !open {
            return false;
        }
        let mut matched = 0;
        for dz in -4..=4 {
            for dy in 0..=2 {
                for dx in -4..=4 {
                    let state = self.state(pos.offset(dx, dy, dz));
                    if self.sculk_in_tag(state, "minecraft:sculk_growth_inhibitors") {
                        matched += 1;
                        if matched >= 3 {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
    /// `SculkVeinBlock.attemptPlaceSculk`.
    fn sculk_place_sculk(&mut self, pos: BlockPos) -> bool {
        let state = self.state(pos);
        let mut directions = Direction::ALL;
        for i in (2..=directions.len()).rev() {
            let swap = self.sculk_rand(i as i32) as usize;
            directions.swap(i - 1, swap);
        }
        for support in directions {
            if !self.vein_has_face(state, support) {
                continue;
            }
            let support_pos = rel(pos, support);
            let support_state = self.state(support_pos);
            if !self.sculk_in_tag(support_state, "minecraft:sculk_replaceable") {
                continue;
            }
            let default_sculk = self.b.default_of(self.b.named.sculk);
            self.emit(LevelEvent::BlockEvent {
                pos: support_pos,
                state: support_state,
                kind: "push_entities_up",
            });
            self.set_block_and_update(support_pos, default_sculk);
            self.sculk_sound(support_pos, "minecraft:block.sculk.spread", 1.0, 1.0);
            self.sculk_spread_all(default_sculk, support_pos, &ALL_SPREADS);
            let skip = support.opposite();
            for direction in Direction::ALL {
                if direction == skip {
                    continue;
                }
                let vein_pos = rel(support_pos, direction);
                let possible = self.state(vein_pos);
                if self.is_vein(possible) {
                    self.sculk_on_discharged(Behaviour::Vein, possible, vein_pos);
                }
            }
            return true;
        }
        false
    }
    /// `SculkVeinBlock.onDischarged`.
    fn sculk_on_discharged(&mut self, behaviour: Behaviour, state: BlockStateId, pos: BlockPos) {
        if behaviour != Behaviour::Vein || !self.is_vein(state) {
            return;
        }
        let b = self.b;
        let mut state = state;
        for direction in Direction::ALL {
            if self.multiface_has_face(state, direction)
                && b.is_block(self.state(rel(pos, direction)), b.named.sculk)
            {
                state = self.vein_with_face(state, direction, false);
            }
        }
        if !Direction::ALL
            .into_iter()
            .any(|d| self.multiface_has_face(state, d))
        {
            state = if self.fluid_at(pos) == FluidState::EMPTY {
                b.default_of(b.named.air)
            } else {
                b.default_of(b.named.water)
            };
        }
        self.set_block_and_update(pos, state);
    }
    /// `ChargeCursor.getValidMovementPos`.
    fn sculk_valid_movement(&mut self, pos: BlockPos) -> Option<BlockPos> {
        let mut offsets = Vec::with_capacity(18);
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    if (x == 0 || y == 0 || z == 0) && (x, y, z) != (0, 0, 0) {
                        offsets.push([x, y, z]);
                    }
                }
            }
        }
        for i in (2..=offsets.len()).rev() {
            let swap = self.sculk_rand(i as i32) as usize;
            offsets.swap(i - 1, swap);
        }
        let b = self.b;
        let mut best = pos;
        for o in offsets {
            let neighbour = pos.offset(o[0], o[1], o[2]);
            let transferee = self.state(neighbour);
            if self.sculk_behaviour(transferee) == Behaviour::Default
                || !self.sculk_unobstructed_move(pos, neighbour)
            {
                continue;
            }
            best = neighbour;
            // SculkVeinBlock.hasSubstrateAccess
            if self.is_vein(transferee)
                && Direction::ALL.into_iter().any(|d| {
                    self.multiface_has_face(transferee, d)
                        && self.sculk_in_tag(
                            self.state(rel(neighbour, d)),
                            "minecraft:sculk_replaceable",
                        )
                })
            {
                break;
            }
        }
        let _ = b;
        (best != pos).then_some(best)
    }
    fn sculk_unobstructed_move(&self, from: BlockPos, to: BlockPos) -> bool {
        let manhattan = (from.x - to.x).abs() + (from.y - to.y).abs() + (from.z - to.z).abs();
        if manhattan == 1 {
            return true;
        }
        let delta = [to.x - from.x, to.y - from.y, to.z - from.z];
        let dir_x = if delta[0] < 0 {
            Direction::West
        } else {
            Direction::East
        };
        let dir_y = if delta[1] < 0 {
            Direction::Down
        } else {
            Direction::Up
        };
        let dir_z = if delta[2] < 0 {
            Direction::North
        } else {
            Direction::South
        };
        let (a, c) = if delta[0] == 0 {
            (dir_y, dir_z)
        } else if delta[1] == 0 {
            (dir_x, dir_z)
        } else {
            (dir_x, dir_y)
        };
        let free = |direction: Direction| {
            let test = rel(from, direction);
            !self
                .b
                .is_face_sturdy(self.state(test), direction.opposite())
        };
        free(a) || free(c)
    }
}

impl Behaviours {
    /// `SculkSensorBlock.canActivate`: the sensor is inactive.
    pub fn sculk_sensor_can_activate(&self, state: BlockStateId) -> bool {
        self.table.text(state, self.props.sculk_sensor_phase) == "inactive"
    }
    /// The sensor's phase (`inactive`, `active` or `cooldown`).
    pub fn sculk_sensor_active(&self, state: BlockStateId) -> bool {
        self.table.text(state, self.props.sculk_sensor_phase) == "active"
    }
    /// `SculkSensorBlock.activate` (the state, its ticks and the redstone
    /// update); the caller sends the tendril and resonance events.
    pub fn sculk_sensor_activate<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        pos: BlockPos,
        power: i32,
    ) {
        let mut sim = self.sim(level, ticks);
        let state = sim.state(pos);
        let active_ticks = if self.block(state) == self.named.calibrated_sculk_sensor {
            CALIBRATED_ACTIVE_TICKS
        } else {
            SENSOR_ACTIVE_TICKS
        };
        sim.sculk_sensor_activate(pos, state, power, active_ticks);
    }
    /// The click sound after an activation.
    pub fn sculk_sensor_click<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        pos: BlockPos,
    ) {
        let mut sim = self.sim(level, ticks);
        let state = sim.state(pos);
        sim.sculk_sensor_click(pos, state);
    }
    /// `CalibratedSculkSensorBlockEntity.VibrationUser.getBackSignal`.
    pub fn calibrated_back_signal<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        pos: BlockPos,
    ) -> i32 {
        let sim = self.sim(level, ticks);
        let state = sim.state(pos);
        let direction = self.facing(state).opposite();
        sim.signal(rel(pos, direction), direction)
    }
    /// `SculkCatalystBlockEntity.CatalystListener.bloom`'s block half.
    pub fn sculk_catalyst_bloom<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        pos: BlockPos,
    ) {
        let mut sim = self.sim(level, ticks);
        let state = sim.state(pos);
        if self.block(state) != self.named.sculk_catalyst {
            return;
        }
        let next = self.table.set_bool(state, self.props.bloom, true);
        sim.set_block_and_update(pos, next);
        let block = self.block(state);
        sim.schedule_tick(pos, block, PULSE_TICKS);
    }
    /// `SculkShriekerBlockEntity.shriek`'s block half: `shrieking` for 90 ticks.
    pub fn sculk_shrieker_shriek<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        pos: BlockPos,
    ) {
        let mut sim = self.sim(level, ticks);
        let state = sim.state(pos);
        if self.block(state) != self.named.sculk_shrieker {
            return;
        }
        let next = self.table.set_bool(state, self.props.shrieking, true);
        sim.set_block(pos, next, 2);
        let block = self.block(state);
        sim.schedule_tick(pos, block, SHRIEKING_TICKS);
        sim.level_event(3007, pos, 0);
    }
    /// `SculkSpreader.updateCursors` for a catalyst at `origin`.
    pub fn sculk_update_cursors<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        spreader: &mut SculkSpreader,
        origin: BlockPos,
        spread_veins: bool,
    ) {
        self.sim(level, ticks)
            .sculk_update_cursors(spreader, origin, spread_veins);
    }
}
