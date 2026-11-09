//! Trait mods implement `Hooks` and link into the game executable.
//! The executable passes the ordered `Mods` to `entry::main_with`.
//! Trait mods are process-wide, like mixin module statics.
//! Hooks use `&self + Send + Sync` for shared and reentrant calls; see `docs/native-mods.md`.
//! Cargo generates the trait and dispatch from the catalog and source signatures into OUT_DIR.
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

pub use crate::biome::{BiomeColors, SpawnEntry};
pub use crate::block_sprites::{BlockSprites, FaceSprite, SpriteRef};
pub use crate::block_states::{BlockStateFacts, BlockStates};
pub use crate::camera::{CameraEntity, FovPlayer};
pub use crate::client_resources::{ClientResources, TexturePixels};
pub use crate::command::Suggestion;
pub use crate::containers::{ModBlockEntity, ModContainer, ModEnergyBuffer, ModFluidTanks};
pub use crate::crafting::{CustomRecipe, Ingredient};
pub use crate::extras::Corner;
pub use crate::extras_hud::Corners;
pub use crate::first_person::Hand;
pub use crate::fluid_render::NativeFluidRenderOptions;
pub use crate::fog::{FogData, FogType};
pub use crate::game::NativeAttributeModifier;
pub use crate::game::NativeChunkSnapshot;
pub use crate::game::NativeEffect;
pub use crate::game::NativeEffectV2;
pub use crate::game::NativeFluidTank;
pub use crate::game::NativeInventoryError;
pub use crate::game::NativeInventoryKind;
pub use crate::game::native_block_entity_draw::{BlockEntityDraw, CubeFace};
pub use crate::game::native_client_services::{
    NativeClientEntity, NativeClientHead, NativeClientItem, NativeClientProfile, NativeClientState,
    NativeScreenKind, NativeWorldId,
};
pub use crate::game::native_text_field::NativeTextField;
pub use crate::game::{
    AdvancementDisplay, ItemEnergyBuffer, NativeBiomeId, NativeBlockEdit, NativeBlockReadError,
    NativeBlockRun, NativeClientBiomes, NativeClientChunks, NativeClock, NativeColumnBlocks,
    NativeComponentError, NativeConfig, NativeConfigFile, NativeConfigKind, NativeCriterion,
    NativeDataMapType, NativeExperienceOrb, NativeFalling, NativeHeightmap, NativeHeldItem,
    NativeHit, NativeItemEntity, NativeMapDecoration, NativeMapInfo, NativeMobRender,
    NativeModEvent, NativePlayer, NativePlayerRender, NativeReloadListener, NativeShake,
    NativeStructureBlock, NativeStructureTemplate, NativeWorldDraw,
};
pub use crate::gamerules::Value as GameRuleValue;
/// What a mod asks of the terrain's picture each frame (`native_world_hold`). API 414.
pub use crate::gfx::WorldHold;
pub use crate::gfx::native_render::{
    BlockSpriteArrays, RenderHooks, RenderNeeds, RenderNeedsEvent, WorldDrawn, WorldDrawnEvent,
    WorldWarp, WorldWarpEvent, wgpu,
};
pub use crate::hud::HudBatch;
pub use crate::hud_layout::{GuiDraw, HealthHud, HealthInputs};
pub use crate::item_data::StackData;
pub use crate::items::ItemStack;
pub use crate::merchant::MerchantOffer;
pub use crate::mesher::TintSource;
pub use crate::mobs::goals::{Cx, Goal, GoalSelector};
pub use crate::mobs::model_layers::{
    CubeDefinition, CubeDeformation, CubeListBuilder, LayerDefinition, MeshDefinition, ModelPart,
    ModelPolygon, ModelVertex, PartId, PartPose,
};
pub use crate::mobs::{Mob, MobWorld};
pub use crate::music::{Music, MusicLayer};
pub use crate::particles::{ParticleOptions, ParticleType};
pub use crate::recipe_book::RecipeBookCategory;
pub use crate::sound_engine::SoundPlayback;
/// A named step of the server tick and its wall and CPU milliseconds (`native_server_tick_timed`).
pub use crate::spans::Span as TickSpan;
pub use crate::terrain_model::{
    NativeModelMaterial, NativeModelRenderOptions, NativeModelShade, NativeModelUvShift,
};
pub use crate::text::Text;
pub use crate::world_primitives::{PrimitiveShape, WorldPrimitive, WorldPrimitives};
pub use horizon_assets::item_model::{DisplayContext, StackView};
pub use horizon_core::{
    geometry::Direction,
    position::BlockPos,
    random::{LegacyRandom, RandomSource},
};
/// A firework explosion (the `firework_explosion` component), for
/// `native_client_fireworks`.
pub use horizon_items::firework::{FireworkExplosion, FireworkShape};
/// A synched entity data value (`ClientboundSetEntityDataPacket`'s), as `entity_data_to` hands
/// them over.
pub use horizon_protocol::entity_data::Value as EntityDataValue;
/// The server's play packets `client_packet` hands over, decoded.
pub use horizon_protocol::packets::play::Clientbound as ClientboundPlayPacket;
/// The play packets a client sends, as `native_send_packet` takes them.
pub use horizon_protocol::packets::play::Serverbound as ServerboundPlayPacket;
pub use horizon_world::consumable::UseAnimation;
/// Vanilla's `TagParser`: the tag an SNBT text holds and the bytes it read
/// (`ItemStack::from_nbt` reads an item from it). API @API:item_from_snbt@.
pub use horizon_world::nbt::parse_snbt;
pub use horizon_world::player_inventory::EquipmentSlot;
pub use horizon_world::{
    attachments::{
        AttachmentCodec, AttachmentError, AttachmentHost, AttachmentType, Attachments, Holder,
    },
    block_state_properties::BlockProperties,
    fluid::{FluidKind, FluidState},
    level::{BlockId, Dimension as LevelDimension, PlacementContext},
    loot::{LootStack, NativeLootContext},
    nbt::{Compound, Tag},
    registry::BlockStateId,
    worldgen::{
        native_generate::{NativeGenerate, NativeLodColumn, NativeProtoChunk},
        structure::{Mirror, Rotation},
    },
};
pub use serde_json;
use serde_json::{Map, Value};
/// The slot range a menu's input slots take (`native_open_menu_with_input`).
/// API @API:menu_input@.
pub use std::ops::Range;
use std::time::Duration;
/// The window events `input_key` hands over (`winit::event::KeyEvent`) and their key types.
pub use winit;
pub use winit::keyboard::KeyCode;

/// An owned fluid payload. Component additions and removals are preserved exactly;
/// the amount is in millibuckets. Empty/negative stacks are not transferable.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFluidStack {
    pub fluid: String,
    pub amount: i32,
    pub components: Compound,
}

/// A tank exposed by a block's fluid capability, in provider order.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFluidTankView {
    pub tank: usize,
    pub content: Option<NativeFluidStack>,
    pub capacity: i32,
    pub can_fill: bool,
    pub can_drain: bool,
}

/// Simulation is read-only, including dirty flags, synchronization and RNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeFluidAction {
    Simulate,
    Execute,
}

/// Maximum-only drain, or a drain matching the full selected fluid payload.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum NativeFluidDrain {
    Any(i32),
    Exact(NativeFluidStack),
}

/// Camera values from the most recently extracted world frame.
#[derive(Clone, Debug)]
pub struct NativeCamera {
    pub position: [f64; 3],
    pub eye_height: f32,
    pub partial_tick: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_degrees: f32,
    pub near: f32,
    pub far: f32,
    /// Camera-relative view matrix in column-major order.
    pub view: [f32; 16],
    /// Reversed-Z projection matrix in column-major order.
    pub projection: [f32; 16],
    pub dimension: String,
}

/// A dispenser hook's answer: the slot's remainder (None empties it) and the
/// level event to play, `CLICK` (1000), `FAIL` (1001) or 0 for none.
pub type NativeDispensed = (Option<ItemStack>, i32);

/// What a mod's fake player uses its item on (`Host::use_as_player`), as a
/// player's click would.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeUseTarget {
    /// `ServerPlayerGameMode.useItemOn`: the block at `pos`, clicked on its
    /// `face` at `point` (world coordinates on that face).
    Block {
        pos: [i32; 3],
        face: Direction,
        point: [f64; 3],
    },
    /// `ServerPlayerGameMode.useItem`: the item alone, in the air.
    Item,
    /// `Player.interactOn`: the entity with this id, clicked at `location`
    /// (from the entity's position), as an interact packet names one.
    Entity { id: i32, location: [f64; 3] },
}

/// What a mod's fake player did (`Host::use_as_player`).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NativeFakeUse {
    /// Whether the use did something (`InteractionResult.consumesAction`).
    pub consumed: bool,
    /// What its hand holds afterwards: what is left of the stack, or what the
    /// item became (a filled bucket).
    pub held: Option<ItemStack>,
    /// What else ended up in its inventory (a bowl of stew from a stack of
    /// bowls, a bottle), for the mod to keep or drop.
    pub other: Vec<ItemStack>,
}

/// One tick of a bot's keys for `native_simulate_player_move`, as a client's input is
/// (`ClientInput`, `LocalPlayer.aiStep`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativePlayerInput {
    /// Forward (1) or back (-1); in between is a stick's partial push.
    pub forward: f32,
    /// Left (1) or right (-1), as vanilla's `leftImpulse`.
    pub strafe: f32,
    pub jump: bool,
    pub sneak: bool,
    /// Sprint is held; it sprints only while moving forward.
    pub sprint: bool,
    /// Where the body faces, in degrees (`Entity.getYRot`, `getXRot`).
    pub yaw: f32,
    pub pitch: f32,
    /// The velocity to start from (the last call's answer), in blocks a tick; None takes the
    /// server's copy of the player.
    pub velocity: Option<[f64; 3]>,
}

/// Where a bot's tick took it (`native_simulate_player_move`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NativeSimulatedMove {
    pub position: [f64; 3],
    /// The velocity after the tick, to pass back as the next tick's `velocity`.
    pub velocity: [f64; 3],
    pub on_ground: bool,
    /// It ran into a wall (`Entity.horizontalCollision`).
    pub horizontal_collision: bool,
    /// Whether it sprints after the tick (a wall or sneaking stops it).
    pub sprinting: bool,
}

/// Context for a player tuning a note block.
#[derive(Clone, Copy, Debug)]
pub struct NativeNoteBlockChange<'a> {
    pub dimension: LevelDimension,
    pub pos: BlockPos,
    pub instrument: &'a str,
    /// The note before tuning, in the range 0 through 24.
    pub old_note: i32,
}

/// What dropped the items heard by the `loot_drops` hook.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct NativeLootDrops<'a> {
    /// The block's or mob's own loot table (`minecraft:blocks/stone`, `minecraft:entities/zombie`);
    /// empty when none rolled: the block or mob has none, the `mob_drops` rule is off, a baby
    /// animal. A mob's drops may hold more: a charged creeper's head, a fox's mouth item.
    pub table: &'a str,
    /// The level they drop in (`minecraft:overworld`).
    pub dimension: &'a str,
    /// `LootContextParams.ORIGIN`: a broken block's centre, a mob's position.
    pub origin: [f64; 3],
    /// The block broken: its position and state.
    pub block: Option<([i32; 3], BlockStateId)>,
    /// The mob that died, or the entity a player's use dropped them from (the
    /// sheep sheared, the item frame broken): its entity ID and type
    /// (`minecraft:zombie`).
    pub entity: Option<(u32, &'a str)>,
    /// The player who broke the block or killed the mob, when known. A mob
    /// names one only while it remembers the player's hit (100 ticks), so one
    /// that dies to lava long after names none.
    pub player: Option<u128>,
    /// What broke the block (`LootContextParams.TOOL`).
    pub tool: Option<&'a ItemStack>,
    /// The radius of the explosion that broke the block.
    pub explosion: Option<f32>,
    /// A player's use dropped them rather than a break or a death: shears on a
    /// sheep, a mooshroom, a snow golem or a bogged, a brush on an armadillo or
    /// a suspicious block, shears carving a pumpkin, picking sweet berries or
    /// cave vines, a hit taking an item frame or its item, or untying a lead
    /// or cutting it with shears. `player` names them; `block` or `entity` says
    /// what dropped them. API 100. A mob that a player killed while
    /// leashed drops its lead on its next tick as a call of its own (`entity`
    /// the mob, `player` its killer, `table` empty, `used` false). API
    /// 102. A dying player's inventory is a call of its own too, after
    /// vanishing curse items are gone and before they are thrown (`entity`
    /// the player's id and `minecraft:player`, `player` them, `table` empty,
    /// `origin` where the items leave, `used` false); `drops` empty means
    /// the mod took the whole inventory. Not heard with keepInventory. API
    /// 405.
    pub used: bool,
}

/// The open screen, for the screen hooks (`native_screen_render_post`, the
/// mouse ones `native_screen_mouse_button_pressed_pre`, `..._released_pre` and
/// `native_screen_mouse_scrolled_pre`, and the key ones
/// `native_screen_key_pressed_pre` and `native_screen_char_typed_pre`): what
/// it is, its menu and title, where its container panel is, how many slots its
/// menu has, and where the mouse and the frame's partial tick are.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct NativeScreenView<'a> {
    /// One id per `Screen` (`Screen::id`): `inventory`, `creative`, `paused`,
    /// `stats`, `options`, `win`… Stable across releases, so a mod can match
    /// on it. Every container, station and mod window draws on the inventory
    /// screen and so reports `inventory`: tell them apart by `menu` and
    /// `title` (both None on a window the client opened itself) and by
    /// `slots`. The title's own screens report theirs (API 326): `title`,
    /// `world_select`, `world_create`, `world_create_flat`, `world_edit`,
    /// `multiplayer`, `add_server`, `direct_connect`…, with no menu, no title
    /// and no slots.
    pub id: &'static str,
    /// The type id of the menu the server opened for this screen
    /// (`minecraft:generic_9x3`, `minecraft:furnace`, a mod's own id; a mount's
    /// inventory is `horizon:mount<n>`). None on a screen without one (the
    /// client's own inventory, creative, the pause menu, stats…) and for a
    /// menu type the client does not know.
    pub menu: Option<&'a str>,
    /// The title the server gave the open menu, if it has one: the component
    /// itself, not a copy (`.plain()` it to draw, `chars()` for the styles).
    /// None when the server gave none, on a screen the client opened, and
    /// while playing.
    pub title: Option<&'a Text>,
    /// The screen's container panel in GUI pixels, `[x, y, width, height]`;
    /// the whole window for a screen with none. The panel is where the game
    /// draws the window: 176 wide for most, wider for the merchant and the
    /// beacon, a mod screen's own size for a mod's.
    pub rect: [f32; 4],
    /// The slots the open menu has, as `Game::slots` counts them: the player
    /// inventory's 36, then the container's, the station's, the mod screen's
    /// or the crafting grid's (a screen with no menu of its own still counts
    /// the inventory menu's). Counted, not listed.
    pub slots: usize,
    /// The pointer in GUI pixels (`Screen.mouseX`, `mouseY`, from the
    /// window's top left), as `native_cursor` gives it: for hover highlights.
    pub mouse: [f32; 2],
    /// This frame's partial tick (`getGameTimeDeltaPartialTick(false)`, 0 to
    /// 1): what the screen draws animation against.
    pub partial_tick: f32,
}

/// A depth-tested outline of the client's block selection shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeBlockRim {
    pub pos: [i32; 3],
    pub argb: u32,
    pub width: f32,
}

/// A camera-local visual light, with no gameplay light or shadow writes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeColoredLight {
    pub at: [f64; 3],
    pub rgb: u32,
    pub radius: f32,
    pub intensity: f32,
}

/// The minimap a native mod shows (`native_minimap`), drawn by Horizon's
/// minimap: its corner, its side in GUI pixels, blocks per map pixel,
/// turning with the player or north up, round or square.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeMinimap {
    pub corner: Corner,
    pub size: f32,
    pub blocks_per_pixel: f32,
    pub rotate: bool,
    pub round: bool,
}

/// One block of a scene a mod draws in a screen (`Host::draw_scene`): its
/// place in the scene, its state, and the block entity drawn with it, if any
/// (a mod's, through `native_block_entity_submit` as in the world).
#[derive(Clone, Copy, Debug)]
pub struct NativeSceneBlock<'a> {
    pub pos: [i32; 3],
    pub state: BlockStateId,
    pub entity: Option<&'a ModBlockEntity>,
}

/// How a scene in a screen is seen (`Host::draw_scene`): the point at the
/// middle of the screen's box, in blocks; the turn about the scene's vertical
/// axis and the tilt of its top towards the viewer, in degrees; and how many
/// GUI pixels a block is across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeSceneView {
    pub center: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub scale: f32,
}

/// A mod block's screen over its menu (`native_screen`): its size, where
/// each of the container's slots is (`Slot.x`, `Slot.y`; none hides the
/// slot, and so do slots past the list; the server's menu does not have a
/// hidden slot either, so clicks and shift-clicks skip it), and the player inventory's first
/// row (vanilla's is 84; the hotbar is 58 below it) and left column, from
/// its top left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeScreen {
    /// Vanilla's container screens are 176 wide.
    pub width: f32,
    pub height: f32,
    pub slots: Vec<Option<[f32; 2]>>,
    pub inventory_y: f32,
    /// The inventory's left column (vanilla's is 8).
    pub inventory_x: f32,
    /// The mod draws the title and the "Inventory" label itself
    /// (`native_screen_draw`); Horizon leaves them out.
    pub own_labels: bool,
    /// The mod draws its own background (`native_screen_draw`), so Horizon
    /// leaves out the plain panel and the slot frames.
    pub own_background: bool,
}

/// A native mod as Options > Mods lists it (`native_mods`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeMod {
    /// Its id: its settings folder and its keys' lines.
    pub id: String,
    /// `Name 1.0`.
    pub title: String,
    pub description: String,
    /// Its keys: name, label and default key (`key.keyboard.z`). The page
    /// lists them first; `native_mod_key` reads the player's choice.
    pub keys: Vec<[String; 3]>,
}

/// An item the panel shows (`native_panel_items`): its id
/// (`minecraft:diamond`), a count (1 draws none) and its components beyond
/// the item's own, in vanilla's item syntax, or "".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeStack {
    pub item: String,
    pub count: u32,
    pub components: String,
}

/// What a mod asks the recipe viewer to do with a stack
/// (`native_show_recipes`, heard as `native_recipe_lookup`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeRecipeLookup {
    /// The recipes that make it (JEI's R, a left click).
    Recipes,
    /// The recipes it is used in (JEI's U, a right click).
    Uses,
    /// A key (vanilla's name, `key.keyboard.a`) went down while the pointer
    /// was on the stack in a screen the mod draws: the viewer acts on it as
    /// over its own list (its recipe, uses and bookmark keys, as the player
    /// bound them).
    Key(String),
}

/// A category of a mod's own recipes for recipe viewers
/// (`native_recipe_categories`): what a fishing spot, a sieve or a machine
/// turns its inputs into, each output with its odds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeRecipeCategory {
    /// Its id (`mymod:sifting`).
    pub id: String,
    /// Its name, as the mod words it in the player's language.
    pub title: String,
    /// One short line under the name, or "".
    pub subtitle: String,
    /// The tab's icon.
    pub icon: NativeStack,
    /// The items and blocks that do it (a rod, a sieve), as JEI's catalysts.
    pub catalysts: Vec<NativeStack>,
    pub recipes: Vec<NativeCategoryRecipe>,
}

/// One recipe of a [`NativeRecipeCategory`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeCategoryRecipe {
    /// Its input slots, each with the stacks it takes; none for a recipe
    /// with no input (a catch).
    pub inputs: Vec<Vec<NativeStack>>,
    pub outputs: Vec<NativeChanceOutput>,
    /// Lines about the whole recipe.
    pub notes: Vec<String>,
}

/// An output of a [`NativeCategoryRecipe`] that comes with a chance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeChanceOutput {
    pub stack: NativeStack,
    /// The chance it comes, 0 to 1, each time the recipe runs (a catch, a
    /// block broken).
    pub chance: f32,
    /// How many come when it does.
    pub min: u32,
    pub max: u32,
    /// Lines about this output (`needs a brush`).
    pub notes: Vec<String>,
}

/// What the player did on a native mod's panel (`native_panel_event`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum NativePanelEvent {
    /// A click on an item of the panel, by its index in
    /// `native_panel_items`' list.
    Clicked { index: u32, right: bool },
    /// The search box's text, each time the player changes it.
    Search(String),
    /// A key (`key.keyboard.r`; compare it with `native_mod_key`) went down
    /// while the pointer was on an item: a slot of the screen's, or the
    /// panel's.
    Key { key: String, stack: NativeStack },
    /// A click on a slot of a recipe the panel shows: the recipe's index in
    /// `native_panel_recipes`' list and the slot's in it (the results come
    /// after the slots), and the stack it showed then.
    SlotClicked {
        recipe: u32,
        slot: u32,
        stack: NativeStack,
        right: bool,
    },
    /// The action button of the recipe at this index.
    Action(u32),
    /// The player turned the recipes' page: the index of the first recipe
    /// now shown.
    Page(u32),
    /// The recipe page's back button.
    Back,
    /// The player picked the tab at this index.
    Tab(u32),
}

/// The zoom a native mod asks for (`native_zoom`): the factor (1 is none), eased through the
/// field of view like the spyglass when `smooth`, and the scale of mouse turning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeZoom {
    pub factor: f32,
    pub smooth: bool,
    pub turn: f32,
}

/// A boss bar a server mod shows one player (`native_boss_bar`), as Bukkit's
/// `BossBar` holds it. API @API:boss_bar@.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeBossBar {
    pub name: Text,
    /// How full the bar is, 0 to 1.
    pub progress: f32,
    /// `BossBarColor`'s ordinal: 0 pink, 1 blue, 2 red, 3 green, 4 yellow,
    /// 5 purple, 6 white.
    pub color: i32,
    /// `BossBarOverlay`'s ordinal: 0 progress (no notches), 1 six notches,
    /// 2 ten, 3 twelve, 4 twenty.
    pub overlay: i32,
    /// `BossEvent`'s flags: the sky darkens, boss music plays, fog thickens.
    pub darken: bool,
    pub music: bool,
    pub fog: bool,
}

/// One open level of a server, counted for a diagnostics mod (`native_level_stats`): Bukkit's
/// `World.getLoadedChunks`, `getForceLoadedChunks`, `getEntities` and `Chunk.getTileEntities`
/// in numbers. API @API:level_stats@.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NativeLevelStats {
    /// The chunks it holds loaded.
    pub chunks: usize,
    /// The chunks `/forceload` and mods' tickets (`native_force_chunk`) keep loaded.
    pub forced_chunks: usize,
    pub block_entities: usize,
    /// The block entities that tick (`Level.blockEntityTickers`).
    pub ticking_block_entities: usize,
    /// Its entities by type (`minecraft:zombie`), players included, the most first and ties
    /// by name.
    pub entities: Vec<(String, usize)>,
    /// Its block entities by type (`minecraft:chest`, else a mod block's name), in the same
    /// order.
    pub block_entity_types: Vec<(String, usize)>,
}

/// What one mob type cost one level's ticks while a mod sampled them
/// (`native_take_mob_costs`). API @API:server_tick_timed@.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NativeMobCost {
    /// The level, `minecraft:overworld`.
    pub dimension: String,
    /// The mob type, `minecraft:zombie`.
    pub kind: String,
    /// The time its mobs' own ticks took (their riders' inside), in nanoseconds.
    pub nanos: u64,
    /// The mob ticks timed.
    pub mob_ticks: u64,
}

/// Where a fishing bobber outside water floats and fishes, as a mod's
/// `native_fishing_medium` answers it: lava, the void under an island, a
/// cloud. Below `surface` the bobber floats and fishes as in still water,
/// above it it falls as anywhere else. API @API:fishing_medium@.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeFishingMedium {
    /// The height the bobber floats at (world Y), as water's surface is.
    pub surface: f64,
    /// The loot table a catch from here rolls (`mymod:gameplay/lava_fishing`);
    /// None rolls vanilla's `minecraft:gameplay/fishing`.
    pub loot_table: Option<String>,
    /// The ticks a fish takes to come, drawn from `min..=max` when the bobber
    /// starts waiting, less the rod's Lure as in water (vanilla draws 100 to
    /// 600). None keeps vanilla's draw.
    pub lure_ticks: Option<(i32, i32)>,
}

/// A player on the server, as a server hook sees them.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NativeServerPlayer {
    pub uuid: u128,
    pub name: String,
    /// The operator level, 0 to 4 (`ops.json`; single player's owner with
    /// cheats on is 4).
    pub permission: u8,
    /// `minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`.
    pub dimension: String,
    pub position: [f64; 3],
    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub game_mode: i32,
    /// Health points, 20 at full; 0 once dead. API 21.
    pub health: f32,
    /// Where they look, in degrees (`yRot`, `xRot`), as their last move told
    /// the server. API 49.
    pub yaw: f32,
    pub pitch: f32,
    /// The network entity id the player has this session, the one hooks that only know entity ids
    /// (`living_incoming_damage`, `living_knockback`) give for the player; 0 when not known.
    /// API 398.
    pub entity_id: i32,
    /// The IP address the player connects from (behind a Velocity proxy: the address the proxy forwards);
    /// empty when not known. API 407.
    pub address: String,
    /// The connection's round-trip time in milliseconds, as the keep-alive measured it and the
    /// player list shows it (`ServerCommonPacketListenerImpl.latency`). The fields below are API
    /// @API:server_player_state@; 0, false or None where the server does not hold them.
    pub latency: i32,
    /// The protocol version the client connected with (the server's own: others are refused).
    pub protocol: i32,
    /// The experience level and the progress to the next one, 0 to 1.
    pub xp_level: i32,
    pub xp_progress: f32,
    /// On the ground, as the player's last accepted move said.
    pub on_ground: bool,
    /// The player's abilities as the client is told them (`Abilities.flying`, `mayfly`,
    /// `invulnerable`, `walkingSpeed`, `flyingSpeed`): speeds 0.1 and 0.05 by default.
    pub flying: bool,
    pub may_fly: bool,
    pub invulnerable: bool,
    pub walk_speed: f32,
    pub fly_speed: f32,
    /// Holding sneak (`Player.isShiftKeyDown`).
    pub sneaking: bool,
    /// The eyes' height above the feet in the current pose (`Entity.getEyeHeight`): 1.62
    /// standing, 1.27 crouching, 0.4 swimming or gliding, 0.2 asleep.
    pub eye_height: f32,
    /// The selected hotbar slot, 0 to 8.
    pub selected_slot: u8,
    /// The name a server mod shows for the player (`native_set_nickname`), if one does.
    pub nickname: Option<String>,
    /// The level a player needs to see this one, when a server mod hid them
    /// (`native_hide_player`).
    pub hidden: Option<u8>,
    /// Whether the player is using an item (`LivingEntity.isUsingItem`), and blocking with a
    /// shield (using one). API @API:server_player_state@.
    pub using_item: bool,
    pub blocking: bool,
    /// Sprinting (`Player.isSprinting`) and gliding with an elytra (`isFallFlying`).
    /// API @API:server_player_state@.
    pub sprinting: bool,
    pub gliding: bool,
}

/// What a player's client told the server about itself, and what the server sent it
/// (`native_player_client`). API @API:player_client@.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NativeClientInfo {
    /// The client's brand (`minecraft:brand`): `vanilla`, `fabric`, `horizon`...
    pub brand: String,
    /// `ClientInformation`: the language (`en_us`), the view distance it asks for, chat
    /// visibility (0 full, 1 system, 2 hidden), main hand (0 left, 1 right) and particles
    /// (0 all, 1 decreased, 2 minimal).
    pub language: String,
    pub view_distance: i8,
    pub chat_visibility: i32,
    pub main_hand: i32,
    pub particle_status: i32,
    /// The chunks the server has sent it and holds as sent.
    pub sent_chunks: usize,
    /// The bytes received from and sent to it on the wire, where the connection counts them.
    pub wire_bytes: Option<(u64, u64)>,
}

/// A command a native mod adds, as one usage: words, then arguments
/// `<name:kind>`, optional ones last in brackets `[name:kind]`:
/// `pay <player:player> <amount:integer>`, `balance [player:player]`,
/// `eco give <player:player> <amount:integer>`. Kinds: `word`, `text` (the
/// rest of the line, last), `integer`, `number`, `boolean`, `player`,
/// `item`, `block_pos`; and the kinds whose completions the player's client
/// asks the server for (`native_tab_complete_v2`): `word?`, `text?` and
/// `player_name` (a word, any name accepted, that the server completes with
/// the online players' names). Usages of one command share their words; the
/// server merges them into one tree.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeCommand {
    pub usage: String,
    /// The operator level a player needs to see and run it (0: everyone).
    pub level: u8,
    /// The permission node that decides instead, when set
    /// ([`NativeCommand::permission`]). API @API:command_permission@.
    pub permission: Option<String>,
    /// Whether the usage replaces vanilla's command of its name
    /// ([`NativeCommand::replace_vanilla`]).
    pub replace_vanilla: bool,
}

impl NativeCommand {
    /// A usage every player may run.
    pub fn new(usage: &str) -> Self {
        Self {
            usage: usage.to_owned(),
            level: 0,
            permission: None,
            replace_vanilla: false,
        }
    }
    /// The same, for operators of `level` and above.
    pub fn op(usage: &str, level: u8) -> Self {
        Self {
            usage: usage.to_owned(),
            level: level.min(4),
            permission: None,
            replace_vanilla: false,
        }
    }
    /// The same, for the players who have the permission node `node`
    /// (Bukkit's `plugin.yml` `permission:`): the server asks
    /// `native_has_permission(player, node, level)` before it shows or runs
    /// the usage, so a permissions mod grants or refuses it whatever the
    /// player's level, and without one the level decides as before. Each
    /// usage has its own node: a command's root is shown when any of its
    /// usages is allowed, and a line runs as the usage its words match.
    /// API @API:command_permission@.
    pub fn permission(mut self, node: &str) -> Self {
        self.permission = Some(node.to_owned());
        self
    }
    /// The same, replacing vanilla's command of this name (Bukkit's plugin
    /// command over a vanilla one, as Tab's `/bossbar`): vanilla's tree of
    /// the name is left out of the players' trees, and their lines of it go
    /// to the mods, which then answer every usage of it. Without it, a usage
    /// of a vanilla name is left out.
    pub fn replace_vanilla(mut self, on: bool) -> Self {
        self.replace_vanilla = on;
        self
    }
}

/// What a command a mod ran did (`native_run_command_captured`): whether it
/// ran without failing, its result and the lines it answered with, in order.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NativeCommandResult {
    pub ran: bool,
    /// Vanilla's `CommandResultCallback` value: the sum of the successful
    /// results (one for a plain command, one per fork of `execute as @a run`),
    /// 0 when it failed (`execute if entity` of nobody). A mod's command gives
    /// 1 when a mod ran it.
    pub result: i32,
    pub output: Vec<Text>,
}

/// A command of the server (`native_command_info`): vanilla's or one a mod
/// registered.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeCommandInfo {
    /// Its own name, an alias resolved (`teleport` for `tp`).
    pub name: String,
    /// The other names that run it (`tp` for `teleport`).
    pub aliases: Vec<String>,
    /// Its usages: vanilla's one line, as `/help` gives it, or each usage a
    /// mod registered.
    pub usages: Vec<String>,
    /// The operator level a player needs to run it (0: everyone).
    pub level: u8,
    /// Whether vanilla has it; false for a mod's.
    pub vanilla: bool,
}

/// Which `Game` observes the phase; a mirror `Game` never fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeAuthority {
    LocalAuthoritative,
    IntegratedServer,
    DedicatedServer,
    RemoteClient,
}

/// One open attempt of one world; it rejects a previous open's commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeWorldEpoch(u64);

impl NativeWorldEpoch {
    /// Create an epoch for the game.
    #[doc(hidden)]
    pub fn new(n: u64) -> Self {
        Self(n)
    }
    /// Read the epoch number.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// An entity of the open world, for the `Host` methods that read or change one
/// (`entity.position(host)`). It holds the world's epoch and the entity's id: ids
/// are shared by every level and never reused while the world is open, and an
/// entity that changes dimension gets a new one, as in vanilla. A handle from an
/// earlier open, of an entity that is gone, or used on a client reads as `None`
/// and changes nothing. `Host::entity` makes one from an event's entity id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityRef {
    epoch: NativeWorldEpoch,
    id: u32,
}

impl EntityRef {
    /// Create a handle for the game.
    #[doc(hidden)]
    pub fn new(epoch: NativeWorldEpoch, id: u32) -> Self {
        Self { epoch, id }
    }
    /// The entity's id (the network id events carry).
    pub fn id(self) -> u32 {
        self.id
    }
    /// The open of the world the handle belongs to.
    pub fn epoch(self) -> NativeWorldEpoch {
        self.epoch
    }
}

/// A living entity of the open world: a mob, a player, an armor stand or the
/// dragon (vanilla's `LivingEntity`). `host.living(entity)` makes one from an
/// [`EntityRef`] of such an entity, and [`LivingRef::entity`] goes back, so the
/// entity's methods work on it too. Stale handles read as `None`, as an
/// `EntityRef`'s do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LivingRef(EntityRef);

impl LivingRef {
    /// Create a handle for the game.
    #[doc(hidden)]
    pub fn new(entity: EntityRef) -> Self {
        Self(entity)
    }
    /// The same entity as an [`EntityRef`].
    pub fn entity(self) -> EntityRef {
        self.0
    }
    /// The entity's id (the network id events carry).
    pub fn id(self) -> u32 {
        self.0.id()
    }
}

impl From<LivingRef> for EntityRef {
    fn from(living: LivingRef) -> Self {
        living.0
    }
}

/// A level of the open world, by its dimension (`minecraft:the_nether`).
/// `Host::level` makes one, and `entity.level(host)` gives an entity's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LevelRef {
    epoch: NativeWorldEpoch,
    dimension: &'static str,
}

impl LevelRef {
    /// Create a handle for the game.
    #[doc(hidden)]
    pub fn new(epoch: NativeWorldEpoch, dimension: &'static str) -> Self {
        Self { epoch, dimension }
    }
    /// The level's dimension (`minecraft:overworld`).
    pub fn dimension(self) -> &'static str {
        self.dimension
    }
    /// The open of the world the handle belongs to.
    pub fn epoch(self) -> NativeWorldEpoch {
        self.epoch
    }
}

/// Non-paused authoritative server ticks since open (not game or frame time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeTickId(u64);

impl NativeTickId {
    /// Create a tick ID for the game.
    #[doc(hidden)]
    pub fn new(n: u64) -> Self {
        Self(n)
    }
    /// Read the tick number.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// The world phases (`native_world_lifecycle`'s `phase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// The phase names are the hook log's stable vocabulary (the Debug text
/// the mixins write); keep them despite the shared prefix.
#[allow(clippy::enum_variant_names)]
#[non_exhaustive]
pub enum WorldPhase {
    WorldOpenReady,
    /// The open failed; `error_text` is the engine's own error text.
    WorldOpenFailed,
    WorldCloseFinal,
    /// The server loop or final save failed; `error_text` carries the error.
    WorldCloseFailed,
}

/// The tick phases carry whether the world runs, including frozen steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerTickPhase {
    ServerTickStart { world_running: bool },
    ServerTickEnd { world_running: bool },
}

/// Context for every world lifecycle phase.
/// `authority` is the server role. LocalAuthoritative and RemoteClient never
/// fire today. `epoch` identifies the open attempt, including failed opens;
/// the same epoch identifies its ready, final-close, or failed-close event.
/// `tick` counts non-paused server ticks since the world opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeLifecycleContext {
    pub authority: NativeAuthority,
    pub epoch: NativeWorldEpoch,
    pub tick: NativeTickId,
}

/// The ordered list of mods shared by the client and server.
pub type Mods = Arc<[Box<dyn Hooks>]>;

pub(crate) static MODS: OnceLock<Mods> = OnceLock::new();

/// The process-wide list in dispatch order, or an empty list before installation.
pub fn mods() -> &'static [Box<dyn Hooks>] {
    MODS.get().map_or(&[], |mods| mods.as_ref())
}

/// The bundles' `[mod] id`s, one a [`mods`] entry in the same order.
static MOD_IDS: OnceLock<&'static [&'static str]> = OnceLock::new();

/// Names the entries of [`mods`] by their bundles' `[mod] id`s, so a panic in a hook names its mod.
/// The launcher's generated executable calls it before `entry::main_with`.
pub fn name_mods(ids: &'static [&'static str]) {
    if MOD_IDS.set(ids).is_err() {
        log::warn!("Trait mods already named; ignoring the second list");
        return;
    }
    let _ = OFF.set(ids.iter().map(|_| AtomicBool::new(false)).collect());
}

/// One flag per entry of [`mods`]: the player turned that mod off on the
/// Mods page of the options. Its hooks are skipped from the next dispatch on (its `RenderHooks`,
/// installed apart at start, keep drawing until a restart); it stays loaded,
/// and what it keeps stays, so turning it on again resumes it at once
/// (docs/design/mod-hotload.md). Registration hooks run at start, before the
/// game reads the player's choice, so every mod's content stays registered.
static OFF: OnceLock<Box<[AtomicBool]>> = OnceLock::new();

/// Turns the trait mod named `id` ([`name_mods`]) on or off for the
/// following dispatches. False when no mod has that id: a mixin-only mod,
/// whose code checks `native_on` itself.
pub fn set_mod_on(id: &str, on: bool) -> bool {
    let (Some(ids), Some(flags)) = (MOD_IDS.get(), OFF.get()) else {
        return false;
    };
    let mut found = false;
    for (flag, _) in flags.iter().zip(ids.iter()).filter(|(_, m)| **m == id) {
        flag.store(!on, Ordering::Relaxed);
        found = true;
    }
    found
}

/// The ids [`name_mods`] gave the entries of [`mods`].
pub fn mod_ids() -> &'static [&'static str] {
    MOD_IDS.get().copied().unwrap_or(&[])
}

/// Whether entry `index` of [`mods`] is turned off.
fn is_off(index: usize) -> bool {
    OFF.get()
        .and_then(|flags| flags.get(index))
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// `mods` in order, each marked as running `hook` until the next one is (or the loop ends), so a
/// panic in it names the mod. The generated dispatch loops over it.
/// A part of [`mods`] (one mod, dispatched on its own) keeps its entries' places in it.
fn entered<'a>(mods: &'a [Box<dyn Hooks>], hook: &'static str) -> Entered<'a> {
    Entered {
        first: place_in(self::mods(), mods),
        mods: mods.iter().enumerate(),
        hook,
        guard: None,
    }
}

/// Where `part` starts in `all`, None when it is not a part of it (a test's
/// own list).
fn place_in<T>(all: &[T], part: &[T]) -> Option<usize> {
    let range = all.as_ptr_range();
    range
        .contains(&part.as_ptr())
        .then(|| (part.as_ptr().addr() - range.start.addr()) / size_of::<T>().max(1))
}

struct Entered<'a> {
    /// Where `mods` starts in [`mods`]; None for a list of its own.
    first: Option<usize>,
    mods: std::iter::Enumerate<std::slice::Iter<'a, Box<dyn Hooks>>>,
    hook: &'static str,
    guard: Option<crate::crash::NativeModGuard>,
}

impl<'a> Iterator for Entered<'a> {
    type Item = &'a Box<dyn Hooks>;

    fn next(&mut self) -> Option<Self::Item> {
        // The last mod's mark goes first, so the next one's puts back what was there before.
        drop(self.guard.take());
        let (index, hooks) = loop {
            let (index, hooks) = self.mods.next()?;
            match self.first {
                Some(first) if is_off(first + index) => continue,
                Some(first) => break (first + index, hooks),
                None => break (index, hooks),
            }
        };
        self.guard = Some(crate::crash::enter_native_mod(
            crate::crash::NativeModCall {
                id: MOD_IDS.get().and_then(|ids| ids.get(index)).copied(),
                index,
                hook: self.hook,
            },
        ));
        Some(hooks)
    }
}

/// Whether a mod claims a cancellable hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Later mods and the game continue.
    Continue,
    /// Set the hook flag and skip later mods.
    Cancel,
}

mod generated_host {
    include!(concat!(env!("OUT_DIR"), "/mod_api_generated_host.rs"));
}
pub use generated_host::Host;

pub use crate::game::native_client_services::HudMode;

/// Result of accessing or decoding an attachment.
pub type AttachmentResult<T> = std::result::Result<T, AttachmentError>;

impl AttachmentHost for dyn Host + '_ {
    fn attachments(&mut self, holder: Holder) -> Result<&mut Attachments, AttachmentError> {
        Host::attachments(self, holder)
    }
}

/// Read-only level services, like vanilla's `LevelReader`.
pub trait LevelHost {
    /// Read a block through `LevelAccess::block_state`.
    fn block_state(&self, pos: [i32; 3]) -> BlockStateId;
    /// `Level.isClientSide`, through `LevelAccess::is_client_side`.
    fn is_client_side(&self) -> bool;
    /// `BlockState.setValue`, through `native_state_with_property`; None if invalid.
    fn state_with_property(
        &self,
        state: BlockStateId,
        key: &str,
        value: &str,
    ) -> Option<BlockStateId>;
}

/// Access vanilla's `Level.random`, so draws come in vanilla's order.
pub trait LevelRandomHost: LevelHost {
    /// Access a holder's server-owned attachment map.
    fn attachments(&mut self, holder: Holder) -> Result<&mut Attachments, AttachmentError>;
    /// The level's random stream through `native_level_random`, or None on a non-authoritative level.
    fn random(&mut self) -> Option<&mut LegacyRandom>;
}

/// A `LevelReader` with vanilla's `ScheduledTickAccess`. Shape updates cannot write blocks.
pub trait LevelTicksHost: LevelHost {
    /// Schedule a block tick through `native_schedule_block_tick`.
    fn schedule_block_tick(&mut self, pos: [i32; 3], block: &str, delay: i32) -> bool;
    /// Schedule a fluid tick through `native_schedule_fluid_tick`.
    fn schedule_fluid_tick(&mut self, pos: [i32; 3], fluid: &str, delay: i32) -> bool;
}

/// Mutable level services, like vanilla's `ServerLevel`. Whole-block writes wait for block entity support.
pub trait LevelWriteHost: LevelTicksHost + LevelRandomHost {
    /// Change a block property through `native_set_block_property`.
    fn set_block_property(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool;
    /// Play a sound for nearby players through `native_play_sound`.
    fn play_sound(
        &mut self,
        sound: &str,
        at: [f64; 3],
        source: usize,
        volume: f32,
        pitch: f32,
    ) -> bool;
    /// `ServerLevel.sendParticles` through `native_send_particles`.
    fn send_particles(
        &mut self,
        options: &ParticleOptions,
        state: Option<BlockStateId>,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: i32,
    ) -> bool;
}

macro_rules! level_attachment_host {
    ($($host:ident),+) => { $(
        impl AttachmentHost for dyn $host + '_ {
            fn attachments(&mut self, holder: Holder) -> Result<&mut Attachments, AttachmentError> {
                LevelRandomHost::attachments(self, holder)
            }
        }
    )+ };
}
level_attachment_host!(LevelRandomHost, LevelWriteHost);

mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod_api_generated.rs"));
}
pub use generated::Hooks;
#[doc(hidden)]
pub use generated::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Quiet;
    impl Hooks for Quiet {}

    /// A mod turned off is skipped in the game's list from the next dispatch, and resumes when
    /// turned on; a test's own list is never switched.
    #[test]
    fn a_mod_turned_off_is_skipped_until_turned_on() {
        name_mods(&["quiet", "broken"]);
        let _ = MODS.set(Arc::from(vec![
            Box::new(Quiet) as Box<dyn Hooks>,
            Box::new(Quiet),
        ]));
        let ids = mod_ids().to_vec();
        let called = || entered(mods(), "test").count();
        assert_eq!(called(), 2);
        assert!(set_mod_on(ids[1], false));
        assert_eq!(called(), 1);
        // Dispatched alone, it is still off.
        assert_eq!(entered(&mods()[1..], "test").count(), 0);
        let own: Vec<Box<dyn Hooks>> = vec![Box::new(Quiet), Box::new(Quiet)];
        assert_eq!(entered(&own, "test").count(), 2);
        assert!(set_mod_on(ids[1], true));
        assert_eq!(called(), 2);
        assert!(!set_mod_on("not_a_mod", false));
    }

    #[test]
    fn one_mod_dispatched_alone_keeps_its_place() {
        let all = [10, 11, 12, 13];
        assert_eq!(place_in(&all, &all[2..=2]), Some(2));
        assert_eq!(place_in(&all, &all), Some(0));
        assert_eq!(place_in(&all, &[12]), None);
    }

    struct Broken;
    impl Hooks for Broken {
        fn component_types(&self, _event: &mut ComponentTypesEvent<'_>) {
            panic!("a broken mod");
        }
    }

    /// A double with the three `Host` methods the probe calls.
    struct Reader {
        block: &'static str,
        lines: Vec<Text>,
    }

    impl Host for Reader {
        fn block(&self, _pos: [i32; 3]) -> String {
            self.block.to_string()
        }
        fn acting_player(&self) -> Option<NativeServerPlayer> {
            Some(NativeServerPlayer {
                uuid: 7,
                name: "Probe".to_string(),
                permission: 0,
                dimension: "minecraft:overworld".to_string(),
                position: [0.0; 3],
                game_mode: 0,
                health: 20.0,
                yaw: 0.0,
                pitch: 0.0,
                entity_id: 0,
                address: String::new(),
                latency: 0,
                protocol: 0,
                xp_level: 0,
                xp_progress: 0.0,
                on_ground: true,
                flying: false,
                may_fly: false,
                invulnerable: false,
                walk_speed: 0.1,
                fly_speed: 0.05,
                sneaking: false,
                eye_height: 1.62,
                selected_slot: 0,
                nickname: None,
                hidden: None,
                using_item: false,
                blocking: false,
                sprinting: false,
                gliding: false,
            })
        }
        fn action_bar(&mut self, _uuid: u128, line: Text) -> bool {
            self.lines.push(line);
            true
        }
    }

    /// Claims a use on stone, after telling the player what the block is.
    struct Probe;

    impl Hooks for Probe {
        fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
            let block = host.block(event.pos);
            let player = host.acting_player().expect("a player acts");
            host.action_bar(player.uuid, Text::literal(block.clone()));
            if block == "minecraft:stone" {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }

    /// Asks the host to tell a player without anyone having implemented it.
    struct Chatty;

    impl Hooks for Chatty {
        fn use_item_on(&self, host: &mut dyn Host, _event: &mut UseItemOnEvent<'_>) -> Flow {
            host.tell(7, Text::literal("hello"));
            Flow::Continue
        }
    }

    fn stick() -> ItemStack {
        ItemStack {
            item: Arc::from("minecraft:stick"),
            count: 1,
            damage: 0,
            extra: None,
        }
    }

    #[test]
    fn a_panic_in_a_hook_names_the_mod_and_the_hook() {
        name_mods(&["quiet", "broken"]);
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Quiet), Box::new(Broken)];
        let caught = Arc::new(Mutex::new(Vec::new()));
        let sink = caught.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let error = crate::crash::panic_report(info, "client").error;
            if error.contains("a broken mod") {
                sink.lock().unwrap().push(error);
            }
        }));
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_component_types(&mods, &mut Vec::new())
        }));
        // Once the dispatch has unwound, the thread is in no mod.
        let _ = std::panic::catch_unwind(|| panic!("a broken mod's caller"));
        std::panic::set_hook(previous);
        // The game stops as before: the panic goes on to the dispatch's caller.
        assert!(unwound.is_err());
        let errors = caught.lock().unwrap();
        assert_eq!(errors.len(), 2);
        assert!(
            errors[0].ends_with("\nin native mod broken, hook component_types"),
            "{}",
            errors[0]
        );
        assert!(!errors[1].contains("in native mod"), "{}", errors[1]);
    }

    #[test]
    fn a_test_host_implements_only_what_its_hook_calls() {
        let mods = [Box::new(Probe) as Box<dyn Hooks>];
        let stack = stick();
        for (block, expected) in [("minecraft:stone", true), ("minecraft:dirt", false)] {
            let mut host = Reader {
                block,
                lines: Vec::new(),
            };
            let mut claimed = false;
            dispatch_use_item_on(&mut host, &mods, &stack, [1, 2, 3], [0, 1, 0], &mut claimed);
            assert_eq!(claimed, expected, "{block}");
            let told: Vec<String> = host.lines.iter().map(Text::plain).collect();
            assert_eq!(told, [block], "{block}");
        }
    }

    struct Backstab;

    impl Hooks for Backstab {
        fn living_incoming_damage_v2(
            &self,
            host: &mut dyn Host,
            event: &mut LivingIncomingDamageV2Event<'_>,
        ) -> Flow {
            assert_eq!(event.causing, 7);
            assert_eq!(event.kind, "minecraft:zombie");
            assert_eq!(event.damage_type, "minecraft:player_attack");
            if event.entity == 0 {
                return Flow::Cancel;
            }
            let from = event.source_position.unwrap();
            let behind = (0..3)
                .map(|axis| (from[axis] - event.victim_position[axis]) * event.victim_view[axis])
                .sum::<f64>()
                < 0.0;
            if behind {
                let weapon = event.weapon.unwrap();
                let level = weapon.enchantment_level("probe:backstab");
                assert_eq!(weapon.enchantments().len(), 1);
                event.amount *= level as f32;
                let player = host.acting_player().unwrap();
                host.action_bar(player.uuid, Text::literal("backstab"));
            }
            Flow::Continue
        }
    }

    #[test]
    fn damage_v2_exposes_weapon_geometry_and_host_without_changing_v1() {
        let mods = [Box::new(Backstab) as Box<dyn Hooks>];
        let mut weapon = stick();
        weapon.enchant("probe:backstab", 3);
        for (entity, source_z, expected, canceled_expected) in [
            (1, -1.0, 12.0, false),
            (1, 1.0, 4.0, false),
            (0, -1.0, 4.0, true),
        ] {
            let mut host = Reader {
                block: "minecraft:air",
                lines: Vec::new(),
            };
            let mut amount = 4.0;
            let mut canceled = false;
            dispatch_living_incoming_damage_v2(
                &mut host,
                &mods,
                entity,
                "minecraft:zombie",
                "minecraft:player_attack",
                7,
                Some([0.0, 0.0, source_z]),
                [0.0; 3],
                [0.0, 0.0, 1.0],
                Some(&weapon),
                &mut amount,
                &mut canceled,
            );
            assert_eq!((amount, canceled), (expected, canceled_expected));
            assert_eq!(host.lines.len(), usize::from(expected == 12.0));
        }
        let mut amount = 4.0;
        let mut canceled = false;
        dispatch_living_incoming_damage(
            &mods,
            1,
            "minecraft:zombie",
            "minecraft:player_attack",
            7,
            7,
            &mut amount,
            &mut canceled,
        );
        assert_eq!((amount, canceled), (4.0, false));
    }

    #[test]
    #[should_panic(expected = "Host::tell")]
    fn an_unexpected_host_call_names_the_method() {
        let mods = [Box::new(Chatty) as Box<dyn Hooks>];
        let mut host = Reader {
            block: "minecraft:stone",
            lines: Vec::new(),
        };
        let mut claimed = false;
        dispatch_use_item_on(&mut host, &mods, &stick(), [0; 3], [0, 1, 0], &mut claimed);
    }

    struct ItemMobReply {
        answer: Option<bool>,
        calls: Arc<Mutex<Vec<Option<bool>>>>,
    }
    impl Hooks for ItemMobReply {
        fn item_interact_mob(
            &self,
            _: &mut dyn Host,
            event: &mut ItemInteractMobEvent<'_>,
        ) -> Option<bool> {
            assert_eq!(event.entity, 17);
            assert_eq!(event.kind, "minecraft:zombie");
            assert_eq!(event.stack, Some(&stick()));
            self.calls.lock().unwrap().push(self.answer);
            self.answer
        }
    }

    #[test]
    fn item_interact_mob_first_response_controls_swing() {
        for (answers, expected, expected_calls) in [
            ([None, Some(false), Some(true)], Some(false), 2),
            ([None, Some(true), Some(false)], Some(true), 2),
            ([None, None, None], None, 3),
        ] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let mods: Vec<Box<dyn Hooks>> = answers
                .into_iter()
                .map(|answer| {
                    Box::new(ItemMobReply {
                        answer,
                        calls: calls.clone(),
                    }) as Box<dyn Hooks>
                })
                .collect();
            let mut host = Reader {
                block: "minecraft:air",
                lines: Vec::new(),
            };
            let mut swing = None;
            dispatch_item_interact_mob(
                &mut host,
                &mods,
                17,
                "minecraft:zombie",
                Some(&stick()),
                &mut swing,
            );
            assert_eq!(swing, expected);
            assert_eq!(calls.lock().unwrap().len(), expected_calls);
            calls.lock().unwrap().clear();
            swing = Some(false);
            dispatch_item_interact_mob(
                &mut host,
                &mods,
                17,
                "minecraft:zombie",
                Some(&stick()),
                &mut swing,
            );
            assert_eq!(
                swing,
                Some(false),
                "an earlier mixin already owned the result"
            );
            assert!(calls.lock().unwrap().is_empty());
        }
    }
}
