//! Minecarts (`AbstractMinecart` with `OldMinecartBehavior`): following
//! rails and slopes, powered and activator rails, coming off the track,
//! riding with the rider's push, pushing by players and other carts,
//! `MinecartItem` placement, `VehicleEntity` breaking, the chest, hopper,
//! furnace and TNT variants, saving, and `AbstractMinecartRenderer`'s old
//! rail-following render with the display block.
use super::{Game, Hit, InteractTarget, Keys, entity::Entity};
use horizon_client::actions::Action;
use horizon_client::entity_rules::EntityRules;
use horizon_client::fast_hash::FxHashSet;
use horizon_client::join_seq::{JoinList, ListStamp};
use horizon_client::mobs::{Mob, seen_grid::SeenGrid};
use horizon_client::{
    blocks::Fluid,
    collisions::SceneCollisions,
    containers::BlockEntity,
    items::{ItemStack, Items},
    mesher::Vertex,
    mobs::{Part, Species, Who, anim::Poses, riders},
    particles::{ParticleOptions, ParticleType},
    random_source,
    scoreboard::pushable_by,
    sound_engine::{CartSound, PlayResult, SoundInstance, Sounds},
};
use horizon_core::{
    geometry::{Aabb, Vec3, trig},
    random::{LegacyRandom, RandomSource},
};
use horizon_world::registry::BlockStateId;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CartKind {
    Rideable,
    Chest,
    Hopper,
    Furnace,
    Tnt,
    /// `MinecartCommandBlock`: its `BaseCommandBlock` is `Minecart::command`,
    /// run by a powered activator rail (`game/command_minecarts.rs`).
    CommandBlock,
    /// `MinecartSpawner`: a `BaseSpawner` that ticks where the cart is
    /// (`Minecart::spawner`). Only `/summon` makes one: there is no item.
    Spawner,
}
impl CartKind {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id.strip_prefix("minecraft:")? {
            "minecart" => Self::Rideable,
            "chest_minecart" => Self::Chest,
            "hopper_minecart" => Self::Hopper,
            "furnace_minecart" => Self::Furnace,
            "tnt_minecart" => Self::Tnt,
            "command_block_minecart" => Self::CommandBlock,
            "spawner_minecart" => Self::Spawner,
            _ => return None,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Rideable => "minecraft:minecart",
            Self::Chest => "minecraft:chest_minecart",
            Self::Hopper => "minecraft:hopper_minecart",
            Self::Furnace => "minecraft:furnace_minecart",
            Self::Tnt => "minecraft:tnt_minecart",
            Self::CommandBlock => "minecraft:command_block_minecart",
            Self::Spawner => "minecraft:spawner_minecart",
        }
    }
    /// `AbstractMinecart.getDropItem` and `getPickResult`: the item a cart drops and
    /// is picked as (a spawner minecart's is a plain minecart: there is no item of its own).
    pub fn item(self) -> &'static str {
        match self {
            Self::Spawner => "minecraft:minecart",
            kind => kind.id(),
        }
    }
    pub(super) fn slots(self) -> usize {
        match self {
            Self::Chest => 27,
            Self::Hopper => 5,
            _ => 0,
        }
    }
    /// Whether this cart's `interact` passes on the client, so that
    /// `Minecraft.startUseItem` uses the held item instead: `MinecartTNT` has
    /// none of its own (`Entity.interact`), `MinecartCommandBlock.interact`
    /// needs a game master, and `Minecart.interact` a player not sneaking at
    /// an empty cart.
    pub fn interact_passes(self, sneaking: bool, ridden: bool, game_master: bool) -> bool {
        match self {
            Self::Tnt | Self::Spawner => true,
            Self::CommandBlock => !game_master,
            Self::Rideable => sneaking || ridden,
            Self::Chest | Self::Hopper | Self::Furnace => false,
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Minecart {
    /// When it joined its list (`horizon_client::join_seq`); set by the list.
    pub(super) join: horizon_client::join_seq::JoinSeq,
    pub id: u32,
    pub kind: CartKind,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub previous_yaw: f32,
    pub pitch: f32,
    pub previous_pitch: f32,
    /// Minecart's client-only accumulated turn and the passenger's eased turn.
    pub rotation_offset: f32,
    pub player_rotation_offset: f32,
    pub passenger_yaw_delta: f32,
    /// Experimental track steps, drained by the server entity tracker.
    pub lerp_steps: Vec<horizon_protocol::packets::play::MinecartStep>,
    pub flipped: bool,
    /// `AbstractMinecart.onRails`: transitions into rails need a position snap.
    on_rails: bool,
    pub(super) on_ground: bool,
    horizontal_collision: bool,
    fall_distance: f64,
    /// The squared speed `MinecartTNT.causeFallDamage` blows up with, set by
    /// a landing of three blocks or more and taken by the cart's next tick
    /// step (vanilla explodes inside the move itself).
    fall_blast: Option<f64>,
    /// `Entity.stuckSpeedMultiplier`: set by a cobweb the cart's box is in,
    /// scaled into its next move.
    stuck: Vec3,
    pub hurt_time: i32,
    pub hurt_dir: i32,
    pub damage: f32,
    /// `Entity.noGravity` (`NoGravity` in its save).
    pub no_gravity: bool,
    pub items: Vec<Option<ItemStack>>,
    /// Furnace minecarts: fuel ticks and `MinecartFurnace.push`. Its y part
    /// stays between ticks (it sets the push's length), only saving drops it.
    pub fuel: i32,
    pub push: Vec3,
    /// TNT minecarts: ticks until the explosion, -1 when not primed.
    pub fuse: i32,
    /// TNT minecarts: `explosionPowerBase` and `explosionSpeedFactor`, the
    /// blast's power at rest and its gain with the cart's speed.
    pub explosion_power: f32,
    pub explosion_speed_factor: f32,
    /// Hopper minecarts: an unpowered activator rail enables them.
    pub enabled: bool,
    /// Chest minecarts: the loot table rolled into `items` on load, saved
    /// again while the items are still that roll.
    pub loot: Option<horizon_client::save::RolledLoot>,
    /// `Entity.firstTick`: not ticked yet (saved as `HasTicked`).
    pub first_tick: bool,
    /// `Entity.isInWater` (`wasTouchingWater`): `AbstractMinecart.tick`
    /// updates it after the move (`updateFluidInteraction`), so the move
    /// goes by where the cart was a tick before, and a new cart's first
    /// move is a dry one.
    pub in_water: bool,
    /// Command block minecarts: the cart's `BaseCommandBlock` (command,
    /// success count, last output, output tracking). The block-only flags of
    /// `CommandBlock` stay unset.
    pub command: horizon_client::command_block::CommandBlock,
    /// `Entity.tickCount`, which `MinecartCommandBlock.activateMinecart`
    /// measures its `ACTIVATION_DELAY` against.
    pub tick_count: i32,
    /// `MinecartCommandBlock.lastActivated`.
    pub last_activated: i32,
    /// An activator rail asked for the command during this tick; it runs once
    /// the cart is back in the level (`command_minecarts.rs`).
    pub command_due: bool,
    /// Spawner minecarts: the cart's `BaseSpawner` (`Delay`, `SpawnData`,
    /// `SpawnPotentials` and the rest as a spawner block saves them).
    pub spawner: Option<Box<horizon_client::mobs::spawner::Spawner>>,
    /// The synched `DATA_ID_COMMAND_NAME` and `DATA_ID_LAST_OUTPUT`, which
    /// follow the command block only at `onUpdated`.
    pub synced_command: String,
    pub synced_output: String,
    /// `AbstractMinecart.customDisplayBlockState` (`DisplayState`): the block
    /// state's NBT form, resolved against the registry where it draws.
    pub custom_display: Option<horizon_world::nbt::Tag>,
    /// `AbstractMinecart.customDisplayOffset` (`DisplayOffset`), in sixteenths.
    pub custom_offset: Option<i32>,
    pub random: LegacyRandom,
}
/// `EntityType.MINECART`'s `sized(0.98F, 0.7F)`: float dimensions, widened.
pub(super) const WIDTH: f64 = 0.98_f32 as f64;
pub(super) const HEIGHT: f64 = 0.7_f32 as f64;
/// `EntityEvent.TNT_PRIME`. `MinecartTNT.primeFuse` broadcasts it;
/// `MinecartTNT.handleEntityEvent` answers by calling `primeFuse`.
pub(super) const TNT_PRIME_EVENT: i8 = 70;
/// `AbstractMinecart.getDefaultDisplayOffset` (6), in sixteenths of a block, as
/// `MinecartChest` (8) and `MinecartHopper` (1) override it.
pub(super) fn default_display_offset(kind: CartKind) -> f64 {
    match kind {
        CartKind::Chest => 8.0,
        CartKind::Hopper => 1.0,
        CartKind::Rideable
        | CartKind::Furnace
        | CartKind::Tnt
        | CartKind::CommandBlock
        | CartKind::Spawner => 6.0,
    }
}
/// `AbstractMinecart.getDisplayOffset`: the saved `DisplayOffset`, else the default.
fn display_offset(c: &Minecart) -> f64 {
    c.custom_offset
        .map_or_else(|| default_display_offset(c.kind), f64::from)
}

/// World-space envelope of the cart model and its optional displayed block,
/// based on the same root position and display offset used by the renderer.
fn cart_render_bounds(position: Vec3, offset: f64, has_display: bool) -> Aabb {
    let (bottom, top) = if has_display {
        let display_shift = 0.75 * (offset - 8.0) / 16.0;
        (
            (position.y - 0.5).min(position.y + 0.375 - 0.4 + display_shift),
            (position.y + 0.9).max(position.y + 0.375 + 0.4 + display_shift + 0.9),
        )
    } else {
        (position.y - 0.5, position.y + 0.9)
    };
    Aabb::new(
        Vec3::new(position.x - 1.0, bottom, position.z - 1.0),
        Vec3::new(position.x + 1.0, top, position.z + 1.0),
    )
}
/// A display state that is air shows nothing.
fn shown_unless_air(
    state: Option<BlockStateId>,
    is_air: impl Fn(BlockStateId) -> bool,
) -> Option<BlockStateId> {
    state.filter(|&s| !is_air(s))
}
/// `readAdditionalSaveData`'s `input.read("DisplayState", CODEC)` is empty
/// when the state does not decode: the cart forgets it (and no longer saves it).
pub(super) fn forget_unreadable_display(
    c: &mut Minecart,
    resolves: impl Fn(&horizon_world::nbt::Tag) -> bool,
) {
    if c.custom_display.as_ref().is_some_and(|t| !resolves(t)) {
        c.custom_display = None;
    }
}
/// The carts' and mobs' boxes for one tick's cart pushes (`push_from_cart`,
/// `pickup_from_cart`), so a cart reads its neighbours instead of every cart and
/// every mob of the level. Both answer in slot order, as the scans did.
///
/// The cart grid follows each ticked cart ([`CartIndex::moved`]); it is
/// rebuilt once the list's length changed (a cart gone or added), and dropped
/// after a command, which can move or replace any cart
/// ([`CartIndex::forget_carts`]). The mob grid holds until the mob
/// list's next write ([`ListStamp`]). A native mod's hook can move either kind
/// unseen, so with mods loaded both scans stay.
#[derive(Default)]
pub(super) struct CartIndex {
    carts: Option<(SeenGrid, Vec<u32>)>,
    /// The grid, and the carts a mob rides.
    mobs: Option<(ListStamp, SeenGrid, FxHashSet<u32>)>,
    near: Vec<u32>,
}
impl CartIndex {
    fn usable() -> bool {
        horizon_client::mod_api::mods().is_empty()
    }
    /// The cart in slot `i` has these bounds now.
    fn moved(&mut self, i: usize, bounds: Aabb) {
        if let Some((grid, ids)) = &mut self.carts
            && i < ids.len()
        {
            grid.update(i, bounds);
        }
    }
    fn forget_carts(&mut self) {
        self.carts = None;
    }
    /// The slots of the carts whose box may meet `area`, for the cart in slot
    /// `i` (just moved); false: scan them all.
    fn carts_near(&mut self, carts: &[Minecart], i: usize, area: Aabb, out: &mut Vec<u32>) -> bool {
        if !Self::usable() {
            return false;
        }
        let fresh = |ids: &[u32]| ids.len() == carts.len() && ids[i] == carts[i].id;
        if !self.carts.as_ref().is_some_and(|(_, ids)| fresh(ids)) {
            self.carts = Some((
                SeenGrid::new(carts.iter().map(Minecart::bounds)),
                carts.iter().map(|c| c.id).collect(),
            ));
        }
        let Some((grid, ids)) = &mut self.carts else {
            return false;
        };
        grid.update(i, carts[i].bounds());
        if !grid.candidates(area, out) {
            return false;
        }
        if out.iter().any(|&j| ids[j as usize] != carts[j as usize].id) {
            // A cart was replaced in place: start over next time.
            self.carts = None;
            return false;
        }
        true
    }
    /// The slots of the mobs whose box may meet `area` into `out`, and
    /// whether a mob rides cart `cart`; None: scan them all.
    fn mobs_near(
        &mut self,
        mobs: &JoinList<Mob>,
        cart: u32,
        area: Aabb,
        out: &mut Vec<u32>,
    ) -> Option<bool> {
        if !Self::usable() {
            return None;
        }
        if !self
            .mobs
            .as_ref()
            .is_some_and(|(stamp, ..)| *stamp == mobs.stamp())
        {
            self.mobs = Some((
                mobs.stamp(),
                SeenGrid::new(mobs.iter().map(Mob::bounds)),
                mobs.iter().filter_map(|m| m.extra.mount).collect(),
            ));
        }
        let (_, grid, mounted) = self.mobs.as_ref()?;
        grid.candidates(area, out).then(|| mounted.contains(&cart))
    }
}
/// Where the cart `id` is in `carts`: at `hint` (where it was a moment ago,
/// the usual case while carts tick in order) or found by a scan. A world
/// with hundreds of mineshaft chest carts asked this of every cart several
/// times a tick.
fn cart_at(carts: &[Minecart], id: u32, hint: usize) -> Option<usize> {
    if carts.get(hint).is_some_and(|c| c.id == id) {
        return Some(hint);
    }
    carts.iter().position(|c| c.id == id)
}
/// `AbstractMinecart.push(entity)` for a non-cart `entity` at `feet`:
/// the push the cart loses (the entity gets a quarter of it back).
pub(super) fn cart_push_from(feet: Vec3, cart: Vec3) -> Option<(f64, f64)> {
    let (xa, za) = (feet.x - cart.x, feet.z - cart.z);
    let dd = xa * xa + za * za;
    if dd < f64::from(1.0e-4f32) {
        return None;
    }
    let d = dd.sqrt();
    let pow = (1.0 / d).min(1.0);
    let k = f64::from(0.1f32) * 0.5;
    Some((xa / d * pow * k, za / d * pow * k))
}
/// `MinecartFurnace.calculateNewPushAlong`: project the push onto movement.
fn push_along(push: Vec3, movement: Vec3) -> Vec3 {
    if push.horizontal_distance_squared() > 1.0e-4 && movement.horizontal_distance_squared() > 0.001
    {
        push.projected_on(movement).normalize().scale(push.length())
    } else {
        push
    }
}
/// AbstractMinecart passenger attachment Y (EntityAttachments).
const PASSENGER_Y: f64 = 0.1875;
/// The minecart's passenger attachment minus Player.DEFAULT_VEHICLE_ATTACHMENT.
pub(super) const SEAT: f64 = PASSENGER_Y - 0.6;
/// The placeholder container key under which an open cart's items live.
pub(super) const CART_KEY_Y: i32 = horizon_client::lan::menu::VEHICLE_KEY_Y;

/// `OldMinecartBehavior.pushAndPickupEntities`: living entities the cart
/// offers `startRiding` instead of `Entity.push`. Players click to board;
/// iron golems always push; a cart that already has a rider is `isVehicle`.
fn pickup_instead_of_push(species: Species, is_passenger: bool, cart_is_vehicle: bool) -> bool {
    !cart_is_vehicle && !is_passenger && species != Species::IronGolem
}

/// `Entity.canRide` for a cart: the boarding cooldown, and a warden or the
/// wither never rides (`canRide` false).
fn can_ride_cart(species: Species, boarding_cooldown: i32) -> bool {
    boarding_cooldown <= 0 && !matches!(species, Species::Warden | Species::Wither)
}

/// `Entity.positionRider` on an `AbstractMinecart`: passenger attachment less
/// the rider's own vehicle attachment.
fn cart_seat(cart: Vec3, rider_offset: f64) -> Vec3 {
    Vec3::new(cart.x, cart.y + PASSENGER_Y - rider_offset, cart.z)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Shape {
    NorthSouth,
    EastWest,
    AscendingEast,
    AscendingWest,
    AscendingNorth,
    AscendingSouth,
    SouthEast,
    SouthWest,
    NorthWest,
    NorthEast,
}
impl Shape {
    fn parse(s: &str) -> Self {
        match s {
            "east_west" => Self::EastWest,
            "ascending_east" => Self::AscendingEast,
            "ascending_west" => Self::AscendingWest,
            "ascending_north" => Self::AscendingNorth,
            "ascending_south" => Self::AscendingSouth,
            "south_east" => Self::SouthEast,
            "south_west" => Self::SouthWest,
            "north_west" => Self::NorthWest,
            "north_east" => Self::NorthEast,
            _ => Self::NorthSouth,
        }
    }
    pub(super) fn is_slope(self) -> bool {
        matches!(
            self,
            Self::AscendingEast | Self::AscendingWest | Self::AscendingNorth | Self::AscendingSouth
        )
    }
    /// `AbstractMinecart.EXITS`.
    fn exits(self) -> ([i32; 3], [i32; 3]) {
        let (w, e, n, s) = ([-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1]);
        let below = |v: [i32; 3]| [v[0], -1, v[2]];
        match self {
            Self::NorthSouth => (n, s),
            Self::EastWest => (w, e),
            Self::AscendingEast => (below(w), e),
            Self::AscendingWest => (w, below(e)),
            Self::AscendingNorth => (n, below(s)),
            Self::AscendingSouth => (below(n), s),
            Self::SouthEast => (s, e),
            Self::SouthWest => (s, w),
            Self::NorthWest => (n, w),
            Self::NorthEast => (n, e),
        }
    }
}
pub(super) struct Rail {
    name: Arc<str>,
    pub(super) shape: Shape,
    powered: bool,
}
impl Minecart {
    /// `Minecart.tick`: stationary rotation does not turn the passenger.
    pub(super) fn accumulate_rotation(&mut self) {
        self.passenger_yaw_delta = 0.0;
        if self.kind == CartKind::Rideable && self.position.subtract(self.previous).length() > 0.01
        {
            self.rotation_offset +=
                ((f64::from(self.yaw) - f64::from(self.previous_yaw)) % 360.0) as f32;
            self.rotation_offset %= 360.0;
        }
    }
    /// `Minecart.positionRider`: double rotLerp, then round back to float.
    /// Only yaw follows; the player's pitch and manual look remain their own.
    pub(super) fn rotate_passenger(&mut self, yaw: &mut f32) {
        let mut delta =
            (f64::from(self.rotation_offset) - f64::from(self.player_rotation_offset)) % 360.0;
        if delta >= 180.0 {
            delta -= 360.0;
        }
        if delta < -180.0 {
            delta += 360.0;
        }
        let next = (f64::from(self.player_rotation_offset) + 0.5 * delta) as f32;
        let previous = *yaw;
        *yaw -= next - self.player_rotation_offset;
        self.passenger_yaw_delta = *yaw - previous;
        self.player_rotation_offset = next;
    }
    /// `Entity.onGround`.
    pub(super) fn on_ground(&self) -> bool {
        self.on_ground
    }
    pub fn new(id: u32, kind: CartKind, position: Vec3) -> Self {
        Self {
            join: Default::default(),
            id,
            kind,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            previous_yaw: 0.0,
            pitch: 0.0,
            previous_pitch: 0.0,
            rotation_offset: 0.0,
            player_rotation_offset: 0.0,
            passenger_yaw_delta: 0.0,
            lerp_steps: Vec::new(),
            flipped: false,
            on_rails: false,
            on_ground: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            fall_blast: None,
            stuck: Vec3::ZERO,
            hurt_time: 0,
            hurt_dir: 1,
            damage: 0.0,
            no_gravity: false,
            items: vec![None; kind.slots()],
            fuel: 0,
            push: Vec3::ZERO,
            fuse: -1,
            explosion_power: DEFAULT_EXPLOSION_POWER,
            explosion_speed_factor: DEFAULT_EXPLOSION_SPEED_FACTOR,
            enabled: true,
            loot: None,
            first_tick: true,
            in_water: false,
            command: horizon_client::command_block::CommandBlock::default(),
            tick_count: 0,
            last_activated: 0,
            command_due: false,
            // A fresh `BaseSpawner`, as `MinecartSpawner`'s constructor makes.
            spawner: (kind == CartKind::Spawner).then(Default::default),
            synced_command: String::new(),
            synced_output: String::new(),
            custom_display: None,
            custom_offset: None,
            random: random_source::entity(),
        }
    }
    pub fn bounds(&self) -> Aabb {
        let (w, p) = (WIDTH / 2.0, self.position);
        Aabb::new(
            Vec3::new(p.x - w, p.y, p.z - w),
            Vec3::new(p.x + w, p.y + HEIGHT, p.z + w),
        )
    }
    fn horizontal(&self) -> f64 {
        self.velocity.x.hypot(self.velocity.z)
    }
    /// `AbstractContainerMenu.getRedstoneSignalFromContainer` of `slots`.
    pub(super) fn container_signal(slots: &[Option<ItemStack>], items: &Items) -> i32 {
        let mut total = 0.0f32;
        for s in slots.iter().flatten() {
            let max = items.get(&s.item).map_or(64, |i| i.max_stack).min(64);
            total += s.count as f32 / max as f32;
        }
        total /= slots.len().max(1) as f32;
        // Mth.lerpDiscrete(total, 0, 15)
        (total * 14.0).floor() as i32 + i32::from(total > 0.0)
    }
}
/// `AbstractMinecartContainer.applyNaturalSlowdown`'s keep factor, in the
/// float arithmetic vanilla uses: 0.98 plus a thousandth per empty step of the
/// comparator signal (none while a loot table is still unrolled), times 0.95
/// in water.
fn container_keep(signal: i32, loot_pending: bool, in_water: bool) -> f64 {
    let mut keep = 0.98_f32;
    if !loot_pending {
        keep += (15 - signal) as f32 * 0.001_f32;
    }
    if in_water {
        keep *= 0.95_f32;
    }
    f64::from(keep)
}
pub(super) struct CartModels {
    pub models: HashMap<CartKind, Part>,
}
impl CartModels {
    pub fn load(document: Option<&horizon_client::mobs::EntityModelDocument>) -> Self {
        let kinds = [
            CartKind::Rideable,
            CartKind::Chest,
            CartKind::Hopper,
            CartKind::Furnace,
            CartKind::Tnt,
            CartKind::CommandBlock,
            CartKind::Spawner,
        ];
        let models = kinds
            .into_iter()
            .filter_map(|k| document?.model(&format!("{}#main", k.id())).map(|m| (k, m)))
            .collect();
        Self { models }
    }
}
impl Game<'_> {
    pub(super) fn rail(&self, x: i32, y: i32, z: i32) -> Option<Rail> {
        let state = self.world.block(x, y, z);
        let name = self.res.facts().get(state).name.clone();
        if !matches!(
            &*name,
            "minecraft:rail"
                | "minecraft:powered_rail"
                | "minecraft:detector_rail"
                | "minecraft:activator_rail"
        ) {
            return None;
        }
        let s = self.pack.assets.registry.state(state)?;
        let shape = Shape::parse(s.properties.get("shape").map_or("", String::as_str));
        let powered = s.properties.get("powered").is_some_and(|v| v == "true");
        Some(Rail {
            name,
            shape,
            powered,
        })
    }
    /// `getCurrentBlockPosOrRailBelow`.
    fn rail_block(&self, p: Vec3) -> [i32; 3] {
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let below = if self.minecart_improvements() {
            (p.y - 0.1 - f64::from(1.0e-5_f32)).floor() as i32
        } else {
            y - 1
        };
        if self.rail(x, below, z).is_some() {
            [x, below, z]
        } else {
            [x, y, z]
        }
    }
    fn conductor(&self, x: i32, y: i32, z: i32) -> bool {
        self.res
            .facts()
            .get(self.world.block(x, y, z))
            .full_collision
    }
    /// `OldMinecartBehavior.getPos`: the point on the rail under (x, y, z).
    fn rail_pos(&self, x: f64, y: f64, z: f64) -> Option<[f64; 3]> {
        let [xt, yt, zt] = self.rail_block(Vec3::new(x, y, z));
        let rail = self.rail(xt, yt, zt)?;
        let (e0, e1) = rail.shape.exits();
        let (fx, fy, fz) = (f64::from(xt), f64::from(yt), f64::from(zt));
        let x0 = fx + 0.5 + f64::from(e0[0]) * 0.5;
        let y0 = fy + 0.0625 + f64::from(e0[1]) * 0.5;
        let z0 = fz + 0.5 + f64::from(e0[2]) * 0.5;
        let x1 = fx + 0.5 + f64::from(e1[0]) * 0.5;
        let y1 = fy + 0.0625 + f64::from(e1[1]) * 0.5;
        let z1 = fz + 0.5 + f64::from(e1[2]) * 0.5;
        let (xd, yd, zd) = (x1 - x0, (y1 - y0) * 2.0, z1 - z0);
        let progress = if xd == 0.0 {
            z - fz
        } else if zd == 0.0 {
            x - fx
        } else {
            ((x - x0) * xd + (z - z0) * zd) * 2.0
        };
        let mut out = [x0 + xd * progress, y0 + yd * progress, z0 + zd * progress];
        if yd < 0.0 {
            out[1] += 1.0;
        } else if yd > 0.0 {
            out[1] += 0.5;
        }
        Some(out)
    }
    /// `OldMinecartBehavior.getPosOffs`.
    fn rail_pos_offs(&self, mut x: f64, y: f64, mut z: f64, offs: f64) -> Option<[f64; 3]> {
        let [xt, yt, zt] = self.rail_block(Vec3::new(x, y, z));
        let rail = self.rail(xt, yt, zt)?;
        let mut y = f64::from(yt) + if rail.shape.is_slope() { 1.0 } else { 0.0 };
        let (e0, e1) = rail.shape.exits();
        let (mut xd, mut zd) = (f64::from(e1[0] - e0[0]), f64::from(e1[2] - e0[2]));
        let dd = xd.hypot(zd);
        xd /= dd;
        zd /= dd;
        x += xd * offs;
        z += zd * offs;
        let at = |e: [i32; 3]| x.floor() as i32 - xt == e[0] && z.floor() as i32 - zt == e[2];
        if e0[1] != 0 && at(e0) {
            y += f64::from(e0[1]);
        } else if e1[1] != 0 && at(e1) {
            y += f64::from(e1[1]);
        }
        self.rail_pos(x, y, z)
    }
    /// `AbstractMinecart.canCollideWith`, for `Entity.collide`'s `getEntityCollisions`:
    /// `AbstractBoat.canVehicleCollide` takes any other cart, boat, shulker or pushable mob and
    /// player, except one riding this cart. A cart that runs into one has a horizontal
    /// collision, which sets a TNT minecart off.
    fn cart_entity_collisions(&self, c: &Minecart) -> Vec<Aabb> {
        let near = c.bounds().inflate(Vec3::new(2.0, 2.0, 2.0));
        // Every cart asks about every entity of the level each tick, and the box
        // turns nearly all of them away: it is tested first, before a mob's
        // pushable state, and only the boxes kept are collected.
        let close = |b: &Aabb| b.intersects(near);
        let reach = self.vehicle_mob_reach();
        let mut boxes: Vec<Aabb> = self
            .carts
            .iter()
            .filter(|o| o.id != c.id)
            .map(Minecart::bounds)
            .filter(close)
            .chain(
                self.boats
                    .iter()
                    .map(super::boats::Boat::bounds)
                    .filter(close),
            )
            .chain(
                self.mobs
                    .iter()
                    .filter(|m| reach.is_none_or(|r| r.may_meet(m, near)))
                    .map(|m| (m, m.bounds()))
                    .filter(|(m, b)| {
                        close(b)
                            && !m.removed
                            && m.extra.mount != Some(c.id)
                            && (m.pushable()
                                || m.species() == horizon_client::mobs::Species::Shulker
                                    && m.alive())
                    })
                    .map(|(_, b)| b),
            )
            .collect();
        if self.riding != Some(c.id)
            && let Some(p) = self.player.as_ref()
            && !self.spectator()
            && close(&p.bounds())
        {
            boxes.push(p.bounds());
        }
        if let Some(host) = self.lan.as_ref() {
            boxes.extend(
                host.hub
                    .guests
                    .iter()
                    .filter(|g| {
                        host.hub.here(g)
                            && g.loaded
                            && !g.dead
                            && !g.spectator()
                            && g.vehicle != Some(c.id)
                    })
                    .map(|g| g.bounds())
                    .filter(close),
            );
        }
        boxes
    }
    /// Entity.move for a cart: collisions zero the blocked components, the world border's walls
    /// among them (`CollisionGetter.borderCollision`, asked for every entity).
    fn move_cart(&self, c: &mut Minecart, mut delta: Vec3) {
        // Entity.move: a stuck multiplier scales this move and zeroes the velocity.
        if c.stuck.length_squared() > 1.0e-7 {
            delta = delta.multiply(c.stuck);
            c.stuck = Vec3::ZERO;
            c.velocity = Vec3::ZERO;
        }
        let scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        // A zero move stays zero whatever surrounds it (`Entity.collide`, and the
        // step-up needs a blocked horizontal move): a cart standing on its rails,
        // as most do, gathers no entity boxes.
        let entities = if delta.length_squared() == 0.0 {
            Vec::new()
        } else {
            self.cart_entity_collisions(c)
        };
        let mut scene = horizon_client::collisions::EntityCollisions {
            blocks: scene,
            entities: &entities,
            border: Some((self.border.geometry(), [c.position.x, c.position.z])),
        };
        if let Ok(moved) = scene.resolve(c.bounds(), delta, 0.0, c.on_ground) {
            c.fall_distance = self.vehicle_fall_reset(c.fall_distance, c.position, moved);
            c.position = Vec3::new(
                c.position.x + moved.x,
                c.position.y + moved.y,
                c.position.z + moved.z,
            );
            c.on_ground = delta.y < 0.0 && moved.y > delta.y;
            c.horizontal_collision =
                (moved.x - delta.x).abs() > 1.0e-7 || (moved.z - delta.z).abs() > 1.0e-7;
            if (moved.x - delta.x).abs() > 1.0e-7 {
                c.velocity.x = 0.0;
            }
            if (moved.z - delta.z).abs() > 1.0e-7 {
                c.velocity.z = 0.0;
            }
            if (moved.y - delta.y).abs() > 1.0e-7 {
                c.velocity.y = 0.0;
            }
            // Entity.checkFallDamage: the move's drop counts (not in water, as of
            // the tick before: `isInWater` is `wasTouchingWater`),
            // and a landing hands the whole fall to causeFallDamage, which
            // sets a TNT minecart off at (fall / 10) squared.
            if moved.y < 0.0 && !c.in_water {
                c.fall_distance -= f64::from(moved.y as f32);
            }
            // A move up or none leaves the fall as it is (a saved one too).
            if c.on_ground {
                if c.kind == CartKind::Tnt && c.fall_distance >= 3.0 {
                    let power = c.fall_distance / 10.0;
                    c.fall_blast = Some(power * power);
                }
                c.fall_distance = 0.0;
            }
        }
        // OldMinecartBehavior applies block effects here and at tick end.
        // NewMinecartBehavior applies them after recording each track step.
        if !self.minecart_improvements() {
            self.cart_block_effects(c);
        }
    }
    fn max_speed(&self, c: &Minecart) -> f64 {
        let base = if c.in_water { 0.2 } else { 0.4 };
        if c.kind == CartKind::Furnace {
            base * if c.in_water { 0.75 } else { 0.5 }
        } else {
            base
        }
    }
    /// `applyNaturalSlowdown` (with `MinecartFurnace`'s push); `factor` is
    /// `getSlowdownFactor`: 0.997 riding, the empty cart's own by behavior.
    fn natural_slowdown(&self, c: &mut Minecart, ridden: bool, factor: f64) -> Vec3 {
        let mut v = c.velocity;
        let water = c.in_water;
        if c.kind == CartKind::Furnace {
            if c.push.length_squared() > 1.0e-7 {
                c.push = push_along(c.push, v);
                v = Vec3::new(v.x * 0.8 + c.push.x, 0.0, v.z * 0.8 + c.push.z);
                if water {
                    v = Vec3::new(v.x * 0.1, 0.0, v.z * 0.1);
                }
            } else {
                v = Vec3::new(v.x * 0.98, 0.0, v.z * 0.98);
            }
        }
        if matches!(c.kind, CartKind::Chest | CartKind::Hopper) {
            // `AbstractMinecartContainer.applyNaturalSlowdown` replaces the
            // base one whole: the keep factor leans on how empty it is, and
            // riders and the behavior's own factor do not enter.
            let signal = Minecart::container_signal(&c.items, self.items());
            let keep = container_keep(signal, c.loot.is_some(), water);
            return Vec3::new(v.x * keep, 0.0, v.z * keep);
        }
        let f = if ridden { 0.997 } else { factor };
        let mut out = Vec3::new(v.x * f, 0.0, v.z * f);
        if water {
            // scale(0.95F): the float widened.
            let drag = f64::from(0.95_f32);
            out = Vec3::new(out.x * drag, 0.0, out.z * drag);
        }
        out
    }
    /// `Entity.isVehicle` for a cart: the local player, a LAN guest or a mob
    /// sits in it. `getSlowdownFactor` (0.997) and the 0.75 speed scale key on
    /// this, not on who rides.
    fn cart_is_vehicle(&self, id: u32) -> bool {
        self.riding == Some(id)
            || self.ridden_by_guest(id)
            || self
                .mobs
                .iter()
                .any(|m| !m.removed && m.extra.mount == Some(id))
    }
    /// `OldMinecartBehavior.moveAlongTrack`.
    fn move_along_track(
        &mut self,
        c: &mut Minecart,
        pos: [i32; 3],
        rail: &Rail,
        intent: [f64; 2],
    ) -> (Vec3, Vec3) {
        c.fall_distance = 0.0;
        let ridden = self.cart_is_vehicle(c.id);
        let (x, z) = (c.position.x, c.position.z);
        let old = self.rail_pos(x, c.position.y, z);
        let mut y = f64::from(pos[1]);
        let (mut power, mut halt) = (false, false);
        if &*rail.name == "minecraft:powered_rail" {
            power = rail.powered;
            halt = !power;
        }
        let slide = if c.in_water {
            0.0078125 * 0.2
        } else {
            0.0078125
        };
        match rail.shape {
            Shape::AscendingEast => c.velocity.x -= slide,
            Shape::AscendingWest => c.velocity.x += slide,
            Shape::AscendingNorth => c.velocity.z += slide,
            Shape::AscendingSouth => c.velocity.z -= slide,
            _ => {}
        }
        if rail.shape.is_slope() {
            y += 1.0;
        }
        let (e0, e1) = rail.shape.exits();
        let (mut xd, mut zd) = (f64::from(e1[0] - e0[0]), f64::from(e1[2] - e0[2]));
        let length = xd.hypot(zd);
        if c.velocity.x * xd + c.velocity.z * zd < 0.0 {
            xd = -xd;
            zd = -zd;
        }
        let pow = c.horizontal().min(2.0);
        c.velocity = Vec3::new(pow * xd / length, c.velocity.y, pow * zd / length);
        if ridden
            && (intent[0] != 0.0 || intent[1] != 0.0)
            && c.velocity.x.powi(2) + c.velocity.z.powi(2) < 0.01
        {
            c.velocity.x += intent[0] * 0.001;
            c.velocity.z += intent[1] * 0.001;
            halt = false;
        }
        if halt {
            if c.horizontal() < 0.03 {
                c.velocity = Vec3::ZERO;
            } else {
                c.velocity = Vec3::new(c.velocity.x * 0.5, 0.0, c.velocity.z * 0.5);
            }
        }
        let (fx, fz) = (f64::from(pos[0]), f64::from(pos[2]));
        let x0 = fx + 0.5 + f64::from(e0[0]) * 0.5;
        let z0 = fz + 0.5 + f64::from(e0[2]) * 0.5;
        let x1 = fx + 0.5 + f64::from(e1[0]) * 0.5;
        let z1 = fz + 0.5 + f64::from(e1[2]) * 0.5;
        let (xd, zd) = (x1 - x0, z1 - z0);
        let progress = if xd == 0.0 {
            z - fz
        } else if zd == 0.0 {
            x - fx
        } else {
            ((x - x0) * xd + (z - z0) * zd) * 2.0
        };
        c.position = Vec3::new(x0 + xd * progress, y, z0 + zd * progress);
        let scale = if ridden { 0.75 } else { 1.0 };
        let max = self.max_speed(c);
        let delta = Vec3::new(
            (scale * c.velocity.x).clamp(-max, max),
            0.0,
            (scale * c.velocity.z).clamp(-max, max),
        );
        // Entity.move records this step at the rail's top height; the tick's
        // applyEffectsFromBlocks sweeps it, whatever the cart does after.
        let step_from = c.position;
        self.move_cart(c, delta);
        let step = (step_from, c.position);
        let at = |e: [i32; 3], c: &Minecart| {
            c.position.x.floor() as i32 - pos[0] == e[0]
                && c.position.z.floor() as i32 - pos[2] == e[2]
        };
        if e0[1] != 0 && at(e0, c) {
            c.position.y += f64::from(e0[1]);
        } else if e1[1] != 0 && at(e1, c) {
            c.position.y += f64::from(e1[1]);
        }
        c.velocity = self.natural_slowdown(c, ridden, 0.96);
        if let (Some(new), Some(old)) =
            (self.rail_pos(c.position.x, c.position.y, c.position.z), old)
        {
            let speed = (old[1] - new[1]) * 0.05;
            let h = c.horizontal();
            if h > 0.0 {
                c.velocity.x *= (h + speed) / h;
                c.velocity.z *= (h + speed) / h;
            }
            c.position.y = new[1];
        }
        let (xn, zn) = (c.position.x.floor() as i32, c.position.z.floor() as i32);
        if xn != pos[0] || zn != pos[2] {
            let h = c.horizontal();
            c.velocity = Vec3::new(
                h * f64::from(xn - pos[0]),
                c.velocity.y,
                h * f64::from(zn - pos[2]),
            );
        }
        if power {
            let h = c.horizontal();
            if h > 0.01 {
                c.velocity.x += c.velocity.x / h * 0.06;
                c.velocity.z += c.velocity.z / h * 0.06;
            } else if rail.shape == Shape::EastWest {
                if self.conductor(pos[0] - 1, pos[1], pos[2]) {
                    c.velocity.x = 0.02;
                } else if self.conductor(pos[0] + 1, pos[1], pos[2]) {
                    c.velocity.x = -0.02;
                }
            } else if rail.shape == Shape::NorthSouth {
                if self.conductor(pos[0], pos[1], pos[2] - 1) {
                    c.velocity.z = 0.02;
                } else if self.conductor(pos[0], pos[1], pos[2] + 1) {
                    c.velocity.z = -0.02;
                }
            }
        }
        step
    }
    /// `Entity.checkInsideBlocks` for a recorded move of the cart from `from`
    /// to `to`: the entity-sensing blocks (plates, buttons, tripwires, detector
    /// rails) on the path run `entityInside` now, with the cart where the tick
    /// left it. `slot` is where the cart is stored.
    fn cart_inside_blocks(&mut self, c: &mut Minecart, slot: usize, from: Vec3, to: Vec3) {
        let shrink = f64::from(1.0e-5f32);
        let half = WIDTH / 2.0;
        let bounds = Aabb::new(
            Vec3::new(to.x - half, to.y, to.z - half),
            Vec3::new(to.x + half, to.y + HEIGHT, to.z + half),
        )
        .deflate(Vec3::new(shrink, shrink, shrink));
        let mut hits = Vec::new();
        let behaviours = self.res.behaviours();
        let world = &self.world;
        let _ = horizon_core::geometry::sweep_intersected_blocks(
            from,
            to,
            bounds,
            1_000_000,
            |p, _| {
                if behaviours.senses_entities(world.block(p.x, p.y, p.z)) {
                    hits.push(p);
                }
                true
            },
        );
        if hits.is_empty() {
            return;
        }
        let Some(i) = cart_at(&self.carts, c.id, slot) else {
            return;
        };
        // The stored cart is the one a plate counts: it takes the moved cart
        // for the walk and hands back what the walk left.
        std::mem::swap(&mut self.carts[i], c);
        self.with_level(|b, level, ticks| {
            for p in hits {
                b.entity_inside(level, ticks, p);
            }
        });
        std::mem::swap(&mut self.carts[i], c);
        let _ = self.process_level_events();
    }
    /// `AbstractMinecart`: `minecart_improvements` picks the smoother
    /// physics.
    pub(super) fn minecart_improvements(&self) -> bool {
        self.remote
            .as_ref()
            .map_or(&self.level.enabled_features, |remote| &remote.features)
            .iter()
            .any(|f| f == "minecraft:minecart_improvements")
    }
    /// `NewMinecartBehavior.getMaxSpeed`: the game rule over water's half,
    /// every cart alike (the old behavior's furnace bonus is its own).
    fn max_speed_new(&self, c: &Minecart) -> f64 {
        f64::from(self.ui.rules.int("max_minecart_speed")) * if c.in_water { 0.5 } else { 1.0 }
            / 20.0
    }
    /// `AbstractMinecart.getRedstoneDirection` (over the old port's
    /// conductor proxy): which way a powered rail pushes a resting cart.
    fn redstone_direction(&self, pos: [i32; 3], shape: Shape) -> Vec3 {
        let (dx, dz) = match shape {
            Shape::NorthSouth => (0, 1),
            Shape::EastWest => (1, 0),
            _ => return Vec3::ZERO,
        };
        // The push points away from the signal's side (getRedstoneDirection).
        if self.conductor(pos[0] - dx, pos[1], pos[2] - dz) {
            Vec3::new(f64::from(dx), 0.0, f64::from(dz))
        } else if self.conductor(pos[0] + dx, pos[1], pos[2] + dz) {
            Vec3::new(f64::from(-dx), 0.0, f64::from(-dz))
        } else {
            Vec3::ZERO
        }
    }
    /// `NewMinecartBehavior.isDecending`: the movement goes down the
    /// shape's rise.
    fn is_descending(movement: Vec3, shape: Shape) -> bool {
        match shape {
            Shape::AscendingEast => movement.x < 0.0,
            Shape::AscendingWest => movement.x > 0.0,
            Shape::AscendingNorth => movement.z > 0.0,
            Shape::AscendingSouth => movement.z < 0.0,
            _ => false,
        }
    }
    /// `NewMinecartBehavior.setRotation`: preserve the cart's facing when
    /// motion reverses, including the pitch sign on a slope.
    fn set_cart_rotation(c: &mut Minecart, mut yaw: f32, mut pitch: f32) {
        if (175.0..=185.0).contains(&f64::from((yaw - c.yaw).abs())) {
            c.flipped = !c.flipped;
            yaw -= 180.0;
            pitch = -pitch;
        }
        c.yaw = yaw % 360.0;
        c.pitch = pitch.clamp(-45.0, 45.0) % 360.0;
    }
    /// `NewMinecartBehavior.MinecartStep`: wire rotations use ROTATION_BYTE.
    fn record_cart_step(c: &mut Minecart, weight: f32) {
        use horizon_protocol::buf::pack_degrees;
        c.lerp_steps
            .push(horizon_protocol::packets::play::MinecartStep {
                position: c.position.components(),
                movement: c.velocity.components(),
                yaw: pack_degrees(c.yaw),
                pitch: pack_degrees(c.pitch),
                weight,
            });
    }
    /// `NewMinecartBehavior.moveAlongTrack`: each step has its own rotation
    /// and interpolation weight, including the final off-rail move.
    fn finish_cart_step(&self, c: &mut Minecart, old: Vec3, initial: Vec3) {
        let delta = old.vector_to(c.position);
        let length = delta.length();
        if length > f64::from(1.0e-5_f32) {
            if delta.horizontal_distance_squared() > f64::from(1.0e-5_f32) {
                let yaw = 180.0 - java_degrees(delta.z, delta.x) as f32
                    + if c.flipped { 180.0 } else { 0.0 };
                let pitch = if c.on_ground && !c.on_rails {
                    0.0
                } else {
                    (90.0 - java_degrees(delta.horizontal_distance(), delta.y) as f32)
                        * if c.flipped { -1.0 } else { 1.0 }
                };
                Self::set_cart_rotation(c, yaw, pitch);
            } else if !c.on_rails {
                c.pitch = if c.on_ground {
                    0.0
                } else {
                    c.pitch + 0.2 * ((-c.pitch + 180.0).rem_euclid(360.0) - 180.0)
                };
            }
            Self::record_cart_step(c, length.min(self.max_speed_new(c)) as f32);
        } else if initial.horizontal_distance_squared() > 0.0 {
            Self::record_cart_step(c, 1.0);
        }
    }
    /// `NewMinecartBehavior.adjustToRails`: snap only on the first tick or
    /// when entering rails; curves project onto the segment between exits.
    fn adjust_cart_to_rails(c: &mut Minecart, pos: [i32; 3], shape: Shape) {
        let (e0, e1) = shape.exits();
        let exit =
            |e: [i32; 3]| Vec3::new(f64::from(e[0]), f64::from(e[1]), f64::from(e[2])).scale(0.5);
        let old = c.position;
        let (exit0, exit1) = (exit(e0), exit(e1));
        let center = Vec3::new(
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]),
            f64::from(pos[2]) + 0.5,
        );
        let mut out = Vec3::new(exit0.x, 0.0, exit0.z);
        let mut into = Vec3::new(exit1.x, 0.0, exit1.z);
        if c.velocity.length() > f64::from(1.0e-5_f32) && c.velocity.dot(out) < c.velocity.dot(into)
            || Self::is_descending(into, shape)
        {
            std::mem::swap(&mut out, &mut into);
        }
        let mut yaw = 180.0 - java_degrees(out.z, out.x) as f32;
        if exit0.x != exit1.x && exit0.z != exit1.z {
            let along = exit1.subtract(exit0);
            let from = c.position.subtract(center).subtract(exit0);
            let projected = along.scale(along.dot(from) / along.dot(along));
            c.position = center.add_vector(exit0).add_vector(projected);
            yaw = 180.0 - java_degrees(projected.z, projected.x) as f32;
        } else {
            if exit0.z != exit1.z {
                c.position.x = center.x;
            }
            if exit0.x != exit1.x {
                c.position.z = center.z;
            }
            c.position.y = center.y;
        }
        let pitch = if shape.is_slope() {
            c.position.y += center.add_vector(into).distance(c.position) + 0.1;
            if c.flipped { 45.0 } else { -45.0 }
        } else {
            c.position.y += 0.1;
            0.0
        };
        Self::set_cart_rotation(c, yaw + if c.flipped { 180.0 } else { 0.0 }, pitch);
        let distance = old.distance(c.position);
        if distance > 0.0 {
            Self::record_cart_step(c, if c.first_tick { 0.0 } else { distance as f32 });
        }
    }
    /// `NewMinecartBehavior.moveAlongTrack`: the iteration keeps moving
    /// while distance is left, over as many rails as it takes, the speed
    /// recomputed per rail (slope, halt, boost).
    fn move_along_track_new(&mut self, c: &mut Minecart, intent: [f64; 2]) {
        // `TrackIteration`: the slope, halt and boost speeds each apply once
        // per tick, and the iteration keeps moving while distance is left.
        let mut first = true;
        let mut movement_left = 0.0;
        let mut slope_done = false;
        let mut halted = false;
        let mut boosted = false;
        let mut step_start = c.previous;
        while first || movement_left > f64::from(1.0e-5_f32) {
            let initial = c.velocity;
            let pos = self.rail_block(c.position);
            let Some(rail) = self.rail(pos[0], pos[1], pos[2]) else {
                c.on_rails = false;
                self.come_off_track(c);
                self.finish_cart_step(c, step_start, initial);
                if first || c.position.distance(step_start) > f64::from(1.0e-5_f32) {
                    self.cart_block_effects(c);
                    self.cart_block_effects(c);
                }
                return;
            };
            if !c.on_rails {
                c.on_rails = true;
                Self::adjust_cart_to_rails(c, pos, rail.shape);
            }
            c.fall_distance = 0.0;
            step_start = c.position;
            if &*rail.name == "minecraft:activator_rail" {
                self.activate_cart(c, rail.powered);
            }
            let ridden = self.cart_is_vehicle(c.id);
            let initial_h = c.velocity.horizontal_distance();
            let mut v = Vec3::new(c.velocity.x, 0.0, c.velocity.z);
            // calculateSlopeSpeed (once per tick): the slide grows with the
            // speed, so a descending cart keeps its pace.
            if !slope_done {
                let slide =
                    0.0078125_f64.max(initial_h * 0.02) * if c.in_water { 0.2 } else { 1.0 };
                let before = v.horizontal_distance_squared();
                match rail.shape {
                    Shape::AscendingEast => v.x -= slide,
                    Shape::AscendingWest => v.x += slide,
                    Shape::AscendingNorth => v.z += slide,
                    Shape::AscendingSouth => v.z -= slide,
                    _ => {}
                }
                slope_done = v.horizontal_distance_squared() != before;
                if !slope_done {
                    v = Vec3::new(c.velocity.x, 0.0, c.velocity.z);
                }
            }
            // calculatePlayerInputSpeed (first iteration).
            if first
                && ridden
                && (intent[0] != 0.0 || intent[1] != 0.0)
                && v.x * v.x + v.z * v.z < 0.01
            {
                let input = Vec3::new(intent[0], 0.0, intent[1])
                    .normalize()
                    .scale(0.001);
                let pushed = v.add_vector(input);
                if pushed.horizontal_distance_squared() != v.horizontal_distance_squared() {
                    v = pushed;
                    halted = true;
                }
            }
            // calculateHaltTrackSpeed (until the input halted).
            if !halted && &*rail.name == "minecraft:powered_rail" && !rail.powered {
                let before = v.x.hypot(v.z);
                if before < 0.03 {
                    v = Vec3::ZERO;
                } else {
                    v = Vec3::new(v.x * 0.5, v.y, v.z * 0.5);
                }
                halted = v.x.hypot(v.z) != before;
            }
            // applyNaturalSlowdown and the speed cap (first iteration).
            if first {
                c.velocity = v;
                v = self.natural_slowdown(c, ridden, 0.975);
                if v.length_squared() > 0.0 {
                    let speed = v.length().min(self.max_speed_new(c));
                    v = v.normalize().scale(speed);
                }
            }
            // calculateBoostTrackSpeed (once per tick).
            if !boosted && &*rail.name == "minecraft:powered_rail" && rail.powered {
                let before = v;
                let len = v.length();
                if len > 0.01 {
                    v = v.normalize().scale(len + 0.06);
                } else {
                    let dir = self.redstone_direction(pos, rail.shape);
                    if dir != Vec3::ZERO {
                        v = dir.scale(len + 0.2);
                    }
                }
                boosted = v.horizontal_distance_squared() != before.horizontal_distance_squared();
                if !boosted {
                    v = before;
                }
            }
            c.velocity = v;
            let new_h = v.horizontal_distance();
            movement_left = if first {
                new_h
            } else {
                movement_left + (new_h - initial_h)
            };
            movement_left = self.new_step_along_track(c, pos, &rail, movement_left);
            self.finish_cart_step(c, step_start, initial);
            // NewMinecartBehavior.moveAlongTrack applies block effects twice,
            // after recording the step, including a stationary first step.
            if first || c.position.distance(step_start) > f64::from(1.0e-5_f32) {
                self.cart_block_effects(c);
                self.cart_block_effects(c);
            }
            first = false;
        }
    }
    /// `NewMinecartBehavior.stepAlongTrack`: toward the rail's exit, the
    /// hill's Y projected, a V of opposing slopes resting a slow cart.
    fn new_step_along_track(
        &mut self,
        c: &mut Minecart,
        pos: [i32; 3],
        rail: &Rail,
        mut movement_left: f64,
    ) -> f64 {
        if movement_left < f64::from(1.0e-5_f32) {
            return 0.0;
        }
        let old = c.position;
        let (e0, e1) = rail.shape.exits();
        let movement = Vec3::new(c.velocity.x, 0.0, c.velocity.z);
        if movement.length() < f64::from(1.0e-5_f32) {
            c.velocity = Vec3::ZERO;
            return 0.0;
        }
        let in_hill = e0[1] != e1[1];
        let mut out_dir = Vec3::new(f64::from(e0[0]) * 0.5, 0.0, f64::from(e0[2]) * 0.5);
        let in_dir = Vec3::new(f64::from(e1[0]) * 0.5, 0.0, f64::from(e1[2]) * 0.5);
        if movement.dot(out_dir) < movement.dot(in_dir) {
            out_dir = in_dir;
        }
        let out_len = out_dir.x.hypot(out_dir.z);
        let mut out_pos = Vec3::new(
            f64::from(pos[0]) + 0.5 + out_dir.x + out_dir.x / out_len * f64::from(1.0e-5_f32),
            f64::from(pos[1]) + 0.1,
            f64::from(pos[2]) + 0.5 + out_dir.z + out_dir.z / out_len * f64::from(1.0e-5_f32),
        );
        if in_hill && !Self::is_descending(c.velocity, rail.shape) {
            out_pos.y += 1.0;
        }
        let towards_out = c.position.vector_to(out_pos).normalize();
        // The movement turns onto the exit's direction, at the same speed
        // (`this.setDeltaMovement(movement)` at stepAlongTrack's end).
        let h_speed = movement.x.hypot(movement.z);
        let th = towards_out.x.hypot(towards_out.z);
        let movement = towards_out.scale(h_speed / th);
        let mut new_pos = if in_hill {
            old.add_vector(
                movement
                    .normalize()
                    .scale(movement_left * f64::from(std::f32::consts::SQRT_2)),
            )
        } else {
            old.add_vector(movement.normalize().scale(movement_left))
        };
        if out_pos.distance(old) <= new_pos.distance(old) {
            movement_left = out_pos.vector_to(new_pos).horizontal_distance();
            new_pos = out_pos;
        } else {
            movement_left = 0.0;
        }
        self.move_cart(c, old.vector_to(new_pos));
        if in_hill {
            let new_block = [
                new_pos.x.floor() as i32,
                new_pos.y.floor() as i32,
                new_pos.z.floor() as i32,
            ];
            if let Some(new_rail) = self.rail(new_block[0], new_block[1], new_block[2]) {
                // restAtVShape: opposing slopes rest a slow cart.
                if c.velocity.length_squared() < 0.005
                    && new_rail.shape.is_slope()
                    && Self::is_descending(c.velocity, rail.shape)
                    && !Self::is_descending(c.velocity, new_rail.shape)
                {
                    c.velocity = Vec3::ZERO;
                    return 0.0;
                }
            }
            let h_dist = (out_pos.x - c.position.x).hypot(out_pos.z - c.position.z);
            let project = out_pos.y
                + if Self::is_descending(movement, rail.shape) {
                    h_dist
                } else {
                    -h_dist
                };
            if c.position.y < project {
                c.position.y = project;
            }
        }
        // The cart did not move but the step would have: it is stuck.
        if c.position.distance(old) < f64::from(1.0e-5_f32)
            && new_pos.distance(old) > f64::from(1.0e-5_f32)
        {
            c.velocity = Vec3::ZERO;
            return 0.0;
        }
        c.velocity = movement;
        movement_left
    }
    /// `comeOffTrack`.
    fn come_off_track(&self, c: &mut Minecart) {
        let max = if self.minecart_improvements() {
            self.max_speed_new(c)
        } else {
            self.max_speed(c)
        };
        c.velocity.x = c.velocity.x.clamp(-max, max);
        c.velocity.z = c.velocity.z.clamp(-max, max);
        if c.on_ground {
            c.velocity = Vec3::new(c.velocity.x * 0.5, c.velocity.y * 0.5, c.velocity.z * 0.5);
        }
        let v = c.velocity;
        self.move_cart(c, v);
        // Entity.move's last step: soul sand and honey slow a derailed cart.
        // The rail paths set the track's velocity again after their move, so
        // the factor changes nothing there (measured on a rail over soul sand).
        let factor = crate::motion::block_speed_factor_at(
            &self.world,
            &self.motion.blocks,
            c.position,
            None,
        );
        c.velocity = horizon_world::entity_move::scale_by_block_speed(c.velocity, factor);
        if !c.on_ground {
            // scale(getAirDrag()): 0.95F, widened.
            let drag = f64::from(0.95_f32);
            c.velocity = Vec3::new(
                c.velocity.x * drag,
                c.velocity.y * drag,
                c.velocity.z * drag,
            );
        }
    }
    /// `applyEffectsFromBlocks` for a cart: a bubble column lifts or drags it
    /// and resets the fall distance.
    fn cart_block_effects(&self, c: &mut Minecart) {
        if c.push_by_bubble_column(
            &self.world,
            self.pack.assets.registry,
            self.res.facts(),
            self.collision,
        ) {
            c.fall_distance = 0.0;
        }
        // WebBlock.entityInside: makeStuckInBlock(0.25, 0.05, 0.25) for every
        // cobweb the box (deflated by 1.0E-5) is in; it resets the fall distance.
        let b = c.bounds().deflate(Vec3::new(1e-5, 1e-5, 1e-5));
        let (lo, hi) = (b.min_position(), b.max_position());
        let facts = self.res.facts();
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    if &*facts.get(self.world.block(x, y, z)).name == "minecraft:cobweb" {
                        c.fall_distance = 0.0;
                        c.stuck = Vec3::new(0.25, f64::from(0.05_f32), 0.25);
                    }
                }
            }
        }
    }
    /// Runs `push_from_cart` for the cart being ticked (`c` is a copy of the
    /// stored one, kept at `slot`): the stored cart is replaced by `c` for the
    /// push and `c` takes back what the push changed. The rolled loot stays
    /// with `c`: the stored cart is without it for the whole tick
    /// (`tick_carts_now`), and a mineshaft's chest cart carries its whole roll
    /// as NBT.
    fn push_from_ticked_cart(&mut self, c: &mut Minecart, slot: usize, index: &mut CartIndex) {
        let Some(i) = cart_at(&self.carts, c.id, slot) else {
            return;
        };
        let loot = c.loot.take();
        if !horizon_client::mod_api::mods().is_empty() {
            // A mod's mount hook can change the list during the push: the
            // cart is found again by id after it.
            self.carts[i] = c.clone();
            self.push_from_cart(i, index);
            if let Some(i) = self.carts.iter().position(|x| x.id == c.id) {
                *c = self.carts[i].clone();
            }
            c.loot = loot;
            return;
        }
        // Without mods, pushing and boarding change velocities and riders,
        // never the list: the ticked cart moves into its slot, and the copy it
        // replaces takes back the pushed cart (reusing its buffers), instead
        // of two searches and two whole copies.
        std::mem::swap(&mut self.carts[i], c);
        self.push_from_cart(i, index);
        debug_assert_eq!(self.carts[i].id, c.id, "a push moved the cart list");
        c.clone_from(&self.carts[i]);
        c.loot = loot;
    }
    /// `AbstractMinecart.tick` for one cart; returns false once it is gone.
    /// `slot` is where the cart is stored (where to look first).
    fn tick_cart(
        &mut self,
        c: &mut Minecart,
        intent: [f64; 2],
        slot: usize,
        index: &mut CartIndex,
    ) -> bool {
        c.tick_count += 1;
        c.previous = c.position;
        c.previous_yaw = c.yaw;
        c.previous_pitch = c.pitch;
        c.hurt_time = (c.hurt_time - 1).max(0);
        c.damage = (c.damage - 1.0).max(0.0);
        // NewMinecartBehavior.tick snaps a newly created cart before gravity.
        if self.minecart_improvements() && c.first_tick {
            let pos = self.rail_block(c.position);
            if let Some(rail) = self.rail(pos[0], pos[1], pos[2]) {
                c.on_rails = true;
                Self::adjust_cart_to_rails(c, pos, rail.shape);
            } else {
                c.on_rails = false;
            }
        }
        // applyGravity
        c.velocity.y -= c.get_gravity();
        let pos = self.rail_block(c.position);
        let mut step = None;
        if self.minecart_improvements() {
            self.move_along_track_new(c, intent);
        } else if let Some(rail) = self.rail(pos[0], pos[1], pos[2]) {
            step = Some(self.move_along_track(c, pos, &rail, intent));
            if &*rail.name == "minecraft:activator_rail" {
                self.activate_cart(c, rail.powered);
            }
        } else {
            self.come_off_track(c);
        }
        c.first_tick = false;
        // The old behavior derives yaw once per tick; the experiment has
        // already recorded each step's yaw and pitch.
        if !self.minecart_improvements() {
            let (xd, zd) = (c.previous.x - c.position.x, c.previous.z - c.position.z);
            if xd * xd + zd * zd > 0.001 {
                c.yaw = horizon_core::geometry::trig::atan2_degrees_double(zd, xd);
                if c.flipped {
                    c.yaw += 180.0;
                }
            }
            let diff = (c.yaw - c.previous_yaw + 180.0).rem_euclid(360.0) - 180.0;
            if !(-170.0..170.0).contains(&diff) {
                c.yaw += 180.0;
                c.flipped = !c.flipped;
            }
            c.yaw %= 360.0;
        }
        if !self.minecart_improvements() {
            self.cart_block_effects(c);
            // applyEffectsFromBlocks over the step moveAlongTrack recorded.
            if let Some((from, to)) = step {
                self.cart_inside_blocks(c, slot, from, to);
            }
        }
        // OldMinecartBehavior.tick ends with pushAndPickupEntities, before
        // AbstractMinecart.tick's updateFluidInteraction. The experiment still
        // uses this fallback until its per-move pickup/push retry is ported.
        self.push_from_ticked_cart(c, slot, index);
        // AbstractMinecart.tick: updateFluidInteraction, a stream pushes the cart.
        c.in_water = c.push_by_fluid(&self.world, &self.res.motion().fluids).0 > 0.0;
        // updateFluidInteraction: being in water resets the fall distance.
        if c.in_water {
            c.fall_distance = 0.0;
        }
        // Entity.checkInsideBlocks runs once after the cart tick, sweeping every
        // movement through powder snow. PowderSnowBlock uses getKnownMovement,
        // which is getDeltaMovement for a minecart.
        self.entity_powder_snow_particles(c.previous, c.position, c.bounds(), c.velocity, false);
        // Variants.
        match c.kind {
            CartKind::Furnace => {
                if c.fuel > 0 {
                    c.fuel -= 1;
                }
                if c.fuel <= 0 {
                    c.push = Vec3::ZERO;
                }
                if c.fuel > 0 && c.random.next_int_bound(4).unwrap_or(1) == 0 {
                    let at = [c.position.x, c.position.y + 0.8, c.position.z];
                    self.particles.add(
                        &ParticleOptions::simple(ParticleType::LargeSmoke),
                        at,
                        [0.0; 3],
                    );
                }
            }
            CartKind::Tnt => {
                // MinecartTNT.causeFallDamage ran inside the move.
                if let Some(speed_sqr) = c.fall_blast.take() {
                    self.explode_cart(c, speed_sqr);
                    return false;
                }
                // MinecartTNT.tick: the fuse and the collision check are separate.
                if c.fuse > 0 {
                    c.fuse -= 1;
                    let at = [c.position.x, c.position.y + 0.5, c.position.z];
                    self.particles
                        .add(&ParticleOptions::simple(ParticleType::Smoke), at, [0.0; 3]);
                } else if c.fuse == 0 {
                    // explode(horizontalDistanceSqr()): a rising cart's
                    // vertical speed adds nothing.
                    let speed = c.velocity.x * c.velocity.x + c.velocity.z * c.velocity.z;
                    self.explode_cart(c, speed);
                    return false;
                }
                // MinecartTNT.tick reads the delta left after AbstractMinecart.tick
                // (pushes and fluids included). The move zeroed the blocked axes
                // (Entity.restituteMovementAfterCollisions) and no track step puts
                // them back: a head-on stop leaves nothing, a diagonal one keeps
                // the free axis (#11380).
                let speed = c.velocity.horizontal_distance_squared();
                if tnt_ignites_by_collision(c.horizontal_collision, speed) {
                    self.explode_cart(c, speed);
                    return false;
                }
            }
            // MinecartSpawner.tick: `BaseSpawner.serverTick` at the cart's block.
            CartKind::Spawner => {
                if let Some(mut spawner) = c.spawner.take() {
                    let key = (
                        c.position.x.floor() as i32,
                        c.position.y.floor() as i32,
                        c.position.z.floor() as i32,
                    );
                    let _ = self.server_tick_spawner_of(key, &mut spawner, Some(c.id));
                    c.spawner = Some(spawner);
                }
            }
            _ => {}
        }
        true
    }
    /// `activateMinecart` on an activator rail.
    fn activate_cart(&mut self, c: &mut Minecart, powered: bool) {
        match c.kind {
            CartKind::Rideable if powered => {
                // Minecart.activateMinecart: ejectPassengers.
                if self.riding == Some(c.id) {
                    self.riding = None;
                }
                let y = c.position.y;
                for m in self.mobs.iter_mut().filter(|m| m.extra.mount == Some(c.id)) {
                    let at = Vec3::new(m.position.x, m.position.y.max(y), m.position.z);
                    riders::stop_riding(m, Some(at));
                    m.previous = at;
                }
                if c.hurt_time == 0 {
                    c.hurt_dir = -c.hurt_dir;
                    c.hurt_time = 10;
                    c.damage = 50.0;
                }
            }
            CartKind::Tnt if powered && c.fuse < 0 => self.prime_cart(c),
            CartKind::Hopper => c.enabled = !powered,
            // MinecartCommandBlock.activateMinecart: the command runs at most
            // every ACTIVATION_DELAY ticks.
            CartKind::CommandBlock if powered && c.tick_count - c.last_activated >= 4 => {
                c.command_due = true;
                c.last_activated = c.tick_count;
            }
            _ => {}
        }
    }
    /// `MinecartTNT.primeFuse`: fuse 80, then `Level.broadcastEntityEvent`
    /// (`EntityEvent.TNT_PRIME`, 70) and `SoundEvents.TNT_PRIMED`. A client
    /// that receives 70 only sets the fuse (`handleEntityEvent`).
    fn prime_cart(&mut self, c: &mut Minecart) {
        if !self.ui.rules.bool("tnt_explodes") {
            return;
        }
        c.fuse = 80;
        let at = [c.position.x, c.position.y, c.position.z];
        let id = c.id as i32;
        self.lan_entity_event(id, TNT_PRIME_EVENT, at);
        self.audio
            .play("minecraft:entity.tnt.primed", Some(at), 1.0, 1.0);
    }
    /// `MinecartTNT.handleEntityEvent`: event 70 calls `primeFuse`, which on a
    /// client only sets the fuse to 80. The window then counts it down
    /// (`MinecartTNT.tick`) for the smoke, the flash and the swell.
    pub(super) fn remote_tnt_cart_event(&mut self, id: i32, event: i8) {
        if event != TNT_PRIME_EVENT {
            return;
        }
        if let Some(c) = self
            .carts
            .iter_mut()
            .find(|c| c.id == id as u32 && c.kind == CartKind::Tnt)
        {
            c.fuse = 80;
        }
    }
    fn explode_cart(&mut self, c: &mut Minecart, speed_sqr: f64) {
        if !self.ui.rules.bool("tnt_explodes") {
            return;
        }
        let speed = speed_sqr.sqrt().min(5.0);
        let r = c.random.next_double();
        // MinecartTNT.explode: the base power plus the speed's share.
        let power = (f64::from(c.explosion_power)
            + f64::from(c.explosion_speed_factor) * r * 1.5 * speed) as f32;
        let at = [c.position.x, c.position.y, c.position.z];
        // The cart is the explosion's source: it is gone before the blast
        // looks for entities, so it can't hit itself.
        let silent = self.entity_silent(c.id);
        if let Some(i) = self.carts.iter().position(|x| x.id == c.id) {
            self.eject_cart_mobs(i);
        }
        self.release_vehicle_container(c.id);
        self.carts.retain(|x| x.id != c.id);
        // ExplosionInteraction.TNT: the tnt_explosion_drop_decay rule.
        let interaction = self.destroy_type("tnt_explosion_drop_decay");
        // MinecartTNT.isPrimed (fuse > -1): a cart that ran into a wall or
        // fell unprimed blows rails up like any blast.
        self.combat.rail_proof_blast = c.fuse >= 0;
        self.combat.explosion_in_water = self.bounds_in_water(c.bounds());
        let _ = self.with_hive_emptying_blast(|g| {
            g.server_explode_from(
                at,
                power,
                interaction,
                false,
                super::explosion::ExplosionSource::default()
                    .with_silence(silent)
                    .with_entity(c.id, "minecraft:tnt_minecart"),
            )
        });
        self.combat.explosion_in_water = false;
        self.combat.rail_proof_blast = false;
    }
    /// `AbstractMinecart.push` from each player walking into a cart
    /// (`LivingEntity.pushEntities`): the window's player and every server
    /// player (single player's own on its server thread, LAN guests). A
    /// player in a vehicle pushes nothing here. The quarter push the player
    /// gets back only moves the server's copy of it (`Entity.push` marks
    /// `needsSync`, which `ServerEntity` sends to the other players only),
    /// so no player feels it.
    fn push_carts(&mut self) {
        if self.carts.is_empty() {
            return;
        }
        // (feet, box)
        let mut pushers: Vec<(Vec3, Aabb)> = Vec::new();
        if self.riding.is_none()
            && let Some(p) = self.player.as_ref()
        {
            pushers.push((p.position, p.bounds()));
        }
        if let Some(host) = self.lan.as_ref() {
            for g in host.hub.guests.iter().filter(|g| {
                host.hub.here(g) && g.loaded && !g.dead && !g.spectator() && g.vehicle.is_none()
            }) {
                let [x, y, z] = g.position;
                pushers.push((Vec3::new(x, y, z), g.bounds()));
            }
        }
        let cells =
            horizon_client::mobs::seen_grid::SeenGrid::new(self.carts.iter().map(Minecart::bounds));
        let mut near = Vec::new();
        for (feet, bb) in pushers {
            if !cells.candidates(bb, &mut near) {
                near.extend(0..self.carts.len() as u32);
            }
            for &k in &near {
                let k = k as usize;
                let c = &mut self.carts[k];
                if !bb.intersects(c.bounds()) {
                    continue;
                }
                let (mut xa, mut za) = (feet.x - c.position.x, feet.z - c.position.z);
                let dd = xa * xa + za * za;
                if dd < f64::from(1.0e-4f32) {
                    continue;
                }
                let d = dd.sqrt();
                let pow = (1.0 / d).min(1.0);
                xa = xa / d * pow * f64::from(0.1f32) * 0.5;
                za = za / d * pow * f64::from(0.1f32) * 0.5;
                c.velocity.x -= xa;
                c.velocity.z -= za;
            }
        }
        self.push_carts_from_mobs(&cells);
    }
    /// `AbstractMinecart.push(mob)` from each mob walking into a cart
    /// (`LivingEntity.pushEntities` -> `doPush`): the cart gets the push and
    /// the mob a quarter of it back (`Entity.push`, which needs the mob to be
    /// pushable and to carry no one). Bats push nothing (`Bat.pushEntities`).
    fn push_carts_from_mobs(&mut self, cells: &horizon_client::mobs::seen_grid::SeenGrid) {
        let teams = self.mob_teams();
        // (mob index, feet, box)
        let pushers: Vec<(usize, Vec3, Aabb)> = self
            .mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                !m.removed
                    && m.alive()
                    && m.species() != Species::Bat
                    && pushable_by(teams.of(Who::Mob(m.id)), None, true)
            })
            .map(|(j, m)| (j, m.position, m.bounds()))
            .collect();
        drop(teams);
        let mut near = Vec::new();
        for (j, feet, bb) in pushers {
            if !cells.candidates(bb, &mut near) {
                near.extend(0..self.carts.len() as u32);
            }
            for &k in &near {
                let k = k as usize;
                let c = &mut self.carts[k];
                // hasPassenger(mob): a mob in this cart never pushes it.
                if self.mobs[j].extra.mount == Some(c.id) || !bb.intersects(c.bounds()) {
                    continue;
                }
                let Some((xa, za)) = cart_push_from(feet, c.position) else {
                    continue;
                };
                c.velocity.x -= xa;
                c.velocity.z -= za;
                let m = &mut self.mobs[j];
                if m.pushable() && m.rider.is_none() && m.extra.rider.is_none() {
                    m.velocity.x += xa / 4.0;
                    m.velocity.z += za / 4.0;
                    m.needs_sync = true;
                }
            }
        }
    }
    /// `OldMinecartBehavior.pushAndPickupEntities` for carts, at the end of
    /// cart `i`'s own tick: each other cart in its box (0.2 wider on each
    /// side) gets `push(cart i)`. Running it per cart, after that cart moved
    /// and before the next one does, is what splits stacked carts apart.
    fn push_from_cart(&mut self, i: usize, index: &mut CartIndex) {
        let reach = f64::from(0.2f32);
        let hitbox = self.carts[i].bounds().inflate(Vec3::new(reach, 0.0, reach));
        let mut near = std::mem::take(&mut index.near);
        if !index.carts_near(&self.carts, i, hitbox, &mut near) {
            near.clear();
            near.extend(0..self.carts.len() as u32);
        }
        for &j in &near {
            let j = j as usize;
            if j == i || !hitbox.intersects(self.carts[j].bounds()) {
                continue;
            }
            // AbstractMinecart.push(entity): `this` is cart j, `entity` cart i.
            let (this, other) = (&self.carts[j], &self.carts[i]);
            let (mut xa, mut za) = (
                other.position.x - this.position.x,
                other.position.z - this.position.z,
            );
            let dd = xa * xa + za * za;
            if dd < f64::from(1.0e-4f32) {
                continue;
            }
            let d = dd.sqrt();
            xa /= d;
            za /= d;
            let pow = (1.0 / d).min(1.0);
            xa = xa * pow * f64::from(0.1f32) * 0.5;
            za = za * pow * f64::from(0.1f32) * 0.5;
            // pushOtherMinecart (old movement): only along this cart's facing.
            let dir = glam::DVec2::new(
                other.position.x - this.position.x,
                other.position.z - this.position.z,
            )
            .normalize_or_zero();
            let r = f64::from(this.yaw * 0.017453292f32);
            let facing = glam::DVec2::new(f64::from(trig::cos(r)), f64::from(trig::sin(r)))
                .normalize_or_zero();
            if dir.dot(facing).abs() < f64::from(0.8f32) {
                continue;
            }
            let (mt, mo) = (this.velocity, other.velocity);
            let (ft, fo) = (
                this.kind == CartKind::Furnace,
                other.kind == CartKind::Furnace,
            );
            let (nt, no) = if fo && !ft {
                (
                    Vec3::new(mt.x * 0.2 + (mo.x - xa), mt.y, mt.z * 0.2 + (mo.z - za)),
                    Vec3::new(mo.x * 0.95, mo.y, mo.z * 0.95),
                )
            } else if !fo && ft {
                (
                    Vec3::new(mt.x * 0.95, mt.y, mt.z * 0.95),
                    Vec3::new(mo.x * 0.2 + (mt.x + xa), mo.y, mo.z * 0.2 + (mt.z + za)),
                )
            } else {
                let (xdd, zdd) = ((mo.x + mt.x) / 2.0, (mo.z + mt.z) / 2.0);
                (
                    Vec3::new(mt.x * 0.2 + (xdd - xa), mt.y, mt.z * 0.2 + (zdd - za)),
                    Vec3::new(mo.x * 0.2 + (xdd + xa), mo.y, mo.z * 0.2 + (zdd + za)),
                )
            };
            self.carts[j].velocity = nt;
            self.carts[i].velocity = no;
        }
        index.near = near;
        self.pickup_from_cart(i, index);
    }
    /// `OldMinecartBehavior.pushAndPickupEntities` for living entities in cart
    /// `i`'s box (0.2 wider on each side): only a rideable cart moving at
    /// `horizontalDistanceSqr() >= 0.01` picks up; an eligible mob boards an
    /// empty cart (`startRiding`); others get `Entity.push`.
    fn pickup_from_cart(&mut self, i: usize, index: &mut CartIndex) {
        let v = self.carts[i].velocity;
        if self.carts[i].kind != CartKind::Rideable || v.x * v.x + v.z * v.z < 0.01 {
            return;
        }
        let reach = f64::from(0.2f32);
        let id = self.carts[i].id;
        let pos = self.carts[i].position;
        let hitbox = self.carts[i].bounds().inflate(Vec3::new(reach, 0.0, reach));
        let mut near = std::mem::take(&mut index.near);
        let mounted = index.mobs_near(&self.mobs, id, hitbox, &mut near);
        let mut occupied = self.riding == Some(id)
            || self.ridden_by_guest(id)
            || mounted.unwrap_or_else(|| self.mobs.iter().any(|m| m.extra.mount == Some(id)));
        if mounted.is_none() {
            near.clear();
            near.extend(0..self.mobs.len() as u32);
        }
        // The box first: it turns away nearly every mob of the level.
        near.retain(|&j| {
            let m = &self.mobs[j as usize];
            !m.removed && m.extra.mount != Some(id) && hitbox.intersects(m.bounds())
        });
        let candidates: Vec<u32> = if near.is_empty() {
            Vec::new()
        } else {
            let teams = self.mob_teams();
            near.iter()
                .map(|&j| &self.mobs[j as usize])
                .filter(|m| m.pushable() && pushable_by(None, teams.of(Who::Mob(m.id)), true))
                .map(|m| m.id)
                .collect()
        };
        index.near = near;
        for mob in candidates {
            // By id: a mod's mount hook may have removed mobs since the list
            // was made, which moves the others' indices.
            let Some(j) = self.mobs.iter().position(|m| m.id == mob) else {
                continue;
            };
            let species = self.mobs[j].species();
            let player = &*self.mobs[j].kind.id == "minecraft:player";
            let cooldown = self.mobs[j].extra.boarding_cooldown;
            let passenger = self.mobs[j].extra.mount.is_some();
            if pickup_instead_of_push(species, passenger, occupied) && !player {
                // Entity.startRiding: a mod may cancel it (EntityMountEvent).
                if can_ride_cart(species, cooldown)
                    && self.native_mob_may_ride(mob, id, true)
                    && let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob)
                {
                    horizon_client::mobs::shulker::on_start_riding(m);
                    m.extra.mount = Some(id);
                    m.extra.passenger = true;
                    m.ai.nav.stop();
                    m.needs_sync = true;
                    occupied = true;
                    self.break_leash(mob, true, false);
                    self.note_started_riding(id);
                    self.vehicle_picked_up();
                }
                continue;
            }
            let other = self.mobs[j].position;
            let Some(v) = super::boat_passengers::push_impulse(pos, other) else {
                continue;
            };
            if !occupied {
                self.carts[i].velocity = self.carts[i].velocity.subtract(v);
            }
            let m = &mut self.mobs[j];
            if m.pushable() {
                m.velocity = m.velocity.add_vector(v);
                m.needs_sync = true;
            }
        }
    }
    /// `positionRider` for the mobs in minecarts: each sits on the cart's
    /// passenger attachment, less its own vehicle attachment.
    pub(super) fn seat_cart_mobs(&mut self, riding: &super::passengers::Riders) {
        for i in 0..self.carts.len() {
            let id = self.carts[i].id;
            let Some(js) = riding.get(&id) else {
                continue;
            };
            let (pos, prev) = (self.carts[i].position, self.carts[i].previous);
            let cart_box = self.carts[i].bounds();
            let seats: Vec<(usize, f64)> = js
                .iter()
                // Minecart's box is 0.98 wide.
                .map(|&j| (j, riders::vehicle_attachment(&self.mobs[j], 0.98)))
                .collect();
            for (j, offset) in seats {
                let m = &mut self.mobs[j];
                m.seated(id, offset, cart_box);
                m.position = cart_seat(pos, offset);
                m.previous = cart_seat(prev, offset);
                m.velocity = Vec3::ZERO;
                m.fall_distance = 0.0;
                m.on_ground = false;
                m.extra.passenger = true;
                m.ai.nav.stop();
            }
        }
    }
    /// `ejectPassengers` for the mobs in cart `i`.
    pub(super) fn eject_cart_mobs(&mut self, i: usize) {
        let (id, y) = (self.carts[i].id, self.carts[i].position.y);
        let js: Vec<usize> = (0..self.mobs.len())
            .filter(|&j| self.mobs[j].extra.mount == Some(id))
            .collect();
        for j in js {
            let at = {
                let m = &self.mobs[j];
                Vec3::new(m.position.x, m.position.y.max(y), m.position.z)
            };
            riders::stop_riding(&mut self.mobs[j], Some(at));
            self.mobs[j].previous = at;
        }
    }
    /// Every cart but the ridden one (which ticks with its rider).
    pub(super) fn tick_carts(&mut self) {
        self.vehicles_ticking(true);
        self.tick_carts_now();
        self.vehicles_ticking(false);
    }
    fn tick_carts_now(&mut self) {
        self.push_carts();
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        let mut round = self.native_tick_round();
        // A server player's minecart moves with its rider (lan_vehicles.rs).
        let ridden = self.guest_vehicles();
        // By id: an exploding cart can destroy or prime others mid-loop.
        let ids: Vec<u32> = self.carts.iter().map(|c| c.id).collect();
        let mut index = CartIndex::default();
        let mut hint = 0;
        for &id in &ids {
            let Some(i) = cart_at(&self.carts, id, hint) else {
                continue;
            };
            hint = i + 1;
            let p = self.carts[i].position;
            if Some(id) == self.riding
                || ridden.contains(&id)
                || !ticking(p.x.floor() as i32, p.z.floor() as i32)
            {
                continue;
            }
            let kind = self.carts[i].kind.id();
            if !self.native_tick_pre_one(&mut round, id, kind) {
                continue;
            }
            let Some(i) = cart_at(&self.carts, id, i) else {
                continue;
            };
            // The ticked copy takes the loot roll instead of copying it: only
            // opening the cart, a command or a save reads it, never a tick, and
            // a mineshaft's chest cart carries its whole roll as NBT. The
            // stored cart keeps its slots for whatever reads them meanwhile.
            let loot = self.carts[i].loot.take();
            let mut c = self.carts[i].clone();
            c.loot = loot;
            let alive = self.tick_cart(&mut c, [0.0; 2], i, &mut index);
            if c.kind == CartKind::Hopper && c.enabled {
                self.hopper_cart_suck(&mut c);
            }
            let Some(i) = cart_at(&self.carts, id, i) else {
                self.native_tick_post_one(&round, id, kind);
                continue;
            };
            if alive {
                let due = std::mem::take(&mut c.command_due);
                index.moved(i, c.bounds());
                self.carts[i] = c;
                if due {
                    self.perform_cart_command(id);
                    // A command can move, add or remove any cart.
                    index.forget_carts();
                }
            } else {
                self.eject_cart_mobs(i);
                self.release_vehicle_container(id);
                self.carts.remove(i);
            }
            // After the ticked cart is stored: a mod reads and changes the cart as it is now.
            self.native_tick_post_one(&round, id, kind);
        }
        // On the server for every cart, the ridden ones too.
        let mut hint = 0;
        for id in ids {
            if round.canceled(id) {
                continue;
            }
            if let Some(i) = cart_at(&self.carts, id, hint) {
                hint = i + 1;
                let p = self.carts[i].position;
                if ticking(p.x.floor() as i32, p.z.floor() as i32) {
                    self.cart_lava_tick(id, i);
                }
            }
        }
    }
    /// `lavaHurt` twice a tick in lava: from `LavaFluid.entityInside` in the
    /// behaviour's `applyEffectsFromBlocks`, then from `AbstractMinecart.tick`'s
    /// own `isInLava`. The second breaks the cart, or primes a TNT minecart
    /// (`MinecartTNT.destroy` for a fire source). A cart has no `baseTick`,
    /// so the fire `lavaIgnite` lights never burns it.
    fn cart_lava_tick(&mut self, id: u32, slot: usize) {
        for _ in 0..2 {
            let Some(i) = cart_at(&self.carts, id, slot) else {
                return;
            };
            if !self.in_lava(self.carts[i].bounds()) {
                return;
            }
            let p = self.carts[i].position;
            self.hurt_cart(i, 4.0, false, true);
            self.lava_hurt_sound([p.x, p.y, p.z]);
        }
    }
    /// `ClientPacketListener.postAddEntitySoundInstance` for the carts that
    /// joined the level since the last tick, and `LocalPlayer.startRiding`'s
    /// two inside sounds when the player got in one; then what the sound
    /// instances read this tick (the ridden cart and every cart).
    pub(super) fn start_cart_sounds(&mut self) -> (Option<u32>, Vec<CartSound>) {
        let carts: Vec<CartSound> = self
            .carts
            .iter()
            .map(|c| CartSound {
                id: c.id,
                position: [c.position.x, c.position.y, c.position.z],
                // Vec3.horizontalDistance.
                speed: (c.velocity.x * c.velocity.x + c.velocity.z * c.velocity.z).sqrt(),
            })
            .collect();
        self.cart_sounds
            .retain(|id| carts.iter().any(|c| c.id == *id));
        for cart in &carts {
            if !self.cart_sounds.contains(&cart.id) {
                let instance = SoundInstance::minecart(cart, random_source::create());
                let (_, res) = self.audio.play_instance(instance, false);
                if res != PlayResult::NotStarted {
                    self.cart_sounds.insert(cart.id);
                }
            }
        }
        let vehicle = self.riding.filter(|id| carts.iter().any(|c| c.id == *id));
        if vehicle != self.riding_cart_sounds
            && let Some(cart) = carts.iter().find(|c| Some(c.id) == vehicle)
        {
            let mut started = true;
            for underwater in [true, false] {
                let instance =
                    SoundInstance::riding_minecart(cart, underwater, random_source::create());
                let (_, res) = self.audio.play_instance(instance, false);
                if res == PlayResult::NotStarted {
                    started = false;
                }
            }
            if started {
                self.riding_cart_sounds = vehicle;
            }
        } else if vehicle.is_none() {
            self.riding_cart_sounds = None;
        }
        (vehicle, carts)
    }
    /// The rider's tick: the cart moves with the rider's input as its push.
    pub(super) fn tick_cart_riding(&mut self, keys: Keys) -> bool {
        let Some(i) = self.carts.iter().position(|c| Some(c.id) == self.riding) else {
            return false;
        };
        if keys.shift {
            self.dismount_cart(i);
            return true;
        }
        // ServerPlayer.getLastClientMoveIntent: getInputVector((left, 0, forward), 1, yRot).
        let left = f64::from(i8::from(keys.left) - i8::from(keys.right));
        let forward = f64::from(i8::from(keys.forward) - i8::from(keys.backward));
        let len = left.hypot(forward);
        let (left, forward) = if len > 1.0 {
            (left / len, forward / len)
        } else {
            (left, forward)
        };
        let r = self.yaw.to_radians();
        let (s, co) = (f64::from(r.sin()), f64::from(r.cos()));
        let intent = [left * co - forward * s, forward * co + left * s];
        let mut c = self.carts[i].clone();
        let alive = self.tick_cart(&mut c, intent, i, &mut CartIndex::default());
        if let Some(p) = self.player.as_mut() {
            self.previous_position = p.position;
            p.position = Vec3::new(c.position.x, c.position.y + SEAT, c.position.z);
            p.velocity = Vec3::ZERO;
            p.fall_distance = 0.0;
        }
        let Some(i) = self.carts.iter().position(|x| x.id == c.id) else {
            self.riding = None;
            return true;
        };
        if alive {
            self.carts[i] = c;
        } else {
            self.eject_cart_mobs(i);
            self.release_vehicle_container(c.id);
            self.carts.remove(i);
            self.riding = None;
        }
        // AbstractMinecart.activateMinecart: a powered activator rail ejects
        // the rider, and getDismountLocationForPassenger places it beside the
        // cart instead of leaving it at the seat.
        if alive && self.riding.is_none() {
            self.dismount_cart(i);
        }
        true
    }
    /// `getDismountLocationForPassenger`: beside the cart (across its facing), else on top.
    pub(super) fn dismount_cart(&mut self, i: usize) {
        let id = self.carts[i].id;
        if self.riding == Some(id) && !self.native_may_leave(id) {
            return;
        }
        self.riding = None;
        if self.dismount_in_portal(self.carts[i].position) {
            return;
        }
        let c = &self.carts[i];
        let (bx, by, bz) = (
            c.position.x.floor() as i32,
            c.position.y.floor() as i32,
            c.position.z.floor() as i32,
        );
        let mut target = None;
        // DismountHelper.offsetsForDirection of the cart's facing
        // (Direction.fromYRot of its yaw): right, left, back right, back left,
        // forward right, forward left, back, forward.
        let step = |q: i32| [(0, 1), (-1, 0), (0, -1), (1, 0)][(q & 3) as usize];
        let facing = (f64::from(c.yaw) / 90.0 + 0.5).floor() as i32;
        let (fx, fz) = step(facing);
        let (rx, rz) = (-fz, fx);
        let offsets = [
            (rx, rz),
            (-rx, -rz),
            (rx - fx, rz - fz),
            (-rx - fx, -rz - fz),
            (fx + rx, fz + rz),
            (fx - rx, fz - rz),
            (-fx, -fz),
            (fx, fz),
        ];
        'search: for dy in [0, 1, -1] {
            for (dx, dz) in offsets {
                let (x, y, z) = (bx + dx, by + dy, bz + dz);
                let below = self.res.facts().get(self.world.block(x, y - 1, z));
                let here = self.res.facts().get(self.world.block(x, y, z));
                let head = self.res.facts().get(self.world.block(x, y + 1, z));
                if (below.full_collision || below.legacy_solid)
                    && !here.full_collision
                    && !head.full_collision
                    && here.fluid == Fluid::None
                {
                    target = Some(Vec3::new(
                        f64::from(x) + 0.5,
                        f64::from(y),
                        f64::from(z) + 0.5,
                    ));
                    break 'search;
                }
            }
        }
        let top = c.bounds().max_position().y;
        let target = target.unwrap_or(Vec3::new(c.position.x, top, c.position.z));
        if let Some(p) = self.player.as_mut() {
            p.position = target;
            p.velocity = Vec3::ZERO;
        }
    }
    /// `MinecartItem.useOn`: a cart on the clicked rail.
    pub(super) fn place_minecart(&mut self, hit: &Hit, stack: &ItemStack) -> bool {
        let Some(kind) = CartKind::from_id(&stack.item) else {
            return false;
        };
        let Some(rail) = self.rail(hit.pos[0], hit.pos[1], hit.pos[2]) else {
            return false;
        };
        let offset = if rail.shape.is_slope() { 0.5 } else { 0.0 };
        let at = Vec3::new(
            f64::from(hit.pos[0]) + 0.5,
            f64::from(hit.pos[1]) + 0.0625 + offset,
            f64::from(hit.pos[2]) + 0.5,
        );
        self.next_entity += 1;
        let cart = Minecart::new(self.next_entity, kind, at);
        self.world
            .modified
            .insert((hit.pos[0] >> 4, hit.pos[2] >> 4));
        self.carts.push(cart);
        if self.serving.is_some() || self.options.dedicated.is_some() {
            let cart = self.carts.last().unwrap();
            self.native_vehicle_create(
                cart.kind.id(),
                [cart.position.x, cart.position.y, cart.position.z],
            );
        }
        // MinecartItem.useOn: ENTITY_PLACE at the rail, by the player, with the block under it.
        let below = self.world.block(hit.pos[0], hit.pos[1] - 1, hit.pos[2]);
        self.post_block_game_event(
            "minecraft:entity_place",
            hit.pos,
            self.sculk_actor(),
            Some(below),
        );
        if !self.creative {
            self.consume_held();
        }
        // ItemStack.useOn counts the use.
        self.award_stat("used", &stack.item, 1);
        self.swing_arm();
        true
    }
    pub(super) fn cart_hit(&self) -> Option<(usize, f64)> {
        let (eye, dir) = self.aim()?;
        let reach: f64 = self.entity_reach;
        let mut best: Option<(usize, f64)> = None;
        for (i, c) in self.carts.iter().enumerate() {
            if Some(c.id) == self.riding {
                continue;
            }
            let bb = c.bounds();
            let (lo, hi) = (bb.min_position(), bb.max_position());
            let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
            let (mut t0, mut t1) = (0.0f64, reach);
            let mut hit = true;
            for a in 0..3 {
                if dir[a].abs() < 1e-9 {
                    hit &= eye[a] >= lo[a] && eye[a] <= hi[a];
                    continue;
                }
                let (mut ta, mut tb) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
            }
            if hit && t0 <= t1 && best.is_none_or(|b| t0 < b.1) {
                best = Some((i, t0));
            }
        }
        best
    }
    /// The slots of the cart `id`, for a command: `getChestVehicleItem` and
    /// `setChestVehicleItem` first unpack its loot table with no player, so
    /// no `player_generates_container_loot` fires later. While its menu is
    /// open the live slots are the placeholder copy (`open_cart_container`).
    pub(super) fn cart_slots(&mut self, id: u32) -> Option<&mut Vec<Option<ItemStack>>> {
        let c = self.carts.iter_mut().find(|c| c.id == id)?;
        c.loot = None;
        match self
            .world
            .block_entities
            .get_mut(&(i32::MAX - 15, CART_KEY_Y, id as i32))
        {
            Some(BlockEntity::Chest(open)) => Some(open),
            _ => Some(&mut c.items),
        }
    }
    /// `ContainerEntity.unpackChestVehicleLootTable` for a player who opens a
    /// chest minecart: a roll it still holds fires `player_generates_container_loot`
    /// for its table, and the cart keeps its items from then on.
    fn unpack_cart_loot(&mut self, i: usize) {
        let Some(loot) = self.carts[i].loot.take() else {
            return;
        };
        if loot.items == horizon_client::save::encode_items(&self.carts[i].items) {
            self.trigger_loot_table(&loot.table);
        }
    }
    /// `interact`: ride, open, fuel or ignite the cart under the crosshair, or
    /// the one a guest's interact packet named.
    pub(super) fn interact_cart(&mut self, target: Option<InteractTarget>) -> bool {
        let Some((i, d)) = (match target {
            Some(t) => Self::named_entity(&self.carts, t.id, |c| c.id).map(|i| (i, 0.0)),
            None => self.cart_hit(),
        }) else {
            return false;
        };
        // Only a client's own pick is stopped by a block: the range check passed.
        if target.is_none() && !self.before_block(d) {
            return false;
        }
        let sneaking = self.keys.held(Action::Sneak);
        let held = self.held().cloned();
        match self.carts[i].kind {
            CartKind::Rideable => {
                let id = self.carts[i].id;
                // isVehicle: a mob already sits in it.
                let has_mob = self.mobs.iter().any(|m| m.extra.mount == Some(id));
                if sneaking || self.riding.is_some() || self.ridden_by_guest(id) || has_mob {
                    return false;
                }
                if !self.native_may_board(id) {
                    return true;
                }
                self.start_riding(id);
            }
            // AbstractMinecartContainer.interact: no sneak check.
            CartKind::Chest | CartKind::Hopper => self.open_cart_container(i),
            CartKind::Furnace => {
                let fuel = held
                    .as_ref()
                    .is_some_and(|s| matches!(&*s.item, "minecraft:coal" | "minecraft:charcoal"));
                let c = &mut self.carts[i];
                if fuel && c.fuel + 3600 <= 32000 {
                    c.fuel += 3600;
                    let p = self.player.as_ref().map_or(c.position, |p| p.position);
                    c.push = Vec3::new(c.position.x - p.x, 0.0, c.position.z - p.z);
                    if !self.creative {
                        self.consume_held();
                    }
                }
            }
            CartKind::Tnt => {
                let lighter = held.as_ref().is_some_and(|s| {
                    matches!(
                        &*s.item,
                        "minecraft:flint_and_steel" | "minecraft:fire_charge"
                    )
                });
                if !lighter || self.carts[i].fuse >= 0 {
                    return false;
                }
                let mut c = self.carts[i].clone();
                self.prime_cart(&mut c);
                self.carts[i] = c;
            }
            // MinecartCommandBlock.interact: a game master gets SUCCESS (the
            // client opens the editor, `open_cart_command_block`), anyone else
            // PASSes to the held item. SUCCESS swings on the client only.
            CartKind::CommandBlock => return self.can_use_game_master_blocks(),
            // MinecartSpawner has no interact of its own (`Entity.interact` passes).
            CartKind::Spawner => return false,
        }
        self.swing_arm();
        true
    }
    /// `AbstractMinecartContainer.interact`: the menu opens over the cart's
    /// own slots, kept under a placeholder key until it closes
    /// (`store_cart_items`/`close_cart_container`).
    pub(super) fn open_cart_container(&mut self, i: usize) {
        self.unpack_cart_loot(i);
        let c = self.carts[i].clone();
        let key = (i32::MAX - 15, CART_KEY_Y, c.id as i32);
        self.world
            .block_entities
            .insert(key, BlockEntity::Chest(c.items.clone()));
        self.container = Some(key);
        self.screen = super::Screen::Inventory;
        self.capture(false);
        self.right = false;
        if c.kind == CartKind::Chest {
            // `MinecartChest.interact` (which overrides
            // `AbstractMinecartContainer.interact`): a successful open fires
            // CONTAINER_OPEN with the player and angers the piglins that see
            // it. `MinecartHopper` does not override it, so a hopper minecart
            // fires neither. Vanilla plays no open sound for a chest minecart.
            let at = [c.position.x, c.position.y, c.position.z];
            let ctx = self.player_context(None);
            self.post_game_event("minecraft:container_open", at, ctx);
            self.anger_piglins(true);
        }
    }
    /// Closing the cart's container open under `key` copies the items back.
    pub(super) fn close_cart_container(&mut self, key: (i32, i32, i32)) {
        if key.1 != CART_KEY_Y {
            return;
        }
        self.store_cart_items(key);
        let guest_viewers = self
            .lan
            .as_ref()
            .is_some_and(|host| Self::vehicle_has_other_viewers(None, &host.hub.guests, None, key));
        if !guest_viewers {
            self.world.block_entities.remove(&key);
        }
    }
    /// A chest or hopper minecart or a chest boat is about to be removed
    /// (`Containers.dropContents` / `AbstractChestBoat.remove`): what an open
    /// menu edited goes back to the vehicle, so it is what drops or travels
    /// with it, and the menu's stand-in leaves the level. A menu that is
    /// still open over it then has nothing left to take.
    pub(super) fn release_vehicle_container(&mut self, id: u32) {
        let key = (i32::MAX - 15, CART_KEY_Y, id as i32);
        self.store_cart_items(key);
        self.world.block_entities.remove(&key);
        if self.container == Some(key) {
            self.container = None;
        }
    }
    /// The menu of a chest or hopper minecart or a chest boat works on the
    /// vehicle's own items (`ContainerEntity`): the placeholder under `key`
    /// that the menu edits is copied back to the vehicle.
    pub(super) fn store_cart_items(&mut self, key: (i32, i32, i32)) {
        if key.1 != CART_KEY_Y {
            return;
        }
        let Some(BlockEntity::Chest(items)) = self.world.block_entities.get(&key) else {
            return;
        };
        let items = items.clone();
        if let Some(c) = self.carts.iter_mut().find(|c| c.id as i32 == key.2) {
            c.items = items;
        } else if let Some(b) = self.boats.iter_mut().find(|b| b.id as i32 == key.2) {
            b.items = items;
        }
    }
    /// The open cart's kind when a cart container is shown.
    pub(super) fn open_cart_kind(&self) -> Option<CartKind> {
        let key = self.container.filter(|k| k.1 == CART_KEY_Y)?;
        // A chest boat's chest shows like a chest minecart's.
        self.carts
            .iter()
            .find(|c| c.id as i32 == key.2)
            .map(|c| c.kind)
            .or_else(|| {
                self.boats
                    .iter()
                    .any(|b| b.chest && b.id as i32 == key.2)
                    .then_some(CartKind::Chest)
            })
    }
    /// `VehicleEntity.hurtServer` from the player's attack.
    pub(super) fn attack_cart(&mut self) -> bool {
        let Some((i, d)) = self.cart_hit() else {
            return false;
        };
        if !self.before_block(d) || self.mob_hit().is_some_and(|m| m.1 < d) {
            return false;
        }
        let burning = self
            .held()
            .is_some_and(|s| s.enchantment_level("minecraft:fire_aspect") > 0);
        let area = self.carts[i].bounds();
        // Player.attack's full damage against the cart's entity type.
        let id = self.carts[i].id as i32;
        let (damage, _, _, magic) = self.melee_damage(self.carts[i].kind.id(), false);
        self.hurt_cart(i, damage, self.creative, burning);
        self.vehicle_attack_sounds(area, magic.then_some(id));
        // Player.onAttack: the hit starts the attack strength again.
        self.attack_ticks = 0;
        self.swing_arm();
        true
    }
    /// An arrow hit cart `i` (`MinecartTNT.hurtServer`, then
    /// `VehicleEntity.hurtServer`): a burning arrow sets off a TNT minecart.
    pub(super) fn arrow_hits_cart(
        &mut self,
        i: usize,
        damage: f32,
        arrow_velocity: Vec3,
        burning: bool,
        creative: bool,
    ) {
        if i >= self.carts.len() {
            return;
        }
        if burning && self.carts[i].kind == CartKind::Tnt {
            self.eject_cart_mobs(i);
            let id = self.carts[i].id;
            self.release_vehicle_container(id);
            let mut c = self.carts.remove(i);
            self.explode_cart(&mut c, arrow_velocity.length_squared());
            return;
        }
        self.hurt_cart(i, damage, creative, false);
    }
    /// `Containers.dropContents(level, entity, container)` at a vehicle's
    /// position `at`: each slot (an empty one too) from a spot in its block,
    /// in pieces of 10 to 30 with the level random's scatter, no pickup
    /// delay. `AbstractChestBoat.remove` and `AbstractMinecartContainer.remove`
    /// spill this way for a destroying removal (kill or discard), and
    /// `chestVehicleDestroyed` again for the emptied container.
    pub(super) fn drop_vehicle_contents(&mut self, at: [f64; 3], items: &[Option<ItemStack>]) {
        for stack in items {
            let count = stack.as_ref().map_or(0, |s| s.count);
            let pieces = horizon_world::level::removal::drop_item_stack_at(
                &mut self.world.random,
                at,
                count,
            );
            if let Some(stack) = stack {
                for piece in pieces {
                    let piece_stack = ItemStack {
                        count: piece.count,
                        ..stack.clone()
                    };
                    self.spawn_item(piece_stack, piece.pos, piece.motion, 0);
                }
            }
        }
    }
    /// `VehicleEntity.hurtServer`: shake, add the damage, break past 40 (at
    /// once for creative players); TNT minecarts prime instead when moving
    /// or hurt by fire or an explosion (`damageSourceIgnitesTnt`).
    pub(super) fn hurt_cart(&mut self, i: usize, damage: f32, creative: bool, fire: bool) {
        self.hurt_cart_result(i, damage, creative, fire);
    }

    /// Accepted damage is distinct from survival, destruction, or TNT priming.
    pub(super) fn hurt_cart_result(
        &mut self,
        i: usize,
        mut damage: f32,
        creative: bool,
        fire: bool,
    ) -> bool {
        let kind = self.carts[i].kind.id();
        let mut cancel = false;
        self.native_vehicle_damage(kind, &mut damage, &mut cancel);
        if cancel {
            return false;
        }
        let destroyed = {
            let c = &mut self.carts[i];
            c.hurt_dir = -c.hurt_dir;
            c.hurt_time = 10;
            c.damage += damage * 10.0;
            // MinecartTNT.shouldSourceDestroy: fire and explosions destroy
            // (so ignite) a TNT cart whatever damage it has taken.
            cart_is_destroyed(creative, c.damage, fire && c.kind == CartKind::Tnt)
        };
        if destroyed {
            let speed = self.carts[i].velocity.x * self.carts[i].velocity.x
                + self.carts[i].velocity.z * self.carts[i].velocity.z;
            let tnt_prime =
                self.carts[i].kind == CartKind::Tnt && (fire || speed >= f64::from(0.01f32));
            if !tnt_prime {
                self.eject_cart_mobs(i);
            }
            let id = self.carts[i].id;
            if !tnt_prime {
                self.release_vehicle_container(id);
            }
            if !tnt_prime {
                let at = [
                    self.carts[i].position.x,
                    self.carts[i].position.y,
                    self.carts[i].position.z,
                ];
                self.native_vehicle_destroy(self.carts[i].kind.id(), at);
            }
            let mut c = self.carts.remove(i);
            if c.kind == CartKind::Tnt && (fire || speed >= f64::from(0.01f32)) {
                // MinecartTNT.destroy: prime instead of breaking.
                if c.fuse < 0 {
                    self.prime_cart(&mut c);
                    c.fuse = c.random.next_int_bound(20).unwrap_or(0)
                        + c.random.next_int_bound(20).unwrap_or(0);
                }
                self.carts.insert(i, c);
            } else {
                let at = [c.position.x, c.position.y, c.position.z];
                // AbstractMinecartContainer.remove: `Containers.dropContents`
                // for every destroying removal (the kill below, and a creative
                // hit's discard), with no `entity_drops` gate of its own.
                self.drop_vehicle_contents(at, &c.items);
                // VehicleEntity.destroy: `kill` (the cart's own ENTITY_DIE
                // after that removal), then the drops; a creative hit only
                // discards it.
                if !creative {
                    self.entity_killed_event(at, false);
                }
                if !creative && self.ui.rules.bool("entity_drops") {
                    // `spawnAtLocation`: an ItemEntity's own motion.
                    self.spawn_item_own(ItemStack::new(c.kind.item(), 1), at, 10);
                    // `chestVehicleDestroyed` drops the emptied container
                    // again, which only draws each slot's spot.
                    let empty = vec![None; c.items.len()];
                    self.drop_vehicle_contents(at, &empty);
                }
            }
        }
        true
    }
    /// Baked block-state faces with default block tints and terrain shade or diffuse lighting.
    pub(super) fn block_model_vertices(
        &self,
        state: BlockStateId,
        seed: i64,
        m: glam::Mat4,
        light: u32,
        diffuse: bool,
        out: &mut Vec<Vertex>,
    ) {
        // BlockTintSource.color(state) with no level: each tinted face's default colour.
        let grass = self.pack.item_renderer.grass_color(0.5, 1.0) as u32;
        let tables = &self.pack.context.tables;
        let tint = |index: i32| tables.default_tint(state, index, grass);
        self.tinted_block_model_vertices(state, seed, m, |_| light, diffuse, tint, out);
    }
    /// `block_model_vertices` with each face multiplied by `tint` of its
    /// tint index (RGB, `None` for untinted faces) and lit by `light` of
    /// each quad.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn tinted_block_model_vertices(
        &self,
        state: BlockStateId,
        seed: i64,
        m: glam::Mat4,
        light: impl Fn(&horizon_assets::bake::ModelQuad) -> u32,
        diffuse: bool,
        tint: impl Fn(i32) -> Option<u32>,
        out: &mut Vec<Vertex>,
    ) {
        let Ok(models) = self.pack.render.model_catalog.collect_seeded(state, seed) else {
            return;
        };
        let models = models.into_iter().map(|(_, model)| model);
        self.baked_model_vertices(models, m, light, diffuse, tint, out);
    }
    /// `tinted_block_model_vertices` for baked models given as they are (a
    /// state's, or a standalone model).
    pub(super) fn baked_model_vertices<'m>(
        &self,
        models: impl IntoIterator<Item = &'m horizon_client::models::catalog::BakedModel>,
        m: glam::Mat4,
        light: impl Fn(&horizon_assets::bake::ModelQuad) -> u32,
        diffuse: bool,
        tint: impl Fn(i32) -> Option<u32>,
        out: &mut Vec<Vertex>,
    ) {
        let mut normals = horizon_client::item_render::FaceNormals::new(m);
        for model in models {
            for face in &model.faces {
                let Some(s) = self
                    .pack
                    .layer_names
                    .get(&face.layer)
                    .and_then(|n| self.gui.atlas.sprites.get(n))
                else {
                    continue;
                };
                use horizon_assets::model::Direction as D;
                let shade: u32 = match face.quad.facing {
                    D::Up => 255,
                    D::Down => 127,
                    D::North | D::South => 204,
                    D::West | D::East => 153,
                };
                let mut color = if diffuse {
                    u32::MAX
                } else {
                    0xFF00_0000 | shade << 16 | shade << 8 | shade
                };
                if let Some(t) = tint(face.quad.tint_index) {
                    color = horizon_client::hud_layout::multiply(
                        color,
                        horizon_client::mobs::render::abgr(t),
                    );
                }
                let normal = if diffuse {
                    let n = normals.face(face.quad.facing);
                    let n = n.to_array().map(|v| (v.clamp(-1.0, 1.0) * 127.0) as i8);
                    [n[0], n[1], n[2], 127]
                } else {
                    [0; 4]
                };
                let light = light(&face.quad);
                let light = if diffuse {
                    let emission = u32::from(face.quad.light_emission) * 16;
                    (light & 0xFF) | (light >> 8 & 0xFF).max(emission) << 8
                } else {
                    light
                };
                for k in 0..4 {
                    let p = face.quad.positions[k];
                    let uv = face.quad.uv[k];
                    out.push(Vertex {
                        normal,
                        pos: m.transform_point3(glam::Vec3::from_array(p)).to_array(),
                        uv: [s.u0 + uv[0] * (s.u1 - s.u0), s.v0 + uv[1] * (s.v1 - s.v0)],
                        layer: 0,
                        color,
                        light,
                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
    }
    /// The block the cart shows: `None` for air (`AbstractMinecartRenderer`
    /// draws and stretches its culling box only for a non-air state).
    fn display_state(&self, c: &Minecart) -> Option<BlockStateId> {
        shown_unless_air(self.display_state_unfiltered(c), |s| {
            self.res.facts().is_air(s)
        })
    }
    fn display_state_unfiltered(&self, c: &Minecart) -> Option<BlockStateId> {
        let registry = &self.pack.assets.registry;
        let with = |name: &str, props: &[(&str, &str)]| {
            let mut s = registry.state(registry.default_state(name)?)?.clone();
            for (k, v) in props {
                s.properties.insert((*k).to_owned(), (*v).to_owned());
            }
            registry.id(&s)
        };
        // AbstractMinecart.getDisplayBlockState: a custom state wins.
        if let Some(state) = c
            .custom_display
            .as_ref()
            .and_then(|tag| horizon_client::entity_nbt::state_from_nbt(registry, tag))
        {
            return Some(state);
        }
        match c.kind {
            CartKind::Rideable => None,
            CartKind::Chest => with("minecraft:chest", &[("facing", "north")]),
            CartKind::Hopper => registry.default_state("minecraft:hopper"),
            CartKind::Furnace => with(
                "minecraft:furnace",
                &[
                    ("facing", "north"),
                    ("lit", if c.fuel > 0 { "true" } else { "false" }),
                ],
            ),
            CartKind::Tnt => registry.default_state("minecraft:tnt"),
            CartKind::CommandBlock => registry.default_state("minecraft:command_block"),
            // MinecartSpawner.getDefaultDisplayBlockState.
            CartKind::Spawner => registry.default_state("minecraft:spawner"),
        }
    }
    /// `AbstractMinecartRenderer` (old render): on the rail between the
    /// front and back points, pitched on slopes, with the display block.
    pub(super) fn cart_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        let a = alpha as f32;
        let Some(uv) = self
            .gui
            .atlas
            .sprites
            .get("entity/minecart/minecart")
            .map(|r| [r.u0, r.v0, r.u1, r.v1])
        else {
            return;
        };
        // Entity.shouldRenderAtSqrDistance: 64 times the bounding box's mean
        // edge, scaled by the view scale.
        let range = (WIDTH + HEIGHT + WIDTH) / 3.0 * 64.0 * self.view_scale();
        for c in &self.carts {
            let Some(model) = self.cart_models.models.get(&c.kind) else {
                continue;
            };
            let lerp = self.remote_minecart_lerp(c.id, a);
            let new_render = self.minecart_improvements() || lerp.is_some();
            let (x, y, z) = if let Some((p, _, _)) = lerp {
                (p[0], p[1], p[2])
            } else {
                (
                    c.previous.x + (c.position.x - c.previous.x) * alpha,
                    c.previous.y + (c.position.y - c.previous.y) * alpha,
                    c.previous.z + (c.position.z - c.previous.z) * alpha,
                )
            };
            // LevelExtractor.isEntityVisible: the cart the player rides always draws.
            let mut shown = None;
            if self.riding != Some(c.id) {
                let d = glam::DVec3::new(c.position.x, c.position.y, c.position.z) - camera;
                if d.length_squared() >= range * range {
                    continue;
                }
                // Looked up once, past the distance cut: the cull and the draw both use it.
                shown = Some(self.display_state(c));
                // EntityRenderer.shouldRender: the interpolated box grown by 0.5;
                // AbstractMinecartRenderer.getBoundingBoxForCulling stretches it
                // up by the display block's offset.
                if let Some(view) = &self.view_frustum {
                    let top = HEIGHT
                        + if shown.flatten().is_none() {
                            0.0
                        } else {
                            display_offset(c) * 0.75 / 16.0
                        };
                    let p = glam::DVec3::new(x, y, z) - camera;
                    let lo = p - glam::DVec3::new(WIDTH / 2.0 + 0.5, 0.5, WIDTH / 2.0 + 0.5);
                    let hi = p + glam::DVec3::new(WIDTH / 2.0 + 0.5, top + 0.5, WIDTH / 2.0 + 0.5);
                    if !view.intersects(lo.as_vec3(), hi.as_vec3()) {
                        continue;
                    }
                }
            }
            let mut yaw =
                c.previous_yaw + ((c.yaw - c.previous_yaw + 180.0).rem_euclid(360.0) - 180.0) * a;
            let mut pitch = 0.0f32;
            let mut offset = glam::DVec3::ZERO;
            if new_render {
                (yaw, pitch) = lerp.map_or((c.yaw, c.pitch), |(_, yaw, pitch)| (yaw, pitch));
            } else if let Some(on) = self.rail_pos(x, y, z) {
                // getPosOffs(..., 0.3F): the float widened.
                let half = f64::from(0.3_f32);
                let front = self.rail_pos_offs(x, y, z, half).unwrap_or(on);
                let back = self.rail_pos_offs(x, y, z, -half).unwrap_or(on);
                offset = glam::DVec3::new(on[0] - x, (front[1] + back[1]) / 2.0 - y, on[2] - z);
                let d =
                    glam::DVec3::new(back[0] - front[0], back[1] - front[1], back[2] - front[2]);
                if d.length() != 0.0 {
                    let d = d.normalize();
                    yaw = d.z.atan2(d.x).to_degrees() as f32;
                    pitch = (d.y.atan() * 73.0) as f32;
                }
            }
            let render_position = Vec3::new(x + offset.x, y + offset.y, z + offset.z);
            let has_display = shown.as_ref().is_some_and(Option::is_some)
                || (shown.is_none() && self.display_state(c).is_some());
            let culling = self.entity_cull_decision_in_bounds(
                c,
                cart_render_bounds(render_position, display_offset(c), has_display),
            );
            if culling.hides_entity() {
                continue;
            }
            let entity_start = out.len();
            self.mark_motion(entity_start, Self::MOTION_OBJECT | u64::from(c.id));
            // The per-entity jitter (offsetSeed) that keeps stacked carts from z-fighting.
            let seed = i64::from(c.id).wrapping_mul(493_286_711);
            let seed = seed
                .wrapping_mul(seed)
                .wrapping_mul(4_392_167_121)
                .wrapping_add(seed.wrapping_mul(98_761));
            let jitter = |shift: i64| (((seed >> shift & 7) as f32 + 0.5) / 8.0 - 0.5) * 0.004;
            let p = glam::DVec3::new(x, y, z) + offset - camera;
            let jittered = p.as_vec3() + glam::Vec3::new(jitter(16), jitter(20), jitter(24));
            let mut base = if new_render {
                // AbstractMinecartRenderer.newRender translates the model
                // after rotating, so the seat follows the pitched cart.
                glam::Mat4::from_translation(jittered)
                    * glam::Mat4::from_rotation_y(yaw.to_radians())
                    * glam::Mat4::from_rotation_z((-pitch).to_radians())
                    * glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.375, 0.0))
            } else {
                glam::Mat4::from_translation(jittered + glam::Vec3::new(0.0, 0.375, 0.0))
                    * glam::Mat4::from_rotation_y((180.0 - yaw).to_radians())
                    * glam::Mat4::from_rotation_z((-pitch).to_radians())
            };
            let hurt = c.hurt_time as f32 - a;
            if hurt > 0.0 {
                let damage = (c.damage - a).max(0.0);
                base *= glam::Mat4::from_rotation_x(
                    (hurt.sin() * hurt * damage / 10.0 * c.hurt_dir as f32).to_radians(),
                );
            }
            let l = self.world.light(
                c.position.x.floor() as i32,
                (c.position.y + 0.35).floor() as i32,
                c.position.z.floor() as i32,
            );
            let packed = (u32::from(l >> 4) * 16) | ((u32::from(l & 15) * 16) << 8);
            if let Some(state) = shown.unwrap_or_else(|| self.display_state(c)) {
                let display = base
                    * glam::Mat4::from_scale(glam::Vec3::splat(0.75))
                    * glam::Mat4::from_translation(glam::Vec3::new(
                        -0.5,
                        (display_offset(c) as f32 - 8.0) / 16.0,
                        0.5,
                    ))
                    * glam::Mat4::from_rotation_y(90f32.to_radians());
                if c.kind == CartKind::Chest {
                    self.special_block_model(state, display, l, out);
                } else {
                    // TNT minecarts flash white as the fuse runs out (TntMinecartRenderer).
                    let swell = if c.kind == CartKind::Tnt && c.fuse > -1 {
                        let fuse = c.fuse as f32 - a + 1.0;
                        if fuse < 10.0 {
                            let g = (1.0 - fuse / 10.0).clamp(0.0, 1.0);
                            1.0 + g * g * g * g * 0.3
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    };
                    let m = display
                        * glam::Mat4::from_translation(glam::Vec3::splat(0.5))
                        * glam::Mat4::from_scale(glam::Vec3::splat(swell))
                        * glam::Mat4::from_translation(glam::Vec3::splat(-0.5));
                    let flash = c.kind == CartKind::Tnt && c.fuse > -1 && (c.fuse / 5) % 2 == 0;
                    let start = out.len();
                    self.block_model_vertices(state, 0, m, packed, false, out);
                    if flash {
                        // submitWhiteSolidBlock: OverlayTexture.u(1.0) on the cart's own light.
                        for v in &mut out[start..] {
                            v.layer = 5;
                        }
                    }
                }
            }
            let m = base * glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0));
            let poses = Poses::new(model);
            horizon_client::mobs::render::draw(model, m, &poses, uv, 0xFFFF_FFFF, packed, 0, out);
            self.finish_entity_occlusion(culling, c, out, entity_start);
        }
    }
    /// The model of a block drawn by its block-entity renderer, as a chest
    /// minecart shows its chest and a block display its special block.
    pub(super) fn special_block_model(
        &self,
        state: BlockStateId,
        m: glam::Mat4,
        light: u8,
        out: &mut Vec<Vertex>,
    ) {
        let lid = |_: [i32; 3]| 0.0;
        let scene = horizon_client::block_entity_render::Scene {
            camera: glam::DVec3::ZERO,
            orientation: glam::Quat::IDENTITY,
            anim: &|_| horizon_client::block_entity_render::BlockAnim::default(),
            texts: None,
            partial: 0.0,
            game_time: self.world.game_time,
            max_y: self.world.shape.max_y() - 1,
            world: &self.world,
            sprites: &self.gui.atlas.sprites,
            emissive: &self.gui.atlas.emissive,
            translucent_sprites: &self.gui.atlas.translucent,
            atlas_revision: self.gui.atlas.revision,
            nether: self.dim == super::Dim::Nether,
            lid: &lid,
            sign: &|_| None,
            banner: &|_| Vec::new(),
            skull: &|_| None,
            glyph: &|c| self.gui.atlas.glyph_cell(c),
            glyphs: self.gui.atlas.font.epoch(),
            font: Some(&self.gui.atlas.font),
            dynamic_visible: None,
        };
        self.res
            .block_entity_render()
            .render_state(&scene, state.0, m, light, out);
    }
}
/// Saving and loading.
impl Minecart {
    pub fn to_nbt(
        &self,
        uuid: [i32; 4],
        raw: Option<&horizon_world::nbt::Tag>,
    ) -> horizon_world::nbt::Tag {
        use horizon_world::nbt::{JavaString, Tag};
        let mut tag = horizon_client::entity_nbt::vehicle_to_nbt(
            self.kind.id(),
            self.position,
            self.velocity,
            [self.yaw, self.pitch],
            (self.on_ground, self.fall_distance),
            uuid,
            raw,
        );
        horizon_client::entity_nbt::write_saved_flags(&mut tag, self.on_ground, self.no_gravity);
        if let Tag::Compound(map) = &mut tag {
            // ContainerEntity.addChestVehicleSaveData belongs only to the
            // container subclasses, never to a plain or specialized cart.
            if self.kind.slots() == 0 {
                map.remove(&JavaString::from("Items"));
            }
            if self.kind == CartKind::CommandBlock {
                for key in ["LastOutput", "LastExecution"] {
                    map.remove(&JavaString::from(key));
                }
            }
            map.remove(&JavaString::from("DisplayOffset"));
            map.remove(&JavaString::from("DisplayState"));
            map.remove(&JavaString::from("explosion_power"));
            map.remove(&JavaString::from("explosion_speed_factor"));
            let mut put = |k: &str, v: Tag| {
                map.insert(JavaString::from(k), v);
            };
            put("FlippedRotation", Tag::Byte(i8::from(self.flipped)));
            // AbstractMinecart saves firstTick as HasTicked. A cart never burns:
            // Entity.move sets -getFireImmuneTicks(), 0 for all but players.
            put("HasTicked", Tag::Byte(i8::from(self.first_tick)));
            put("Fire", Tag::Short(0));
            // addAdditionalSaveData: the offset only off its default, the state only when set.
            if let Some(offset) = self.custom_offset
                && f64::from(offset) != default_display_offset(self.kind)
            {
                put("DisplayOffset", Tag::Int(offset));
            }
            if let Some(state) = &self.custom_display {
                put("DisplayState", state.clone());
            }
            match self.kind {
                CartKind::Chest | CartKind::Hopper => {
                    put("Items", horizon_client::save::encode_items(&self.items));
                    if self.kind == CartKind::Hopper {
                        put("Enabled", Tag::Byte(i8::from(self.enabled)));
                    }
                }
                CartKind::Furnace => {
                    put("PushX", Tag::Double(self.push.x.to_bits()));
                    put("PushZ", Tag::Double(self.push.z.to_bits()));
                    put("Fuel", Tag::Short(self.fuel as i16));
                }
                // MinecartTNT.addAdditionalSaveData: the powers only off their defaults.
                CartKind::Tnt => {
                    put("fuse", Tag::Int(self.fuse));
                    for (key, value, default) in [
                        (
                            "explosion_power",
                            self.explosion_power,
                            DEFAULT_EXPLOSION_POWER,
                        ),
                        (
                            "explosion_speed_factor",
                            self.explosion_speed_factor,
                            DEFAULT_EXPLOSION_SPEED_FACTOR,
                        ),
                    ] {
                        if value != default {
                            put(key, Tag::Float(value.to_bits()));
                        }
                    }
                }
                // BaseCommandBlock.save (its CustomName shares the entity's key).
                CartKind::CommandBlock => {
                    for (key, value) in self.command.save() {
                        if !matches!(key, "CustomName" | "powered" | "conditionMet" | "auto") {
                            put(key, value);
                        }
                    }
                }
                // BaseSpawner.save: the delays, ranges, SpawnData and SpawnPotentials.
                CartKind::Spawner => {
                    if let Some(spawner) = &self.spawner {
                        for (key, value) in
                            horizon_client::mobs::spawner::SpawnerExt::save(&**spawner)
                        {
                            put(key, value);
                        }
                    }
                }
                CartKind::Rideable => {}
            }
            // A loaded loot table goes back while the items are still its roll;
            // otherwise the items replace it, as vanilla's unpackLootTable does.
            if self.loot.as_ref().is_some_and(|l| l.restore(map)) {
                if let Some(table) = horizon_client::save::text(map.get(&"LootTable".into()))
                    .and_then(|id| horizon_client_core::identifier::normalize_id(&id))
                {
                    map.insert("LootTable".into(), Tag::String(table.as_str().into()));
                }
            } else {
                map.remove(&JavaString::from("LootTable"));
                map.remove(&JavaString::from("LootTableSeed"));
            }
        }
        tag
    }
    pub fn from_nbt(tag: &horizon_world::nbt::Tag, id: u32, kind: CartKind) -> Option<Self> {
        let (position, _, _) = horizon_client::entity_nbt::boat_from_nbt(tag)?;
        let mut c = Self::new(id, kind, position);
        c.load(tag)?;
        Some(c)
    }
    /// `Entity.load` on this cart: the saved fields are read over the live
    /// ones, the rest (riders, lerp, hurt state) stays; its kind never changes.
    /// `None`, with nothing changed, when the compound has no position.
    pub fn load(&mut self, tag: &horizon_world::nbt::Tag) -> Option<()> {
        use horizon_world::nbt::Tag;
        let (position, velocity, yaw) = horizon_client::entity_nbt::boat_from_nbt(tag)?;
        let kind = self.kind;
        let c = self;
        c.position = position;
        c.previous = position;
        c.velocity = velocity;
        c.yaw = yaw;
        c.previous_yaw = yaw;
        c.no_gravity = horizon_client::entity_nbt::flag(tag, "NoGravity");
        c.on_ground = horizon_client::entity_nbt::flag(tag, "OnGround");
        c.pitch = horizon_client::entity_nbt::rotation(tag)[1];
        c.previous_pitch = c.pitch;
        // ValueInput's numeric getters take any NumericTag: getIntOr is its
        // intValue, getShortOr the low 16 bits of it, getBooleanOr
        // `byteValue() != 0` (the low 8), getDoubleOr its doubleValue.
        let int = |k: &str| {
            tag.get(k)
                .and_then(horizon_client_core::nbt_path::int_value)
        };
        let flag = |k: &str, default: bool| int(k).map_or(default, |v| v as i8 != 0);
        let double = |k: &str| match tag.get(k) {
            Some(Tag::Byte(v)) => f64::from(*v),
            Some(Tag::Short(v)) => f64::from(*v),
            Some(Tag::Int(v)) => f64::from(*v),
            Some(Tag::Long(v)) => *v as f64,
            Some(Tag::Float(v)) => f64::from(f32::from_bits(*v)),
            Some(Tag::Double(v)) => f64::from_bits(*v),
            _ => 0.0,
        };
        // Entity.load: OnGround and fall_distance, as saved.
        (c.on_ground, c.fall_distance) = horizon_client::entity_nbt::ground_from_nbt(tag);
        c.flipped = flag("FlippedRotation", false);
        c.first_tick = flag("HasTicked", false);
        if kind.slots() > 0 {
            c.items = horizon_client::save::decode_items(tag.get("Items"), kind.slots());
        }
        c.enabled = flag("Enabled", true);
        c.fuel = int("Fuel").map_or(0, |v| i32::from(v as i16));
        // readAdditionalSaveData: the saved push has no y.
        c.push = Vec3::new(double("PushX"), 0.0, double("PushZ"));
        c.fuse = int("fuse").unwrap_or(-1);
        if kind == CartKind::Tnt {
            // MinecartTNT.readAdditionalSaveData: getFloatOr, kept to 0..128.
            let float = |k: &str, default: f32| match tag.get(k) {
                Some(Tag::Float(v)) => f32::from_bits(*v),
                Some(Tag::Double(v)) => f64::from_bits(*v) as f32,
                Some(Tag::Long(v)) => *v as f32,
                Some(other) => {
                    horizon_client_core::nbt_path::int_value(other).map_or(default, |v| v as f32)
                }
                None => default,
            };
            c.explosion_power = float("explosion_power", DEFAULT_EXPLOSION_POWER).clamp(0.0, 128.0);
            c.explosion_speed_factor =
                float("explosion_speed_factor", DEFAULT_EXPLOSION_SPEED_FACTOR).clamp(0.0, 128.0);
        }
        // readAdditionalSaveData: DisplayState, DisplayOffset (default offset when absent).
        c.custom_display = tag.get("DisplayState").cloned();
        c.custom_offset = int("DisplayOffset");
        if kind == CartKind::CommandBlock {
            c.command = horizon_client::command_block::CommandBlock::load(tag);
            // MinecartCommandBlock.readAdditionalSaveData sets the synched command.
            // A changed value invokes onSyncedDataUpdated -> setCommand, resetting successes.
            if c.synced_command != c.command.command {
                c.synced_command.clone_from(&c.command.command);
                c.command.set_command(&c.synced_command);
            }
            c.synced_output = c.command.last_output_string();
        }
        // MinecartSpawner.readAdditionalSaveData: BaseSpawner.load.
        if kind == CartKind::Spawner {
            c.spawner = Some(Box::new(horizon_client::mobs::spawner::SpawnerExt::load(
                tag,
            )));
        }
        Some(())
    }
}

/// `MinecartTNT.DEFAULT_EXPLOSION_POWER_BASE` and `DEFAULT_EXPLOSION_SPEED_FACTOR`.
const DEFAULT_EXPLOSION_POWER: f32 = 4.0;
const DEFAULT_EXPLOSION_SPEED_FACTOR: f32 = 1.0;

/// `VehicleEntity.hurtServer`'s destroy test: a creative player, damage past
/// 40, or (`MinecartTNT.shouldSourceDestroy`) fire or an explosion hitting a
/// TNT cart.
fn cart_is_destroyed(creative: bool, damage: f32, ignites_tnt: bool) -> bool {
    creative || damage > 40.0 || ignites_tnt
}

/// `MinecartTNT.tick`'s collision detonation: a cart still moving at a tenth
/// of a block a tick or more when a block stops it blows up.
fn tnt_ignites_by_collision(horizontal_collision: bool, speed_sqr: f64) -> bool {
    horizontal_collision && speed_sqr >= 0.009_999_999_776_482_582
}

/// NewMinecartBehavior's `(float) (Math.atan2(y, x) * 180.0 / Math.PI)` before
/// the cast: HotSpot's atan2, multiplied then divided in Java's order.
fn java_degrees(y: f64, x: f64) -> f64 {
    horizon_core::jmath::atan2(y, x) * 180.0 / std::f64::consts::PI
}

#[cfg(test)]
mod tests {
    /// The cart index hands a moved cart every cart a scan finds in its box,
    /// as the carts move one by one, after one is gone, and after a command
    /// (which forgets the index) put another cart in a slot.
    #[test]
    fn cart_index_finds_what_a_scan_finds() {
        use super::{CartIndex, CartKind, Minecart};
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = |m: f64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 1_000_000) as f64 / 1_000_000.0 * m
        };
        // Clustered, so most carts have neighbours.
        let mut carts: Vec<Minecart> = (0..300)
            .map(|id| Minecart::new(id, CartKind::Chest, Vec3::new(next(40.0), 64.0, next(40.0))))
            .collect();
        let mut index = CartIndex::default();
        let mut near = Vec::new();
        let check = |index: &mut CartIndex, carts: &[Minecart], i: usize, near: &mut Vec<u32>| {
            let area = carts[i].bounds().inflate(Vec3::new(0.2, 0.0, 0.2));
            let scan: Vec<u32> = (0..carts.len() as u32)
                .filter(|&j| area.intersects(carts[j as usize].bounds()))
                .collect();
            if index.carts_near(carts, i, area, near) {
                let found: Vec<u32> = near
                    .iter()
                    .copied()
                    .filter(|&j| area.intersects(carts[j as usize].bounds()))
                    .collect();
                assert_eq!(found, scan, "cart {i}");
            }
        };
        for round in 0..3 {
            for i in 0..carts.len() {
                // Each cart moves up to two blocks before its push.
                carts[i].position.x += next(4.0) - 2.0;
                carts[i].position.z += next(4.0) - 2.0;
                check(&mut index, &carts, i, &mut near);
                index.moved(i, carts[i].bounds());
                if i == 100 && round == 1 {
                    carts.remove(50);
                }
                if i == 200 && round == 2 {
                    let at = carts[0].position;
                    carts[150] = Minecart::new(999, CartKind::Rideable, at);
                    index.forget_carts();
                }
                if i >= carts.len() - 1 {
                    break;
                }
            }
        }
        assert!(index.carts.is_some(), "the index was used");
    }
    /// The cart index hands a cart every mob a scan finds in its box, and
    /// knows whether a mob rides it, however the mob list changed since.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn cart_index_finds_the_mobs_a_scan_finds() {
        use super::{CartIndex, CartKind, Minecart};
        use horizon_client::mobs::Mob;
        let mut game = super::super::native_machines::tests::server_game();
        let kinds: Vec<_> = ["minecraft:zombie", "minecraft:chicken", "minecraft:ravager"]
            .iter()
            .map(|k| game.res.mob_types().get(*k).cloned().unwrap())
            .collect();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = |m: f64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 1_000_000) as f64 / 1_000_000.0 * m
        };
        for id in 0..200u32 {
            let at = Vec3::new(next(40.0), 64.0, next(40.0));
            let kind = kinds[id as usize % kinds.len()].clone();
            game.mobs.push(Mob::new(id + 1, kind, at, 0.0));
        }
        let carts: Vec<Minecart> = (0..60)
            .map(|id| {
                Minecart::new(
                    1000 + id,
                    CartKind::Rideable,
                    Vec3::new(next(40.0), 64.0, next(40.0)),
                )
            })
            .collect();
        let mut index = CartIndex::default();
        let mut near = Vec::new();
        let mut check = |game: &Game<'_>, what: &str| {
            for c in &carts {
                let area = c.bounds().inflate(Vec3::new(0.2, 0.0, 0.2));
                let scan: Vec<u32> = (0..game.mobs.len() as u32)
                    .filter(|&j| area.intersects(game.mobs[j as usize].bounds()))
                    .collect();
                let ridden = game.mobs.iter().any(|m| m.extra.mount == Some(c.id));
                let mounted = index.mobs_near(&game.mobs, c.id, area, &mut near);
                assert_eq!(mounted, Some(ridden), "{what}: cart {}", c.id);
                near.retain(|&j| area.intersects(game.mobs[j as usize].bounds()));
                assert_eq!(near, scan, "{what}: cart {}", c.id);
            }
        };
        check(&game, "as placed");
        game.mobs[10].position = carts[3].position;
        game.mobs[11].extra.mount = Some(carts[4].id);
        check(&game, "one moved, one boarded");
        let kind = kinds[0].clone();
        game.mobs
            .insert(0, Mob::new(500, kind, carts[5].position, 0.0));
        game.mobs.retain(|m| m.id % 7 != 0);
        check(&game, "added and removed");
    }
    #[test]
    fn mob_walking_into_a_cart_pushes_it_by_a_twentieth() {
        // Issue #6776: a villager at x=0.13 next to a cart at x=0.5 east-west.
        let (xa, za) =
            super::cart_push_from(Vec3::new(0.13, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0)).unwrap();
        assert!((xa + 0.05).abs() < 1e-6 && za == 0.0, "{xa} {za}");
        assert!(super::cart_push_from(Vec3::ZERO, Vec3::ZERO).is_none());
    }
    /// Issue #6431: a blast too weak to push a cart past 40 damage still
    /// primes a TNT cart.
    #[test]
    fn a_blast_ignites_a_tnt_cart_however_little_damage_it_does() {
        assert!(cart_is_destroyed(false, 10.0, true));
        assert!(!cart_is_destroyed(false, 10.0, false));
        assert!(cart_is_destroyed(false, 40.5, false));
        assert!(!cart_is_destroyed(false, 40.0, false));
    }
    /// The collision detonation of a TNT minecart needs both the collision and
    /// a cart that was still moving: a nudge below a tenth of a block a tick
    /// does not set it off, and a fast cart on an open track does not either.
    #[test]
    fn a_tnt_cart_detonates_only_when_a_collision_stops_a_moving_one() {
        assert!(tnt_ignites_by_collision(true, 0.01));
        assert!(tnt_ignites_by_collision(true, 0.36), "three tenths a tick");
        assert!(!tnt_ignites_by_collision(true, 0.0), "stopped at the wall");
        assert!(!tnt_ignites_by_collision(true, 0.009), "a nudge");
        assert!(!tnt_ignites_by_collision(false, 0.36), "nothing in the way");
    }
    use super::*;

    /// Issue #8624: chest and hopper carts keep 0.995 of their speed when
    /// empty (0.98 when full), not the plain cart's 0.96, so powered rails
    /// carry them much further.
    #[test]
    fn container_carts_keep_more_speed_than_the_plain_cart() {
        assert!((container_keep(0, false, false) - 0.995).abs() < 1e-6);
        assert!((container_keep(15, false, false) - 0.98).abs() < 1e-6);
        assert!((container_keep(5, true, false) - 0.98).abs() < 1e-6);
        assert!((container_keep(0, false, true) - 0.995 * 0.95).abs() < 1e-6);
    }

    #[test]
    fn a_tnt_cart_lets_the_held_item_be_used() {
        // Entity.interact passes: a crossbow aimed at it shoots.
        assert!(CartKind::Tnt.interact_passes(false, false, true));
        assert!(CartKind::CommandBlock.interact_passes(false, false, false));
        assert!(!CartKind::CommandBlock.interact_passes(false, false, true));
        assert!(!CartKind::Rideable.interact_passes(false, false, false));
        assert!(CartKind::Rideable.interact_passes(true, false, false));
        assert!(CartKind::Rideable.interact_passes(false, true, false));
        for kind in [CartKind::Chest, CartKind::Hopper, CartKind::Furnace] {
            assert!(!kind.interact_passes(true, true, false));
        }
    }

    use horizon_world::nbt::Tag;

    fn saved_cart(kind: CartKind, extra: &[(&str, Tag)]) -> Tag {
        use horizon_world::nbt::JavaString;
        let mut tag = Minecart::new(1, kind, Vec3::ZERO).to_nbt([0; 4], None);
        if let Tag::Compound(map) = &mut tag {
            for (k, v) in extra {
                map.insert(JavaString::from(*k), v.clone());
            }
        }
        tag
    }

    /// AbstractMinecart.read/addAdditionalSaveData: `DisplayState` and
    /// `DisplayOffset` survive a save, the offset only off its default.
    #[test]
    fn custom_display_state_and_offset_roundtrip() {
        // BlockState.CODEC for a non-default state: {Name, Properties}.
        let state = Tag::Compound(
            [
                ("Name".into(), Tag::String("minecraft:furnace".into())),
                (
                    "Properties".into(),
                    Tag::Compound(
                        [("lit".into(), Tag::String("true".into()))]
                            .into_iter()
                            .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let saved = saved_cart(
            CartKind::Rideable,
            &[
                ("DisplayState", state.clone()),
                ("DisplayOffset", Tag::Int(10)),
            ],
        );
        let cart = Minecart::from_nbt(&saved, 1, CartKind::Rideable).unwrap();
        assert_eq!(cart.custom_display, Some(state.clone()));
        assert_eq!(display_offset(&cart), 10.0);
        let out = cart.to_nbt([0; 4], None);
        assert_eq!(out.get("DisplayState"), Some(&state));
        assert_eq!(out.get("DisplayOffset"), Some(&Tag::Int(10)));
    }

    /// `/data merge entity` on a cart: `EntityDataAccessor.setData` loads the
    /// merged compound into the live cart (`Entity.load`). Its display state,
    /// offset and name read back; its id, kind and unsaved state stay.
    #[test]
    fn data_merge_loads_into_a_live_cart() {
        let mut cart = Minecart::new(7, CartKind::Chest, Vec3::new(1.5, 64.0, 2.5));
        cart.hurt_time = 5;
        let old = cart.to_nbt([0; 4], None);
        let (patch, _) = horizon_world::nbt::parse_snbt(
            r#"{DisplayState:"minecraft:furnace",DisplayOffset:3,CustomName:"Cart",Pos:[1.5d,70.0d,2.5d]}"#,
        )
        .unwrap();
        let merged = horizon_client::data_cmd::merged(&old, &patch);
        cart.load(&merged).unwrap();
        assert_eq!(
            (cart.id, cart.kind, cart.hurt_time),
            (7, CartKind::Chest, 5)
        );
        assert_eq!(cart.position, Vec3::new(1.5, 70.0, 2.5));
        assert_eq!(display_offset(&cart), 3.0);
        let out = cart.to_nbt([0; 4], Some(&merged));
        assert_eq!(out.get("DisplayState"), patch.get("DisplayState"));
        assert_eq!(out.get("DisplayOffset"), Some(&Tag::Int(3)));
        assert_eq!(out.get("CustomName"), Some(&Tag::String("Cart".into())));
        // A compound without a position loads nothing.
        let mut no_pos = merged.clone();
        if let Tag::Compound(map) = &mut no_pos {
            map.remove(&"Pos".into());
            map.insert("DisplayOffset".into(), Tag::Int(9));
        }
        assert!(cart.load(&no_pos).is_none());
        assert_eq!(display_offset(&cart), 3.0);
    }

    /// A merge of an unrelated key loads the cart's own saved state back
    /// unchanged: a furnace cart keeps its fuel and push, a TNT cart its fuse,
    /// a hopper cart its Enabled flag.
    #[test]
    fn data_merge_of_a_name_keeps_the_carts_state() {
        let mut furnace = Minecart::new(1, CartKind::Furnace, Vec3::ZERO);
        (furnace.fuel, furnace.push) = (1200, Vec3::new(0.5, 0.0, -0.25));
        let mut tnt = Minecart::new(2, CartKind::Tnt, Vec3::ZERO);
        tnt.fuse = 40;
        let mut hopper = Minecart::new(3, CartKind::Hopper, Vec3::ZERO);
        hopper.enabled = false;
        let (patch, _) = horizon_world::nbt::parse_snbt(r#"{CustomName:"x"}"#).unwrap();
        for cart in [furnace, tnt, hopper] {
            let mut merged_cart = cart.clone();
            let merged = horizon_client::data_cmd::merged(&cart.to_nbt([0; 4], None), &patch);
            merged_cart.load(&merged).unwrap();
            assert_eq!(
                (
                    merged_cart.fuel,
                    merged_cart.push,
                    merged_cart.fuse,
                    merged_cart.enabled
                ),
                (cart.fuel, cart.push, cart.fuse, cart.enabled),
                "{:?}",
                cart.kind
            );
        }
    }

    #[test]
    fn air_display_state_shows_nothing() {
        let (air, stone) = (BlockStateId(0), BlockStateId(1));
        assert_eq!(shown_unless_air(Some(air), |s| s == air), None);
        assert_eq!(shown_unless_air(Some(stone), |s| s == air), Some(stone));
        assert_eq!(shown_unless_air(None, |s| s == air), None);
    }

    #[test]
    fn an_unreadable_display_state_is_dropped_and_not_saved() {
        let bad = Tag::String("mod:gone".into());
        let saved = saved_cart(CartKind::Rideable, &[("DisplayState", bad)]);
        let mut cart = Minecart::from_nbt(&saved, 1, CartKind::Rideable).unwrap();
        forget_unreadable_display(&mut cart, |_| false);
        assert!(cart.custom_display.is_none());
        assert!(
            cart.to_nbt([0; 4], Some(&saved))
                .get("DisplayState")
                .is_none()
        );
        // A readable one is kept.
        let mut cart = Minecart::from_nbt(&saved, 1, CartKind::Rideable).unwrap();
        forget_unreadable_display(&mut cart, |_| true);
        assert!(cart.custom_display.is_some());
    }

    #[test]
    fn display_offset_is_not_saved_at_its_default() {
        // A hopper cart's default is 1, a chest cart's 8, any other kind's 6.
        for (kind, default) in [
            (CartKind::Hopper, 1),
            (CartKind::Chest, 8),
            (CartKind::Rideable, 6),
        ] {
            let saved = saved_cart(kind, &[("DisplayOffset", Tag::Int(default))]);
            let cart = Minecart::from_nbt(&saved, 1, kind).unwrap();
            assert_eq!(display_offset(&cart), f64::from(default));
            let out = cart.to_nbt([0; 4], Some(&saved));
            assert!(out.get("DisplayOffset").is_none(), "{kind:?}");
            assert!(out.get("DisplayState").is_none(), "{kind:?}");
        }
        // A hopper cart saved with 6 keeps it: that is not its default.
        let saved = saved_cart(CartKind::Hopper, &[("DisplayOffset", Tag::Int(6))]);
        let cart = Minecart::from_nbt(&saved, 1, CartKind::Hopper).unwrap();
        assert_eq!(
            cart.to_nbt([0; 4], None).get("DisplayOffset"),
            Some(&Tag::Int(6))
        );
        // And a chest cart saved with 6, the base default, keeps it too.
        let saved = saved_cart(CartKind::Chest, &[("DisplayOffset", Tag::Int(6))]);
        let cart = Minecart::from_nbt(&saved, 1, CartKind::Chest).unwrap();
        assert_eq!(display_offset(&cart), 6.0);
        assert_eq!(
            cart.to_nbt([0; 4], None).get("DisplayOffset"),
            Some(&Tag::Int(6))
        );
    }

    #[test]
    fn display_offset_matches_vanilla_for_each_cart_kind() {
        // MinecartChest and MinecartHopper override getDefaultDisplayOffset;
        // the rest keep AbstractMinecart's 6.
        assert_eq!(default_display_offset(CartKind::Chest), 8.0);
        assert_eq!(default_display_offset(CartKind::Hopper), 1.0);
        assert_eq!(default_display_offset(CartKind::Furnace), 6.0);
        assert_eq!(default_display_offset(CartKind::Tnt), 6.0);
        assert_eq!(default_display_offset(CartKind::CommandBlock), 6.0);
        assert_eq!(default_display_offset(CartKind::Spawner), 6.0);
        assert_eq!(default_display_offset(CartKind::Rideable), 6.0);
        // A cart without a saved offset draws at its kind's default.
        let chest = Minecart::new(1, CartKind::Chest, Vec3::ZERO);
        assert_eq!(display_offset(&chest), 8.0);
    }

    /// `MinecartSpawner`: its `BaseSpawner` is saved and loaded with the cart (the delay, the
    /// ranges and the `SpawnData`), it drops and is picked as a plain minecart, and it shows a
    /// spawner block.
    #[test]
    fn a_spawner_cart_keeps_its_spawner_and_drops_a_minecart() {
        use horizon_world::nbt::Tag;
        let saved = Tag::compound([
            ("id", Tag::String("minecraft:spawner_minecart".into())),
            (
                "Pos",
                Tag::List(
                    [0.5f64, 40.5, 0.5]
                        .map(|v| Tag::Double(v.to_bits()))
                        .to_vec(),
                ),
            ),
            ("Delay", Tag::Short(37)),
            ("MaxSpawnDelay", Tag::Short(120)),
            ("SpawnRange", Tag::Short(2)),
            (
                "SpawnData",
                Tag::compound([(
                    "entity",
                    Tag::compound([("id", Tag::String("minecraft:zombie".into()))]),
                )]),
            ),
        ]);
        let mut cart = Minecart::from_nbt(&saved, 1, CartKind::Spawner).unwrap();
        let spawner = cart.spawner.as_mut().expect("a spawner cart has a spawner");
        assert_eq!(
            (
                spawner.spawn_delay,
                spawner.max_spawn_delay,
                spawner.spawn_range
            ),
            (37, 120, 2)
        );
        // No raw tag to copy from: what comes out is the cart's own save.
        spawner.spawn_delay = 41;
        let out = cart.to_nbt([0; 4], None);
        assert_eq!(out.get("Delay"), Some(&Tag::Short(41)));
        assert_eq!(out.get("MaxSpawnDelay"), Some(&Tag::Short(120)));
        assert_eq!(
            out.get("SpawnData")
                .and_then(|d| d.get("entity"))
                .and_then(|e| e.get("id")),
            Some(&Tag::String("minecraft:zombie".into()))
        );
        assert_eq!(CartKind::Spawner.item(), "minecraft:minecart");
        assert_eq!(
            CartKind::from_id("minecraft:spawner_minecart"),
            Some(CartKind::Spawner)
        );
        // A summoned one has a spawner of its own.
        assert!(
            Minecart::new(2, CartKind::Spawner, Vec3::ZERO)
                .spawner
                .is_some()
        );
    }

    #[test]
    fn a_cow_boards_an_empty_cart_and_an_iron_golem_does_not() {
        assert!(pickup_instead_of_push(Species::Cow, false, false));
        assert!(can_ride_cart(Species::Cow, 0));
        assert!(!pickup_instead_of_push(Species::IronGolem, false, false));
        assert!(!pickup_instead_of_push(Species::Cow, false, true));
        assert!(!pickup_instead_of_push(Species::Cow, true, false));
        assert!(!can_ride_cart(Species::Cow, 60));
        assert!(!can_ride_cart(Species::Warden, 0));
    }

    /// `WitherBoss.canRide` returns false, so a moving cart cannot scoop the boss up.
    #[test]
    fn a_wither_never_rides_a_cart() {
        assert!(!can_ride_cart(Species::Wither, 0));
    }

    #[test]
    fn a_cart_seat_matches_the_player_offset() {
        let at = Vec3::new(1.0, 10.0, 2.0);
        assert!((cart_seat(at, 0.0).y - 10.1875).abs() < 1e-12);
        assert!((cart_seat(at, 0.6).y - (10.0 + SEAT)).abs() < 1e-12);
        assert_eq!(cart_seat(at, 0.0).x, 1.0);
        assert_eq!(cart_seat(at, 0.0).z, 2.0);
    }

    #[test]
    fn a_cart_saves_as_never_ticked_until_it_ticks() {
        use horizon_world::nbt::Tag;
        let generated = Tag::compound([
            ("id", Tag::String("minecraft:chest_minecart".into())),
            (
                "Pos",
                Tag::List(
                    [0.5f64, 40.5, 0.5]
                        .map(|v| Tag::Double(v.to_bits()))
                        .to_vec(),
                ),
            ),
            ("HasTicked", Tag::Byte(1)),
            ("Fire", Tag::Short(0)),
        ]);
        let mut cart = Minecart::from_nbt(&generated, 1, CartKind::Chest).unwrap();
        let saved = cart.to_nbt([0; 4], Some(&generated));
        assert_eq!(saved.get("HasTicked"), Some(&Tag::Byte(1)));
        assert_eq!(saved.get("Fire"), Some(&Tag::Short(0)));
        cart.first_tick = false;
        let saved = cart.to_nbt([0; 4], Some(&generated));
        assert_eq!(saved.get("HasTicked"), Some(&Tag::Byte(0)));
        assert_eq!(saved.get("Fire"), Some(&Tag::Short(0)));
    }

    #[test]
    fn a_command_cart_saves_its_base_command_block() {
        use horizon_world::nbt::Tag;
        let saved = Tag::compound([
            (
                "Pos",
                Tag::List(
                    [0.5f64, 40.5, 0.5]
                        .map(|v| Tag::Double(v.to_bits()))
                        .to_vec(),
                ),
            ),
            ("Command", Tag::String("say hi".into())),
            ("SuccessCount", Tag::Int(3)),
            ("TrackOutput", Tag::Byte(1)),
            ("LastOutput", Tag::String("[12:00:00] hi".into())),
            ("LastExecution", Tag::Long(42)),
        ]);
        let cart = Minecart::from_nbt(&saved, 1, CartKind::CommandBlock).unwrap();
        assert_eq!(cart.command.command, "say hi");
        assert_eq!(cart.command.success_count, 0);
        // readAdditionalSaveData syncs both values.
        assert_eq!(cart.synced_command, "say hi");
        assert_eq!(cart.synced_output, "[12:00:00] hi");
        let out = cart.to_nbt([0; 4], Some(&saved));
        assert_eq!(out.get("SuccessCount"), Some(&Tag::Int(0)));
        for key in ["Command", "TrackOutput", "LastOutput", "LastExecution"] {
            assert_eq!(out.get(key), saved.get(key), "{key}");
        }
        assert_eq!(out.get("UpdateLastExecution"), Some(&Tag::Byte(1)));
        // A block-only flag never reaches the entity.
        assert!(out.get("powered").is_none() && out.get("auto").is_none());
        // Untracked output drops the saved last output.
        let mut quiet = cart.clone();
        quiet.command.track_output = false;
        let out = quiet.to_nbt([0; 4], Some(&saved));
        assert!(out.get("LastOutput").is_none());
        assert_eq!(out.get("TrackOutput"), Some(&Tag::Byte(0)));
    }

    #[test]
    fn furnace_push_keeps_its_length_under_gravity() {
        // Gravity gives the movement a vertical part. The push turns onto the
        // movement at the same 3D length every tick, as vanilla's does.
        let movement = Vec3::new(0.2, -0.04, 0.0);
        let mut push = Vec3::new(0.5, 0.0, 0.0);
        for _ in 0..1000 {
            push = push_along(push, movement);
        }
        assert!((push.length() - 0.5).abs() < 1e-12);
        assert!((push.x - 0.5 * 0.2 / movement.length()).abs() < 1e-12);
    }

    #[test]
    fn a_furnace_saves_its_push_without_y() {
        use horizon_world::nbt::Tag;
        let generated = Tag::compound([
            ("id", Tag::String("minecraft:furnace_minecart".into())),
            (
                "Pos",
                Tag::List(
                    [0.5f64, 40.5, 0.5]
                        .map(|v| Tag::Double(v.to_bits()))
                        .to_vec(),
                ),
            ),
        ]);
        let mut cart = Minecart::from_nbt(&generated, 1, CartKind::Furnace).unwrap();
        cart.push = Vec3::new(0.5, 0.7, 0.25);
        let saved = cart.to_nbt([0; 4], Some(&generated));
        assert_eq!(saved.get("PushX"), Some(&Tag::Double(0.5f64.to_bits())));
        assert_eq!(saved.get("PushZ"), Some(&Tag::Double(0.25f64.to_bits())));
        let loaded = Minecart::from_nbt(&saved, 1, CartKind::Furnace).unwrap();
        assert_eq!(loaded.push, Vec3::new(0.5, 0.0, 0.25));
    }
}
horizon_client::impl_joined!(Minecart);

#[cfg(test)]
mod tnt_impact_tests {
    use super::{CartKind, Minecart};
    use crate::game::native_machines::tests::server_game;
    use horizon_core::geometry::Vec3;

    /// Ticks a moving TNT cart until it stops or blows up; true when it
    /// blew up.
    fn runs_into_the_wall(rails: bool, at: Vec3, velocity: Vec3) -> bool {
        let mut game = server_game();
        let registry = game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let rail = registry.default_state("minecraft:rail").unwrap();
        let mut east_west = registry.state(rail).unwrap().clone();
        east_west
            .properties
            .insert("shape".into(), "east_west".into());
        let rail = registry.id(&east_west).unwrap();
        for x in 0..16 {
            for z in 0..16 {
                game.world.set(x, 63, z, stone).unwrap();
            }
        }
        for z in 0..16 {
            game.world.set(9, 64, z, stone).unwrap();
            game.world.set(9, 65, z, stone).unwrap();
        }
        if rails {
            for x in 1..9 {
                game.world.set(x, 64, 8, rail).unwrap();
            }
        }
        let mut cart = Minecart::new(1, CartKind::Tnt, at);
        cart.velocity = velocity;
        for _ in 0..40 {
            if !game.tick_cart(&mut cart, [0.0; 2], 0, &mut super::CartIndex::default()) {
                return true;
            }
        }
        false
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_tnt_cart_stopped_head_on_by_a_wall_does_not_blow_up() {
        // OldMinecartBehavior.moveAlongTrack: the move zeroes the blocked x
        // and the track's re-pointing scales what is left, so MinecartTNT
        // reads no speed at the wall.
        assert!(!runs_into_the_wall(
            true,
            Vec3::new(3.5, 64.0, 8.5),
            Vec3::new(0.4, 0.0, 0.0)
        ));
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_tnt_cart_off_its_rails_glancing_off_a_wall_blows_up() {
        // comeOffTrack keeps the delta the move resolved: the wall zeroes x,
        // z goes on at over a tenth of a block a tick.
        assert!(runs_into_the_wall(
            false,
            Vec3::new(8.3, 64.0, 4.5),
            Vec3::new(0.4, 0.0, 0.4)
        ));
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_cart_on_a_slope_presses_a_plate_at_its_temporary_position() {
        // OldMinecartBehavior.moveAlongTrack places the cart at the rail's top
        // height, moves it horizontally (Entity.move records the step) and settles
        // it on the slope; the tick's applyEffectsFromBlocks still sweeps the step.
        let mut game = server_game();
        let registry = game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let plate = registry
            .default_state("minecraft:oak_pressure_plate")
            .unwrap();
        let rail = registry.default_state("minecraft:rail").unwrap();
        let mut slope = registry.state(rail).unwrap().clone();
        slope
            .properties
            .insert("shape".into(), "ascending_east".into());
        let rail = registry.id(&slope).unwrap();
        for x in 0..16 {
            for z in 0..16 {
                game.world.set(x, 63, z, stone).unwrap();
            }
        }
        game.world.set(5, 64, 8, rail).unwrap();
        game.world.set(6, 64, 8, stone).unwrap();
        game.world.set(6, 65, 8, plate).unwrap();
        let mut cart = Minecart::new(1, CartKind::Rideable, Vec3::new(5.5, 64.5, 8.5));
        cart.velocity = Vec3::new(0.4, 0.0, 0.0);
        game.carts.push(cart.clone());
        assert!(game.tick_cart(&mut cart, [0.0; 2], 0, &mut super::CartIndex::default()));
        let powered = registry
            .state(game.world.block(6, 65, 8))
            .unwrap()
            .properties
            .get("powered")
            .cloned();
        assert_eq!(
            powered.as_deref(),
            Some("true"),
            "{:?}",
            registry.state(game.world.block(6, 65, 8))
        );
    }
}

#[cfg(test)]
mod cart_push_tests {
    use super::super::native_machines::tests::server_game;
    use super::{CartKind, Minecart};
    use horizon_client::{items::ItemStack, save::RolledLoot};
    use horizon_core::geometry::Vec3;

    /// Two chest carts pushed apart (`OldMinecartBehavior.pushAndPickupEntities`):
    /// the push runs on copies of the ticked cart, and the one still holding
    /// its mineshaft roll keeps the roll, so it is saved as its loot table.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_pushed_chest_cart_keeps_its_rolled_loot() {
        let mut game = server_game();
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        for x in 4..13 {
            for z in 4..13 {
                game.world.set(x, 63, z, stone).unwrap();
            }
        }
        let items = vec![Some(ItemStack::new("minecraft:rail", 12)); 27];
        let loot = RolledLoot {
            pos: (8, 64, 8),
            table: "minecraft:chests/abandoned_mineshaft".into(),
            seed: Some(42),
            items: horizon_client::save::encode_items(&items),
        };
        let mut rolled = Minecart::new(1, CartKind::Chest, Vec3::new(8.3, 64.0, 8.5));
        rolled.items = items;
        rolled.loot = Some(loot.clone());
        game.carts.push(rolled);
        // Facing along X (yaw 0) and 0.4 apart on X: the old behaviour pushes.
        game.carts
            .push(Minecart::new(2, CartKind::Chest, Vec3::new(8.7, 64.0, 8.5)));
        for _ in 0..5 {
            game.tick_carts();
        }
        let cart = |id| game.carts.iter().find(|c| c.id == id).unwrap();
        assert!(
            cart(2).position.x - cart(1).position.x > 0.4,
            "the carts were pushed apart: {} {}",
            cart(1).position.x,
            cart(2).position.x
        );
        assert_eq!(cart(1).loot.as_ref(), Some(&loot));
        assert_eq!(cart(2).loot, None);
    }
}

#[cfg(test)]
mod container_drop_tests {
    use super::super::boats::Boat;
    use super::super::native_machines::tests::server_game;
    use super::{CartKind, Minecart};
    use horizon_client::items::ItemStack;
    use horizon_core::geometry::Vec3;
    use horizon_core::random::{LegacyRandom, RandomSource};
    use std::sync::Arc;

    /// Advances `r` past the draws a spill takes from the level random, as
    /// vanilla's `Containers.dropContents` takes them: for each slot
    /// (`Containers.dropItemStack`) its spot, then for each pile the split
    /// size and the triangle motion. Every count here is at most 10, so a
    /// slot with items is a single pile.
    fn vanilla_spill_draws(r: &mut LegacyRandom, counts: &[u32]) {
        for &count in counts {
            for _ in 0..3 {
                r.next_double();
            }
            if count > 0 {
                let _ = r.next_int_bound(21);
                for _ in 0..3 {
                    r.triangle(0.0, 0.114_850_001_711_398_36);
                }
            }
        }
    }

    /// A chest boat or chest minecart broken with entity_drops on spills its
    /// slots in `remove` and then the emptied container again in
    /// `chestVehicleDestroyed`; the hull item takes the item entity's own
    /// random. The level random must end where vanilla's does.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_broken_chest_vehicle_draws_the_level_random_like_vanilla() {
        let mut slots = vec![None; 27];
        slots[0] = Some(ItemStack::new("minecraft:diamond", 5));
        slots[4] = Some(ItemStack::new("minecraft:dirt", 8));
        let mut counts = vec![0; 27];
        counts[0] = 5;
        counts[4] = 8;
        for boat in [true, false] {
            let mut game = server_game();
            assert!(game.ui.rules.bool("entity_drops"));
            let at = Vec3::new(8.5, 64.0, 8.5);
            if boat {
                let mut b = Boat::new(1, Arc::from("oak"), at, 0.0);
                b.chest = true;
                b.items = slots.clone();
                game.boats.push(b);
            } else {
                let mut c = Minecart::new(1, CartKind::Chest, at);
                c.items = slots.clone();
                game.carts.push(c);
            }
            game.world.random = LegacyRandom::new(1234);
            if boat {
                assert_eq!(game.hurt_boat_result(0, 100.0, false), Some(true));
                assert!(game.boats.is_empty());
            } else {
                assert!(game.hurt_cart_result(0, 100.0, false, false));
                assert!(game.carts.is_empty());
            }
            let mut expected = LegacyRandom::new(1234);
            vanilla_spill_draws(&mut expected, &counts);
            vanilla_spill_draws(&mut expected, &[0; 27]);
            assert_eq!(
                game.world.random.next_long(),
                expected.next_long(),
                "boat: {boat}"
            );
            // The two piles and the hull item.
            assert_eq!(game.dropped.len(), 3, "boat: {boat}");
        }
    }
}
