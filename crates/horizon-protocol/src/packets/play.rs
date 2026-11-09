//! Play state packets: world, entities, player state, inventory and chat.
use crate::{
    Error, Result,
    buf::{
        Bool, Byte, BytesN, ChunkPos, Codec, Double, Float, Ident, Int, List, Long, LpVec3, Nbt,
        Opt, OptVarInt, Pos, Reader, SectionPos, Short, Str, StrN, UByte, Uuid, VarInt, VarLong,
        VarVec3i, Vec3d, Writer,
    },
    chunk::{ChunkData, LightData},
    entity_data::{EntityData, Particle, Value},
    item::{Component, DelimitedSlot, Hashed, HashedSlot, Slot, Stack},
    packet, packet_enum,
    packets::{
        Empty,
        configuration::{
            ClientInformation, CookieRequest, CookieResponse, CustomClickAction, CustomPayload,
            Disconnect, KeepAlive, Ping, ResourcePackPop, ResourcePackPush, ResourcePackResponse,
            ServerLinks, StoreCookie, Transfer, UpdateTags,
        },
        login::Property,
    },
};
use horizon_world::nbt::Tag;

mod commands;
pub use commands::{ARGUMENT_TYPES, ArgumentProperties, CommandNode, Commands};

// ---- shared structures -------------------------------------------------

packet!(
    /// GlobalPos: a dimension and a block position.
    GlobalPos { dimension: String => Ident, pos: [i32; 3] => Pos }
);
packet!(
    /// CommonPlayerSpawnInfo.
    SpawnInfo {
        dimension_type: i32 => VarInt,
        dimension: String => Ident,
        seed: i64 => Long,
        game_type: i32 => VarInt,
        previous_game_type: Option<i32> => OptVarInt,
        debug: bool => Bool,
        flat: bool => Bool,
        last_death: Option<GlobalPos> => Opt<GlobalPos>,
        portal_cooldown: i32 => VarInt,
        sea_level: i32 => VarInt,
    }
);

/// Relative flags (Relative.pack): X, Y, Z, Y_ROT, X_ROT, DELTA_X/Y/Z, ROTATE_DELTA.
pub mod relative {
    pub const X: i32 = 1;
    pub const Y: i32 = 2;
    pub const Z: i32 = 4;
    pub const Y_ROT: i32 = 8;
    pub const X_ROT: i32 = 16;
    pub const DELTA_X: i32 = 32;
    pub const DELTA_Y: i32 = 64;
    pub const DELTA_Z: i32 = 128;
    pub const ROTATE_DELTA: i32 = 256;
}

// ---- clientbound -------------------------------------------------------

packet!(AddEntity {
    id: i32 => VarInt,
    uuid: u128 => Uuid,
    kind: i32 => VarInt,
    position: [f64; 3] => Vec3d,
    velocity: [f64; 3] => LpVec3,
    pitch: i8 => Byte,
    yaw: i8 => Byte,
    head_yaw: i8 => Byte,
    data: i32 => VarInt,
});
packet!(Animate { id: i32 => VarInt, action: u8 => UByte });
packet!(BlockChangedAck { sequence: i32 => VarInt });
packet!(
    /// ClientboundSelectAdvancementsTabPacket: the advancement tab the server keeps for the
    /// player, or none. `ClientPacketListener.handleSelectAdvancementsTab` opens the screen on
    /// that tab, and closes it for none.
    SelectAdvancementsTab { tab: Option<String> => Opt<Ident> }
);
packet!(BlockDestruction { id: i32 => VarInt, pos: [i32; 3] => Pos, progress: u8 => UByte });
packet!(BlockEntityData { pos: [i32; 3] => Pos, kind: i32 => VarInt, tag: Tag => Nbt });
packet!(BlockUpdate { pos: [i32; 3] => Pos, state: i32 => VarInt });
// ClientboundAddTransientBlockPacket: a block the client shows for a moment
// while its section mesh catches up (a falling block that landed).
packet!(AddTransientBlock { pos: [i32; 3] => Pos, state: i32 => VarInt });
packet!(ChangeDifficulty { difficulty: i32 => VarInt, locked: bool => Bool });
// ResourceKey<GameRule<?>> is an identifier, followed by its string value.
packet!(GameRuleEntry { name: String => Ident, value: String => Str });
packet!(GameRuleValues { entries: Vec<GameRuleEntry> => List<GameRuleEntry> });
packet!(ChunkBatchFinished { size: i32 => VarInt });
packet!(ContainerClose { container: i32 => VarInt });
packet!(ContainerSetContent {
    container: i32 => VarInt,
    state: i32 => VarInt,
    items: Vec<Option<Stack>> => List<Slot>,
    carried: Option<Stack> => Slot,
});
/// ItemCost.STREAM_CODEC: item, count, then the exact component predicate
/// (a list of typed component values, with no patch removal count).
#[derive(Clone, Debug, PartialEq)]
pub struct NetCost {
    pub item: i32,
    pub count: i32,
    pub components: Vec<(i32, Component)>,
}
impl From<Stack> for NetCost {
    fn from(stack: Stack) -> Self {
        Self {
            item: stack.item,
            count: stack.count,
            components: stack.added,
        }
    }
}
impl From<NetCost> for Stack {
    fn from(cost: NetCost) -> Self {
        Self {
            item: cost.item,
            count: cost.count,
            added: cost.components,
            removed: Vec::new(),
            wire: Default::default(),
        }
    }
}
/// MerchantOffer.STREAM_CODEC.
#[derive(Clone, Debug, PartialEq)]
pub struct NetOffer {
    pub cost_a: NetCost,
    pub result: Stack,
    pub cost_b: Option<NetCost>,
    pub out_of_stock: bool,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub special_price: i32,
    pub price_multiplier: f32,
    pub demand: i32,
}
pub struct Offers;
impl Codec<Vec<NetOffer>> for Offers {
    fn read(r: &mut Reader) -> Result<Vec<NetOffer>> {
        fn cost(r: &mut Reader) -> Result<NetCost> {
            let item = r.varint()?;
            let count = r.varint()?;
            let n = r.count(r.remaining())?;
            let mut components = Vec::with_capacity(n.min(64));
            for _ in 0..n {
                let kind = r.varint()?;
                components.push((kind, Component::read(kind, r)?));
            }
            Ok(NetCost {
                item,
                count,
                components,
            })
        }
        let n = r.count(r.remaining())?;
        let mut out = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            let cost_a = cost(r)?;
            let result = Slot::read(r)?.ok_or_else(|| Error::new("empty merchant result"))?;
            let cost_b = if r.bool()? { Some(cost(r)?) } else { None };
            out.push(NetOffer {
                cost_a,
                result,
                cost_b,
                out_of_stock: r.bool()?,
                uses: r.i32()?,
                max_uses: r.i32()?,
                xp: r.i32()?,
                special_price: r.i32()?,
                price_multiplier: r.f32()?,
                demand: r.i32()?,
            });
        }
        Ok(out)
    }
    fn write(v: &Vec<NetOffer>, w: &mut Writer) {
        fn cost(c: &NetCost, w: &mut Writer) {
            w.varint(c.item);
            w.varint(c.count);
            w.varint(c.components.len() as i32);
            for (kind, value) in &c.components {
                w.varint(*kind);
                value.write(*kind, w);
            }
        }
        w.varint(v.len() as i32);
        for o in v {
            cost(&o.cost_a, w);
            Slot::write(&Some(o.result.clone()), w);
            w.bool(o.cost_b.is_some());
            if let Some(b) = &o.cost_b {
                cost(b, w);
            }
            w.bool(o.out_of_stock);
            w.i32(o.uses);
            w.i32(o.max_uses);
            w.i32(o.xp);
            w.i32(o.special_price);
            w.f32(o.price_multiplier);
            w.i32(o.demand);
        }
    }
}
packet!(
    /// ClientboundMerchantOffersPacket.
    MerchantOffers {
        container: i32 => VarInt,
        offers: Vec<NetOffer> => Offers,
        level: i32 => VarInt,
        xp: i32 => VarInt,
        show_progress: bool => Bool,
        can_restock: bool => Bool,
    }
);
packet!(ContainerSetData { container: i32 => VarInt, id: i16 => Short, value: i16 => Short });
// ClientboundCooldownPacket: the cooldown group (an identifier) and the ticks
// its entry lasts (0 ends it).
packet!(Cooldown { group: String => Ident, duration: i32 => VarInt });
packet!(
    /// ClientboundCustomChatCompletionsPacket: how the server changes the
    /// extra chat completions (`custom_chat_action`) and the entries.
    CustomChatCompletions { action: i32 => VarInt, entries: Vec<String> => List<Str> }
);
/// ClientboundCustomChatCompletionsPacket.Action, by id (`ByIdMap.continuous`
/// with `OutOfBoundsStrategy.ZERO`: an unknown id is `ADD`).
pub mod custom_chat_action {
    pub const ADD: i32 = 0;
    pub const REMOVE: i32 = 1;
    pub const SET: i32 = 2;
}
packet!(ContainerSetSlot { container: i32 => VarInt, state: i32 => VarInt, slot: i16 => Short, item: Option<Stack> => Slot });
/// A holder reference: registry id + 1, or 0 for an inline value (unsupported).
pub struct HolderId;
impl Codec<i32> for HolderId {
    fn read(r: &mut Reader) -> Result<i32> {
        match r.varint()? {
            0 => Err(Error::new("inline registry values are not supported")),
            id => Ok(id - 1),
        }
    }
    fn write(v: &i32, w: &mut Writer) {
        w.varint(v + 1)
    }
}
/// `Holder<Dialog>` as Dialog.STREAM_CODEC sends it: an entry of the
/// `minecraft:dialog` registry the server sent in configuration, or the
/// dialog itself.
#[derive(Clone, Debug, PartialEq)]
pub enum DialogHolder {
    Registry(i32),
    Direct(Tag),
}
impl Codec<DialogHolder> for DialogHolder {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.varint()? {
            0 => Self::Direct(r.nbt()?),
            id => Self::Registry(id - 1),
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        match v {
            Self::Registry(id) => w.varint(id + 1),
            Self::Direct(tag) => {
                w.varint(0);
                w.nbt(tag);
            }
        }
    }
}
packet!(
    /// ClientboundShowDialogPacket.
    ShowDialog { dialog: DialogHolder => DialogHolder }
);
packet!(
    /// ChatType.Bound.
    ChatTypeBound { chat_type: i32 => HolderId, name: Tag => Nbt, target: Option<Tag> => Opt<Nbt> }
);
packet!(DisguisedChat { message: Tag => Nbt, chat_type: ChatTypeBound => ChatTypeBound });
packet!(EntityEvent { id: i32 => Int, event: i8 => Byte });
/// PositionPath: Linear(end position) or Stepped(list of (position, tick offset)).
#[derive(Clone, Debug, PartialEq)]
pub enum PositionPath {
    Linear([f64; 3]),
    Stepped(Vec<([f64; 3], i32)>),
}
impl PositionPath {
    pub fn end(&self) -> [f64; 3] {
        match self {
            Self::Linear(p) => *p,
            Self::Stepped(steps) => steps.last().map_or([0.0; 3], |s| s.0),
        }
    }
}
impl Codec<PositionPath> for PositionPath {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.varint()? {
            1 => {
                let n = r.count(r.remaining() / 25)?;
                Self::Stepped(
                    (0..n)
                        .map(|_| Ok((Vec3d::read(r)?, r.varint()?)))
                        .collect::<Result<_>>()?,
                )
            }
            _ => Self::Linear(Vec3d::read(r)?),
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        match v {
            Self::Linear(p) => {
                w.varint(0);
                Vec3d::write(p, w);
            }
            Self::Stepped(steps) => {
                w.varint(1);
                w.varint(steps.len() as i32);
                for (p, t) in steps {
                    Vec3d::write(p, w);
                    w.varint(*t);
                }
            }
        }
    }
}
packet!(EntityPositionSync { id: i32 => VarInt, path: PositionPath => PositionPath, yaw: f32 => Float, pitch: f32 => Float, on_ground: bool => Bool });
packet!(ForgetLevelChunk { pos: [i32; 2] => ChunkPos });
packet!(
    /// ClientboundChunksBiomesPacket.ChunkBiomeData: a column and its
    /// sections' biome containers, in order (`extractChunkData`).
    ChunkBiomeData { pos: [i32; 2] => ChunkPos, buffer: Vec<u8> => BytesN<2097152> }
);
impl ChunkBiomeData {
    /// `LevelChunk.replaceBiomes`' read: `count` sections' biome
    /// containers, one after another.
    pub fn biomes(&self, count: usize, biome_bits: u8) -> Result<Vec<crate::chunk::Paletted>> {
        let mut r = Reader::new(&self.buffer);
        (0..count)
            .map(|_| {
                crate::chunk::Paletted::read(&mut r, crate::chunk::Strategy::Biomes, biome_bits)
            })
            .collect()
    }
    /// `extractChunkData`: the sections' biome containers, in order.
    pub fn from_biomes(pos: [i32; 2], biomes: &[crate::chunk::Paletted]) -> Self {
        let mut w = Writer::new();
        for b in biomes {
            b.write(&mut w);
        }
        Self {
            pos,
            buffer: w.data,
        }
    }
}
packet!(
    /// ClientboundChunksBiomesPacket: columns whose biomes changed
    /// (`/fillbiome`, through `ChunkMap.resendBiomesForChunks`).
    ChunksBiomes { chunks: Vec<ChunkBiomeData> => List<ChunkBiomeData> }
);
packet!(GameEvent { event: u8 => UByte, param: f32 => Float });
packet!(HurtAnimation { id: i32 => VarInt, yaw: f32 => Float });
packet!(
    /// ClientboundDamageEventPacket: the hurt entity, the damage type's
    /// registry id, the causing and the direct entity (none: -1 on the wire's
    /// id + 1) and the source's position.
    DamageEvent {
        id: i32 => VarInt,
        source_type: i32 => VarInt,
        cause: Option<i32> => OptVarInt,
        direct: Option<i32> => OptVarInt,
        position: Option<[f64; 3]> => Opt<Vec3d>,
    }
);
packet!(LevelChunkWithLight { x: i32 => Int, z: i32 => Int, chunk: ChunkData => ChunkData, light: LightData => LightData });
packet!(LevelEvent { kind: i32 => Int, pos: [i32; 3] => Pos, data: i32 => Int, global: bool => Bool });
packet!(LightUpdate { x: i32 => VarInt, z: i32 => VarInt, light: LightData => LightData });
packet!(Login {
    player_id: i32 => Int,
    hardcore: bool => Bool,
    levels: Vec<String> => List<Ident>,
    max_players: i32 => VarInt,
    chunk_radius: i32 => VarInt,
    simulation_distance: i32 => VarInt,
    reduced_debug_info: bool => Bool,
    show_death_screen: bool => Bool,
    limited_crafting: bool => Bool,
    spawn: SpawnInfo => SpawnInfo,
    online_mode: bool => Bool,
    enforces_secure_chat: bool => Bool,
});
packet!(
/// MapDecoration: a registry holder, its map position and rotation, and an
/// optional name.
MapDecoration {
    kind: i32 => VarInt,
    x: i8 => Byte,
    y: i8 => Byte,
    rot: i8 => Byte,
    name: Option<Tag> => Opt<Nbt>,
}
);

/// MapItemSavedData.MapPatch: an empty patch is a zero width.
#[derive(Clone, Debug, PartialEq)]
pub struct MapPatch {
    pub start_x: i32,
    pub start_y: i32,
    pub width: i32,
    pub height: i32,
    pub colors: Vec<u8>,
}
pub struct MapPatchOpt;
impl Codec<Option<MapPatch>> for MapPatchOpt {
    fn read(r: &mut Reader) -> Result<Option<MapPatch>> {
        let width = i32::from(r.u8()?);
        if width == 0 {
            return Ok(None);
        }
        let height = i32::from(r.u8()?);
        let start_x = i32::from(r.u8()?);
        let start_y = i32::from(r.u8()?);
        let n = r.count(r.remaining())?;
        let colors = r.bytes(n)?.to_vec();
        Ok(Some(MapPatch {
            start_x,
            start_y,
            width,
            height,
            colors,
        }))
    }
    fn write(patch: &Option<MapPatch>, w: &mut Writer) {
        let Some(patch) = patch else {
            w.u8(0);
            return;
        };
        w.u8(patch.width as u8);
        w.u8(patch.height as u8);
        w.u8(patch.start_x as u8);
        w.u8(patch.start_y as u8);
        w.varint(patch.colors.len() as i32);
        w.bytes(&patch.colors);
    }
}

packet!(MapItemData {
    map_id: i32 => VarInt,
    scale: i8 => Byte,
    locked: bool => Bool,
    decorations: Option<Vec<MapDecoration>> => Opt<List<MapDecoration>>,
    patch: Option<MapPatch> => MapPatchOpt,
});
/// VecDelta: per-axis deltas in 1/4096 blocks, linear or in timed steps.
#[derive(Clone, Debug, PartialEq)]
pub enum VecDelta {
    Linear([i16; 3]),
    Stepped(Vec<([i16; 3], i32)>),
}
impl VecDelta {
    /// VecDeltaCodec.decode against a base position: zero deltas keep the
    /// base coordinate exactly, others add to its 1/4096 quantization.
    pub fn apply(base: [f64; 3], delta: [i16; 3]) -> [f64; 3] {
        let mut out = base;
        for i in 0..3 {
            if delta[i] != 0 {
                out[i] = (horizon_core::jmath::round(base[i] * 4096.0) + i64::from(delta[i]))
                    as f64
                    / 4096.0;
            }
        }
        out
    }
    /// The final position after all steps.
    pub fn end(&self, base: [f64; 3]) -> [f64; 3] {
        match self {
            Self::Linear(d) => Self::apply(base, *d),
            Self::Stepped(steps) => steps.iter().fold(base, |p, (d, _)| Self::apply(p, *d)),
        }
    }
    fn read(r: &mut Reader, steps: usize) -> Result<Self> {
        if steps == 0 {
            return Ok(Self::Linear([r.i16()?, r.i16()?, r.i16()?]));
        }
        if steps > r.remaining() / 7 {
            return Err(Error::new("VecDelta with too many steps"));
        }
        Ok(Self::Stepped(
            (0..steps)
                .map(|_| {
                    let ticks = r.varint()?;
                    Ok(([r.i16()?, r.i16()?, r.i16()?], ticks))
                })
                .collect::<Result<_>>()?,
        ))
    }
    fn steps(&self) -> usize {
        match self {
            Self::Linear(_) => 0,
            Self::Stepped(s) => s.len(),
        }
    }
    fn write(&self, w: &mut Writer) {
        match self {
            Self::Linear(d) => d.iter().for_each(|v| w.i16(*v)),
            Self::Stepped(steps) => {
                for (d, ticks) in steps {
                    w.varint(*ticks);
                    d.iter().for_each(|v| w.i16(*v));
                }
            }
        }
    }
}
/// ClientboundMoveEntityPacket.Pos / PosRot / Rot.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveEntity {
    pub id: i32,
    pub delta: Option<VecDelta>,
    pub rotation: Option<(i8, i8)>,
    pub on_ground: bool,
}
macro_rules! move_entity {
    ($name:ident, pos: $pos:expr, rot: $rot:expr) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub MoveEntity);
        impl Codec<$name> for $name {
            fn read(r: &mut Reader) -> Result<$name> {
                Self::read_inner(r).map($name)
            }
            fn write(v: &$name, w: &mut Writer) {
                Self::write_inner(&v.0, w)
            }
        }
        impl $name {
            fn read_inner(r: &mut Reader) -> Result<MoveEntity> {
                let id = r.varint()?;
                if !$pos {
                    let on_ground = r.bool()?;
                    let rotation = Some((r.i8()?, r.i8()?));
                    return Ok(MoveEntity {
                        id,
                        delta: None,
                        rotation,
                        on_ground,
                    });
                }
                let properties = r.varint()?;
                let delta = Some(VecDelta::read(r, (properties as u32 >> 1) as usize)?);
                let rotation = if $rot { Some((r.i8()?, r.i8()?)) } else { None };
                Ok(MoveEntity {
                    id,
                    delta,
                    rotation,
                    on_ground: properties & 1 != 0,
                })
            }
            fn write_inner(v: &MoveEntity, w: &mut Writer) {
                w.varint(v.id);
                if !$pos {
                    w.bool(v.on_ground);
                } else {
                    let delta = v.delta.clone().unwrap_or(VecDelta::Linear([0; 3]));
                    w.varint(i32::from(v.on_ground) | (delta.steps() as i32) << 1);
                    delta.write(w);
                }
                if let Some((y, x)) = v.rotation.filter(|_| $rot) {
                    w.i8(y);
                    w.i8(x);
                }
            }
        }
    };
}
move_entity!(MoveEntityPos, pos: true, rot: false);
move_entity!(MoveEntityPosRot, pos: true, rot: true);
move_entity!(MoveEntityRot, pos: false, rot: true);
packet!(OpenScreen { container: i32 => VarInt, menu: i32 => VarInt, title: Tag => Nbt });
// ClientboundMountScreenOpenPacket: a horse's or llama's inventory, with its
// chest's columns (`getInventoryColumns`).
packet!(MountScreenOpen { container: i32 => VarInt, columns: i32 => VarInt, entity: i32 => Int });
packet!(
    /// ClientboundOpenSignEditorPacket: the sign whose side the player edits.
    OpenSignEditor { pos: [i32; 3] => Pos, front: bool => SignSide }
);
packet!(
    /// ClientboundOpenBookPacket: the hand (InteractionHand, 0 main, 1 off)
    /// whose written book the player reads.
    OpenBook { hand: i32 => VarInt }
);
packet!(PlayerAbilities { flags: i8 => Byte, flying_speed: f32 => Float, walking_speed: f32 => Float });
/// MessageSignature.Packed: an index into the last-seen cache or a full signature.
pub struct PackedSignature;
impl Codec<(i32, Option<Vec<u8>>)> for PackedSignature {
    fn read(r: &mut Reader) -> Result<(i32, Option<Vec<u8>>)> {
        let id = r.varint()? - 1;
        Ok(if id == -1 {
            (-1, Some(r.bytes(256)?.to_vec()))
        } else {
            (id, None)
        })
    }
    fn write(v: &(i32, Option<Vec<u8>>), w: &mut Writer) {
        w.varint(v.0 + 1);
        if let Some(sig) = &v.1 {
            w.bytes(sig);
        }
    }
}
/// MessageSignature: 256 raw bytes.
pub struct Signature;
impl Codec<Vec<u8>> for Signature {
    fn read(r: &mut Reader) -> Result<Vec<u8>> {
        Ok(r.bytes(256)?.to_vec())
    }
    fn write(v: &Vec<u8>, w: &mut Writer) {
        w.bytes(v)
    }
}
/// FilterMask: 0 pass-through, 1 fully filtered, 2 partially (with a BitSet).
pub struct FilterMask;
impl Codec<(i32, Vec<i64>)> for FilterMask {
    fn read(r: &mut Reader) -> Result<(i32, Vec<i64>)> {
        let kind = r.varint()?;
        Ok((
            kind,
            if kind == 2 {
                r.long_array()?
            } else {
                Vec::new()
            },
        ))
    }
    fn write(v: &(i32, Vec<i64>), w: &mut Writer) {
        w.varint(v.0);
        if v.0 == 2 {
            w.long_array(&v.1);
        }
    }
}
packet!(PlayerChat {
    global_index: i32 => VarInt,
    sender: u128 => Uuid,
    index: i32 => VarInt,
    signature: Option<Vec<u8>> => Opt<Signature>,
    content: String => StrN<256>,
    timestamp: i64 => Long,
    salt: i64 => Long,
    last_seen: Vec<(i32, Option<Vec<u8>>)> => List<PackedSignature>,
    unsigned_content: Option<Tag> => Opt<Nbt>,
    filter: (i32, Vec<i64>) => FilterMask,
    chat_type: ChatTypeBound => ChatTypeBound,
});
packet!(
    /// ClientboundDeleteChatPacket: the server deleted a player message,
    /// named by its packed signature.
    DeleteChat { signature: (i32, Option<Vec<u8>>) => PackedSignature }
);
packet!(PlayerCombatKill { id: i32 => VarInt, message: Tag => Nbt });
packet!(PlayerInfoRemove { uuids: Vec<u128> => List<Uuid> });
/// One ClientboundPlayerInfoUpdatePacket entry; fields are set for the
/// actions in the packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerInfo {
    pub uuid: u128,
    pub name: Option<String>,
    pub properties: Vec<Property>,
    /// RemoteChatSession.Data: session id, key expiry, key, key signature.
    pub chat_session: Option<(u128, i64, Vec<u8>, Vec<u8>)>,
    pub game_mode: Option<i32>,
    pub listed: Option<bool>,
    pub latency: Option<i32>,
    pub display_name: Option<Option<Tag>>,
    pub list_order: Option<i32>,
    pub show_hat: Option<bool>,
}
/// Actions bits: ADD_PLAYER, INITIALIZE_CHAT, UPDATE_GAME_MODE, UPDATE_LISTED,
/// UPDATE_LATENCY, UPDATE_DISPLAY_NAME, UPDATE_LIST_ORDER, UPDATE_HAT.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfoUpdate {
    pub actions: u8,
    pub entries: Vec<PlayerInfo>,
}
impl Codec<PlayerInfoUpdate> for PlayerInfoUpdate {
    fn read(r: &mut Reader) -> Result<Self> {
        let actions = r.u8()?;
        let n = r.count(r.remaining())?;
        let mut entries = Vec::with_capacity(n.min(256));
        for _ in 0..n {
            let mut e = PlayerInfo {
                uuid: r.uuid()?,
                ..PlayerInfo::default()
            };
            if actions & 1 != 0 {
                e.name = Some(r.string(16)?);
                let count = r.count(16)?;
                e.properties = (0..count)
                    .map(|_| Property::read(r))
                    .collect::<Result<_>>()?;
            }
            if actions & 2 != 0 && r.bool()? {
                e.chat_session =
                    Some((r.uuid()?, r.i64()?, r.byte_array(512)?, r.byte_array(4096)?));
            }
            if actions & 4 != 0 {
                e.game_mode = Some(r.varint()?);
            }
            if actions & 8 != 0 {
                e.listed = Some(r.bool()?);
            }
            if actions & 16 != 0 {
                e.latency = Some(r.varint()?);
            }
            if actions & 32 != 0 {
                e.display_name = Some(if r.bool()? { Some(r.nbt()?) } else { None });
            }
            if actions & 64 != 0 {
                e.list_order = Some(r.varint()?);
            }
            if actions & 128 != 0 {
                e.show_hat = Some(r.bool()?);
            }
            entries.push(e);
        }
        Ok(Self { actions, entries })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.u8(v.actions);
        w.varint(v.entries.len() as i32);
        for e in &v.entries {
            w.uuid(e.uuid);
            if v.actions & 1 != 0 {
                w.string(e.name.as_deref().unwrap_or(""));
                w.varint(e.properties.len() as i32);
                e.properties.iter().for_each(|p| Property::write(p, w));
            }
            if v.actions & 2 != 0 {
                w.bool(e.chat_session.is_some());
                if let Some((id, expires, key, sig)) = &e.chat_session {
                    w.uuid(*id);
                    w.i64(*expires);
                    w.byte_array(key);
                    w.byte_array(sig);
                }
            }
            if v.actions & 4 != 0 {
                w.varint(e.game_mode.unwrap_or(0));
            }
            if v.actions & 8 != 0 {
                w.bool(e.listed.unwrap_or(false));
            }
            if v.actions & 16 != 0 {
                w.varint(e.latency.unwrap_or(0));
            }
            if v.actions & 32 != 0 {
                let name = e.display_name.clone().flatten();
                w.bool(name.is_some());
                if let Some(t) = &name {
                    w.nbt(t);
                }
            }
            if v.actions & 64 != 0 {
                w.varint(e.list_order.unwrap_or(0));
            }
            if v.actions & 128 != 0 {
                w.bool(e.show_hat.unwrap_or(false));
            }
        }
    }
}
packet!(PlayerPosition {
    teleport: i32 => VarInt,
    position: [f64; 3] => Vec3d,
    delta: [f64; 3] => Vec3d,
    yaw: f32 => Float,
    pitch: f32 => Float,
    relatives: i32 => Int,
});
packet!(
    /// ClientboundProjectilePowerPacket: an AbstractHurtingProjectile's accelerationPower.
    ProjectilePower { id: i32 => VarInt, power: f64 => Double }
);
packet!(
    /// ClientboundPlayerLookAtPacket's entity target: its id and the anchor
    /// (`EntityAnchorArgument.Anchor`: 0 feet, 1 eyes) to look at on it.
    LookAtEntity { id: i32 => VarInt, anchor: i32 => VarInt }
);
packet!(
    /// ClientboundPlayerLookAtPacket: turn this player's `from` anchor towards
    /// a point, or towards an entity when the client knows it.
    PlayerLookAt {
        from: i32 => VarInt,
        target: [f64; 3] => Vec3d,
        entity: Option<LookAtEntity> => Opt<LookAtEntity>,
    }
);
packet!(
    /// ClientboundPostEffectsPacket: the `post_effect` chains (ids) the player's
    /// screen runs, in order (`ServerPlayer.postEffects`).
    PostEffects { effects: Vec<String> => List<Ident> }
);
packet!(PlayerRotation { yaw: f32 => Float, relative_yaw: bool => Bool, pitch: f32 => Float, relative_pitch: bool => Bool });
packet!(RemoveEntities { ids: Vec<i32> => List<VarInt> });
packet!(Respawn { spawn: SpawnInfo => SpawnInfo, keep: i8 => Byte });
packet!(RotateHead { id: i32 => VarInt, head_yaw: i8 => Byte });
packet!(
    /// Each change is `state << 12 | x << 8 | z << 4 | y` within the section.
    SectionBlocksUpdate { section: [i32; 3] => SectionPos, changes: Vec<i64> => List<VarLong> }
);
packet!(Text { text: Tag => Nbt });
packet!(SetChunkCacheCenter { x: i32 => VarInt, z: i32 => VarInt });
packet!(
    /// ClientboundTickingStatePacket: the tick rate and whether the level is frozen.
    TickingState { rate: f32 => Float, frozen: bool => Bool }
);
packet!(
    /// ClientboundTickingStepPacket: the ticks a frozen level runs (`/tick step`).
    TickingStep { steps: i32 => VarInt }
);
packet!(Distance { distance: i32 => VarInt });
packet!(SetCursorItem { item: Option<Stack> => Slot });
packet!(SetDefaultSpawnPosition { pos: GlobalPos => GlobalPos, yaw: f32 => Float, pitch: f32 => Float });
packet!(SetEntityData { id: i32 => VarInt, values: Vec<(u8, i32, Value)> => EntityData });
packet!(SetEntityMotion { id: i32 => VarInt, velocity: [f64; 3] => LpVec3 });
packet!(
    /// ClientboundSetEntityLinkPacket: the entity holding a leashed entity's
    /// lead, 0 when none (Leashable.setDelayedLeashHolderId).
    SetEntityLink { source: i32 => Int, dest: i32 => Int }
);
packet!(
    /// ClientboundSetPassengersPacket: a vehicle and every entity riding it.
    SetPassengers { vehicle: i32 => VarInt, passengers: Vec<i32> => List<VarInt> }
);
packet!(
    /// ClientboundMoveVehiclePacket: the server puts a vehicle its rider
    /// controls back where it was (PositionAndRotation: x, y, z, yRot, xRot).
    MoveVehicle { position: [f64; 3] => Vec3d, yaw: f32 => Float, pitch: f32 => Float }
);
packet!(
    /// NewMinecartBehavior.MinecartStep: where a minecart is at the end of one
    /// of the server's steps along the track, its motion, its rotation (yRot,
    /// xRot) and the step's weight in the 3 ticks they are played over.
    MinecartStep {
        position: [f64; 3] => Vec3d,
        movement: [f64; 3] => Vec3d,
        yaw: i8 => Byte,
        pitch: i8 => Byte,
        weight: f32 => Float,
    }
);
packet!(
    /// ClientboundMoveMinecartPacket: the steps a minecart took along the
    /// track since the last one (the `minecart_improvements` experiment sends
    /// these in place of move packets).
    MoveMinecartAlongTrack { id: i32 => VarInt, steps: Vec<MinecartStep> => List<MinecartStep> }
);
/// ClientboundSetEquipmentPacket: (EquipmentSlot ordinal, stack) pairs.
pub struct Equipment;
impl Codec<Vec<(u8, Option<Stack>)>> for Equipment {
    fn read(r: &mut Reader) -> Result<Vec<(u8, Option<Stack>)>> {
        let mut out = Vec::new();
        loop {
            let slot = r.u8()?;
            out.push((slot & 127, Slot::read(r)?));
            if slot & 128 == 0 {
                return Ok(out);
            }
        }
    }
    fn write(v: &Vec<(u8, Option<Stack>)>, w: &mut Writer) {
        for (i, (slot, stack)) in v.iter().enumerate() {
            w.u8(if i + 1 < v.len() { slot | 128 } else { *slot });
            Slot::write(stack, w);
        }
    }
}
packet!(SetEquipment { id: i32 => VarInt, slots: Vec<(u8, Option<Stack>)> => Equipment });
packet!(SetExperience { progress: f32 => Float, level: i32 => VarInt, total: i32 => VarInt });
packet!(SetHealth { health: f32 => Float, food: i32 => VarInt, saturation: f32 => Float });
packet!(SetHeldSlot { slot: i32 => VarInt });
packet!(
    /// ClientboundSetCameraPacket: the entity the view is attached to (the
    /// player's own id puts it back on the player).
    SetCamera { camera_id: i32 => VarInt }
);
packet!(SetPlayerInventory { slot: i32 => VarInt, item: Option<Stack> => Slot });
packet!(
    /// ClockNetworkState for one world clock (registry id).
    ///
    /// `ClientboundSetTimePacket` keys its map by `Holder<WorldClock>`, but
    /// `WorldClock.STREAM_CODEC` is `ByteBufCodecs.holderRegistry`, not
    /// `holder`: the key is the plain registry id, with no inline form and no
    /// off-by-one. The test below is a join packet captured from a vanilla
    /// 26.3 server.
    ClockState { clock: i32 => VarInt, total_ticks: i64 => VarLong, partial_tick: f32 => Float, rate: f32 => Float }
);
packet!(SetTime { game_time: i64 => Long, clocks: Vec<ClockState> => List<ClockState> });
packet!(TitlesAnimation { fade_in: i32 => Int, stay: i32 => Int, fade_out: i32 => Int });
/// SoundEvent holder: a registry id, or an inline location with optional range.
pub struct SoundHolder;
impl Codec<(i32, Option<(String, Option<f32>)>)> for SoundHolder {
    fn read(r: &mut Reader) -> Result<(i32, Option<(String, Option<f32>)>)> {
        let id = r.varint()?;
        if id != 0 {
            return Ok((crate::adapter::ids::sound_event(id - 1), None));
        }
        let location = r.string(32767)?;
        let range = if r.bool()? { Some(r.f32()?) } else { None };
        Ok((-1, Some((location, range))))
    }
    fn write(v: &(i32, Option<(String, Option<f32>)>), w: &mut Writer) {
        match &v.1 {
            None => w.varint(v.0 + 1),
            Some((location, range)) => {
                w.varint(0);
                w.string(location);
                <Opt<Float> as Codec<Option<f32>>>::write(range, w);
            }
        }
    }
}
packet!(Sound {
    sound: (i32, Option<(String, Option<f32>)>) => SoundHolder,
    source: i32 => VarInt,
    x: i32 => Int,
    y: i32 => Int,
    z: i32 => Int,
    volume: f32 => Float,
    pitch: f32 => Float,
    seed: i64 => Long,
});
packet!(SystemChat { content: Tag => Nbt, overlay: bool => Bool });
packet!(
    /// ClientboundTagQueryPacket: the answer to a debug NBT query (F3+I), with
    /// the transaction it answers; `Tag::End` when there was no block entity.
    TagQuery { transaction: i32 => VarInt, tag: Tag => Nbt }
);
packet!(TakeItemEntity { item: i32 => VarInt, player: i32 => VarInt, amount: i32 => VarInt });
packet!(TeleportEntity {
    id: i32 => VarInt,
    position: [f64; 3] => Vec3d,
    delta: [f64; 3] => Vec3d,
    yaw: f32 => Float,
    pitch: f32 => Float,
    relatives: i32 => Int,
    on_ground: bool => Bool,
});

packet!(BlockEvent { pos: [i32; 3] => Pos, b0: u8 => UByte, b1: u8 => UByte, block: i32 => VarInt });
packet!(
    /// ClientboundInitializeBorderPacket.
    InitializeBorder {
        center_x: f64 => Double,
        center_z: f64 => Double,
        old_size: f64 => Double,
        new_size: f64 => Double,
        lerp_time: i64 => VarLong,
        absolute_max_size: i32 => VarInt,
        warning_blocks: i32 => VarInt,
        warning_time: i32 => VarInt,
    }
);
packet!(TabList { header: Tag => Nbt, footer: Tag => Nbt });
packet!(
    /// ClientboundSetBorderCenterPacket.
    SetBorderCenter { x: f64 => Double, z: f64 => Double }
);
packet!(
    /// ClientboundSetBorderLerpSizePacket.
    SetBorderLerpSize { old_size: f64 => Double, new_size: f64 => Double, lerp_time: i64 => VarLong }
);
packet!(
    /// ClientboundSetBorderSizePacket.
    SetBorderSize { size: f64 => Double }
);
packet!(
    /// ClientboundSetBorderWarningDelayPacket (ticks).
    SetBorderWarningDelay { delay: i32 => VarInt }
);
packet!(
    /// ClientboundSetBorderWarningDistancePacket.
    SetBorderWarningDistance { blocks: i32 => VarInt }
);
packet!(
    /// ClientboundSetDisplayObjectivePacket: `DisplaySlot` id (0 list,
    /// 1 sidebar, 2 below name, 3.. team sidebars) and the objective ("" clears).
    SetDisplayObjective { slot: i32 => VarInt, objective: String => Str }
);
packet!(
    /// ClientboundResetScorePacket: one objective, or all of them when absent.
    ResetScore { owner: String => Str, objective: Option<String> => Opt<Str> }
);
/// `NumberFormatTypes.STREAM_CODEC`, by `number_format_type` registry id.
#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    /// 0: `minecraft:blank`.
    Blank,
    /// 1: `minecraft:styled`, a style compound.
    Styled(Tag),
    /// 2: `minecraft:fixed`, a component.
    Fixed(Tag),
}
impl Codec<NumberFormat> for NumberFormat {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.varint()? {
            0 => NumberFormat::Blank,
            1 => NumberFormat::Styled(r.nbt()?),
            2 => NumberFormat::Fixed(r.nbt()?),
            other => return Err(crate::Error::new(format!("unknown number format {other}"))),
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        match v {
            NumberFormat::Blank => w.varint(0),
            NumberFormat::Styled(t) => {
                w.varint(1);
                w.nbt(t);
            }
            NumberFormat::Fixed(t) => {
                w.varint(2);
                w.nbt(t);
            }
        }
    }
}
packet!(
    /// ClientboundSetScorePacket.
    SetScore {
        owner: String => Str,
        objective: String => Str,
        score: i32 => VarInt,
        display: Option<Tag> => Opt<Nbt>,
        number_format: Option<NumberFormat> => Opt<NumberFormat>,
    }
);
/// ClientboundSetObjectivePacket: method 0 adds, 1 removes, 2 changes;
/// adds and changes carry the display name, render type (0 integer,
/// 1 hearts) and number format.
#[derive(Clone, Debug, PartialEq)]
pub struct SetObjective {
    pub name: String,
    pub method: i8,
    pub display: Option<(Tag, i32, Option<NumberFormat>)>,
}
impl Codec<SetObjective> for SetObjective {
    fn read(r: &mut Reader) -> Result<Self> {
        let name = r.string(32767)?;
        let method = r.i8()?;
        let display = if method == 0 || method == 2 {
            Some((r.nbt()?, r.varint()?, <Opt<NumberFormat>>::read(r)?))
        } else {
            None
        };
        Ok(Self {
            name,
            method,
            display,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.string(&v.name);
        w.i8(v.method);
        if v.method == 0 || v.method == 2 {
            let (name, render, format) = v.display.clone().unwrap_or((Tag::End, 0, None));
            w.nbt(&name);
            w.varint(render);
            <Opt<NumberFormat>>::write(&format, w);
        }
    }
}
/// ClientboundLevelParticlesPacket: a particle, whether it ignores the
/// client's particle setting (`overrideLimiter`) and its distance limit
/// (`alwaysShow`), where, the spread and the speed on each axis, how many
/// (0: one, moving by speed times spread) and the randomization type
/// (0 default: gaussian spread and speed; 1 ALTERNATIVE: nextDouble times
/// spread, the packet's max speed as velocity).
#[derive(Clone, Debug, PartialEq)]
pub struct LevelParticles {
    pub particle: Particle,
    pub override_limiter: bool,
    pub always_show: bool,
    pub pos: [f64; 3],
    pub dist: [f32; 3],
    pub max_speed: [f32; 3],
    pub count: i32,
    pub randomization: i32,
}
impl Codec<LevelParticles> for LevelParticles {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            particle: Particle::read(r)?,
            override_limiter: r.bool()?,
            always_show: r.bool()?,
            pos: Vec3d::read(r)?,
            dist: [r.f32()?, r.f32()?, r.f32()?],
            max_speed: [r.f32()?, r.f32()?, r.f32()?],
            count: r.varint()?,
            randomization: r.varint()?,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        v.particle.write(w);
        w.bool(v.override_limiter);
        w.bool(v.always_show);
        Vec3d::write(&v.pos, w);
        for f in v.dist.iter().chain(&v.max_speed) {
            w.f32(*f);
        }
        w.varint(v.count);
        w.varint(v.randomization);
    }
}
/// `ExplosionParticleInfo` in a `WeightedList`: a particle the blast
/// scatters, its scaling and speed, then its weight.
#[derive(Clone, Debug, PartialEq)]
pub struct ExplosionParticle {
    pub particle: Particle,
    pub scaling: f32,
    pub speed: f32,
    pub weight: i32,
}
/// ClientboundExplodePacket: the centre, radius and blocks destroyed, the
/// push the receiving player takes, the particle at the centre, the sound,
/// the particles the blast scatters, and whether the sound plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Explode {
    pub center: [f64; 3],
    pub radius: f32,
    pub block_count: i32,
    pub knockback: Option<[f64; 3]>,
    pub particle: Particle,
    pub sound: (i32, Option<(String, Option<f32>)>),
    pub block_particles: Vec<ExplosionParticle>,
    pub play_sound: bool,
}
impl Codec<Explode> for Explode {
    fn read(r: &mut Reader) -> Result<Self> {
        let center = Vec3d::read(r)?;
        let radius = r.f32()?;
        let block_count = r.i32()?;
        let knockback = <Opt<Vec3d>>::read(r)?;
        let particle = Particle::read(r)?;
        let sound = SoundHolder::read(r)?;
        let n = r.varint()?;
        let mut block_particles = Vec::new();
        for _ in 0..n {
            block_particles.push(ExplosionParticle {
                particle: Particle::read(r)?,
                scaling: r.f32()?,
                speed: r.f32()?,
                weight: r.varint()?,
            });
        }
        Ok(Self {
            center,
            radius,
            block_count,
            knockback,
            particle,
            sound,
            block_particles,
            play_sound: r.bool()?,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        Vec3d::write(&v.center, w);
        w.f32(v.radius);
        w.i32(v.block_count);
        <Opt<Vec3d>>::write(&v.knockback, w);
        v.particle.write(w);
        SoundHolder::write(&v.sound, w);
        w.varint(v.block_particles.len() as i32);
        for b in &v.block_particles {
            b.particle.write(w);
            w.f32(b.scaling);
            w.f32(b.speed);
            w.varint(b.weight);
        }
        w.bool(v.play_sound);
    }
}
/// `ClientboundSetPlayerTeamPacket.Parameters`.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamParameters {
    pub display_name: Tag,
    pub prefix: Tag,
    pub suffix: Tag,
    /// `Team.Visibility` id.
    pub name_tag_visibility: i32,
    /// `Team.CollisionRule` id.
    pub collision_rule: i32,
    /// `TeamColor` id (0 black .. 15 white), absent for no colour.
    pub color: Option<i32>,
    /// 1: friendly fire, 2: see friendly invisibles.
    pub options: i8,
}
/// ClientboundSetPlayerTeamPacket: method 0 adds (parameters and players),
/// 1 removes, 2 changes (parameters), 3 joins and 4 leaves (players).
#[derive(Clone, Debug, PartialEq)]
pub struct SetPlayerTeam {
    pub name: String,
    pub method: i8,
    pub parameters: Option<TeamParameters>,
    pub players: Vec<String>,
}
impl Codec<SetPlayerTeam> for SetPlayerTeam {
    fn read(r: &mut Reader) -> Result<Self> {
        let name = r.string(32767)?;
        let method = r.i8()?;
        let parameters = if method == 0 || method == 2 {
            Some(TeamParameters {
                display_name: r.nbt()?,
                prefix: r.nbt()?,
                suffix: r.nbt()?,
                name_tag_visibility: r.varint()?,
                collision_rule: r.varint()?,
                color: <Opt<VarInt>>::read(r)?,
                options: r.i8()?,
            })
        } else {
            None
        };
        let players = if matches!(method, 0 | 3 | 4) {
            <List<Str>>::read(r)?
        } else {
            Vec::new()
        };
        Ok(Self {
            name,
            method,
            parameters,
            players,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.string(&v.name);
        w.i8(v.method);
        if v.method == 0 || v.method == 2 {
            let p = v.parameters.clone().unwrap_or(TeamParameters {
                display_name: Tag::End,
                prefix: Tag::End,
                suffix: Tag::End,
                name_tag_visibility: 0,
                collision_rule: 0,
                color: None,
                options: 0,
            });
            w.nbt(&p.display_name);
            w.nbt(&p.prefix);
            w.nbt(&p.suffix);
            w.varint(p.name_tag_visibility);
            w.varint(p.collision_rule);
            <Opt<VarInt>>::write(&p.color, w);
            w.i8(p.options);
        }
        if matches!(v.method, 0 | 3 | 4) {
            <List<Str>>::write(&v.players, w);
        }
    }
}
packet!(ServerData { motd: Tag => Nbt, icon: Option<Vec<u8>> => Opt<crate::buf::Bytes> });
packet!(
    /// ClientboundUpdateMobEffectPacket; flags: 1 ambient, 2 visible, 4 icon, 8 blend.
    UpdateMobEffect { id: i32 => VarInt, effect: i32 => VarInt, amplifier: i32 => VarInt, duration: i32 => VarInt, flags: i8 => Byte }
);
packet!(RemoveMobEffect { id: i32 => VarInt, effect: i32 => VarInt });
packet!(
    /// AttributeModifier.STREAM_CODEC: id, amount and operation (0 add_value,
    /// 1 add_multiplied_base, 2 add_multiplied_total).
    AttributeModifier { id: String => Ident, amount: f64 => Double, operation: i32 => VarInt }
);
packet!(
    /// ClientboundUpdateAttributesPacket.AttributeSnapshot: the attribute's
    /// registry id, its base value and every modifier.
    AttributeSnapshot {
        attribute: i32 => VarInt,
        base: f64 => Double,
        modifiers: Vec<AttributeModifier> => List<AttributeModifier>,
    }
);
packet!(
    /// ClientboundUpdateAttributesPacket: an entity's synced attribute instances.
    UpdateAttributes { id: i32 => VarInt, values: Vec<AttributeSnapshot> => List<AttributeSnapshot> }
);
packet!(
    /// ClientboundSwingAnimationPacket: hand (0 main, 1 off) and the swing
    /// type (0 none, 1 whack, 2 stab) with its duration in ticks.
    SwingAnimation { id: i32 => VarInt, hand: i32 => VarInt, kind: i32 => VarInt, duration: i32 => VarInt }
);
packet!(SoundEntity {
    sound: (i32, Option<(String, Option<f32>)>) => SoundHolder,
    source: i32 => VarInt,
    id: i32 => VarInt,
    volume: f32 => Float,
    pitch: f32 => Float,
    seed: i64 => Long,
});

/// ClientboundStopSoundPacket: bit 0 selects a source, bit 1 an event id.
#[derive(Clone, Debug, PartialEq)]
pub struct StopSound {
    pub source: Option<i32>,
    pub sound: Option<String>,
}
impl Codec<StopSound> for StopSound {
    fn read(r: &mut Reader) -> crate::Result<Self> {
        let flags = r.u8()?;
        let source = if flags & 1 != 0 {
            let value = r.varint()?;
            if !(0..11).contains(&value) {
                return Err(crate::Error::new("invalid sound source"));
            }
            Some(value)
        } else {
            None
        };
        let sound = if flags & 2 != 0 {
            Some(r.string(32767)?)
        } else {
            None
        };
        Ok(Self { source, sound })
    }
    fn write(p: &Self, w: &mut Writer) {
        w.u8(u8::from(p.source.is_some()) | (u8::from(p.sound.is_some()) << 1));
        if let Some(source) = p.source {
            w.varint(source);
        }
        if let Some(sound) = &p.sound {
            w.string(sound);
        }
    }
}
/// `DisplayInfo` of an advancement as sent.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvancementDisplay {
    pub title: Tag,
    pub description: Tag,
    pub icon: crate::item::Stack,
    /// `AdvancementType` ordinal: task, challenge, goal.
    pub frame: i32,
    pub show_toast: bool,
    pub hidden: bool,
    pub background: Option<String>,
}
/// `ClientboundUpdateAdvancementsPacket.PositionedAdvancement`.
#[derive(Clone, Debug, PartialEq)]
pub struct Advancement {
    pub id: String,
    pub parent: Option<String>,
    pub display: Option<AdvancementDisplay>,
    pub requirements: Vec<Vec<String>>,
    pub telemetry: bool,
    pub x: f32,
    pub y: f32,
}
/// `ClientboundUpdateAdvancementsPacket`: whether it replaces what the
/// client has, the advancements added and removed, the progress of each
/// changed one (criterion, when obtained in epoch milliseconds), and whether
/// to show toasts.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateAdvancements {
    pub reset: bool,
    pub added: Vec<Advancement>,
    pub removed: Vec<String>,
    pub progress: Vec<(String, Vec<(String, Option<i64>)>)>,
    pub show: bool,
}
impl Codec<UpdateAdvancements> for UpdateAdvancements {
    fn read(r: &mut Reader) -> Result<Self> {
        let reset = r.bool()?;
        let n = r.count(r.remaining())?;
        let mut added = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let id = r.string(32767)?;
            let parent = if r.bool()? {
                Some(r.string(32767)?)
            } else {
                None
            };
            let display = if r.bool()? {
                let title = r.nbt()?;
                let description = r.nbt()?;
                let icon = crate::item::Template::read(r)?;
                let frame = r.varint()?;
                let flags = r.i32()?;
                let background = if flags & 1 != 0 {
                    Some(r.string(32767)?)
                } else {
                    None
                };
                Some(AdvancementDisplay {
                    title,
                    description,
                    icon,
                    frame,
                    show_toast: flags & 2 != 0,
                    hidden: flags & 4 != 0,
                    background,
                })
            } else {
                None
            };
            let groups = r.count(r.remaining())?;
            let mut requirements = Vec::with_capacity(groups.min(256));
            for _ in 0..groups {
                let k = r.count(r.remaining())?;
                requirements.push((0..k).map(|_| r.string(32767)).collect::<Result<_>>()?);
            }
            let telemetry = r.bool()?;
            let (x, y) = (r.f32()?, r.f32()?);
            added.push(Advancement {
                id,
                parent,
                display,
                requirements,
                telemetry,
                x,
                y,
            });
        }
        let n = r.count(r.remaining())?;
        let removed = (0..n).map(|_| r.string(32767)).collect::<Result<_>>()?;
        let n = r.count(r.remaining())?;
        let mut progress = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let id = r.string(32767)?;
            let k = r.count(r.remaining())?;
            let mut criteria = Vec::with_capacity(k.min(256));
            for _ in 0..k {
                let name = r.string(32767)?;
                let at = if r.bool()? { Some(r.i64()?) } else { None };
                criteria.push((name, at));
            }
            progress.push((id, criteria));
        }
        let show = r.bool()?;
        Ok(Self {
            reset,
            added,
            removed,
            progress,
            show,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        w.bool(v.reset);
        w.varint(v.added.len() as i32);
        for a in &v.added {
            w.string(&a.id);
            w.bool(a.parent.is_some());
            if let Some(p) = &a.parent {
                w.string(p);
            }
            w.bool(a.display.is_some());
            if let Some(d) = &a.display {
                w.nbt(&d.title);
                w.nbt(&d.description);
                crate::item::Template::write(&d.icon, w);
                w.varint(d.frame);
                w.i32(
                    i32::from(d.background.is_some())
                        | i32::from(d.show_toast) << 1
                        | i32::from(d.hidden) << 2,
                );
                if let Some(b) = &d.background {
                    w.string(b);
                }
            }
            w.varint(a.requirements.len() as i32);
            for g in &a.requirements {
                w.varint(g.len() as i32);
                for c in g {
                    w.string(c);
                }
            }
            w.bool(a.telemetry);
            w.f32(a.x);
            w.f32(a.y);
        }
        w.varint(v.removed.len() as i32);
        for id in &v.removed {
            w.string(id);
        }
        w.varint(v.progress.len() as i32);
        for (id, criteria) in &v.progress {
            w.string(id);
            w.varint(criteria.len() as i32);
            for (name, at) in criteria {
                w.string(name);
                w.bool(at.is_some());
                if let Some(at) = at {
                    w.i64(*at);
                }
            }
        }
        w.bool(v.show);
    }
}
packet!(
    /// ClientboundAwardStatsPacket: (stat type, value, count) by network id.
    AwardStats { stats: Vec<(i32, i32, i32)> => StatList }
);
/// `Object2IntMap<Stat<?>>`: each stat's type and value ids, then its count.
pub struct StatList;
impl Codec<Vec<(i32, i32, i32)>> for StatList {
    fn read(r: &mut Reader) -> Result<Vec<(i32, i32, i32)>> {
        let n = r.count(r.remaining())?;
        (0..n)
            .map(|_| Ok((r.varint()?, r.varint()?, r.varint()?)))
            .collect()
    }
    fn write(v: &Vec<(i32, i32, i32)>, w: &mut Writer) {
        w.varint(v.len() as i32);
        for &(a, b, c) in v {
            w.varint(a);
            w.varint(b);
            w.varint(c);
        }
    }
}

/// `ClientboundBossEventPacket` (`boss_event`).
#[derive(Clone, Debug, PartialEq)]
pub enum BossAction {
    Add {
        name: Tag,
        progress: f32,
        /// `BossBarColor` ordinal.
        color: i32,
        /// `BossBarOverlay` ordinal.
        overlay: i32,
        darken: bool,
        music: bool,
        fog: bool,
    },
    Remove,
    UpdateProgress {
        progress: f32,
    },
    UpdateName {
        name: Tag,
    },
    UpdateStyle {
        color: i32,
        overlay: i32,
    },
    UpdateProperties {
        darken: bool,
        music: bool,
        fog: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct BossEvent {
    pub id: u128,
    pub action: BossAction,
}
fn boss_flags(darken: bool, music: bool, fog: bool) -> u8 {
    u8::from(darken) | (u8::from(music) << 1) | (u8::from(fog) << 2)
}
fn split_flags(flags: u8) -> (bool, bool, bool) {
    (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0)
}
impl Codec<BossEvent> for BossEvent {
    fn read(r: &mut Reader) -> Result<BossEvent> {
        let id = r.uuid()?;
        let action = match r.varint()? {
            0 => {
                let name = r.nbt()?;
                let progress = r.f32()?;
                let color = r.varint()?;
                let overlay = r.varint()?;
                let (darken, music, fog) = split_flags(r.u8()?);
                BossAction::Add {
                    name,
                    progress,
                    color,
                    overlay,
                    darken,
                    music,
                    fog,
                }
            }
            1 => BossAction::Remove,
            2 => BossAction::UpdateProgress { progress: r.f32()? },
            3 => BossAction::UpdateName { name: r.nbt()? },
            4 => BossAction::UpdateStyle {
                color: r.varint()?,
                overlay: r.varint()?,
            },
            5 => {
                let (darken, music, fog) = split_flags(r.u8()?);
                BossAction::UpdateProperties { darken, music, fog }
            }
            op => return Err(Error::new(format!("unknown boss event operation {op}"))),
        };
        Ok(BossEvent { id, action })
    }
    fn write(v: &BossEvent, w: &mut Writer) {
        w.uuid(v.id);
        match &v.action {
            BossAction::Add {
                name,
                progress,
                color,
                overlay,
                darken,
                music,
                fog,
            } => {
                w.varint(0);
                w.nbt(name);
                w.f32(*progress);
                w.varint(*color);
                w.varint(*overlay);
                w.u8(boss_flags(*darken, *music, *fog));
            }
            BossAction::Remove => w.varint(1),
            BossAction::UpdateProgress { progress } => {
                w.varint(2);
                w.f32(*progress);
            }
            BossAction::UpdateName { name } => {
                w.varint(3);
                w.nbt(name);
            }
            BossAction::UpdateStyle { color, overlay } => {
                w.varint(4);
                w.varint(*color);
                w.varint(*overlay);
            }
            BossAction::UpdateProperties { darken, music, fog } => {
                w.varint(5);
                w.u8(boss_flags(*darken, *music, *fog));
            }
        }
    }
}

packet_enum!(Clientbound, crate::ids::play::clientbound {
    BundleDelimiter(Empty) = BUNDLE_DELIMITER,
    AddEntity(AddEntity) = ADD_ENTITY,
    Animate(Animate) = ANIMATE,
    AwardStats(AwardStats) = AWARD_STATS,
    BlockChangedAck(BlockChangedAck) = BLOCK_CHANGED_ACK,
    BlockDestruction(BlockDestruction) = BLOCK_DESTRUCTION,
    BlockEntityData(BlockEntityData) = BLOCK_ENTITY_DATA,
    BlockEvent(BlockEvent) = BLOCK_EVENT,
    BlockUpdate(BlockUpdate) = BLOCK_UPDATE,
    BossEvent(BossEvent) = BOSS_EVENT,
    ChangeDifficulty(ChangeDifficulty) = CHANGE_DIFFICULTY,
    ChunkBatchFinished(ChunkBatchFinished) = CHUNK_BATCH_FINISHED,
    ChunkBatchStart(Empty) = CHUNK_BATCH_START,
    ClearTitles(Bool1) = CLEAR_TITLES,
    LowDiskSpaceWarning(Empty) = LOW_DISK_SPACE_WARNING,
    Commands(Commands) = COMMANDS,
    RecipeBookAdd(super::recipes::RecipeBookAdd) = RECIPE_BOOK_ADD,
    RecipeBookRemove(super::recipes::RecipeBookRemove) = RECIPE_BOOK_REMOVE,
    RecipeBookSettings(super::recipes::RecipeBookSettings) = RECIPE_BOOK_SETTINGS,
    PlaceGhostRecipe(super::recipes::PlaceGhostRecipe) = PLACE_GHOST_RECIPE,
    ContainerClose(ContainerClose) = CONTAINER_CLOSE,
    ContainerSetContent(ContainerSetContent) = CONTAINER_SET_CONTENT,
    ContainerSetData(ContainerSetData) = CONTAINER_SET_DATA,
    ContainerSetSlot(ContainerSetSlot) = CONTAINER_SET_SLOT,
    CookieRequest(CookieRequest) = COOKIE_REQUEST,
    Cooldown(Cooldown) = COOLDOWN,
    CustomChatCompletions(CustomChatCompletions) = CUSTOM_CHAT_COMPLETIONS,
    CustomPayload(CustomPayload) = CUSTOM_PAYLOAD,
    Disconnect(Disconnect) = DISCONNECT,
    DeleteChat(DeleteChat) = DELETE_CHAT,
    DisguisedChat(DisguisedChat) = DISGUISED_CHAT,
    EntityEvent(EntityEvent) = ENTITY_EVENT,
    EntityPositionSync(EntityPositionSync) = ENTITY_POSITION_SYNC,
    ChunksBiomes(ChunksBiomes) = CHUNKS_BIOMES,
    ForgetLevelChunk(ForgetLevelChunk) = FORGET_LEVEL_CHUNK,
    GameEvent(GameEvent) = GAME_EVENT,
    GameTestHighlightPos(GameTestHighlightPos) = GAME_TEST_HIGHLIGHT_POS,
    GameRuleValues(GameRuleValues) = GAME_RULE_VALUES,
    HurtAnimation(HurtAnimation) = HURT_ANIMATION,
    DamageEvent(DamageEvent) = DAMAGE_EVENT,
    KeepAlive(KeepAlive) = KEEP_ALIVE,
    InitializeBorder(InitializeBorder) = INITIALIZE_BORDER,
    LevelChunkWithLight(LevelChunkWithLight) = LEVEL_CHUNK_WITH_LIGHT,
    LevelEvent(LevelEvent) = LEVEL_EVENT,
    LightUpdate(LightUpdate) = LIGHT_UPDATE,
    Login(Login) = LOGIN,
    MapItemData(MapItemData) = MAP_ITEM_DATA,
    MoveEntityPos(MoveEntityPos) = MOVE_ENTITY_POS,
    MoveEntityPosRot(MoveEntityPosRot) = MOVE_ENTITY_POS_ROT,
    MoveEntityRot(MoveEntityRot) = MOVE_ENTITY_ROT,
    MerchantOffers(MerchantOffers) = MERCHANT_OFFERS,
    MountScreenOpen(MountScreenOpen) = MOUNT_SCREEN_OPEN,
    OpenScreen(OpenScreen) = OPEN_SCREEN,
    OpenBook(OpenBook) = OPEN_BOOK,
    OpenSignEditor(OpenSignEditor) = OPEN_SIGN_EDITOR,
    Ping(Ping) = PING,
    PongResponse(crate::packets::status::Ping) = PONG_RESPONSE,
    PlayerAbilities(PlayerAbilities) = PLAYER_ABILITIES,
    PlayerChat(PlayerChat) = PLAYER_CHAT,
    PlayerCombatKill(PlayerCombatKill) = PLAYER_COMBAT_KILL,
    PlayerInfoRemove(PlayerInfoRemove) = PLAYER_INFO_REMOVE,
    PlayerInfoUpdate(PlayerInfoUpdate) = PLAYER_INFO_UPDATE,
    PlayerLookAt(PlayerLookAt) = PLAYER_LOOK_AT,
    PlayerPosition(PlayerPosition) = PLAYER_POSITION,
    PlayerRotation(PlayerRotation) = PLAYER_ROTATION,
    PostEffects(PostEffects) = POST_EFFECTS,
    RemoveEntities(RemoveEntities) = REMOVE_ENTITIES,
    RemoveMobEffect(RemoveMobEffect) = REMOVE_MOB_EFFECT,
    ResetScore(ResetScore) = RESET_SCORE,
    ResourcePackPop(ResourcePackPop) = RESOURCE_PACK_POP,
    ResourcePackPush(ResourcePackPush) = RESOURCE_PACK_PUSH,
    Respawn(Respawn) = RESPAWN,
    RotateHead(RotateHead) = ROTATE_HEAD,
    SectionBlocksUpdate(SectionBlocksUpdate) = SECTION_BLOCKS_UPDATE,
    ServerData(ServerData) = SERVER_DATA,
    SetActionBarText(Text) = SET_ACTION_BAR_TEXT,
    SetBorderCenter(SetBorderCenter) = SET_BORDER_CENTER,
    SetBorderLerpSize(SetBorderLerpSize) = SET_BORDER_LERP_SIZE,
    SetBorderSize(SetBorderSize) = SET_BORDER_SIZE,
    SetBorderWarningDelay(SetBorderWarningDelay) = SET_BORDER_WARNING_DELAY,
    SetBorderWarningDistance(SetBorderWarningDistance) = SET_BORDER_WARNING_DISTANCE,
    SetChunkCacheCenter(SetChunkCacheCenter) = SET_CHUNK_CACHE_CENTER,
    SetCamera(SetCamera) = SET_CAMERA,
    SetChunkCacheRadius(Distance) = SET_CHUNK_CACHE_RADIUS,
    SetCursorItem(SetCursorItem) = SET_CURSOR_ITEM,
    SetDefaultSpawnPosition(SetDefaultSpawnPosition) = SET_DEFAULT_SPAWN_POSITION,
    SetDisplayObjective(SetDisplayObjective) = SET_DISPLAY_OBJECTIVE,
    SetEntityData(SetEntityData) = SET_ENTITY_DATA,
    SetEntityMotion(SetEntityMotion) = SET_ENTITY_MOTION,
    SetEntityLink(SetEntityLink) = SET_ENTITY_LINK,
    SetEquipment(SetEquipment) = SET_EQUIPMENT,
    SetExperience(SetExperience) = SET_EXPERIENCE,
    SetHealth(SetHealth) = SET_HEALTH,
    SetHeldSlot(SetHeldSlot) = SET_HELD_SLOT,
    SetObjective(SetObjective) = SET_OBJECTIVE,
    SetPassengers(SetPassengers) = SET_PASSENGERS,
    MoveVehicle(MoveVehicle) = MOVE_VEHICLE,
    MoveMinecartAlongTrack(MoveMinecartAlongTrack) = MOVE_MINECART_ALONG_TRACK,
    ProjectilePower(ProjectilePower) = PROJECTILE_POWER,
    SetPlayerInventory(SetPlayerInventory) = SET_PLAYER_INVENTORY,
    SetPlayerTeam(SetPlayerTeam) = SET_PLAYER_TEAM,
    SetScore(SetScore) = SET_SCORE,
    SetSimulationDistance(Distance) = SET_SIMULATION_DISTANCE,
    SetSubtitleText(Text) = SET_SUBTITLE_TEXT,
    SetTime(SetTime) = SET_TIME,
    SetTitleText(Text) = SET_TITLE_TEXT,
    SetTitlesAnimation(TitlesAnimation) = SET_TITLES_ANIMATION,
    Sound(Sound) = SOUND,
    SoundEntity(SoundEntity) = SOUND_ENTITY,
    StopSound(StopSound) = STOP_SOUND,
    StartConfiguration(Empty) = START_CONFIGURATION,
    StoreCookie(StoreCookie) = STORE_COOKIE,
    // A common packet: the same `configuration::ServerLinks` the
    // configuration phase sends.
    ServerLinks(ServerLinks) = SERVER_LINKS,
    SelectAdvancementsTab(SelectAdvancementsTab) = SELECT_ADVANCEMENTS_TAB,
    SwingAnimation(SwingAnimation) = SWING_ANIMATION,
    SystemChat(SystemChat) = SYSTEM_CHAT,
    TabList(TabList) = TAB_LIST,
    TagQuery(TagQuery) = TAG_QUERY,
    TakeItemEntity(TakeItemEntity) = TAKE_ITEM_ENTITY,
    TeleportEntity(TeleportEntity) = TELEPORT_ENTITY,
    TestInstanceBlockStatus(TestInstanceBlockStatus) = TEST_INSTANCE_BLOCK_STATUS,
    TickingState(TickingState) = TICKING_STATE,
    TickingStep(TickingStep) = TICKING_STEP,
    Transfer(Transfer) = TRANSFER,
    UpdateAdvancements(UpdateAdvancements) = UPDATE_ADVANCEMENTS,
    UpdateAttributes(UpdateAttributes) = UPDATE_ATTRIBUTES,
    LevelParticles(LevelParticles) = LEVEL_PARTICLES,
    Explode(Explode) = EXPLODE,
    AddTransientBlock(AddTransientBlock) = ADD_TRANSIENT_BLOCK,
    UpdateMobEffect(UpdateMobEffect) = UPDATE_MOB_EFFECT,
    UpdateTags(UpdateTags) = UPDATE_TAGS,
    TrackedWaypoint(TrackedWaypoint) = WAYPOINT,
    ClearDialog(Empty) = CLEAR_DIALOG,
    ShowDialog(ShowDialog) = SHOW_DIALOG,
});
packet!(
    /// ClientboundClearTitlesPacket: whether to also reset the timings.
    Bool1 { value: bool => Bool }
);

/// ClientboundTrackedWaypointPacket.Operation, by ordinal.
pub mod waypoint_operation {
    pub const TRACK: i32 = 0;
    pub const UNTRACK: i32 = 1;
    pub const UPDATE: i32 = 2;
}

/// TrackedWaypoint's identifier: `Either<UUID, String>`.
#[derive(Clone, Debug, PartialEq)]
pub enum WaypointId {
    Uuid(u128),
    Name(String),
}

/// What a TrackedWaypoint points at (its `Type` and contents).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WaypointTarget {
    /// `TrackedWaypoint.empty`: an untracked waypoint.
    Empty,
    /// A block position (`Vec3iWaypoint`).
    Block([i32; 3]),
    /// A chunk (`ChunkWaypoint`).
    Chunk([i32; 2]),
    /// An angle in radians (`AzimuthWaypoint`).
    Azimuth(f32),
}

/// ClientboundTrackedWaypointPacket: the locator bar's waypoints.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackedWaypoint {
    /// `waypoint_operation`.
    pub operation: i32,
    pub id: WaypointId,
    /// Waypoint.Icon: the style (a `waypoint_style_asset` key) and an RGB colour.
    pub style: String,
    pub color: Option<i32>,
    pub target: WaypointTarget,
}

impl Codec<TrackedWaypoint> for TrackedWaypoint {
    fn read(r: &mut Reader) -> Result<TrackedWaypoint> {
        let operation = r.varint()?;
        if !(0..=2).contains(&operation) {
            return Err(Error::new(format!("waypoint operation {operation}")));
        }
        let id = if r.bool()? {
            WaypointId::Uuid(r.uuid()?)
        } else {
            WaypointId::Name(r.string(32767)?)
        };
        let style = Ident::read(r)?;
        // ByteBufCodecs.RGB_COLOR: three bytes, read back as an opaque ARGB colour.
        let color = if r.bool()? {
            let [red, green, blue] = [r.u8()?, r.u8()?, r.u8()?].map(i32::from);
            Some(-16_777_216 | red << 16 | green << 8 | blue)
        } else {
            None
        };
        let target = match r.varint()? {
            0 => WaypointTarget::Empty,
            1 => WaypointTarget::Block([r.varint()?, r.varint()?, r.varint()?]),
            2 => WaypointTarget::Chunk([r.varint()?, r.varint()?]),
            3 => WaypointTarget::Azimuth(r.f32()?),
            other => return Err(Error::new(format!("waypoint type {other}"))),
        };
        Ok(TrackedWaypoint {
            operation,
            id,
            style,
            color,
            target,
        })
    }
    fn write(v: &TrackedWaypoint, w: &mut Writer) {
        w.varint(v.operation);
        match &v.id {
            WaypointId::Uuid(uuid) => {
                w.bool(true);
                w.uuid(*uuid);
            }
            WaypointId::Name(name) => {
                w.bool(false);
                w.string(name);
            }
        }
        w.string(&v.style);
        w.bool(v.color.is_some());
        if let Some(color) = v.color {
            for shift in [16, 8, 0] {
                w.u8((color >> shift) as u8);
            }
        }
        match v.target {
            WaypointTarget::Empty => w.varint(0),
            WaypointTarget::Block([x, y, z]) => {
                w.varint(1);
                w.varint(x);
                w.varint(y);
                w.varint(z);
            }
            WaypointTarget::Chunk([x, z]) => {
                w.varint(2);
                w.varint(x);
                w.varint(z);
            }
            WaypointTarget::Azimuth(angle) => {
                w.varint(3);
                w.f32(angle);
            }
        }
    }
}

// ---- serverbound -------------------------------------------------------

packet!(AcceptTeleportation { id: i32 => VarInt, x: f64 => Double, y: f64 => Double, z: f64 => Double, yaw: f32 => Float, pitch: f32 => Float });
packet!(Attack { id: i32 => VarInt });
packet!(TeleportToEntity { uuid: u128 => Uuid });
packet!(
    /// ServerboundSpectatorActionPacket: the entity a spectator clicked, or
    /// none for a click on nothing.
    SpectatorAction { target: Option<i32> => OptVarInt }
);
packet!(
    /// GameType ids: 0 survival, 1 creative, 2 adventure, 3 spectator.
    ChangeGameMode { mode: i32 => VarInt }
);
packet!(LastSeenUpdate { offset: i32 => VarInt, acknowledged: [u8; 3] => Fixed3, checksum: i8 => Byte });
/// LastSeenMessages.Update's fixed 20-bit BitSet (3 bytes).
pub struct Fixed3;
impl Codec<[u8; 3]> for Fixed3 {
    fn read(r: &mut Reader) -> Result<[u8; 3]> {
        Ok(r.bytes(3)?.try_into().expect("3 bytes"))
    }
    fn write(v: &[u8; 3], w: &mut Writer) {
        w.bytes(v)
    }
}
packet!(Chat {
    message: String => StrN<256>,
    timestamp: i64 => Long,
    salt: i64 => Long,
    signature: Option<Vec<u8>> => Opt<Signature>,
    last_seen: LastSeenUpdate => LastSeenUpdate,
});
packet!(ChatCommand { command: String => Str });
packet!(ChatAck { offset: i32 => VarInt });
/// ArgumentSignatures: each signed argument's name (at most 16 chars) and
/// signature, at most 8.
pub struct ArgumentSignatures;
impl Codec<Vec<(String, Vec<u8>)>> for ArgumentSignatures {
    fn read(r: &mut Reader) -> Result<Vec<(String, Vec<u8>)>> {
        let n = r.count(8)?;
        (0..n)
            .map(|_| Ok((r.string(16)?, r.bytes(256)?.to_vec())))
            .collect()
    }
    fn write(v: &Vec<(String, Vec<u8>)>, w: &mut Writer) {
        w.varint(v.len() as i32);
        for (name, signature) in v {
            w.string(name);
            w.bytes(signature);
        }
    }
}
packet!(ChatCommandSigned {
    command: String => Str,
    timestamp: i64 => Long,
    salt: i64 => Long,
    arguments: Vec<(String, Vec<u8>)> => ArgumentSignatures,
    last_seen: LastSeenUpdate => LastSeenUpdate,
});
packet!(
    /// RemoteChatSession.Data: the session id, then ProfilePublicKey.Data
    /// (expiry in epoch milliseconds, X.509 key, the services' signature).
    ChatSessionUpdate {
        session: u128 => Uuid,
        expires_at: i64 => Long,
        key: Vec<u8> => BytesN<512>,
        key_signature: Vec<u8> => BytesN<4096>,
    }
);
packet!(ChangeDifficultyRequest { difficulty: i32 => VarInt });
packet!(LockDifficulty { locked: bool => Bool });
// ServerboundPlayerAbilitiesPacket: only whether the player flies (2).
packet!(PlayerAbilitiesRequest { flags: i8 => Byte });
packet!(SetGameRule { entries: Vec<GameRuleEntry> => List<GameRuleEntry> });
packet!(ChunkBatchReceived { chunks_per_tick: f32 => Float });
packet!(
    /// ServerboundClientCommandPacket.Action: 0 respawn, 1 request stats, 2 request game rules.
    ClientCommand { action: i32 => VarInt }
);
/// Changed slots: (slot, hashed stack) pairs.
pub struct ChangedSlots;
impl Codec<Vec<(i16, Option<Hashed>)>> for ChangedSlots {
    fn read(r: &mut Reader) -> Result<Vec<(i16, Option<Hashed>)>> {
        let n = r.count(128)?;
        (0..n)
            .map(|_| Ok((r.i16()?, HashedSlot::read(r)?)))
            .collect()
    }
    fn write(v: &Vec<(i16, Option<Hashed>)>, w: &mut Writer) {
        w.varint(v.len() as i32);
        for (slot, h) in v {
            w.i16(*slot);
            HashedSlot::write(h, w);
        }
    }
}
packet!(
    /// ContainerInput: 0 pickup, 1 quick move, 2 swap, 3 clone, 4 throw, 5 quick craft, 6 pickup all.
    ContainerClick {
        container: i32 => VarInt,
        state: i32 => VarInt,
        slot: i16 => Short,
        button: i8 => Byte,
        input: i32 => VarInt,
        changed: Vec<(i16, Option<Hashed>)> => ChangedSlots,
        carried: Option<Hashed> => HashedSlot,
    }
);
/// ServerboundSeenAdvancementsPacket: the tab the player opened (action 0, `OPENED_TAB`), or
/// that its screen closed (1, `CLOSED_SCREEN`, which carries no tab).
#[derive(Clone, Debug, PartialEq)]
pub struct SeenAdvancements {
    pub action: i32,
    pub tab: Option<String>,
}
impl Codec<SeenAdvancements> for SeenAdvancements {
    fn read(r: &mut Reader) -> Result<SeenAdvancements> {
        let action = r.varint()?;
        // The identifier is there for OPENED_TAB only (`readIdentifier` in the other branch).
        let tab = match action {
            0 => Some(r.string(32767)?),
            _ => None,
        };
        Ok(SeenAdvancements { action, tab })
    }
    fn write(v: &SeenAdvancements, w: &mut Writer) {
        w.varint(v.action);
        if v.action == 0 {
            w.string(v.tab.as_deref().unwrap_or_default());
        }
    }
}
packet!(SelectTrade { item: i32 => VarInt });
packet!(
    /// ServerboundBlockEntityTagQueryPacket: an operator's F3+I asks for the
    /// block entity's data at a position.
    BlockEntityTagQuery { transaction: i32 => VarInt, pos: [i32; 3] => Pos }
);
packet!(
    /// ServerboundEntityTagQueryPacket: an operator's F3+I asks for an
    /// entity's data, by its network id.
    EntityTagQuery { transaction: i32 => VarInt, entity: i32 => VarInt }
);
packet!(
    /// ServerboundContainerButtonClickPacket: `AbstractContainerMenu.clickMenuButton`
    /// (the loom's patterns, the stonecutter's recipes, the enchanting table's offers).
    ContainerButtonClick { container: i32 => VarInt, button: i32 => VarInt }
);
packet!(
    /// ServerboundContainerSlotStateChangedPacket: the crafter's slot toggle
    /// (`CrafterBlockEntity.setSlotState`).
    ContainerSlotStateChanged { slot: i32 => VarInt, container: i32 => VarInt, enabled: bool => Bool }
);
packet!(
    /// ServerboundRenameItemPacket: `AnvilMenu.setItemName`.
    RenameItem { name: String => Str }
);
packet!(
    /// ServerboundSignUpdatePacket: the four lines the player wrote on a side.
    SignUpdate { pos: [i32; 3] => Pos, lines: [String; 4] => SignLines, front: bool => SignSide }
);
packet!(
    /// ServerboundSetJigsawBlockPacket: the jigsaw block's edit screen. The
    /// joint is `JointType.getSerializedName`; an unknown one reads as aligned.
    SetJigsawBlock {
        pos: [i32; 3] => Pos,
        name: String => Ident,
        target: String => Ident,
        pool: String => Ident,
        final_state: String => Str,
        joint: String => Str,
        selection_priority: i32 => VarInt,
        placement_priority: i32 => VarInt
    }
);
packet!(
    /// ServerboundSetTestBlockPacket: the test block's edit screen; `mode` is
    /// a `TestBlockMode` id.
    SetTestBlock { pos: [i32; 3] => Pos, mode: i32 => VarInt, message: String => Str }
);
packet!(
    /// `TestInstanceBlockEntity.Data.STREAM_CODEC`: the test, size, rotation
    /// (ordinal), ignore-entities flag, status (index) and error message.
    TestInstanceData {
        test: Option<String> => Opt<Ident>,
        size: [i32; 3] => VarVec3i,
        rotation: i32 => VarInt,
        ignore_entities: bool => Bool,
        status: i32 => VarInt,
        error_message: Option<Tag> => Opt<Nbt>
    }
);
packet!(
    /// ServerboundTestInstanceBlockActionPacket: `action` is the `Action` id
    /// (INIT 0, QUERY 1, SET 2, RESET 3, SAVE 4, EXPORT 5, RUN 6).
    TestInstanceBlockAction { pos: [i32; 3] => Pos, action: i32 => VarInt, data: TestInstanceData => TestInstanceData }
);
packet!(
    /// ClientboundGameTestHighlightPosPacket: the absolute position and the
    /// position relative to the test's origin.
    GameTestHighlightPos { absolute: [i32; 3] => Pos, relative: [i32; 3] => Pos }
);
packet!(
    /// ClientboundTestInstanceBlockStatus: the test's description and, for a
    /// query, the size of its structure.
    TestInstanceBlockStatus { status: Tag => Nbt, size: Option<[i32; 3]> => Opt<VarVec3i> }
);
packet!(
    /// ServerboundJigsawGeneratePacket: the jigsaw block's Generate button.
    JigsawGenerate { pos: [i32; 3] => Pos, levels: i32 => VarInt, keep_jigsaws: bool => Bool }
);
packet!(
    /// ServerboundSetCommandBlockPacket: the command block's command, its
    /// mode (`CommandBlockEntity.Mode` ordinal) and the track output (1),
    /// conditional (2) and automatic (4) flags.
    SetCommandBlock { pos: [i32; 3] => Pos, command: String => Str, mode: i32 => VarInt, flags: i8 => Byte }
);
packet!(
    /// ServerboundSetCommandMinecartPacket: the command minecart's entity id,
    /// its command and whether it tracks output.
    SetCommandMinecart { entity: i32 => VarInt, command: String => Str, track_output: bool => Bool }
);
/// ServerboundSetStructureBlockPacket: the structure block's edit screen.
/// `update_type` and `mode` are `StructureBlockEntity.UpdateType` and
/// `StructureMode` ordinals, `mirror` and `rotation` the enums' ordinals.
#[derive(Clone, Debug, PartialEq)]
pub struct SetStructureBlock {
    pub pos: [i32; 3],
    pub update_type: i32,
    pub mode: i32,
    pub name: String,
    pub offset: [i32; 3],
    pub size: [i32; 3],
    pub mirror: i32,
    pub rotation: i32,
    pub data: String,
    pub ignore_entities: bool,
    pub strict: bool,
    pub show_air: bool,
    pub show_bounding_box: bool,
    pub integrity: f32,
    pub seed: i64,
}
impl SetStructureBlock {
    const FLAG_IGNORE_ENTITIES: i8 = 1;
    const FLAG_SHOW_AIR: i8 = 2;
    const FLAG_SHOW_BOUNDING_BOX: i8 = 4;
    const FLAG_STRICT: i8 = 8;
}
impl Codec<SetStructureBlock> for SetStructureBlock {
    /// `ServerboundSetStructureBlockPacket(FriendlyByteBuf)`: the offsets,
    /// sizes and integrity are clamped as they are read.
    fn read(r: &mut Reader) -> Result<Self> {
        let pos = Pos::read(r)?;
        let update_type = r.varint()?;
        let mode = r.varint()?;
        let name = r.string(32767)?;
        let mut offset = [0; 3];
        for v in &mut offset {
            *v = i32::from(r.i8()?).clamp(-48, 48);
        }
        let mut size = [0; 3];
        for v in &mut size {
            *v = i32::from(r.i8()?).clamp(0, 48);
        }
        let mirror = r.varint()?;
        let rotation = r.varint()?;
        let data = r.string(128)?;
        let integrity = r.f32()?.clamp(0.0, 1.0);
        let seed = r.varlong()?;
        let flags = r.i8()?;
        Ok(Self {
            pos,
            update_type,
            mode,
            name,
            offset,
            size,
            mirror,
            rotation,
            data,
            ignore_entities: flags & Self::FLAG_IGNORE_ENTITIES != 0,
            strict: flags & Self::FLAG_STRICT != 0,
            show_air: flags & Self::FLAG_SHOW_AIR != 0,
            show_bounding_box: flags & Self::FLAG_SHOW_BOUNDING_BOX != 0,
            integrity,
            seed,
        })
    }
    fn write(v: &Self, w: &mut Writer) {
        Pos::write(&v.pos, w);
        w.varint(v.update_type);
        w.varint(v.mode);
        w.string(&v.name);
        for o in v.offset {
            w.i8(o as i8);
        }
        for s in v.size {
            w.i8(s as i8);
        }
        w.varint(v.mirror);
        w.varint(v.rotation);
        w.string(&v.data);
        w.f32(v.integrity);
        w.varlong(v.seed);
        let mut flags = 0;
        if v.ignore_entities {
            flags |= Self::FLAG_IGNORE_ENTITIES;
        }
        if v.show_air {
            flags |= Self::FLAG_SHOW_AIR;
        }
        if v.show_bounding_box {
            flags |= Self::FLAG_SHOW_BOUNDING_BOX;
        }
        if v.strict {
            flags |= Self::FLAG_STRICT;
        }
        w.i8(flags);
    }
}
packet!(
    /// ServerboundEditBookPacket: the book's slot (a hotbar slot, or 40 for
    /// the off hand), its pages and, to sign it, its title.
    EditBook {
        slot: i32 => VarInt,
        pages: Vec<String> => BookPages,
        title: Option<String> => Opt<StrN<32>>,
    }
);
/// `stringUtf8(1024)` in `list(100)`: a book's pages.
pub struct BookPages;
impl Codec<Vec<String>> for BookPages {
    fn read(r: &mut Reader) -> Result<Vec<String>> {
        let n = r.varint()?;
        if !(0..=100).contains(&n) {
            return Err(Error::new(format!("{n} book pages, at most 100")));
        }
        (0..n).map(|_| r.string(1024)).collect()
    }
    fn write(v: &Vec<String>, w: &mut Writer) {
        List::<StrN<1024>>::write(v, w);
    }
}
/// SignTextSlot.STREAM_CODEC: VarInt 0 for the back, 1 for the front (any
/// other id reads as the back, ByIdMap's ZERO strategy).
pub struct SignSide;
impl Codec<bool> for SignSide {
    fn read(r: &mut Reader) -> Result<bool> {
        Ok(r.varint()? == 1)
    }
    fn write(v: &bool, w: &mut Writer) {
        w.varint(i32::from(*v));
    }
}
/// `stringUtf8(384)` in `fixedSizeList(4)`: four lines, no count.
pub struct SignLines;
impl Codec<[String; 4]> for SignLines {
    fn read(r: &mut Reader) -> Result<[String; 4]> {
        Ok([
            r.string(384)?,
            r.string(384)?,
            r.string(384)?,
            r.string(384)?,
        ])
    }
    fn write(v: &[String; 4], w: &mut Writer) {
        for line in v {
            w.string(line);
        }
    }
}
packet!(
    /// ServerboundSetBeaconPacket: `BeaconMenu.updateEffects`, each power an
    /// optional `mob_effect` id.
    SetBeacon { primary: Option<i32> => Opt<VarInt>, secondary: Option<i32> => Opt<VarInt> }
);
packet!(Interact { id: i32 => VarInt, hand: i32 => VarInt, location: [f64; 3] => LpVec3, secondary: bool => Bool });
packet!(
    /// ServerboundMovePlayerPacket flags: 1 on ground, 2 horizontal collision.
    MovePlayerPos { x: f64 => Double, y: f64 => Double, z: f64 => Double, flags: u8 => UByte }
);
packet!(MovePlayerPosRot { x: f64 => Double, y: f64 => Double, z: f64 => Double, yaw: f32 => Float, pitch: f32 => Float, flags: u8 => UByte });
packet!(MovePlayerRot { yaw: f32 => Float, pitch: f32 => Float, flags: u8 => UByte });
packet!(MovePlayerStatus { flags: u8 => UByte });
packet!(
    /// ServerboundMoveVehiclePacket: where the rider moved the vehicle it
    /// controls (PositionAndRotation, then onGround).
    MoveVehicleRequest { position: [f64; 3] => Vec3d, yaw: f32 => Float, pitch: f32 => Float, on_ground: bool => Bool }
);
packet!(
    /// ServerboundPaddleBoatPacket: the boat's left and right paddles.
    PaddleBoat { left: bool => Bool, right: bool => Bool }
);
packet!(PickItemFromBlock { pos: [i32; 3] => Pos, include_data: bool => Bool });
packet!(PickItemFromEntity { id: i32 => VarInt, include_data: bool => Bool });
packet!(
    /// ServerboundPlayerActionPacket.Action: 0 start destroy, 1 change destroy
    /// direction, 2 abort, 3 stop destroy, 4 drop all, 5 drop item, 6 release
    /// use item, 7 swap offhand, 8 stab. Direction is the 3D data value.
    PlayerAction { action: i32 => VarInt, pos: [i32; 3] => Pos, direction: u8 => UByte, sequence: i32 => VarInt }
);
packet!(
    /// ServerboundPlayerCommandPacket.Action: 0 stop sleeping, 1 start sprinting,
    /// 2 stop sprinting, 3/4 riding jump, 5 open inventory, 6 start fall flying.
    PlayerCommand { id: i32 => VarInt, action: i32 => VarInt, data: i32 => VarInt }
);
packet!(
    /// Input flags: forward 1, backward 2, left 4, right 8, jump 16, shift 32, sprint 64.
    PlayerInput { flags: u8 => UByte }
);
packet!(
    /// ServerboundSelectBundleItemPacket: the menu slot of a bundle and the
    /// index of its selected item (-1: none).
    SelectBundleItem { slot: i32 => VarInt, index: i32 => VarInt }
);
packet!(SetCarriedItem { slot: i16 => Short });
packet!(SetCreativeModeSlot { slot: i16 => Short, item: Option<Stack> => DelimitedSlot });
packet!(UseItem { hand: i32 => VarInt, sequence: i32 => VarInt, yaw: f32 => Float, pitch: f32 => Float });
packet!(UseItemOn {
    hand: i32 => VarInt,
    pos: [i32; 3] => Pos,
    face: i32 => VarInt,
    cursor: [f32; 3] => Cursor,
    inside: bool => Bool,
    world_border: bool => Bool,
    sequence: i32 => VarInt,
});
/// The click location relative to the block (three floats).
pub struct Cursor;
impl Codec<[f32; 3]> for Cursor {
    fn read(r: &mut Reader) -> Result<[f32; 3]> {
        Ok([r.f32()?, r.f32()?, r.f32()?])
    }
    fn write(v: &[f32; 3], w: &mut Writer) {
        v.iter().for_each(|f| w.f32(*f));
    }
}

packet_enum!(Serverbound, crate::ids::play::serverbound {
    ChangeDifficulty(ChangeDifficultyRequest) = CHANGE_DIFFICULTY,
    LockDifficulty(LockDifficulty) = LOCK_DIFFICULTY,
    SetGameRule(SetGameRule) = SET_GAME_RULE,
    AcceptTeleportation(AcceptTeleportation) = ACCEPT_TELEPORTATION,
    Attack(Attack) = ATTACK,
    BlockEntityTagQuery(BlockEntityTagQuery) = BLOCK_ENTITY_TAG_QUERY,
    EntityTagQuery(EntityTagQuery) = ENTITY_TAG_QUERY,
    TeleportToEntity(TeleportToEntity) = TELEPORT_TO_ENTITY,
    SpectatorAction(SpectatorAction) = SPECTATOR_ACTION,
    ChangeGameMode(ChangeGameMode) = CHANGE_GAME_MODE,
    Chat(Chat) = CHAT,
    ChatAck(ChatAck) = CHAT_ACK,
    ChatCommand(ChatCommand) = CHAT_COMMAND,
    ChatCommandSigned(ChatCommandSigned) = CHAT_COMMAND_SIGNED,
    ChatSessionUpdate(ChatSessionUpdate) = CHAT_SESSION_UPDATE,
    PlaceRecipe(super::recipes::PlaceRecipe) = PLACE_RECIPE,
    RecipeBookChangeSettings(super::recipes::RecipeBookChangeSettings) = RECIPE_BOOK_CHANGE_SETTINGS,
    RecipeBookSeenRecipe(super::recipes::RecipeBookSeenRecipe) = RECIPE_BOOK_SEEN_RECIPE,
    ChunkBatchReceived(ChunkBatchReceived) = CHUNK_BATCH_RECEIVED,
    ClientCommand(ClientCommand) = CLIENT_COMMAND,
    ClientInformation(ClientInformation) = CLIENT_INFORMATION,
    ClientTickEnd(Empty) = CLIENT_TICK_END,
    ConfigurationAcknowledged(Empty) = CONFIGURATION_ACKNOWLEDGED,
    ContainerButtonClick(ContainerButtonClick) = CONTAINER_BUTTON_CLICK,
    ContainerClick(ContainerClick) = CONTAINER_CLICK,
    ContainerClose(ContainerClose) = CONTAINER_CLOSE,
    CookieResponse(CookieResponse) = COOKIE_RESPONSE,
    CustomPayload(CustomPayload) = CUSTOM_PAYLOAD,
    CustomClickAction(CustomClickAction) = CUSTOM_CLICK_ACTION,
    EditBook(EditBook) = EDIT_BOOK,
    Interact(Interact) = INTERACT,
    KeepAlive(KeepAlive) = KEEP_ALIVE,
    MovePlayerPos(MovePlayerPos) = MOVE_PLAYER_POS,
    MovePlayerPosRot(MovePlayerPosRot) = MOVE_PLAYER_POS_ROT,
    MovePlayerRot(MovePlayerRot) = MOVE_PLAYER_ROT,
    MovePlayerStatus(MovePlayerStatus) = MOVE_PLAYER_STATUS_ONLY,
    MoveVehicle(MoveVehicleRequest) = MOVE_VEHICLE,
    PaddleBoat(PaddleBoat) = PADDLE_BOAT,
    PickItemFromBlock(PickItemFromBlock) = PICK_ITEM_FROM_BLOCK,
    PickItemFromEntity(PickItemFromEntity) = PICK_ITEM_FROM_ENTITY,
    PingRequest(crate::packets::status::Ping) = PING_REQUEST,
    PlayerAbilities(PlayerAbilitiesRequest) = PLAYER_ABILITIES,
    PlayerAction(PlayerAction) = PLAYER_ACTION,
    PlayerCommand(PlayerCommand) = PLAYER_COMMAND,
    PlayerInput(PlayerInput) = PLAYER_INPUT,
    PlayerLoaded(Empty) = PLAYER_LOADED,
    Pong(Ping) = PONG,
    RenameItem(RenameItem) = RENAME_ITEM,
    SeenAdvancements(SeenAdvancements) = SEEN_ADVANCEMENTS,
    ContainerSlotStateChanged(ContainerSlotStateChanged) = CONTAINER_SLOT_STATE_CHANGED,
    Punch(Empty) = PUNCH,
    JigsawGenerate(JigsawGenerate) = JIGSAW_GENERATE,
    ResourcePack(ResourcePackResponse) = RESOURCE_PACK,
    SelectBundleItem(SelectBundleItem) = BUNDLE_ITEM_SELECTED,
    SelectTrade(SelectTrade) = SELECT_TRADE,
    SetBeacon(SetBeacon) = SET_BEACON,
    SetCarriedItem(SetCarriedItem) = SET_CARRIED_ITEM,
    SetCommandBlock(SetCommandBlock) = SET_COMMAND_BLOCK,
    SetCommandMinecart(SetCommandMinecart) = SET_COMMAND_MINECART,
    SetCreativeModeSlot(SetCreativeModeSlot) = SET_CREATIVE_MODE_SLOT,
    SetJigsawBlock(SetJigsawBlock) = SET_JIGSAW_BLOCK,
    SetTestBlock(SetTestBlock) = SET_TEST_BLOCK,
    TestInstanceBlockAction(TestInstanceBlockAction) = TEST_INSTANCE_BLOCK_ACTION,
    SetStructureBlock(SetStructureBlock) = SET_STRUCTURE_BLOCK,
    SignUpdate(SignUpdate) = SIGN_UPDATE,
    UseItem(UseItem) = USE_ITEM,
    UseItemOn(UseItemOn) = USE_ITEM_ON,
});

#[cfg(test)]
mod tests;
