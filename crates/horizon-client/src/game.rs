//! The playable game loop: streaming world, player simulation at 20 ticks/s,
//! block interaction, inventory and HUD.

/// The content tables the world plays with, as field-level borrows: the
/// world's data packs' over the game's own. The `Game::items`/`recipes`/
/// `stations` methods read the same tables, but taking `&self` blocks a
/// `&mut self.…` in the same scope; these expand to a match on the fields,
/// which borrows stay disjoint from.
macro_rules! world_items {
    ($s:ident) => {
        match &$s.world_tables {
            Some(t) => &t.items,
            None => $s.res.items(),
        }
    };
}
macro_rules! world_recipes {
    ($s:ident) => {
        match &$s.world_tables {
            Some(t) => &t.recipes,
            None => $s.res.recipes(),
        }
    };
}
macro_rules! world_stations {
    ($s:ident) => {
        match &$s.world_tables {
            Some(t) => &t.stations,
            None => $s.res.stations(),
        }
    };
}
/// The biome table the world plays with (`Game::biomes`), as a field-level
/// borrow: the world's packs' over the game's own.
macro_rules! world_biomes {
    ($s:expr) => {
        match &$s.world_biomes {
            Some(b) => b,
            None => $s.res.biomes(),
        }
    };
}

/// The world's packs' content files, as a field-level borrow (see the macros).
fn pack_files(world_tables: &Option<WorldTables>) -> horizon_world::loot::PackOverlay<'_> {
    match world_tables {
        Some(t) => horizon_world::loot::PackOverlay { files: &t.files },
        None => horizon_world::loot::PackOverlay::NONE,
    }
}

mod advancements;
mod advancements_screen;
mod agent;
mod agent_settings;
mod allays;
mod armor_stands;
mod atlas_slots;
mod attachment_sync;
mod autosave;
mod banners;
mod beacon;
mod beds;
mod beehives;
mod block_editor;
mod block_edits;
mod block_entity_anims;
mod block_entity_ticks;
mod block_items;
mod block_particles;
mod block_ticks;
mod blockctl;
mod boarding;
mod boat_authority_fixture;
mod boat_leads;
mod boat_passengers;
mod boat_rider_fixture;
mod boats;
mod books;
mod bossbars;
mod breezes;
mod brush;
mod building_players;
mod bundles;
mod campfire;
mod campfire_douse;
mod cartography;
mod cat_spawner;
mod cauldrons;
mod chat;
mod chat_filter;
mod chiseled_bookshelf;
mod client_ambient;
mod clip_ray;
mod clipboard;
#[cfg(test)]
mod clone_block_ticks_tests;
#[cfg(test)]
mod clone_dimensions_tests;
mod code_of_conduct;
mod combat;
mod command_blocks;
mod command_harness;
mod command_minecarts;
mod commands;
mod composter;
mod consume;
mod control;
mod copper_golems;
#[cfg(test)]
mod core_tests;
mod cosmetics;
mod creaking;
mod creative_hotbar;
mod creative_screen;
mod creatures;
mod crossbow;
pub(crate) mod cushions;
mod custom_sky;
mod debug_keys;
mod debug_lines;
mod debug_options;
mod debug_renderers;
mod debug_stick;
mod decorated_pot;
mod dedicated;
mod deflect;
mod dialogs;
#[cfg(feature = "discord")]
mod discord;
mod dispense_fixture;
mod dispenser;
mod dragon;
mod dynamic_lights;
mod emote_wheel;
mod emotes;
mod enchant_effects;
mod end;
mod enderman;
#[cfg(test)]
mod enderman_hurt_teleport_tests;
#[cfg(test)]
mod enderman_placement_tests;
mod entity;
mod entity_culling;
mod entity_sounds;
mod event_loop;
mod events;
mod execution_role;
mod explosion;
mod extras;
mod falling_blocks;
mod falling_damage;
mod fangs;
mod fireworks;
#[cfg(test)]
mod first_tick_registration_tests;
mod fishing;
mod fov;
mod foxes;
mod frame_geometry;
mod frames;
#[cfg(feature = "friends")]
mod friends;
#[cfg(test)]
mod setblock_order_tests;
// Without Play with Friends: nothing shared, no Friends button (game/friends_off.rs).
#[cfg(test)]
mod entity_inside_tests;
#[cfg(not(feature = "friends"))]
#[path = "game/friends_off.rs"]
mod friends;
mod functions;
mod game_mode;
mod game_tests;
mod gamemode_switcher;
mod ghasts;
mod glowing;
mod golems;
mod growth;
mod hand;
mod headless;
mod hud_editor;
mod hud_room;
mod ime;
mod initial_spawn;
mod input_trace;
mod integrated;
mod interaction_shapes;
mod interactions;
mod inventory_input;
mod inventory_slots;
mod item_actions;
mod item_areas;
#[cfg(test)]
mod item_contact_tests;
mod item_draw;
mod item_durability;
mod item_entities;
#[cfg(test)]
mod item_interact_mob_tests;
mod item_particles;
mod item_use;
mod jigsaw_blocks;
mod jigsaw_edit;
mod jukebox;
mod key_codes;
mod key_focus;
mod lan;
mod lan_actions;
mod lan_attributes;
mod lan_camera;
mod lan_chat;
mod lan_crafting;
mod lan_entities;
mod lan_far;
mod lan_idle;
mod lan_mobs;
mod lan_mount_menu;
mod lan_particles;
mod lan_pvp;
mod lan_quiet;
mod lan_screen;
mod lan_stations;
mod lan_trading;
mod lan_vehicles;
mod lan_waypoints;
mod launch_api;
mod launch_options;
mod leads;
mod legacy_rules;
mod level_particles;
mod levels;
mod lightning;
mod llamas;
mod loading_screen;
mod loading_window;
mod locator_bar;
mod loom;
mod mace;
mod machines;
mod maps;
mod melee;
mod memory;
mod menu_clicks;
mod minecarts;
#[cfg(test)]
mod mining_component_tests;
mod mob_blocks;
#[cfg(test)]
mod mob_discard_tests;
mod mob_gear;
mod mob_index;
mod mob_looting;
mod mob_sounds;
mod mod_api_host;
mod mod_browser;
mod mods;
mod mount_menu;
mod mounts;
mod moving_pistons;
mod music;
mod names;
mod narrator;
mod native_biome_colors;
pub mod native_block_entity_draw;
mod native_block_sprites;
mod native_bots;
mod native_chunk_snapshot;
mod native_chunk_tickets;
mod native_claimed_items;
mod native_client_entities;
mod native_client_particles;
mod native_client_particles_fixture;
pub mod native_client_services;
mod native_client_world;
mod native_components;
mod native_configs;
mod native_criteria;
mod native_damage;
mod native_data_maps;
mod native_diagnostics;
mod native_entities;
mod native_entity_data;
mod native_entity_motion;
mod native_entity_moves;
mod native_entity_refs;
mod native_entity_state;
mod native_explosions;
mod native_falling;
mod native_fluid_render;
mod native_guards;
mod native_hooks;
mod native_host_bridge;
mod native_inventories;
mod native_item_container;
mod native_item_energy;
mod native_item_fluids;
mod native_level_effects;
mod native_level_refs;
mod native_level_world;
mod native_lifecycle;
mod native_living_refs;
mod native_machines;
mod native_menu_stacks;
mod native_menus;
mod native_mob_draw;
mod native_mobs;
mod native_motion;
mod native_mounts;
mod native_net;
mod native_npcs;
mod native_packets;
mod native_panel;
mod native_panel_recipes;
mod native_permissions;
mod native_player_break;
mod native_player_fx;
mod native_player_reads;
mod native_player_state;
mod native_profiles;
mod native_projectiles;
mod native_quick_move;
mod native_recipes;
mod native_reload_listeners;
mod native_scene;
mod native_server;
mod native_shaders;
mod native_slot_roles;
mod native_structures;
mod native_terrain_model;
pub mod native_text_field;
mod native_ticks;
mod native_travel;
mod native_view_distance;
mod native_visuals;
mod native_world_border;
mod native_world_columns;
pub mod native_world_draw;
mod native_world_edit;
mod native_world_read;
mod native_world_seed;
mod natural_spawning;
mod nautili;
mod net_harness;
mod nether_portal;
mod no_resources;
mod occlusion_culling;
mod ominous_item_spawner;
mod options;
mod ordered_tick;
mod packs;
mod pad;
mod pad_actions;
mod pad_craft;
mod pad_menu;
mod paintings;
mod pandas;
mod particle_culling;
mod passengers;
mod persist;
mod perspective;
mod physical_trigger;
mod pick_entity;
mod pickups;
mod piglins;
mod piston_push;
#[cfg(test)]
mod place_feature_tests;
mod play_input;
mod play_keys;
mod player_equip;
mod player_spawn;
mod pointer_capture;
#[cfg(test)]
mod portal_ticking_tests;
mod postprocess;
mod potent_sulfur;
mod potions;
mod powder_snow;
mod prediction;
mod pumpkin;
mod pvp;
mod quick_actions;
mod raids;
mod random_textures;
mod recipe_book;
mod remote;
mod remote_camera;
mod remote_chat;
mod remote_chunks;
mod remote_mob_data;
mod remote_mob_particles;
mod remote_mob_use;
mod remote_objects;
mod remote_projectiles;
mod remote_stations;
mod remote_vehicles;
mod render_control;
mod render_settings;
mod render_tick;
mod repro;
mod resource_load;
mod resource_loader;
mod resource_tables;
mod respawn_anchor;
mod saves_control;
mod saving;
mod scoreboard;
mod screen_backgrounds;
mod screen_info;
mod screens;
mod screenshot_sequence;
mod sculk;
mod sculk_fixture;
mod seats;
mod server_links;
mod server_packs;
mod server_travel;
mod shader_packs;
mod sheep_dye;
mod shelf;
mod shield;
mod shoulders;
mod sign_edit;
mod skin_models;
mod sniffers;
mod social;
mod spawner;
mod spear;
mod spear_players;
mod spectator;
mod spill;
mod splash_water;
mod split;
mod startup_menu;
mod station_control;
mod stations;
mod stats;
mod stats_screen;
mod storage;
mod structure_blocks;
mod structure_edit;
mod structures;
mod studio;
mod subtitle_hud;
mod sulfur_cubes;
mod tempban;
mod test_block_edit;
mod test_blocks;
mod test_entities;
mod test_environments;
mod test_instance_edit;
mod test_instances;
mod text_displays;
mod tick_profile;
mod tick_script;
mod titles;
mod tnt;
mod totem;
mod trading;
mod transient_blocks;
mod trial_spawner;
mod trial_visuals;
mod trident;
mod tutorial;
mod tweaks;
mod ui_control;
mod use_hand;
mod using_item;
mod vault;
mod vehicle_teleport;
mod villagers;
mod wandering_trader;
mod win;
mod wind;
mod window_handler;
mod wither;
mod world_entry;
mod world_loading;
mod world_primitives;
mod world_session;
#[cfg(test)]
mod world_test_def_tests;
mod world_update;
mod zombie_reinforcements;
mod zombie_villagers;
use event_loop::EventLoopCtl;
use horizon_client::random_source;
// `Action` here is the title screen's buttons (`title::Action`).
pub use advancements::AdvancementDisplay;
use horizon_client::actions::Action as Mapping;
use horizon_client::held::Source;
use horizon_client::spans::startup;
use horizon_client::{
    Result,
    assets::{Assets, RenderAssets},
    audio::Audio,
    biome::Biomes,
    block_entity_render::{
        self, BlockEntityRenderer, CacheOptions, Scene as BlockEntityScene, StaticCacheFrameStats,
        StaticSectionSnapshot,
    },
    blocks::{BlockFacts, Fluid},
    clock::FixedStep,
    collisions,
    containers::{BlockEntity, Providers},
    crafting::Recipes,
    creative::{Creative, Names},
    entities::{Experience, ExperienceOrb, ItemEntity},
    environment::Environment,
    first_person::{Hand, SwingAnimation},
    furnace::FurnaceCooking,
    gfx::{Frame, Renderer},
    graphics::EntityCullingMode,
    hud::{HudAtlas, HudBatch, rgba},
    hud_layout, hud_overlays,
    items::{ItemStack, Items},
    keymapping::{KeyAction, SNEAK, SPRINT},
    light::LightTables,
    mesher::{MeshTables, Vertex},
    mobs::{Mob, MobType, MobTypes},
    models,
    motion::{MotionEvent, MotionResources, PlayerMotion},
    mouse::{LookOptions, MouseLook},
    particles::Particle,
    save::{Level, Persistent, SaveDir},
    settings::Settings,
    spans,
    sprint::Keys,
    streaming::{Streaming, WorkerContext},
    testgen::TestTerrain,
    title::{Action, Title},
    vanillagen::VanillaTerrain,
    world::World,
};
use horizon_core::{
    biome::{BiomeZoom, obfuscate_seed},
    geometry::{Aabb, Direction},
    position::BlockPos,
};
use horizon_core::{geometry::Vec3, random::RandomSource};
use horizon_world::{
    block_collision::BlockCollisionRegistry,
    collision_context::{CollisionContext, CollisionEntity, CollisionItem},
    entity_obstruction::{EntityIdentity, EntityObstructionWorld, ObstructionEntity},
    fluid::FluidState,
    level::{
        Behaviours, BucketContent, Dimension as LevelDimension, EntityQuery, Landing, LevelAccess,
        LevelEvent, PlaceContext, PlaceOutcome, PlacementContext, PlayerContext, PlayerEffect,
        Ticks, flags as level_flags,
    },
    registry::BlockStateId,
};
use key_codes::{
    container_key_clones, drop_held_behind_dialog, follow_named_keymap, gameplay_has_keyboard,
    glfw_button, glfw_modifiers, keyboard_button,
};
use launch_options::Options;
use loading_screen::{
    App, Loading, leave_fullscreen_after, missing_resources, run_event_loop, unclean_start_recovery,
};
use loading_window::{
    build_renderer, loading_frame, loading_renderer, prepare_renderer_textures, start_game,
};
pub use machines::native_fluids::NativeFluidTank;
pub use maps::{NativeMapDecoration, NativeMapInfo};
pub use native_block_entity_draw::BlockEntityDraw;
pub use native_chunk_snapshot::NativeChunkSnapshot;
pub use native_claimed_items::{NativeHeldItem, NativeHit, NativeModEvent};
pub use native_client_world::{NativeBiomeId, NativeClientBiomes, NativeClientChunks};
pub use native_components::NativeComponentError;
pub use native_configs::{NativeConfig, NativeConfigFile, NativeConfigKind};
pub use native_criteria::NativeCriterion;
pub use native_data_maps::NativeDataMapType;
pub use native_falling::NativeFalling;
pub use native_hooks::{NativeClock, NativeItemEntity, NativePlayer, NativeShake};
pub use native_inventories::{NativeInventoryError, NativeInventoryKind};
pub use native_item_energy::ItemEnergyBuffer;
pub use native_mob_draw::{NativeMobRender, NativePlayerRender};
pub use native_player_state::{NativeAttributeModifier, NativeEffect, NativeEffectV2};
pub use native_reload_listeners::NativeReloadListener;
pub use native_server::NativeExperienceOrb;
pub use native_slot_roles::NativeSlotRole;
pub use native_structures::{NativeStructureBlock, NativeStructureTemplate};
pub use native_world_columns::NativeColumnBlocks;
pub use native_world_draw::NativeWorldDraw;
pub use native_world_edit::{NativeBlockEdit, NativeBlockNbt, NativeBlockRun, NativeHeightmap};
pub use native_world_read::NativeBlockReadError;
use resource_load::{Progress, launch_world, load_resources, named_scope, synchronous_startup};
use resource_loader::{
    EntityModels, GameCpuResources, Loaded, LoaderMsg, PendingWorldEntry, WorldRestLoader,
    run_loading, write_report,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{Receiver, TryRecvError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    error::EventLoopError,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

/// The time a frame spends uploading chunk meshes when the player set no
/// Chunk Updates or Upload Budget: a burst past it waits for the next frames.
const AUTO_UPLOAD_TIME: Duration = Duration::from_millis(2);
/// `AUTO_UPLOAD_TIME` while the view's terrain is still arriving (a world's
/// open, a teleport): the meshes waiting then are the view itself, not edits
/// to it, so a frame gives them more of its time to fill the view sooner.
const AUTO_UPLOAD_TIME_ARRIVING: Duration = Duration::from_millis(6);
/// A frame's finished meshes up to this many upload at once, unqueued.
const AUTO_UPLOADS_AT_ONCE: usize = 8;
/// The most changed columns far terrain waits on behind a mod's hold (`Game::lod_held`): past
/// it, they go in as ever.
const HELD_FAR_COLUMNS: usize = 1 << 13;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Playing,
    GameModeSwitcher,
    /// Horizon's emote wheel (`game/emote_wheel.rs`): over the world, not pausing.
    EmoteWheel,
    Paused,
    Options,
    Chat,
    Creative,
    Inventory,
    Dead,
    /// The Statistics screen.
    Stats,
    /// The Advancements screen.
    Advancements,
    /// WorldOptionsScreen (game/lan_screen.rs).
    WorldOptions,
    /// Play with Friends, from the pause menu's Friends button
    /// (`friends_screen`, game/friends.rs).
    Friends,
    /// `SocialInteractionsScreen` (`social_screen`, game/social.rs).
    Social,
    /// CommandBlockEditScreen (game/command_blocks.rs).
    CommandBlock,
    /// A game master block's edit screen (game/block_editor.rs).
    BlockEditor,
    /// ServerLinksDialogScreen: a server's links (game/server_links.rs).
    ServerLinks,
    /// ConfirmLinkScreen for a server link (game/server_links.rs).
    ConfirmLink,
    /// DialogScreen.WarningScreen: the note about custom screens.
    DialogWarning,
    /// WinScreen: the end poem and the credits (game/win.rs).
    Win,
    /// DebugOptionsScreen, F3+F6 (game/debug_options.rs).
    DebugOptions,
    /// A screen of a native mod's own (`native_mod_screen_open`): it draws and
    /// takes its input through the screen hooks.
    Mod,
    /// PvP & HUD's widget editor (game/hud_editor.rs).
    HudEditor,
    /// Horizon Studio script screen (`studio_screen_open`): canvas draws
    /// itself, takes input through the screen hooks, does not draw chat.
    Studio,
}
impl Screen {
    /// The id native mods see for this screen (`NativeScreenView.id`, the
    /// `screen_render_post` hook): one per variant, and a hook's contract, so
    /// it stays the same from release to release. A new variant must add one.
    fn id(self) -> &'static str {
        match self {
            Screen::Playing => "playing",
            Screen::GameModeSwitcher => "game_mode_switcher",
            Screen::EmoteWheel => "emote_wheel",
            Screen::Paused => "paused",
            Screen::Options => "options",
            Screen::Chat => "chat",
            Screen::Creative => "creative",
            Screen::Inventory => "inventory",
            Screen::Dead => "dead",
            Screen::Stats => "stats",
            Screen::Advancements => "advancements",
            Screen::WorldOptions => "world_options",
            Screen::Friends => "friends",
            Screen::Social => "social",
            Screen::CommandBlock => "command_block",
            Screen::BlockEditor => "block_editor",
            Screen::ServerLinks => "server_links",
            Screen::ConfirmLink => "confirm_link",
            Screen::DialogWarning => "dialog_warning",
            Screen::Win => "win",
            Screen::DebugOptions => "debug_options",
            Screen::Mod => "mod",
            Screen::HudEditor => "hud_editor",
            Screen::Studio => "studio",
        }
    }
}
struct Mining {
    pos: [i32; 3],
    progress: f32,
    /// The latest destroy-speed query, reused for server crack feedback.
    per_tick: f32,
    ticks: u32,
}
#[derive(Clone, Copy)]
struct Hit {
    pos: [i32; 3],
    face: [i32; 3],
    point: [f64; 3],
}
impl Hit {
    /// The block the clicked face leads to (`pos.relative(direction)`).
    fn beside(&self) -> [i32; 3] {
        [
            self.pos[0] + self.face[0],
            self.pos[1] + self.face[1],
            self.pos[2] + self.face[2],
        ]
    }
}
/// What a falling block's tick did (`FallingBlockEntity.tick`).
enum FallStep {
    Falling,
    Gone,
    /// Gave up falling: drops itself as an item here.
    Dropped(Vec3),
    /// Hit the ground in this block.
    Landed([i32; 3]),
}
/// A guest's right click comes as one packet per hand and part
/// (`Minecraft.startUseItem`): `ServerGamePacketListenerImpl.handleUseItemOn`
/// does the block's and `ItemStack.useOn`'s part, `handleUseItem` the
/// item's own (`Item.use`). Running both for either would use the item twice.
#[derive(Clone, Copy)]
enum GuestUse {
    OnBlock(Hand),
    Item(Hand),
    /// `handleInteract`: `Player.interactOn` with the hand, nothing else (the
    /// client sends UseItem itself when the entity passed), on the entity the
    /// packet named: `ServerGamePacketListenerImpl.handleInteract` resolves
    /// `packet.entityId()` once and range checks it, so the click never picks
    /// an entity again from the view.
    Entity(Hand, InteractTarget),
}

/// The entity an interact packet names and where its client clicked it (the
/// packet's location, measured from the entity's position): what
/// `ServerGamePacketListenerImpl.handleInteract` takes from
/// `ServerboundInteractPacket` and hands to `Player.interactOn`, which passes
/// the point on to `interactAt`.
#[derive(Clone, Copy)]
struct InteractTarget {
    id: i32,
    location: [f64; 3],
}

/// The client world as seen by the vanilla block-behaviour library.
struct GameLevel<'w> {
    gateway_servers: &'w mut horizon_client::fast_hash::FxHashMap<
        [i32; 3],
        horizon_client::block_entity_anim::EndGateway,
    >,
    border: &'w horizon_core::geometry::border::BorderGeometry,
    world: &'w mut World,
    redstone_experiments: bool,
    facts: &'w BlockFacts,
    collision: &'w BlockCollisionRegistry<'w>,
    events: &'w mut Vec<LevelEvent>,
    dimension: LevelDimension,
    darken: i32,
    /// The environment of the level's dimension (`SUN_ANGLE`).
    environment: &'w Environment,
    raining: bool,
    thundering: bool,
    player_box: Option<([f64; 3], [f64; 3])>,
    /// The LAN guests in this level who trigger blocks (not spectators).
    guest_boxes: Vec<([f64; 3], [f64; 3])>,
    /// Whose `guest_boxes` are, in order.
    guest_box_uuids: Vec<u128>,
    /// Whose `player_box` is: the acting player (`native_player_physical_trigger`).
    player_box_uuid: u128,
    /// Other players that block placement during a guest turn, whose hub is
    /// borrowed out; None outside one, where `guest_boxes` are those players.
    building_player_boxes: Option<Vec<([f64; 3], [f64; 3])>>,
    player_collision: Option<PlayerCollision>,
    /// Minecart boxes (detector rails).
    cart_boxes: Vec<([f64; 3], [f64; 3])>,
    /// The item frames a comparator reads through a conductor.
    item_frames: &'w [horizon_client::entities::ItemFrame],
    /// The minecarts, in `cart_boxes` order, for a detector rail's comparator.
    carts: &'w [minecarts::Minecart],
    /// Arrow and thrown trident boxes (wooden buttons).
    arrow_boxes: Vec<([f64; 3], [f64; 3])>,
    /// Every entity on the walk; see `entity::for_each_entity`.
    entities: entity::EntityLists<'w>,
    /// End crystals, which aren't on the walk yet.
    crystals: &'w [dragon::Crystal],
    /// The fishing hooks here, which aren't on the walk either. None ignores
    /// block triggers, so each counts on a plate or tripwire. The flag says
    /// whether it also reaches `entityInside` (`applyEffectsFromBlocks`): not
    /// a hook on an entity, whose tick skips that step.
    trigger_hooks: Vec<(Aabb, bool)>,
    /// The lightning bolts here: none ignores block triggers
    /// (`Entity.isIgnoringBlockTriggers`).
    trigger_extras: Vec<Aabb>,
    biomes: &'w Biomes,
    seed: i64,
    behaviours: &'w Behaviours,
    items: &'w Items,
    /// Cooking recipes' experience, which a removed furnace pays out.
    recipes: &'w horizon_client::crafting::Recipes,
    /// Container loot tables (`unpackLootTable`).
    loot: Option<std::sync::Arc<horizon_world::loot::LootData>>,
    registry: &'static horizon_world::registry::BlockRegistry,
    rules: horizon_world::level::GameRules,
    /// `Difficulty.getId()` (peaceful 0 .. hard 3).
    difficulty: i32,
    /// The feet of the players who keep fire spreading around them: the local
    /// player and the LAN guests in this level, spectators aside.
    fire_players: Vec<[f64; 3]>,
    /// Block loot random sequences (`ServerLevel.getRandomSequence`).
    rand_sequences: &'w mut horizon_client::random_sequences::RandomSequences,
    pending: &'w mut spill::Pending,
    spill: &'w mut spill::Spill,
    /// Where the chunk tickets let blocks tick.
    tickets: horizon_client::ticket_levels::Levels,
    random_tick_sections: &'w mut horizon_client::random_tick_sections::RandomTickSections,
    /// Players with each chest open (a trapped chest's signal), also what a
    /// recount of the openers replaces (`ContainerOpenersCounter.recheckOpeners`).
    lid_openers: &'w mut HashMap<(i32, i32, i32), i32>,
    /// The containers players have open at this moment, by position: the local
    /// player's and the guests' (`getEntitiesWithContainerOpen`). A double
    /// chest is in it as both halves.
    open_containers: &'w [(i32, i32, i32)],
    /// The chests copper golems have open, a double chest as both halves. They
    /// keep their own count (`chest_openers`), so they stay out of `open_containers`.
    golem_chests: &'w [(i32, i32, i32)],
    /// `Level.isClientSide`: a client's copy of the level (a guest of a LAN
    /// world, a window on its server thread, a dedicated server's client).
    client_side: bool,
    /// Only the integrated or dedicated server dispatches native observations.
    native_authoritative: bool,
    native_player: Option<u128>,
    /// `Level.isDebug`: `setBlock` changes nothing.
    debug: bool,
    native_sounds: &'w mut Vec<creatures::NativeSound>,
    native_particles: &'w mut Vec<lan_particles::QueuedParticle>,
    /// The block a mod's machine breaks and the tool it breaks it with
    /// (`native_machine_destroy_block`): `Block.dropResources` rolls that
    /// block's loot with the tool instead of with none. Neighbours that pop
    /// off because of the break drop as with no tool, as in vanilla.
    drop_tool: Option<((i32, i32, i32), Option<ItemStack>)>,
    /// Only this destroyed block has an entity as its loot `this` parameter.
    /// Neighbour/default/machine drops keep the no-entity context.
    drop_breaker_at: Option<BlockPos>,
    /// Where `Block.dropResources` puts the drops instead of popping them,
    /// while a mod's machine breaks a block (`native_machine_destroy_block`).
    drop_into: Option<Vec<ItemStack>>,
}
struct PlayerCollision {
    y: f64,
    descending: bool,
    fall_distance: f64,
    main_hand_item: Arc<str>,
    main_hand_count: i32,
    leather_boots: bool,
}
/// The boxes of the entities that block building, as obstruction candidates.
struct BuildingObstruction(Vec<Aabb>);
impl EntityObstructionWorld for BuildingObstruction {
    type Error = std::convert::Infallible;
    fn obstruction_candidates(
        &mut self,
        _excluded: Option<EntityIdentity>,
        bounds: Aabb,
    ) -> std::result::Result<Vec<ObstructionEntity>, Self::Error> {
        Ok(self
            .0
            .iter()
            .filter(|b| b.intersects(bounds))
            .enumerate()
            .map(|(i, &bounds)| ObstructionEntity {
                identity: EntityIdentity(i as u64),
                root_vehicle: EntityIdentity(i as u64),
                bounds,
                removed: false,
                spectator: false,
                blocks_building: true,
            })
            .collect())
    }
}
/// Whether a block's successful use is `InteractionResult.SUCCESS_SERVER`,
/// which a client does not predict: `useWithoutItem` of a bed, a light
/// block, a charged respawn anchor or a sign. Every other block use the
/// level answers with SUCCESS is `SUCCESS` (`SwingSource.PREDICTED`).
fn server_only_block_use(
    block: &horizon_items::blocks::StateFacts,
    branch: horizon_world::level::UseBranch,
) -> bool {
    branch == horizon_world::level::UseBranch::WithoutItem
        && [
            "AbstractBedBlock",
            "LightBlock",
            "RespawnAnchorBlock",
            "SignBlock",
        ]
        .iter()
        .any(|class| block.has_class(class))
}
/// An entity's box (`EntityDimensions.makeBoundingBox`): `width` wide and
/// `height` tall, standing on `feet`.
fn point_box(feet: [f64; 3], width: f64, height: f64) -> Aabb {
    Aabb::new(
        Vec3::new(feet[0] - width / 2.0, feet[1], feet[2] - width / 2.0),
        Vec3::new(
            feet[0] + width / 2.0,
            feet[1] + height,
            feet[2] + width / 2.0,
        ),
    )
}
impl GameLevel<'_> {
    /// `LilyPadBlock.entityInside`: the boxes of the boats that can break a pad.
    fn boat_trigger_boxes(&self) -> impl Iterator<Item = Aabb> + '_ {
        self.entities.boats.iter().map(boats::Boat::bounds)
    }
    /// How many entities in `area` press plates and tripwires, or with
    /// `LIVING` only the living ones; a constant so a list of other kinds
    /// compiles to no loop at all.
    fn count_triggers<const LIVING: bool>(&self, area: Aabb) -> usize {
        use entity::{Entity as _, for_each_entity};
        let mut count = 0;
        for_each_entity!(self.entities, entity => {
            if (!LIVING || entity.is_living())
                && entity.triggers_blocks()
                && area.intersects(entity.bounds())
            {
                count += 1;
            }
        });
        count
    }
    /// `DetectorRailBlock.getAnalogOutputSignal`: a powered rail reads the
    /// minecart on it, a command block cart first (its success count), else the first container cart's fullness.
    fn detector_rail_signal(&self, pos: BlockPos, powered: bool) -> i32 {
        if !powered {
            return 0;
        }
        let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
        let area = Aabb::new(
            Vec3::new(x + 0.2, y, z + 0.2),
            Vec3::new(x + 0.8, y + 0.8, z + 0.8),
        );
        let mut container = None;
        for ((lo, hi), cart) in self.cart_boxes.iter().zip(self.carts) {
            let bounds = Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            );
            if !area.intersects(bounds) {
                continue;
            }
            match cart.kind {
                minecarts::CartKind::CommandBlock => return cart.command.success_count,
                minecarts::CartKind::Chest | minecarts::CartKind::Hopper => {
                    // Counted here, for the cart a comparator reads, not for
                    // every cart on every block tick.
                    container = container.or_else(|| Some(self.cart_signal(cart)));
                }
                _ => {}
            }
        }
        container.unwrap_or(0)
    }
    /// A container minecart's comparator signal: its open menu's slots while
    /// someone has it open, else its own.
    fn cart_signal(&self, cart: &minecarts::Minecart) -> i32 {
        let key = (i32::MAX - 15, minecarts::CART_KEY_Y, cart.id as i32);
        let slots = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Chest(open)) => open,
            _ => &cart.items,
        };
        minecarts::Minecart::container_signal(slots, self.items)
    }
    /// Every entity box that blocks building: the players who aren't
    /// spectators, the minecarts and the `Blockers` (a removed mob or a marker
    /// armor stand, whose box is empty, doesn't).
    fn building_blocker_boxes(&self) -> Vec<Aabb> {
        let boxed = |(lo, hi): &([f64; 3], [f64; 3])| {
            Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            )
        };
        let b = self.entities;
        self.player_box
            .iter()
            .chain(
                self.building_player_boxes
                    .as_ref()
                    .unwrap_or(&self.guest_boxes),
            )
            .chain(&self.cart_boxes)
            .map(boxed)
            .chain(b.mobs.iter().filter(|m| !m.removed).map(Mob::bounds))
            .chain(
                b.armor_stands
                    .iter()
                    .filter(|s| !s.marker)
                    .map(|s| s.bounds()),
            )
            .chain(b.boats.iter().map(boats::Boat::bounds))
            .chain(self.crystals.iter().map(dragon::Crystal::bounds))
            .chain(b.tnts.iter().map(tnt::PrimedTnt::bounds))
            .chain(
                b.falling
                    .iter()
                    .map(|f| horizon_client::falling_block::bounds(f.position)),
            )
            .collect()
    }
}
impl LevelAccess for GameLevel<'_> {
    fn within_world_border(&self, pos: BlockPos) -> bool {
        self.border
            .is_within_bounds(f64::from(pos.x), f64::from(pos.z), 0.0)
    }
    fn redstone_experiments(&self) -> bool {
        self.redstone_experiments
    }
    fn allow_hopper_pickup_item(&self, pos: BlockPos, item: &str, count: u32) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_hopper_pickup_item(self.dimension, pos, item, count, &mut cancel);
        }
        !cancel
    }
    fn has_native_block_entity(&self, state: BlockStateId) -> bool {
        self.world.block_entities.stateful_entity(state.0)
    }
    fn ensure_block_entity(&mut self, pos: BlockPos, state: BlockStateId) {
        let key = (pos.x, pos.y, pos.z);
        if !self.world.block_entities.stateful_entity(state.0)
            || self.world.block_entities.get(&key).is_some()
        {
            return;
        }
        let mut entity = None;
        let Some(state) = self.registry.state(state) else {
            return;
        };
        Self::native_block_entity_from_state(&state.name, &state.properties, &mut entity);
        if let Some((id, nbt)) = entity {
            self.world.block_entities.insert(
                key,
                BlockEntity::Mod(Box::new(
                    horizon_client::containers::ModBlockEntityFields {
                        id,
                        nbt,
                        ..Default::default()
                    }
                    .into(),
                )),
            );
        }
    }
    fn observe_block_changed(&self, pos: BlockPos, old: BlockStateId, new: BlockStateId) {
        if self.native_authoritative {
            self.native_block_changed(self.dimension, pos, old, new);
        }
    }
    fn observe_neighbor_changed(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        changed: horizon_world::level::BlockId,
        moved: bool,
    ) {
        if self.native_authoritative {
            self.native_neighbor_changed(self.dimension, pos, state, changed, moved);
        }
    }
    fn redstone_change(&self, pos: BlockPos, old: i32, new: &mut i32) {
        if self.native_authoritative {
            self.native_block_redstone(self.dimension, pos, old, new);
        }
    }
    fn allow_spread(
        &self,
        kind: horizon_world::level::Spread,
        from: BlockPos,
        to: BlockPos,
    ) -> bool {
        let mut canceled = false;
        if self.native_authoritative {
            let kind = match kind {
                horizon_world::level::Spread::Fluid => "fluid",
                horizon_world::level::Spread::Fire => "fire",
            };
            self.native_block_spread(self.dimension, kind, from, to, &mut canceled);
        }
        !canceled
    }
    fn allow_fluid_level_change(
        &self,
        pos: BlockPos,
        fluid: &'static str,
        from: i32,
        to: i32,
    ) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_fluid_level_change(self.dimension, pos, fluid, from, to, &mut cancel);
        }
        !cancel
    }
    fn allow_block_ignite(&self, pos: BlockPos, cause: &'static str) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_block_ignite(self.dimension, self.native_player, pos, cause, &mut cancel);
        }
        !cancel
    }
    fn allow_block_burn(&self, pos: BlockPos) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_block_burn(self.dimension, pos, &mut cancel);
        }
        !cancel
    }
    fn allow_leaves_decay(&self, pos: BlockPos) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_leaves_decay(self.dimension, pos, &mut cancel);
        }
        !cancel
    }
    fn allow_moisture_change(&self, pos: BlockPos, from: i32, to: i32) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_moisture_change(self.dimension, pos, from, to, &mut cancel);
        }
        !cancel
    }
    fn observe_sponge_absorb(&self, pos: BlockPos) {
        if self.native_authoritative {
            self.native_sponge_absorb(self.dimension, pos);
        }
    }
    fn allow_block_fade(&self, pos: BlockPos, from: BlockStateId, to: BlockStateId) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_block_fade(self.dimension, pos, from, to, &mut cancel);
        }
        !cancel
    }
    fn allow_block_form(&self, pos: BlockPos, from: BlockStateId, to: BlockStateId) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_block_form(self.dimension, pos, from, to, &mut cancel);
        }
        !cancel
    }
    fn note_block_change(
        &self,
        pos: BlockPos,
        instrument: &str,
        old: i32,
        mut new: i32,
    ) -> Option<i32> {
        let mut cancel = false;
        if self.native_authoritative {
            let context = horizon_client::mod_api::NativeNoteBlockChange {
                dimension: self.dimension,
                pos,
                instrument,
                old_note: old,
            };
            self.native_note_block_change(&context, &mut new, &mut cancel);
        }
        (!cancel).then_some(new)
    }
    fn allow_note_play(&self, pos: BlockPos, instrument: &str, note: i32) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_note_play(
                self.dimension,
                self.native_player,
                pos,
                instrument,
                note,
                &mut cancel,
            );
        }
        !cancel
    }
    fn allow_bell_ring(&self, pos: BlockPos) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_bell_ring(self.dimension, self.native_player, pos, &mut cancel);
        }
        !cancel
    }
    fn allow_cauldron_level_change(
        &self,
        pos: BlockPos,
        from: i32,
        to: i32,
        reason: &'static str,
    ) -> bool {
        let mut cancel = false;
        if self.native_authoritative {
            self.native_cauldron_level_changed(
                self.dimension,
                self.native_player,
                pos,
                from,
                to,
                reason,
                &mut cancel,
            );
        }
        !cancel
    }
    fn allow_piston_move(
        &self,
        piston: BlockPos,
        direction: horizon_core::geometry::Direction,
        push: &[BlockPos],
        destroy: &[BlockPos],
    ) -> bool {
        let mut canceled = false;
        if self.native_authoritative {
            let step = direction.normal();
            self.native_piston_move(self.dimension, piston, step, push, destroy, &mut canceled);
        }
        !canceled
    }
    fn adjust_signal_source(&self, state: BlockStateId, vanilla: bool) -> bool {
        let mut source = vanilla;
        if self.native_authoritative {
            self.native_is_signal_source(state, &mut source);
        }
        source
    }
    fn adjust_signal(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        direct: bool,
        vanilla: i32,
    ) -> i32 {
        let mut signal = vanilla;
        if self.native_authoritative {
            let face = direction.opposite();
            self.native_block_signal(self.dimension, pos, state, face, direct, &mut signal);
        }
        signal.clamp(0, 15)
    }
    fn adjust_has_analog_output(&self, state: BlockStateId, vanilla: bool) -> bool {
        let mut output = vanilla;
        if self.native_authoritative {
            self.native_has_analog_output(state, &mut output);
        }
        output
    }
    fn adjust_analog_output(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        vanilla: i32,
    ) -> i32 {
        let mut output = vanilla;
        if self.native_authoritative {
            self.native_analog_output(self.dimension, pos, state, direction, &mut output);
        }
        output.clamp(0, 15)
    }
    fn before_random_tick(
        &mut self,
        ticks: &mut Ticks,
        pos: BlockPos,
        state: &mut BlockStateId,
        handled: &mut bool,
    ) {
        if self.native_authoritative {
            // Any block the hook writes moves the world's revision on.
            let revision = self.world.revision;
            self.native_random_tick(ticks, self.dimension, pos, *state, handled);
            if !*handled && self.world.revision != revision {
                *state = self.block_state(pos);
            }
        }
    }
    fn block_grow(
        &mut self,
        pos: BlockPos,
        from: BlockStateId,
        to: BlockStateId,
        cancel: &mut bool,
    ) {
        if self.native_authoritative {
            let name = |s: BlockStateId| self.facts.get(s).name.to_string();
            let (from, to) = (name(from), name(to));
            self.native_block_grow(
                native_hooks::dimension_id(self.dimension),
                [pos.x, pos.y, pos.z],
                &from,
                &to,
                cancel,
            );
        }
    }
    fn before_tick_block(
        &mut self,
        ticks: &mut Ticks,
        pos: BlockPos,
        state: &mut BlockStateId,
        handled: &mut bool,
    ) {
        if self.native_authoritative {
            // Any block the hook writes moves the world's revision on.
            let revision = self.world.revision;
            self.native_tick_block(ticks, self.dimension, pos, *state, handled);
            if !*handled && self.world.revision != revision {
                *state = self.block_state(pos);
            }
        }
    }
    fn before_update_shape(
        &self,
        ticks: &mut Ticks,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        neighbour_pos: BlockPos,
        neighbour: BlockStateId,
        new: &mut Option<BlockStateId>,
    ) {
        if self.native_authoritative {
            self.native_update_shape(
                ticks,
                self.dimension,
                pos,
                state,
                direction,
                neighbour_pos,
                neighbour,
                new,
            );
        }
    }
    #[allow(clippy::option_option)]
    fn before_state_for_placement(
        &self,
        state: BlockStateId,
        context: &PlacementContext,
        fluid: FluidState,
        answer: &mut Option<Option<BlockStateId>>,
    ) {
        if self.native_authoritative {
            self.native_state_for_placement(self.dimension, state, context, fluid, answer);
        }
    }
    fn before_can_survive(&self, pos: BlockPos, state: BlockStateId, survives: &mut Option<bool>) {
        if self.native_authoritative {
            self.native_can_survive(self.dimension, pos, state, survives);
        }
    }
    fn before_may_place_on(
        &self,
        state: BlockStateId,
        ground: BlockStateId,
        ground_pos: BlockPos,
        may: &mut Option<bool>,
    ) {
        if self.native_authoritative {
            self.native_may_place_on(self.dimension, state, ground, ground_pos, may);
        }
    }
    fn is_client_side(&self) -> bool {
        self.client_side
    }
    fn is_valid_bonemeal_target(&self, pos: BlockPos, state: BlockStateId) -> Option<bool> {
        // Asked on both sides, as vanilla asks `isValidBonemealTarget`: a
        // client predicts the use with it. Only a mod answers.
        let mut valid = None;
        self.native_is_valid_bonemeal_target(self.dimension, pos, state, &mut valid);
        valid
    }
    fn is_bonemeal_success(&mut self, pos: BlockPos, state: BlockStateId) -> bool {
        let mut success = false;
        if self.native_authoritative {
            self.native_is_bonemeal_success(self.dimension, pos, state, &mut success);
        }
        success
    }
    fn perform_bonemeal(&mut self, ticks: &mut Ticks, pos: BlockPos, state: BlockStateId) {
        if self.native_authoritative {
            self.native_perform_bonemeal(ticks, self.dimension, pos, state);
        }
    }
    fn before_fire_odds(&self, state: BlockStateId, odds: &mut Option<(i32, i32)>) {
        if self.native_authoritative {
            self.native_fire_odds(self.dimension, state, odds);
        }
    }
    fn before_smoke_source(&self, state: BlockStateId, smoke: &mut Option<bool>) {
        if self.native_authoritative {
            self.native_smoke_source(self.dimension, state, smoke);
        }
    }
    fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.world.world_surface(x, z, |s| self.facts.is_air(s))
    }
    fn precipitation(&self, pos: BlockPos) -> Option<horizon_world::level::Precipitation> {
        self.behaviours.precipitation_info(self, pos)
    }
    fn is_raining_at(&self, pos: BlockPos) -> bool {
        self.behaviours.is_raining_at(self, pos)
    }
    /// `getEntitiesWithContainerOpen` is not empty: a player or guest has the
    /// container open, or a copper golem has the chest open
    /// (`CopperGolem.hasContainerOpen`).
    fn container_open(&self, pos: BlockPos) -> bool {
        let key = (pos.x, pos.y, pos.z);
        self.open_containers.contains(&key) || self.golem_chests.contains(&key)
    }
    /// `ContainerOpenersCounter.recheckOpeners` for the block entity at `pos`,
    /// through `ChestBlock.tick`, `EnderChestBlock.tick` and `BarrelBlock.tick`:
    /// the count a recount of the players with that container open finds, and
    /// the count it replaces. Only the block entities that keep a counter and
    /// a scheduled recheck answer; anything else is `None`.
    fn recheck_container_openers(&mut self, pos: BlockPos) -> Option<(i32, i32)> {
        use horizon_world::level::classes::Class;
        let data = self.behaviours.data(self.world.block(pos.x, pos.y, pos.z));
        if !(data.is(Class::Chest) || data.is(Class::EnderChest) || data.is(Class::Barrel)) {
            return None;
        }
        Some(crate::game::block_entity_anims::recheck_opener_count(
            self.lid_openers,
            self.open_containers,
            (pos.x, pos.y, pos.z),
        ))
    }

    fn block_entity_analog_signal(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
    ) -> i32 {
        if let Some(s) = self.registry.state(state)
            && s.name == "minecraft:detector_rail"
        {
            return self.detector_rail_signal(
                pos,
                s.properties.get("powered").is_some_and(|v| v == "true"),
            );
        }
        if let Some(signal) = command_blocks::analog_signal(self.world, (pos.x, pos.y, pos.z)) {
            return signal;
        }
        if let Some(signal) = sculk::sensor_signal(self.world, self.behaviours, state, pos) {
            return signal;
        }
        if let Some((_, comparator)) = jukebox::jukebox_signals(self.world, self.items, pos) {
            return comparator;
        }
        if let Some(signal) = creaking::heart_signal(self.world, pos) {
            return if self.registry.state(state).is_some_and(|s| {
                s.properties
                    .get("creaking_heart_state")
                    .is_some_and(|v| v == "uprooted")
            }) {
                0
            } else {
                signal
            };
        }
        if let Some(signal) = books::lectern_signal(self.world, pos) {
            return signal;
        }
        if let Some(signal) =
            machines::shelf_signal(self.world, self.registry, state, pos, direction)
        {
            return signal;
        }
        machines::container_signal(self.world, self.behaviours, self.items, self.registry, pos)
    }
    fn item_frame_analog_output(&self, pos: BlockPos, facing: Direction) -> Option<i32> {
        let mut frames = self
            .item_frames
            .iter()
            .filter(|f| f.pos == [pos.x, pos.y, pos.z] && f.facing == facing as u8);
        let frame = frames.next().filter(|_| frames.next().is_none())?;
        // ItemFrame.getAnalogOutput.
        Some(
            frame
                .item
                .as_ref()
                .map_or(0, |_| i32::from(frame.rotation % 8) + 1),
        )
    }
    fn block_entity_signal(
        &self,
        pos: BlockPos,
        _state: BlockStateId,
        _direction: Direction,
    ) -> i32 {
        // TestBlock.ownSignal.
        if let Some(BlockEntity::TestBlock(b)) =
            self.world.block_entities.get(&(pos.x, pos.y, pos.z))
        {
            return b.own_signal();
        }
        // TrappedChestBlock.getSignal: the players with it open, at most 15.
        if let Some(count) = self.lid_openers.get(&(pos.x, pos.y, pos.z)) {
            return (*count).clamp(0, 15);
        }
        jukebox::jukebox_signals(self.world, self.items, pos).map_or(0, |(signal, _)| signal)
    }
    fn biome_name(&self, pos: BlockPos) -> Option<&str> {
        let chunk = self.world.chunks.get(&(pos.x >> 4, pos.z >> 4))?;
        Some(&self.biomes.get(chunk.biome(pos.x, pos.y, pos.z)).name)
    }
    fn world_seed(&self) -> i64 {
        self.seed
    }
    fn day_time(&self) -> i64 {
        self.world.day_time
    }
    fn game_rules(&self) -> horizon_world::level::GameRules {
        self.rules
    }
    fn difficulty(&self) -> i32 {
        self.difficulty
    }
    /// `ServerLevel.canSpreadFireAround`: a player (not a spectator) closer
    /// than the `fire_spread_radius_around_player` rule to the block's corner
    /// (`ChunkMap.anyPlayerCloseEnoughTo`); -1 spreads everywhere.
    fn can_spread_fire_around(&self, pos: BlockPos) -> bool {
        let radius = self.rules.fire_spread_radius_around_player;
        radius == -1
            || self.fire_players.iter().any(|p| {
                let d = [
                    p[0] - f64::from(pos.x),
                    p[1] - f64::from(pos.y),
                    p[2] - f64::from(pos.z),
                ];
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < f64::from(radius) * f64::from(radius)
            })
    }
    fn increased_fire_burnout(&self, pos: BlockPos) -> bool {
        self.world
            .chunks
            .get(&(pos.x >> 4, pos.z >> 4))
            .is_some_and(|chunk| {
                self.biomes
                    .get(chunk.biome(pos.x, pos.y, pos.z))
                    .increased_fire_burnout
            })
    }
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.world.block(pos.x, pos.y, pos.z)
    }
    fn set_raw(&mut self, pos: BlockPos, state: BlockStateId) -> Option<BlockStateId> {
        // `Level.setBlock`: a server level of the Debug preset never changes.
        if self.debug && !self.client_side {
            return None;
        }
        if !self.world.loaded(pos.x, pos.z)
            || pos.y < self.world.bottom_y()
            || pos.y >= self.world.top_y()
        {
            return None;
        }
        let previous = self.world.block(pos.x, pos.y, pos.z);
        self.world.set(pos.x, pos.y, pos.z, state).ok()?;
        Some(previous)
    }
    fn is_loaded(&self, pos: BlockPos) -> bool {
        self.world.loaded(pos.x, pos.z)
    }
    fn min_y(&self) -> i32 {
        self.world.shape.min_y
    }
    fn max_y(&self) -> i32 {
        // Exclusive, as LevelAccess::max_y documents.
        self.world.shape.max_y()
    }
    fn random(&mut self) -> &mut horizon_core::random::LegacyRandom {
        &mut self.world.random
    }
    fn game_time(&self) -> i64 {
        self.world.game_time
    }
    fn block_light(&self, pos: BlockPos) -> i32 {
        i32::from(self.world.light(pos.x, pos.y, pos.z) & 15)
    }
    fn sky_light(&self, pos: BlockPos) -> i32 {
        i32::from(self.world.light(pos.x, pos.y, pos.z) >> 4)
    }
    fn sky_darken(&self) -> i32 {
        self.darken
    }
    fn sun_angle(&self, _pos: BlockPos) -> f32 {
        self.environment
            .sample(&horizon_client::environment::Sample {
                day_time: self.world.day_time,
                ..Default::default()
            })
            .sun_angle
    }
    fn is_raining(&self) -> bool {
        self.raining
    }
    fn is_thundering(&self) -> bool {
        self.thundering
    }
    fn dimension(&self) -> LevelDimension {
        self.dimension
    }
    /// `Level.isUnobstructed`: the placed state's collision shape against the player.
    /// `Level.isUnobstructed(state, pos, CollisionContext.of(player))`: no
    /// entity that blocks building is in the state's collision shape.
    fn is_unobstructed(&self, pos: BlockPos, state: BlockStateId) -> bool {
        let player = self.player_collision.as_ref();
        let context = player.map_or_else(
            || CollisionContext::empty(false),
            |player| {
                CollisionContext::for_entity(
                    CollisionEntity {
                        identity: EntityIdentity(0),
                        kind: horizon_world::collision_context::EntityContextKind::Player,
                        y: player.y,
                        descending: player.descending,
                        main_hand: Some(CollisionItem {
                            item: &player.main_hand_item,
                            count: player.main_hand_count,
                        }),
                    },
                    false,
                )
            },
        );
        let mut scene = collisions::SceneCollisions {
            world: &*self.world,
            binding: self.collision,
            subject: player.map(|player| horizon_client::collisions::PowderSnowSubject {
                fall_distance: player.fall_distance,
                falling_block: false,
                leather_boots: player.leather_boots,
                walkable_type: false,
                lava_walker: false,
            }),
        };
        let Ok(shape) = self
            .collision
            .collision_shape_with_context(&mut scene, state, pos, &context)
        else {
            return false;
        };
        if shape.is_empty() {
            return true;
        }
        let shape = shape.moved(horizon_core::geometry::Vec3::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
        ));
        horizon_world::entity_obstruction::is_unobstructed(
            &mut BuildingObstruction(self.building_blocker_boxes()),
            None,
            &shape,
        )
        .unwrap_or(false)
    }
    /// `ServerLevel.shouldTickBlocksAt`: only the block-ticking ticket range.
    fn should_tick_blocks_at(&self, chunk: horizon_core::position::ChunkPos) -> bool {
        self.tickets.block_ticking(chunk.x, chunk.z)
    }
    /// `ServerLevel.isPositionTickingWithEntitiesLoaded`: block ticking, a
    /// ticking chunk, and its entities loaded.
    fn is_ticking_chunk(&self, chunk: horizon_core::position::ChunkPos) -> bool {
        self.tickets.block_ticking(chunk.x, chunk.z)
            && ticking_chunk(self.world, chunk.x, chunk.z)
            && !self.world.entities_pending.contains(&(chunk.x, chunk.z))
    }
    /// Entities that trigger pressure plates, tripwires, detector rails and
    /// wooden buttons: the players (the host and the LAN guests), the mobs,
    /// the minecarts and the other entities that don't ignore block triggers,
    /// and for `EntityQuery::Arrows` the arrows.
    fn count_entities(&self, area: Aabb, query: EntityQuery) -> usize {
        self.count_entities_for(area, query, |_| true)
    }
    /// The plate or tripwire `block` at `pos` is set off by a player only when
    /// the mods let them (`native_player_physical_trigger`).
    fn count_entities_at(
        &self,
        pos: BlockPos,
        block: &str,
        area: Aabb,
        query: EntityQuery,
    ) -> usize {
        self.count_entities_for(area, query, |player| {
            self.allow_physical_trigger(player, pos, block)
        })
    }
    fn emit(&mut self, event: LevelEvent) {
        match event {
            // Block.dropResources draws its loot and spots right away.
            // `popResource` only drops server-side (`!level.isClientSide`):
            // a client's moveBlocks (a piston's block event) skips it.
            LevelEvent::DropResources { pos, state } if !self.client_side => {
                self.drop_resources_now(pos, state)
            }
            LevelEvent::DropResources { .. } => {}
            // `ClientLevel.playSeededSound` plays only when the excluded
            // player is this client's own: the block's `playSound(null, ...)`
            // (a piston's move, a note) reaches it from the server instead.
            LevelEvent::Sound { .. } if self.client_side => {}
            // NoteBlock.triggerEvent plays the head's sound with a null
            // player, so only the server's level plays it; a guest or a
            // remote client hears it from the sound packet.
            LevelEvent::BlockEvent {
                kind: "note_custom_sound",
                ..
            } if self.client_side => {}
            // Block.popResource of the block's own item (tall flowers, flower beds).
            LevelEvent::BlockEvent {
                pos,
                state,
                kind: "pop_block_item",
            } => self.pop_block_item(pos, state),
            // ComposterBlock.extractProduce: bone meal above the composter.
            LevelEvent::Player {
                pos,
                effect: PlayerEffect::DropItem(item),
                ..
            } => self.pop_above(pos, item),
            // Picking a sweet berry bush or cave vines: the harvest loot,
            // popped before the pick sound draws its pitch.
            LevelEvent::Player {
                pos,
                state,
                effect: PlayerEffect::InteractLoot(table),
            } => self.drop_interact_loot(pos, state, table),
            event => self.events.push(event),
        }
    }
    fn has_mod_block_entity(&self, pos: BlockPos) -> bool {
        // Asked on every block change: no lookup until a mod block entity exists.
        self.world.block_entities.has_mods()
            && matches!(
                self.world.block_entities.get(&(pos.x, pos.y, pos.z)),
                Some(BlockEntity::Mod(_))
            )
    }
    /// `ShulkerBoxBlockEntity.isClosed`: the lid state the block-entity tick
    /// copied from `be_anims` (`ShulkerBoxBlock.getBlockSupportShape`).
    fn shulker_is_closed(&self, pos: BlockPos) -> Option<bool> {
        self.world
            .shulkers
            .get(&[pos.x, pos.y, pos.z])
            .map(|s| s.status == horizon_client::block_entity_anim::ShulkerStatus::Closed)
    }
    fn remove_block_entity(&mut self, pos: BlockPos, old: BlockStateId, side_effects: bool) -> u8 {
        self.remove_entity(pos, old, side_effects)
    }
    fn load_block_entity(&mut self, pos: BlockPos, mut nbt: horizon_world::nbt::Compound) {
        use horizon_world::nbt::Tag;
        for (k, v) in [("x", pos.x), ("y", pos.y), ("z", pos.z)] {
            nbt.insert(k.into(), Tag::Int(v));
        }
        let tag = Tag::Compound(nbt);
        if let Some((key, entity)) = horizon_client::save::decode_block_entity(&tag) {
            self.world.block_entities.insert(key, entity);
            if tag.get("LootTable").is_some() {
                self.emit(LevelEvent::ContainerLootTable { pos, tag });
            }
        }
    }
    fn create_feature_spawner(&mut self, pos: BlockPos, nbt: horizon_world::nbt::Compound) {
        // Guard at the owner/insert boundary as well as the live feature sink.
        // Existing entries of every class retain the prior no-op behavior.
        if self
            .registry
            .state(self.world.block(pos.x, pos.y, pos.z))
            .is_none_or(|state| state.name != "minecraft:spawner")
            || self
                .world
                .block_entities
                .contains_key(&(pos.x, pos.y, pos.z))
        {
            return;
        }
        self.load_block_entity(pos, nbt);
    }
    fn set_command_block_powered(&mut self, pos: BlockPos, powered: bool) -> Option<(bool, bool)> {
        command_blocks::set_powered(self.world, self.registry, (pos.x, pos.y, pos.z), powered)
    }
    /// `PotentSulfurBlockEntity.resetCountdown`.
    fn reset_potent_sulfur_countdown(&mut self, pos: BlockPos) {
        if let Some(BlockEntity::PotentSulfur(sulfur)) =
            self.world.block_entities.get_mut(&(pos.x, pos.y, pos.z))
        {
            sulfur.waiting_countdown = -1;
        }
    }
}
/// FallingBlockEntity carrying a block state.
#[derive(Clone, Debug)]
struct FallingBlock {
    /// `Entity.yRot` and `xRot` (`/tp`, `/data get ... Rotation`).
    pub rotation: [f32; 2],
    /// When it joined its list (`horizon_client::join_seq`); set by the list.
    pub(super) join: horizon_client::join_seq::JoinSeq,
    /// Its entity id: from the entity counter on the server, the server's on
    /// a client.
    id: u32,
    state: BlockStateId,
    /// `DATA_START_POS`: spawn position used to select weighted block models.
    start_pos: BlockPos,
    position: Vec3,
    previous: Vec3,
    velocity: Vec3,
    /// `FallingBlockEntity.time`: signed, a save can start it below zero.
    age: i32,
    /// `Entity.isNoGravity`.
    no_gravity: bool,
    /// `FallingBlockEntity.tick` handles portals after moving, before landing.
    portal: horizon_client::nether_portal::PortalState,
    /// `FallingBlockEntity.dropItem`.
    drop_item: bool,
    /// `Entity.fallDistance` (powder snow holds a block that fell far at 0.9).
    fall_distance: f64,
    /// `Entity.onGround`: whether its last move was stopped going down.
    on_ground: bool,
    /// `Entity.stuckSpeedMultiplier`: a cobweb it is in scales its next move.
    stuck: Vec3,
    damage: horizon_client::falling_block::Damage,
    /// The UUID and compound it was loaded with, kept while it moves.
    saved: Option<Box<([i32; 4], horizon_world::nbt::Tag)>>,
}
impl FallingBlock {
    /// Where `FallingBlockRenderer` draws the block's model corner, `alpha`
    /// into the tick: centred on the entity, then moved by `offset`, the
    /// block's random model offset (`ModelBlockRenderer.tesselateBlock`:
    /// `BlockState.getOffset` of `BlockPos.containing(x, maxY, z)`), so a
    /// pointed dripstone keeps the place it hung at while it falls.
    fn model_origin(
        &self,
        alpha: f64,
        offset: impl Fn(BlockStateId, i32, i32) -> Option<[f64; 3]>,
    ) -> glam::DVec3 {
        let [dx, dy, dz] = offset(
            self.state,
            self.position.x.floor() as i32,
            self.position.z.floor() as i32,
        )
        .unwrap_or([0.0; 3]);
        glam::DVec3::new(
            self.previous.x + (self.position.x - self.previous.x) * alpha - 0.5 + dx,
            self.previous.y + (self.position.y - self.previous.y) * alpha + dy,
            self.previous.z + (self.position.z - self.previous.z) * alpha - 0.5 + dz,
        )
    }
}

/// A block entity's `mark_motion` id: its position, packed into the 32 bits beside the tag
/// (x and z wrap at 2048 blocks and y at 1024; two block entities that far apart in one frame's
/// view are told apart by their vertex counts, as `PackEntities` matches only equal runs).
fn motion_block_entity(pos: [i32; 3]) -> u64 {
    motion_at(Game::MOTION_BLOCK_ENTITY, pos)
}

/// `tag` with a block position packed into its low 32 bits (`motion_block_entity`).
fn motion_at(tag: u64, pos: [i32; 3]) -> u64 {
    let [x, y, z] = pos.map(|c| c as u32);
    tag | u64::from((x & 0x7FF) | (z & 0x7FF) << 11 | (y & 0x3FF) << 22)
}

/// `FallingBlockRenderer.shouldRender` drops a falling block whose own state is
/// the state of the cell it is in (`level.getBlockState(entity.blockPosition())`):
/// a `NoGravity` block, or one that has just landed, sits inside an identical
/// block, and vanilla draws no model for it.
fn falling_block_visible(state: BlockStateId, cell: BlockStateId) -> bool {
    state != cell
}

/// The data loaded once at startup and never changed: blocks, items,
/// recipes, biomes, mobs, collision and motion. Read-only and `Send + Sync`,
/// so the client and an integrated server thread can share one copy.
pub struct Resources {
    core: WorldCore,
    rest: std::sync::OnceLock<WorldRest>,
}

struct WorldCore {
    facts: BlockFacts,
    biomes: Arc<Biomes>,
    environment: Environment,
    collision: Arc<BlockCollisionRegistry<'static>>,
    motion: MotionResources,
    suffocation: horizon_world::suffocation::SuffocationRegistry<'static>,
    threads: usize,
}

struct WorldRest {
    items: Items,
    recipes: Recipes,
    transformers: horizon_client::block_transform::Transformers,
    mob_types: HashMap<String, Arc<MobType>>,
    mob_data: horizon_client::mobs::MobData,
    providers: Providers,
    block_entity_render: BlockEntityRenderer,
    animate: horizon_client::animate_tick::AnimateTables,
    fog: horizon_client::fog::FogTables,
    music: horizon_client::music::MusicTables,
    ambient: horizon_client::ambience::AmbientTables,
    nether_environment: Environment,
    end_environment: Environment,
    behaviours: Behaviours,
    /// The player model's right and left arms.
    player_arm: [Option<horizon_client::mobs::Part>; 2],
    /// The slim model's right and left arms (3 pixels wide).
    player_arm_slim: [Option<horizon_client::mobs::Part>; 2],
    player_type: Option<Arc<horizon_client::mobs::MobType>>,
    /// `SpinAttackEffectModel` (Riptide's spinning boxes).
    spin_attack: Option<horizon_client::mobs::Part>,
    stations: horizon_client::stations::StationData,
}

/// The content tables a world's data packs change, loaded when the world
/// opens and on `/reload`: `ReloadableServerResources` loads a server's
/// recipes, loot tables and tags from the world's pack stack, over the
/// resource root's own. Worlds without packs use `Resources.rest` directly.
struct WorldTables {
    suffocation: horizon_world::suffocation::SuffocationRegistry<'static>,
    /// The pack files the tables were built from (a native mod's among
    /// them), for the advancement tree reload and the loot data.
    files: Vec<(String, Vec<Vec<u8>>)>,
    items: Items,
    recipes: Recipes,
    stations: horizon_client::stations::StationData,
    /// The values of the data map types native mods registered
    /// (`native_data_map_types`).
    data_maps: native_data_maps::DataMaps,
    /// The entity type, biome, structure and damage type tags with the packs'
    /// over the game's own, by registry (`TagLoader.loadTagsForExistingRegistries`):
    /// what the predicates' `#tag`s of those registries read.
    pack_tags: HashMap<&'static str, HashMap<String, HashSet<String>>>,
}

impl WorldTables {
    /// The registries [`WorldTables::pack_tags`] holds.
    const PACK_TAG_REGISTRIES: [&'static str; 4] = [
        "entity_type",
        "worldgen/biome",
        "worldgen/structure",
        "damage_type",
    ];
}

/// The equipment slot of `armor[index]`: head, chest, legs, feet.
pub(super) fn armor_slot(index: usize) -> horizon_world::player_inventory::EquipmentSlot {
    use horizon_world::player_inventory::EquipmentSlot;
    match index {
        0 => EquipmentSlot::Head,
        1 => EquipmentSlot::Chest,
        2 => EquipmentSlot::Legs,
        _ => EquipmentSlot::Feet,
    }
}

/// Everything the title and menus draw: the GUI atlas with its font and the
/// title panorama. Kept apart from the world bundle so the title can appear
/// before the block sprites, models and gameplay tables are ready.
pub struct UiResources {
    /// The GUI, font, particle, map and skin atlas.
    pub atlas: HudAtlas,
    /// The title screen's panorama faces.
    pub panorama: Arc<horizon_client::gfx::panorama::Faces>,
}
/// What a resource pack reload replaces (`Minecraft.reloadResourcePacks`):
/// block models and sprites with their meshing tables, item models,
/// translations and the textures read on the CPU. Each game holds its own;
/// cloning shares the loaded data.
#[derive(Clone)]
pub struct PackResources {
    assets: Arc<Assets>,
    render: Arc<RenderAssets>,
    context: Arc<WorkerContext>,
    layer_names: Arc<HashMap<u32, String>>,
    /// Item definitions, baked item models and special renderers.
    item_renderer: Arc<horizon_client::item_render::ItemRenderer>,
    names: Arc<Names>,
    /// clouds.png: (width, height, filled cells).
    clouds: Arc<(u32, u32, Vec<bool>)>,
    /// misc/forcefield.png: (width, height, RGBA) for the world border wall.
    forcefield: Arc<(u32, u32, Vec<u8>)>,
    /// OptiFine custom sky layers (`optifine/sky`) with their textures; empty without them.
    custom_sky: Arc<horizon_client::custom_sky::CustomSky>,
    /// The overworld's, Nether's and End's attribute stacks with the packs' Polytone biome and
    /// dimension modifiers (#11486), when a pack has some: what the frame draws.
    environments: Option<Arc<[Environment; 3]>>,
    /// The packs' Polytone custom particles and block emitters (#11486), when a pack has some.
    polytone_particles: Option<Arc<horizon_client::particles::polytone::Registry>>,
    /// The packs' Polytone post chains (#11486), when a pack has some.
    polytone_post: Option<Arc<horizon_client::polytone_post::PolytonePost>>,
    /// The packs' Polytone custom colours (`polytone/colors.json`).
    polytone_colors: Arc<horizon_client::polytone_colors::PackColors>,
}
/// A quick craft drag as the screen draws it: each dragged slot's count and
/// whether the slot caps it (None: hidden), and what stays in hand.
struct DragView {
    slots: HashMap<usize, Option<(u32, bool)>>,
    left: Option<u32>,
}
/// Whether an item modifier of slot `group` applies in the worn or held `slot`
/// (`EquipmentSlotGroup.test`).
fn equipment_modifier_applies(group: &str, slot: &str) -> bool {
    group == slot
        || group == "any"
        || (group == "armor" && slot != "mainhand" && slot != "offhand")
        || (group == "hand" && (slot == "mainhand" || slot == "offhand"))
}
/// Ctrl+Q reads the physical keyboard, independently of sprint key mappings.
/// A chunk whose ticking future can complete (`ChunkMap.prepareTickingChunk`):
/// lit, with its neighbours loaded, as `postprocess_chunks` waits for.
fn ticking_chunk(world: &World, x: i32, z: i32) -> bool {
    let Some(center) = world.chunks.get(&(x, z)) else {
        return false;
    };
    center.lit && world.chunks.surrounded((x, z))
}
/// `TicketStorage.canTicketExpire` for a `PORTAL` ticket: its chunk's holder is
/// ready for saving once the futures of its level are done, the chunk lit and
/// its neighbours loaded out to one chunk at a ticking level
/// (`prepareTickingChunk`) and two at an entity-ticking one
/// (`prepareEntityTickingChunk`). The ticket's own level stands for the
/// holder's: the area it waits for is the one its own streaming loads.
fn portal_ticket_can_expire(
    world: &World,
    ticket: &horizon_client::ticket_levels::PortalTicket,
) -> bool {
    use horizon_client::ticket_levels::{BLOCK_TICKING, ENTITY_TICKING};
    let r = match ticket.level {
        l if l <= i32::from(ENTITY_TICKING) => 2,
        l if l == i32::from(BLOCK_TICKING) => 1,
        _ => 0,
    };
    let (x, z) = ticket.chunk;
    world.chunks.get(&(x, z)).is_some_and(|c| c.lit)
        && (-r..=r).all(|dz| (-r..=r).all(|dx| world.chunks.contains_key(&(x + dx, z + dz))))
}
/// Key events carry the scancode's US-layout code, which ignores XKB options like
/// `caps:swapescape`. For the keys whose meaning the keymap may remap, take the code
/// from the keysym the keymap resolved instead. This includes function keys on
/// remapped media rows; ordinary character bindings still use their physical key.
fn follow_keymap(event: &mut WindowEvent) {
    if let WindowEvent::KeyboardInput { event, .. } = event {
        follow_named_keymap(&event.logical_key, &mut event.physical_key);
    }
}

#[cfg(test)]
mod keymap_tests;

fn select_creative_tab(
    creative: &mut Creative,
    clicks: &mut horizon_client::container_screen::Clicks,
    tab: usize,
) {
    creative.select(tab);
    *clicks = Default::default();
    // Tabs are outside the panel; their release must not drop a carried stack.
    clicks.skip_release();
}

/// The icon the Select World list shows for the world in play
/// (`GameRenderer.tryTakeScreenshotIfNeeded`): its path in the save, and when
/// the last frame grab was asked for.
struct WorldIcon {
    path: PathBuf,
    last_attempt: Option<Instant>,
}

/// Temporary block-contact collections rebuilt on every server tick. Keeping
/// their backing storage on `Game` avoids reallocating them while the world is
/// ticking; the wrapper around `tick_blocks` restores the scratch even when a
/// phase returns an error.
#[derive(Default)]
struct TickScratch {
    ticking: Vec<(i32, i32)>,
    inside: Vec<BlockPos>,
    standing: Vec<(BlockPos, bool, f64)>,
    boat_cells: Vec<BlockPos>,
    touched: Vec<BlockPos>,
    sensing: Vec<BlockPos>,
    rest: Vec<BlockPos>,
    later: Vec<BlockPos>,
}

struct Game<'a> {
    /// Shared with an integrated server; `collision` and `motion` borrow from it.
    res: Arc<Resources>,
    rest_loader: Option<WorldRestLoader>,
    pending_entry: Option<PendingWorldEntry>,
    /// `DebugEntryChunkGeneration`'s `lastPos` and `result`: F3's `Density` line is sampled
    /// again only when the feet block (or the dimension) changes.
    debug_density: RefCell<debug_keys::DensityCache>,
    pending_pack_reload: bool,
    /// HDR, Smooth Gradients, picked path and outcome at the last frame attempt, including a failed
    /// replacement (`follow_frame`).
    frame_attempted: render_settings::FrameAttempt,
    /// When `resourcepacks/` was last looked at for the open pack screen, and what it held
    /// (`watch_pack_folder`).
    pack_folder: Option<(
        std::time::Instant,
        Vec<(String, u64, Option<std::time::SystemTime>)>,
    )>,
    /// A pack reload staged on a worker (`LoadingOverlay`): the frame draws
    /// its progress until `drive_pack_reload` applies it.
    pack_reload: Option<packs::PackReload>,
    /// A server's resource packs (game/server_packs.rs).
    server_packs: server_packs::ServerPackState,
    /// The link a chat click asks to open (`ConfirmLinkScreen`).
    link_prompt: Option<String>,
    /// A command the player didn't type, waiting for their answer
    /// (game/remote_chat.rs).
    command_prompt: Option<remote_chat::CommandPrompt>,
    /// A server's Code of Conduct to accept (game/code_of_conduct.rs).
    code_of_conduct: Option<code_of_conduct::Prompt>,
    dialogs: dialogs::DialogState,
    /// Single player's server while it loads its world (`Minecraft.doWorldLoad`).
    integrated_starting: Option<integrated::Starting>,
    world_renderer_ready: bool,
    collision: &'a BlockCollisionRegistry<'a>,
    motion: &'a MotionResources,
    /// This world's frozen damage definitions and reloadable tag snapshot.
    pub(super) health_resources: horizon_client::health::HealthResources,
    damage_tag_generation: u64,
    /// This game's resource packs (`reload_packs` replaces them).
    pack: PackResources,
    /// The title and menu bundle (GUI atlas, font and panorama).
    gui: UiResources,
    /// `FogRenderer`'s evolving state.
    fog: horizon_client::fog::FogState,
    screen_fx: horizon_client::screen_effects::ScreenEffects,
    /// `LocalPlayer.itemActivation`: the totem flying up to the screen.
    item_activation: horizon_client::item_activation::ItemActivation,
    weather_parameters: horizon_client::weather::ParameterCache,
    panorama_rotation: horizon_client::gfx::panorama::Rotation,
    world: World,
    /// The biome table the world plays with: the game's own with the world's
    /// packs over it (`Biomes::with_packs`), `None` before a world opens.
    world_biomes: Option<Arc<Biomes>>,
    /// `world_biomes` as the biome files made it, before `native_biome_colors`.
    world_biomes_base: Option<Arc<Biomes>>,
    streaming: Streaming,
    lod: Option<horizon_client::lod::Lod>,
    lod_source_attempted: bool,
    lod_observe: horizon_client::fast_hash::FxHashSet<(i32, i32)>,
    /// Columns the single-player server changed in bulk inside a mod's hold on the terrain
    /// (`native_world_hold`): far terrain takes them in when the hold lets go of them.
    lod_held: horizon_client::fast_hash::FxHashMap<(i32, i32), Arc<horizon_client::chunk::Chunk>>,
    /// The server level far terrain keeps the chunks of.
    lod_level: Option<String>,
    /// The server's BlueMap far terrain draws from, when the player named one
    /// or Find BlueMap found it.
    lod_blue_map: Option<Arc<horizon_client::bluemap::BlueMap>>,
    /// Beacon beam sections kept after their chunk unloads, so far terrain
    /// LOD still draws their beams (`beacon_geometry` refreshes them).
    far_beacons: HashMap<(i32, i32, i32), Vec<(u32, i32)>>,
    /// The detail revision, centre and radius `Lod::update` last had covered for.
    lod_covered_at: Option<(u64, (i32, i32), i32)>,
    /// Whether the view's columns have all arrived, and how new it is: large
    /// translucent pages wait for both before they shrink (`set_terrain_loading`).
    view_arrival: horizon_client::streaming::ViewArrival,
    options: Options,
    player: Option<PlayerMotion>,
    spawn: Option<Vec3>,
    clock: FixedStep,
    previous_position: Vec3,
    /// `yRotO` / `xRotO` of the local player: the view at the start of the tick,
    /// moved by `Entity.turn` and a boat's `clampRotation` as the view is
    /// (`LocalPlayer.getRopeHoldPosition` eases the held lead from it).
    rope_rot_old: (f32, f32),
    eye: f32,
    previous_eye: f32,
    yaw: f32,
    pitch: f32,
    inventory: Vec<Option<ItemStack>>,
    selected: usize,
    carried: Option<ItemStack>,
    craft_grid: Vec<Option<ItemStack>>,
    craft_size: usize,
    /// The crafting table the 3x3 grid belongs to (`CraftingMenu.access`).
    craft_table: Option<[i32; 3]>,
    /// A quick craft drag over inventory slots: (button, game slots so far).
    inv_drag: Option<(i32, Vec<usize>)>,
    /// `AbstractContainerScreen`'s quick craft and skip-release state.
    inv_clicks: horizon_client::container_screen::Clicks,
    /// The Mouse Tweaks extra's drag and wheel (`tweaks`).
    tweaks: tweaks::Tweaks,
    /// The wheel and hovered slot of `BundleMouseActions`.
    bundle_mouse: bundles::BundleMouse,
    /// The last inventory click, for the double click that gathers a stack.
    last_inv_click: Option<(std::time::Instant, Option<usize>, bool)>,
    /// `lastQuickMoved`: what the last Shift click moved, which a Shift
    /// double click moves every stack of.
    last_quick_moved: Option<ItemStack>,
    /// Open container block entity, if any.
    container: Option<(i32, i32, i32)>,
    /// The open container and the slots it had when it opened:
    /// `Container.stillValid` holds only while every block entity the menu
    /// took is still there, so a half of a double chest (or the container)
    /// going away closes it.
    open_container_slots: Option<((i32, i32, i32), usize)>,
    /// Open workstation menu (enchanting table, anvil, ...).
    station: Option<stations::Station>,
    /// Status effects and thrown potions (game/potions.rs).
    potions: potions::PotionState,
    scratch_slot: Option<ItemStack>,
    mining: Option<Mining>,
    /// The other players' cracks (`LevelRenderer.destroyingBlocks`): breaker id to the block,
    /// the stage (0..=9) and the game time it last changed.
    destroying: HashMap<i32, ([i32; 3], u8, i64)>,
    destroy_delay: u32,
    use_delay: u32,
    creative: bool,
    /// The game mode; `creative` mirrors `mode == Creative`.
    mode: horizon_client::command::args::GameMode,
    keys: horizon_client::held::HeldKeys,
    // Physical input is separate from remapped and toggled movement keys.
    physical_keys: HashSet<KeyCode>,
    physical_buttons: HashSet<MouseButton>,
    drag_button: Option<MouseButton>,
    /// Control + left click as a right click (macOS).
    right_click_emulation: horizon_client_input::mouse::RightClickEmulation,
    left: bool,
    right: bool,
    cursor: (f32, f32),
    /// The cursor icon the window shows (`widgets::take_cursor` each frame).
    cursor_icon: horizon_client::widgets::Cursor,
    captured: bool,
    /// Native grab mode used by winit. Linux window managers can drop the
    /// grab without a focus event, leaving `captured` true but stale.
    capture_mode: Option<CursorGrabMode>,
    /// The window held the pointer when it lost focus: the system lets a
    /// grabbed pointer go with the focus, and a window that comes back still
    /// wanting it takes it again (`MouseHandler.grabMouse`, whose grab
    /// outlives the window's focus).
    regrab_on_refocus: bool,
    /// Only the local player owning keyboard and mouse changes the native pointer.
    keyboard_mouse: bool,
    /// Offscreen rendering into a visible splitscreen window still accepts native input.
    split_window: bool,
    /// This view's corner in its shared window; cursor warps include the offset.
    view_at: [u32; 2],
    /// Splitscreen: what it draws over this player's view (its badge, questions, notices).
    split_hud: split::Hud,
    /// Splitscreen: this player's own Microsoft account session (one of the launcher's other
    /// accounts), in place of the process's (P1's) for its connections and skin.
    split_auth: Option<std::sync::Arc<horizon_client::auth::AuthSession>>,
    focused: bool,
    /// The visible window of a game an agent started, until a person focuses it: it opened
    /// without the focus, and its pointer grab stays inside the game as `--offscreen`'s does,
    /// so the person's pointer is never taken, hidden or moved.
    unclaimed_window: bool,
    /// The window's last focus change during a scripted run, applied once the run ends.
    focus_after_run: Option<bool>,
    /// Whether the window currently accepts input method events.
    ime_allowed: bool,
    /// The candidate window's area last given to the window, in pixels.
    ime_area: Option<[i32; 4]>,
    /// When the window lost focus, while it is still without
    /// (`Minecraft.lastActiveTime`: pausing waits 500 ms).
    focus_lost: Option<Instant>,
    /// The keyboard's modifier state (`KeyEvent.modifiers`).
    modifiers: winit::keyboard::ModifiersState,
    /// The window's fullscreen request and reported state.
    fullscreen: horizon_client::window::FullscreenState,
    /// The monitor whose video modes `settings.video_modes` holds.
    video_modes: horizon_client::window::VideoModeList,
    /// MouseHandler's accumulated look and wheel state.
    mouse: MouseLook,
    /// An agent moved the mouse (`mouse-move`) since the pointer was last
    /// grabbed or released: its motion turns the view like a grabbed pointer.
    agent_pointer: bool,
    /// `Options.smoothCamera` (cinematic camera), not persisted.
    smooth_camera: bool,
    /// Sneak, sprint, attack and use as ToggleKeyMappings.
    toggles: horizon_client::keymapping::ToggleKeys,
    /// A world screen with the pointer captured was open last frame.
    was_playing: bool,
    /// Minecraft.pause: a single-player screen that pauses the game is open.
    pause: bool,
    debug: bool,
    /// `horizon-ctl gpu-passes`: time Horizon's own passes on the GPU with F3 shut.
    gpu_passes: bool,
    screen: Screen,
    observed_screen: Option<native_hooks::ObservedScreen>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// Avoid an endless rebuild loop if the new device is lost as well.
    last_device_recovery: Option<Instant>,
    /// CPU geometry storage retained across frames, including empty/title frames.
    entity_vertices: Vec<Vertex>,
    /// Horizon Extras' dynamic lights this frame (`dynamic_lights.rs`), and how brightly each
    /// item glows (built the first time the extra is on).
    dynamic_lights: Vec<[f32; 4]>,
    luminance: std::cell::OnceCell<horizon_client::dynamic_light::Luminance>,
    /// Name tags drawn over everything (`RenderPipelines.TEXT_SEE_THROUGH`).
    see_through_vertices: Vec<Vertex>,
    /// The first-person player's model, which only a native pack's shadow pass draws.
    shadow_only_vertices: std::cell::RefCell<Vec<Vertex>>,
    /// Runs of `entity_vertices` by entity, for a shader pack's per-entity draws; recorded only
    /// while `pack_ids` is set (game/shader_packs.rs).
    entity_spans: std::cell::RefCell<Vec<horizon_client::gfx::pack_draw::EntitySpan>>,
    /// Which entity this frame's entity vertices belong to (`Frame::entity_motion`).
    entity_motion: std::cell::RefCell<Vec<(u32, u64)>>,
    /// The GPU-posed vertex count at each of `entity_motion`'s marks (`Frame::entity_motion_posed`).
    entity_motion_posed: std::cell::RefCell<Vec<u32>>,
    /// Mob models posed on the GPU (`Frame::skin`, `mobs::skin`): the models stored there,
    /// and this frame's draws.
    skin_models: std::cell::RefCell<horizon_client::mobs::skin::SkinModels>,
    skin_frame: std::cell::RefCell<horizon_client::gfx::entity_skin::SkinFrame>,
    /// The glowing entities' silhouettes this frame (game/glowing.rs).
    glowing_vertices: std::cell::RefCell<Vec<Vertex>>,
    /// The shader pack's entity and block entity ids, while one draws entities.
    pack_ids: Option<shader_packs::PackIds>,
    started: Instant,
    /// `SpectatorGui`: the menu a spectator opens with the hotbar keys.
    spectator_gui: horizon_client::spectator_menu::SpectatorGui,
    previous: Instant,
    /// When the last frame's work ended: what runs before the next one starts (`idle`, the
    /// event loop) shows in HORIZON_SPIKES as the time between frames.
    frame_end: Instant,
    /// FramerateLimitTracker.latestInputTime.
    last_input: Instant,
    /// Controllers (Horizon's own).
    pad: pad::Pads,
    frames: u64,
    /// The glint clock and options, read once at the start of each frame
    /// (game/item_draw.rs).
    glint_frame: horizon_client::item_render::GlintFrame,
    /// Mod block entities' geometry their mods kept
    /// (game/native_block_entity_draw.rs).
    kept_block_entities: std::cell::RefCell<native_block_entity_draw::KeptGeometry>,
    /// `item_level` for one frame, game time, day time and dimension.
    item_level_cache: std::cell::RefCell<
        Option<(
            (u64, i64, i64, &'static str),
            horizon_assets::item_model::LevelState,
        )>,
    >,
    /// Frames drawn with the player in the world, for `--world-frames`.
    world_frames: u64,
    fps: f32,
    fps_frames: u64,
    fps_at: Instant,
    ticks: u64,
    /// The sprite animation clock (`TextureManager.tick`): counts only the ticks the level
    /// runs normally, so `/tick freeze` holds it and `/tick step` moves it on.
    texture_ticks: u64,
    health_hud: hud_layout::HealthHud,
    /// What ends the game with an error; kept as it is, so main can tell a debug crash.
    error: Option<Box<dyn std::error::Error>>,
    message: Option<(String, Instant)>,
    save: Arc<SaveDir>,
    level: Level,
    /// Must precede session_lock: dropping the writer drains writes while the world is locked.
    player_autosaves: horizon_client::lan::playerdata::Autosaves,
    session_lock: Option<horizon_client::save::SessionLock>,
    audio: Audio,
    music: horizon_client::music::MusicManager,
    /// The tracks native mods play over the music (`native_music_layers`).
    music_layers: horizon_client::music::MusicLayers,
    /// `ToastManager.nowPlayingToast`: there while Music Toast is not Never.
    now_playing: Option<horizon_client::toasts::NowPlayingToast>,
    /// The Music Toast option the toast was last set up for.
    music_toast_state: u8,
    music_notes: horizon_client::toasts::MusicNotes,
    ambience: horizon_client::ambience::Ambience,
    /// The sound engine was told the game paused (pauseAllExcept).
    sound_paused: bool,
    /// Minecarts whose `MinecartSoundInstance` is playing.
    cart_sounds: HashSet<u32>,
    /// The minecart the riding sounds were started for (`LocalPlayer.startRiding`).
    riding_cart_sounds: Option<u32>,
    /// The happy ghast its riding sound was started for (`LocalPlayer.startRiding`).
    riding_sound: Option<u32>,
    /// `LocalPlayer.wasFallFlying` for the elytra's flying sound.
    was_fall_flying: bool,
    /// Bees whose buzz is playing, and whether it is the aggressive one.
    bee_sounds: HashMap<u32, bool>,
    /// Guardians whose attack hum is playing.
    guardian_sounds: HashSet<u32>,
    /// Remote guardian eye targets reused between ticks.
    guardian_look_scratch: Vec<(usize, [f64; 3])>,
    dropped: horizon_client::join_seq::JoinList<ItemEntity>,
    next_entity: u32,
    /// The last entity id when this level's entity pass began: an item with a
    /// later one was added during that pass and first ticks on the next
    /// (`EntityTickList`). `u32::MAX` until a tick has run.
    ticked_entities_through: u32,
    /// The items `tick_blocks` dropped while applying the entities' block
    /// contacts (a boat breaking a lily pad). Vanilla runs those in each
    /// entity's own tick (`applyEffectsFromBlocks`), inside the entity pass,
    /// so they too first tick on the next tick.
    contact_drops: std::ops::Range<u32>,
    particles: horizon_client::particles::ParticleEngine,
    /// `ItemPickupParticle`s: taken items, orbs and arrows flying to their collector.
    pickups: pickups::Pickups,
    /// Sign text being edited (`SignEditScreen`).
    sign_edit: Option<sign_edit::SignEditor>,
    /// An open book, book and quill or lectern screen.
    book: Option<books::BookScreen>,
    /// The last click on the book screen, for double clicks.
    book_last_click: Option<std::time::Instant>,
    /// Lecterns whose page-turn pulse is running (ticks left).
    lectern_pulses: Vec<((i32, i32, i32), i32)>,
    /// LevelLoadingScreen.smoothedProgress while the player is not placed yet.
    loading_progress: f32,
    /// `CreateWorldScreen.openCreateWorldScreen`'s data read, on a worker,
    /// while the title says `createWorld.preparing`.
    creation_job: Option<std::thread::JoinHandle<std::result::Result<(), String>>>,
    /// When `loading_progress` last eased (the server thread's load, 50 ms a step).
    loading_progress_at: Option<std::time::Instant>,
    /// A received packet started or ended a piston's move: `poll_remote`
    /// syncs the moving pistons once its packets are handled, not once per packet.
    moving_pistons_stale: bool,
    /// The world spawn search's origin chunk and fallback height, once known.
    spawn_origin: Option<((i32, i32), i32)>,
    spawn_spiral: horizon_client::world_spawn::ChunkSpiral,
    /// A new player's spawn search, while its candidates' chunks load.
    player_spawn: Option<horizon_client::world_spawn::PlayerSpawnFinder>,
    /// Chunks a respawn search waits for this tick, kept loading beside the
    /// player's own (taken by `update_world`).
    spawn_wait: Vec<(i32, i32)>,
    mobs: horizon_client::join_seq::JoinList<Mob>,
    /// `mobs` by network id, for the packets that name one (`game/mob_index.rs`).
    mob_index: mob_index::MobIndex,
    creatures: creatures::Creatures,
    sculk: sculk::SculkState,
    raiding: raids::Raiding,
    attack_ticks: u32,
    /// The main hand's item at the last player tick, whose attack damage and
    /// speed modifiers apply (`LivingEntity.detectEquipmentUpdates`): a hotbar
    /// swap just before an attack keeps them, with the new item's enchantments
    /// (attribute swapping).
    attack_item: Option<Arc<str>>,
    /// Mobs a player hurt (`LivingEntity.lastHurtByPlayer`), with the guest
    /// it was by connection (None: the host).
    last_hurt_by_player: HashMap<u32, Option<u32>>,
    /// Leashed mobs a player killed, with the killer's UUID: the lead drops
    /// on the corpse's next tick (`Leashable.tickLeash`), for the mods. With
    /// the corpse's dimension: each level's tick prunes only its own.
    lead_killers: HashMap<u32, (u128, Dim)>,
    /// Animals a player fed into love (`Animal.loveCause`), with the guest it was by
    /// connection (None: the host). Breeding credits them, wherever they are.
    love_cause: HashMap<u32, Option<u32>>,
    /// Mobs whose death (`LivingEntity.die`) has been handled.
    /// Looked up per mob each tick: a fast hasher (never iterated).
    died: horizon_client::fast_hash::FxHashSet<u32>,
    /// Client-ticked block-entity animation state (`game/block_entity_anims.rs`).
    be_anims: horizon_client::block_entity_anim::BlockEntityAnims,
    /// Item being used (eaten): hotbar slot and ticks used.
    using: Option<(usize, u32)>,
    /// `getUseDuration` when that use started, as a mod left it
    /// (`game/item_use.rs`): consumables and mod items count to it.
    use_duration: i32,
    /// A mod's `Start` or `Tick` set that use's ticks left (`game/item_use.rs`).
    use_ticks_set: bool,
    /// The crossbow charge's start and middle loading sounds played so far
    /// (`game/crossbow.rs`): each charge plays each of them once, however
    /// far a mod's tick moved the charge at a time.
    crossbow_sounds: [bool; 2],
    /// Camera.oldFovModifier / fovModifier.
    /// `Camera` (eye height and field-of-view easing, view placement).
    view: horizon_client::camera::Camera,
    /// `FramerateLimiter` and its clock.
    limiter: horizon_client::window::FramerateLimiter,
    frame_clock: horizon_client::window::SystemClock,
    /// Hud.scopeScale (spyglass overlay size).
    scope_scale: f32,
    /// Duration of the last frame in ticks (GUI easing).
    frame_ticks: f32,
    title: Option<Title>,
    /// Armor slots: head, chest, legs, feet.
    armor: [Option<ItemStack>; 4],
    /// The off hand (Inventory slot 40, saved as slot 150; menu slot 504 here).
    offhand: Option<ItemStack>,
    /// `ItemStack.popTime` of the nine hotbar stacks, then the off hand: set to 5
    /// when a slot gains items, counted down each tick.
    hotbar_pop: [u8; 10],
    /// The hand an item use acts with (`InteractionHand`): the off hand only
    /// for its turn in `use_item` and while its item is being used.
    hand: Hand,
    /// The shield's item cooldown after an axe disabled it (ticks).
    shield_cooldown: u32,
    /// `Player.getCooldowns()`: the `use_cooldown` component's entries, what
    /// `itemDecorations` shades a slot by. A server's players have their own
    /// (`GuestPlay::cooldowns`), swapped in for their turn.
    cooldowns: horizon_client::cooldowns::Cooldowns,
    /// The source of a command run through /execute (the player otherwise).
    command_source: Option<commands::execute::Source>,
    titles: titles::Titles,
    /// `Hud.toolHighlightTimer`: the selected item name.
    tool_highlight: horizon_client::hud_overlays::ToolHighlight,
    /// `Hud.autosaveIndicatorValue`.
    saving_indicator: horizon_client::hud_overlays::SavingIndicator,
    /// This game's world is autosaving this tick (`MinecraftServer.isSaving`).
    autosaving: bool,
    /// `MinecraftServer.ticksUntilAutosave`: the ticks left before the next
    /// autosave, counted down a tick at a time.
    ticks_until_autosave: i64,
    applied_modifiers: Vec<(String, String)>,
    experience: Experience,
    orbs: horizon_client::join_seq::JoinList<ExperienceOrb>,
    xp_delay: u32,
    ordered: ordered_tick::OrderedTicks,
    level_up_sound: horizon_client::entities::LevelUpSound,
    /// The arm swing (`LivingEntity.SwingState`) and how many swings started.
    swing: horizon_client::first_person::SwingState,
    swings_started: u32,
    /// Swings of uses whose client predicts them (`SwingSource.PREDICTED`):
    /// a server does not send these back to the player who swung.
    predicted_swings: u32,
    /// The first-person equip heights and the items the hands show (with
    /// the slot each came from).
    heights: horizon_client::first_person::HandHeights,
    /// The hands' lag behind the view (`LocalPlayer.xBob`, `yBob`); a new
    /// LocalPlayer (joining, respawning, changing dimension) starts at 0.
    sway: horizon_client::first_person::HandSway,
    visible_hands: [(usize, Option<ItemStack>); 2],
    /// Whether the main and off hand's slots were set to a new stack that
    /// the hand doesn't show yet (`Game::hand_stack_replaced`).
    hands_replaced: [bool; 2],
    /// `Minecraft`'s attack handling, attack clicks since the last tick and
    /// the attack strength tickers the hand reads.
    clicks: horizon_client::first_person::Clicks,
    attack_clicks: u32,
    /// Use clicks since the last tick; `handleKeybinds` consumes them after the attack clicks.
    use_clicks: u32,
    hand_strength: horizon_client::attack_strength::AttackStrength,
    settings: Settings,
    tutorial: tutorial::Tutorial,
    tutorial_toasts: Vec<tutorial::HintToast>,
    /// The `SystemToastId.NARRATOR_TOGGLE` toast, wherever the player is.
    narrator_toast: Option<horizon_client::toasts::SystemToast>,
    /// The open screen's narration (`Screen`'s narration state and timing).
    narration: horizon_client::screen_narrator::ScreenNarrator,
    /// Where the options screen returns: true = title, false = pause menu.
    options_from_title: bool,
    dragging: bool,
    /// `ClientAvatarState`: walked distance and view bob.
    avatar: horizon_client::camera::AvatarBob,
    flash: u32,
    /// OptiFine custom sky layers' eased brightness (`custom_sky.rs`).
    custom_sky: custom_sky::CustomSkyView,
    /// `ClientLevel.rainSoundTime`: ticks of rain since the last rain sound.
    rain_sound_time: i32,
    /// Bolts that struck here and sky flashes they made, in total (`status`).
    lightning_seen: [u32; 2],
    /// The head of the bed while sleeping; the visual timer also survives waking.
    sleeping: Option<[i32; 3]>,
    sleep_timer: u32,
    /// The beds players sleep in that this game marked `occupied`, by level
    /// (game/beds.rs).
    player_beds: Vec<(Dim, [i32; 3])>,
    /// `SleepStatus`'s active and sleeping players at the last announcement.
    sleep_seen: (i64, i64),
    dim: Dim,
    world_root: PathBuf,
    /// The owner's offline-to-account UUID move of this world, when it opened under
    /// the account (`playerdata::migrate_owner`): entities the world loads still name
    /// the player by the offline UUID until their chunk is saved again.
    owner_migration: Option<(u128, u128)>,
    /// The world's data packs' content tables, when it has packs (`None`
    /// keeps `Resources.rest`'s own in use).
    world_tables: Option<WorldTables>,
    /// The world is superflat (`ServerLevel.isFlat`, for every dimension).
    world_flat: bool,
    /// The world is the Debug preset (`Level.isDebug`, for every dimension): its blocks
    /// cannot be changed and nothing ticks.
    world_debug: bool,
    /// A server's players on their way to another level, each waiting for
    /// the destination to load around them.
    server_trips: Vec<server_travel::Trip>,
    /// Players a command sends to another level, once it has run.
    level_teleports: Vec<server_travel::LevelTeleport>,
    /// Teleports a command made inside a level, for `native_player_teleported`
    /// once the player's move has reached their client (`leave_guest`).
    teleport_notes: Vec<server_travel::TeleportNote>,
    /// A server's levels other than the one in place (game/levels.rs).
    levels: levels::Levels,
    /// Which pass of a server tick runs (`tick_server`): the first one reads
    /// the network, the last one ends the players' tick.
    level_pass: levels::Pass,
    /// Nether portals: the player's, items' and mobs' (game/nether_portal.rs).
    portals: nether_portal::Portals,
    /// The respawn point when it is not the world spawn or a bed: a respawn
    /// anchor (`game/respawn_anchor.rs`).
    respawn_config: Option<respawn_anchor::RespawnConfig>,
    /// The next enter_dimension call comes from an End portal.
    end_portal_travel: bool,
    /// The open WinScreen (game/win.rs).
    win: Option<win::Win>,
    end: end::EndState,
    block_ticks: Ticks,
    tick_scratch: TickScratch,
    tick_chunks: horizon_client::tick_chunk_membership::TickChunkMembership,
    tickets: horizon_client::ticket_levels::ChunkTickets,
    random_tick_sections: horizon_client::random_tick_sections::RandomTickSections,
    level_events: Vec<LevelEvent>,
    falling: horizon_client::join_seq::JoinList<FallingBlock>,
    /// Blocks the server shows for a moment (`ClientboundAddTransientBlockPacket`).
    transient_blocks: transient_blocks::TransientBlocks,
    paintings: Vec<horizon_client::entities::Painting>,
    painting_variants: paintings::PaintingVariants,
    item_frames: Vec<horizon_client::entities::ItemFrame>,
    frame_model: frames::FrameModel,
    glow_frame_model: frames::FrameModel,
    /// The `map=true` frame variants.
    map_frame_model: frames::FrameModel,
    glow_map_frame_model: frames::FrameModel,
    armor_stands: horizon_client::join_seq::JoinList<horizon_client::entities::ArmorStand>,
    /// Worn armor as of the last `collectEquipmentChanges` look (frost walker).
    location_armor: [Option<ItemStack>; 4],
    /// `Cushion` decoration entities (game/cushions.rs).
    cushions: Vec<horizon_client::entities::Cushion>,
    /// Entities of types Horizon does not simulate.
    others: Vec<commands::OtherEntity>,
    armor_stand_assets: armor_stands::ArmorStandAssets,
    /// `WaypointStyleManager`.
    waypoint_styles: horizon_client::locator_bar::WaypointStyles,
    /// `ClientWaypointManager`: the locator bar's waypoints.
    waypoints: horizon_client::locator_bar::ClientWaypoints,
    /// The camera the last frame drew from (position, yaw, pitch, fov), for the locator bar.
    locator_camera: Option<([f64; 3], f32, f32, f32)>,
    native_camera_frame: Option<horizon_client::mod_api::NativeCamera>,
    /// The yaw and pitch a native mod's camera drew the last frame from (`native_camera_pose`),
    /// none when it drew from vanilla's.
    posed_rotation: Option<(f32, f32)>,
    /// `LocalPlayer.postEffects`: the chains the server has the screen run
    /// (`ClientboundPostEffectsPacket`), in order.
    post_effects: Vec<String>,
    /// The game time (ticks and partial) of the last frame, for `PolyDeltaTime`.
    polytone_post_ticks: f64,
    /// `GameRenderer.spectatedEntityPostEffect` (`checkEntityPostEffect`).
    spectated_post_effect: Option<&'static str>,
    /// `LocalPlayer.experienceDisplayStartTick`: (last progress, seen a change, tick).
    experience_display: (f32, bool, u64),
    boats: horizon_client::join_seq::JoinList<boats::Boat>,
    /// The boats a mob's lead can hang on, for this tick's mob ticks.
    leash_boats: Vec<(u32, horizon_client::mobs::leash::HolderState)>,
    boat_models: boats::BoatModels,
    /// EndCrystalModel's baked layer.
    crystal_model: Option<horizon_client::mobs::Part>,
    /// CushionModel's baked layer.
    cushion_model: Option<horizon_client::mobs::Part>,
    /// Models drawn only once such an entity shows up (lead knots, wind charges...).
    entity_models: EntityModels,
    carts: horizon_client::join_seq::JoinList<minecarts::Minecart>,
    /// The boats and minecarts `native_vehicle_moved` follows (lan_vehicles.rs).
    watched_vehicles: Vec<lan_vehicles::WatchedVehicle>,
    /// What the mods asked for server players out of reach (`native_teleport_player`...),
    /// done after the players' packets of the tick (native_travel.rs).
    native_moves: Vec<(u128, native_travel::NativeMove)>,
    /// Songs playing from jukeboxes (tracked sound handles).
    jukebox_sounds: HashMap<(i32, i32, i32), u64>,
    bolts: Vec<lightning::Bolt>,
    maps: maps::Maps,
    /// The ender dragon fight (game/dragon.rs).
    fight: dragon::Fight,
    dragon: Option<dragon::Dragon>,
    crystals: Vec<dragon::Crystal>,
    breath_clouds: Vec<dragon::Breath>,
    dragon_model: Option<horizon_client::mobs::Part>,
    /// Map textures waiting for the next HUD batch to upload them.
    map_patches: Vec<horizon_client::font::Patch>,
    name_tag_layouts: names::NameTagLayouts,
    text_displays: std::collections::HashMap<i32, text_displays::RemoteTextDisplay>,
    interactions: std::collections::HashMap<i32, interactions::RemoteInteraction>,
    /// The fence knots a server tracks to this client, by entity id, at their
    /// block (`LeashFenceKnotEntity`).
    remote_knots: HashMap<u32, [i32; 3]>,
    /// The entity ids a hosted server gives the knots its mobs hang on, by
    /// block, while a lead is tied there.
    knot_ids: HashMap<[i32; 3], u32>,
    text_display_vertices: usize,
    /// Player skin downloads, and which skin each atlas skin slot holds
    /// (least recently shown first).
    skin_downloads: RefCell<horizon_client::skins::Downloader>,
    /// `ProfileResolver.Cached`: `Dynamic` profiles (name- or id-only heads
    /// and mannequins) resolve through the services in the background.
    skin_profiles: RefCell<horizon_client::skins::Resolver>,
    skin_slots: horizon_client::skins::SkinSlots,
    /// Cosmetics on players (game/cosmetics.rs), off by default.
    cosmetics: cosmetics::Cosmetics,
    /// Emotes: those the wheel offers and the one playing (`game/emotes.rs`).
    emotes: emotes::Emotes,
    emote_wheel: emote_wheel::Wheel,
    /// 3D skin layers: each player's outer layer as one slab per painted
    /// texel, built once per (skin, slim) (`mobs::skin_layers`).
    skin_3d: RefCell<skin_models::SkinModels>,
    /// Runtime trim slots and the last completed preparation for this atlas.
    trim_slots: horizon_client::trims::TrimSlots,
    cart_models: minecarts::CartModels,
    /// The boat the player rides.
    riding: Option<u32>,
    /// Vehicles that gained a rider this tick whose `START_RIDING_TRIGGER` was
    /// not fired yet (`game/boarding.rs`), for a guest's turn.
    started_riding: Vec<u32>,
    /// A rotation-only write (`/rotate`, `Entity.forceSetRotation`) to the player of this
    /// turn: `leave_guest` sends it as `ClientboundPlayerRotationPacket`, not a teleport.
    rotation_forced: bool,
    /// The mount the last `position_rider` put the player on, to tell the tick
    /// a seat is taken (`Entity.moveTo`, no camera sweep) from the ones that
    /// follow it (`positionRider`, interpolated as usual).
    posed_rider: Option<u32>,
    /// The vehicle the player's file names, waiting for it to be in the world
    /// (`ServerPlayer.loadAndSpawnParentVehicle`).
    pending_vehicle: Option<[i32; 4]>,
    /// The ender inventory (27 slots) and the ender chest showing it.
    ender_items: Vec<Option<ItemStack>>,
    ender_open: Option<(i32, i32, i32)>,
    /// Block entity data of broken blocks waiting for their dropped item.
    pending: spill::Pending,
    /// Item entities spawned and block entities removed while the block
    /// behaviour ran (spilled contents, popped resources).
    spill: spill::Spill,
    /// The recipe book of the open screen (game/recipe_book.rs).
    recipe_ui: recipe_book::RecipeUi,
    /// The horse inventory (game/mount_menu.rs).
    mount_menu: mount_menu::MountMenu,
    /// The player's fishing hook.
    hook: Option<fishing::Hook>,
    /// The other players' hooks a server tracks to this client.
    remote_hooks: Vec<fishing::RemoteHook>,
    tridents: horizon_client::join_seq::JoinList<trident::ThrownTrident>,
    rockets: horizon_client::join_seq::JoinList<fireworks::Rocket>,
    /// Ominous trial spawners' item spawners.
    ominous_spawners: Vec<ominous_item_spawner::OminousItemSpawner>,
    creaking: creaking::CreakingState,
    /// Monster spawners' display mobs (`game/spawner.rs`).
    spawners: spawner::SpawnerState,
    /// Reused candidate buffers for natural spawning.
    spawn_scratch: natural_spawning::SpawnScratch,
    trident_model: Option<horizon_client::mobs::Part>,
    arrow_model: Option<horizon_client::mobs::Part>,
    bee_stinger_model: Option<horizon_client::mobs::Part>,
    tnts: horizon_client::join_seq::JoinList<tnt::PrimedTnt>,
    hoppers: HashMap<(i32, i32, i32), machines::HopperState>,
    /// The screen of a mod block's menu open on this client (`native_screen`).
    mod_screen: Option<native_hooks::OpenNativeScreen>,
    /// The players with each mod container open, by level (`tick_mod_container_viewers`).
    mod_container_viewers: HashMap<(&'static str, (i32, i32, i32)), u32>,
    /// The owners of each chunk a native mod's ticket holds, by level
    /// (`native_force_chunk`); `streaming.mod_forced` is the level's chunks.
    mod_chunk_tickets: HashMap<Dim, HashMap<(i32, i32), std::collections::BTreeSet<String>>>,
    /// Where item entities are, while the block entities tick (hoppers).
    item_columns: Option<machines::ItemColumns>,
    /// Item columns for the players' pickups (`tick_guests`) and hopper carts
    /// (`tick_carts`), exact at any time: any write to the items (a move, a
    /// pickup, a command) makes them scan, then rebuild (`StampedItemColumns`).
    pickup_columns: Option<machines::StampedItemColumns>,
    /// Where the mobs are, while the block entities tick (potent sulfur gas).
    pass_mobs: Option<potent_sulfur::PassMobs>,
    /// Block-entity positions retained between ticks for client effects.
    spawner_tick_positions: Vec<horizon_client::block_entities::Pos>,
    trial_spawner_tick_positions: Vec<horizon_client::block_entities::Pos>,
    creative_screen: Creative,
    /// `CreativeModeInventoryScreen.ignoreTextInput`: the chat key opened the
    /// search tab, so the letter it carries is not typed into the box.
    creative_ignore_text: bool,
    control: Option<horizon_client::control::Server>,
    /// The Agent Control setting the socket was last opened or closed for (`agent_settings`).
    agent_control_on: bool,
    /// When an agent's last command came.
    agent_last: Option<Instant>,
    frame_stats: horizon_client::control::FrameStats,
    /// This play session's frame and tick times, for opt-in performance statistics.
    perf: horizon_client::telemetry::Session,
    /// Control replies waiting for a rendered frame or a benchmark deadline.
    pending_screenshot: Option<std::sync::mpsc::Sender<String>>,
    /// `screenshot-sequence` while it runs.
    screenshot_sequence: Option<screenshot_sequence::Sequence>,
    /// `render-capture DIR`: replied once the renderer wrote the scene.
    pending_render_capture: Option<std::sync::mpsc::Sender<String>>,
    pending_bench: Option<(Instant, std::sync::mpsc::Sender<String>)>,
    /// Recent frame times for the performance overlay.
    frame_graph: horizon_client::perf_overlay::FrameGraph,
    /// Horizon's extras (minimap, auto-reconnect; mods' counters): `game/extras.rs`.
    extras: extras::ExtrasState,
    /// The PvP & HUD options' state (`game/pvp.rs`).
    pvp: pvp::PvpState,
    /// Horizon mods (`game/mods.rs`), none running unless turned on.
    mods: mods::ModsState,
    /// Every item and recipe, for mods (`game/native_recipes.rs`), made once.
    recipe_tables: native_recipes::RecipeTables,
    /// The item panel beside container screens (`game/native_panel.rs`).
    panel: native_panel::Panel,
    /// Horizon's own shader pack, while one is picked (`game/native_shaders.rs`).
    native_shaders: native_shaders::NativeShaders,
    /// The items native client mods claim and the events they haven't polled (game/native_claimed_items.rs).
    native_input: native_claimed_items::ModInput,
    /// The native lifecycle bookkeeping.
    native_lifecycle: native_lifecycle::NativeLifecycle,
    /// Native mods' config files this game loaded (game/native_configs.rs).
    native_configs: native_configs::NativeConfigs,
    /// Native mods' messages waiting (`game/native_net.rs`).
    native_net: native_net::NativeNet,
    /// What native mods send one player, waiting for the hub (`game/native_player_fx.rs`).
    native_player_fx: native_player_fx::NativePlayerFx,
    /// Native mods' kicks waiting for the tick's end (`game/native_packets.rs`).
    native_packets: native_packets::NativePackets,
    /// The mod whose own screen is open (`Screen::Mod`).
    native_mod_screen: Option<String>,
    /// Meshed sections waiting for an upload budget (Horizon settings).
    pending_uploads: Vec<((i32, i32, i32), horizon_client::mesher::SectionMesh)>,
    /// This frame's view frustum, for culling entities and block entities.
    view_frustum: Option<horizon_client::gfx::Frustum>,
    /// Whether the renderer's section-occlusion graph is active for this view.
    entity_section_visibility_valid: bool,
    /// The camera section in the current world layout.
    entity_section_camera: Option<(i32, i32, i32)>,
    sync_occlusion_culler: Option<horizon_client::voxel_occlusion::Culler>,
    async_occlusion_culler: Option<Box<occlusion_culling::AsyncOcclusionCuller>>,
    async_occlusion_culling_failed: bool,
    async_particle_culler: RefCell<particle_culling::ParticleAsyncState>,
    /// `Renderer::pack_shadow_casters` for this frame's entities: (keep what the culler hides,
    /// build the player's model in first person); (false, false) without a shader pack.
    pack_shadow_casters: (bool, bool),
    /// A scripted flight from the control socket (`fly`).
    flight: Option<control::Flight>,
    /// The running agent action from the control socket (game/agent.rs).
    agent: agent::Agent,
    /// Widget commands waiting for the frame after a click (game/ui_control.rs).
    ui_wait: ui_control::UiWait,
    /// What happened, for `events` and `wait-for` (game/events.rs).
    events: events::Events,
    /// Who caused each input event: a person, an agent or the game (game/input_trace.rs).
    input_trace: input_trace::InputTrace,
    /// A repro recording, export or replay (game/repro.rs).
    repro: repro::Repro,
    /// `HORIZON_TICK_SCRIPT`'s scenario (game/tick_script.rs).
    tick_script: tick_script::TickScript,
    /// The overworld's last `sky_darken` and its inputs (time, rain, thunder).
    sky_darken_memo: std::cell::Cell<Option<((i64, u32, u32), i32)>>,
    /// The last water plane `water_plane` found, kept while none is in reach.
    water_plane_memo: std::cell::Cell<Option<f32>>,
    /// The camera's block and the second `water_plane` last scanned at.
    water_plane_scan: std::cell::Cell<Option<([i32; 3], i64)>>,
    /// The mobs' box reach while the boats and minecarts tick, with the mob count it was
    /// worked out for (`vehicle_mob_reach`).
    vehicle_mob_reach: std::cell::Cell<Option<(usize, horizon_client::mobs::BoxReach)>>,
    persist: persist::EntityMemory,
    entity_attachments: HashMap<[i32; 4], horizon_world::attachments::Attachments>,
    /// Connected to a server: the world comes from the network (game/remote.rs).
    remote: Option<remote::Remote>,
    /// The open connection joined from Realms: `ServerData.isRealm` for the friends
    /// service's presence status.
    realm_session: bool,
    /// Active packet replay session (game/studio.rs).
    pub(super) studio_replay: Option<horizon_client::studio::ReplayDriver>,
    pub(super) studio_freecam: horizon_client::studio::StudioFreecam,
    pub(super) studio_timeline: horizon_client::studio::TimelineUi,
    pub(super) studio_topbar: horizon_client::studio::TopBarUi,
    pub(super) studio_rmb_down: bool,
    studio_camera_track_dirty: bool,
    /// Camera perspective to restore after leaving replay playback.
    studio_replay_previous_camera_type: Option<perspective::CameraType>,
    /// Replay opened from the title screen owns the synthetic remote session.
    studio_replay_standalone: bool,
    last_replay_tick: Option<Instant>,
    pub(super) studio_virtual_tick_ms: f32,
    /// This world is open to the network (game/lan.rs).
    lan: Option<lan::LanHost>,
    /// Single player's server thread, which `remote` is connected to.
    integrated: Option<integrated::Integrated>,
    /// The shapes of the open world's levels, from its dimension types.
    level_shapes: horizon_client::chunk::LevelShapes,
    /// This game is that server thread's (game/integrated.rs).
    serving: Option<integrated::Serving>,
    /// The open world is shared behind an invite code (game/friends.rs).
    #[cfg(feature = "friends")]
    friends: Option<friends::Sharing>,
    /// The Social Interactions screen and the players whose chat is hidden (game/social.rs).
    social: social::Social,
    world_options: Option<lan_screen::WorldOptions>,
    /// Chat Settings opened from RestrictionsScreen: Done returns to World Options.
    options_to_world_options: bool,
    /// HDR's settings opened with their key from the game: Done returns to the game.
    options_to_game: bool,
    command_block_screen: Option<command_blocks::CommandBlockScreen>,
    block_editor: Option<block_editor::BlockEditScreen>,
    /// `StructureTemplateManager.structureRepository`: the templates in memory
    /// by id, a miss included (game/structure_blocks.rs).
    structure_templates: HashMap<String, Option<structure_blocks::TemplateEntry>>,
    /// The game tests read so far, by id (the test instance registry).
    test_defs: std::cell::RefCell<HashMap<String, Option<test_instances::TestDef>>>,
    /// `test_defs` holds the world's whole test instance registry
    /// (`snapshot_test_defs`), so a miss is final.
    test_defs_snapshot: bool,
    /// `GameTestBlockHighlightRenderer.markers`: block, text, removal time.
    test_highlights: HashMap<(i32, i32, i32), (String, std::time::Instant)>,
    /// `GameTestTicker.SINGLETON`: the game tests running on this server.
    game_tests: game_tests::GameTests,
    /// KeyboardHandler's debug key state (game/debug_keys.rs), the physical
    /// keys held, and the debug renderers F3+B and F3+G switch on.
    debug_keys: horizon_client::debug_keys::DebugKeys,
    /// The F3 overlay's charts (F3+2, F3+3).
    debug_charts: horizon_client::debug_chart::DebugCharts,
    /// `Minecraft.debugEntries`: what the overlay and the debug renderers
    /// show (`debug-profile.json`, F3+F6).
    debug_entries: horizon_client::debug_entries::DebugEntries,
    /// `DebugOptionsScreen` while it is open (game/debug_options.rs).
    debug_options: Option<debug_options::DebugOptionsUi>,
    /// `DebugEntryMemory`'s readings (game/debug_lines.rs).
    memory_meter: std::cell::RefCell<horizon_client::debug_overlay::MemoryMeter>,
    /// The F3 overlay's vertices between rebuilds (`draw_debug_overlay_under`).
    debug_paint: RefCell<horizon_client::debug_overlay::PaintCache>,
    /// The F3 overlay's last reading of the refresh rate, and when it was taken.
    debug_refresh: std::cell::Cell<Option<(Instant, Option<u32>)>>,
    /// `Minecraft.gpuUtilization`: the last timed frame's GPU time over its
    /// CPU time, in percent.
    gpu_utilization: f64,
    /// GPU Culling's Auto mode: what it has decided from the frames' CPU and GPU times.
    gpu_culling_auto: horizon_client::autotune::GpuCullingAuto,
    /// The mobs `mob_geometry` drew last frame (`lastEntityRenderStateCount`).
    rendered_mobs: std::cell::Cell<usize>,
    /// Whether a native mod requested background blur for this frame (`apply_screen_blur`).
    screen_blur_requested: std::cell::Cell<bool>,
    /// `ServerChunkCache.lastSpawnState` of each level: its spawnable chunks
    /// and each `MobCategory`'s count.
    spawn_counts: HashMap<Dim, (i32, [i32; 8])>,
    /// PauseScreen.showPauseMenu (false after F3+Escape).
    pause_menu: bool,
    pause_focus: Option<u8>,
    death_focus: Option<u8>,
    /// The screen ([`Game::key_screen`]) the menu focus belongs to. Another
    /// screen in front drops it, as `Gui.setScreen` drops the focus of the
    /// screen it leaves (`Screen.clearFocus`).
    focus_screen: u64,
    /// MultiPlayerGameMode.previousLocalPlayerMode (F3+N goes back to it).
    previous_mode: Option<horizon_client::command::args::GameMode>,
    gamemode_switcher: Option<horizon_client::game_mode_switcher::GameModeSwitcher>,
    /// The system clipboard (`KeyboardHandler.clipboardManager`).
    clipboard: horizon_client::clipboard::SystemClipboard,
    /// Select All in the append-only text fields.
    text_selection: horizon_client::clipboard::AppendSelection,
    /// The world was saved for the application ending (`save_for_exit`).
    saved_for_exit: bool,
    native_client_stopped: bool,
    pending_save: Option<saving::ClosingSave>,
    /// Scheduled end of the server tick (`MinecraftServer.haveTime`).
    server_tick_deadline: Option<Instant>,
    /// The last world frame reached the swap chain. A window the compositor
    /// stops showing gets no frames, and the event loop must not spin on it.
    frame_drawn: bool,
    /// The world list's icon in progress (`GameRenderer.tryTakeScreenshotIfNeeded`):
    /// `None` without a local world, or once the save has its icon.png.
    world_icon: Option<WorldIcon>,
    /// The block a guest is acting on (game/lan_actions.rs).
    forced_hit: Option<Hit>,
    /// `Minecraft.hitResult` while the clicks of a tick act (game/remote.rs
    /// `remote_handle_input`): picked once, so a creative break does not
    /// move the use that follows it past the broken block.
    tick_hit: Option<Option<Hit>>,
    /// The mob of that `hitResult`, picked with the item held at the pick
    /// (`LocalPlayer.raycastHitResult`: a spear's `attack_range`), before the
    /// tick's number keys change it.
    tick_mob: Option<Option<(usize, f64, [f64; 3])>>,
    /// Number keys since the last tick (`keyHotbarSlots[i].consumeClick`):
    /// `Minecraft.handleKeybinds` selects them after the tick's pick.
    hotbar_presses: Vec<usize>,
    /// `Player.entityInteractionRange` and `blockInteractionRange`, refreshed
    /// each tick from the attribute (a server's reach modifier, creative's
    /// +2): the pick helpers' `&self` reach.
    entity_reach: f64,
    block_reach: f64,
    /// The part of a right click a guest's packet asks for (game/lan_actions.rs).
    guest_use: Option<GuestUse>,
    /// The last `use_hand` that ended the click did so with `FAIL`, which
    /// does not consume the action (handleUseItemOn's ANY_BLOCK_USE).
    use_failed: bool,
    /// A mod canceled a use the acting guest's client predicted (a throw, a
    /// placement): their inventory goes out whole when the turn ends.
    guest_inventory_resend: bool,
    /// Level events of this tick for the guests, and the guest acting now.
    lan_events: Vec<(i32, [i32; 3], i32, Option<u32>)>,
    /// Jukebox level events 1010 (song name) and 1011 (`None`) for the guests;
    /// the song's network id is only known once the hub is in hand.
    lan_songs: Vec<([i32; 3], Option<String>)>,
    lan_cushion_breaks: Vec<(horizon_world::registry::BlockStateId, [f64; 3])>,
    /// Block events (position, b0, b1, state) and entity events (id, event,
    /// position) for the guests.
    lan_block_events: Vec<([i32; 3], i32, i32, u32)>,
    lan_entity_events: Vec<(i32, i8, [f64; 3])>,
    /// Successful crits (attacking guest, hit entity), queued while the hub is taken.
    lan_critical_hits: Vec<(Option<u32>, i32, bool)>,
    /// Spear stabs that landed (the stabbing guest), queued while the hub is
    /// taken: `broadcastEntityEvent(player, 2)`.
    lan_kinetic_hits: Vec<Option<u32>>,
    /// `LivingEntity.recentKineticEnemies`: when each player (a guest, or the
    /// window's own) last stabbed each entity this use.
    stabbed: HashMap<(Option<u32>, u32), i64>,
    /// The other players the acting player's spear can reach this turn,
    /// taken from the hub before it (`spear_players`).
    stab_players: Vec<spear_players::StabPlayer>,
    /// Spear stabs on players worked out in a guest's turn, landed once the
    /// hub is back (`land_player_stabs`).
    player_stabs: Vec<spear_players::StabBatch>,
    /// The movement a guest's client reported this tick
    /// (`ServerPlayer.getKnownMovement`), for their turn.
    known_movement: Vec3,
    /// `LivingEntity.lastKineticHitFeedbackTime` of the entities that felt a
    /// spear's hit (`spear::OWN` is this window's player), in game time.
    kinetic_feedback: HashMap<i32, i64>,
    /// Global level events for every guest (`ServerLevel.globalLevelEvent`).
    lan_global_events: Vec<(i32, [i32; 3], i32)>,
    /// `ContainerOpenersCounter`: how many players (the host and the guests)
    /// have each chest open.
    lid_openers: HashMap<(i32, i32, i32), i32>,
    lan_acting: Option<u32>,
    /// A mod asked for the command trees again (`native_refresh_commands`):
    /// the next server tick asks every mod for its commands and resends them.
    native_commands_refresh: bool,
    /// Other players in the current guest turn, while its hub is borrowed out.
    lan_building_boxes: building_players::BuildingPlayers,
    /// A guest's `/save-all`: saved once their turn ends (`run_guest_command`),
    /// when their own state is back in their record.
    save_after_guest_turn: bool,
    player_attachments: horizon_world::attachments::Attachments,
    /// The joined guests' attachments, by UUID: here rather than in their
    /// records, so a hook in one guest's turn (when the hub is out of
    /// `self.lan`) still reaches the others'. The acting guest's are in
    /// `player_attachments` for the turn.
    guest_attachments: HashMap<u128, horizon_world::attachments::Attachments>,
    /// The block the acting player is breaking (`break_block`): its destroy
    /// event, and the ones before it, are the player's own (`Level.levelEvent`
    /// with the player excluded); the events after it are not.
    lan_break_pos: Option<[i32; 3]>,
    /// Where a player's break puts the block's own drops instead of popping
    /// them (`native_player_break_block` with `into_inventory`).
    break_collect: Option<Vec<ItemStack>>,
    /// The name of the guest `lan_acting` acts for (`Player.getName`).
    lan_acting_name: Option<String>,
    /// Their UUID (`Player.getUUID`).
    lan_acting_uuid: Option<u128>,
    /// The player and the stack on their cursor as they clicked a native
    /// menu, while `native_menu_clicked` hears it (`native_menu_cursor`).
    native_click_cursor: Option<(u128, Option<horizon_client::items::ItemStack>)>,
    /// Where the mods refused to prime TNT: the sound and game event the level
    /// queued right after the prime are dropped (`prime_tnt_event`).
    tnt_refused: Option<BlockPos>,
    /// Their permission level: an op's above 0 (`PlayerList.isOp`).
    lan_acting_permission: Option<u8>,
    /// The spawn protection radius that holds the acting guest back
    /// (`DedicatedServer.isUnderSpawnProtection` before the position test):
    /// None for an operator, without operators, or off a dedicated server.
    lan_acting_spawn_protection: Option<i32>,
    /// Their entity id, which the hub (often taken out of `lan` while they
    /// act) would otherwise have to be asked for.
    lan_acting_entity: Option<u32>,
    /// The player `native_player_login` hears and their permission level, while it runs
    /// (`native_player_permission`): the hub is out of reach there.
    native_login_permission: Option<(u128, u8)>,
    /// Command feedback to forward to a guest.
    chat_capture: Option<Vec<horizon_client::text::Text>>,
    /// Packets for the guest acting now, sent once its action returns.
    guest_packets: Vec<horizon_protocol::packets::play::Clientbound>,
    /// The acting guest's cooldowns, as (item, ticks): their packet needs the
    /// hub's item registry, which the guest's turn does not hold.
    guest_cooldowns: Vec<(Arc<str>, i32)>,
    /// Players whose command tree the mods asked to send again
    /// (`native_resend_commands`), sent in the next server tick.
    native_resend_trees: Vec<u128>,
    /// The acting guest's own entity events (`ServerPlayer.connection.send`),
    /// sent with their entity id, which the guest's turn does not hold.
    guest_entity_events: Vec<i8>,
    /// The acting guest's equipment breaks (`onEquippedItemBroken`: event and
    /// position), broadcast with their entity id once their turn ends.
    guest_equipment_breaks: Vec<(i8, [f64; 3])>,
    /// `Level.playSound(player, ...)` of the acting guest in `SoundSource.BLOCKS`
    /// (event, position, volume, pitch): everyone near hears it but the guest.
    guest_block_sounds: Vec<(String, [f64; 3], f32, f32)>,
    /// `Player.playSound` of the acting guest (event, position, volume,
    /// pitch): `level.playSound(player, ...)` leaves the guest out, whose
    /// own client plays it.
    guest_player_sounds: Vec<(String, [f64; 3], f32, f32)>,
    /// `broadcastToAdmins` lines a guest's command queued, sent once it
    /// returns: the line, whether operators get it and whether the console
    /// logs it.
    admin_feedback: Vec<(horizon_client::text::Text, bool, bool)>,
    /// The server console is running a command: whether its feedback goes to
    /// the operators (`DedicatedServer.shouldInformAdmins`,
    /// `broadcast-console-to-ops`).
    console_broadcast: Option<bool>,
    /// The command the console is running came over RCON: its source is named `Rcon`
    /// (`RconConsoleSource.createCommandSourceStack`), not `Server`.
    rcon_source: bool,
    /// A dedicated server's operator, whitelist and ban lists, for `/op`,
    /// `/ban` and friends (`DedicatedPlayerList`).
    server_lists: Option<std::sync::Arc<std::sync::Mutex<dedicated::Lists>>>,
    /// State of `/spectate`, `/posteffect`, `/debug`, `/waypoint` and friends.
    cmds: commands::extra::CommandExtras,
    ui: chat::UiState,
    combat: combat::CombatTracker,
    /// Each guest's own `CombatTracker`, out of `combat` between turns.
    guest_combat: std::collections::HashMap<u32, combat::CombatTracker>,
    mount: mounts::MountState,
    stats: stats::Stats,
    score: scoreboard::ScoreState,
    fns: functions::FnState,
    bosses: bossbars::BossState,
    /// `Hud.bossOverlay`: the bars this window is showing.
    boss_overlay: horizon_client::boss_overlay::BossOverlay,
    exec: commands::execute::ExecState,
    rand_sequences: horizon_client::random_sequences::RandomSequences,
    /// `BrushableBlockEntity`'s brush counts of the blocks being brushed.
    brushing: brush::Brushing,
    border: horizon_client::worldborder::WorldBorder,
    /// `WorldBorderRenderer` and its render state.
    border_renderer: horizon_client::border_render::BorderRenderer,
    /// The camera's 16-block section when the wall was last invalidated.
    border_section: Option<[i32; 3]>,
    border_state: horizon_client::border_render::BorderRenderState,
    /// `Hud.tabList`: the player list's header, footer and heart states.
    tab_list: horizon_client::tab_list::TabList,
    subtitles: subtitle_hud::SubtitleState,
    adv: advancements::AdvState,
    restore_player_state: bool,
    stats_ui: stats_screen::StatsUi,
    adv_ui: advancements_screen::AdvUi,
    /// The Server Links screens' state (game/server_links.rs).
    links_ui: server_links::LinksUi,
    camera_type: perspective::CameraType,
    fangs: fangs::FangState,
    /// The local player as a model (third person, the inventory).
    player_model: Option<Mob>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Dim {
    Overworld,
    Nether,
    End,
}
impl Dim {
    fn id(self) -> &'static str {
        match self {
            Self::Overworld => "minecraft:overworld",
            Self::Nether => "minecraft:the_nether",
            Self::End => "minecraft:the_end",
        }
    }
    fn from_id(id: &str) -> Self {
        match id {
            "minecraft:the_nether" => Self::Nether,
            "minecraft:the_end" => Self::End,
            _ => Self::Overworld,
        }
    }
    /// The level's shape in a world of vanilla's dimension types.
    fn shape(self) -> horizon_client::save::LevelShape {
        use horizon_client::save::LevelShape;
        match self {
            Self::Overworld => LevelShape::OVERWORLD,
            Self::Nether => LevelShape::NETHER,
            Self::End => LevelShape::END,
        }
    }
}
fn death_drops(
    inventory: &mut [Option<ItemStack>],
    offhand: &mut Option<ItemStack>,
    armor: &mut [Option<ItemStack>],
) -> Vec<ItemStack> {
    let mut stacks: Vec<ItemStack> = inventory.iter_mut().filter_map(Option::take).collect();
    stacks.extend(offhand.take());
    stacks.extend(armor.iter_mut().rev().filter_map(Option::take));
    stacks.retain(|s| s.enchantment_level("minecraft:vanishing_curse") == 0);
    stacks
}

/// `Inventory.add` on a player's 36 slots (hotbar first) and off hand,
/// `max` the stack's own maximum: onto equal stacks first
/// (`getSlotWithRemainingSpace`: the selected slot, the off hand, then the
/// hotbar and the main inventory), then into empty slots of the inventory
/// (`getFreeSlot`, never the off hand). What does not fit stays in `stack`;
/// true when some of it is left.
fn inventory_add(
    inventory: &mut [Option<ItemStack>],
    offhand: &mut Option<ItemStack>,
    selected: usize,
    max: u32,
    stack: &mut ItemStack,
) -> bool {
    let order = std::iter::once(selected)
        .chain(std::iter::once(shield::OFFHAND_SLOT))
        .chain((0..36).filter(|&i| i != selected));
    for i in order {
        let slot = if i == shield::OFFHAND_SLOT {
            &mut *offhand
        } else {
            &mut inventory[i]
        };
        if let Some(slot) = slot
            && slot.same_item_same_components(stack)
            && slot.count < max
        {
            let moved = (max - slot.count).min(stack.count);
            slot.count += moved;
            stack.count -= moved;
            if stack.count == 0 {
                return false;
            }
        }
    }
    for slot in inventory.iter_mut().take(36) {
        if slot.is_none() {
            let moved = stack.count.min(max);
            *slot = Some(ItemStack {
                count: moved,
                ..stack.clone()
            });
            stack.count -= moved;
            if stack.count == 0 {
                return false;
            }
        }
    }
    true
}

fn facing_offset(facing: &str) -> [i32; 3] {
    match facing {
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        "up" => [0, 1, 0],
        _ => [0, -1, 0],
    }
}
/// The point a `ServerboundInteractPacket` carries: `EntityHitResult.getLocation()
/// - entity.position()`, measured from the entity's feet, as
/// `MultiPlayerGameMode.interact` computes it.
fn interact_location(point: Vec3, feet: Vec3) -> [f64; 3] {
    [point.x - feet.x, point.y - feet.y, point.z - feet.z]
}
/// How far along `dir` (within `reach`) a ray from `eye` enters `b`.
fn ray_box(eye: [f64; 3], dir: [f64; 3], reach: f64, b: Aabb) -> Option<f64> {
    let (min, max) = (b.min_position(), b.max_position());
    let (lo, hi) = ([min.x, min.y, min.z], [max.x, max.y, max.z]);
    let (mut t0, mut t1) = (0.0f64, reach);
    for a in 0..3 {
        if dir[a].abs() < 1e-9 {
            if eye[a] < lo[a] || eye[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (mut ta, mut tb) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
    }
    (t0 <= t1).then_some(t0)
}
/// `Entity.calculateViewVector`, which `GameRenderer.pick` aims with: float radians through `Mth.sin` and
/// `Mth.cos`, vanilla's 65536-entry table. A placement's cursor and an interaction's point come from this ray, and
/// the exact trigonometry below moved them by up to ~1e-4 of a block.
fn pick_view_vector(yaw: f32, pitch: f32) -> [f64; 3] {
    let v = horizon_world::view_direction::view_vector(pitch, yaw);
    [v.x, v.y, v.z]
}
fn view_vector(yaw: f32, pitch: f32) -> [f64; 3] {
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
}
/// `LivingEntity.canGlideUsing`: for each worn piece (head, chest, legs,
/// feet), whether it is a glider (its own component, or one the server
/// patched onto the stack) in the slot its `equippable` names, and will not
/// break with its next point of wear (`ItemStack.nextDamageWillBreak`).
fn gliding_pieces(items: &Items, armor: &[Option<ItemStack>; 4]) -> [bool; 4] {
    const SLOTS: [&str; 4] = ["head", "chest", "legs", "feet"];
    // `canGlideUsing`: the stack's own `glider` and `equippable` (its
    // components, a server's patch), else its item's.
    std::array::from_fn(|i| {
        armor[i]
            .as_ref()
            .is_some_and(|s| horizon_client::item_sync::can_glide_using(s, SLOTS[i], items))
    })
}
/// `Level.canHaveWeather`: sky light, no ceiling, and not the End (the one
/// level the rule names by id).
fn level_can_have_weather(sky_light: bool, has_ceiling: bool, is_end: bool) -> bool {
    sky_light && !has_ceiling && !is_end
}
/// `Camera.setRotation` (`rotationYXZ(π - yaw, -pitch, 0)`) for the player's
/// view, turned round in third-person front view (`Camera.setup`).
fn camera_rotation(camera_type: perspective::CameraType, yaw: f32, pitch: f32) -> glam::Quat {
    let (view_yaw, view_pitch) = if camera_type == perspective::CameraType::ThirdPersonFront {
        (yaw + 180.0, -pitch)
    } else {
        (yaw, pitch)
    };
    glam::Quat::from_euler(
        glam::EulerRot::YXZ,
        std::f32::consts::PI - view_yaw.to_radians(),
        -view_pitch.to_radians(),
        0.0,
    )
}
impl Game<'_> {
    /// `Level.canHaveWeather`, from the level's dimension type (sky light, no
    /// ceiling) and the End, which vanilla names by id. Every rain, thunder
    /// and lightning rule asks this, never the dimension's id.
    fn level_key(&self) -> horizon_core::LevelKey {
        horizon_core::LevelKey::builtin_or_overworld(self.dim.id())
    }
    fn dimension_type(&self) -> &'static horizon_world::dimension_type::DimensionFacts {
        horizon_world::dimension_type::DimensionFacts::of_builtin_level(self.level_key())
    }
    fn dimension_type_of(
        &self,
        dim: Dim,
    ) -> &'static horizon_world::dimension_type::DimensionFacts {
        horizon_world::dimension_type::DimensionFacts::of_builtin_level(
            horizon_core::LevelKey::builtin_or_overworld(dim.id()),
        )
    }
    /// `Level.isDarkOutside`.
    fn dark_outside(&self) -> bool {
        !self.dimension_type().has_fixed_time && self.sky_darken() >= 4
    }
    fn can_have_weather(&self) -> bool {
        level_can_have_weather(
            self.world.shape.sky_light,
            self.world.shape.has_ceiling,
            self.dim == Dim::End,
        )
    }
    /// The rain and thunder levels (`Level.getRainLevel`, `getThunderLevel`):
    /// the world's, where the level can have weather, else none.
    fn weather_levels(&self, partial: f32) -> (f32, f32) {
        if self.can_have_weather() {
            self.world.weather.levels(partial)
        } else {
            (0.0, 0.0)
        }
    }
    fn eye_position(&self) -> Option<[f64; 3]> {
        let p = self.player.as_ref()?;
        Some([
            p.position.x,
            p.position.y + f64::from(p.dimensions.eye_height),
            p.position.z,
        ])
    }
    /// Where targeting starts and points: the eyes along the view. Every
    /// crosshair target (blocks, fluids, entities, mod raycasts) is found
    /// along this ray, so a tracked controller can stand in for it.
    fn aim(&self) -> Option<([f64; 3], [f64; 3])> {
        // Minecraft.pick: from the camera entity's eyes along its view
        // (LivingEntity.getViewYRot: the head's yaw).
        if let Some(m) = self.camera_mob() {
            return Some((
                [m.position.x, m.eye_y(), m.position.z],
                pick_view_vector(m.head_yaw, m.head_pitch),
            ));
        }
        Some((self.eye_position()?, pick_view_vector(self.yaw, self.pitch)))
    }
    /// `Player.blockInteractionRange`: the block pick's and mining's reach.
    fn reach(&self) -> f64 {
        self.block_reach
    }
    /// Outline shapes that depend on more than the block state, as one box
    /// in block space: `LightBlock` and `ScaffoldingBlock.getShape` are a full
    /// block while the main hand holds the block's own item
    /// (`CollisionContext.isHoldingItem`), and `ShulkerBoxBlock.getShape`
    /// grows with the lid (`ShulkerBoxBlockEntity.getBoundingBox`).
    fn context_outline(&self, pos: [i32; 3], state: BlockStateId) -> Option<[f64; 6]> {
        let name = &self.res.facts().get(state).name;
        if matches!(&**name, "minecraft:light" | "minecraft:scaffolding") {
            return self
                .held()
                .is_some_and(|s| *s.item == **name)
                .then_some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        }
        if !name.ends_with("shulker_box") {
            return None;
        }
        let progress = self.be_anims.shulkers.get(&pos).map_or(0.0, |s| s.progress);
        let facing = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("facing").cloned());
        let d = facing_offset(facing.as_deref().unwrap_or("up"));
        let mut b = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        for a in 0..3 {
            match d[a] {
                1 => b[a + 3] += 0.5 * f64::from(progress),
                -1 => b[a] -= 0.5 * f64::from(progress),
                _ => {}
            }
        }
        Some(b)
    }
    fn outline_context_kind(&self) -> horizon_world::collision_context::EntityContextKind {
        if self.camera_mob().is_some() {
            horizon_world::collision_context::EntityContextKind::Living
        } else {
            horizon_world::collision_context::EntityContextKind::Player
        }
    }
    /// A block's outline boxes in world space (`getShape` plus the random offset).
    fn outline_boxes(&self, pos: [i32; 3]) -> Vec<[f64; 6]> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let declared = self
            .collision
            .cached_shapes()
            .context_outline_shape(state, self.outline_context_kind());
        if declared.is_none()
            && let Some(b) = self.context_outline(pos, state)
        {
            return vec![std::array::from_fn(|i| f64::from(pos[i % 3]) + b[i])];
        }
        let f = self.res.facts().get(state);
        let o = if f.outline_offset {
            self.pack
                .context
                .tables
                .offset(state, pos[0], pos[2])
                .unwrap_or([0.0; 3])
        } else {
            [0.0; 3]
        };
        let base = [
            f64::from(pos[0]) + o[0],
            f64::from(pos[1]) + o[1],
            f64::from(pos[2]) + o[2],
        ];
        if let Some(shape) = declared {
            return shape
                .boxes()
                .iter()
                .map(|b| {
                    let p = b.min_position();
                    let min = [p.x, p.y, p.z];
                    let p = b.max_position();
                    let max = [p.x, p.y, p.z];
                    std::array::from_fn(|i| base[i % 3] + if i < 3 { min[i] } else { max[i - 3] })
                })
                .collect();
        }
        f.outline
            .iter()
            .map(|b| std::array::from_fn(|i| base[i % 3] + b[i]))
            .collect()
    }
    /// The declared outline uses vanilla's inside-ray test and interaction override.
    fn clip_declared_outline(
        &self,
        state: BlockStateId,
        pos: [i32; 3],
        origin: [f64; 3],
        dir: [f64; 3],
        reach: f64,
    ) -> Option<Hit> {
        use horizon_core::geometry::{Aabb, Vec3};
        use std::borrow::Cow;
        let facts = self.res.facts().get(state);
        let cached = self.collision.cached_shapes();
        let shape = cached.context_outline_shape(state, self.outline_context_kind());
        let boxes: Cow<'_, [Aabb]> = if let Some(shape) = shape {
            Cow::Borrowed(shape.boxes())
        } else if let Some(b) = self.context_outline(pos, state) {
            Cow::Owned(vec![Aabb::new(
                Vec3::new(b[0], b[1], b[2]),
                Vec3::new(b[3], b[4], b[5]),
            )])
        } else {
            Cow::Owned(
                facts
                    .outline
                    .iter()
                    .map(|b| Aabb::new(Vec3::new(b[0], b[1], b[2]), Vec3::new(b[3], b[4], b[5])))
                    .collect(),
            )
        };
        let offset = if facts.outline_offset {
            self.pack
                .context
                .tables
                .offset(state, pos[0], pos[2])
                .unwrap_or([0.; 3])
        } else {
            [0.; 3]
        };
        let offset = Vec3::new(offset[0], offset[1], offset[2]);
        let interaction: Cow<'_, [Aabb]> = if let Some(shape) = cached.interaction_shape(state) {
            Cow::Borrowed(shape.boxes())
        } else {
            let facing = self
                .pack
                .assets
                .registry
                .state(state)
                .and_then(|s| s.properties.get("facing"))
                .map(String::as_str);
            Cow::Owned(
                interaction_shapes::boxes(&facts.name, facing)
                    .into_iter()
                    .map(|b| Aabb::new(Vec3::new(b[0], b[1], b[2]), Vec3::new(b[3], b[4], b[5])))
                    .collect(),
            )
        };
        let start = Vec3::new(origin[0], origin[1], origin[2]);
        let end = Vec3::new(
            origin[0] + dir[0] * reach,
            origin[1] + dir[1] * reach,
            origin[2] + dir[2] * reach,
        );
        let hit = horizon_world::interaction_shapes::clip_with_interaction_override(
            boxes.iter().map(|b| b.move_by(offset)),
            interaction.iter().copied(),
            start,
            end,
            BlockPos::new(pos[0], pos[1], pos[2]),
        )?;
        Some(Hit {
            pos,
            face: hit.direction.normal(),
            point: [hit.location.x, hit.location.y, hit.location.z],
        })
    }
    /// Voxel traversal against non-air, non-fluid blocks, clipped against
    /// each block's outline shape (`BlockGetter.clip` with `ClipContext.Block.OUTLINE`).
    fn pick(&self) -> Option<Hit> {
        // A guest of a hosted world acts on the block their client targeted.
        if let Some(hit) = self.forced_hit {
            return Some(hit);
        }
        if let Some(hit) = self.tick_hit {
            return hit;
        }
        let (origin, dir) = self.aim()?;
        // GameRenderer.pick: the block ray runs out to the larger of the two
        // interaction ranges; filterHitResult then drops a hit past the
        // block's.
        self.clip_ray(origin, dir, self.block_reach.max(self.entity_reach))
            .filter(|hit| {
                let distance: [f64; 3] = std::array::from_fn(|i| hit.point[i] - origin[i]);
                distance.iter().map(|v| v * v).sum::<f64>() <= self.block_reach.powi(2)
            })
    }
    /// The same traversal without `filterHitResult`: the block hit that
    /// bounds the entity ray in `GameRenderer.pick`, out to the larger
    /// interaction range — a wall between the two still hides the entity.
    fn pick_unfiltered(&self) -> Option<Hit> {
        if let Some(hit) = self.forced_hit {
            return Some(hit);
        }
        let (origin, dir) = self.aim()?;
        self.clip_ray(origin, dir, self.block_reach.max(self.entity_reach))
    }
    /// The first block outline hit within `reach` along the unit `dir`.
    fn clip_ray(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<Hit> {
        self.clip_ray_fluids(origin, dir, reach, false)
    }
    /// The stack in the hand that acts (`getItemInHand(hand)`).
    fn held(&self) -> Option<&ItemStack> {
        self.stack_in(self.held_slot())
    }
    fn held_mut(&mut self) -> &mut Option<ItemStack> {
        self.stack_in_mut(self.held_slot())
    }
    /// The slot of the hand that acts: the selected one, or `OFFHAND_SLOT`.
    /// `LivingEntity.canGlide`: some worn piece `gliding_pieces` lets glide,
    /// or a stack in a hand that glides there (`canGlideUsing` walks every
    /// `EquipmentSlot`).
    fn glider_worn(&self) -> bool {
        let items = self.res.items();
        let hand = |stack: Option<&ItemStack>, slot: &str| {
            stack.is_some_and(|s| horizon_client::item_sync::can_glide_using(s, slot, items))
        };
        gliding_pieces(items, &self.armor).contains(&true)
            || hand(self.inventory[self.selected].as_ref(), "mainhand")
            || hand(self.offhand.as_ref(), "offhand")
    }
    /// `LivingEntity.updateFallFlying`: the glide's wear. `Util.getRandom`
    /// picks one of the gliders that will not break (from the entity's own
    /// random), and
    /// `ItemStack.hurtAndBreak` takes `points` of its durability (Unbreaking
    /// may save it). Callers skip creative.
    pub(super) fn wear_glider(&mut self, points: u32) {
        let gliding = gliding_pieces(self.res.items(), &self.armor);
        let pieces: Vec<usize> = (0..4).filter(|&i| gliding[i]).collect();
        if pieces.is_empty() {
            return;
        }
        // `Util.getRandom(slotsWithGliders, this.random)`: the entity's own
        // random, which never touches the level's stream.
        let pick = random_source::entity().next_int_bound(pieces.len() as i32);
        let pick = pieces[pick.unwrap_or(0) as usize];
        if let Some(stack) = self.armor[pick].clone() {
            let loss = self.wear(&stack, points);
            if let Some(piece) = self.armor[pick].as_mut() {
                piece.damage += loss;
            }
        }
    }
    /// `LivingEntity.canFreeze`'s armor half: any worn piece in
    /// `freeze_immune_wearables`.
    pub(crate) fn wears_freeze_immune(&self) -> bool {
        let immune = self
            .res
            .items()
            .item_tags
            .get("minecraft:freeze_immune_wearables");
        self.armor
            .iter()
            .flatten()
            .any(|s| immune.is_some_and(|t| t.contains(&*s.item)))
    }
    /// `LocalPlayer.moveTowardsClosestSpace` at the body's four corners
    /// (aiStep, unless a spectator or a passenger): a corner inside a
    /// suffocating block sets a 0.1 speed towards the nearest side that is
    /// free, on that side's axis; a later corner overwrites an earlier one.
    /// Suffocating is a full collision cube that is also a full render cube,
    /// as `in_wall` has it.
    fn closest_space_nudge(&self) -> [Option<f64>; 2] {
        match self.player.as_ref() {
            Some(p) if !p.spectator && self.riding.is_none() => {
                closest_space_nudge(p, &self.world, self.res.facts())
            }
            _ => [None; 2],
        }
    }
    /// What the window decides for its player's body every tick: the look
    /// (Player.travel steers sprint-swimming by the pitch), Auto Jump (LocalPlayer.sendPosition), the slowdown of an
    /// item in use (LocalPlayer.modifyInput), the helmet's Respiration
    /// (decreaseAirSupply), the wings, leather boots on powder snow
    /// (PowderSnowBlock.canEntityWalkOnPowderSnow), freeze-immune wear
    /// (LivingEntity.canFreeze), the dimension's FAST_LAVA, the sleeping
    /// pose and blindness (Player.isMobilityRestricted, which stops a sprint).
    pub(crate) fn sync_player_body(&mut self) {
        let glider = self.glider_worn();
        let sleeping = self.sleeping.is_some();
        let item_in_use = self
            .using
            .and_then(|(slot, _)| {
                if slot == shield::OFFHAND_SLOT {
                    self.offhand.as_ref()
                } else {
                    self.inventory.get(slot)?.as_ref()
                }
            })
            .and_then(|s| self.items().get(&s.item));
        let item_use_speed = item_in_use.map(|i| i.use_speed);
        let item_use_slows_sprint = item_in_use.is_some_and(|i| !i.can_sprint);
        let respiration = self.armor[0]
            .as_ref()
            .map_or(0, |s| s.enchantment_level("minecraft:respiration") as i32);
        let leather_boots = self.armor[3]
            .as_ref()
            .is_some_and(|s| &*s.item == "minecraft:leather_boots");
        let freeze_immune = self.wears_freeze_immune();
        let burn_immune = self.wears_burn_from_stepping_immunity();
        let nudge = self.closest_space_nudge();
        let fast_lava = self.dimension_type().fast_lava;
        let blind = self.potions.effects.get("blindness").is_some();
        let Some(player) = self.player.as_mut() else {
            return;
        };
        player.nudge = nudge;
        player.mobility_restricted = blind;
        player.yaw = self.yaw;
        player.pitch = self.pitch;
        player.auto_jump_enabled = self.settings.auto_jump;
        player.item_use_speed = item_use_speed;
        player.item_use_slows_sprint = item_use_slows_sprint;
        player.respiration = respiration;
        player.glider = glider;
        // On someone else's server the glide flag is the server's
        // (`DATA_SHARED_FLAGS_ID`, applied by the SetEntityData handler); the
        // game's own integrated server has no such round trip, so the client
        // keeps predicting the clear a tick after the landing glide.
        player.server_owns_glide = self.integrated.is_none();
        let _ = player.set_sleeping(sleeping);
        player.leather_boots = leather_boots;
        player.freeze_immune = freeze_immune;
        player.burn_from_stepping_immune = burn_immune;
        player.fluids.fast_lava = fast_lava;
    }
    fn held_slot(&self) -> usize {
        match self.hand {
            Hand::Main => self.selected,
            Hand::Off => shield::OFFHAND_SLOT,
        }
    }
    /// `LivingEntity.getUsedItemHand`: the hand of the item in use.
    fn used_hand(&self) -> Option<Hand> {
        self.using.map(|(slot, _)| {
            if slot == shield::OFFHAND_SLOT {
                Hand::Off
            } else {
                Hand::Main
            }
        })
    }
    /// Run `f` with `hand` acting (`held`, `consume_held`, `swing_arm`).
    fn with_hand<T>(&mut self, hand: Hand, f: impl FnOnce(&mut Self) -> T) -> T {
        let prior = std::mem::replace(&mut self.hand, hand);
        let out = f(self);
        self.hand = prior;
        out
    }
    /// Inventory.add: returns the stack that did not fit.
    fn give(&mut self, mut stack: ItemStack) -> Option<ItemStack> {
        let tutorial_item = matches!(
            self.tutorial.step,
            tutorial::Step::FindTree | tutorial::Step::PunchTree | tutorial::Step::CraftPlanks
        )
        .then(|| stack.clone());
        let max = self
            .res
            .items()
            .get(&stack.item)
            .map_or(64, |i| i.max_stack);
        let before = stack.count;
        let left = inventory_add(
            &mut self.inventory,
            &mut self.offhand,
            self.selected,
            max,
            &mut stack,
        );
        if stack.count < before {
            self.tutorial_get_item(tutorial_item.as_ref());
        }
        left.then_some(stack)
    }
    fn particle_sprite(&self, state: BlockStateId, pos: [i32; 3]) -> Option<(String, u32)> {
        let sprite = Self::block_particle_sprite(&self.pack, state)?;
        let chunks = &self.world.chunks;
        let (cx, cz) = (pos[0] >> 4, pos[2] >> 4);
        let region: [[Option<&horizon_client::chunk::Chunk>; 3]; 3] = std::array::from_fn(|dz| {
            std::array::from_fn(|dx| {
                chunks
                    .get(&(cx + dx as i32 - 1, cz + dz as i32 - 1))
                    .map(|c| &**c)
            })
        });
        let tint = self
            .pack
            .context
            .tables
            .terrain_particle_tint(
                state,
                pos,
                &region,
                world_biomes!(self),
                self.streaming.biome_zoom,
                self.streaming.biome_blend,
            )
            .unwrap_or(0xFFFFFF);
        Some((sprite, tint))
    }
    /// ClientLevel.addBreakingParticles: one crumb on the face being mined.
    fn crack_particle(&mut self, pos: [i32; 3], face: [i32; 3], state: BlockStateId) {
        if self.settings.extras.hide_block_particles {
            return;
        }
        let Some((sprite, tint)) = self.particle_sprite(state, pos) else {
            return;
        };
        let boxes = self.particle_shape(pos, state);
        // Keep client effects separate from the authoritative world RNG.
        let r = self.particles.random();
        let Some(p) = horizon_client::particles::breaking_position(r, pos, face, &boxes) else {
            return;
        };
        let mut particle = Particle::terrain(r, p, [0.0; 3], sprite, tint);
        particle.set_power(0.2);
        particle.scale(0.6);
        self.particles.push(particle);
    }
    /// `Player.dropEquipment` for the player who died: unless keep_inventory
    /// is on, `destroyVanishingCursedItems`, then `Inventory.dropAll` from
    /// `at` (the eyes less 0.3): slots 0..36, armor feet to head (36..39),
    /// the off hand (40), each thrown in a random direction. The mods
    /// (`native_loot_drops`, the dying player `who`: uuid and entity id)
    /// may add to, change or take out the stacks before they are thrown.
    fn drop_inventory(&mut self, at: [f64; 3], who: (u128, i32)) {
        if self.ui.rules.bool("keep_inventory") {
            return;
        }
        let mut stacks = death_drops(&mut self.inventory, &mut self.offhand, &mut self.armor);
        self.apply_equipment();
        let context = horizon_client::mod_api::NativeLootDrops {
            table: "",
            dimension: self.dim.id(),
            origin: at,
            block: None,
            entity: Some((who.1 as u32, "minecraft:player")),
            player: Some(who.0),
            tool: None,
            explosion: None,
            used: false,
        };
        self.native_loot_drops(&context, &mut stacks);
        stacks.retain(|stack| stack.count > 0 && &*stack.item != "minecraft:air");
        for stack in stacks {
            let r = &mut self.world.random;
            let v = horizon_client::entities::drop_motion(r, self.yaw, self.pitch, true);
            self.spawn_item(stack, at, v, 40);
        }
    }
    /// Item entity `i` leaves the level (`Entity.discard`); mods hear it.
    pub(super) fn remove_dropped(&mut self, i: usize) -> ItemEntity {
        let e = self.dropped.remove(i);
        self.native_entity_left(e.id);
        e
    }
    /// Experience orb `i` leaves the level (`Entity.discard`); mods hear it.
    pub(super) fn remove_orb(&mut self, i: usize) -> ExperienceOrb {
        let o = self.orbs.remove(i);
        self.native_entity_left(o.id);
        o
    }
    /// The item entity `spawn_item` just added, unless a native mod kept it
    /// out of the level (`native_entity_join_level`).
    pub(super) fn just_spawned_item(&mut self) -> Option<&mut ItemEntity> {
        let id = self.next_entity;
        self.dropped.last_mut().filter(|d| d.id == id)
    }
    /// `new ItemEntity(level, x, y, z, stack, dx, dy, dz)`.
    fn spawn_item(
        &mut self,
        stack: ItemStack,
        position: [f64; 3],
        velocity: [f64; 3],
        pickup_delay: u32,
    ) {
        self.add_item_entity(stack, position, Some(velocity), pickup_delay);
    }
    /// `new ItemEntity(level, x, y, z, stack)`: the item leaves with its own
    /// random's motion, which this returns.
    pub(super) fn spawn_item_own(
        &mut self,
        stack: ItemStack,
        position: [f64; 3],
        pickup_delay: u32,
    ) -> [f64; 3] {
        self.add_item_entity(stack, position, None, pickup_delay)
    }
    fn add_item_entity(
        &mut self,
        stack: ItemStack,
        position: [f64; 3],
        velocity: Option<[f64; 3]>,
        pickup_delay: u32,
    ) -> [f64; 3] {
        let (item, count) = (stack.item.clone(), stack.count as i32);
        self.next_entity += 1;
        let p = Vec3::new(position[0], position[1], position[2]);
        // The item's own random, in vanilla's order: Entity's UUID, ItemEntity's
        // bobOffs and yaw, then (without a given motion) the constructor's motion.
        let mut own = random_source::entity();
        let uuid = horizon_client::save::insecure_uuid(&mut own);
        self.persist.uuids.insert(self.next_entity, uuid);
        let bob = own.next_float() * std::f32::consts::PI * 2.0;
        let yaw = own.next_float() * 360.0;
        let velocity = velocity.unwrap_or_else(|| {
            let x = own.next_double() * 0.2 - 0.1;
            [x, 0.2, own.next_double() * 0.2 - 0.1]
        });
        self.dropped.push(ItemEntity {
            rotation: [yaw, 0.0],
            pickup_delay: i64::from(pickup_delay),
            random: own,
            ..ItemEntity::new(
                self.next_entity,
                stack,
                p,
                Vec3::new(velocity[0], velocity[1], velocity[2]),
                bob,
            )
        });
        // ServerLevel.addFreshEntity.
        if self.native_joins_flush(false, self.next_entity) {
            self.native_item_spawn(self.next_entity, &item, count, position);
        }
        velocity
    }
    /// The block entity at `pos` saved with its id and position
    /// (`saveWithFullMetadata`), or the saved one Horizon keeps as loaded:
    /// what a broken block's loot sees as `LootContextParams.BLOCK_ENTITY`.
    fn loot_block_entity(&self, pos: [i32; 3], block: &str) -> Option<horizon_world::nbt::Tag> {
        let key = (pos[0], pos[1], pos[2]);
        match self.world.block_entities.get(&key) {
            Some(entity) => Some(horizon_client::save::encode_block_entity(
                key, entity, block,
            )),
            None => spill::saved_entity(&self.world, pos),
        }
    }
    /// Where a loot roll at `origin` happens, for a mod's own loot kinds:
    /// the block entity it may copy from, and this level as
    /// `LootContext.getLevel` gives it (`Level.isRaining`: rain above 0.2;
    /// `isThundering`: thunder above 0.9; neither where `canHaveWeather` is
    /// false: a level with a ceiling or no sky light, or the End).
    fn loot_place_at(
        &self,
        origin: [f64; 3],
        block_entity: Option<horizon_world::nbt::Tag>,
    ) -> horizon_world::loot::NativeLootPlace {
        let (rain, thunder) = self.world.weather.levels(1.0);
        // Level.canHaveWeather: sky light, no ceiling and not the End.
        let weather =
            self.world.shape.sky_light && !self.world.shape.has_ceiling && self.dim != Dim::End;
        horizon_world::loot::NativeLootPlace {
            origin: Some(origin),
            block_entity,
            level: Some(horizon_world::loot::NativeLootLevel {
                dimension: self.dim.id().to_owned(),
                game_time: self.world.game_time,
                day_time: self.world.day_time,
                raining: weather && rain > 0.2,
                thundering: weather && thunder > 0.9,
            }),
        }
    }
    /// `Block.getDrops` for a block a player breaks: the block's loot table
    /// with the player as `this`, drawing from its server random sequence
    /// (`LootContext.Builder.create`), else the level random. The roll's
    /// origin is the block's centre and `block_entity` the entity it had.
    fn block_loot_at(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        held: Option<&ItemStack>,
        block_entity: Option<horizon_world::nbt::Tag>,
    ) -> Vec<ItemStack> {
        let registry = self.pack.assets.registry;
        let dynamic = self.pending.dynamic(pos);
        let place = self.loot_place_at(pos.map(|v| f64::from(v) + 0.5), block_entity);
        let mut drops =
            horizon_world::loot::with_native_loot_place(place, || {
                match world_items!(self).block_loot_sequence(registry, state) {
                    Some(sequence) => {
                        let random = self.rand_sequences.get(&sequence, self.level.seed);
                        world_items!(self)
                            .block_drops_by_player(registry, state, held, &dynamic, random)
                    }
                    None => world_items!(self).block_drops_by_player(
                        registry,
                        state,
                        held,
                        &dynamic,
                        &mut self.world.random,
                    ),
                }
            });
        let player = self.native_acting_id();
        self.native_block_drops(pos, state, Some(player), held, None, &mut drops);
        drops
    }
    /// Block.popResource
    fn pop_resource(&mut self, pos: [i32; 3], mut stack: ItemStack) {
        self.pending.attach(pos, &mut stack);
        // Block.popResource: Mth.nextDouble offsets from the level random; the
        // motion is `new ItemEntity(..)`'s own, from the item's random.
        let p = spill::pop_position(&mut self.world.random, pos);
        // Block.popResource draws its offsets before the private overload checks the gamerule.
        if !self.ui.rules.bool("block_drops") {
            return;
        }
        self.spawn_item_own(stack, p, 10);
    }
    /// `Block.popResourceFromFace`: the loot leaves the clicked face with the
    /// face's own motion instead of the item's random one.
    fn pop_resource_from_face(&mut self, pos: [i32; 3], face: [i32; 3], mut stack: ItemStack) {
        self.pending.attach(pos, &mut stack);
        let (p, v) = spill::pop_position_from_face(&mut self.world.random, pos, face);
        // The private overload checks the gamerule after the offsets are drawn.
        if !self.ui.rules.bool("block_drops") {
            return;
        }
        self.spawn_item(stack, p, v, 10);
    }
    /// Player.drop: thrown from the eyes along the view direction.
    fn drop_held(&mut self, all: bool) {
        if self.eye_position().is_none() {
            return;
        }
        let spectator = self.spectator();
        let Some(thrown) = horizon_client::items::take_held_drop(
            &mut self.inventory[self.selected],
            all,
            spectator,
        ) else {
            return;
        };
        self.player_drop(thrown);
    }
    /// `Player.drop`: the statistics and a thrown item entity in front of the player.
    pub(super) fn player_drop(&mut self, thrown: ItemStack) {
        let Some(eye) = self.eye_position() else {
            return;
        };
        // LivingEntity.drop: every drop swings the main hand, a menu's
        // THROW and a creative tab's drop included.
        self.swing_hand(Hand::Main, SwingAnimation::DEFAULT);
        // Player.drop: the drop statistics.
        self.award_stat("dropped", &thrown.item, thrown.count as i32);
        self.award_custom_stat("drop", 1);
        let player = self.native_acting_id();
        let mut cancel = false;
        self.native_item_dropped_v2(player, &thrown, &mut cancel);
        if cancel {
            // The stack stays the player's: what doesn't fit is thrown after all.
            if let Some(rest) = self.give(thrown) {
                self.throw_from_eyes(rest, eye, true);
            }
            return;
        }
        self.native_item_dropped(player, &thrown.item, thrown.count);
        self.throw_from_eyes(thrown, eye, true);
    }
    /// `Player.createItemStackToDrop`: an item entity thrown from the eyes
    /// along the view direction. `thrown_from_hand` (`drop(stack, true)`, the
    /// drop key) names the player as the thrower; `drop(stack, false)`, for
    /// what no longer fits in the inventory, does not.
    fn throw_from_eyes(&mut self, thrown: ItemStack, eye: [f64; 3], thrown_from_hand: bool) {
        let r = &mut self.world.random;
        let v = horizon_client::entities::drop_motion(r, self.yaw, self.pitch, false);
        // `getEyeY() - 0.3F`.
        let y = eye[1] - f64::from(0.3f32);
        self.spawn_item(thrown, [eye[0], y, eye[2]], v, 40);
        // A native mod can keep the item out of the level (native_entity_join_level).
        if thrown_from_hand {
            let uuid = self.acting_player_uuid();
            if let Some(item) = self.just_spawned_item() {
                item.thrower = Some(uuid);
            }
        }
    }
    /// The UUID of the player acting: the guest whose turn it is, else the host.
    pub(super) fn acting_player_uuid(&mut self) -> [i32; 4] {
        match self.lan_acting_uuid {
            Some(guest) => horizon_client::lan::playerdata::uuid_ints(guest),
            None => self.level.ensure_player_uuid(),
        }
    }
    /// `Entity.doWaterSplashEffect` for a dropped item. The client draws the
    /// bubble and splash particles (`Level.addParticle` is empty on a server).
    /// The server plays `entity.generic.splash` (`Entity.playSound`); a
    /// client's `playSeededSound` ignores that call.
    pub(super) fn apply_item_splash(&mut self, splash: horizon_client::entities::ItemSplash) {
        use horizon_client::entities::ItemSplash;
        let client = self.options.execution_role.is_client();
        match splash {
            ItemSplash::Sound { at, volume, pitch } if !client => {
                self.audio
                    .play("minecraft:entity.generic.splash", Some(at), volume, pitch);
            }
            ItemSplash::Particle {
                bubble,
                at,
                velocity,
            } if client => {
                use horizon_client::particles::{ParticleOptions, ParticleType};
                let kind = if bubble {
                    ParticleType::Bubble
                } else {
                    ParticleType::Splash
                };
                self.particles
                    .add(&ParticleOptions::simple(kind), at, velocity);
            }
            _ => {}
        }
    }
    /// Run vanilla block behaviour against the world through `LevelAccess`.
    fn with_level<R>(
        &mut self,
        f: impl FnOnce(&Behaviours, &mut GameLevel<'_>, &mut Ticks) -> R,
    ) -> R {
        self.ensure_stateful_entity_table();
        let darken = self.sky_darken();
        let (rain, thunder) = self.weather_levels(1.0);
        // Spectators ignore block triggers (Entity.isIgnoringBlockTriggers).
        let spectator = self.spectator();
        let player_box = self.player.as_ref().filter(|_| !spectator).map(|p| {
            let w = f64::from(p.dimensions.width) / 2.0;
            (
                [p.position.x - w, p.position.y, p.position.z - w],
                [
                    p.position.x + w,
                    p.position.y + f64::from(p.dimensions.height),
                    p.position.z + w,
                ],
            )
        });
        let player_collision = self.player.as_ref().filter(|_| !spectator).map(|p| {
            let main_hand = self.inventory.get(self.selected).and_then(Option::as_ref);
            PlayerCollision {
                y: p.position.y,
                descending: p.velocity.y < 0.0,
                fall_distance: p.fall_distance,
                main_hand_item: main_hand.map_or_else(
                    || Arc::from("minecraft:air"),
                    |stack| Arc::clone(&stack.item),
                ),
                main_hand_count: main_hand.map_or(0, |stack| stack.count as i32),
                leather_boots: self.armor[3]
                    .as_ref()
                    .is_some_and(|stack| stack.item.as_ref() == "minecraft:leather_boots"),
            }
        });
        let cart_boxes = self
            .carts
            .iter()
            .map(|c| {
                let p = c.position;
                (
                    [p.x - 0.49, p.y, p.z - 0.49],
                    [p.x + 0.49, p.y + 0.7, p.z + 0.49],
                )
            })
            .collect();
        let arrow_boxes = self
            .creatures
            .projectiles
            .iter()
            .filter(|p| p.arrow.is_some())
            .map(|p| p.position)
            .chain(self.tridents.iter().map(|t| t.position))
            .map(|at| {
                let b = horizon_client::arrows::arrow_bounds(at);
                (b.min_position().components(), b.max_position().components())
            })
            .collect();
        let (guest_box_uuids, guest_boxes): (Vec<_>, Vec<_>) =
            self.lan_block_trigger_boxes().into_iter().unzip();
        let player_box_uuid = horizon_client::lan::playerdata::ints_uuid(self.acting_uuid());
        let building_player_boxes = self
            .lan_acting
            .is_some()
            .then(|| self.lan_building_boxes.boxes(self.dim));
        let dimension = match self.dim {
            Dim::Overworld => LevelDimension::Overworld,
            Dim::Nether => LevelDimension::Nether,
            Dim::End => LevelDimension::End,
        };
        let rules = self.level_rules();
        let fire_players: Vec<[f64; 3]> = self
            .player
            .iter()
            .filter(|_| !spectator)
            .map(|p| [p.position.x, p.position.y, p.position.z])
            .chain(self.lan.iter().flat_map(|h| {
                h.hub
                    .guests
                    .iter()
                    .filter(|g| h.hub.here(g) && !g.spectator())
                    .map(|g| g.position)
            }))
            .collect();
        let mut trigger_hooks: Vec<(Aabb, bool)> = self
            .hook
            .iter()
            .map(|h| (h.bounds(), h.applies_block_effects()))
            .collect();
        self.lan_hook_triggers(&mut trigger_hooks);
        // EntityType.LIGHTNING_BOLT: 0 x 0.
        let trigger_extras: Vec<Aabb> = self
            .bolts
            .iter()
            .map(|b| point_box(b.at, 0.0, 0.0))
            .collect();
        // `ContainerOpenersCounter.getEntitiesWithContainerOpen`: the players
        // whose menu still holds a container of the block at a position. A
        // double chest answers for both halves (`ChestBlockEntity$1` takes a
        // `CompoundContainer`), and a spectator's menu counts for nothing.
        let mut open_containers: Vec<(i32, i32, i32)> = Vec::new();
        for key in self.container.into_iter().filter(|_| !spectator).chain(
            self.lan
                .iter()
                .flat_map(|h| {
                    h.hub
                        .guests
                        .iter()
                        .filter(|g| h.hub.here(g) && !g.spectator())
                })
                .filter_map(|g| g.menu.as_ref().and_then(|m| m.pos)),
        ) {
            open_containers.push(key);
            if let Some(other) = self.chest_other_half(key) {
                open_containers.push(other);
            }
        }
        // CopperGolem.hasContainerOpen: the chest a golem opened, and its other half.
        let mut golem_chests: Vec<(i32, i32, i32)> = Vec::new();
        for &key in self.world.chest_openers.keys() {
            golem_chests.push(key);
            golem_chests.extend(self.chest_other_half(key));
        }
        let native_authoritative = self.native_authoritative();
        let mut level = GameLevel {
            gateway_servers: &mut self.be_anims.gateway_servers,
            border: self.border.geometry(),
            redstone_experiments: self
                .level
                .enabled_features
                .iter()
                .any(|feature| feature == "minecraft:redstone_experiments"),
            cart_boxes,
            item_frames: &self.item_frames,
            carts: &self.carts,
            arrow_boxes,
            entities: entity::entity_lists_of!(self),
            crystals: &self.crystals,
            trigger_hooks,
            trigger_extras,
            world: &mut self.world,
            facts: self.res.facts(),
            collision: self.collision,
            events: &mut self.level_events,
            dimension,
            darken,
            environment: match dimension {
                LevelDimension::Overworld => self.res.environment(),
                LevelDimension::Nether => self.res.nether_environment(),
                LevelDimension::End => self.res.end_environment(),
            },
            raining: rain > 0.2,
            thundering: thunder > 0.9,
            player_box,
            guest_boxes,
            guest_box_uuids,
            player_box_uuid,
            building_player_boxes,
            player_collision,
            biomes: world_biomes!(self),
            seed: self.level.seed,
            behaviours: self.res.behaviours(),
            items: world_items!(self),
            recipes: world_recipes!(self),
            // A client rolls no loot (LootTable.getRandomItems needs a
            // ServerLevel): it doesn't load the tables on its game thread.
            loot: if self.remote.is_some() {
                None
            } else {
                horizon_client::structure_blocks::loot_data(
                    &self.options.resources,
                    &pack_files(&self.world_tables),
                )
            },
            registry: self.pack.assets.registry,
            rules,
            difficulty: i32::from(self.ui.difficulty),
            fire_players,
            rand_sequences: &mut self.rand_sequences,
            pending: &mut self.pending,
            spill: &mut self.spill,
            tickets: self.tickets.levels(),
            random_tick_sections: &mut self.random_tick_sections,
            lid_openers: &mut self.lid_openers,
            open_containers: &open_containers,
            golem_chests: &golem_chests,
            client_side: self.remote.is_some(),
            native_authoritative,
            native_player: self.lan_acting_uuid,
            debug: self.world_debug,
            native_sounds: &mut self.creatures.lan_sounds,
            native_particles: &mut self.creatures.lan_particles,
            drop_tool: None,
            drop_breaker_at: None,
            drop_into: None,
        };
        let result = f(self.res.behaviours(), &mut level, &mut self.block_ticks);
        self.flush_spill();
        if self.lan.is_some() {
            self.flush_lan_particles();
            self.flush_native_sounds();
        }
        result
    }
    pub(super) fn native_block_ignite_allowed(
        &mut self,
        pos: BlockPos,
        cause: &'static str,
    ) -> bool {
        self.with_level(|_, level, _| level.allow_block_ignite(pos, cause))
    }
    pub(super) fn native_cauldron_level_change_allowed(
        &mut self,
        pos: BlockPos,
        from: i32,
        to: i32,
        reason: &'static str,
    ) -> bool {
        self.with_level(|_, level, _| level.allow_cauldron_level_change(pos, from, to, reason))
    }
    /// Level.setBlock(pos, state, UPDATE_ALL) with neighbour/shape updates.
    fn level_set(&mut self, pos: [i32; 3], state: BlockStateId) -> Result<()> {
        self.level_set_with_flags(pos, state, level_flags::UPDATE_ALL)
    }
    /// Level.setBlock with the caller's update flags.
    fn level_set_with_flags(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        flags: i32,
    ) -> Result<()> {
        self.with_level(|b, level, ticks| {
            b.set_block(
                level,
                ticks,
                BlockPos::new(pos[0], pos[1], pos[2]),
                state,
                flags,
            );
        });
        self.process_level_events()
    }
    fn attack_block(&mut self, pos: [i32; 3]) -> Result<()> {
        let note = self.with_level(|b, level, ticks| {
            b.attack_block(level, ticks, BlockPos::new(pos[0], pos[1], pos[2]))
        });
        // NoteBlock.attack awards the statistic.
        if note {
            self.award_custom_stat("play_noteblock", 1);
        }
        self.process_level_events()
    }
    /// `AbstractCandleBlock.getParticleOffsets`, in sixteenths of a block:
    /// `CandleBlock.PARTICLE_OFFSETS` by its `candles` (one to four wicks),
    /// then `CandleCakeBlock.PARTICLE_OFFSETS`, whose one wick sits a block up.
    fn candle_offsets(name: &str, candles: Option<usize>) -> &'static [[u8; 3]] {
        const OFFSETS: [&[[u8; 3]]; 5] = [
            &[[8, 8, 8]],
            &[[6, 7, 8], [10, 8, 7]],
            &[[8, 5, 10], [6, 7, 8], [9, 8, 7]],
            &[[7, 5, 9], [10, 7, 9], [6, 7, 6], [9, 8, 6]],
            &[[8, 16, 8]],
        ];
        if name.ends_with("candle_cake") {
            OFFSETS[4]
        } else {
            OFFSETS[candles.unwrap_or(1).clamp(1, 4) - 1]
        }
    }
    /// `AbstractCandleBlock.extinguish`: the smoke from each wick, at the
    /// block's `getParticleOffsets` (sixteenths of a block), rising 0.1.
    fn candle_smoke(&mut self, pos: [i32; 3], state: BlockStateId) {
        use horizon_client::particles::{ParticleOptions, ParticleType};
        let candles = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("candles")?.parse::<usize>().ok());
        let offsets = Self::candle_offsets(&self.res.facts().get(state).name, candles);
        for o in offsets {
            self.particles.add(
                &ParticleOptions::simple(ParticleType::Smoke),
                [
                    f64::from(pos[0]) + f64::from(o[0]) / 16.0,
                    f64::from(pos[1]) + f64::from(o[1]) / 16.0,
                    f64::from(pos[2]) + f64::from(o[2]) / 16.0,
                ],
                // AbstractCandleBlock.extinguish: ySpeed 0.1F, widened.
                [0.0, f64::from(0.1_f32), 0.0],
            );
        }
    }
    /// `LeverBlock.makeParticle` at full scale: a powered lever puffs one red
    /// dust on the clicking client (`LeverBlock.useWithoutItem`).
    fn lever_puff(&mut self, pos: [i32; 3]) {
        // The server's side of a guest's pull draws nothing.
        if self.lan_acting.is_some() {
            return;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let Some(registry_state) = self.pack.assets.registry.state(state) else {
            return;
        };
        let prop = |key: &str| registry_state.properties.get(key).map(String::as_str);
        if prop("powered") != Some("true") {
            return;
        }
        // FACING is horizontal; getConnectedDirection is the face's direction.
        let facing = match prop("facing") {
            Some("north") => [0, 0, -1],
            Some("south") => [0, 0, 1],
            Some("west") => [-1, 0, 0],
            Some("east") => [1, 0, 0],
            _ => return,
        };
        let connected = match prop("face") {
            Some("ceiling") => [0, -1, 0],
            Some("floor") => [0, 1, 0],
            _ => facing,
        };
        let at = |base: i32, i: usize| {
            f64::from(base) + 0.5 + 0.1 * f64::from(-facing[i]) + 0.2 * f64::from(-connected[i])
        };
        let p = [at(pos[0], 0), at(pos[1], 1), at(pos[2], 2)];
        self.particles.add(
            &horizon_client::particles::ParticleOptions::dust([1.0, 0.0, 0.0], 1.0),
            p,
            [0.0; 3],
        );
    }
    /// Track which chunks are ticking (Ticks needs chunk load/unload).
    fn sync_tick_chunks(&mut self) {
        if !self.tick_chunks.synchronize(&self.world.chunks) {
            return;
        }
        // Move only the added keys out so registration can borrow all of Game.
        let added = self.tick_chunks.take_added();
        for &key in &added {
            self.register_chunk_ticks(key);
        }
        for &key in self.tick_chunks.removed() {
            self.random_tick_sections.remove(&key);
            self.portals.sections.remove(&key);
            self.block_ticks
                .remove_chunk(horizon_core::position::ChunkPos::new(key.0, key.1));
            self.block_ticks
                .comparator_outputs
                .retain(|p, _| (p.x >> 4, p.z >> 4) != key);
            self.hoppers.retain(|p, _| (p.0 >> 4, p.2 >> 4) != key);
            // Its moving pistons were saved with it and load back with it.
            self.block_ticks
                .moving_pistons
                .retain(|(p, _)| (p.x >> 4, p.z >> 4) != key);
        }
        self.tick_chunks.recycle_added(added);
    }
    /// `DistanceManager`'s tickets of the level in place, from every player in
    /// it (the local player, LAN guests, a server's players; a spectator the
    /// rule skips holds none), its forced chunks and its portal tickets. A game
    /// with neither a server nor a player (the headless fixtures) ticks
    /// everywhere.
    fn update_chunk_tickets(&mut self) {
        let rule = self.ui.rules.bool("spectators_generate_chunks");
        let mut players: Vec<((i32, i32), bool)> = self.lan.as_ref().map_or_else(Vec::new, |h| {
            h.hub
                .guests
                .iter()
                .filter(|g| h.hub.here(g))
                .filter(|g| !g.fake) // fake players hold small forced tickets below
                .map(|g| (g.chunk(), g.spectator()))
                .collect()
        });
        if let Some(p) = &self.player {
            players.push((
                (
                    (p.position.x.floor() as i32) >> 4,
                    (p.position.z.floor() as i32) >> 4,
                ),
                self.spectator(),
            ));
        }
        // A spectator the rule skips is still a player: a game that has one
        // must not take the headless fixtures' tick-everywhere case below.
        let any_player = !players.is_empty();
        let sources = (!self.options.execution_role.is_client() || any_player).then(|| {
            horizon_client::ticket_levels::Sources {
                players: players
                    .into_iter()
                    .filter(|&(_, spectator)| game_mode::loads_chunks(rule, spectator))
                    .map(|(chunk, _)| chunk)
                    .collect(),
                // A native mod's ticket holds its chunk as `/forceload` does
                // (NeoForge's `TicketController` uses the same `FORCED` level).
                forced: self
                    .streaming
                    .forced
                    .iter()
                    .chain(
                        self.streaming
                            .mod_forced
                            .iter()
                            .filter(|c| !self.streaming.forced.contains(c)),
                    )
                    .copied()
                    .collect(),
                // Filled from the `PORTAL` tickets by `update`.
                portal: Vec::new(),
                // EnderDragonFight.tick: while a player watches the fight, the
                // arena holds a `DRAGON` ticket of radius 9 at (0, 0), so its
                // island ticks out to 7 chunks whatever the watcher's
                // simulation distance. The fight lives where the type has one.
                dragon: (self.dimension_type().has_ender_dragon_fight
                    && self.fight.arena_watched())
                .then_some((0, 0)),
                view_distance: self.settings.render_distance.clamp(2, 32),
                simulation_distance: self.settings.simulation_distance.max(2),
            }
        });
        // A fake player holds a forced chunk's 3x3 ticket, not a whole view distance.
        let sources = sources.map(|mut s| {
            if let Some(h) = self.lan.as_ref() {
                s.forced.extend(
                    h.hub
                        .guests
                        .iter()
                        .filter(|g| {
                            g.fake && h.hub.here(g) && game_mode::loads_chunks(rule, g.spectator())
                        })
                        .map(|g| g.chunk()),
                );
            }
            s
        });
        self.tickets.update(sources);
    }
    /// `ServerLevel.startTickingChunk`: a loaded chunk's saved ticks are
    /// scheduled from the game time at which it becomes a ticking chunk
    /// (block ticking, with its neighbours loaded). Chunks become ticking
    /// between ticks in vanilla: before this tick's clock advanced.
    fn start_ticking_chunks(&mut self) {
        let levels = self.tickets.levels();
        let world = &self.world;
        let started = self
            .tick_chunks
            .start_ticking(|(x, z)| levels.loaded_ticking(x, z) && ticking_chunk(world, x, z));
        let time = self.world.game_time - 1;
        for (x, z) in started {
            let pos = horizon_core::position::ChunkPos::new(x, z);
            self.block_ticks.blocks.unpack_chunk(pos, time);
            self.block_ticks.fluids.unpack_chunk(pos, time);
        }
    }

    /// `ServerLevel.tick`: moving block entities push after entity travel.
    fn tick_moving_pistons(&mut self) -> Result<()> {
        self.with_level(|b, level, ticks| b.tick_block_entities(level, ticks));
        let pushes = std::mem::take(&mut self.block_ticks.piston_pushes);
        self.push_piston_entities(pushes);
        self.process_level_events()?;
        self.sync_moving_pistons();
        Ok(())
    }
    /// Copies the moving-piston block entities into the world, where the
    /// collision queries and the renderer read them.
    ///
    /// A client runs this for every block change that ends a piston's move,
    /// so it stays linear in the pistons moving: a clock of a thousand pistons
    /// ends two thousand moves at once.
    fn sync_moving_pistons(&mut self) {
        use horizon_client::fast_hash::{FxHashMap, FxHashSet};
        let previous = std::mem::take(&mut self.world.moving_pistons);
        let current: Vec<_> = self
            .block_ticks
            .moving_pistons
            .iter()
            .map(|(p, piston)| ([p.x, p.y, p.z], piston.shape_state()))
            .collect();
        // The positions drawn by `current` or `fallback` so far.
        let mut drawn: FxHashSet<[i32; 3]> = current.iter().map(|(p, _)| *p).collect();
        let mut fallback: Vec<_> = previous
            .into_iter()
            .filter(|(pos, _)| !drawn.contains(pos))
            .collect();
        drawn.extend(fallback.iter().map(|(p, _)| *p));
        // A finished piston stays drawn until the chunk mesh shows the block
        // it placed, however many ticks that takes. Pistons share a few
        // sections, so each section is asked once.
        let older = std::mem::take(&mut self.world.moving_piston_fallback);
        let mut pending: FxHashMap<(i32, i32, i32), bool> = FxHashMap::default();
        for (pos, piston) in older {
            if drawn.contains(&pos) {
                continue;
            }
            let Some(section) = self.section_of(pos) else {
                continue;
            };
            if *pending
                .entry(section)
                .or_insert_with(|| self.section_mesh_pending_in(section))
            {
                drawn.insert(pos);
                fallback.push((pos, piston));
            }
        }
        self.world.moving_piston_fallback = fallback;
        self.world.moving_pistons = current;
    }
    /// `ServerLevel.tick`'s scheduled block then fluid ticks, one at a time:
    /// the side effects of each (a shrieker summoning a warden, drops, game
    /// events) happen before the next tick runs, as they do inline in vanilla.
    pub(super) fn tick_scheduled(&mut self) -> Result<()> {
        for fluids in [false, true] {
            let mut laps = horizon_client::spans::laps();
            let name = if fluids { "tb_fluids" } else { "tb_scheduled" };
            if !self.with_level(|b, level, ticks| b.scheduled_begin(level, ticks, fluids)) {
                laps.lap(name);
                continue;
            }
            loop {
                // Due ticks run against one level while each leaves nothing for
                // the game to apply (level events, spilled items or orbs): the
                // next one would see the same level built again.
                let more = self.with_level(|b, level, ticks| {
                    loop {
                        let more = b.scheduled_step(level, ticks, fluids);
                        if !more || !level.events.is_empty() || !level.spill.is_empty() {
                            break more;
                        }
                    }
                });
                if !self.level_events.is_empty() {
                    self.process_level_events()?;
                }
                if !more {
                    break;
                }
            }
            self.with_level(|b, _, ticks| b.scheduled_finish(ticks, fluids));
            laps.lap(name);
        }
        Ok(())
    }
    /// Side effects reported by the behaviour library, in vanilla order.
    fn process_level_events(&mut self) -> Result<()> {
        let events = std::mem::take(&mut self.level_events);
        self.lan_record_events(&events);
        dispense_fixture::record_events(&events);
        for event in events {
            match event {
                LevelEvent::LevelEvent {
                    id: 2001,
                    pos,
                    data,
                } => {
                    // LevelEventHandler: destroy particles and the block's
                    // break sound (none for air).
                    let state = BlockStateId(data as u32);
                    let p = [pos.x, pos.y, pos.z];
                    self.destroy_particles(p, state);
                    if self.res.facts().is_air(state) {
                        continue;
                    }
                    let sounds = self.res.facts().get(state).sounds.clone();
                    let c = [
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ];
                    self.audio.play(
                        &sounds.break_sound,
                        Some(c),
                        (sounds.volume + 1.0) / 2.0,
                        sounds.pitch * 0.8,
                    );
                }
                LevelEvent::LevelEvent {
                    id: 3006,
                    pos,
                    data,
                } => self.sculk_charge_event(pos, data),
                LevelEvent::LevelEvent { id: 3007, pos, .. } => self.shriek_event(pos),
                // LevelEventHandler 1010 and 1011: a jukebox's song starts
                // (`playJukeboxSong`) or stops, and the parrots nearby dance or stop.
                LevelEvent::LevelEvent {
                    id: 1010,
                    pos,
                    data,
                } => self.jukebox_song_event([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent { id: 1011, pos, .. } => {
                    self.jukebox_stop_event([pos.x, pos.y, pos.z])
                }
                // LevelEventHandler 2019 and 2020: the crack on a face of the
                // block being mined, with its hit sound for 2020.
                LevelEvent::LevelEvent {
                    id: id @ (2019 | 2020),
                    pos,
                    data,
                } => {
                    let face = horizon_core::geometry::Direction::ALL[data.clamp(0, 5) as usize];
                    self.breaking_block_effects([pos.x, pos.y, pos.z], face.normal(), id == 2020);
                }
                LevelEvent::LevelEvent {
                    id: 3008,
                    pos,
                    data,
                } => self.brush_complete_event([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent { id: 2004, pos, .. } => {
                    self.spawner_burst([pos.x, pos.y, pos.z])
                }
                // LevelEventHandler 1505: the growth particles, then the sound.
                LevelEvent::LevelEvent {
                    id: 1505,
                    pos,
                    data,
                } => {
                    self.growth_particles(pos, data);
                    let at = [pos.x, pos.y, pos.z].map(|v| f64::from(v) + 0.5);
                    self.audio
                        .play("minecraft:item.bone_meal.use", Some(at), 1.0, 1.0);
                }
                LevelEvent::LevelEvent {
                    id: 2015,
                    pos,
                    data,
                } => self.dragon_egg_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent {
                    id: 2016,
                    pos,
                    data,
                } => self.shulker_teleport_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent {
                    id: 2017,
                    pos,
                    data,
                } => self.consume_teleport_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent {
                    id: 2018,
                    pos,
                    data,
                } => self.enderman_teleport_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent { id: 2003, pos, .. } => {
                    self.eye_break_particles([pos.x, pos.y, pos.z]);
                }
                LevelEvent::LevelEvent {
                    id: 2013,
                    pos,
                    data,
                } => self.smash_dust(pos, data),
                // LevelEventHandler 2002 and 2007: a potion's splash.
                LevelEvent::LevelEvent {
                    id: 2002,
                    pos,
                    data,
                } => self.potion_splash_particles(pos, data, false),
                LevelEvent::LevelEvent {
                    id: 2007,
                    pos,
                    data,
                } => self.potion_splash_particles(pos, data, true),
                LevelEvent::LevelEvent {
                    id: id @ 3002..=3005,
                    pos,
                    data,
                } => self.block_particle_event(id, pos, data),
                LevelEvent::LevelEvent {
                    id: 1500,
                    pos,
                    data,
                } => self.composter_fill_event(pos, data == 1),
                // LevelEventHandler 1504: the stalactite's drip particle.
                LevelEvent::LevelEvent { id: 1504, pos, .. } => self.dripstone_drip_particle(pos),
                // LevelEventHandler 2006: the dragon fireball's burst.
                LevelEvent::LevelEvent {
                    id: 2006,
                    pos,
                    data,
                } => self.dragon_fireball_burst(pos, data),
                // LevelEventHandler 2009: a wet sponge drying.
                LevelEvent::LevelEvent { id: 2009, pos, .. } => {
                    self.puffs_above_block(pos, horizon_client::particles::ParticleType::Cloud)
                }
                // LevelEventHandler 2011 and 2012: a bee growing a crop, a
                // turtle egg laid.
                LevelEvent::LevelEvent {
                    id: 2011 | 2012,
                    pos,
                    data,
                } => self.turtle_egg_particles(pos, data),
                // LevelEventHandler 2014: a straw bed destroyed, with no sound.
                LevelEvent::LevelEvent {
                    id: 2014,
                    pos,
                    data,
                } => self.destroy_particles([pos.x, pos.y, pos.z], BlockStateId(data as u32)),
                // EnderDragonFight.spawnNewGateway: an always-visible burst
                // and a sound at the gateway for every nearby player.
                LevelEvent::LevelEvent { id: 3000, pos, .. } => {
                    let at = [
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ];
                    self.particles.add_with(
                        &horizon_client::particles::ParticleOptions::simple(
                            horizon_client::particles::ParticleType::ExplosionEmitter,
                        ),
                        at,
                        [0.0; 3],
                        true,
                        true,
                    );
                    let random = self.particles.level_random();
                    let pitch = (1.0 + (random.next_float() - random.next_float()) * 0.2) * 0.7;
                    self.audio.play_unshared(
                        "minecraft:block.end_gateway.spawn",
                        Some(at),
                        10.0,
                        pitch,
                    );
                }
                // LevelEventHandler 3009: a sniffer egg cracking.
                LevelEvent::LevelEvent { id: 3009, pos, .. } => {
                    self.sniffer_egg_crack_particles(pos)
                }
                LevelEvent::LevelEvent {
                    id: id @ 3011..=3021,
                    pos,
                    data,
                } => self.trial_level_event(id, [pos.x, pos.y, pos.z], data),
                // LevelEvent.SOUND_PORTAL_TRAVEL: LevelEventHandler.levelEvent
                // plays SimpleSoundInstance.forLocalAmbience (ambient, no
                // position). EndPortalBlock.entityInside travels through
                // TeleportTransition.PLAY_PORTAL_SOUND, which sends 1032 to
                // the traveling player. The packet's position is BlockPos.ZERO,
                // so a positional play would not be heard; at None is the
                // relative sound, and it is not captured for other players.
                LevelEvent::LevelEvent { id: 1032, .. } => {
                    let pitch = self.particles.level_random().next_float() * 0.4 + 0.8;
                    self.audio.play_on(
                        horizon_client::sound_engine::AMBIENT,
                        "minecraft:block.portal.travel",
                        None,
                        0.25,
                        pitch,
                    );
                }
                LevelEvent::LevelEvent { id, pos, .. } => {
                    let c = Some([
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ]);
                    let sound = match id {
                        1503 => Some(("minecraft:block.end_portal_frame.fill", 1.0)),
                        1501 => Some(("minecraft:block.lava.extinguish", 0.5)),
                        1009 => Some(("minecraft:block.fire.extinguish", 0.5)),
                        1502 => Some(("minecraft:block.redstone_torch.burnout", 0.5)),
                        1033 => Some(("minecraft:block.chorus_flower.grow", 1.0)),
                        1034 => Some(("minecraft:block.chorus_flower.death", 1.0)),
                        1045 => Some(("minecraft:block.pointed_dripstone.land", 2.0)),
                        1052 => Some(("minecraft:block.sulfur_spike.land", 2.0)),
                        1046 => Some((
                            "minecraft:block.pointed_dripstone.drip_lava_into_cauldron",
                            2.0,
                        )),
                        1047 => Some((
                            "minecraft:block.pointed_dripstone.drip_water_into_cauldron",
                            2.0,
                        )),
                        // LevelEvent.SOUND_ZOMBIE_WOODEN_DOOR (attack, break).
                        1019 => Some(("minecraft:entity.zombie.attack_wooden_door", 2.0)),
                        1021 => Some(("minecraft:entity.zombie.break_wooden_door", 2.0)),
                        1026 => Some(("minecraft:entity.zombie.infect", 2.0)),
                        1027 => Some(("minecraft:entity.zombie_villager.converted", 2.0)),
                        1030 => Some(("minecraft:block.anvil.use", 1.0)),
                        1029 => Some(("minecraft:block.anvil.destroy", 1.0)),
                        1031 => Some(("minecraft:block.anvil.land", 0.3)),
                        1042 => Some(("minecraft:block.grindstone.use", 1.0)),
                        1044 => Some(("minecraft:block.smithing_table.use", 1.0)),
                        // LevelEvent.SOUND_PAGE_TURN (a lectern's page).
                        1043 => Some(("minecraft:item.book.page_turn", 1.0)),
                        // LevelEvent.SOUND_SPLASH_POTION and its instant twin.
                        1053 | 1054 => Some(("minecraft:entity.splash_potion.break", 1.0)),
                        _ => None,
                    };
                    if let Some((name, volume)) = sound {
                        let pitch = match id {
                            1009 => {
                                2.6 + (self.world.random.next_float()
                                    - self.world.random.next_float())
                                    * 0.8
                            }
                            // LevelEventHandler draws these from the client
                            // level's random, before 1501's and 1502's smoke.
                            1501 | 1502 => {
                                let r = self.particles.level_random();
                                2.6 + (r.next_float() - r.next_float()) * 0.8
                            }
                            1045 | 1046 | 1047 | 1052 => {
                                self.particles.level_random().next_float() * 0.1 + 0.9
                            }
                            1053 | 1054 => {
                                // LevelEventHandler's client random: impact audio
                                // must not change the server's bottle XP stream.
                                self.particles.level_random().next_float() * 0.1 + 0.9
                            }
                            1029 | 1030 | 1042 | 1043 | 1044 => {
                                self.world.random.next_float() * 0.1 + 0.9
                            }
                            1026 | 1027 => {
                                let r = self.particles.level_random();
                                (r.next_float() - r.next_float()) * 0.2 + 1.0
                            }
                            1019 | 1021 => {
                                (self.world.random.next_float() - self.world.random.next_float())
                                    * 0.2
                                    + 1.0
                            }
                            _ => 1.0,
                        };
                        if matches!(
                            id,
                            1019 | 1021
                                | 1026
                                | 1027
                                | 1029
                                | 1030
                                | 1045
                                | 1052
                                | 1053
                                | 1054
                                | 1501
                                | 1503
                        ) {
                            // Clients play these sounds from their level events;
                            // capturing it also sends a duplicate sound packet.
                            self.audio.play_unshared(name, c, volume, pitch);
                        } else {
                            self.audio.play(name, c, volume, pitch);
                        }
                    }
                    // LevelEventHandler draws smoke after the sound.
                    match id {
                        1501 => self.puffs_above_block(
                            pos,
                            horizon_client::particles::ParticleType::LargeSmoke,
                        ),
                        1502 => self.torch_burnout_smoke(pos),
                        1503 => self.end_portal_frame_smoke(pos),
                        _ => {}
                    }
                }
                LevelEvent::ContainerLootTable { pos, tag } => {
                    let key = (pos.x, pos.y, pos.z);
                    if let Some(mut entity) = self.world.block_entities.remove(&key) {
                        self.take_loot_table(key, &tag, &mut entity);
                        self.world.block_entities.insert(key, entity);
                        self.world.modified.insert((pos.x >> 4, pos.z >> 4));
                    }
                }
                // A prime the mods refused is silent: no fuse sound, no game event.
                LevelEvent::Sound { pos, ref sound, .. }
                    if self.tnt_refused == Some(pos)
                        && &**sound == "minecraft:entity.tnt.primed" => {}
                LevelEvent::GameEvent {
                    pos,
                    event: "minecraft:prime_fuse",
                    ..
                } if self.tnt_refused == Some(pos) => self.tnt_refused = None,
                LevelEvent::Sound {
                    pos,
                    sound,
                    volume,
                    pitch,
                } => {
                    let c = [
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ];
                    let name = if sound.contains(':') {
                        sound.to_string()
                    } else {
                        format!("minecraft:{sound}")
                    };
                    self.audio.play(&name, Some(c), volume, pitch);
                }
                // Handled as the behaviour emits it (GameLevel::emit).
                LevelEvent::DropResources { pos, state } => {
                    self.with_level(|_, level, _| level.drop_resources_now(pos, state));
                }
                LevelEvent::FallingBlock {
                    pos,
                    state,
                    hurt_per_distance,
                } => {
                    let mut damage = horizon_client::falling_block::Damage::falling(
                        &self.res.facts().get(state).name,
                    );
                    if let Some(per_distance) = hurt_per_distance {
                        damage.hurt_entities = true;
                        damage.per_distance = per_distance;
                    }
                    let p = Vec3::new(
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y),
                        f64::from(pos.z) + 0.5,
                    );
                    self.next_entity += 1;
                    self.falling.push(FallingBlock {
                        rotation: [0.0, 0.0],
                        join: Default::default(),
                        id: self.next_entity,
                        state,
                        start_pos: pos,
                        position: p,
                        previous: p,
                        velocity: Vec3::ZERO,
                        age: 0,
                        no_gravity: false,
                        portal: Default::default(),
                        drop_item: true,
                        fall_distance: 0.0,
                        on_ground: false,
                        stuck: Vec3::ZERO,
                        damage,
                        saved: None,
                    });
                }
                LevelEvent::Player { pos, effect, .. } => match effect {
                    PlayerEffect::OpenMenu => self.open_menu([pos.x, pos.y, pos.z]),
                    PlayerEffect::Sleep if self.beds_destroy_on_use() => {
                        self.destroy_bed_on_use([pos.x, pos.y, pos.z])?
                    }
                    PlayerEffect::Sleep => self.try_sleep([pos.x, pos.y, pos.z]),
                    PlayerEffect::GiveItem(item) => {
                        if let Some(rest) = self.give(ItemStack {
                            item,
                            count: 1,
                            damage: 0,
                            extra: None,
                        }) {
                            self.pop_resource([pos.x, pos.y, pos.z], rest);
                        }
                    }
                    // Popped as the behaviour emits it (GameLevel::emit).
                    PlayerEffect::DropItem(_) => {}
                    PlayerEffect::EatCake => {
                        // CakeBlock.eat
                        self.award_custom_stat("eat_cake_slice", 1);
                        if let Some(p) = self.player.as_mut() {
                            let food = &mut p.health.food;
                            food.food = (food.food + 2).min(20);
                            food.saturation = (food.saturation + 0.4).min(food.food as f32);
                        }
                    }
                    PlayerEffect::RespawnAnchor => {
                        self.respawn_anchor_used([pos.x, pos.y, pos.z])?
                    }
                    PlayerEffect::InteractLoot(_) => {}
                },
                LevelEvent::PrimeTnt { pos, rot } => self.prime_tnt_event(pos, rot),
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "dispense",
                } => self.dispense_from(pos, state)?,
                LevelEvent::BlockEvent {
                    pos, kind: "golem", ..
                } => self.check_golem_spawn([pos.x, pos.y, pos.z])?,
                LevelEvent::BlockEvent {
                    pos,
                    kind: "command_block",
                    ..
                } => self.command_block_tick(pos),
                LevelEvent::BlockEvent {
                    pos,
                    kind: "structure_block",
                    ..
                } => self.structure_block_neighbor_changed((pos.x, pos.y, pos.z)),
                LevelEvent::BlockEvent {
                    pos,
                    kind: "test_block",
                    ..
                } => self.test_block_neighbor_changed((pos.x, pos.y, pos.z)),
                LevelEvent::BlockEvent {
                    pos,
                    kind: "test_block_tick",
                    ..
                } => self.reset_test_block((pos.x, pos.y, pos.z)),
                LevelEvent::BlockEvent {
                    pos,
                    kind: "brushable",
                    ..
                } => self.brushable_tick([pos.x, pos.y, pos.z]),
                LevelEvent::BlockEvent {
                    pos, kind: "hatch", ..
                } => {
                    // FrogspawnBlock.hatchFrogspawn: 2-5 tadpoles.
                    let at = [
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ];
                    self.audio
                        .play("minecraft:block.frogspawn.hatch", Some(at), 1.0, 1.0);
                    let count = 2 + self.world.random.next_int_bound(4).unwrap_or(0);
                    for _ in 0..count {
                        // getRandomTadpolePositionOffset: 0.2..0.8 inside the block.
                        let r = &mut self.world.random;
                        let off = |r: &mut horizon_core::random::LegacyRandom| {
                            f64::from(r.next_float()) * 0.6 + 0.2
                        };
                        let (dx, dz) = (off(r), off(r));
                        let spawn = [
                            f64::from(pos.x) + dx,
                            f64::from(pos.y) - 0.5,
                            f64::from(pos.z) + dz,
                        ];
                        let _ = self.spawn_mob_raw("minecraft:tadpole", spawn, true);
                    }
                }
                LevelEvent::BlockEvent {
                    pos,
                    kind: "hatch_sniffer",
                    ..
                } => self.hatch_sniffer([pos.x, pos.y, pos.z]),
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "hatch_turtles",
                } => self.hatch_turtles([pos.x, pos.y, pos.z], state),
                LevelEvent::BlockEvent {
                    pos,
                    kind: "portal_spawn",
                    ..
                } => self.portal_spawn([pos.x, pos.y, pos.z]),
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "spawn_ghastling",
                } => self.spawn_ghastling([pos.x, pos.y, pos.z], state),
                LevelEvent::GameEvent { pos, event, state } => {
                    self.sculk_level_event(pos, event, state)
                }
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "shrieker_respond",
                } => self.shrieker_block_event(pos, state),
                LevelEvent::BlockEntityEvent { pos, a, b, .. } => {
                    self.block_entity_event([pos.x, pos.y, pos.z], a, b)
                }
                // NoteBlock.triggerEvent: a NOTE particle above, coloured by
                // the note (`note / 24.0` in the x speed).
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "note_particle",
                } => {
                    use horizon_client::particles::{ParticleOptions, ParticleType};
                    let note = self
                        .pack
                        .assets
                        .registry
                        .state(state)
                        .and_then(|s| s.properties.get("note")?.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    self.particles.add(
                        &ParticleOptions::simple(ParticleType::Note),
                        [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y) + 1.2,
                            f64::from(pos.z) + 0.5,
                        ],
                        [note / 24.0, 0.0, 0.0],
                    );
                }
                // NoteBlock.getCustomSoundId: the skull above's note block sound,
                // played at volume 3 (vanilla's pitch for a head is 1).
                LevelEvent::BlockEvent {
                    pos,
                    kind: "note_custom_sound",
                    ..
                } => {
                    if let Some(BlockEntity::Skull(skull)) =
                        self.world.block_entities.get(&(pos.x, pos.y + 1, pos.z))
                        && let Some(sound) = skull.note_block_sound.as_deref()
                    {
                        let at = [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y) + 0.5,
                            f64::from(pos.z) + 0.5,
                        ];
                        self.audio.play(sound, Some(at), 3.0, 1.0);
                    }
                }
                // `level.addParticle`: only the local player's own click draws it.
                LevelEvent::BlockEvent {
                    kind: "candle_smoke",
                    ..
                } if self.lan_acting.is_some() => {}
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "candle_smoke",
                } => self.candle_smoke([pos.x, pos.y, pos.z], state),
                // The block at `pos` has just replaced `state`: Block.pushEntitiesUp.
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "push_entities_up",
                } => self.push_entities_up(pos, state),
                LevelEvent::GrowTree { .. }
                | LevelEvent::PlaceFeature { .. }
                | LevelEvent::BlockEvent { .. } => {}
            }
        }
        Ok(())
    }
    /// Menus opened by PlayerEffect::OpenMenu.
    fn open_menu(&mut self, pos: [i32; 3]) {
        let name = self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name
            .clone();
        let key = (pos[0], pos[1], pos[2]);
        if self.chest_menu_blocked(key, &name) {
            // The use still succeeds, but no menu, stat or piglin anger.
            return;
        }
        // BaseContainerBlockEntity.createMenu: a lock the opener cannot
        // unlock refuses the menu with vanilla's action bar and sound.
        if self.refuse_locked_container(key, &name) {
            return;
        }
        if &*name == "minecraft:lectern" {
            self.open_lectern(pos);
            return;
        }
        if horizon_client::command_block::Mode::of_block(&name).is_some() {
            self.open_command_block(key);
            return;
        }
        // The game master blocks' screens open on the player's own client
        // (`BlockEntity.usedBy`); the server has nothing to open.
        if matches!(
            &*name,
            "minecraft:structure_block"
                | "minecraft:jigsaw"
                | "minecraft:test_block"
                | "minecraft:test_instance_block"
        ) {
            return;
        }
        // SignBlock.useWithoutItem: the text editor, not a container.
        let block = self.world.block(pos[0], pos[1], pos[2]);
        if self.res.facts().get(block).sign_form().is_some() {
            self.use_sign_empty_hand(pos);
            return;
        }
        if &*name == "minecraft:crafting_table" {
            self.craft_size = 3;
            self.craft_table = Some(pos);
        } else if &*name == "minecraft:ender_chest" {
            self.open_ender_chest(key);
            // EnderChestBlock.useWithoutItem
            self.award_custom_stat("open_enderchest", 1);
            self.anger_piglins(true);
            // EnderChestBlockEntity.startOpen: the first opener's CONTAINER_OPEN
            // (ContainerOpenersCounter.incrementOpeners), as for chests below.
            if self.container_opener_event(key, true) {
                self.post_block_game_event(
                    "minecraft:container_open",
                    [key.0, key.1, key.2],
                    Some(horizon_client::mobs::Who::Player),
                    None,
                );
                self.container_edge_sound(key, true);
            }
        } else if matches!(&*name, "minecraft:beacon" | "minecraft:enchanting_table")
            && self.open_station(pos, &name)
        {
            // Station menus whose block has a block entity, not a container.
            self.world.block_entities.get_or_insert_with(key, || {
                BlockEntity::for_block(&name).unwrap_or(BlockEntity::Chest(Vec::new()))
            });
        } else if storage::is_shulker_box(&name) && !self.shulker_can_open(pos) {
            // ShulkerBoxBlock.canOpen: the lid has no room; nothing opens.
        } else if let Some(entity) = BlockEntity::for_block(&name) {
            self.world.block_entities.get_or_insert_with(key, || entity);
            horizon_client::spans::time("open_loot", || self.unpack_opened_loot(key, &name));
            let centre = [
                f64::from(key.0) + 0.5,
                f64::from(key.1) + 0.5,
                f64::from(key.2) + 0.5,
            ];
            horizon_client::spans::time("open_maps", || {
                self.resolve_container_maps(key, centre);
            });
            self.container = Some(key);
            self.open_container_slots = Some((key, self.container_size()));
            let first = self.container_opener_event(key, true);
            if first {
                self.post_block_game_event(
                    "minecraft:container_open",
                    [key.0, key.1, key.2],
                    Some(horizon_client::mobs::Who::Player),
                    None,
                );
            }
            let stat = match &*name {
                "minecraft:chest" => "open_chest",
                "minecraft:trapped_chest" => "trigger_trapped_chest",
                "minecraft:barrel" => "open_barrel",
                "minecraft:hopper" => "inspect_hopper",
                "minecraft:dropper" => "inspect_dropper",
                "minecraft:dispenser" => "inspect_dispenser",
                n if storage::is_shulker_box(n) => "open_shulker_box",
                _ => "",
            };
            if !stat.is_empty() {
                self.award_custom_stat(stat, 1);
            }
            // Opening a piglin-guarded container angers the piglins who see it.
            if self.guarded_by_piglins(&name) {
                self.anger_piglins(true);
            }
            if first {
                self.container_edge_sound(key, true);
            }
        } else if self.open_mod_menu(key) {
            // A mod block's container (`native_block_entity_container`).
            self.container = Some(key);
        } else if self.open_station(pos, &name) {
            // Workstation menus: game/stations.rs.
        } else {
            self.message = Some((
                format!(
                    "{} is not available yet",
                    name.trim_start_matches("minecraft:").replace('_', " ")
                ),
                Instant::now(),
            ));
            return;
        }
        self.screen = Screen::Inventory;
        self.capture(false);
        self.right = false;
    }
    /// `DecoratedPotBlock.playerWillDestroy`: a pot mined with an item in
    /// `breaks_decorated_pots`, unless an enchantment prevents shattering, is
    /// cracked first (flags 260), so it shatters into its sherds.
    fn crack_mined_pot(&mut self, pos: [i32; 3], state: BlockStateId) -> BlockStateId {
        let registry = &self.pack.assets.registry;
        let Some(mut pot) = registry
            .state(state)
            .filter(|s| s.name == "minecraft:decorated_pot")
            .cloned()
        else {
            return state;
        };
        let Some(held) = self.held() else {
            return state;
        };
        let breaks = self
            .res
            .items()
            .item_tags
            .get("minecraft:breaks_decorated_pots")
            .is_some_and(|t| t.contains(&*held.item));
        let enchantments = &self.stations().enchantments;
        let kept = held
            .enchantments()
            .iter()
            .any(|(e, _)| enchantments.in_tag("minecraft:prevents_decorated_pot_shattering", e));
        if !breaks || kept {
            return state;
        }
        pot.properties.insert("cracked".into(), "true".into());
        let Ok(cracked) = registry.resolve(&pot) else {
            return state;
        };
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.set_block(level, ticks, at, cracked, 260));
        cracked
    }
    /// `ItemStack.mineBlock` for the held stack once a survival player has
    /// mined the block `name`: an item with a `tool` component counts as used
    /// and loses its `damage_per_block` on a block that doesn't break
    /// instantly (`ShearsItem`: on any block but fire).
    fn mine_block_with_held(&mut self, name: &str, hardness: f32) {
        let Some(stack) = self.inventory[self.selected].clone() else {
            return;
        };
        let Some(per_block) = self.items().mining_damage(&stack) else {
            return;
        };
        let max = self.items().mining_damage_capacity(&stack);
        let wears = if &*stack.item == "minecraft:shears" {
            !self.items().block_in_tag("minecraft:fire", name)
        } else {
            hardness != 0.0
        };
        if wears && per_block > 0 && max > 0 {
            self.damage_held_with_max(per_block, max);
        }
        self.award_stat("used", &stack.item, 1);
    }
    fn break_block(&mut self, pos: [i32; 3]) -> Result<()> {
        self.player_break_block(pos).map(|_| ())
    }
    /// `ServerPlayerGameMode.destroyBlock`: whether the block was removed.
    /// A break inside another (a mod breaking more blocks from its
    /// `native_destroy_block`) keeps the outer break's own events.
    pub(super) fn player_break_block(&mut self, pos: [i32; 3]) -> Result<bool> {
        let outer = self.lan_break_pos.replace(pos);
        let broken = self.with_player_actor(|game| game.break_block_as_player(pos));
        self.lan_break_pos = outer;
        broken
    }
    fn break_block_as_player(&mut self, pos: [i32; 3]) -> Result<bool> {
        // DebugStickItem.canDestroyBlock: a left click selects a property and
        // never destroys the block.
        if !self.debug_stick_can_destroy(pos)? {
            return Ok(false);
        }
        // ServerPlayerGameMode.destroyBlock.
        if self.block_action_restricted(pos) {
            return Ok(false);
        }
        if !self
            .res
            .items()
            .can_destroy_block(self.held(), self.creative)
        {
            return Ok(false);
        }
        // ServerPlayerGameMode.destroyBlock (and MultiPlayerGameMode.destroyBlock
        // on the client): a GameMasterBlock stays when the player cannot use
        // game master blocks (Player.canUseGameMasterBlocks).
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if !self.can_use_game_master_blocks()
            && is_game_master_block(&self.res.facts().get(state).name)
        {
            return Ok(false);
        }
        // Native mods, once the game allowed the break: a canceled one keeps
        // the block, and a guest's acknowledgement puts it back in its world.
        let mut canceled = false;
        self.native_destroy_block(pos, state, &mut canceled);
        if canceled {
            return Ok(false);
        }
        self.pad_rumble(horizon_client::gamepad::Rumble::BlockBroken);
        let broken = state;
        let facts = self.res.facts().get(state).clone();
        let state = self.crack_mined_pot(pos, state);
        // Block.playerWillDestroy: mining their gold angers all nearby piglins.
        if self.guarded_by_piglins(&facts.name) {
            self.anger_piglins(false);
        }
        // The block entity as it was, for what the drops copy from it.
        let key = (pos[0], pos[1], pos[2]);
        let loot_entity = if self.creative {
            None
        } else {
            self.loot_block_entity(pos, &facts.name)
        };
        if let Some(entity) = self.world.block_entities.get(&key).cloned() {
            let name = self.world.block_names.get(&key).cloned();
            self.pending.record(pos, &entity, &facts.name, name);
        } else if let Some(tag) = spill::saved_entity(&self.world, pos) {
            self.pending.record_saved(pos, &tag);
        }
        self.creaking_heart_broken(pos, true);
        let hive = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Beehive(h)) => self.hive_state(pos).map(|s| ((**h).clone(), s)),
            _ => None,
        };
        // playerWillDestroy and Level.removeBlock through the vanilla behaviour
        // library: the block entity's side effects (containers spill), neighbour
        // and shape updates, and the destroy event (particles, sound).
        let creative = self.creative;
        // TripWireBlock.playerWillDestroy: shears disarm the string first (flags
        // 260), so taking it down does not trip the hook.
        if &*facts.name == "minecraft:tripwire"
            && self.held().is_some_and(|s| &*s.item == "minecraft:shears")
            && let Some(disarmed) = self.with_props(state, &[("disarmed", "true")])
        {
            let at = BlockPos::new(pos[0], pos[1], pos[2]);
            self.with_level(|b, level, ticks| b.set_block(level, ticks, at, disarmed, 260));
            self.post_block_game_event("minecraft:shear", pos, self.sculk_actor(), None);
        }
        if creative && storage::is_shulker_box(&facts.name) {
            self.drop_creative_shulker(pos);
        }
        // BeehiveBlock.playerWillDestroy: a creative break still drops the hive
        // with its bees and honey level.
        if creative
            && self.ui.rules.bool("block_drops")
            && let Some((h, (honey, _))) = &hive
            && (!h.bees.is_empty() || honey.is_some_and(|l| l > 0))
            && let Some(stack) = self.picked_stack(pos, true)
        {
            let at = pos.map(f64::from);
            self.spawn_item_own(stack, at, 10);
        }
        let correct_tool =
            !facts.requires_tool || self.res.items().correct_for_drops(self.held(), &facts.name);
        let removed = self.with_level(|b, level, ticks| {
            b.player_destroy_block(
                level,
                ticks,
                BlockPos::new(pos[0], pos[1], pos[2]),
                creative,
                correct_tool,
            )
        });
        if removed && !creative {
            let held = self.held().cloned();
            self.mine_block_with_held(&facts.name, facts.hardness);
            let harvest = !facts.requires_tool
                || self
                    .res
                    .items()
                    .correct_for_drops(held.as_ref(), &facts.name);
            if harvest {
                // Block.playerDestroy: the mined statistic, the exhaustion and the drops.
                self.award_stat("mined", &facts.name, 1);
                if let Some(p) = self.player.as_mut() {
                    p.cause_food_exhaustion(0.005);
                }
                let drops = self.block_loot_at(pos, state, held.as_ref(), loot_entity);
                let silk = held
                    .as_ref()
                    .is_some_and(|h| h.enchantment_level("minecraft:silk_touch") > 0);
                for d in drops {
                    if &*d.item != "minecraft:air" {
                        self.pop_break_drop(pos, d);
                    }
                }
                // BeehiveBlock.playerDestroy: silk touch keeps the bees in, so
                // the trigger counts them; otherwise they are out already.
                if let Some((hive, (honey, facing))) = hive {
                    let bees = if silk { hive.bees.len() as i64 } else { 0 };
                    if !silk {
                        self.hive_broken_by_player(pos, hive, honey, facing);
                    }
                    self.trigger_bee_nest_destroyed(broken, held.as_ref(), bees);
                }
                // InfestedBlock.spawnAfterBreak, unless the tool has silk touch
                // (#prevents_infested_spawns).
                if facts.name.starts_with("minecraft:infested_") && !silk {
                    self.spawn_infestation(pos);
                }
                // Block.tryDropExperience samples before Silk Touch removes the XP.
                // SpawnerBlock.spawnAfterBreak ignores Silk Touch. Both sample even
                // when Block.popExperience suppresses the award via block_drops.
                let sampled_xp = self.block_experience(&facts.name);
                let xp = if silk && &*facts.name != "minecraft:spawner" {
                    0
                } else {
                    sampled_xp
                };
                if xp > 0 && self.ui.rules.bool("block_drops") {
                    self.spawn_orbs(
                        [
                            f64::from(pos[0]) + 0.5,
                            f64::from(pos[1]) + 0.5,
                            f64::from(pos[2]) + 0.5,
                        ],
                        xp,
                    );
                }
                // TurtleEggBlock.playerDestroy: after the drops, one egg of a
                // mined stack is taken and the rest stay.
                self.with_level(|b, level, ticks| {
                    b.turtle_egg_destroyed_by_player(
                        level,
                        ticks,
                        BlockPos::new(pos[0], pos[1], pos[2]),
                        broken,
                    );
                });
            }
            // IceBlock.playerDestroy: the ice melts back into water (the
            // #prevents_ice_melting tool tag holds only silk touch).
            let prevents_melting = held
                .as_ref()
                .is_some_and(|h| h.enchantment_level("minecraft:silk_touch") > 0);
            self.with_level(|b, level, ticks| {
                b.ice_destroyed_by_player(
                    level,
                    ticks,
                    BlockPos::new(pos[0], pos[1], pos[2]),
                    broken,
                    prevents_melting,
                );
            });
        }
        self.pending.clear(pos);
        self.process_level_events()?;
        Ok(removed)
    }
    /// `Block.popResource` for a player's break, or into `break_collect`
    /// when a mod takes the drops to the player's inventory: the stack still
    /// gets what the block entity copies into it, and nothing is kept where
    /// blocks don't drop.
    fn pop_break_drop(&mut self, pos: [i32; 3], mut stack: ItemStack) {
        if self.break_collect.is_none() {
            return self.pop_resource(pos, stack);
        }
        self.pending.attach(pos, &mut stack);
        if self.ui.rules.bool("block_drops")
            && let Some(kept) = self.break_collect.as_mut()
        {
            kept.push(stack);
        }
    }
    /// The `ItemStack.interactLivingEntity` fallback in `Player.interactOn`,
    /// currently for native Mob entries after the entity's own action passed.
    fn item_interact_mob(&mut self, target: Option<InteractTarget>) -> bool {
        if self.spectator() {
            return false;
        }
        let Some(index) = self.mob_interaction_target(target) else {
            return false;
        };
        let mob = &self.mobs[index];
        if mob.removed || !mob.alive() {
            return false;
        }
        let (entity, kind) = (
            mob.id as i32,
            horizon_client::command::args::normalize(&mob.kind.id),
        );
        let Some(stack) = self.held().filter(|stack| stack.count > 0).cloned() else {
            return false;
        };
        let mut swing = None;
        self.native_item_interact_mob(entity, &kind, Some(&stack), &mut swing);
        if swing == Some(true) {
            self.swing_arm();
        }
        swing.is_some()
    }
    /// `ItemStack.applyAfterUseComponentSideEffects`: a used stack with a
    /// `use_cooldown` component starts its entry (`UseCooldown.apply`). A
    /// server's guest also gets `ClientboundCooldownPacket`
    /// (`ServerItemCooldowns.onCooldownStarted`), queued for their turn's end.
    pub(super) fn apply_use_cooldown(&mut self, stack: &ItemStack) {
        let default = self
            .res
            .items()
            .get(&stack.item)
            .and_then(|i| i.use_cooldown.clone());
        let Some(component) = horizon_client::cooldowns::component(stack, default.as_ref()) else {
            return;
        };
        let group = horizon_client::cooldowns::group(stack, default.as_ref());
        let mut cancel = false;
        if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            self.native_player_item_cooldown(
                player,
                &name,
                &stack.item,
                component.ticks(),
                &mut cancel,
            );
        }
        if !cancel {
            self.add_cooldown(group, component.ticks());
        } else if self.lan_acting.is_some() {
            // The guest's client started the cooldown itself; 0 ends it there.
            self.guest_cooldowns.push((group, 0));
        }
    }
    /// `ItemCooldowns.addCooldown` (a guest also gets the packet).
    pub(super) fn add_cooldown(&mut self, group: Arc<str>, ticks: i32) {
        self.cooldowns.add(Arc::clone(&group), ticks);
        if self.lan_acting.is_some() {
            self.guest_cooldowns.push((group, ticks));
        }
    }
    /// `BundleItem.dropContent`: `removeOneItemFromBundle` and `Player.drop`,
    /// then the drop sound and the used statistic.
    fn drop_bundle_content(&mut self, slot: usize) {
        let items = self.items();
        let Some(mut bundle) = self.stack_in(slot).cloned() else {
            return;
        };
        let Some(mut contents) = horizon_client::bundle::Contents::of(items, &bundle) else {
            return;
        };
        let Some(removed) = contents.remove_one(items) else {
            return;
        };
        contents.store(&mut bundle);
        let item = Arc::clone(&bundle.item);
        *self.stack_in_mut(slot) = Some(bundle);
        let Some(position) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        let at = [position.x, position.y, position.z];
        // playRemoveOneSound, then Player.drop and playDropContentsSound at
        // the player's block.
        let pitch = 0.8 + self.world.random.next_float() * 0.4;
        self.audio
            .play(horizon_client::bundle::REMOVE_ONE, Some(at), 0.8, pitch);
        self.player_drop(removed);
        let block = BlockPos::containing(position);
        let pitch = 0.8 + self.world.random.next_float() * 0.4;
        self.audio.play(
            horizon_client::bundle::DROP_CONTENTS,
            Some([block.x, block.y, block.z].map(|v| f64::from(v) + 0.5)),
            0.8,
            pitch,
        );
        self.award_stat("used", &item, 1);
    }
    /// `ServerPlayerGameMode.tick` while a player digs: level event 2020 every
    /// fourth tick since the START (the hit sound), else 2019 (the crack only),
    /// for everyone around, the digger included (`levelEvent(null, ...)`).
    fn destroy_progress_event(&mut self, pos: [i32; 3], face: [i32; 3], ticks: u32) {
        let id = if ticks.is_multiple_of(4) { 2020 } else { 2019 };
        let direction = horizon_core::geometry::Direction::ALL
            .iter()
            .position(|d| d.normal() == face)
            .unwrap_or(1);
        self.level_event_at(id, pos, direction as i32);
    }
    fn tick_mining(&mut self) -> Result<()> {
        if self.destroy_delay > 0 {
            self.destroy_delay -= 1;
        }
        if !self.left || self.screen != Screen::Playing || self.dialog_open() {
            self.mining = None;
            return Ok(());
        }
        let Some(hit) = self.pick() else {
            self.mining = None;
            return Ok(());
        };
        // Player.blockActionRestricted: spectators, and adventure players
        // without a matching can_break item.
        if self.block_action_restricted(hit.pos)
            || !self
                .res
                .items()
                .can_destroy_block(self.held(), self.creative)
        {
            self.mining = None;
            return Ok(());
        }
        if self.creative {
            if self.destroy_delay == 0 {
                self.break_block(hit.pos)?;
                self.destroy_delay = 5;
            }
            return Ok(());
        }
        if self.destroy_delay > 0 {
            return Ok(());
        }
        if self.mining.as_ref().is_none_or(|m| m.pos != hit.pos) {
            self.attack_block(hit.pos)?;
            self.mining = Some(Mining {
                pos: hit.pos,
                progress: 0.0,
                per_tick: 0.0,
                ticks: 0,
            });
        }
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        if self.res.facts().get(state).hardness < 0.0 {
            // The server still counts its destroy ticks on an unbreakable block.
            let m = self.mining.as_mut().ok_or("mining")?;
            m.per_tick = 0.0;
            m.ticks += 1;
            let (pos, ticks) = (m.pos, m.ticks);
            self.destroy_progress_event(pos, hit.face, ticks);
            return Ok(());
        }
        let progress = self.destroy_progress(hit.pos);
        self.swing_arm();
        let m = self.mining.as_mut().ok_or("mining")?;
        m.per_tick = progress;
        m.progress += progress;
        m.ticks += 1;
        // An instant break at START sends no destroy events.
        if m.progress < 1.0 || m.ticks > 1 {
            let (pos, ticks) = (m.pos, m.ticks);
            self.destroy_progress_event(pos, hit.face, ticks);
        }
        let m = self.mining.as_mut().ok_or("mining")?;
        // ServerPlayerGameMode breaks a player's block at once only when it is instant (START's
        // insta-mine); otherwise it waits for the client's STOP_DESTROY_BLOCK
        // (`guest_stop_digging`), never breaking it on its own count. Breaking it here would
        // take the block from under the player's crack before the player's client predicts the
        // break, so it gets neither particles nor sound.
        if m.progress >= 1.0 && (self.lan_acting.is_none() || m.ticks == 1) {
            let pos = m.pos;
            self.mining = None;
            self.break_block(pos)?;
            self.destroy_delay = 5;
        }
        Ok(())
    }
    /// A column whose surface is a full solid, non-fluid, non-leaf block open to the sky.
    fn land_column(&self, x: i32, z: i32) -> Option<i32> {
        if self.dim == Dim::Nether {
            // Below the bedrock ceiling: a solid floor with two air blocks above.
            for y in (32..120).rev() {
                let floor = self.res.facts().get(self.world.block(x, y - 1, z));
                if floor.full_collision
                    && floor.fluid == Fluid::None
                    && self.res.facts().is_air(self.world.block(x, y, z))
                    && self.res.facts().is_air(self.world.block(x, y + 1, z))
                {
                    return Some(y);
                }
            }
            return None;
        }
        let mut y = self.world.top_y() - 1;
        while y > self.world.bottom_y() {
            let s = self.world.block(x, y, z);
            if !self.res.facts().is_air(s) {
                let f = self.res.facts().get(s);
                let ok = f.full_collision
                    && f.fluid == Fluid::None
                    && !f.name.contains("leaves")
                    && f.hardness >= 0.0;
                return ok.then_some(y + 1);
            }
            y -= 1;
        }
        None
    }
    /// Landing rules (farmland trampling, turtle eggs) wrote or removed the block
    /// raw: finish `Level.setBlock`'s neighbour updates, so a crop on trampled
    /// farmland breaks. The local player's and every server player's landings.
    /// The level events they send go out too: a turtle egg's break particles
    /// and sound (`decreaseEggs`' 2001, `Level.destroyBlock`'s for the last).
    ///
    /// A mod hears the change first (`native_entity_change_block_v2`, Bukkit's
    /// `EntityChangeBlockEvent`): cancelling puts the block back and skips the
    /// updates and the events, so nothing the change would have caused happens -
    /// the crop on trampled farmland stays, because breaking it is the
    /// neighbour update below and not a write of its own.
    ///
    /// The game event the rule posts comes back for the caller to post with the
    /// landing player in place (`post_landing_vibration`).
    fn landing_block_updates(
        &mut self,
        player: Option<u128>,
        event: &horizon_client::landing::FallEvent,
    ) -> Option<LandingVibration> {
        use horizon_client::landing::FallEvent;
        use horizon_world::block_landing::LandingEvent;
        let mut vibration = None;
        // The block the landing changed, what it replaced, and what it became.
        let (position, from, to, flags) = match *event {
            FallEvent::Write {
                position,
                from,
                state,
                flags,
            } => (position, from, state, flags as i32),
            FallEvent::Destroy {
                position, state, ..
            } => {
                // `Level.destroyBlock`: BLOCK_DESTROY with no source, after the
                // write (`TurtleEggBlock.decreaseEggs`' last egg).
                vibration = Some(LandingVibration {
                    event: "minecraft:block_destroy",
                    position,
                    state,
                    from_entity: false,
                });
                (position, state, self.air_state(), level_flags::UPDATE_ALL)
            }
            // `FarmlandBlock.turnToBaseBlock`'s BLOCK_CHANGE from the entity that
            // trampled it, and `TurtleEggBlock.decreaseEggs`' BLOCK_DESTROY
            // (`Context.of(state)`, no source): the write is already applied.
            FallEvent::BlockEvent {
                kind,
                position,
                state,
                with_entity,
            } => {
                // A cancelled change put the block back: nothing happened.
                let current = self.world.block(position.x, position.y, position.z);
                let cancelled = match kind {
                    LandingEvent::BlockChange => current != state,
                    LandingEvent::BlockDestroy => current == state,
                };
                if cancelled {
                    return None;
                }
                return Some(LandingVibration {
                    event: match kind {
                        LandingEvent::BlockChange => "minecraft:block_change",
                        LandingEvent::BlockDestroy => "minecraft:block_destroy",
                    },
                    position,
                    state,
                    from_entity: matches!(kind, LandingEvent::BlockChange) && with_entity,
                });
            }
            FallEvent::LevelEvent {
                event,
                position,
                data,
            } => {
                self.level_event_at(event, [position.x, position.y, position.z], data);
                return None;
            }
            _ => return None,
        };
        if self.landing_change_cancelled(player, position, from, to) {
            self.world
                .set(position.x, position.y, position.z, from)
                .ok();
            return None;
        }
        if matches!(event, FallEvent::Destroy { .. }) {
            self.level_event_at(2001, [position.x, position.y, position.z], from.0 as i32);
        }
        self.with_level(|b, level, ticks| b.notify_written(level, ticks, position, flags));
        vibration
    }
    /// Post a landing's block game event (`FarmlandBlock.turnToBaseBlock`'s
    /// BLOCK_CHANGE, `TurtleEggBlock.decreaseEggs`' BLOCK_DESTROY) as the player
    /// whose landing it was, who must be in place (`as_guest`). A sculk sensor, a
    /// calibrated sensor and a warden hear it.
    fn post_landing_vibration(&mut self, vibration: LandingVibration) {
        let at = [
            vibration.position.x,
            vibration.position.y,
            vibration.position.z,
        ];
        let who = vibration
            .from_entity
            .then_some(horizon_client::mobs::Who::Player);
        self.post_block_game_event(vibration.event, at, who, Some(vibration.state));
    }
    /// `Blocks.AIR.defaultBlockState`: what a landing's destroy leaves.
    fn air_state(&self) -> BlockStateId {
        self.pack
            .assets
            .registry
            .default_state("minecraft:air")
            .unwrap_or(BlockStateId(0))
    }
    /// Whether a mod cancelled a landing's block change
    /// (`native_entity_change_block_v2`): asked with the two blocks' names and
    /// the player whose landing it was (`None` when no player did it).
    fn landing_change_cancelled(
        &mut self,
        player: Option<u128>,
        position: BlockPos,
        from: BlockStateId,
        to: BlockStateId,
    ) -> bool {
        let name = |s: BlockStateId| self.res.facts().get(s).name.to_string();
        let (from, to) = (name(from), name(to));
        let mut cancel = false;
        self.native_entity_change_block_v2(
            player,
            None,
            "minecraft:player",
            [position.x, position.y, position.z],
            &from,
            &to,
            &mut cancel,
        );
        cancel
    }
    /// The ticks the frame shows (animated textures, the HUD, a shader pack's clock): the
    /// client's, or under `HORIZON_STILL` (`FixedStep::still`) the world's game time, which
    /// moves only while the world runs.
    pub(super) fn visual_ticks(&self) -> u64 {
        if self.clock.still() {
            self.world.game_time.max(0) as u64
        } else {
            self.ticks
        }
    }
    /// The clock of the animated textures: `Minecraft.runTick` ticks the `TextureManager`
    /// only while the level runs normally. Under `HORIZON_STILL` it is the world's ticks.
    pub(super) fn texture_clock(&self) -> u64 {
        if self.clock.still() {
            self.visual_ticks()
        } else {
            self.texture_ticks
        }
    }
    /// `Util.getMillis` for what the frame shows (the world border's scroll, the hearts' blink,
    /// the glint's under the switch): under `HORIZON_STILL`, the world's ticks in milliseconds.
    pub(super) fn visual_millis(&self) -> i64 {
        if self.clock.still() {
            self.visual_ticks() as i64 * 50
        } else {
            self.started.elapsed().as_millis() as i64
        }
    }
    /// `PresenceHandler.getPresenceStatus`: what an all-sharing player is doing, for
    /// the friends service's `updatePresence`.
    fn presence_status(&self) -> horizon_client::social::PresenceStatus {
        use horizon_client::social::PresenceStatus;
        if self.integrated.is_some() {
            // `getSingleplayerServer().getMultiplayerScope()`: LAN while the world is
            // opened to LAN (a pause-menu Open to LAN or an invite code).
            if self.lan.is_some() {
                PresenceStatus::PlayingHostedServer
            } else {
                PresenceStatus::PlayingOffline
            }
        } else if self.remote.is_some() {
            // `getCurrentServer().isRealm()`: the connection came from Realms.
            if self.realm_session {
                PresenceStatus::PlayingRealms
            } else {
                PresenceStatus::PlayingServer
            }
        } else {
            PresenceStatus::Online
        }
    }
    fn tick(&mut self) -> Result<()> {
        if self.agent.hold_server {
            return Ok(());
        }
        // `Player.entityInteractionRange`/`blockInteractionRange` for the tick's
        // picks: the attribute carries creative's and a server's modifiers.
        self.entity_reach = self.interaction_range("minecraft:entity_interaction_range", 3.0, 5.0);
        self.block_reach = self.interaction_range("minecraft:block_interaction_range", 4.5, 5.0);
        self.tick_debug_keys()?;
        self.tick_debug_charts();
        self.tick_tutorial();
        if self.screen == Screen::Creative {
            self.refresh_creative_tabs();
            // CreativeModeInventoryScreen.containerTick: a player without
            // infinite materials gets the survival inventory instead.
            if !self.creative {
                if let Some(c) = self.carried.take() {
                    let _ = self.give(c);
                }
                self.screen = Screen::Inventory;
            }
        }
        if self.remote.is_some() {
            return self.tick_remote();
        }
        // Only a server ticks its own world from here: its players are guests, swapped in for
        // their turns (`as_guest`), and the window's player is a client of it.
        self.ticks += 1;
        self.repro_tick();
        self.track_experience_display();
        self.tick_enchanting_book();
        // `/tick freeze` stops the world but not the player.
        let running = match self.level_pass.running {
            Some(running) => running,
            None => self.clock.world_runs(),
        };
        // GameRenderer.tick: a wither's bar darkens the world, then fades;
        // the activated item counts down.
        self.boss_overlay.tick_darken(running);
        self.tick_item_activation();
        self.native_inventory_ticks();
        if running {
            // `MinecraftServer.tickChildren`: `commandFunctions` runs before
            // any level ticks, so a tick function reads the game time and the
            // day clock from before this tick. Vanilla's SavedData is loaded
            // long before this point; the lazy scoreboard/storage read has to
            // be done by then too, or a load tag writes over what it restores.
            self.load_scoreboard();
            self.tick_functions();
            self.world.game_time += 1;
            if self.ui.rules.bool("advance_weather") {
                self.world.weather.tick_timers(&mut self.world.random);
            }
            self.world.weather.tick_levels();
        } else if self.clock.still() {
            // Under `HORIZON_STILL` a frozen world's rain and thunder sit at their levels, as a
            // shader pack's smoothed values do, so `/weather rain` shows full rain without
            // stepping the world (and its mobs) through the fade.
            self.world.weather.settle_levels();
        }
        // ClientLevel.tick: the sky flash fades in a frozen level too.
        self.tick_weather_effects();
        let _ = self.finish_player_autosaves(false);
        // MinecraftServer.tickServer's countdown, then autoSave: three
        // hundred seconds of ticks at the rate the server runs at now.
        self.ticks_until_autosave -= 1;
        self.autosaving = self.ticks_until_autosave <= 0;
        if let Some(serving) = &self.serving {
            serving.set_saving(self.autosaving);
        }
        if self.autosaving {
            self.save_game_with(false)?;
            self.ticks_until_autosave = self.next_autosave_interval();
            if let Some(serving) = &self.serving {
                serving.set_saving(false);
            }
        }
        if running {
            self.advance_clock();
            // `ServerLevel.tickTime`, once the clock has moved on.
            self.run_scheduled_functions();
        }
        if self.use_delay > 0 {
            self.use_delay -= 1;
        }
        self.tick_totem();
        self.update_totem_ready();
        self.tick_command_extras();
        self.native_tick();
        self.tell_native_now();
        self.tick_world(running)
    }
    /// What the tick runs after the local player's own update: effects, the
    /// world's entities and blocks, and the network hub.
    fn tick_world(&mut self, running: bool) -> Result<()> {
        // Entities added since the last tick join the level before it ticks.
        self.native_joins_flush(false, 0);
        self.share_spawn_conditions();
        horizon_client::mobs::sulfur_cube::set_tnt_explodes(self.ui.rules.bool("tnt_explodes"));
        self.update_chunk_tickets();
        self.score_destroyed_entities();
        horizon_client::spans::time("tick_effects", || self.tick_effects(running));
        self.tick_sign_editor();
        // Player.tick's ItemCooldowns.tick for the window's player (a
        // server's players count theirs down in `tick_guests`).
        self.shield_cooldown = self.shield_cooldown.saturating_sub(1);
        self.cooldowns.tick();
        horizon_client::spans::time("tick_using", || self.tick_using());
        self.tick_bundle_hover();
        horizon_client::spans::time("tick_hud", || self.tick_hud());
        // The player's digging comes in with its packets, before the level
        // ticks; ServerLevel.tick then runs the scheduled and random block
        // ticks before any entity, so what they drop or spawn (dispensed
        // items, popped crops) moves on the tick it appears, as in vanilla.
        // On the server thread the window is a hub guest: drain its packets
        // here too, before falling-block schedules run (#4510).
        horizon_client::spans::time("tick_local_hand", || self.tick_local_hand())?;
        horizon_client::spans::time("postprocess_chunks", || self.postprocess_chunks())?;
        let mut sent = None;
        if running {
            horizon_client::spans::time("tick_lan_packets", || self.tick_lan_packets());
            sent = horizon_client::spans::time("tick_blocks", || self.tick_blocks())?;
            // What the blocks spawned (a dispensed minecart, a lit TNT, a
            // placed boat) joins before the entities tick.
            self.native_joins_flush(false, 0);
            self.lan_mark_synced();
            // `EntityTickList.forEach`: what joined before the entity pass
            // (function summons, dropped or dispensed items) ticks in it;
            // what joins during it waits for the next tick
            // (`ensureActiveIsNotIterated`). Primed TNT and falling blocks
            // keep their own packet cutoff (`ordered.announced`, #10063).
            self.ticked_entities_through = self.next_entity;
        }
        // ChunkMap still ticks while /tick freeze holds block/entity ticks.
        if !running {
            self.save_chunks_eagerly();
        }
        self.begin_ordered_ticks();
        if running {
            horizon_client::spans::time("tick_paintings", || self.tick_paintings());
            horizon_client::spans::time("tick_item_frames", || self.tick_item_frames());
            self.tick_explicit_knots();
            self.tick_summoned_eyes();
            self.ordered_before(ordered_tick::Batch::ArmorStands)?;
            horizon_client::spans::time("tick_armor_stands", || self.tick_armor_stands())?;
            horizon_client::spans::time("tick_cushions", || self.tick_cushions());
            // ServerLevel.tick: the arrivals of this server tick are in the level
            // (`addDuringTeleport`), and each vehicle handles its portal as its
            // own tick opens.
            self.land_portal_arrivals();
            self.handle_vehicle_portals();
            self.ordered_before(ordered_tick::Batch::Boats)?;
            horizon_client::spans::time("tick_boats", || self.tick_boats());
            self.ordered_before(ordered_tick::Batch::Carts)?;
            horizon_client::spans::time("tick_carts", || self.tick_carts());
            self.tick_vehicle_watch();
            let from = self.lan_synced_from();
            horizon_client::spans::time("tick_thunder", || self.tick_thunder());
            self.lan_synced_since(from);
            horizon_client::spans::time("tick_lecterns", || self.tick_lecterns());
            horizon_client::spans::time("tick_maps", || self.tick_maps());
            self.tick_lodestone_compasses();
            horizon_client::spans::time("tick_bolts", || self.tick_bolts());
            horizon_client::spans::time("tick_fishing", || self.tick_fishing());
            self.ordered_before(ordered_tick::Batch::Tridents)?;
            horizon_client::spans::time("tick_tridents", || self.tick_tridents());
            self.ordered_before(ordered_tick::Batch::Rockets)?;
            horizon_client::spans::time("tick_rockets", || self.tick_rockets());
            horizon_client::spans::time("tick_ominous_item_spawners", || {
                self.tick_ominous_item_spawners()
            });
        }
        if running {
            horizon_client::spans::time("entity_portals", || {
                self.handle_entity_portals();
                self.handle_end_gateway_portals();
            });
            self.ordered_before(ordered_tick::Batch::Items)?;
            horizon_client::spans::time("tick_items", || self.tick_items());
            if self.remote.is_none() {
                self.hoppers_catch_items();
            }
            // Merged, burnt, despawned or picked up.
            self.native_leaves_flush();
            self.native_joins_flush(false, 0);
            horizon_client::spans::time("touch_arrows", || self.touch_arrows());
        }
        horizon_client::spans::time("spawn_loaded_entities", || self.spawn_loaded_entities());
        // The ride the player's file names, once the vehicle is in the world.
        self.restore_root_vehicle();
        if running {
            self.ordered_before(ordered_tick::Batch::Mobs)?;
            horizon_client::spans::time("tick_mobs", || self.tick_mobs())?;
            // Dead, despawned or gone into a hive.
            self.native_leaves_flush();
            horizon_client::spans::time("recheck_golem_openers", || self.recheck_golem_openers());
            self.tick_mod_container_viewers();
            // Orbs, primed TNT and falling blocks still to tick, in join order.
            self.run_ordered_ticks(horizon_client::join_seq::JoinSeq::MAX, false)?;
            horizon_client::spans::time("tick_experience", || self.tick_experience());
            // Merged, despawned or picked up.
            self.native_leaves_flush();
            horizon_client::spans::time("entity_portals", || {
                self.find_entity_portals();
                self.resolve_arrivals();
            });
        }
        // The rest of the tick's spawns join, and what left is told of.
        self.native_joins_flush(false, 0);
        self.native_leaves_flush();
        // ServerLevel.tick broadcasts the changed blocks after the scheduled and
        // random ticks (`tick_blocks`), before the block events, the entities
        // and the block entities: what those change goes out with the next
        // tick's. A piston's moved block reaches the clients once their own
        // copy of it finished moving, so a slow tick rate shows it slide
        // (#8430).
        if let (Some(sent), Some(log)) = (sent, self.world.block_log.as_mut()) {
            self.world.block_log_held = log.split_off(sent.min(log.len()));
        }
        horizon_client::spans::time("tick_lan", || self.tick_lan(false));
        let held = std::mem::take(&mut self.world.block_log_held);
        if let Some(log) = self.world.block_log.as_mut() {
            log.extend(held);
        }
        self.tick_friends();
        horizon_client::spans::time("tick_hand", || self.tick_hand());
        horizon_client::spans::time("tick_sleep", || self.tick_sleep());
        self.announce_sleep_status();
        self.free_left_beds();
        if running {
            horizon_client::spans::time("tick_end", || self.tick_end())?;
            horizon_client::spans::time("end_gateways", || {
                self.find_end_gateway_portals();
                self.resolve_end_gateway_arrivals()
            })?;
            self.keep_gateway_destinations();
        }
        if running {
            // Primed TNT and falling blocks that joined since the spawn packets.
            self.run_ordered_ticks(horizon_client::join_seq::JoinSeq::MAX, true)?;
            self.apply_late_wind_pushes();
        }
        // Everything that has joined has had its spawn packet and, if it joined
        // before the entity passes (TNT a piston or repeater primed), its first
        // tick in the late pass above (#10063).
        self.ordered.announced = horizon_client::join_seq::JoinSeq::upcoming();
        if running {
            horizon_client::spans::time("tick_moving_pistons", || self.tick_moving_pistons())?;
            horizon_client::spans::time("tick_block_entities", || self.tick_block_entities())?;
            horizon_client::spans::time("tick_sculk_entities", || self.tick_sculk_entities());
            horizon_client::spans::time("tick_sculk_blocks", || self.tick_sculk_blocks())?;
        }
        if running {
            // ClientLevel.tickBlockEntities (not while paused or frozen):
            // chest lids, books, shulker boxes, bells, conduits, heads.
            horizon_client::spans::time("tick_block_entity_anims", || {
                self.tick_block_entity_anims()
            });
            // A conduit's hurt teleports an enderman now (`Enderman.hurtServer`).
            self.settle_hurt_teleports()?;
            horizon_client::spans::time("tick_particles", || self.tick_particles());
            self.tick_client_ambient();
        }
        // ServerLevel.tick's `entityManager.tick()`, frozen or not, after every
        // block entity ticker (the bells, conduits and gateways above too): the
        // chunks loaded before this pass have their entities loaded from the next.
        self.world.entities_pending.clear();
        self.tick_frame_visuals(running);
        if running {
            // MinecraftServer.tickServer: GameTestTicker.SINGLETON.tick().
            self.tick_game_tests();
        }
        // ServerBossEvent broadcasts, after this level's entities have ticked.
        self.publish_boss_bars();
        // Minecraft.handleKeybinds: held use repeats only while no item is in
        // use (a raised shield or a drawn bow places and uses nothing).
        if self.right
            && self.use_delay == 0
            && self.using.is_none()
            && self.screen == Screen::Playing
            && !self.dialog_open()
            && self.sleeping.is_none()
        {
            self.use_with_hand()?;
            self.use_delay = 4;
        }
        if running {
            horizon_client::spans::time("lan_note_joined", || self.lan_note_joined());
        }
        Ok(())
    }
    /// The title remains the no-level sentinel while debug options covers it.
    /// Only UI dispatch ignores it; world loading and simulation still see it.
    fn title_input_active(&self) -> bool {
        self.title.is_some() && self.screen != Screen::DebugOptions
    }
    fn hud(&mut self, size: [u32; 2]) -> HudBatch {
        // Off a world (title, options, Win, a prompt before joining) the HUD shows nothing to
        // keep clear of; `hud_layers` fills it in once the world's HUD draws.
        let gui = HudBatch::new(size);
        self.ui.room = horizon_client::hud_kit::room::Room::new(gui.width, gui.height);
        // The pack prompt replaces the connecting screen (setScreen), but
        // in game it opens over the world.
        let mut batch = if (self.server_packs.prompt.is_some()
            || self.code_of_conduct.is_some()
            || self.dialog_open())
            && self.player.is_none()
        {
            HudBatch::new(size)
        } else {
            self.hud_layers(size)
        };
        // Vanilla's confirmation replaces the dialog as the screen.
        if self.command_prompt.is_none() {
            self.draw_dialog(&mut batch);
        }
        self.draw_pack_prompt(&mut batch);
        self.draw_code_of_conduct(&mut batch);
        self.draw_link_prompt(&mut batch);
        self.draw_command_prompt(&mut batch);
        // The ToastManager draws over every screen, the title and the
        // options included: the narrator's toast follows the player there.
        self.draw_narrator_toast(&mut batch);
        // Gui.extractRenderState: the debug overlay comes after the screen
        // and the toasts, so it is over everything, menus included.
        self.draw_debug_overlay(&mut batch, size);
        self.frame_graph
            .draw(self.settings.horizon.overlay, &mut batch, &self.gui.atlas);
        self.draw_pad_cursor(&mut batch);
        batch
    }
    fn hud_layers(&mut self, size: [u32; 2]) -> HudBatch {
        self.pad.guide_shown.clear();
        let mut batch = HudBatch::new(size);
        batch.glint_colour = self.settings.extras.pvp.tweaks.glint_texel();
        // GlobalSettingsUniform's GameTime, for a resource pack's text shader.
        batch.game_time = ((self.world.game_time.rem_euclid(24000)) as f32
            + self.clock.fraction() as f32)
            / 24000.0;
        let (w, h) = (batch.width, batch.height);
        // HDR calibration ends when its page does, whatever moved the screen
        // off Options (a death screen opened by a server, say).
        if self.screen != Screen::Options && self.ui.options.calibration_fullscreen.is_some() {
            self.show_calibration(h);
        }
        if self.screen == Screen::Win {
            self.draw_win_screen(&mut batch);
            // The early return skips the match below: the hook still sees the
            // screen, after its own drawing.
            self.fire_screen_render_post(&mut batch);
            return batch;
        }
        if self.screen == Screen::Options {
            if self.title.is_none() {
                self.flush_skin_slots();
                self.update_remote_skins();
            }
            self.draw_options(&mut batch);
            self.fire_screen_render_post(&mut batch);
            self.draw_split_hud(&mut batch, true);
            return batch;
        }
        if let Some(title) = &mut self.title {
            title.poll_backup();
            if std::mem::take(&mut title.onboarding_finished) {
                // Options.onboardingAccessibilityFinished.
                self.settings.file.set(
                    "onboardAccessibility",
                    horizon_client::options_file::V::Bool(false),
                );
                self.settings.save(&Self::options_path());
            }
            if let Some(sound) = title.update_toast(self.settings.notification_display_time) {
                self.audio.play(sound, None, 1.0, 1.0);
            }
        }
        if let Some(title) = &self.title {
            for found in title.lan_found_narrations((w, h)) {
                self.say_system_queued(&found);
            }
        }
        if self.screen == Screen::DebugOptions && self.title.is_some() {
            // DebugOptionsScreen replaces the title; no level HUD is behind it.
            self.draw_debug_options(&mut batch);
            self.fire_screen_render_post(&mut batch);
            self.draw_toasts(&mut batch, false);
            self.draw_tutorial_toasts(&mut batch, false);
            self.draw_pack_toasts(&mut batch, false);
            self.draw_music_toast(&mut batch, false);
            return batch;
        }
        if self.title.is_some() {
            // TitleScreen.extractRenderState, then ToastManager over it: vanilla
            // draws the toast manager on every screen, so a pack download or a
            // resource-reload failure still shows on the title (#4428).
            if let Some(title) = &self.title {
                title.draw_with_items(
                    &self.gui.atlas,
                    Some(&self.pack.item_renderer),
                    &mut batch,
                    self.cursor,
                    self.settings.file.bool("hideSplashTexts"),
                    self.settings.file.bool("hideServerAddress"),
                );
            }
            // ScreenEvent.Render.Post over the title's own screens: a mod draws
            // over them here (a pack browser over Create New World, say), and the
            // toasts still come after, as over any screen.
            self.fire_title_screen_render_post(&mut batch);
            self.draw_toasts(&mut batch, false);
            self.draw_tutorial_toasts(&mut batch, false);
            self.draw_pack_toasts(&mut batch, false);
            self.draw_music_toast(&mut batch, false);
            return batch;
        }
        let atlas_ptr: *const HudAtlas = &self.gui.atlas;
        let _ = atlas_ptr;
        // The ToastManager draws over every screen: a server's packs
        // download while the connection screen shows.
        if self.remote_overlay(&mut batch) {
            self.draw_pack_toasts(&mut batch, false);
            self.draw_music_toast(&mut batch, false);
            return batch;
        }
        // Hud.extractRenderState: F1 (keyToggleGui) hides camera overlays, the
        // crosshair, hotbar, effects, bars, sidebar, action bar, titles and chat;
        // the sleep overlay, debug screen and open screens still draw.
        let spectator = self.spectator();
        let (wi, hi) = (w as i32, h as i32);
        let in_studio = self.studio_replay.is_some();
        let show_hud = !self.ui.hide_gui && !in_studio;
        // Hud.extractRenderState asks `Screen.isInGameUi` twice: such a screen
        // defers the subtitles over itself, and keeps them while F1 hides the
        // rest of the HUD.
        let subtitles = subtitles(
            show_hud,
            in_game_ui(self.screen, self.book.is_some() || self.sign_edit.is_some()),
        );
        self.ui.room = self.hud_room(w, h, show_hud);
        // What the gameplay guide keeps off starts after the view's full overlays.
        let mut guide_from = 0;
        if show_hud {
            self.draw_spyglass(&mut batch);
            guide_from = batch.vertices.len();
            // Hud.extractCrosshair / extractItemHotbar / extractPlayerHealth /
            // extractVehicleHealth and the experience bar, laid out by hud_layout.
            // Combat feel Legacy hides the attack indicator unless the player keeps it.
            let combat = self.settings.extras.pvp.combat;
            let indicator = if combat.legacy && !combat.indicator {
                0
            } else {
                self.settings.attack_indicator
            };
            let delay = horizon_client::attack_strength::delay(self.main_hand_attack_speed());
            let strength = self.hand_strength.scale(0.0, self.main_hand_attack_speed());
            // The 3D crosshair (F3) takes the 2D one's place.
            // Hud.extractCrosshair: a spectator only sees it over something to open.
            if self.camera_type.first_person()
                && (!spectator || self.spectator_crosshair())
                && !self.debug_crosshair()
            {
                let attack = (indicator == 1).then_some(strength);
                let full = attack.is_some_and(|a| {
                    hud_layout::full_attack_indicator(a, delay, self.crosshair_living())
                });
                let draws = hud_layout::crosshair(wi, hi, attack, full);
                self.gui_layer("minecraft:crosshair", &mut batch, |g, batch| {
                    // The custom crosshair takes the sprite's place (the first draw); the
                    // attack indicator stays.
                    if g.custom_crosshair() {
                        g.draw_custom_crosshair(batch, wi, hi);
                        hud_layout::paint(batch, &g.gui.atlas, &draws[1..]);
                    } else {
                        hud_layout::paint(batch, &g.gui.atlas, &draws);
                    }
                });
            }
            self.gui_layer("minecraft:hotbar", &mut batch, |g, batch| {
                if spectator {
                    g.draw_spectator_hotbar(batch);
                } else {
                    let partial = g.clock.fraction() as f32;
                    let inputs = hud_layout::HotbarInputs {
                        selected: g.selected as i32,
                        right_handed: !g.settings.main_hand_left,
                        offhand: g.offhand.is_some(),
                        slots: std::array::from_fn(|i| {
                            g.inventory[i]
                                .as_ref()
                                .map(|_| (f32::from(g.hotbar_pop[i]) - partial).max(0.0))
                        }),
                        offhand_pop: (f32::from(g.hotbar_pop[9]) - partial).max(0.0),
                        attack: (indicator == 2).then_some(strength),
                    };
                    let draws = hud_layout::hotbar(wi, hi, &inputs);
                    hud_layout::paint(batch, &g.gui.atlas, &draws);
                    for d in &draws {
                        if let hud_layout::GuiDraw::Item { slot, x, y, .. } = *d {
                            let stack = if slot == 40 {
                                g.offhand.as_ref()
                            } else {
                                g.inventory[slot as usize].as_ref()
                            };
                            if let Some(stack) = stack {
                                g.draw_item(batch, stack, x as f32, y as f32);
                            }
                        }
                    }
                }
            });
            self.gui_layer("minecraft:effects", &mut batch, |g, batch| {
                // The PvP & HUD effects widget takes the icons' place.
                if !g.pvp_effects_shown() {
                    g.draw_effect_icons(batch);
                }
            });
            // Hud: the action bar and titles draw over the hotbar (nextStratum).
            self.gui_layer("minecraft:boss_overlay", &mut batch, |g, batch| {
                g.draw_boss_bars(batch);
            });
            self.draw_extras(&mut batch);
        }
        if show_hud {
            // Hud.nextContextualInfoState: the jump bar, the experience bar or
            // the locator bar above the hotbar.
            let has_experience = !self.creative && !spectator;
            let contextual = horizon_client::locator_bar::ContextualBar::next(
                self.waypoints.has_waypoints(),
                !spectator && self.jumpable_mount(),
                self.mount_jumping(),
                has_experience,
                self.experience_recent(),
            );
            use horizon_client::locator_bar::ContextualBar;
            // The mount's hearts.
            let vehicle_hearts = if spectator {
                0
            } else {
                self.draw_mount_hud(&mut batch, wi, hi)
            };
            let (visual_ticks, visual_millis) = (self.visual_ticks(), self.visual_millis());
            if !self.creative
                && !spectator
                && let Some(p) = &mut self.player
            {
                let effects = &self.potions.effects;
                let kind = if effects.get("poison").is_some() {
                    hud_layout::HeartKind::Poisoned
                } else if effects.get("wither").is_some() {
                    hud_layout::HeartKind::Withered
                } else if p.is_fully_frozen() {
                    hud_layout::HeartKind::Frozen
                } else {
                    hud_layout::HeartKind::Normal
                };
                // Hud.extractAirBubbles: `isEyeInFluidMatching`, so the eyes are below
                // the surface, not merely in the water block.
                let under_water = p.is_eye_in_fluid(horizon_client::motion::WATER);
                let inputs = hud_layout::HealthInputs {
                    tick: visual_ticks as i32,
                    millis: visual_millis,
                    health: p.health.health(),
                    max_health: p.attribute("minecraft:max_health").unwrap_or(20.0) as f32,
                    absorption: p.health.absorption(),
                    armor: p
                        .attribute("minecraft:armor")
                        .or_else(|_| p.attribute("armor"))
                        .unwrap_or(0.0) as i32,
                    food: p.health.food.food,
                    saturation: p.health.food.saturation,
                    kind,
                    regeneration: effects.get("regeneration").is_some(),
                    hunger: effects.get("hunger").is_some(),
                    hardcore: self.level.hardcore,
                    damage_cooldown: p.health.hurt.cooldown > 0,
                    air: p.air,
                    max_air: 300,
                    under_water,
                    vehicle_hearts,
                };
                let at = [p.position.x, p.position.y, p.position.z];
                // The native mods' health HUD (`native_health_hud`): a mod
                // that answers draws every row instead.
                let mut native_health_hud = None;
                self.native_health_hud(&inputs, wi, hi, &mut native_health_hud);
                let draws = match native_health_hud {
                    Some(draws) => draws,
                    None => self.health_hud.layout(&inputs, wi, hi),
                };
                // The rows draw in one pass here, so a mod's `player_health`
                // layer stands for the armor, food and air rows too.
                self.gui_layer("minecraft:player_health", &mut batch, |g, batch| {
                    hud_layout::paint(batch, &g.gui.atlas, &draws);
                });
                // Hud.playAirBubblePoppedSound: Player.playSound (PLAYERS), heard
                // by this client only.
                if let Some((volume, pitch)) = self.health_hud.bubble_pop_sound.take() {
                    let players = horizon_client::settings::SOUND_SOURCES
                        .iter()
                        .position(|s| *s == "player")
                        .unwrap_or(0);
                    let capture = self.audio.capture.take();
                    self.audio
                        .play_on(players, "ui.hud.bubble_pop", Some(at), volume, pitch);
                    self.audio.capture = capture;
                }
            }
            // After the health rows (Hud.extractHotbarAndDecorations): jump, locator or XP bar.
            self.gui_layer("minecraft:contextual_info_bar", &mut batch, |g, batch| {
                if contextual == ContextualBar::JumpableVehicle {
                    g.draw_jump_bar(batch, wi, hi);
                }
                if contextual == ContextualBar::Locator {
                    let (sprite, [x, y, w, h]) = horizon_client::locator_bar::background(wi, hi);
                    horizon_client::ui::nine_slice(
                        batch,
                        &g.gui.atlas,
                        sprite,
                        x as f32,
                        y as f32,
                        w as f32,
                        h as f32,
                        u32::MAX,
                    );
                }
                if has_experience && g.player.is_some() {
                    let atlas = &g.gui.atlas;
                    let width = |s: &str| HudBatch::text_width(atlas, s) as i32;
                    let draws = hud_layout::experience_colored(
                        g.pack.polytone_colors.xp_bar(),
                        wi,
                        hi,
                        g.experience.progress,
                        g.experience.level,
                        contextual == ContextualBar::Experience,
                        &width,
                    );
                    hud_layout::paint(batch, atlas, &draws);
                }
                if contextual == ContextualBar::Locator {
                    let draws = g.locator_bar_draws(wi, hi);
                    hud_layout::paint(batch, &g.gui.atlas, &draws);
                }
            });
            if !spectator {
                self.gui_layer("minecraft:selected_item_name", &mut batch, |g, batch| {
                    g.draw_selected_name(batch, !g.creative);
                });
            }
        }
        // Hud.extractSleepOverlay: over all health/food bars, under titles and chat.
        let draws = hud_overlays::sleep_overlay(wi, hi, self.sleep_timer as i32);
        self.gui_layer("minecraft:sleep_overlay", &mut batch, |g, batch| {
            hud_layout::paint(batch, &g.gui.atlas, &draws);
        });
        if show_hud {
            self.gui_layer("minecraft:overlay_message", &mut batch, |g, batch| {
                g.draw_overlay_message(batch);
            });
            self.gui_layer("minecraft:title", &mut batch, |g, batch| {
                g.draw_title(batch);
            });
            self.gui_layer("minecraft:scoreboard_sidebar", &mut batch, |g, batch| {
                g.draw_scoreboard(batch);
            });
            if subtitles != Subtitles::Hidden {
                self.gui_layer("minecraft:subtitle_overlay", &mut batch, |g, batch| {
                    g.draw_subtitles(batch);
                });
            }
        } else if subtitles == Subtitles::Deferred {
            // F1 hid the HUD, but an in-game screen still shows the subtitles.
            self.gui_layer("minecraft:subtitle_overlay", &mut batch, |g, batch| {
                g.draw_subtitles(batch);
            });
        }
        let atlas = &self.gui.atlas;
        if show_hud
            && let Some((text, at)) = &self.message
            && at.elapsed() < Duration::from_secs(3)
        {
            let tw = HudBatch::text_width(atlas, text);
            batch.text(atlas, text, (w - tw) / 2.0, h - 59.0, u32::MAX, true);
        }
        // The chat screen draws the focused chat itself; F1 hides the rest.
        if self.screen == Screen::Chat {
            self.draw_chat(&mut batch);
        } else if show_hud {
            self.gui_layer("minecraft:chat", &mut batch, |g, batch| g.draw_chat(batch));
            self.gui_layer("minecraft:tab_list", &mut batch, |g, batch| {
                g.draw_tab_list(batch);
            });
        }
        // Screen.extractBlurredBackground blurs the world and HUD, while the
        // screen's background, widgets, toasts and debug overlay stay sharp.
        batch.blur_at = batch.vertices.len() as u32;
        // PackConfirmScreen takes the open screen's place (it comes back once
        // the prompt is answered): the screen under the prompt isn't drawn.
        let screen = if self.server_packs.prompt.is_some() || self.book.is_some() {
            Screen::Playing
        } else {
            self.screen
        };
        match screen {
            Screen::Paused if self.pause_menu => self.draw_pause_menu(&mut batch),
            // PauseScreen(false): only its title, at the top.
            Screen::Paused => {
                let title = self.translate("menu.paused", "Game Paused");
                horizon_client::widgets::begin_screen(|| {
                    horizon_client::widgets::ScreenInfo::new("paused", title.clone())
                });
                let tw = HudBatch::text_width(&self.gui.atlas, &title);
                batch.text(
                    &self.gui.atlas,
                    &title,
                    ((batch.width / 2.0).floor() - (tw / 2.0).floor()).floor(),
                    10.0,
                    u32::MAX,
                    true,
                );
            }
            Screen::GameModeSwitcher => self.draw_gamemode_switcher(&mut batch),
            Screen::EmoteWheel => self.draw_emote_wheel(&mut batch),
            Screen::Dead => self.draw_death_screen(&mut batch),
            Screen::Stats => self.draw_stats(&mut batch),
            Screen::Advancements => self.draw_advancements(&mut batch),
            Screen::Mod => {}
            Screen::WorldOptions => self.draw_world_options(&mut batch),
            Screen::Friends => self.draw_friends_screen(&mut batch),
            Screen::Social => self.draw_social_screen(&mut batch),
            Screen::ServerLinks => self.draw_server_links(&mut batch),
            Screen::ConfirmLink => self.draw_confirm_link(&mut batch),
            Screen::DialogWarning => self.draw_dialog_warning(&mut batch),
            Screen::CommandBlock => self.draw_command_block(&mut batch),
            Screen::BlockEditor => self.draw_block_editor(&mut batch),
            Screen::DebugOptions => self.draw_debug_options(&mut batch),
            Screen::HudEditor => self.draw_hud_editor(&mut batch),
            Screen::Studio => {}
            Screen::Inventory => {
                let from = batch.vertices.len();
                self.draw_inventory(&mut batch);
                self.draw_mod_browser(&mut batch);
                self.draw_pad_menu_guide(&mut batch, from);
            }
            Screen::Creative => {
                let from = batch.vertices.len();
                self.draw_creative(&mut batch);
                self.draw_mod_browser(&mut batch);
                self.draw_pad_menu_guide(&mut batch, from);
            }
            Screen::Chat => self.draw_chat_screen(&mut batch),
            Screen::Playing | Screen::Options | Screen::Win => {}
        }
        // ScreenEvent.Render.Post: the open screen has drawn itself and its
        // tooltips, so a native mod draws over it here. Never `Playing`: the
        // prompts, books and sign editors drawn over gameplay keep it, draw
        // after this point and so over what a mod drew.
        self.fire_screen_render_post(&mut batch);
        self.draw_saving_indicator(&mut batch);
        // ToastManager.extractRenderState: F1 hides the toasts too.
        self.draw_toasts(&mut batch, !show_hud);
        self.draw_tutorial_toasts(&mut batch, !show_hud);
        self.draw_pack_toasts(&mut batch, !show_hud);
        self.draw_music_toast(&mut batch, !show_hud);
        if self.server_packs.prompt.is_none() {
            self.remote_hud(&mut batch);
            self.draw_sign_editor(&mut batch);
            self.draw_book(&mut batch);
        }
        if !self.ui.hide_gui
            && self.screen == Screen::Playing
            && (self.studio_replay.is_some() || self.studio_freecam.active)
        {
            self.studio_topbar.render(
                &mut batch,
                &self.gui.atlas,
                self.cursor,
                self.studio_freecam.locked_to_path,
            );
            self.studio_timeline.render(
                &mut batch,
                &self.gui.atlas,
                self.cursor,
                &self.studio_freecam,
            );
        }
        if show_hud {
            self.draw_pad_guide(&mut batch, guide_from);
        }
        batch
    }
    /// Slot ids: 0..36 player inventory, 100..109 crafting grid, 200 result.
    fn container_kind(&self) -> Option<&BlockEntity> {
        self.container
            .and_then(|k| self.world.block_entities.get(&k))
    }
    /// 300.. are the open container's slots.
    fn slots(&self) -> Vec<usize> {
        if self.creative_inventory_open() {
            return Creative::inventory_slots().collect();
        }
        if self.creative_tab_open() {
            return Creative::tab_slots().collect();
        }
        if let Some(v) = self.station_slots().or_else(|| self.mount_slots()) {
            return v;
        }
        let mut v: Vec<usize> = (0..36).collect();
        if let Some(shown) = self.native_screen_slots() {
            v.extend(shown);
            return v;
        }
        match self.container_kind() {
            Some(BlockEntity::Chest(_)) => v.extend((0..self.container_size()).map(|i| 300 + i)),
            Some(BlockEntity::Furnace(_)) => v.extend(300..303),
            Some(BlockEntity::Brewing(_)) => v.extend(300..305),
            Some(BlockEntity::Crafter(_)) => v.extend(300..310),
            Some(
                BlockEntity::Sign(_)
                | BlockEntity::Jukebox(_)
                | BlockEntity::Beacon(_)
                | BlockEntity::Lectern(_)
                | BlockEntity::Sculk(_)
                | BlockEntity::Banner(_)
                | BlockEntity::CopperGolemStatue(_)
                | BlockEntity::CreakingHeart(_)
                | BlockEntity::Spawner(_)
                | BlockEntity::TrialSpawner(_)
                | BlockEntity::Vault(_)
                | BlockEntity::Beehive(_)
                | BlockEntity::CommandBlock(_)
                | BlockEntity::EndGateway(_)
                | BlockEntity::Skull(_)
                | BlockEntity::DaylightDetector
                | BlockEntity::Bell
                | BlockEntity::Conduit(_)
                | BlockEntity::EndPortal
                | BlockEntity::EnchantingTable
                | BlockEntity::Campfire(_)
                | BlockEntity::ChiseledBookshelf(_)
                | BlockEntity::Brushable(_)
                | BlockEntity::Shelf(_)
                | BlockEntity::Mod(_)
                | BlockEntity::PotentSulfur(_)
                | BlockEntity::StructureBlock(_)
                | BlockEntity::JigsawBlock(_)
                | BlockEntity::TestBlock(_)
                | BlockEntity::TestInstanceBlock(_),
            ) => {}
            None => {
                v.extend((0..self.craft_size * self.craft_size).map(|i| 100 + i));
                v.push(200);
                if self.craft_size == 2 {
                    v.extend(500..505);
                }
            }
        }
        v
    }
    /// How many slots `slots` lists, counted without building the list: the
    /// screen render hook asks once a frame and must not allocate. Same
    /// branches in the same order, so the two stay in step.
    fn slot_count(&self) -> usize {
        if self.creative_inventory_open() {
            return Creative::inventory_slots().count();
        }
        if self.creative_tab_open() {
            return Creative::tab_slots().count();
        }
        if let Some(n) = self
            .station_slot_count()
            .or_else(|| self.mount_slot_count())
        {
            return n;
        }
        let inventory = 36;
        if let Some(shown) = self.native_screen_slot_count() {
            return inventory + shown;
        }
        let container = match self.container_kind() {
            Some(BlockEntity::Chest(_)) => self.container_size(),
            Some(BlockEntity::Furnace(_)) => 3,
            Some(BlockEntity::Brewing(_)) => 5,
            Some(BlockEntity::Crafter(_)) => 10,
            Some(
                BlockEntity::Sign(_)
                | BlockEntity::Jukebox(_)
                | BlockEntity::Beacon(_)
                | BlockEntity::Lectern(_)
                | BlockEntity::Sculk(_)
                | BlockEntity::Banner(_)
                | BlockEntity::CopperGolemStatue(_)
                | BlockEntity::CreakingHeart(_)
                | BlockEntity::Spawner(_)
                | BlockEntity::TrialSpawner(_)
                | BlockEntity::Vault(_)
                | BlockEntity::Beehive(_)
                | BlockEntity::CommandBlock(_)
                | BlockEntity::EndGateway(_)
                | BlockEntity::Skull(_)
                | BlockEntity::DaylightDetector
                | BlockEntity::Bell
                | BlockEntity::Conduit(_)
                | BlockEntity::EndPortal
                | BlockEntity::EnchantingTable
                | BlockEntity::Campfire(_)
                | BlockEntity::ChiseledBookshelf(_)
                | BlockEntity::Brushable(_)
                | BlockEntity::Shelf(_)
                | BlockEntity::Mod(_)
                | BlockEntity::PotentSulfur(_)
                | BlockEntity::StructureBlock(_)
                | BlockEntity::JigsawBlock(_)
                | BlockEntity::TestBlock(_)
                | BlockEntity::TestInstanceBlock(_),
            ) => 0,
            None => {
                self.craft_size * self.craft_size + 1 + if self.craft_size == 2 { 5 } else { 0 }
            }
        };
        inventory + container
    }

    fn draw_inventory(&mut self, batch: &mut HudBatch) {
        screen_info::begin_container("container");
        if self.draw_station(batch) {
            return;
        }
        let (ox, oy) = self.inventory_origin(batch);
        {
            let atlas = &self.gui.atlas;
            self.render_transparent_background("inventory", batch);
            let part = |batch: &mut HudBatch,
                        name: &str,
                        sx: f32,
                        sy: f32,
                        w: f32,
                        h: f32,
                        dx: f32,
                        dy: f32| {
                if let Some(mut r) = atlas.sprites.get(name).copied() {
                    let (du, dv) = ((r.u1 - r.u0) / r.w as f32, (r.v1 - r.v0) / r.h as f32);
                    r.u0 += sx * du;
                    r.v0 += sy * dv;
                    r.u1 = r.u0 + w * du;
                    r.v1 = r.v0 + h * dv;
                    batch.rect_uv(dx, dy, w, h, r, u32::MAX);
                }
            };
            let grey = rgba(64, 64, 64, 255);
            match self
                .container
                .and_then(|k| self.world.block_entities.get(&k))
            {
                Some(_) if self.draw_native_screen(batch, ox, oy) => {}
                Some(BlockEntity::Chest(_)) if self.draw_machine_screen(batch, ox, oy) => {}
                Some(BlockEntity::Chest(_)) => self.draw_chest_screen(batch, ox, oy),
                Some(BlockEntity::Furnace(f)) => {
                    let (tex, key, fallback) = match f.kind {
                        horizon_client::containers::FurnaceKind::Furnace => {
                            ("gui/container/furnace", "container.furnace", "Furnace")
                        }
                        horizon_client::containers::FurnaceKind::BlastFurnace => (
                            "gui/container/blast_furnace",
                            "container.blast_furnace",
                            "Blast Furnace",
                        ),
                        horizon_client::containers::FurnaceKind::Smoker => {
                            ("gui/container/smoker", "container.smoker", "Smoker")
                        }
                    };
                    part(batch, tex, 0.0, 0.0, 176.0, 166.0, ox, oy);
                    // A server's furnace menu has the title it was given.
                    let title = self.remote_menu_title().unwrap_or_else(|| {
                        horizon_client::text::Text::literal(
                            self.container_title(self.translate(key, fallback)),
                        )
                        .resolve_legacy()
                    });
                    screen_info::name_container(|| title.plain());
                    let tw = atlas.font.width_text(&title);
                    batch.text_styled(
                        atlas,
                        &title,
                        ox + (176.0 - tw) / 2.0,
                        oy + 6.0,
                        grey,
                        false,
                    );
                    let inventory = self.translate("container.inventory", "Inventory");
                    batch.text(atlas, &inventory, ox + 8.0, oy + 72.0, grey, false);
                    if f.lit_remaining > 0 {
                        // AbstractFurnaceScreen: flame height ceil(lit * 13) + 1.
                        let total = if f.lit_total == 0 { 200 } else { f.lit_total };
                        let h = ((f.lit_remaining as f32 / total as f32) * 13.0).ceil() + 1.0;
                        part(
                            batch,
                            "gui/sprites/container/furnace/lit_progress",
                            0.0,
                            14.0 - h,
                            14.0,
                            h,
                            ox + 56.0,
                            oy + 36.0 + 14.0 - h,
                        );
                    }
                    if f.cooking_timer > 0 && f.cooking_total > 0 {
                        let w = ((f.cooking_timer as f32 / f.cooking_total as f32) * 24.0).ceil();
                        part(
                            batch,
                            "gui/sprites/container/furnace/burn_progress",
                            0.0,
                            0.0,
                            w,
                            16.0,
                            ox + 79.0,
                            oy + 34.0,
                        );
                    }
                }
                Some(BlockEntity::Brewing(_)) => {}
                Some(BlockEntity::Crafter(_)) => self.draw_crafter_screen(batch, ox, oy),
                Some(
                    BlockEntity::Sign(_)
                    | BlockEntity::Jukebox(_)
                    | BlockEntity::Beacon(_)
                    | BlockEntity::Lectern(_)
                    | BlockEntity::Sculk(_)
                    | BlockEntity::Banner(_)
                    | BlockEntity::CopperGolemStatue(_)
                    | BlockEntity::CreakingHeart(_)
                    | BlockEntity::Spawner(_)
                    | BlockEntity::TrialSpawner(_)
                    | BlockEntity::Vault(_)
                    | BlockEntity::Beehive(_)
                    | BlockEntity::CommandBlock(_)
                    | BlockEntity::EndGateway(_)
                    | BlockEntity::Skull(_)
                    | BlockEntity::DaylightDetector
                    | BlockEntity::Bell
                    | BlockEntity::Conduit(_)
                    | BlockEntity::EndPortal
                    | BlockEntity::EnchantingTable
                    | BlockEntity::Campfire(_)
                    | BlockEntity::ChiseledBookshelf(_)
                    | BlockEntity::Brushable(_)
                    | BlockEntity::Shelf(_)
                    | BlockEntity::Mod(_)
                    | BlockEntity::PotentSulfur(_)
                    | BlockEntity::StructureBlock(_)
                    | BlockEntity::JigsawBlock(_)
                    | BlockEntity::TestBlock(_)
                    | BlockEntity::TestInstanceBlock(_),
                ) => {}
                None if self.mount_active() => self.draw_mount_background(batch, ox, oy),
                None => {
                    let texture = if self.craft_size == 3 {
                        "gui/container/crafting_table"
                    } else {
                        "gui/container/inventory"
                    };
                    part(batch, texture, 0.0, 0.0, 176.0, 166.0, ox, oy);
                    let title = self.translate("container.crafting", "Crafting");
                    screen_info::name_container(|| title.clone());
                    let tx = if self.craft_size == 3 { 29.0 } else { 97.0 };
                    batch.text(atlas, &title, ox + tx, oy + 6.0, grey, false);
                    if self.craft_size == 3 {
                        let inventory = self.translate("container.inventory", "Inventory");
                        batch.text(atlas, &inventory, ox + 8.0, oy + 72.0, grey, false);
                    }
                }
            }
        }
        if self.container.is_none() && self.craft_size != 3 && !self.mount_active() {
            self.draw_inventory_player(batch, ox, oy);
        }
        self.draw_hovered_slot_highlight(batch, false, ox, oy);
        self.draw_inventory_slots(batch, ox, oy);
        self.draw_hovered_slot_highlight(batch, true, ox, oy);
        self.draw_sort_buttons(batch, ox, oy);
        self.draw_recipe_book(batch);
        let effect_tip = if self.container.is_none() && self.craft_size == 2 && !self.mount_active()
        {
            self.draw_inventory_effects(batch, (ox, oy), 176)
        } else {
            None
        };
        self.draw_carried(batch);
        self.draw_slot_tooltip(batch);
        self.draw_native_screen_tooltip(batch);
        if let Some(lines) = effect_tip {
            self.draw_tooltip_lines(batch, &lines, self.cursor.0, self.cursor.1);
        }
    }
    /// `AbstractContainerScreen` keeps `isQuickCrafting` and the drag it
    /// belongs to on the screen. Horizon keeps both on `Game` (`inv_clicks`,
    /// `inv_drag`, `last_inv_click`), so they are dropped when the pointer
    /// changes hands: a screen that opens or closes starts with its own
    /// state, and a press whose release never reached the screen -- the
    /// pointer left the window, or the screen closed first -- would
    /// otherwise leave every later press on every later screen swallowed.
    pub(super) fn reset_inventory_clicks(&mut self) {
        self.inv_clicks.cancel_drag();
        self.inv_drag = None;
        self.last_inv_click = None;
    }
    pub(super) fn close_container(&mut self) {
        // `Gui.setScreen` clears the old screen's focus before it goes: the
        // key that closed the container (Tab bound to the inventory) has
        // already moved the focus, and it must not outlive the screen.
        self.pad_clear_focus();
        self.mod_browser_closed();
        self.reset_inventory_clicks();
        self.close_station();
        self.mount_closed();
        self.recipe_ui.reset_screen();
        if self.remote.is_some() {
            return self.remote_close_container();
        }
        let mut returned: Vec<ItemStack> = self.carried.take().into_iter().collect();
        returned.extend(self.craft_grid.iter_mut().filter_map(Option::take));
        for stack in returned {
            if let Some(rest) = self.give(stack)
                && let Some(eye) = self.eye_position()
            {
                // AbstractContainerMenu.dropOrPlaceInInventory: `drop(stack, false)`.
                self.throw_from_eyes(rest, eye, false);
            }
        }
        self.craft_size = 2;
        self.craft_table = None;
        self.open_container_slots = None;
        if let Some(key) = self.container.take() {
            // A cart's or chest boat's items go back to it.
            self.close_cart_container(key);
            let last = self.container_opener_event(key, false);
            if last {
                self.post_block_game_event(
                    "minecraft:container_close",
                    [key.0, key.1, key.2],
                    Some(horizon_client::mobs::Who::Player),
                    None,
                );
                self.container_edge_sound(key, false);
            }
            self.world.modified.insert((key.0 >> 4, key.2 >> 4));
        }
        self.close_ender_chest();
    }
    /// A sound a menu click made the player play (a bundle's insert or
    /// remove): `Entity.playSound` at the player.
    pub(super) fn player_menu_sound(&mut self, sound: &str) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let at = [p.position.x, p.position.y, p.position.z];
        let (volume, pitch) = if sound == horizon_client::bundle::INSERT_FAIL {
            (1.0, 1.0)
        } else {
            (0.8, 0.8 + self.world.random.next_float() * 0.4)
        };
        self.audio.play(sound, Some(at), volume, pitch);
    }
    /// `Camera.setRotation`: the camera's orientation, turned round in
    /// third-person front view.
    pub(super) fn camera_orientation(&self) -> glam::Quat {
        if self.studio_freecam.active {
            return camera_rotation(
                perspective::CameraType::FirstPerson,
                self.studio_freecam.yaw,
                self.studio_freecam.pitch,
            ) * glam::Quat::from_rotation_z(self.studio_freecam.roll.to_radians());
        }
        match self.posed_rotation {
            // The frame's camera, already turned round in front view.
            Some((yaw, pitch)) => camera_rotation(perspective::CameraType::FirstPerson, yaw, pitch),
            None => camera_rotation(self.camera_type, self.yaw, self.pitch),
        }
    }
    /// `mark_motion` ids of entities that aren't mobs or dropped items (boats, minecarts, armor
    /// stands, item frames, paintings, primed TNT, falling blocks, rockets, tridents, thrown
    /// potions): their entity id under this tag.
    pub(super) const MOTION_OBJECT: u64 = 3 << 32;
    /// An item frame's held item, apart from the frame (one id may mark one run).
    pub(super) const MOTION_FRAME_ITEM: u64 = 4 << 32;
    /// A block entity, by its position (`motion_block_entity`).
    pub(super) const MOTION_BLOCK_ENTITY: u64 = 5 << 32;
    /// A block a piston moves, by the cell it moves to (`motion_at`).
    pub(super) const MOTION_MOVING_BLOCK: u64 = 6 << 32;
    /// The player's own model (third person, and a shader pack's shadow pass).
    pub(super) const MOTION_OWN_PLAYER: u64 = 7 << 32;
    /// The entity whose geometry starts at vertex `start` (0: none), for frame
    /// generation to follow its own motion. Each run lasts until the next mark.
    pub(super) fn mark_motion(&self, start: usize, id: u64) {
        self.entity_motion.borrow_mut().push((start as u32, id));
        // Where its GPU-posed vertices start (`entity_skin`): a shader pack's `at_velocity`
        // follows those too.
        let posed = self.skin_frame.borrow().vertices;
        self.entity_motion_posed.borrow_mut().push(posed);
    }
    /// ItemEntityRenderer: items bob and spin; blocks render as small cubes.
    fn entity_geometry(
        &mut self,
        camera: glam::DVec3,
        alpha: f64,
        fov_degrees: f32,
        billboard_frustum: Option<horizon_client::gfx::Frustum>,
        mut out: Vec<Vertex>,
    ) -> (
        Vec<Vertex>,
        Arc<[StaticSectionSnapshot]>,
        Vec<[i32; 3]>,
        StaticCacheFrameStats,
    ) {
        // Its steps as spans (HORIZON_SPIKES): each lap names the step that just ended.
        let mut laps = horizon_client::spans::laps();
        let mut static_block_entities = std::sync::Arc::from([]);
        let mut visible_static_block_entity_sections = Vec::new();
        let mut static_block_entity_stats = StaticCacheFrameStats::default();
        self.entity_spans.borrow_mut().clear();
        self.entity_motion.borrow_mut().clear();
        self.entity_motion_posed.borrow_mut().clear();
        self.maps.slots.next_frame();
        // LevelExtractor.extract: the transient blocks queued since the last frame.
        self.drain_transient_block_queue();
        self.prepare_entity_culling();
        self.prepare_occlusion_culling(camera);
        self.prepare_trims();
        self.begin_skinning();
        self.mark_entity(out.len(), "minecraft:entity_flame");
        // One scan for both passes that draw them (block entities below and the
        // overlay near the end of this function).
        let cracks = self.cracks(camera);
        let fire_yaw = if self.camera_type == perspective::CameraType::ThirdPersonFront {
            self.yaw + 180.0
        } else {
            self.yaw
        };
        // Packs may move vertices or draw from another camera. The guarded
        // billboard frustum covers TAA's jitter without changing model culling.
        let fire_view = billboard_frustum
            .as_ref()
            .filter(|_| self.pack_ids.is_none() && self.native_shaders.picked.is_empty());
        let fire_visible = |position, width, height| {
            fire_view.is_none_or(|view| {
                horizon_client::fire_render::in_view(view, camera, position, width, height)
            })
        };
        laps.lap("eg_prep");
        for m in &self.mobs {
            // Blaze.isOnFire: a charged blaze burns.
            let blazing = m.species() == horizon_client::mobs::Species::Blaze && m.extra.flag;
            if (m.is_on_fire() || blazing) && !m.removed {
                let at = m.previous.lerp(m.position, alpha);
                if !fire_visible([at.x, at.y, at.z], m.width(), m.height()) {
                    continue;
                }
                horizon_client::fire_render::flames(
                    [at.x, at.y, at.z],
                    m.width(),
                    m.height(),
                    camera,
                    fire_yaw,
                    &self.gui.atlas.sprites,
                    self.world.light(
                        at.x.floor() as i32,
                        at.y.floor() as i32,
                        at.z.floor() as i32,
                    ),
                    &mut out,
                );
            }
        }
        if self.own_body_in_view()
            && let Some(p) = &self.player
            && p.fire_ticks > 0
            && !p.spectator
        {
            let feet = self
                .previous_position
                .lerp(p.position, self.clock.camera_fraction());
            if fire_visible(
                [feet.x, feet.y, feet.z],
                p.dimensions.width,
                p.dimensions.height,
            ) {
                horizon_client::fire_render::flames(
                    [feet.x, feet.y, feet.z],
                    p.dimensions.width,
                    p.dimensions.height,
                    camera,
                    fire_yaw,
                    &self.gui.atlas.sprites,
                    self.world.light(
                        feet.x.floor() as i32,
                        feet.y.floor() as i32,
                        feet.z.floor() as i32,
                    ),
                    &mut out,
                );
            }
        }
        laps.lap("eg_fire_mobs");
        for item in &self.dropped {
            if item.fire_ticks > 0
                && !self
                    .res
                    .items()
                    .resists_fire(&item.stack, "minecraft:in_fire")
            {
                let at = item.previous.lerp(item.position, alpha);
                if !fire_visible([at.x, at.y, at.z], 0.25, 0.25) {
                    continue;
                }
                horizon_client::fire_render::flames(
                    [at.x, at.y, at.z],
                    0.25,
                    0.25,
                    camera,
                    fire_yaw,
                    &self.gui.atlas.sprites,
                    self.world.light(
                        at.x.floor() as i32,
                        at.y.floor() as i32,
                        at.z.floor() as i32,
                    ),
                    &mut out,
                );
            }
        }
        laps.lap("eg_fire_items");
        for stand in &self.armor_stands {
            if stand.fire_ticks > 0 && !stand.marker {
                let scale = if stand.small { 0.5 } else { 1.0 };
                let at = stand.render_position(alpha);
                if !fire_visible([at.x, at.y, at.z], 0.5 * scale, 1.975 * scale) {
                    continue;
                }
                horizon_client::fire_render::flames(
                    [at.x, at.y, at.z],
                    0.5 * scale,
                    1.975 * scale,
                    camera,
                    fire_yaw,
                    &self.gui.atlas.sprites,
                    self.world.light(
                        at.x.floor() as i32,
                        at.y.floor() as i32,
                        at.z.floor() as i32,
                    ),
                    &mut out,
                );
            }
        }
        laps.lap("eg_fire_stands");
        // `ItemEntityRenderer`.
        self.mark_entity(out.len(), "minecraft:item");
        self.dropped_item_geometry(camera, alpha, &mut out);
        self.mark_motion(out.len(), 0);
        // `VaultRenderer`.
        self.mark_entity(out.len(), "");
        self.vault_geometry(camera, alpha, &mut out);
        // `CampfireRenderer`.
        self.mark_entity(out.len(), "");
        self.campfire_geometry(camera, &mut out);
        self.brushable_geometry(camera, &mut out);
        self.shelf_geometry(camera, &mut out);
        self.mark_entity(out.len(), "minecraft:ominous_item_spawner");
        self.ominous_item_spawner_geometry(camera, alpha, &mut out);
        laps.lap("eg_items_misc");
        // Mobs and their projectiles (a span each).
        self.mob_geometry(camera, alpha, &mut out);
        self.mark_motion(out.len(), 0);
        laps.lap("eg_mobs");
        // In first person only a shader pack's shadow pass draws the player.
        self.mark_player(out.len(), !self.own_body_in_view());
        let player_start = out.len();
        self.mark_motion(player_start, Self::MOTION_OWN_PLAYER);
        let player_alpha = if self.studio_replay.is_some() {
            (self.studio_virtual_tick_ms as f64 / 50.0).clamp(0.0, 1.0)
        } else {
            self.clock.camera_fraction()
        };
        self.player_model_geometry(camera, player_alpha, &mut out);
        // A native pack's shadow pass alone gets the first-person model (no per-entity draws).
        if self.pack_ids.is_none() && !self.own_body_in_view() {
            self.shadow_only_vertices
                .borrow_mut()
                .extend(out.drain(player_start..));
        }
        self.mark_motion(out.len(), 0);
        // Companions in first person, trails and auras (game/cosmetics/fx.rs).
        self.cosmetics_fx_geometry(camera, alpha, &mut out);
        self.mark_entity(out.len(), "minecraft:evoker_fangs");
        self.fangs_geometry(camera, alpha, &mut out);
        self.block_item_display_geometry(camera, alpha, &mut out);
        laps.lap("eg_players");
        let mut see_through = std::mem::take(&mut self.see_through_vertices);
        see_through.clear();
        self.name_tag_geometry(camera, alpha, &mut out, &mut see_through);
        self.test_instance_marker_geometry(camera, &mut out, &mut see_through);
        self.debug_renderer_geometry(camera, &mut out);
        self.text_display_geometry(camera, alpha, &mut out, &mut see_through);
        self.see_through_vertices = see_through;
        laps.lap("eg_name_tags");
        // Entity shadows (EntityRenderer.extractShadow); a native pack's sun shadows replace them.
        self.mark_translucent_entity(out.len(), "minecraft:entity_shadow");
        let pack_shadows = self.renderer.as_ref().is_some_and(|r| {
            r.pack_entity_shadows() || (self.dim == Dim::Overworld && r.native_shadow())
        });
        let shadows = self.settings.entity_shadows && self.pack.polytone_colors.entity_shadows();
        let shadow_sprite = (shadows && !pack_shadows)
            .then(|| self.gui.atlas.sprites.get("misc/shadow").copied())
            .flatten();
        if let Some(sprite) = shadow_sprite {
            // DimensionType.ambientLight: the_nether 0.1, the_end 0.25.
            let ambient = match self.dim {
                Dim::Nether => 0.1,
                Dim::End => 0.25,
                Dim::Overworld => 0.0,
            };
            let darken = self.sky_darken();
            let a = alpha;
            let lerp = |p: Vec3, q: Vec3| {
                glam::DVec3::new(
                    p.x + (q.x - p.x) * a,
                    p.y + (q.y - p.y) * a,
                    p.z + (q.z - p.z) * a,
                )
            };
            let mut casters: Vec<(glam::DVec3, f32, f32)> =
                horizon_client::shadows::mob_casters(&self.mobs, a).collect();
            let item = horizon_client::shadows::shadow_params("minecraft:item");
            casters.extend(
                self.dropped
                    .iter()
                    .map(|i| (lerp(i.previous, i.position), item.0, item.1)),
            );
            let orb = horizon_client::shadows::shadow_params("minecraft:experience_orb");
            casters.extend(
                self.orbs
                    .iter()
                    .map(|o| (lerp(o.previous, o.position), orb.0, orb.1)),
            );
            // Text, block and item displays (DisplayRenderer's shadow radius).
            casters.extend(self.text_displays.values().filter_map(|d| d.shadow(a)));
            // Packs may move vertices or draw from another view.
            let shadow_view = self
                .view_frustum
                .as_ref()
                .filter(|_| self.pack_ids.is_none() && self.native_shaders.picked.is_empty());
            for (pos, radius, strength) in casters {
                if let Some(view) = shadow_view
                    && !horizon_client::shadows::in_view(view, camera, pos, radius, strength)
                {
                    continue;
                }
                horizon_client::shadows::shadow(
                    &self.world,
                    self.res.facts(),
                    &|state| {
                        self.res
                            .collision()
                            .cached_shapes()
                            .cached_shape(state)
                            .ok()
                            .flatten()
                            .and_then(|shape| shape.bounds())
                    },
                    sprite,
                    camera,
                    pos,
                    radius,
                    strength,
                    ambient,
                    darken,
                    &mut out,
                );
            }
        }
        laps.lap("eg_shadows");
        // Block entities (BlockEntityRenderers): chests, shulker boxes, banners, skulls...
        if let Some(r) = &self.renderer {
            // Block entities outside the view frustum are skipped (with a
            // block of margin for models larger than their block).
            let view = self.view_frustum;
            // Beams and structure outlines reach far outside their block
            // (BlockEntityRenderer.shouldRenderOffScreen): never culled.
            let off_screen = |state: u32| {
                let name = &*self.res.facts().get(BlockStateId(state)).name;
                matches!(
                    name,
                    "minecraft:beacon"
                        | "minecraft:end_gateway"
                        | "minecraft:structure_block"
                        | "minecraft:test_instance_block"
                )
            };
            let long_range = |state: u32| {
                &*self.res.facts().get(BlockStateId(state)).name == "minecraft:end_gateway"
            };
            let in_view = |p: &[i32; 3]| {
                view.is_none_or(|v| {
                    let o = (glam::DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
                        - camera)
                        .as_vec3();
                    v.intersects(o - glam::Vec3::ONE, o + glam::Vec3::splat(2.0))
                })
            };
            let camera_list: Vec<&([i32; 3], u32)> = r
                .visible_block_entities_in_frustum(
                    self.settings.chunk_fade,
                    camera,
                    view,
                    &off_screen,
                    long_range,
                )
                .filter(|(block, section_inside)| {
                    let (pos, state) = **block;
                    self.res.block_entity_render().renders(state)
                        && (off_screen(state) || *section_inside || in_view(&pos))
                })
                .map(|(block, _)| block)
                .collect();
            // A shader pack's shadow pass draws the block entities of every section it draws,
            // in the view or not (`Renderer::shadow_pass_block_entities`, #8464): those the
            // view leaves out go in hidden from the camera.
            let shadow_only: Vec<&([i32; 3], u32)> =
                if self.pack_ids.is_some() && self.pack_shadow_casters.0 {
                    let listed: HashSet<[i32; 3]> = camera_list.iter().map(|b| b.0).collect();
                    r.shadow_pass_block_entities(camera)
                        .filter(|(pos, state)| {
                            !listed.contains(pos) && self.res.block_entity_render().renders(*state)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
            let shadow_only_at: HashSet<[i32; 3]> = shadow_only.iter().map(|b| b.0).collect();
            let dynamic_visible = |pos: [i32; 3], state: u32, lo: glam::DVec3, hi: glam::DVec3| {
                if off_screen(state) || shadow_only_at.contains(&pos) {
                    return true;
                }
                if let Some(view) = view
                    && !view.intersects(
                        (lo - camera - glam::DVec3::splat(0.5)).as_vec3(),
                        (hi - camera + glam::DVec3::splat(0.5)).as_vec3(),
                    )
                {
                    return false;
                }
                let name = &*self.res.facts().get(BlockStateId(state)).name;
                if self.settings.horizon.entity_culling == EntityCullingMode::Off
                    || self.pack_shadow_casters.0
                    || name == "minecraft:end_portal"
                    || name == "minecraft:end_gateway"
                    || hi.y - lo.y > 2.0
                {
                    return true;
                }
                // Culler::aabb_bounds adds 0.05 on each side. Keep its final
                // query just inside the conservative rendered-model envelope.
                let bounds = Aabb::new(
                    Vec3::new(lo.x + 0.06, lo.y + 0.06, lo.z + 0.06),
                    Vec3::new(hi.x - 0.06, hi.y - 0.06, hi.z - 0.06),
                );
                !self.block_entity_occluded(bounds)
            };
            let lid = |p: [i32; 3]| self.chest_lid(p, alpha as f32);
            let anim = |p: [i32; 3]| self.block_anim(p, alpha as f32);
            let scene = BlockEntityScene {
                camera,
                orientation: self.camera_orientation(),
                anim: &anim,
                texts: None,
                partial: alpha as f32,
                game_time: self.world.game_time,
                max_y: self.world.shape.max_y() - 1,
                world: &self.world,
                sprites: &self.gui.atlas.sprites,
                emissive: &self.gui.atlas.emissive,
                translucent_sprites: &self.gui.atlas.translucent,
                atlas_revision: self.gui.atlas.revision,
                nether: self.dim == Dim::Nether,
                lid: &lid,
                sign: &|p| match self.world.block_entities.get(&(p[0], p[1], p[2])) {
                    Some(BlockEntity::Sign(sign)) => Some(sign),
                    _ => None,
                },
                banner: &|p| self.banner_layers_at(p),
                skull: &|p| self.skull_skin_at(p),
                glyph: &|c| self.gui.atlas.glyph_cell(c),
                glyphs: self.gui.atlas.font.epoch(),
                font: Some(&self.gui.atlas.font),
                dynamic_visible: Some(&dynamic_visible),
            };
            let visible = camera_list.into_iter().chain(shadow_only.iter().copied());
            let block_entity_cache = CacheOptions {
                shulker_boxes: self.settings.horizon.cache_shulker_boxes,
                signs: self.settings.horizon.cache_signs,
                decorated_pots: self.settings.horizon.cache_decorated_pots,
                bells: self.settings.horizon.cache_bells,
                chests: self.settings.horizon.cache_chests,
                banners: self.settings.horizon.cache_banners,
                copper_golem_statues: self.settings.horizon.cache_copper_golem_statues,
            };
            // Entity occlusion is for ordinary entity meshes. Block entities
            // already have section and per-block frustum culling; routing them
            // through the per-vertex occlusion callback both repeats that work
            // and disables their per-section GPU cache. Shader packs still need
            // per-entity span marks, so they keep the callback path.
            if self.pack_ids.is_some() {
                let section_revision = |pos| r.block_entity_section_revision(pos);
                static_block_entity_stats = self
                    .res
                    .block_entity_render()
                    .render_with_cache_options_and_callbacks(
                        &scene,
                        visible,
                        block_entity_cache,
                        r.block_entity_revision(),
                        Some(&section_revision),
                        &mut out,
                        |pos, state, start| {
                            self.mark_block_entity(start, state);
                            self.mark_motion(start, motion_block_entity(pos));
                        },
                        |pos, _, start, out| {
                            if shadow_only_at.contains(&pos) {
                                self.hide_spans_from(start, out.len());
                            }
                        },
                    );
            } else {
                let cache_candidates = r
                    .visible_block_entities_in_frustum(
                        self.settings.chunk_fade,
                        camera,
                        None,
                        |_| false,
                        |_| false,
                    )
                    .filter(|(block, _)| self.res.block_entity_render().renders(block.1))
                    .map(|(block, _)| block);
                (
                    static_block_entities,
                    visible_static_block_entity_sections,
                    static_block_entity_stats,
                ) = self
                    .res
                    .block_entity_render()
                    .render_with_gpu_cache_options(
                        &scene,
                        visible,
                        cache_candidates,
                        block_entity_cache,
                        r.block_entity_revision(),
                        |pos| r.block_entity_section_revision(pos),
                        &mut out,
                    );
            }
            // Mod block entities: their mods draw them (`native_block_entity_submit`),
            // culled by the bounds they give. A world without any (`has_mods`) has none
            // to find among its other unrendered block entities (furnaces, hoppers...).
            let mods = self
                .world
                .block_entities
                .has_mods()
                .then(|| {
                    r.visible_block_entities(self.settings.chunk_fade)
                        .filter(|(_, state)| !self.res.block_entity_render().renders(*state))
                })
                .into_iter()
                .flatten();
            let untemplated = self.untemplated_mod_block_entities(camera);
            self.mod_block_entity_geometry(
                mods.chain(untemplated.iter()),
                camera,
                alpha as f32,
                &mut out,
            );
            self.mark_stage(out.len(), "DESTROY");
            for &(pos, stage) in &cracks {
                let state = self.world.block(pos[0], pos[1], pos[2]);
                self.res
                    .block_entity_render()
                    .render_breaking(&scene, pos, state.0, stage, &mut out);
            }
        }
        laps.lap("eg_block_entities");
        // Experience orbs (ExperienceOrbRenderer): pulsing billboards from a 4x4 icon sheet.
        self.mark_entity(out.len(), "minecraft:experience_orb");
        // Shader packs can move vertices or use another camera. TAA's own
        // sub-pixel projection shifts are covered by billboard_frustum's guard band.
        let orb_view = billboard_frustum
            .as_ref()
            .filter(|_| self.pack_ids.is_none() && self.native_shaders.picked.is_empty());
        if let Some(r) = self
            .gui
            .atlas
            .sprites
            .get("entity/experience/experience_orb")
            .copied()
        {
            let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
            let forward = glam::Vec3::new(
                -yaw.sin() * pitch.cos(),
                -pitch.sin(),
                yaw.cos() * pitch.cos(),
            );
            let right = forward.cross(glam::Vec3::Y).normalize_or_zero();
            let up = right.cross(forward).normalize_or_zero();
            let live = self.orbs.iter().map(|o| {
                let at = [
                    o.previous.x + (o.position.x - o.previous.x) * alpha,
                    o.previous.y + (o.position.y - o.previous.y) * alpha,
                    o.previous.z + (o.position.z - o.previous.z) * alpha,
                ];
                (o, at, alpha as f32)
            });
            // ItemPickupParticle: the render state extracted at partial tick 1.
            let taken = self.pickups.live.iter().filter_map(|p| match &p.taken {
                pickups::Taken::Orb(o) => Some((&**o, pickups::position(p, alpha), 1.0)),
                _ => None,
            });
            for (o, at, partial) in live.chain(taken) {
                let p = glam::DVec3::new(at[0], at[1] + 0.1, at[2]) - camera;
                let c = p.as_vec3();
                // A cheap eligibility test avoids six plane tests for visible
                // sprites. The actual camera frustum still decides rejection,
                // including when a native camera hook changes the view.
                if orb_view.is_some_and(|view| {
                    forward.dot(c) < 0.0 && !entity_culling::orb_in_view(view, c)
                }) {
                    continue;
                }
                let icon = o.icon();
                let (du, dv) = ((r.u1 - r.u0) / 4.0, (r.v1 - r.v0) / 4.0);
                let (u0, v0) = (r.u0 + (icon % 4) as f32 * du, r.v0 + (icon / 4) as f32 * dv);
                let t = (o.age as f32 + partial) / 2.0;
                let red = ((t.sin() + 1.0) * 0.5 * 255.0) as u32;
                let blue = (((t + 4.188_79).sin() + 1.0) * 0.1 * 255.0) as u32;
                let color = 0x8000_0000 | blue << 16 | 255 << 8 | red;
                // `ExperienceOrbRenderer.getBlockLightLevel` (the block light
                // plus 7, clamped) at the block the orb's eye is in.
                let light = self.world.light(
                    o.position.x.floor() as i32,
                    o.light_probe_y().floor() as i32,
                    o.position.z.floor() as i32,
                );
                let packed = o.render_light(i32::from(light));
                // `ExperienceOrbRenderer.submit`: the quad runs from
                // (-0.5, -0.25) to (0.5, 0.75) and is scaled by 0.3, so the
                // sprite hangs a quarter of its height below the orb's centre.
                let s = horizon_client::entities::ORB_QUAD_SCALE;
                for (k, uv) in horizon_client::entities::ORB_QUAD.iter().zip([
                    [u0, v0 + dv],
                    [u0 + du, v0 + dv],
                    [u0 + du, v0],
                    [u0, v0],
                ]) {
                    let v = c + right * (k[0] * s) + up * (k[1] * s);
                    out.push(Vertex {
                        normal: [0; 4],
                        pos: v.to_array(),
                        uv,
                        // ExperienceOrbRenderer: `RenderTypes.entityTranslucent`; in HDR
                        // output the orbs shine (entity.wgsl fs_hdr_mark).
                        layer: horizon_client::gfx::OIT_ENTITY
                            | u32::from(horizon_client::mesher::VERY_BRIGHT) << 16,
                        color,
                        light: packed,

                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
        laps.lap("eg_orbs");
        // Falling blocks use their baked model, including non-cubic shapes.
        self.mark_entity(out.len(), "minecraft:falling_block");
        // Packs can move vertices or draw another view; otherwise only depth
        // planes are used, which TAA's X/Y projection jitter cannot change.
        let falling_view = self
            .view_frustum
            .as_ref()
            .filter(|_| self.pack_ids.is_none() && self.native_shaders.picked.is_empty());
        let tables = &self.pack.context.tables;
        let falling_forward = self.camera_orientation() * -glam::Vec3::Z;
        for f in &self.falling {
            let p = f.model_origin(alpha, |state, x, z| tables.offset(state, x, z)) - camera;
            let o = p.as_vec3();
            // Only rear-origin models pay for the envelope/depth check. The
            // full posed-camera test still decides rejection, including models
            // whose origin is behind but whose geometry reaches into view.
            if falling_forward.dot(o) < 0.0
                && falling_view.is_some_and(|view| {
                    self.pack
                        .render
                        .model_catalog
                        .render_bounds(f.state)
                        .is_some_and(|[lo, hi]| {
                            let lo = glam::Vec3::from_array(lo) + o;
                            let hi = glam::Vec3::from_array(hi) + o;
                            let magnitude = o
                                .abs()
                                .max_element()
                                .max(lo.abs().max_element())
                                .max(hi.abs().max_element());
                            let padding =
                                glam::Vec3::splat(1.0e-4 + magnitude * f32::EPSILON * 4.0);
                            !view.intersects_depth(lo - padding, hi + padding)
                        })
                })
            {
                continue;
            }
            // `FallingBlockEntity.blockPosition()`: the cell its feet are in.
            let (x, y, z) = (
                f.position.x.floor() as i32,
                f.position.y.floor() as i32,
                f.position.z.floor() as i32,
            );
            if !falling_block_visible(f.state, self.world.block(x, y, z)) {
                continue;
            }
            let Some((_, tint)) = self.block_face_sprite(f.state) else {
                continue;
            };
            let light = self.world.light(x, y + 1, z);
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            let start = out.len();
            self.mark_motion(start, Self::MOTION_OBJECT | u64::from(f.id));
            self.block_model_vertices(
                f.state,
                horizon_core::random::position_seed(f.start_pos),
                glam::Mat4::from_translation(o),
                packed,
                false,
                &mut out,
            );
            let tint = horizon_client::mobs::render::abgr(tint);
            for vertex in &mut out[start..] {
                vertex.color = horizon_client::hud_layout::multiply(vertex.color, tint);
            }
        }
        self.mark_motion(out.len(), 0);
        self.mark_entity(out.len(), "");
        self.moving_piston_geometry(camera, alpha, &mut out);
        // LevelRenderer.submitTransientBlocks.
        self.transient_block_geometry(camera, &mut out);
        self.mark_entity(out.len(), "minecraft:tnt");
        self.tnt_geometry(camera, alpha, &mut out);
        self.mark_motion(out.len(), 0);
        self.mark_entity(out.len(), "minecraft:painting");
        self.painting_geometry(camera, &mut out);
        self.mark_motion(out.len(), 0);
        self.mark_entity(out.len(), "minecraft:item_frame");
        self.frame_geometry(camera, &mut out);
        self.mark_motion(out.len(), 0);
        self.mark_entity(out.len(), "minecraft:armor_stand");
        self.armor_stand_geometry(camera, alpha, &mut out);
        self.mark_motion(out.len(), 0);
        self.cushion_geometry(camera, &mut out);
        self.mark_entity(out.len(), "minecraft:oak_boat");
        self.boat_geometry(camera, alpha, &mut out);
        self.mark_motion(out.len(), 0);
        self.mark_entity(out.len(), "minecraft:minecart");
        self.cart_geometry(camera, alpha, &mut out);
        self.mark_motion(out.len(), 0);
        // Beams: `gbuffers_beaconbeam`, not a block entity's program (5.4 has no stage for them).
        self.mark_stage(out.len(), "NONE");
        self.beacon_geometry(camera, alpha, &mut out);
        self.mark_entity(out.len(), "minecraft:fishing_bobber");
        self.fishing_geometry(camera, alpha, &mut out);
        self.mark_entity(out.len(), "minecraft:leash_knot");
        self.leash_geometry(camera, alpha, &mut out);
        self.mark_entity(out.len(), "minecraft:trident");
        self.trident_geometry(camera, alpha, &mut out);
        self.mark_entity(out.len(), "minecraft:firework_rocket");
        self.rocket_geometry(camera, alpha, &mut out);
        self.mark_entity(out.len(), "minecraft:splash_potion");
        self.thrown_potion_geometry(camera, alpha, &mut out);
        laps.lap("eg_objects");
        // Particles (ParticleEngine.extract): camera-facing quads.
        self.mark_stage(out.len(), "PARTICLES");
        let world = &self.world;
        let particle_mode = self.settings.horizon.particle_culling;
        if particle_mode == horizon_client::graphics::ParticleCullingMode::Off {
            self.particles.render(
                camera,
                self.yaw,
                self.pitch,
                alpha as f32,
                &|b| world.light(b[0], b[1], b[2]),
                &mut out,
            );
        } else {
            let frustum = self.view_frustum;
            let pixel_focal_length = self.renderer.as_ref().map_or(0.0, |renderer| {
                let tangent = (f64::from(fov_degrees).to_radians() * 0.5).tan();
                if tangent.is_finite() && tangent > 0.0 {
                    (f64::from(renderer.config.height.max(1)) / (2.0 * tangent)) as f32
                } else {
                    0.0
                }
            });
            {
                let mut particle_culling = self.particle_occlusion_context(camera);
                self.particles.render_culled(
                    camera,
                    self.yaw,
                    self.pitch,
                    alpha as f32,
                    pixel_focal_length,
                    true,
                    frustum.as_ref(),
                    &mut |min, max| particle_culling.hidden(min, max),
                    &|b| world.light(b[0], b[1], b[2]),
                    &mut out,
                );
            }
            self.finish_async_particle_culling();
        }
        laps.lap("eg_particles");
        // Breaking progress overlays (destroy_stage_0..9): ours and the other players'.
        self.mark_stage(out.len(), "DESTROY");
        for &(pos, stage) in &cracks {
            let o = (glam::DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]))
                - camera)
                .as_vec3();
            let state = self.world.block(pos[0], pos[1], pos[2]);
            if self.res.facts().get(state).render_shape
                == horizon_client::blocks::RenderShape::Model
                && let Ok(models) = self
                    .pack
                    .render
                    .model_catalog
                    .collect(state, BlockPos::new(pos[0], pos[1], pos[2]))
            {
                let offset = self
                    .pack
                    .context
                    .tables
                    .offset(state, pos[0], pos[2])
                    .unwrap_or([0.0; 3]);
                let origin = o + glam::DVec3::from(offset).as_vec3();
                let translucent = models
                    .iter()
                    .any(|(_, model)| model.faces.iter().any(|f| f.translucent));
                for (_, model) in models {
                    horizon_client::breaking::geometry(model, origin, translucent, stage, &mut out);
                }
            }
        }
        laps.lap("eg_cracks");
        self.finish_async_occlusion_culling();
        laps.lap("eg_culling_finish");
        self.native_world_geometry(camera, alpha as f32, &mut out);
        (
            out,
            static_block_entities,
            visible_static_block_entity_sections,
            static_block_entity_stats,
        )
    }

    /// `LevelRenderer.destroyBlockProgress`: a player's crack on a block (0..=9), or none
    /// (any other progress). Cracks not updated for 400 ticks are dropped.
    pub(crate) fn destroy_block_progress(&mut self, breaker: i32, pos: [i32; 3], progress: i32) {
        let now = self.world.game_time;
        self.destroying
            .retain(|_, c| now.saturating_sub(c.2) <= 400);
        if (0..10).contains(&progress) {
            self.destroying.insert(breaker, (pos, progress as u8, now));
        } else {
            self.destroying.remove(&breaker);
        }
    }

    /// `ServerLevel.destroyBlockProgress`: breaker `breaker`'s crack on `pos`
    /// (0..=9, anything else takes it away), for every player within 64 blocks.
    pub(super) fn server_destroy_block_progress(
        &mut self,
        breaker: i32,
        pos: [i32; 3],
        progress: i32,
    ) {
        self.destroy_block_progress(breaker, pos, progress);
        if let Some(host) = self.lan.as_ref() {
            host.hub.broadcast_near(
                [f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])],
                64.0,
                &horizon_protocol::packets::play::Clientbound::BlockDestruction(
                    horizon_protocol::packets::play::BlockDestruction {
                        id: breaker,
                        pos,
                        progress: if (0..10).contains(&progress) {
                            progress as u8
                        } else {
                            255
                        },
                    },
                ),
                None,
            );
        }
    }

    fn sky_darken(&self) -> i32 {
        if self.dim == Dim::Nether {
            return 11;
        }
        if self.dim == Dim::End {
            let sky = self
                .res
                .end_environment()
                .sample(&horizon_client::environment::Sample {
                    day_time: self.world.day_time,
                    ..Default::default()
                });
            // Level.updateSkyBrightness truncates (`(int)`), see `environment::sky_darken`.
            return (15.0 - 15.0 * sky.sky_light_level) as i32;
        }
        let (rain, thunder) = self.weather_levels(1.0);
        // Sampled once per (time, weather): every mob asks each tick.
        let key = (self.world.day_time, rain.to_bits(), thunder.to_bits());
        if let Some((k, darken)) = self.sky_darken_memo.get()
            && k == key
        {
            return darken;
        }
        let darken = horizon_client::environment::sky_darken(
            self.res.environment(),
            self.world.day_time,
            rain,
            thunder,
        );
        self.sky_darken_memo.set(Some((key, darken)));
        darken
    }
    /// The mob hit along the view, the distance to it and the point the ray
    /// met its box at, measured from the mob's feet (`EntityHitResult
    /// .getLocation() - entity.position()`, the interact packet's location).
    fn mob_hit(&self) -> Option<(usize, f64, [f64; 3])> {
        if let Some(hit) = self.tick_mob {
            return hit;
        }
        self.mob_hit_within(self.entity_reach)
    }
    /// [`Game::mob_hit`] out to `reach` instead of the entity interaction range
    /// (`ProjectileUtil.getHitResultOnViewVector` with another range).
    pub(super) fn mob_hit_within(&self, reach: f64) -> Option<(usize, f64, [f64; 3])> {
        let (eye, dir) = self.aim()?;
        let mut best: Option<(usize, f64, [f64; 3])> = None;
        for (i, m) in self.mobs.iter().enumerate() {
            // ProjectileUtil.getEntityHitResult: the mob the player rides
            // shares its root vehicle and is never the hit, nor is the
            // camera entity the pick starts from. EntitySelector.CAN_BE_PICKED
            // is isPickable — `!isRemoved`, not alive: a health-0 fake player
            // still answers a click.
            if m.removed || Some(m.id) == self.riding || Some(m.id) == self.remote_camera_id() {
                continue;
            }
            if let Some(t) = ray_box(eye, dir, reach, m.bounds())
                && best.is_none_or(|b| t < b.1)
            {
                let point = Vec3::new(
                    eye[0] + dir[0] * t,
                    eye[1] + dir[1] * t,
                    eye[2] + dir[2] * t,
                );
                best = Some((i, t, interact_location(point, m.position)));
            }
        }
        best
    }
    /// `Player.attack`'s damage against `target_kind` (with the attack
    /// strength and whether it is a critical hit): the weapon and effects
    /// scaled by the attack strength, a mace's smash, the critical hit, then
    /// enchantments.
    pub(super) fn melee_damage(
        &mut self,
        target_kind: &str,
        living: bool,
    ) -> (f32, f32, bool, bool) {
        if self.legacy_rules() {
            return self.legacy_melee_damage(target_kind, living);
        }
        let hand_damage = self.attack_damage();
        // Riptide's spin replaces the attack damage attribute.
        let spin = self.player.as_ref().map_or(0.0, |p| p.auto_spin_attack_dmg);
        let damage = if spin > 0.0 { spin } else { hand_damage };
        let scale = self.attack_strength_scale();
        // getEnchantedDamage(target, damage, source) - damage, taken before the
        // attack strength scales the base damage.
        let enchant_bonus = self.enchanted_damage(target_kind, damage) - damage;
        let mut amount = damage * (0.2 + scale * scale * 0.8);
        // Player.attack decides the critical hit only for a hit with base or
        // enchantment damage: checked before the mace's smash bonus is added.
        let damaging = amount > 0.0 || enchant_bonus * scale > 0.0;
        // Item.getAttackDamageBonus (a mace's smash), before the critical hit.
        amount += self.smash_damage_bonus(target_kind);
        let vanilla_crit = living && self.critical_attack(scale);
        let mut crit = vanilla_crit;
        let mut multiplier = if vanilla_crit { 1.5 } else { 1.0 };
        if living && damaging {
            self.native_critical_hit(target_kind, vanilla_crit, &mut crit, &mut multiplier);
        }
        if crit {
            amount *= multiplier;
        }
        // Enchantment damage (sharpness, smite, bane) scaled by the attack strength.
        amount += enchant_bonus * scale;
        (amount, scale, crit, enchant_bonus * scale > 0.0)
    }
    /// `Player.attack`'s sounds when a vehicle takes the hit (it always does,
    /// and can't be hit critically): knockback when sprinting at full
    /// strength, then strong or weak, or the sweep around the vehicle's
    /// box `area`. Before the hit resets the charge (`Player.onAttack`).
    pub(super) fn vehicle_attack_sounds(&mut self, area: Aabb, magic: Option<i32>) {
        self.attack_effects(area, None, false, mace::Spared::Nothing, magic);
    }
    /// ServerExplosion: rays from a 16^3 grid lose (resistance + 0.3) * 0.3 per step.
    /// A mob explosion (`ServerLevel.explode` with `ExplosionInteraction.MOB`:
    /// blocks are kept without mobGriefing, else the mob_explosion_drop_decay
    /// rule picks decay).
    fn explode(
        &mut self,
        center: [f64; 3],
        radius: f32,
        source: explosion::ExplosionSource,
    ) -> Result<()> {
        self.explode_mob(center, radius, false, source)
    }
    /// `ServerLevel.explode` with `ExplosionInteraction.MOB` and the caller's fire flag.
    fn explode_mob(
        &mut self,
        center: [f64; 3],
        radius: f32,
        fire: bool,
        source: explosion::ExplosionSource,
    ) -> Result<()> {
        let interaction = if !self.ui.rules.bool("mob_griefing") {
            explosion::BlockInteraction::Keep
        } else {
            self.destroy_type("mob_explosion_drop_decay")
        };
        self.server_explode_from(center, radius, interaction, fire, source)
    }
    /// `SulfurCube.tickFuse`: `ExplosionInteraction.TNT` (the
    /// tnt_explosion_drop_decay rule) with mobGriefing, `NONE` without.
    fn explode_sulfur_cube(
        &mut self,
        center: [f64; 3],
        radius: f32,
        source: explosion::ExplosionSource,
    ) -> Result<()> {
        let interaction = if !self.ui.rules.bool("mob_griefing") {
            explosion::BlockInteraction::Keep
        } else {
            self.destroy_type("tnt_explosion_drop_decay")
        };
        self.server_explode_from(center, radius, interaction, false, source)
    }
    /// Copies of a save batch carrying its live block entities and save data.
    fn with_entities<'a>(
        &self,
        chunks: impl IntoIterator<Item = &'a Arc<horizon_client::chunk::Chunk>>,
    ) -> Vec<Arc<horizon_client::chunk::Chunk>> {
        horizon_client::save::chunks_with_block_entities(
            chunks,
            &self.world.block_entities,
            &self.world.block_names,
        )
        .into_iter()
        .map(|mut c| {
            self.attach_save_data(&mut c);
            Arc::new(c)
        })
        .collect()
    }
    fn tick_mobs(&mut self) -> Result<()> {
        self.sync_mob_death_protection();
        self.tick_creatures()?;
        if !self.entity_attachments.is_empty() {
            self.prune_entity_attachments();
        }
        self.native_mobs_equipment();
        self.position_rider();
        self.tick_stats();
        self.tick_scoreboard();
        self.load_bossbars();
        self.tick_worldborder();
        self.tick_subtitles();
        self.spawn_phantoms();
        self.tick_advancements();
        self.tick_player_model();
        self.tick_emotes();
        Ok(())
    }

    fn finish_open_world(&mut self, prepared: world_loading::PreparedWorld) -> Result<()> {
        self.reset_tutorial();
        // Another world: its generator, not the last one's Density line.
        self.debug_density.borrow_mut().clear();
        let world_loading::PreparedWorld {
            save_root,
            shapes,
            level,
            world,
            streaming,
            save,
            session_lock,
            rand_sequences,
            locator,
            owner_migration,
        } = prepared;
        let dim = Dim::from_id(&level.dimension);
        self.level_shapes = shapes;
        self.world_root = save_root.clone();
        self.owner_migration = owner_migration;
        // The world's packs over the game's biomes (`Biomes::with_packs`),
        // the table its chunks were built with.
        self.world_biomes = self.packed_biomes();
        self.world_biomes_base = self.world_biomes.clone();
        horizon_client::crash::set_detail(
            "World",
            format!("{} ({})", level.name, save_root.display()),
        );
        self.end.set_locator(locator);
        self.share_spawn_conditions();
        self.score = Default::default();
        self.fns = Default::default();
        (self.world_tables, self.health_resources) =
            self.build_world_tables(false, &level.enabled_features)?;
        self.damage_tag_generation = self.damage_tag_generation.wrapping_add(1);
        // Native tag reads use the lock to tell that the tag tables are ready.
        self.session_lock = Some(session_lock);
        // Keep the no-pack preload; a world's packs replace it atomically.
        // A later /reload keeps this session's synchronized registries.
        lan::open_world_sync_data(
            &self.options.resources,
            &pack_files(&self.world_tables),
            &self.health_resources.damage,
        )
        .map_err(|e| format!("world registries: {e}"))?;
        // Warm the loot data with this world's packs before chunks decode
        // (`save::roll_loaded_loot` reads the cached copy).
        let _ = horizon_client::structure_blocks::world_loot_data(
            &self.options.resources,
            &pack_files(&self.world_tables),
        );
        // The mods' own data folders, then TagsUpdatedEvent (SERVER_DATA_LOAD):
        // the world's data is in, before its chunks and players are.
        self.native_reload_listeners();
        self.native_server_data_loaded(false);
        self.sync_recipe_content();
        self.bosses = Default::default();
        reset_world_progress(&mut self.adv, &mut self.stats);
        self.prefetch_advancement_tree();
        self.restore_player_state = true;
        self.rand_sequences = rand_sequences;
        // ServerLevel.getWorldBorder: the level in place reads its own saved
        // border when it is made, before any command (the parked levels read
        // theirs in `open_level`).
        self.border = self.load_border(dim);
        self.set_mode_id(level.mode());
        self.previous_mode = level.previous_game_mode.map(game_mode::mode_from_id);
        let mut inventory = vec![None; 36];
        self.armor = [None, None, None, None];
        self.offhand = None;
        self.ender_items = if level.ender_items.len() == 27 {
            level.ender_items.clone()
        } else {
            vec![None; 27]
        };
        for (slot, id, count, damage) in &level.inventory {
            let stack = ItemStack {
                item: Arc::from(id.as_str()),
                count: *count,
                damage: *damage,
                extra: level.inventory_components.get(slot).cloned(),
            };
            if (100..104).contains(slot) {
                self.armor[usize::from(103 - slot)] = Some(stack);
            } else if *slot == 150 {
                self.offhand = Some(stack);
            } else if let Some(cell) = inventory.get_mut(usize::from(*slot)) {
                *cell = Some(stack);
            }
        }
        if self.creative && level.inventory.is_empty() && level.position.is_none() {
            for (i, name) in [
                "minecraft:grass_block",
                "minecraft:stone",
                "minecraft:oak_planks",
                "minecraft:glass",
                "minecraft:torch",
                "minecraft:oak_log",
                "minecraft:chest",
                "minecraft:furnace",
                "minecraft:diamond_pickaxe",
            ]
            .iter()
            .enumerate()
            {
                inventory[i] = Some(ItemStack {
                    item: Arc::from(*name),
                    count: 1,
                    damage: 0,
                    extra: None,
                });
            }
        }
        let (yaw, pitch) = self
            .options
            .look
            .unwrap_or((level.rotation[0], level.rotation[1]));
        self.yaw = yaw;
        self.pitch = pitch;
        self.selected = level.selected_slot.clamp(0, 8) as usize;
        self.streaming = streaming;
        self.world = world;
        self.inventory = inventory;
        self.save = save;
        // MinecraftServer.prepareLevels: the saved tickets come back.
        self.tickets = if self.options.execution_role.is_client() {
            Default::default()
        } else {
            horizon_client::ticket_levels::ChunkTickets::with_saved_portal(
                self.save.load_portal_tickets(),
            )
        };
        // The ride the player's file names, once the vehicle is in the world.
        self.pending_vehicle = level.root_vehicle;
        self.level = level;
        // ServerRecipeBook.loadUntrusted: recipes the manager does not have are dropped.
        let index = &world_stations!(self).recipe_index;
        let book = &mut self.level.recipe_book;
        book.recipes.retain(|r| index.contains(r));
        book.to_be_displayed.retain(|r| index.contains(r));
        self.dim = dim;
        self.audio.set_world(dim.id());
        // LevelStorageAccess.getIconFile: this save's icon.png is what the
        // Select World list shows.
        self.world_icon = Some(WorldIcon {
            path: save_root.join("icon.png"),
            last_attempt: None,
        });
        self.world_flat = horizon_client::save::WorldGen::read(&save_root)
            .is_some_and(|w| w.preset == horizon_client::save::WorldPreset::Flat);
        self.world_debug = horizon_client::save::WorldGen::read(&save_root)
            .is_some_and(|w| w.preset == horizon_client::save::WorldPreset::Debug);
        self.block_ticks = Ticks::new(self.level.seed as i32);
        self.tick_chunks.clear();
        self.clear_entities();
        self.player = None;
        self.sleeping = None;
        self.sleep_timer = 0;
        self.player_beds.clear();
        self.spawn = None;
        self.respawn_config = self
            .level
            .respawn_elsewhere
            .as_ref()
            .map(respawn_anchor::RespawnConfig::from_saved);
        self.spawn_origin = None;
        self.spawn_spiral = Default::default();
        self.player_spawn = None;
        self.creatures.reset_trader();
        self.creatures.cat_tick = 0;
        self.portals = Default::default();
        self.experience = Experience {
            level: self.level.xp_level,
            progress: self.level.xp_progress,
            total: self.level.xp_total,
        };
        self.restore_effects();
        self.mining = None;
        self.container = None;
        let kinds = std::mem::take(&mut self.be_anims.kinds);
        self.be_anims = horizon_client::block_entity_anim::BlockEntityAnims {
            kinds,
            ..Default::default()
        };
        self.carried = None;
        self.using = None;
        self.screen = Screen::Playing;
        self.title = None;
        self.clear_lod();
        self.pending_uploads.clear();
        if let Some(r) = self.renderer.as_mut() {
            r.clear_sections(self.world.layout());
        }
        self.apply_settings();
        self.streaming.sky_light = dim != Dim::Nether;
        self.streaming.nether_shading = dim == Dim::Nether;
        self.load_world_state();
        // The level the world opens in, before any of its chunks.
        self.native_level_opened(dim);
        self.open_saved_portal_levels()
    }

    fn finish_close_world(&mut self) -> Result<()> {
        self.ambience.clear(&mut self.audio);
        // updateLevelInEngines(null): every sound stops; the music manager
        // lets go of its track.
        self.music.stop_playing(None, &mut self.audio);
        self.audio.stop_all();
        if self.remote.is_some() {
            self.world_icon = None;
            return self.leave_server(None);
        }
        // The save screen has finished every write before releasing storage.
        self.session_lock = None;
        self.world_icon = None;
        horizon_client::crash::clear_detail("World");
        self.debug_charts.reset();
        self.boss_overlay.reset();
        // GameRenderer.resetData (Minecraft.clearClientLevel).
        self.item_activation.reset();
        self.kept_block_entities.get_mut().clear();
        self.bosses = Default::default();
        self.streaming.paused = true;
        self.world = World::new(self.res.facts().air, 0);
        self.player = None;
        // `LocalPlayer`'s constructor: `KeyMapping.resetToggleKeys`, so a sneak or sprint toggled
        // in this world is not still on in the next one.
        self.reset_toggle_keys();
        self.sleeping = None;
        self.sleep_timer = 0;
        self.player_beds.clear();
        self.clear_entities();
        self.screen = Screen::Playing;
        self.clear_lod();
        self.pending_uploads.clear();
        if let Some(r) = self.renderer.as_mut() {
            r.clear_sections(self.world.layout());
        }
        // Clear sync data (registries, dialog tags) so dialogs don't leak
        // between worlds.
        lan::forget_sync_data();
        self.title = Some(
            Title::new(
                std::env::current_dir()?.join("saves"),
                &self.options.resources,
            )
            .with_user(|| self.options.player_name()),
        );
        if let Some(title) = &mut self.title {
            title.last_server = self.settings.file.string("lastServer");
            title.skip_multiplayer_warning = self.settings.file.bool("skipMultiplayerWarning");
        }
        self.capture(false);
        Ok(())
    }
    /// Let go of every entity of the level (saved first): the next world
    /// opened must not inherit them, nor save them as its own.
    pub(super) fn clear_entities(&mut self) {
        self.mobs.clear();
        self.dropped.clear();
        self.orbs.clear();
        self.creatures.projectiles.clear();
        self.falling.clear();
        self.transient_blocks.clear();
        self.tnts.clear();
        self.boats.clear();
        self.carts.clear();
        self.armor_stands.clear();
        self.cushions.clear();
        self.item_frames.clear();
        self.paintings.clear();
        self.others.clear();
        self.dragon = None;
        self.crystals.clear();
        self.breath_clouds.clear();
        self.tridents.clear();
        self.rockets.clear();
        self.bolts.clear();
        self.particles.clear();
        self.pickups.clear();
        self.riding = None;
        self.persist = Default::default();
        self.entity_attachments.clear();
        self.clear_remote_objects();
        self.clear_remote_projectiles();
        self.levels.clear();
        self.entity_vertices.clear();
        self.see_through_vertices.clear();
        self.glowing_vertices.get_mut().clear();
        self.shadow_only_vertices.get_mut().clear();
        self.entity_spans.borrow_mut().clear();
        self.entity_motion.borrow_mut().clear();
        self.entity_motion_posed.borrow_mut().clear();
        self.skin_models
            .borrow_mut()
            .restart(&mut self.skin_frame.borrow_mut());
        self.skin_frame.borrow_mut().clear_draws();
    }
    /// Equipment attribute modifiers (LivingEntity.collectEquipmentChanges).
    fn apply_equipment(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        for (attribute, id) in std::mem::take(&mut self.applied_modifiers) {
            let _ = player.set_modifier_unclamped(&attribute, &id, 0.0, "add_value", false);
        }
        let slots = ["head", "chest", "legs", "feet"];
        let mut equipped: Vec<(Option<&ItemStack>, &str)> = self
            .armor
            .iter()
            .zip(slots)
            .map(|(s, n)| (s.as_ref(), n))
            .collect();
        equipped.push((self.inventory[self.selected].as_ref(), "mainhand"));
        equipped.push((self.offhand.as_ref(), "offhand"));
        for (stack, slot) in equipped {
            let Some(info) = stack.and_then(|s| world_items!(self).get(&s.item)) else {
                continue;
            };
            for (attribute, id, amount, op, group) in &info.modifiers {
                // The attack code still reads the held item's attack damage and
                // speed directly; the map copy is what `/attribute` reports.
                if !equipment_modifier_applies(group, slot) {
                    continue;
                }
                if player
                    .set_modifier_unclamped(attribute, id, *amount, op, true)
                    .is_ok()
                {
                    self.applied_modifiers.push((attribute.clone(), id.clone()));
                }
            }
        }
        let _ = player.clamp_health_to_max();
        self.apply_enchantment_attributes();
    }
    /// Voxel traversal that also stops at fluid source blocks (bucket pick).
    fn pick_fluid(&self) -> Option<Hit> {
        let (origin, dir) = self.aim()?;
        let mut t = 0.0;
        let mut last = origin.map(|v| v.floor() as i32);
        let mut first = true;
        while t <= self.reach() {
            let p = [
                origin[0] + dir[0] * t,
                origin[1] + dir[1] * t,
                origin[2] + dir[2] * t,
            ];
            let b = p.map(|v| v.floor() as i32);
            // `BlockGetter.traverseBlocks` visits the block the ray starts in
            // first: the eyes' own water, under water (#3787).
            if first || b != last {
                let state = self.world.block(b[0], b[1], b[2]);
                let f = self.res.facts().get(state);
                let face = [last[0] - b[0], last[1] - b[1], last[2] - b[2]];
                if f.fluid != Fluid::None
                    && f.fluid_level == 8
                    && !f.fluid_falling
                    && !f.waterlogged
                    && f.render_shape != horizon_client::blocks::RenderShape::Model
                {
                    if !first {
                        return Some(Hit {
                            pos: b,
                            face,
                            point: p,
                        });
                    }
                    let above = self.world.block(b[0], b[1] + 1, b[2]);
                    let same_above = self.res.facts().get(above).fluid == f.fluid;
                    if let Some((face, point)) =
                        clip_start_source(origin, dir, self.reach(), b, same_above)
                    {
                        return Some(Hit {
                            pos: b,
                            face,
                            point,
                        });
                    }
                } else if !first && !self.res.facts().is_air(state) && f.fluid == Fluid::None {
                    // `ClipContext.Block.OUTLINE`: the ray stops at the block's
                    // outline, not its cell; it passes through the empty part
                    // of a torch, a fence or a flower to the water behind.
                    if let Some(hit) =
                        outline_stop(origin, dir, self.reach(), b, face, || self.outline_boxes(b))
                    {
                        return Some(hit);
                    }
                }
                last = b;
                first = false;
            }
            t += 0.05;
        }
        None
    }
    /// `getPlayerPOVHitResult(level, player, SOURCE_ONLY)` for an empty
    /// bucket: the block outline or a source fluid, whichever is nearer. A
    /// waterlogged block is a block hit (its outline is the block's, its
    /// fluid a source); flowing fluid has no shape to pick.
    pub(super) fn bucket_hit(&self) -> Option<Hit> {
        let origin = self.aim().map_or([0.0; 3], |(origin, _)| origin);
        let distance = |p: [f64; 3]| {
            (p[0] - origin[0]).powi(2) + (p[1] - origin[1]).powi(2) + (p[2] - origin[2]).powi(2)
        };
        match (self.pick(), self.pick_fluid()) {
            (Some(block), Some(fluid)) => Some(if distance(block.point) <= distance(fluid.point) {
                block
            } else {
                fluid
            }),
            (block, fluid) => block.or(fluid),
        }
    }
    /// `BucketItem.use`'s place position: a water bucket fills the clicked
    /// block itself when that block can hold it (`canBlockContainFluid`: a
    /// waterloggable stair, slab, fence, ..., but not a double slab, kelp or
    /// seagrass); every other bucket empties into the block the clicked face
    /// leads to. `SolidBucketItem` places like a block item, into a
    /// replaceable clicked block.
    pub(super) fn bucket_target(&self, content: BucketContent, hit: &Hit) -> [i32; 3] {
        let clicked = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let behaviours = self.res.behaviours();
        if content == BucketContent::Powder {
            if self.res.facts().get(clicked).replaceable {
                return hit.pos;
            }
        } else if content.is_water()
            && behaviours.is_liquid_container(clicked)
            && behaviours.can_place_liquid(
                clicked,
                horizon_world::fluid::FluidKind::Water,
                self.creative,
            )
        {
            return hit.pos;
        }
        hit.beside()
    }
    /// The cauldron at `pos`, and whether water is above it
    /// (`CauldronInteractions.isUnderWater`).
    pub(super) fn cauldron_at(
        &self,
        pos: [i32; 3],
    ) -> Option<(horizon_world::chunk::BlockState, bool)> {
        let registry = self.pack.assets.registry;
        let id = self.world.block(pos[0], pos[1], pos[2]);
        let state = registry
            .state(id)
            .filter(|s| s.name.ends_with("cauldron"))
            .cloned()?;
        let above = self.world.block(pos[0], pos[1] + 1, pos[2]);
        let under_water = registry
            .state(above)
            .is_some_and(|s| s.name == "minecraft:water");
        Some((state, under_water))
    }
    /// What `BlockTransformer.transformBlock` would do with a held axe, hoe
    /// or shovel on the block at `pos`, without doing it: the new state, its
    /// sound, loot table, the durability it costs, its particle and its
    /// transform type.
    pub(super) fn block_transform(
        &self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
    ) -> Option<(BlockStateId, String, Option<String>, u32, String, String)> {
        use horizon_client::block_transform::Around;
        struct Column<'w> {
            world: &'w World,
            registry: &'w horizon_world::registry::BlockRegistry,
            pos: [i32; 3],
        }
        impl Around for Column<'_> {
            fn state(&self, o: [i32; 3]) -> horizon_world::chunk::BlockState {
                let id =
                    self.world
                        .block(self.pos[0] + o[0], self.pos[1] + o[1], self.pos[2] + o[2]);
                self.registry.state(id).cloned().unwrap_or_else(|| {
                    horizon_world::chunk::BlockState {
                        name: "minecraft:air".into(),
                        properties: Default::default(),
                    }
                })
            }
        }
        let id = self
            .res
            .items()
            .get(&stack.item)
            .and_then(|i| i.block_transformer.clone())?;
        // playerHasBlockingItemUseIntent: a shield in the off hand is raised
        // instead, unless sneaking.
        let shield = self
            .offhand
            .as_ref()
            .is_some_and(|s| self.blocks_attacks(s).is_some());
        if shield && !self.keys.held(Mapping::Sneak) {
            return None;
        }
        let face_name = match face {
            [0, -1, 0] => "down",
            [0, 1, 0] => "up",
            [0, 0, -1] => "north",
            [0, 0, 1] => "south",
            [-1, 0, 0] => "west",
            _ => "east",
        };
        let registry = self.pack.assets.registry;
        let around = Column {
            world: &self.world,
            registry,
            pos,
        };
        let (_, t, state) =
            self.res
                .transformers()
                .apply(self.items(), registry, &id, face_name, &around)?;
        let new = registry.resolve(&state).ok()?;
        Some((
            new,
            t.sound.clone(),
            t.loot.clone(),
            t.damage,
            t.particle.clone(),
            t.transform_type.clone(),
        ))
    }
    /// `BlockTransformer.TransformParticle.send`: its level event.
    pub(super) fn transform_particle(&mut self, particle: &str, pos: [i32; 3]) {
        match particle {
            "scrape" => self.level_event_at(3005, pos, 0),
            "wax_on" => self.level_event_at(3003, pos, 0),
            "wax_off" => self.level_event_at(3004, pos, 0),
            _ => {}
        }
    }
    /// `BlockTransformer.transformBlock` for a held axe, hoe or shovel.
    fn use_block_transformer(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
    ) -> Result<bool> {
        let Some((new, sound, loot, damage, particle, transform_type)) =
            self.block_transform(stack, pos, face)
        else {
            return Ok(false);
        };
        // BlockTransformer.transformBlock: ITEM_USED_ON_BLOCK before the swap.
        self.trigger_block("minecraft:item_used_on_block", pos, stack);
        // A loot table drops on the clicked face, before the block changes.
        if loot.as_deref() == Some("minecraft:till/rooted_dirt") {
            self.pop_resource_from_face(pos, face, ItemStack::new("minecraft:hanging_roots", 1));
        }
        let old = self.world.block(pos[0], pos[1], pos[2]);
        self.level_set(pos, new)?;
        // BlockTransformer.TransformParticle.send.
        self.transform_particle(&particle, pos);
        // `Level.playSound(player, ...)`: the actor's client already played
        // the strip/scrape/till sound in `predict_item_on_block` (#4681).
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        if self.lan_acting.is_some() {
            self.audio.play_unshared(&sound, Some(at), 1.0, 1.0);
            self.guest_player_sounds.push((sound.clone(), at, 1.0, 1.0));
        } else {
            self.audio.play(&sound, Some(at), 1.0, 1.0);
        }
        // ItemStack.useOn: a use that acted counts as one of the item.
        self.award_stat("used", &stack.item, 1);
        self.damage_held(damage);
        self.post_block_game_event("minecraft:block_change", pos, self.sculk_actor(), Some(new));
        if transform_type == "copper_chest" {
            self.transform_copper_chest_neighbour(pos, old, &particle);
        }
        Ok(true)
    }
    /// `HoneycombItem.useOn`'s `Level.playSound(player, ...)`: the actor's client
    /// already played it in `predict_item_on_block`.
    fn wax_on_sound(&mut self, at: [f64; 3]) {
        let sound = "minecraft:item.honeycomb.wax_on";
        if self.lan_acting.is_some() {
            self.audio.play_unshared(sound, Some(at), 1.0, 1.0);
            self.guest_block_sounds
                .push((sound.to_owned(), at, 1.0, 1.0));
        } else {
            self.audio.play(sound, Some(at), 1.0, 1.0);
        }
    }
    /// `BlockTransformer.transformBlock`'s `COPPER_CHEST` extra: a double
    /// chest's other half hears a block change and gets the same particle.
    fn transform_copper_chest_neighbour(
        &mut self,
        pos: [i32; 3],
        old: BlockStateId,
        particle: &str,
    ) {
        let old = self.pack.assets.registry.state(old).cloned();
        let Some(old) = old else { return };
        if !old.name.ends_with("copper_chest") {
            return;
        }
        let kind = old
            .properties
            .get("type")
            .map(String::as_str)
            .unwrap_or("single");
        let Some(facing) = old.properties.get("facing").map(String::as_str) else {
            return;
        };
        // `ChestBlock.getConnectedDirection`: LEFT is facing.getClockWise().
        let (dx, dz) = match (kind, facing) {
            ("left", "north") => (1, 0),
            ("left", "south") => (-1, 0),
            ("left", "west") => (0, -1),
            ("left", "east") => (0, 1),
            ("right", "north") => (-1, 0),
            ("right", "south") => (1, 0),
            ("right", "west") => (0, 1),
            ("right", "east") => (0, -1),
            _ => return,
        };
        let neighbour = [pos[0] + dx, pos[1], pos[2] + dz];
        let state = self.world.block(neighbour[0], neighbour[1], neighbour[2]);
        self.post_block_game_event(
            "minecraft:block_change",
            neighbour,
            self.sculk_actor(),
            Some(state),
        );
        self.transform_particle(particle, neighbour);
    }
    fn set_state_named(&mut self, pos: [i32; 3], name: &str, keep: &[&str]) -> Result<bool> {
        let registry = &self.pack.assets.registry;
        let old = self.world.block(pos[0], pos[1], pos[2]);
        let Some(default) = registry.default_state(name) else {
            return Ok(false);
        };
        let mut state = registry.state(default).cloned().ok_or("state")?;
        if let Some(previous) = registry.state(old) {
            for k in keep {
                if let Some(v) = previous.properties.get(*k) {
                    state.properties.insert((*k).to_owned(), v.clone());
                }
            }
        }
        let id = registry.resolve(&state).unwrap_or(default);
        self.level_set(pos, id)?;
        Ok(true)
    }
    /// `LayeredCauldronBlock.handleEntityOnFireInside` for a burning player
    /// in the cauldron at `pos` (`mayInteract`: not a spectator): the fill
    /// level drops (`lowerFillLevel`, a `block_change` with no source). A
    /// server's cauldron is the server's.
    pub(super) fn cauldron_extinguish(&mut self, pos: BlockPos) {
        if self.remote.is_some() {
            return;
        }
        let at = [pos.x, pos.y, pos.z];
        let registry = self.pack.assets.registry;
        let Some(old) = registry
            .state(self.world.block(at[0], at[1], at[2]))
            .filter(|s| {
                matches!(
                    s.name.as_str(),
                    "minecraft:water_cauldron" | "minecraft:powder_snow_cauldron"
                )
            })
            .cloned()
        else {
            return;
        };
        let Ok(new) = registry.resolve(&horizon_client::cauldron::extinguished(&old)) else {
            return;
        };
        let level = |state: &horizon_world::chunk::BlockState| {
            if matches!(state.name.as_str(), "minecraft:cauldron") {
                0
            } else if state.name == "minecraft:lava_cauldron" {
                3
            } else {
                state
                    .properties
                    .get("level")
                    .and_then(|level| level.parse().ok())
                    .unwrap_or(1)
            }
        };
        let Some(new_state) = registry.state(new).cloned() else {
            return;
        };
        if !self.native_cauldron_level_change_allowed(
            pos,
            level(&old),
            level(&new_state),
            "extinguishing",
        ) {
            return;
        }
        if self.level_set(at, new).is_ok() {
            self.post_block_game_event("minecraft:block_change", at, None, Some(new));
        }
    }
    /// Block tool use: distinguish success, failure and fall-through for hand feedback.
    fn use_tool(
        &mut self,
        stack: &ItemStack,
        allow_block_actions: bool,
    ) -> Result<horizon_client::first_person::UseResult> {
        use horizon_client::first_person::UseResult;
        let item = stack.item.trim_start_matches("minecraft:").to_owned();
        let at = |p: [i32; 3]| {
            [
                f64::from(p[0]) + 0.5,
                f64::from(p[1]) + 0.5,
                f64::from(p[2]) + 0.5,
            ]
        };
        if allow_block_actions
            && let Some(hit) = self.pick()
            && self.use_on_cauldron(hit.pos)?
        {
            return Ok(UseResult::Success { swing: true });
        }
        if allow_block_actions
            && let Some(hit) = self.pick()
            && let Some(placed) = self.use_on_campfire(hit.pos)?
        {
            // SUCCESS_SERVER swings; a full campfire's CONSUME does not.
            return Ok(UseResult::Success { swing: placed });
        }
        // BucketItem has no useOn: a guest's UseItemOn leaves it to UseItem.
        let bucket = horizon_client::items::bucket_contents(&stack.item);
        if matches!(self.guest_use, Some(GuestUse::OnBlock(_)))
            && (item == "bucket" || bucket.is_some())
        {
            return Ok(UseResult::Pass);
        }
        if item == "bucket" {
            let Some(hit) = self.bucket_hit() else {
                return Ok(UseResult::Pass);
            };
            // `BucketItem.use`: `level.mayInteract` and `mayUseItemAt`.
            if !self.may_build() && !self.can_place_on_in_adventure(hit.pos) {
                return Ok(UseResult::Fail);
            }
            let mut canceled = false;
            self.native_fill_bucket(stack, hit.pos, &mut canceled);
            if canceled {
                return Ok(UseResult::Fail);
            }
            if self.is_powder_snow(hit.pos) {
                self.pick_up_powder_snow(hit.pos)?;
                return Ok(UseResult::Success { swing: true });
            }
            // `BucketPickup.pickupBlock`: a fluid source, a waterlogged block
            // or powder snow. A block that gives nothing fails the click.
            let pos = BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]);
            let creative = self.creative;
            let Some(filled) =
                self.with_level(|b, level, ticks| b.bucket_pickup(level, ticks, pos, creative))
            else {
                return Ok(UseResult::Fail);
            };
            self.process_level_events()?;
            // `player.awardStat(Stats.ITEM_USED.get(this))`,
            // `getPickupSound` and `level.gameEvent(player, FLUID_PICKUP)`.
            self.award_stat("used", "minecraft:bucket", 1);
            let sound = if horizon_world::level::is_lava_bucket(filled) {
                "minecraft:item.bucket.fill_lava"
            } else {
                "minecraft:item.bucket.fill"
            };
            self.audio.play(sound, Some(at(hit.pos)), 1.0, 1.0);
            self.post_block_game_event(
                "minecraft:fluid_pickup",
                hit.pos,
                Some(horizon_client::mobs::Who::Player),
                None,
            );
            let filled = ItemStack::new(filled, 1);
            self.filled_result(filled.clone());
            self.trigger_item("minecraft:filled_bucket", &filled);
            return Ok(UseResult::Success { swing: true });
        }
        // BucketItem.use has its own ray, even when the crosshair hits an entity.
        if !allow_block_actions && bucket.is_none() {
            return Ok(UseResult::Pass);
        }
        let Some(hit) = self.pick() else {
            return Ok(UseResult::Pass);
        };
        let clicked = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        if let Some((content, sound, mob)) = bucket {
            if !self.may_build() && !self.can_place_on_in_adventure(hit.pos) {
                return Ok(UseResult::Fail);
            }
            let target = self.bucket_target(content, &hit);
            let mut canceled = false;
            self.native_fill_bucket(stack, target, &mut canceled);
            // `mayUseItemAt(blockPos2)`: a block that refuses the fluid passes
            // it to the block beside, so a mod hears that block too (powder
            // snow places like a block and never goes there).
            let beside_too = content != BucketContent::Powder && hit.beside() != target;
            if !canceled && beside_too {
                self.native_fill_bucket(stack, hit.beside(), &mut canceled);
            }
            if canceled
                || self.bucket_empty_cancelled(stack, target)
                || beside_too && self.bucket_empty_cancelled(stack, hit.beside())
            {
                return Ok(UseResult::Fail);
            }
            let pos = BlockPos::new(target[0], target[1], target[2]);
            let [x, y, z] = hit.beside();
            let beside = Some(BlockPos::new(x, y, z));
            // `Player.isShiftKeyDown`: a block that refuses the fluid (any
            // non-air block under Shift) passes it to the block beside, where
            // Shift no longer counts.
            let sneaking = self.keys.held(Mapping::Sneak);
            let creative = self.creative;
            let emptied = self.with_level(|b, level, ticks| {
                b.bucket_empty(
                    level, ticks, pos, beside, content, sound, sneaking, creative,
                )
            });
            self.process_level_events()?;
            let Some(emptied) = emptied else {
                return Ok(UseResult::Fail);
            };
            if self.dimension_type().water_evaporates && content.is_water() {
                self.evaporation_smoke([emptied.x, emptied.y, emptied.z]);
            }
            // `BucketItem.emptyContents` uses `playSound(player, …)`: the
            // actor's client already played the empty / evaporate sound from
            // its DryLevel prediction (#4684).
            if self.lan_acting.is_some()
                && let Some(capture) = self.audio.capture.as_mut()
            {
                let want = sound.trim_start_matches("minecraft:");
                let mut keep = Vec::with_capacity(capture.len());
                for entry in capture.drain(..) {
                    let name = entry.event.trim_start_matches("minecraft:");
                    if name == want || name == "block.fire.extinguish" {
                        self.guest_player_sounds.push((
                            entry.event,
                            entry.position,
                            entry.volume,
                            entry.pitch,
                        ));
                    } else {
                        keep.push(entry);
                    }
                }
                *capture = keep;
            }
            if let Some(kind) = mob {
                if let Some(id) = self.spawn_mob_at(kind, (target[0], target[1], target[2]), true) {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                        horizon_client::entity_nbt::apply_item_components(m, stack);
                    }
                    self.mob_placed(id)?;
                }
                self.post_block_game_event(
                    "minecraft:entity_place",
                    target,
                    self.sculk_actor(),
                    None,
                );
            }
            self.trigger_block("minecraft:placed_block", target, stack);
            if !self.creative {
                *self.held_mut() = Some(ItemStack::new("minecraft:bucket", 1));
            }
            return Ok(UseResult::Success { swing: true });
        }
        if self.use_block_transformer(stack, hit.pos, hit.face)? {
            return Ok(UseResult::Success { swing: true });
        }
        if item == "honeycomb" {
            let registry = self.pack.assets.registry;
            let old = registry.state(clicked);
            if let Some(waxed) = old.and_then(|old| self.res.transformers().waxed(registry, old))
                && let Ok(state) = registry.resolve(&waxed)
            {
                let other = old.and_then(|old| storage::chest_connected_pos(old, hit.pos));
                // HoneycombItem.useOn: ITEM_USED_ON_BLOCK before the swap.
                self.trigger_block("minecraft:item_used_on_block", hit.pos, stack);
                self.level_set(hit.pos, state)?;
                self.level_event_at(3003, hit.pos, 0);
                self.wax_on_sound(at(hit.pos));
                // HoneycombItem.useOn's game event, and ItemStack.useOn's used statistic.
                self.post_block_game_event(
                    "minecraft:block_change",
                    hit.pos,
                    self.sculk_actor(),
                    Some(state),
                );
                // A double chest's other half: its game event and particles, and
                // a second sound from the clicked block.
                if let Some(other) = other {
                    let now = self.world.block(other[0], other[1], other[2]);
                    self.post_block_game_event(
                        "minecraft:block_change",
                        other,
                        self.sculk_actor(),
                        Some(now),
                    );
                    self.level_event_at(3003, other, 0);
                    self.wax_on_sound(at(hit.pos));
                }
                self.award_stat("used", &stack.item, 1);
                if !self.creative {
                    self.consume_held();
                }
                return Ok(UseResult::Success { swing: true });
            }
        }
        if item == "flint_and_steel" || item == "fire_charge" {
            if !self.may_build() && !self.can_place_on_in_adventure(hit.pos) {
                return Ok(UseResult::Fail);
            }
            let name = self.res.facts().get(clicked).name.clone();
            if self.can_light(clicked, &name) {
                let registry = self.pack.assets.registry;
                let mut state = registry
                    .state(clicked)
                    .cloned()
                    .ok_or("missing lit block state")?;
                state.properties.insert("lit".into(), "true".into());
                self.level_set(hit.pos, registry.resolve(&state)?)?;
                self.audio.play(
                    if item == "fire_charge" {
                        "minecraft:item.firecharge.use"
                    } else {
                        "minecraft:item.flintandsteel.use"
                    },
                    Some(at(hit.pos)),
                    1.0,
                    horizon_client::items::fire_starter_pitch(
                        item == "fire_charge",
                        &mut self.world.random,
                    ),
                );
                // FlintAndSteelItem / FireChargeItem.useOn: BLOCK_CHANGE at the lit block,
                // and ItemStack.useOn's used statistic.
                self.post_block_game_event(
                    "minecraft:block_change",
                    hit.pos,
                    self.sculk_actor(),
                    None,
                );
                self.award_stat("used", &stack.item, 1);
                if item == "fire_charge" {
                    self.consume_held();
                } else {
                    self.damage_held(1);
                }
                return Ok(UseResult::Success { swing: true });
            }
            let target = [
                hit.pos[0] + hit.face[0],
                hit.pos[1] + hit.face[1],
                hit.pos[2] + hit.face[2],
            ];
            if self.can_place_used_fire(target) {
                if !self.try_light_portal(target)? {
                    self.place_used_fire(target)?;
                }
                let sound = horizon_client::items::fire_starter_sound(item == "fire_charge");
                let pitch = horizon_client::items::fire_starter_pitch(
                    item == "fire_charge",
                    &mut self.world.random,
                );
                self.audio.play(sound, Some(at(target)), 1.0, pitch);
                // BLOCK_PLACE: flint and steel names the clicked block, a fire charge the
                // fire. Then ItemStack.useOn's used statistic.
                let placed_at = if item == "fire_charge" {
                    target
                } else {
                    hit.pos
                };
                self.post_block_game_event(
                    "minecraft:block_place",
                    placed_at,
                    self.sculk_actor(),
                    None,
                );
                self.award_stat("used", &stack.item, 1);
                if item == "fire_charge" {
                    self.consume_held();
                } else {
                    self.damage_held(1);
                }
                return Ok(UseResult::Success { swing: true });
            }
            return Ok(UseResult::Fail);
        }
        Ok(UseResult::Pass)
    }
    fn can_place_used_fire(&mut self, target: [i32; 3]) -> bool {
        let pos = BlockPos::new(target[0], target[1], target[2]);
        let direction = match ((self.yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) {
            0 => Direction::South,
            1 => Direction::West,
            2 => Direction::North,
            _ => Direction::East,
        };
        self.with_level(|b, level, ticks| b.fire_can_be_placed_at(level, ticks, pos, direction))
    }
    fn place_used_fire(&mut self, target: [i32; 3]) -> Result<()> {
        let pos = BlockPos::new(target[0], target[1], target[2]);
        let state = self.with_level(|b, level, ticks| b.fire_state_for(level, ticks, pos));
        self.level_set(target, state)
    }
    /// DropExperienceBlock / RedStoneOreBlock / SculkBlock experience ranges.
    fn block_experience(&mut self, name: &str) -> u32 {
        let (min, max) = match name.trim_start_matches("minecraft:") {
            "coal_ore" | "deepslate_coal_ore" => (0, 2),
            "nether_gold_ore" => (0, 1),
            "lapis_ore" | "deepslate_lapis_ore" | "nether_quartz_ore" => (2, 5),
            "diamond_ore" | "deepslate_diamond_ore" | "emerald_ore" | "deepslate_emerald_ore" => {
                (3, 7)
            }
            "redstone_ore" | "deepslate_redstone_ore" => (1, 5),
            "sculk" => (1, 1),
            "sculk_catalyst" | "sculk_sensor" | "calibrated_sculk_sensor" | "sculk_shrieker" => {
                (5, 5)
            }
            // SpawnerBlock.spawnAfterBreak: 15 + nextInt(15) + nextInt(15).
            "spawner" => {
                let r = &mut self.world.random;
                let a = r.next_int_bound(15).unwrap_or(0);
                return (15 + a + r.next_int_bound(15).unwrap_or(0)) as u32;
            }
            _ => (0, 0),
        };
        // ConstantInt.sample (sculk) does not advance the level random.
        if min == max {
            return min as u32;
        }
        (min + self.world.random.next_int_bound(max - min + 1).unwrap_or(0)) as u32
    }
    /// `ExperienceOrb.followNearbyPlayer`'s candidates: each orb goes for the
    /// nearest living player within 8 blocks, the host's own or a guest. Each
    /// is the player's entity id (by which an orb keeps its
    /// `followingPlayer`), the player's feet, and the point the orb homes on.
    pub(super) fn orb_followed(&self) -> Vec<(u32, [f64; 3], [f64; 3])> {
        let spectator = self.spectator();
        let player = self
            .player
            .as_ref()
            .filter(|p| !p.health.dead_or_dying() && !spectator);
        let mut followed: Vec<(u32, [f64; 3], [f64; 3])> = player
            .map(|p| {
                let feet = [p.position.x, p.position.y, p.position.z];
                let eye = f64::from(p.dimensions.eye_height);
                (
                    p.entity_id as u32,
                    feet,
                    [feet[0], feet[1] + eye / 2.0, feet[2]],
                )
            })
            .into_iter()
            .collect();
        if let Some(host) = self.lan.as_ref() {
            followed.extend(
                host.hub
                    .guests
                    .iter()
                    .filter(|g| g.placed && g.loaded && !g.dead && g.health > 0.0)
                    .filter(|g| !g.spectator())
                    .map(|g| {
                        let p = g.position;
                        (
                            g.entity_id as u32,
                            p,
                            [p[0], p[1] + g.eye_height() / 2.0, p[2]],
                        )
                    }),
            );
        }
        followed
    }
    /// `Player.getVehicle().getBoundingBox()` for the local player's vehicle
    /// (`riding`): a mob, one of `object_target`'s kinds (boat, cart, armour
    /// stand, item frame, painting) or a tracked special — the same chain
    /// `guest_target_box` resolves a guest's vehicle through.
    fn riding_box(&self) -> Option<Aabb> {
        let id = self.riding?;
        if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
            return Some(m.bounds());
        }
        if let Some((lo, hi, _)) = self.object_target(id as i32) {
            return Some(Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            ));
        }
        self.tracked_special_box(id as i32)
    }
    /// `FurnaceResultSlot.checkTakeAchievements`: the player took from the output of the
    /// furnace at `pos`, which pops the experience of what it made at them
    /// (`awardUsedRecipesAndPopExperience`) and forgets it.
    pub(super) fn award_furnace_recipes(&mut self, pos: (i32, i32, i32), at: [f64; 3]) {
        let used = match self.world.block_entities.get_mut(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                let used = f.recipe_experience(world_recipes!(self));
                f.recipes_used.clear();
                used
            }
            _ => return,
        };
        for (count, experience) in used {
            let xp = horizon_client::containers::create_experience(
                count,
                experience,
                &mut self.world.random,
            );
            self.spawn_orbs(at, xp);
        }
    }
    /// `Player.giveExperiencePoints`, and `giveExperienceLevels`' level-up
    /// sound every fifth level (at most every 100 ticks).
    pub(super) fn give_orb_points(&mut self, gained: u32) {
        let Some(gained) = self.xp_change(gained as i32) else {
            return;
        };
        let gained = gained.max(0);
        let old_level = self.experience.level;
        let gave = self.experience.give(gained);
        self.native_player_level_changed(old_level);
        // Player.giveExperienceLevels runs per level, so the first multiple of 5
        // crossed plays the sound (and starts the cooldown for the rest).
        if let Some(level) = gave {
            self.play_level_up(1, level);
        }
    }
    /// The renderer for the window, with the current block sprites, GUI
    /// atlas and world border texture.
    /// A new renderer's atlas holds only the pack's sprites: what was drawn into it at run time
    /// (skins, maps, the themed UI's glyphs and corners) has to be drawn again.
    pub(super) fn forget_atlas_slots(&mut self) {
        self.skin_slots.forget();
        self.maps.slots.forget();
        self.trim_slots.forget();
        self.map_patches.clear();
        self.kept_block_entities.get_mut().clear();
        horizon_client::ui_theme::reset_atlas();
    }

    fn new_renderer(&self, window: Arc<Window>) -> Result<Renderer> {
        self.renderer_for(window, &self.pack, &self.gui)
    }
    /// Rebuild GPU-owned state after the backend reports device loss.
    fn recover_device(&mut self) -> Result<()> {
        if !self.renderer.as_ref().is_some_and(Renderer::device_lost) {
            return Ok(());
        }
        // Split-screen compositors and guests share the device; rebuilding one
        // renderer alone would leave the other windows using invalid resources.
        if self.options.paced_by_split {
            return Err("the graphics device was lost during split-screen play".into());
        }
        let now = Instant::now();
        if self
            .last_device_recovery
            .is_some_and(|last| now.duration_since(last) < Duration::from_secs(60))
        {
            return Err("the graphics device was lost again after recovery".into());
        }
        self.last_device_recovery = Some(now);
        let window = self
            .window
            .clone()
            .ok_or("no window for graphics recovery")?;
        log::warn!("Horizon: rebuilding the renderer after graphics device loss");
        // Drop the old surface and device before opening another on this window.
        let mut old = self
            .renderer
            .take()
            .ok_or("no renderer for graphics recovery")?;
        // A GPU-culled frame may be what lost it: the renderers after this one don't
        // offer GPU culling, until Horizon restarts (`graphics::lose_gpu_culling`).
        if old.gpu_frame() {
            log::warn!(
                "Horizon: GPU culling off until the next start: the graphics device was lost in one of its frames"
            );
            horizon_client::graphics::lose_gpu_culling();
        }
        let (screenshot, screenshot_result) = (old.screenshot.take(), old.screenshot_result.take());
        let display = old.display_reading();
        drop(old);
        let mut renderer = build_renderer(
            window,
            self.options.offscreen,
            &self.pack,
            &self.gui,
            None,
            None,
            self.settings.horizon.api,
            self.settings.horizon.translucent_batching,
            &self.settings.horizon,
            shader_packs::picked_pack_shape(&self.settings),
            display,
            (
                self.settings.file.int("textureFiltering"),
                self.settings.file.int("maxAnisotropyBit"),
            ),
        )?;
        (renderer.screenshot, renderer.screenshot_result) = (screenshot, screenshot_result);
        self.adopt_renderer(renderer);
        log::info!("Horizon: graphics device recovered; rebuilding world meshes");
        Ok(())
    }
    /// A renderer made again from the resources loaded takes over: what named the old one's
    /// atlas slots is forgotten, the world is meshed again, and the shader pack and the
    /// settings are applied again.
    pub(super) fn adopt_renderer(&mut self, mut renderer: Renderer) {
        // A new renderer starts with the vanilla column and without the world border's wall.
        renderer.clear_sections(self.world.layout());
        self.border_renderer.invalidate();
        self.renderer = Some(renderer);
        self.forget_atlas_slots();
        self.clear_lod();
        self.pending_uploads.clear();
        let context = self.worker_context();
        self.streaming.reload_resources(&mut self.world, context);
        self.native_shaders = native_shaders::NativeShaders::picking(&std::mem::take(
            &mut self.native_shaders.picked,
        ));
        self.apply_ui_settings();
    }
    /// `LevelExtractor.allChanged`: displayed sections and mesh work are
    /// discarded, then every loaded section is rebuilt.
    fn all_sections_changed(&mut self) {
        self.clear_lod();
        self.pending_uploads.clear();
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.clear_sections(self.world.layout());
        }
        self.streaming.reload_sections(&mut self.world);
    }
    /// Whether precipitation at a column is snow (Biome.coldEnoughToSnow:
    /// height adjusted temperature) or none (no precipitation).
    fn precipitation(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        let chunk = self.world.chunks.get(&(x >> 4, z >> 4))?;
        self.res
            .biomes()
            .get(chunk.biome(x, y, z))
            .snows_at(x, y, z)
    }
    /// `Level.isRainingAt`: it rains, the sky light is 15, nothing that stops
    /// motion is above, and the biome's precipitation there is rain.
    fn raining_at(&self, p: BlockPos) -> bool {
        self.weather_levels(1.0).0 > 0.2
            && self.world.light(p.x, p.y, p.z) >> 4 == 15
            && self.rain_height(p.x, p.z) <= p.y
            && self.precipitation(p.x, p.y, p.z) == Some(false)
    }
    /// `getHeight(Heightmap.Types.MOTION_BLOCKING, x, z)`: one above the
    /// highest block in `blocks_motion_in_heightmap` or holding a fluid.
    fn rain_height(&self, x: i32, z: i32) -> i32 {
        render_tick::motion_blocking_height(&self.world, &self.pack.assets.properties, x, z)
    }
    fn tick_weather_effects(&mut self) {
        if self.flash > 0 {
            self.flash -= 1;
        }
        // The rain sounds come with the rain splashes (`tick_particles`).
    }
    /// A translated message for the player the game acts for: `overlay` is
    /// `sendOverlayMessage` (the action bar), else `sendSystemMessage` (chat).
    /// A guest gets a packet, so its own client translates it.
    fn tell_sleeper(&mut self, key: &str, fallback: &str, overlay: bool) {
        if self.lan_acting.is_some() {
            self.guest_packets
                .push(horizon_protocol::packets::play::Clientbound::SystemChat(
                    horizon_protocol::packets::play::SystemChat {
                        content: horizon_protocol::text::translatable(key, &[]),
                        overlay,
                    },
                ));
        } else if overlay {
            self.message = Some((self.translate(key, fallback), Instant::now()));
        } else {
            self.system_message(horizon_client::text::Text::translate(key, &[]));
        }
    }
    /// ServerPlayer.startSleepInBed: in reach, not obstructed, night or thunder,
    /// and (outside creative) no monsters within 8 blocks; the bed becomes the
    /// respawn point either way.
    fn try_sleep(&mut self, pos: [i32; 3]) {
        let Some((position, yaw)) = self.player.as_ref().map(|p| (p.position, p.yaw)) else {
            return;
        };
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let props = self
            .pack
            .assets
            .registry
            .state(state)
            .map(|s| s.properties.clone())
            .unwrap_or_default();
        // The head and the foot: the block faces from the foot to the head.
        let d = facing_offset(props.get("facing").map_or("north", String::as_str));
        let (head, foot) = if props.get("part").map(String::as_str) == Some("foot") {
            ([pos[0] + d[0], pos[1], pos[2] + d[2]], pos)
        } else {
            (pos, [pos[0] - d[0], pos[1], pos[2] - d[2]])
        };
        // AbstractBedBlock.useWithoutItem: in an occupied bed a villager
        // asleep there is woken (kickVillagerOutOfBed), else someone else
        // sleeps in it; the player stays up either way.
        let head_state = self.world.block(head[0], head[1], head[2]);
        let occupied = self
            .pack
            .assets
            .registry
            .state(head_state)
            .is_some_and(|s| s.properties.get("occupied").is_some_and(|v| v == "true"));
        if occupied {
            if !self.kick_villager_out_of_bed(head) {
                self.tell_sleeper("block.minecraft.bed.occupied", "This bed is occupied", true);
            }
            return;
        }
        // ServerPlayer.bedInRange / isReachableBedBlock: within 3 blocks
        // sideways and 2 vertically of the head or the foot.
        let reachable = |bed: [i32; 3]| {
            (position.x - f64::from(bed[0]) - 0.5).abs() <= 3.0
                && (position.y - f64::from(bed[1])).abs() <= 2.0
                && (position.z - f64::from(bed[2]) - 0.5).abs() <= 3.0
        };
        let mut problem: Option<&'static str> = if !reachable(head) && !reachable(foot) {
            Some("too_far_away")
        } else {
            None
        };
        // ServerPlayer.bedBlocked: Player.freeAt (not suffocating) for the
        // blocks above the head and the foot, as `in_wall` reads suffocating.
        let blocked = problem.is_none()
            && [head, foot].iter().any(|bed| {
                let f = self
                    .res
                    .facts()
                    .get(self.world.block(bed[0], bed[1] + 1, bed[2]));
                f.full_collision && f.solid_render
            });
        if blocked {
            problem = Some("obstructed");
        }
        // In reach (bedInRange), day or night: setRespawnPosition(
        // RespawnConfig(the head, the player's yaw), not forced). It replaces
        // an anchor or a spawnpoint; respawning checks the bed is still there
        // and finds where to stand up (findRespawnAndUseSpawnBlock).
        // Straw beds never set it (`straw_bed_rule`).
        if problem.is_none() && self.bed_sets_spawn(state) {
            let config = respawn_anchor::RespawnConfig::wrapped(self.dim, head, yaw, false);
            // ServerPlayer.setRespawnPosition: the chat message only when the
            // point moved (`isSamePosition`).
            let moved = self
                .respawn_config
                .is_none_or(|c| (c.dim, c.pos) != (config.dim, config.pos));
            let mut cancel = false;
            if moved
                && let (Some(player), Some(name)) =
                    (self.lan_acting_uuid, self.lan_acting_name.clone())
            {
                self.native_player_spawn_point_set(
                    player,
                    &name,
                    self.dim.id(),
                    head,
                    "bed",
                    &mut cancel,
                );
            }
            if !cancel {
                self.respawn_config = Some(config);
                if moved {
                    self.tell_sleeper("block.minecraft.set_spawn", "Respawn point set", false);
                }
                // The fallback becomes the world spawn: a guest's is none of its
                // own, so it follows `setworldspawn`.
                self.spawn = if self.lan_acting.is_some() {
                    None
                } else {
                    self.level
                        .world_spawn
                        .map(|[x, y, z]| {
                            Vec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5)
                        })
                        .or(self.spawn)
                };
            }
        }
        if problem.is_none()
            && !self
                .dimension_type()
                .bed_rule
                .can_sleep
                .test(self.dark_outside())
        {
            problem = Some("not_possible_now");
        }
        // Creative players skip the monster check (`!this.isCreative()`).
        if problem.is_none()
            && !self.creative
            && self.mobs.iter().any(|m| {
                // `Monster.class` entities that `isPreventingPlayerRest`: a
                // zombified piglin only while it is angry.
                horizon_client::mobs::ai::is_monster(m.species())
                    && (m.species() != horizon_client::mobs::Species::ZombifiedPiglin
                        || (m.extra.anger > 0 && m.ai.target.is_some()))
                    && m.alive()
                    && (m.position.x - position.x).abs() < 8.0
                    && (m.position.y - position.y).abs() < 5.0
                    && (m.position.z - position.z).abs() < 8.0
            })
        {
            problem = Some("not_safe");
        }
        // NeoForge's CanPlayerSleepEvent: a mod may clear or replace the
        // problem vanilla found, or add one.
        let mut problem = problem.map(str::to_owned);
        if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            self.native_can_player_sleep(player, &name, head, &mut problem);
        }
        if let Some(problem) = problem {
            match problem.as_str() {
                "too_far_away" => self.tell_sleeper(
                    "block.minecraft.bed.too_far_away",
                    "You may not rest now; the bed is too far away",
                    true,
                ),
                "obstructed" => self.tell_sleeper(
                    "block.minecraft.bed.obstructed",
                    "This bed is obstructed",
                    true,
                ),
                "not_safe" => self.tell_sleeper(
                    "block.minecraft.bed.not_safe",
                    "You may not rest now; there are monsters nearby",
                    true,
                ),
                "not_possible_now" => self.tell_sleeper(
                    "block.minecraft.bed.no_sleep",
                    "You can sleep only at night or during thunderstorms",
                    true,
                ),
                // `other_problem` and any name a mod invents: refused without a message.
                _ => {}
            }
            return;
        }
        // ServerPlayer.startSleepInBed: a percentage above 100 cannot skip the night.
        if self.ui.rules.int("players_sleeping_percentage") > 100 {
            self.tell_sleeper(
                "sleep.not_possible",
                "No amount of rest can pass this night",
                true,
            );
        }
        if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            let mut cancel = false;
            self.native_player_bed_enter(player, &name, head, &mut cancel);
            if cancel {
                return;
            }
        }
        // LivingEntity.startSleeping: the shape the bed gives the player to
        // lie on (`getSleepHeight`). A straw bed's head answers with its
        // foot's shape, and with no foot of its own there is none, so the
        // player stays up as `startSleeping` returning false leaves them.
        let Some(at) = self.sleeping_position(head) else {
            return;
        };
        // LivingEntity.startSleeping: stopRiding once the bed is valid.
        self.stop_riding();
        self.stop_mining();
        self.left = false;
        self.right = false;
        self.using = None;
        self.sleeping = Some(head);
        self.sleep_timer = 0;
        // LivingEntity.startSleeping and ServerPlayer.startSleeping: setPosToBed
        // and connection.teleport.
        if let Some(p) = self.player.as_mut() {
            p.position = at;
            p.velocity = Vec3::ZERO;
            p.pitch = 0.0;
            p.teleported = true;
        }
        self.pitch = 0.0;
        if self.lan_acting.is_none() {
            self.previous_position = at;
            self.snap_player_model_pitch();
        }
        self.occupy_bed(head);
        self.award_custom_stat("sleep_in_bed", 1);
        self.trigger_player("minecraft:slept_in_bed");
        self.stats.reset("custom", "minecraft:time_since_rest");
    }
    fn wake_up(&mut self) {
        if self.remote.is_some() {
            // InBedChatScreen.sendWakeUp: the server wakes the player.
            self.remote_stop_sleeping();
            return;
        }
        let Some(head) = self.sleeping.take() else {
            return;
        };
        self.sleep_timer = 100;
        let stand = self.bed_wake_pose(head, self.yaw);
        if let Some(p) = self.player.as_mut() {
            if let Some((at, yaw)) = stand {
                p.position = at;
                p.velocity = Vec3::ZERO;
                p.yaw = yaw;
                p.pitch = 0.0;
            }
            let _ = p.set_sleeping(false);
        }
        if let Some((at, yaw)) = stand {
            self.previous_position = at;
            self.yaw = yaw;
            self.pitch = 0.0;
            self.snap_player_model_pitch();
        }
        if let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            self.native_player_bed_leave(player, &name, head);
        }
    }
    fn tick_sleep(&mut self) {
        self.sleep_timer = tick_sleep_timer(self.sleep_timer, self.sleeping.is_some());
        if let Some(head) = self.sleeping {
            // LivingEntity.tick's checkBedExists: a sleeper whose bed is
            // gone gets up.
            let bed = self.world.block(head[0], head[1], head[2]);
            let bed_gone = !self.res.facts().get(bed).name.ends_with("_bed");
            // Vanilla has no Shift wake-up: the chat screen's capitals must
            // not get the sleeper up.
            if (self.screen != Screen::Chat && self.keys.held(Mapping::Sneak))
                || bed_gone
                || self
                    .player
                    .as_ref()
                    .is_none_or(|p| p.health.dead_or_dying())
            {
                self.wake_up();
                return;
            }
            // setPosToBed: the bed's top (`getSleepHeight`) and 0.125 above
            // it. A sleeper whose bed no longer gives a shape to lie on stays
            // where it is: vanilla only ever places a sleeper when it starts.
            let at = self.sleeping_position(head);
            if let (Some(p), Some(at)) = (self.player.as_mut(), at) {
                p.position = at;
                p.velocity = Vec3::ZERO;
                p.pitch = 0.0;
            }
            // LivingEntity.tick: a sleeper's xRot is 0.
            self.pitch = 0.0;
            self.snap_player_model_pitch();
        }
        // ServerLevel.tick: once enough of the players have slept 100 ticks,
        // skip to morning if `advance_time` is on, wake everyone, and
        // resetWeatherCycle if `advance_weather` is on and it is raining.
        if self.enough_slept() {
            if self.ui.rules.bool("advance_time") {
                let t = self.world.day_time;
                let mut target = t + 24000 - t.rem_euclid(24000);
                // SleepFinishedTimeEvent: the mods may move the wake-up time,
                // never before now (`setTimeAddition` refuses a lower one).
                let mut asked = target;
                self.native_sleep_finished_time(self.dim.id(), t, &mut asked);
                if asked >= t {
                    target = asked;
                }
                let mut cancel = false;
                self.native_time_skip(self.dim.id(), t, target, "sleep", &mut cancel);
                if !cancel {
                    self.world.day_time = target;
                }
            }
            // Level.isRaining: the rain level is above 0.2.
            if self.ui.rules.bool("advance_weather") && self.weather_levels(1.0).0 > 0.2 {
                self.world.weather.raining = false;
                self.world.weather.thundering = false;
                self.world.weather.rain_time = 0;
                self.world.weather.thunder_time = 0;
            }
            if self.sleeping.is_some() {
                self.wake_up();
            }
            self.wake_guests();
            // SleepStatus.removeAllSleepers: waking everyone is not announced.
            self.sleep_seen.1 = 0;
        }
    }
    /// Copy the live player and world state into the `Level` that gets saved.
    pub(super) fn sync_level(&mut self) {
        self.level.player_impulse_grace = self.player.as_ref().map(|p| p.impulse.reset_grace_time);
        self.level.player_fall_particles = self
            .player
            .as_ref()
            .map(|p| p.spawn_extra_particles_on_fall);
        // Level.write saves the game rules (game_rules.dat) with the rest.
        self.level.game_rules = self.ui.rules.by_name();
        // PrimaryLevelData.setDifficulty: /difficulty and the options button change it.
        self.level.difficulty =
            horizon_client::title::DIFFICULTIES[usize::from(self.ui.difficulty) & 3].into();
        self.level.time = self.world.game_time;
        self.level.weather = self.world.weather;
        self.level.day_time = self.world.day_time;
        self.level.end_clock = Some(self.world.end_clock);
        // A server has no player of its own; `enter_guest` puts one in
        // `self.player` only while a command reads it.
        if self.player.is_none() && self.options.dedicated.is_some() {
            return;
        }
        self.level.selected_slot = self.selected as i32;
        let mode = self.mode_id();
        // The vehicle the player rides (`ServerPlayer.saveParentVehicle`).
        let others = self
            .lan
            .as_ref()
            .map(|h| self.ridden_by_others(&h.hub.guests, self.dim.id(), None, false))
            .unwrap_or_default();
        let vehicle = self.parent_vehicle_tag(self.riding, &others);
        // Only the local player's file is this one's to rewrite.
        let local = self.serving.is_none() && self.options.dedicated.is_none();
        let level = &mut self.level;
        level.creative = self.creative;
        level.game_mode = mode;
        level.previous_game_mode = self.previous_mode.map(game_mode::mode_id_of);
        level.spawn = self.spawn.map(|s| [s.x, s.y, s.z]);
        level.respawn_elsewhere = self.respawn_config.map(|c| c.saved());
        level.rotation = [self.yaw, self.pitch];
        level.xp_level = self.experience.level;
        level.xp_progress = self.experience.progress;
        level.xp_total = self.experience.total;
        level.root_vehicle = vehicle.as_ref().map(|v| v.2).or(self.pending_vehicle);
        // The ride's tree, or none once the file's own came back.
        match vehicle {
            Some((tag, ..)) => {
                let extra = level
                    .player_extra
                    .get_or_insert_with(|| horizon_world::nbt::Tag::Compound(Default::default()));
                if let horizon_world::nbt::Tag::Compound(map) = extra {
                    map.insert("RootVehicle".into(), tag);
                }
            }
            None if local && self.pending_vehicle.is_none() => {
                if let Some(horizon_world::nbt::Tag::Compound(map)) = &mut level.player_extra {
                    map.remove(&"RootVehicle".into());
                }
            }
            None => {}
        }
        if let Some(p) = &self.player {
            level.position = Some([p.position.x, p.position.y, p.position.z]);
            level.health = p.health.health();
            level.absorption = p.health.absorption();
            let extra = level
                .player_extra
                .get_or_insert_with(|| horizon_world::nbt::Tag::Compound(Default::default()));
            if let horizon_world::nbt::Tag::Compound(map) = extra {
                map.insert(
                    "Fire".into(),
                    horizon_world::nbt::Tag::Short(
                        p.fire_ticks.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                    ),
                );
                map.insert("Air".into(), horizon_world::nbt::Tag::Short(p.air as i16));
                // LivingEntity.addAdditionalSaveData: AttributeMap.pack.
                map.insert("attributes".into(), p.saved_attributes());
            }
            level.fall_distance = p.fall_distance;

            level.food = p.health.food.food;
            level.saturation = p.health.food.saturation;
            level.flying = p.flying;
        }
        level.inventory = self
            .inventory
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.as_ref()
                    .map(|s| (i as u8, s.item.to_string(), s.count, s.damage))
            })
            .collect();
        level.inventory_components = self
            .inventory
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Some((i as u8, s.as_ref()?.extra.clone()?)))
            .collect();
        if let Some(s) = &self.offhand {
            level
                .inventory
                .push((150, s.item.to_string(), s.count, s.damage));
            if let Some(extra) = s.extra.clone() {
                level.inventory_components.insert(150, extra);
            }
        }
        // Vanilla stores armor in slots 100 (feet) to 103 (head).
        for (i, s) in self.armor.iter().enumerate() {
            if let Some(s) = s {
                level
                    .inventory
                    .push((103 - i as u8, s.item.to_string(), s.count, s.damage));
                if let Some(extra) = s.extra.clone() {
                    level.inventory_components.insert(103 - i as u8, extra);
                }
            }
        }
        level.dimension = self.dim.id().to_owned();
        // An open ender chest holds the live copy.
        if let Some(key) = self.ender_open
            && let Some(BlockEntity::Chest(items)) = self.world.block_entities.get(&key)
        {
            self.ender_items = items.clone();
        }
        level.ender_items = self.ender_items.clone();
        level.active_effects = self.potions.effects.saved();
    }
    /// The autosave doesn't `wait` for the chunks to be on disk: every
    /// generated chunk is saved, hundreds at a time, encoded and written on
    /// the region writer meanwhile.
    fn save_game_with(&mut self, wait: bool) -> Result<()> {
        if self.remote.is_some() {
            return Ok(());
        }
        // `DedicatedServer.saveAllChunks` tells the management protocol's clients when a save starts and when
        // it ends without failing, for every save: the command, the autosave, the pause.
        let dedicated = self.options.dedicated.is_some();
        if dedicated {
            dedicated::events::record_server("saving", serde_json::json!({}));
        }
        self.save_level(wait)?;
        self.save_parked_levels(wait)?;
        self.save_game_metadata()?;
        let _ = self.finish_player_autosaves(wait);
        self.native_world_saved(wait);
        if dedicated {
            dedicated::events::record_server("saved", serde_json::json!({}));
        }
        Ok(())
    }
    /// Save the level in place: its changed chunks, its entities and its raids.
    fn save_level(&mut self, wait: bool) -> Result<()> {
        self.save_border();
        self.save.save_attachments(&self.world.attachments)?;
        self.save
            .save_chunk_tickets(&self.streaming.forced, self.tickets.portal_tickets())?;
        self.streaming.save(&mut self.world, wait);
        let now = Instant::now();
        let keys: Vec<_> = self.world.modified.drain().collect();
        let chunks = self.with_entities(keys.iter().filter_map(|k| self.world.chunks.get(k)));
        self.save
            .save_later(&chunks, self.pack.assets.registry, world_biomes!(self))?;
        // Prepare entities while the chunk writer runs, then use one barrier.
        if !wait {
            self.world.modified.autosaved(&keys, now);
        }
        self.save_entities(false)?;
        self.save_raids();
        if wait {
            self.save.flush()?;
        }
        Ok(())
    }
    /// `ChunkMap.saveChunksEagerly`: snapshot only while the tick and writer
    /// have room. NBT encoding, compression and durable writes stay on workers.
    fn save_chunks_eagerly(&mut self) {
        // No world open (`world_root` is set by `finish_open_world`): the save
        // is `Game::new`'s placeholder under the working directory.
        if self.remote.is_some()
            || self.world_root.as_os_str().is_empty()
            || self.agent.hold_server
            || self.clock.sprinting()
            || self
                .server_lists
                .as_ref()
                .is_some_and(|lists| !lists.lock().unwrap().autosave)
        {
            return;
        }
        let deadline = self
            .server_tick_deadline
            .unwrap_or_else(|| Instant::now() + self.clock.tick_duration());
        let mut modified = std::mem::take(&mut self.world.modified);
        modified.save_eagerly(
            Instant::now(),
            || Instant::now() < deadline && self.save.active_chunk_writes() < 128,
            |key| self.world.chunks.contains_key(&key),
            |key| {
                // `saveChunkIfNeeded` saves only a LevelChunk, so a lit one; a
                // column still waiting for its light keeps its queue entry.
                let Some(chunk) = self.world.chunks.get(&key).filter(|c| c.lit) else {
                    return false;
                };
                let chunks = self.with_entities([chunk]);
                match self
                    .save
                    .save_later(&chunks, self.pack.assets.registry, world_biomes!(self))
                {
                    Ok(_) => true,
                    Err(e) => {
                        log::warn!("Horizon: failed to save changed chunks: {e}");
                        false
                    }
                }
            },
        );
        self.world.modified = modified;
    }
    /// `GameRenderer.tryTakeScreenshotIfNeeded`: while a local world without a
    /// saved icon is on the screen, grab the rendered frame once a second until
    /// the save has one. The Select World list shows it as the world's preview.
    fn tick_world_icon(&mut self) {
        // Its grab would take a sequence's frame.
        if self.screenshot_sequence.is_some() {
            return;
        }
        let Some(icon) = self.world_icon.as_ref() else {
            return;
        };
        if icon
            .last_attempt
            .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        if icon.path.is_file() {
            // tryTakeScreenshotIfNeeded's hasWorldScreenshot: it has one, stop asking.
            self.world_icon = None;
            return;
        }
        // takeAutoScreenshot only saves with every loaded section rendered, or
        // the icon would picture the holes the streamer has not filled yet.
        if self.streaming.pending() != 0
            || !self.pending_uploads.is_empty()
            || self.remote_backlog() != 0
        {
            return;
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        // countRenderedSections() > 10: a client that joined its server
        // thread has nothing pending before the first chunks arrive, and the
        // icon would be only sky.
        if renderer.drawn_sections <= 10 {
            return;
        }
        if renderer.screenshot.is_some() {
            // F2's or an agent's screenshot owns the frame this time; ours
            // comes around again with the next attempt.
            return;
        }
        let path = icon.path.clone();
        if let Some(icon) = self.world_icon.as_mut() {
            icon.last_attempt = Some(Instant::now());
        }
        renderer.screenshot = Some(horizon_client::gfx::ScreenshotRequest {
            path,
            size: Some([64, 64]),
            scaled: false,
        });
    }
    fn render(&mut self) -> Result<bool> {
        if self.screenshot_sequence.is_some() {
            return self.render_sequence_frame();
        }
        self.render_frame()
    }
    fn render_frame(&mut self) -> Result<bool> {
        let prepare = spans::guard("prepare");
        let mut pr = spans::laps();
        // The window's GPU holds the atlas: its copy in memory is kept
        // compressed (a no-op once it is).
        if self.renderer.is_some() {
            self.gui.atlas.pack_pixels();
            self.pack.render.pixels.pack();
        }
        // Before any text of this frame: a full glyph region starts over.
        self.gui.atlas.font.begin_frame();
        if self.clock.still() {
            // Obfuscated text picks the same glyphs in a frame of the same world tick.
            self.gui.atlas.font.reseed(self.visual_ticks() as i64);
        }
        self.glint_frame = self.capture_glint_frame();
        if self.renderer.as_mut().is_some_and(|r| r.display_changed()) {
            self.apply_ui_settings();
        }
        self.follow_frame();
        self.observe_screen();
        // `Gui.setScreen` clears the focus of the screen it leaves: a menu
        // that comes back starts with no widget highlighted.
        self.drop_left_screen_focus();
        // Minecraft.doWorldLoad: the LevelLoadingScreen while single player's
        // server starts, with the server's chunk map and progress.
        if let Some(starting) = &self.integrated_starting {
            // WorldOpenFlows.upgradeAndOpenWorld: the server's file fixers run on
            // FileFixerProgressScreen, before the LevelLoadingScreen has progress.
            let upgrade = starting.handle.upgrade();
            if upgrade.is_file_fixing() {
                horizon_client::widgets::begin_screen(|| {
                    horizon_client::widgets::ScreenInfo::new(
                        "file_fixer",
                        screen_info::tr("upgradeWorld.title", &[]),
                    )
                });
                if let Some(renderer) = &mut self.renderer {
                    let mut hud = HudBatch::new([renderer.config.width, renderer.config.height]);
                    horizon_client::level_loading::draw_file_fixer(
                        &mut hud,
                        &self.gui.atlas,
                        upgrade,
                        self.cursor,
                    );
                    pr.lap("pr_loading");
                    drop(prepare);
                    return renderer.render(&loading_frame(&hud));
                }
                return Ok(false);
            }
            // The server's progress once it has some (`hasProgress`).
            let progress = Some(starting.handle.load_view())
                .filter(|v| v.has_progress())
                .map(|v| v.server_progress());
            horizon_client::widgets::begin_screen(|| {
                screen_info::level_loading(progress, &screen_info::tr)
            });
            if let Some(renderer) = &mut self.renderer {
                let mut hud = HudBatch::new([renderer.config.width, renderer.config.height]);
                let (w, h) = (hud.width, hud.height);
                horizon_client::ui::menu_background(
                    &mut hud,
                    &self.gui.atlas,
                    0.0,
                    0.0,
                    w,
                    h,
                    false,
                );
                // The chunk map and progress are the server thread's.
                let view = self
                    .integrated_starting
                    .as_ref()
                    .map(|s| s.handle.load_view().clone());
                let status = |x: i32, z: i32| view.as_ref().and_then(|v| v.status(x, z));
                let progress = view
                    .as_ref()
                    .is_some_and(|v| v.has_progress())
                    .then_some(self.loading_progress);
                horizon_client::level_loading::draw(
                    &mut hud,
                    &self.gui.atlas,
                    Some(&status),
                    progress,
                );
                pr.lap("pr_loading");
                drop(prepare);
                return renderer.render(&loading_frame(&hud));
            }
            return Ok(false);
        }
        if self.pending_entry.is_some() {
            if let (Some(renderer), Some(loader)) = (&mut self.renderer, &self.rest_loader) {
                horizon_client::widgets::begin_screen(|| {
                    screen_info::level_loading(Some(loader.progress.fraction()), &screen_info::tr)
                });
                let mut hud = HudBatch::new([renderer.config.width, renderer.config.height]);
                horizon_client::level_loading::draw_progress(
                    &mut hud,
                    &self.gui.atlas,
                    loader.progress.line(),
                    loader.progress.fraction(),
                );
                pr.lap("pr_loading");
                drop(prepare);
                return renderer.render(&loading_frame(&hud));
            }
            return Ok(false);
        }
        // A pack reload covers the frame with its progress
        // (`LoadingOverlay`) until `drive_pack_reload` applies it.
        if let Some(reload) = &self.pack_reload {
            if let Some(renderer) = &mut self.renderer {
                let mut hud = HudBatch::new([renderer.config.width, renderer.config.height]);
                horizon_client::level_loading::draw_loading_overlay(
                    &mut hud,
                    &self.gui.atlas,
                    reload.progress.line(),
                    reload.progress.fraction(),
                    self.settings.file.bool("darkMojangStudiosBackground"),
                );
                pr.lap("pr_loading");
                drop(prepare);
                return renderer.render(&loading_frame(&hud));
            }
            return Ok(false);
        }
        let Some(size) = self
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])
        else {
            return Ok(false);
        };
        pr.lap("pr_setup");
        let alpha = self.clock.fraction();
        // Camera.getCameraEntityPartialTicks: the view, the hand and the
        // player's own model stay smooth while the level is frozen.
        let camera_alpha = self.clock.camera_fraction();
        let vanilla = self.camera_view(camera_alpha as f32);
        let view = vanilla.map(|view| {
            if self.shows_panorama() || self.studio_freecam.active {
                view
            } else {
                self.native_camera_pose_now(camera_alpha as f32, view)
            }
        });
        // What faces the camera (particles, item sprites, block entity text) faces a mod's.
        // A spectated entity's view is not the local player's yaw and pitch
        // (`Camera.setup` reads the camera entity), so it is kept too.
        let spectating = self.camera_mob().is_some();
        self.posed_rotation = view
            .filter(|posed| spectating || Some(*posed) != vanilla)
            .map(|(_, yaw, pitch)| (yaw, pitch));
        let eye = view.map(|_| {
            (
                glam::DVec3::from_array(self.view.eye),
                self.view.eye_rotation,
            )
        });
        let (camera, underwater, view_yaw, view_pitch) = match view {
            Some((c, yaw, pitch)) => {
                let s =
                    self.world
                        .block(c.x.floor() as i32, c.y.floor() as i32, c.z.floor() as i32);
                (c, self.res.facts().get(s).fluid == Fluid::Water, yaw, pitch)
            }
            None => (
                glam::DVec3::new(0.5, 100.0, 0.5),
                false,
                self.yaw,
                self.pitch,
            ),
        };
        pr.lap("pr_camera");
        self.audio
            .set_listener([camera.x, camera.y, camera.z], view_yaw, view_pitch);
        pr.lap("pr_listener");
        let block = [
            camera.x.floor() as i32,
            camera.y.floor() as i32,
            camera.z.floor() as i32,
        ];
        // The camera inside an opaque block sees nothing through it: disable.
        let cave_culling = self.settings.horizon.cave_culling
            && !self
                .res
                .facts()
                .get(self.world.block(block[0], block[1], block[2]))
                .solid_render;
        // The cave culling search runs on its worker while the frame is prepared.
        if cave_culling
            && self.title.is_none()
            && let Some(renderer) = self.renderer.as_mut()
        {
            horizon_client::spans::time("occlusion", || {
                renderer.start_occlusion(camera, (self.streaming.radius * 16) as f32)
            });
        }
        pr.lap("pr_occlusion");
        let biome = self
            .world
            .chunks
            .get(&(block[0] >> 4, block[2] >> 4))
            .map(|c| c.biome(block[0], block[1], block[2]))
            .unwrap_or(0);
        let environment = self.view_environment();
        // EnvironmentAttributeProbe: the spatially interpolated biome colours,
        // Gaussian-weighted over the noise biomes around the camera (plains
        // where no chunk is loaded, as ClientLevel.getUncachedNoiseBiome).
        let biomes = world_biomes!(self);
        let weights = {
            let chunks = &self.world.chunks;
            let plains = biomes.id("minecraft:plains");
            horizon_client::biome::gaussian_weights(camera.to_array(), |qx, qy, qz| {
                chunks
                    .get(&(qx >> 2, qz >> 2))
                    .map_or(plains, |c| c.biome(qx * 4, qy * 4, qz * 4))
            })
        };
        // visual/water_fog_color: no dimension sets it, its default is #050533.
        let water_fog = horizon_client::biome::blend_rgb(&weights, |b| {
            horizon_client::biome::rgb(biomes.get(b).water_fog)
        });
        // The attribute stack, with the camera's biome mix in the probe's order; the biome
        // maps as the packs left them.
        let attributes_of = |b: u16| {
            let info = biomes.get(b);
            environment.biome(&info.name).unwrap_or(&info.attributes)
        };
        let mut blend = horizon_world::environment_system::BiomeBlend::default();
        for &(b, weight) in &weights {
            blend.accumulate(weight, attributes_of(b));
        }
        // The packs' biome post-process group, mixed the same way where they set one.
        let empty = horizon_world::environment_attributes::AttributeMap::default();
        let post_of = |b: u16| {
            environment
                .biome_post(&biomes.get(b).name)
                .unwrap_or(&empty)
        };
        let mut post_blend = horizon_world::environment_system::BiomeBlend::default();
        if environment.has_biome_post() {
            for &(b, weight) in &weights {
                post_blend.accumulate(weight, post_of(b));
            }
        }
        let (rain, thunder) = self.view_weather(alpha as f32);
        // What the packs' attribute expressions read (only a pack's stack has any).
        let mut polytone_frame = None;
        if self.pack.environments.is_some()
            || self.pack.polytone_particles.is_some()
            || self.pack.polytone_post.is_some()
        {
            let light = self.world.light(block[0], block[1], block[2]);
            let frame = horizon_client::environment::Frame {
                rain,
                thunder,
                game_time: self.world.game_time,
                day_time: self.view_day_time(),
                render_distance: self.streaming.radius,
                sky_type: match self.dim {
                    Dim::Overworld => 1,
                    Dim::End => 2,
                    Dim::Nether => 0,
                },
                dimension: match self.dim {
                    Dim::Overworld => "minecraft:overworld",
                    Dim::Nether => "minecraft:the_nether",
                    Dim::End => "minecraft:the_end",
                }
                .to_owned(),
                player: self.player.as_ref().map_or(camera.to_array(), |p| {
                    [p.position.x, p.position.y, p.position.z]
                }),
                player_velocity: self
                    .player
                    .as_ref()
                    .map_or([0.0; 3], |p| [p.velocity.x, p.velocity.y, p.velocity.z]),
                player_on_ground: self.player.as_ref().is_some_and(|p| p.ground.on_ground),
                in_water: underwater,
                camera: camera.to_array(),
                sky_light: i32::from(light >> 4),
                block_light: i32::from(light & 15),
                biome: biomes.get(biome).name.clone(),
                temperature: biomes.get(biome).temperature,
                downfall: biomes.get(biome).downfall,
            };
            if let Some(particles) = &self.pack.polytone_particles
                && let Ok(mut f) = particles.frame.write()
            {
                f.clone_from(&frame);
            }
            polytone_frame = Some(frame.clone());
            environment.set_frame(frame);
        }
        let mut sky = environment.sample(&horizon_client::environment::Sample {
            day_time: self.view_day_time(),
            rain,
            thunder,
            blend: Some(&blend),
            biome: Some(attributes_of(biome)),
            post_blend: Some(&post_blend),
            post_biome: Some(post_of(biome)),
        });
        self.end_flash_sky(&mut sky, alpha);
        if self.flash > 0 && !self.settings.file.bool("hideLightningFlashes") {
            // ClientLevel's flash layer while skyFlashTime > 0: the sky colour
            // moves 22% of the way to (0.8, 0.8, 1.0).
            let flash = [0.8, 0.8, 1.0];
            sky.sky_color =
                std::array::from_fn(|i| sky.sky_color[i] + (flash[i] - sky.sky_color[i]) * 0.22);
            sky.sky_light_factor = 1.0;
        }
        pr.lap("pr_environment");
        // FishingHookRenderer's line, then DebugRenderer: chunk borders and
        // hitboxes (F3+G, F3+B).
        let mut lines = self.fishing_line(alpha, size);
        let mut g = self.debug_gizmos(alpha as f32, camera.to_array());
        if self.studio_freecam.active || self.studio_replay.is_some() {
            horizon_client::studio::render_camera_path(
                &self.studio_freecam.track,
                &mut g,
                None,
                100,
            );
        }
        lines.extend(g.lines);
        let on_top = g.on_top;
        pr.lap("pr_debug");
        let colored_lights = if self.title.is_none() {
            self.native_frame_visuals(camera, alpha, &mut lines)
        } else {
            Vec::new()
        };
        let emissive_layers = if self.title.is_none() {
            let mut requests = Vec::new();
            self.native_emissive(&mut requests);
            // Native mods' overlays first, then the packs' OptiFine emissive blocks.
            horizon_client::native_emissive::table(
                &self.pack.render.sprite_layers,
                requests,
                &self.pack.render.emissive,
            )
        } else {
            Vec::new()
        };
        pr.lap("pr_native_visuals");
        let view = if self.studio_freecam.active {
            glam::Mat4::from_quat(self.camera_orientation().conjugate())
        } else {
            horizon_client::camera::view_matrix(view_yaw, view_pitch)
        };
        let world_primitives = if self.title.is_none() {
            self.world_primitive_geometry(camera, alpha, view)
        } else {
            Default::default()
        };
        pr.lap("pr_primitives");
        let at = camera.to_array();
        // GameRenderer.render3dHud: the debug crosshair, first person with the HUD shown.
        let mut crosshair = horizon_client::gizmos::Gizmos::default();
        if debug_crosshair_drawn(
            self.debug_crosshair(),
            self.camera_type.first_person(),
            self.ui.hide_gui,
            self.shows_panorama(),
        ) {
            let scale = HudBatch::new(size).scale;
            crosshair.debug_crosshair(at, view_pitch, view_yaw, scale);
        }
        let debug_lines = horizon_client::gizmos::camera_relative(&lines, at, view);
        let debug_lines_on_top = horizon_client::gizmos::camera_relative(&on_top, at, view);
        let debug_crosshair = horizon_client::gizmos::camera_relative(&crosshair.on_top, at, view);
        // LevelExtractor.extractBlockOutline: model translucency controls draw order;
        // no outline when an entity is targeted (Minecraft.hitResult an EntityHitResult).
        let mut outline_after_terrain = false;
        // GameRenderer.shouldRenderBlockOutline: not with the HUD hidden (F1), and only for a
        // player who may build. Adventure players only get one with a CAN_BREAK or
        // CAN_PLACE_ON item matching the block, and spectators with a menu block; Horizon
        // does not open menus for spectators, so those never show it. An open
        // screen does not hide it: vanilla checks the HUD and the game mode, not the screen.
        let adventure = self.mode == horizon_client::command::args::GameMode::Adventure;
        let can_build = self.may_build() || adventure;
        let outline = if block_outline_drawn(
            self.ui.hide_gui,
            self.entity_targeted(),
            can_build,
            self.shows_panorama(),
        ) {
            self.pick()
                .filter(|h| {
                    self.may_build()
                        || !self.block_action_restricted(h.pos)
                        || self.can_place_on_in_adventure(h.pos)
                })
                .filter(|h| {
                    let state = self.world.block(h.pos[0], h.pos[1], h.pos[2]);
                    let mut canceled = false;
                    self.native_block_highlight(h.pos, state, alpha as f32, &mut canceled);
                    !canceled
                })
                .map(|h| {
                    let state = self.world.block(h.pos[0], h.pos[1], h.pos[2]);
                    let f = self.res.facts().get(state);
                    outline_after_terrain = self
                        .pack
                        .render
                        .model_catalog
                        .collect(state, BlockPos::new(h.pos[0], h.pos[1], h.pos[2]))
                        .is_ok_and(|models| {
                            models
                                .iter()
                                .any(|(_, model)| model.faces.iter().any(|f| f.translucent))
                        });
                    let o = if f.outline_offset {
                        self.pack
                            .context
                            .tables
                            .offset(state, h.pos[0], h.pos[2])
                            .unwrap_or([0.0; 3])
                    } else {
                        [0.0; 3]
                    };
                    let p = h.pos.map(f64::from);
                    if let Some(edges) = self
                        .collision
                        .cached_shapes()
                        .context_outline_edges(state, self.outline_context_kind())
                    {
                        return edges
                            .iter()
                            .map(|e| std::array::from_fn(|i| p[i % 3] + o[i % 3] + e[i]))
                            .collect();
                    }
                    if let Some(b) = self.context_outline(h.pos, state) {
                        return horizon_client::blocks::cube_edges()
                            .iter()
                            .map(|e| {
                                std::array::from_fn(|i| {
                                    let k = i % 3;
                                    p[k] + b[k] + e[i] * (b[k + 3] - b[k])
                                })
                            })
                            .collect();
                    }
                    f.outline_edges
                        .iter()
                        .map(|e| std::array::from_fn(|i| p[i % 3] + o[i % 3] + e[i]))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        pr.lap("pr_outline");
        let title = self.shows_panorama();
        let (yaw, pitch) = if title {
            // PanoramaRenderer: the title cube spins at 0.1 degrees per tick, pitched 10 degrees.
            (
                self.panorama_rotation.update(
                    self.started.elapsed().as_secs_f32(),
                    self.settings.file.double("panoramaScrollSpeed") as f32,
                ),
                10.0,
            )
        } else {
            (view_yaw, view_pitch)
        };
        let fov_degrees = if title {
            85.0
        } else {
            self.fov_degrees(camera_alpha as f32, camera.to_array())
        };
        let camera_effects = fov::camera_effects_enabled(title, self.studio_freecam.active);
        let bob = if !camera_effects {
            glam::Mat4::IDENTITY
        } else {
            self.bob_matrix(camera_alpha as f32)
        };
        let spin = if !camera_effects {
            glam::Mat4::IDENTITY
        } else {
            self.spinning_matrix(camera_alpha as f32)
        };
        let terrain_distance = self.terrain_distance();
        let distance = (terrain_distance * 16) as f32;
        let depth_far =
            horizon_client::camera::depth_far(terrain_distance, self.settings.cloud_range);
        // Only native mods read it: no matrices or allocation per frame without one.
        let mods_read_camera = !horizon_client::mod_api::mods().is_empty();
        self.screen_blur_requested.set(false);
        self.native_camera_frame = (mods_read_camera && !title).then(|| {
            let partial = camera_alpha as f32;
            horizon_client::mod_api::NativeCamera {
                position: camera.to_array(),
                eye_height: self.view.eye_height_old
                    + (self.view.eye_height - self.view.eye_height_old) * partial,
                partial_tick: partial,
                yaw,
                pitch,
                fov_degrees,
                near: 0.05,
                far: depth_far,
                view: (bob * spin * horizon_client::camera::view_matrix(yaw, pitch))
                    .to_cols_array(),
                projection: glam::Mat4::perspective_rh(
                    fov_degrees.to_radians(),
                    size[0] as f32 / size[1].max(1) as f32,
                    depth_far,
                    0.05,
                )
                .to_cols_array(),
                dimension: self.dim.id().to_string(),
            }
        });
        pr.lap("pr_projection");
        // GUI hooks project against this frame's camera, including its bob/spin and FOV.
        let mut hud = horizon_client::spans::time("hud", || self.hud(size));
        self.flush_ime(&mut hud);
        self.native_gui_render_post(&mut hud, self.ui.hide_gui, self.focused);
        self.resolve_inline_profiles(&hud.inline_profiles);
        pr.lap("pr_hud");
        self.locator_camera =
            (!title).then_some((camera.to_array(), view_yaw, view_pitch, fov_degrees));
        let mut requested_post_effects = self.requested_post_effects();
        // The packs' Polytone post chains, after vanilla's (#11486).
        let game_ticks = self.world.game_time as f64 + alpha;
        let delta_ticks = (game_ticks - self.polytone_post_ticks).clamp(0.0, 20.0) as f32;
        self.polytone_post_ticks = game_ticks;
        let polytone_extras = match (&self.pack.polytone_post, &polytone_frame) {
            (Some(post), Some(frame)) => {
                let view = bob * spin * horizon_client::camera::view_matrix(yaw, pitch);
                let globals = horizon_client::polytone_post::PolyGlobals {
                    // 26.3's reverse-Z projection, as the world's (and its depth) is.
                    projection: glam::Mat4::perspective_rh(
                        fov_degrees.to_radians(),
                        size[0] as f32 / size[1].max(1) as f32,
                        depth_far,
                        0.05,
                    ),
                    model_view: view,
                    sun_angle: sky.sun_angle.to_radians() - std::f32::consts::FRAC_PI_2,
                    day_time: self.view_day_time().rem_euclid(24000) as f32,
                    delta_time: delta_ticks,
                    player: self.player.as_ref().map_or(camera.to_array(), |p| {
                        [p.position.x, p.position.y, p.position.z]
                    }),
                };
                let (effects, extras) = post.frame(frame, &globals);
                requested_post_effects.extend(effects);
                Some(extras)
            }
            _ => None,
        };
        self.view_frustum = self
            .renderer
            .as_ref()
            .map(|r| r.camera_frustum(fov_degrees, yaw, pitch, bob * spin, depth_far));
        let billboard_frustum = if self.settings.horizon.taa {
            self.renderer.as_ref().map(|r| {
                r.camera_frustum_with_jitter(fov_degrees, yaw, pitch, bob * spin, depth_far)
            })
        } else {
            self.view_frustum
        };
        // Keep the buffer local while builders need mutable access to the game.
        let mut entities = std::mem::take(&mut self.entity_vertices);
        entities.clear();
        let mut static_block_entities = std::sync::Arc::from([]);
        let mut visible_static_block_entity_sections = Vec::new();
        let mut static_block_entity_stats = StaticCacheFrameStats::default();
        self.see_through_vertices.clear();
        self.glowing_vertices.get_mut().clear();
        self.shadow_only_vertices.get_mut().clear();
        pr.lap("pr_entity_setup");
        if !title {
            let (built, static_sections, visible_static_sections, static_stats) =
                spans::time("entity_geometry", || {
                    self.entity_geometry(camera, alpha, fov_degrees, billboard_frustum, entities)
                });
            entities = built;
            static_block_entities = static_sections;
            visible_static_block_entity_sections = visible_static_sections;
            static_block_entity_stats = static_stats;
        } else {
            // No level behind the panorama, so no entity either: the GPU path's
            // draws are started by `begin_skinning`, which only `entity_geometry`
            // runs, and would otherwise stay those of the last world frame and
            // be drawn over the title (#8192). Its models stay stored.
            self.skin_frame.get_mut().clear_draws();
        }
        pr.lap("pr_entities");
        let bolts = if title {
            Vec::new()
        } else {
            self.bolt_instances(camera)
        };
        // Particles blended additively (resource packs' Polytone ones, #11486), from the
        // render that built `entities`.
        let mut additive = self.particles.take_additive();
        // The dragon is drawn in every dimension, as any entity: a dragon
        // summoned outside the End (the server's AddEntity) is shown too.
        if !title {
            let mut dragon = Vec::new();
            self.dragon_geometry(camera, alpha, &mut dragon, &mut additive);
            self.dragon_fireball_geometry(camera, alpha, &mut dragon);
            entities.extend(dragon);
        }
        pr.lap("pr_dragon_bolts");
        let (celestial, stars) = if title {
            (Vec::new(), None)
        } else if self.dim == Dim::Overworld
            && self.potions.effects.get("darkness").is_none()
            && self.potions.effects.get("blindness").is_none()
        {
            // Camera.doesMobEffectBlockSky: no sun, moon or stars.
            self.celestial_geometry(&sky, rain)
        } else if self.dim == Dim::End && self.settings.horizon.sky {
            (self.end_flash_geometry(), None)
        } else {
            (Vec::new(), None)
        };
        self.custom_sky_frame([camera.x, camera.y, camera.z], &sky, (rain, thunder), title);
        pr.lap("pr_celestial");
        let (mut hand, hand_items) = if title || self.scoping() {
            Default::default()
        } else {
            self.hand_geometry(camera_alpha)
        };
        // GameRenderer.render3dHud: the screen effects after the hand, in
        // its pass (the hand pass draws with `bob * view`).
        if !title {
            let view = bob * horizon_client::camera::view_matrix(yaw, pitch);
            let aspect = size[0] as f32 / size[1].max(1) as f32;
            self.item_activation_geometry(camera_alpha as f32, view, aspect, &mut hand);
        }
        pr.lap("pr_hand");
        let screen = if title {
            Vec::new()
        } else {
            let scale = self
                .player
                .as_mut()
                .and_then(|p| p.attribute("minecraft:scale").ok())
                .unwrap_or(1.0) as f32;
            self.screen_quads(alpha as f32, size, scale)
        };
        if !title {
            self.end_geometry(camera, &mut entities);
        }
        self.outline_fill(&outline, camera, &mut entities);
        pr.lap("pr_screen_end");
        let weather = if !title && self.can_have_weather() && self.settings.horizon.weather {
            horizon_client::spans::time("weather", || self.weather_geometry(camera, alpha))
        } else {
            Vec::new()
        };
        let border = (!title)
            .then(|| self.border_draw(camera.to_array(), alpha as f32))
            .flatten();
        let see_through = std::mem::take(&mut self.see_through_vertices);
        pr.lap("pr_weather_border");
        if title {
            self.dynamic_lights.clear();
        } else {
            self.gather_dynamic_lights(camera, alpha);
        }
        pr.lap("pr_lights");
        let mut pack_inputs = if title {
            Default::default()
        } else {
            self.shader_pack_inputs(camera, alpha as f32, &sky)
        };
        pr.lap("pr_shader_inputs");
        pack_inputs.frame_timing = self
            .screenshot_sequence
            .as_ref()
            .map(screenshot_sequence::Sequence::timing);
        // Map textures and font glyphs changed while building this frame's
        // geometry (game/maps.rs, world text) upload with this batch; draining
        // map_patches any earlier leaves them a frame behind.
        hud.patches.append(&mut self.map_patches);
        let entity_spans = std::mem::take(self.entity_spans.get_mut());
        let entity_motion = std::mem::take(self.entity_motion.get_mut());
        let entity_motion_posed = std::mem::take(self.entity_motion_posed.get_mut());
        let glowing = std::mem::take(self.glowing_vertices.get_mut());
        let shadow_only = std::mem::take(self.shadow_only_vertices.get_mut());
        let skin = std::mem::take(self.skin_frame.get_mut());
        let frame = Frame {
            weather: &weather,
            weather_time: horizon_client::weather::Time::new(self.world.game_time, alpha as f32),
            nether_lighting: self.dim == Dim::Nether,
            camera,
            yaw,
            pitch,
            fov_degrees,
            eye: eye.filter(|_| !title).map_or(camera, |e| e.0),
            eye_rotation: eye.filter(|_| !title).map_or([yaw, pitch], |e| e.1),
            first_person: eye.is_none() || title || !self.view.detached,
            bob,
            spin,
            sky,
            shadow_scene_revision: (self.world.revision, self.world.chunks.membership_revision()),
            shadow_cache: self.settings.horizon.shadow_cache,
            distance,
            detail_distance: (self.streaming.radius * 16) as f32,
            depth_far,
            underwater,
            water_plane: if title
                || underwater
                || !self
                    .renderer
                    .as_ref()
                    .is_some_and(|r| r.wants_water_plane())
            {
                None
            } else {
                self.water_plane(camera)
            },
            title,
            sky_blocked: horizon_client::fog::sky_pass_skipped(
                horizon_client::fog::fog_type(
                    &self.world,
                    self.res.facts(),
                    [camera.x, camera.y, camera.z],
                    &self.near_plane(),
                ),
                self.potions.effects.get("blindness").is_some()
                    || self.potions.effects.get("darkness").is_some(),
                self.boss_overlay.should_create_world_fog(),
            ),
            skybox: self.dim == Dim::Overworld && self.settings.horizon.sky,
            end_sky: self.dim == Dim::End && self.settings.horizon.sky,
            end_sky_sprite: self
                .gui
                .atlas
                .sprites
                .get("environment/end_sky")
                .map(|r| [r.u0, r.v0, r.u1, r.v1]),
            boss_world_fog: self.boss_overlay.should_create_world_fog(),
            environment_fog: (self.dim == Dim::Nether).then_some((sky.fog_start, sky.fog_end)),
            cave_culling,
            water_fog,
            fog: Some(self.frame_fog(camera, &sky, biome, water_fog)),
            dark_disc: self.dark_disc(camera),
            outline,
            outline_after_terrain,
            outline_high_contrast: self.settings.file.bool("highContrastBlockOutline"),
            pvp: self.settings.extras.pvp.frame(),
            debug_lines: &debug_lines,
            debug_lines_on_top: &debug_lines_on_top,
            debug_crosshair: &debug_crosshair,
            hud: &hud,
            glint_alpha: self.glint_frame.alpha,
            glint_ticks: self.glint_frame.scroll_ticks(),
            clouds: (!title
                && self.dim == Dim::Overworld
                && horizon_client::clouds::visible(&self.settings.clouds, sky.cloud_color[3]))
            .then(|| self.gui.atlas.sprites.get("horizon/white"))
            .flatten()
            .map(|white| horizon_client::clouds::Input {
                texture: &self.pack.clouds,
                uv: [(white.u0 + white.u1) / 2.0, (white.v0 + white.v1) / 2.0],
                camera,
                time: self.world.game_time,
                partial_tick: alpha,
                range: self.settings.cloud_range,
                fast: self.settings.clouds == "fast",
                color: sky.cloud_color,
            }),
            entities: &entities,
            static_block_entities: &static_block_entities,
            visible_static_block_entity_sections: &visible_static_block_entity_sections,
            static_block_entity_stats,
            entity_motion: &entity_motion,
            entity_motion_posed: &entity_motion_posed,
            entity_spans: &entity_spans,
            skin: skin.pending().then_some(&skin),
            glowing: &glowing,
            dynamic_lights: &self.dynamic_lights,
            colored_lights: &colored_lights,
            emissive_layers: &emissive_layers,
            lightning: horizon_client::gfx::lightning::Input {
                bolts: &bolts,
                uv: horizon_client::gfx::lightning::white_uv(&self.gui.atlas.font.white()),
            },
            additive: &additive,
            see_through: &see_through,
            world_primitives: world_primitives.batch(),
            shadow_only: &shadow_only,
            celestial: if self.settings.horizon.sky || title {
                &celestial
            } else {
                &[]
            },
            stars: stars.filter(|_| self.settings.horizon.sky || title),
            hand: &hand,
            hand_items: &hand_items,
            hand_fov_degrees: self.hud_fov_degrees(camera_alpha as f32, camera.to_array()),
            screen: &screen,
            pack: pack_inputs,
            border,
            game_time: self.world.game_time as f32,
            shader_game_time: ((self.world.game_time % 24000) as f32 + alpha as f32) / 24000.0,
            crumbling_sprite: self
                .gui
                .atlas
                .sprites
                .get("block/destroy_stage_0")
                .map_or([0.0; 4], |r| [r.u0, r.v0, r.u1 - r.u0, r.v1 - r.v0]),
            end_portal_sprites: ["environment/end_sky", "entity/end_portal/end_portal"].map(|n| {
                self.gui
                    .atlas
                    .sprites
                    .get(n)
                    .map_or([0.0; 4], |r| [r.u0, r.v0, r.u1 - r.u0, r.v1 - r.v0])
            }),
            ticks: self.texture_clock(),
            darkness: self.darkness_lighting(alpha as f32),
            night_vision: self.night_vision_intensity(alpha as f32),
            block_flicker: self.fog.block_light_flicker,
            boss_darken: self
                .boss_overlay
                .darken_amount(self.clock.fraction() as f32),
            menu_blur: self.menu_blur(),
            post_effects: horizon_client::post_chain::Request {
                effects: &requested_post_effects,
                root: &self.options.resources,
                extras: polytone_extras.as_ref(),
                menu_blur_radius: self.settings.menu_blur as f32,
                // GlobalSettingsUniform.update.
                globals: horizon_client::post_chain::Globals {
                    camera_block_pos: [
                        camera.x.floor() as i32,
                        camera.y.floor() as i32,
                        camera.z.floor() as i32,
                    ],
                    camera_offset: [
                        (camera.x.floor() - camera.x) as f32,
                        (camera.y.floor() - camera.y) as f32,
                        (camera.z.floor() - camera.z) as f32,
                    ],
                    game_time: ((self.world.game_time % 24000) as f32 + alpha as f32) / 24000.0,
                    ..Default::default()
                },
            },
            chunk_fade: self.settings.chunk_fade,
        };
        pr.lap("pr_frame");
        // Renderer encoding and GPU waiting belong to render, not prepare.
        drop(prepare);
        let debug_passes = self.debug_lists_gpu_passes();
        let result = match self.renderer.as_mut() {
            Some(renderer) => {
                renderer.set_pass_timing(debug_passes || self.gpu_passes);
                // `gpu_passes` is a measurement: it takes every sample it can.
                renderer.set_pass_timing_gap(if self.gpu_passes {
                    Duration::ZERO
                } else {
                    horizon_client::debug_overlay::GPU_TIMING_GAP
                });
                horizon_client::spans::time("renderer", || renderer.render(&frame))
            }
            None => Err("renderer".into()),
        };
        let _cleanup = spans::guard("pr_cleanup");
        // A frame the surface refused (occluded, lost, timed out) is `Ok(false)`.
        self.frame_drawn = matches!(result, Ok(true));
        // Restore storage even when rendering fails (for example, a lost surface).
        self.entity_vertices = entities;
        *self.entity_spans.get_mut() = entity_spans;
        *self.entity_motion.get_mut() = entity_motion;
        *self.entity_motion_posed.get_mut() = entity_motion_posed;
        *self.glowing_vertices.get_mut() = glowing;
        *self.shadow_only_vertices.get_mut() = shadow_only;
        let mut skin = skin;
        // The renderer took the new models when its store grew by them; otherwise they stay
        // pending, or the stores restart (`SkinModels::begin`).
        if self
            .renderer
            .as_ref()
            .and_then(|r| r.skinned_models())
            .is_some_and(|stored| stored == skin.base + skin.new_model.len() as u32)
        {
            skin.handed();
        }
        *self.skin_frame.get_mut() = skin;
        self.see_through_vertices = see_through;
        result
    }
}
/// `computeNextAutosaveInterval`'s arithmetic: `Math.max(100, (int)(rate * 300.0F))`.
fn autosave_interval(rate: f32) -> i64 {
    i64::from((rate * 300.0) as i32).max(100)
}

/// winit's events for a window's game, with any loop: a headless client (#4951) has no
/// winit loop and calls these from its own.
impl Game<'_> {
    /// One window event, in the order vanilla's `KeyboardHandler` and `MouseHandler` see it.
    ///
    /// A controller doesn't come through here in play: `pad_action` goes straight to the mappings
    /// (`mapped_key`, `mouse_mapping`). So a key handled below before `mapped_key_event` (a window
    /// shortcut, a special case) never reaches a controller. If a player could want it on one, give
    /// it a handler both can call and a `gamepad::Route` (screenshot and fullscreen do this). A
    /// screen's own raw keys, text and mods' key hooks stay raw on purpose.
    fn handle_window_event_inner(&mut self, event_loop: &dyn EventLoopCtl, mut event: WindowEvent) {
        if let WindowEvent::Focused(false) = &event {
            self.physical_keys.clear();
            self.physical_buttons.clear();
            self.drag_button = None;
        }
        if let WindowEvent::ModifiersChanged(m) = &event {
            self.modifiers = m.state();
            horizon_client_input::modifiers::reconcile(
                self.modifiers,
                &mut self.physical_keys,
                &self.settings.keys,
                &mut self.toggles,
                [self.settings.toggle_crouch, self.settings.toggle_sprint],
                [
                    self.pad
                        .holds_action(&self.settings.controller, "key.sneak"),
                    self.pad
                        .holds_action(&self.settings.controller, "key.sprint"),
                ],
            );
            self.sync_toggle(SNEAK);
            self.sync_toggle(SPRINT);
        }
        if let WindowEvent::DroppedFile(path) = &event {
            self.drop_pack_file(path);
        }
        // GLFW never hands a field the keys an input method consumes: while a
        // composition is open, and the Enter or Escape that ends it (winit on
        // Windows still sends that one right after `Ime::Commit`).
        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &event
            && key_event.state == ElementState::Pressed
            && self.text_field_focused()
            && horizon_client::preedit::should_suppress_key()
        {
            return;
        }
        // A character that comes with no key (Windows' Alt codes, `Alt` + digits on the numeric
        // keypad: a `WM_CHAR` after the Alt is released) reaches winit as a key event with no
        // physical key. GLFW's char callback types it in the focused text field (`charTyped`),
        // and every screen's key handler here would drop it for want of a key code.
        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &event
            && key_event.state == ElementState::Pressed
            && matches!(key_event.physical_key, PhysicalKey::Unidentified(_))
            && let Some(text) = key_event.text.as_deref()
        {
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            if !text.is_empty() && self.text_field_focused() && self.types_text() {
                let _ = self.type_chars_into_screen(&text);
            }
            return;
        }
        // Fullscreen is a window shortcut, before screens consume keyboard input.
        // Not Alt or Super on their own: leaving the window isn't playing with the keyboard.
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && event.state == ElementState::Pressed
            && !event.repeat
            && !matches!(
                event.physical_key,
                PhysicalKey::Code(
                    KeyCode::AltLeft | KeyCode::AltRight | KeyCode::SuperLeft | KeyCode::SuperRight
                )
            )
        {
            self.pad.keyboard_took_over = true;
        }
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && event.state == ElementState::Pressed
            && let PhysicalKey::Code(key) = event.physical_key
            && horizon_client::keybinds::key_name(key)
                == Some(self.settings.keys.get("key.fullscreen"))
            && !(self.screen == Screen::Options && self.options_waiting_for_key())
        {
            self.last_input = Instant::now();
            if !event.repeat {
                self.toggle_fullscreen();
            }
            return;
        }
        // A screenshot is a window shortcut too: vanilla takes it before a
        // screen or a text box consumes the key (`handleGlobalKeyPress`), so
        // it works with chat, a sign or a book open.
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && event.state == ElementState::Pressed
            && let PhysicalKey::Code(key) = event.physical_key
        {
            self.last_input = Instant::now();
            if self.screenshot_shortcut(key, event.repeat) {
                return;
            }
        }
        // KeyboardHandler.keyPress: the debug keys come before the game's.
        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &event
            && let PhysicalKey::Code(key) = key_event.physical_key
        {
            use horizon_client::debug_keys::KeyAction;
            let action = match (key_event.state, key_event.repeat) {
                (ElementState::Released, _) => KeyAction::Release,
                (_, false) => KeyAction::Press,
                (_, true) => KeyAction::Repeat,
            };
            if self.debug_key(key, action) {
                return;
            }
        }
        // KeyboardHandler.keyPress: the narrator hotkey, on every screen and
        // in game, before the screen takes the key.
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            self.narrator_hotkey(event);
        }
        if matches!(
            event,
            WindowEvent::KeyboardInput { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. }
                | WindowEvent::CursorMoved { .. }
        ) {
            self.last_input = Instant::now();
        }
        if !self.pad_synthetic_input()
            && matches!(
                event,
                WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
            )
        {
            self.reassert_pointer_capture();
        }
        // The real mouse takes the pointer back from the keyboard's focus.
        self.key_focus_mouse(event_loop, &event);
        // The screen's narration hears of the input before the screen takes
        // it; the pointer events the keyboard's focus sends aren't the mouse.
        if !self.key_focus_quiet() {
            narrator::narrate_input(&mut self.narration, &event, self.cursor);
        }
        // FileFixerProgressScreen: Cancel is the only way out (`shouldCloseOnEsc`
        // is false), so it takes the pointer and Escape does nothing.
        if let Some(starting) = &self.integrated_starting
            && starting.handle.upgrade().is_file_fixing()
        {
            let upgrade = starting.handle.upgrade();
            match &event {
                WindowEvent::CursorMoved { position, .. } => {
                    if let Some(r) = &self.renderer {
                        let b = HudBatch::new([r.config.width, r.config.height]);
                        self.cursor = (position.x as f32 / b.scale, position.y as f32 / b.scale);
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } if !upgrade.is_canceled() => {
                    let (w, h) = self.gui_size();
                    let (x, y) = horizon_client::level_loading::file_fixer_cancel(w, h);
                    if horizon_client::ui::hovered(self.cursor, x, y, 200.0, 20.0) {
                        self.audio.play_button_click();
                        upgrade.set_canceled();
                    }
                }
                _ => {}
            }
            if matches!(&event, WindowEvent::KeyboardInput { .. }) {
                return;
            }
        }
        if self.pending_entry.is_some()
            && matches!(&event, WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.physical_key == PhysicalKey::Code(KeyCode::Escape))
        {
            self.pending_entry = None;
            self.perf.cancel_world_opening();
            return;
        }
        if (self.pending_entry.is_some() || self.integrated_starting.is_some())
            && !matches!(
                &event,
                WindowEvent::CloseRequested
                    | WindowEvent::Resized(_)
                    | WindowEvent::RedrawRequested
            )
        {
            return;
        }
        // The pack reload's progress overlay takes no input
        // (`LoadingOverlay`): `spawn_pack_reload` released what was held.
        if self.pack_reload.is_some()
            && !matches!(
                &event,
                WindowEvent::CloseRequested
                    | WindowEvent::Focused(_)
                    | WindowEvent::Resized(_)
                    | WindowEvent::RedrawRequested
            )
        {
            return;
        }
        if self.pending_save.is_some() {
            match event {
                WindowEvent::CloseRequested => {
                    if !self.settings.quit_shortcuts
                        && horizon_client::input::is_quit_shortcut_down(
                            self.modifiers,
                            &self.physical_keys,
                        )
                    {
                        return;
                    }
                    self.pending_save.as_mut().unwrap().quit = true;
                }
                WindowEvent::Focused(focused) => {
                    self.focused = focused;
                    if !focused {
                        self.capture(false);
                    }
                }
                WindowEvent::Resized(size) => {
                    if let Some(r) = &mut self.renderer {
                        r.resize(size.width, size.height);
                    }
                }
                WindowEvent::RedrawRequested => {
                    if let Err(error) = self.render_closing_save() {
                        self.error = Some(error);
                        self.exit_on_error(event_loop);
                    }
                }
                _ => {}
            }
            return;
        }
        // ScreenEvent.KeyPressed.Pre: the open screen hears of the key before
        // it acts on it, and a cancelled press goes no further — no focus
        // move, no Escape-close, no inventory or hotbar key, no typing by the
        // key itself, no `native_key`. It fires after the window's own
        // shortcuts (fullscreen, screenshot, the debug keys, the narrator) so
        // those keep working with a mod that cancels; the key's release still
        // comes through.
        //
        // A key's characters are a separate callback of GLFW's, and it comes
        // after the key callback (`KeyboardHandler.charTyped`): they are taken
        // off the key here and dispatched once the screen has acted on it, on
        // the screen that is open then (`ScreenEvent.CharacterTyped.Pre`).
        // Options keeps its text on the key: its own key path routes a key's
        // text between its boxes and its key-bind capture, and no key that
        // carries text closes that screen or moves its focus. The title's
        // screens keep theirs for the same reason: their key path types into
        // the world name, the seed and a server's address.
        let mut typed: Option<String> = None;
        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &mut event
            && key_event.state == ElementState::Pressed
        {
            if self.screen_hooks_open()
                && self.screen != Screen::Options
                && !control::title_owns_text(&self.screen, self.title.is_some())
            {
                typed = key_event.text.take().map(|text| text.to_string());
            }
            let canceled = match key_event.physical_key {
                PhysicalKey::Code(key) => self.screen_key_pressed(key, key_event.repeat),
                PhysicalKey::Unidentified(_) => false,
            };
            if self.screen == Screen::Options && self.screen_hooks_open() {
                // The hook after the key's own, then the text the key path
                // types in place (its own routing keeps it).
                if let Some(text) = key_event.text.as_deref() {
                    let left = self.screen_text_typed(text);
                    if left.as_deref() != Some(text) {
                        key_event.text = left.map(Into::into);
                    }
                }
            }
            // Title boxes keep their text on the key path. A field hook takes
            // it through the same per-character editor as IME and control input.
            if !canceled
                && control::title_owns_text(&self.screen, self.title.is_some())
                && self.types_text()
                && let Some(text) = key_event.text.as_deref()
                && let Some(result) = self.type_text_field_input(text)
            {
                if let Err(error) = result {
                    log::warn!("Horizon: text input: {error}");
                }
                key_event.text = None;
            }
            if canceled {
                // charTyped is a separate callback: the characters still
                // reach the text box, without the key's own effects. Options'
                // characters already passed the hook in place.
                if let Some(text) = typed.as_deref() {
                    self.screen_chars_typed(Some(text));
                } else if self.screen == Screen::Options
                    && let Some(text) = key_event.text.as_deref()
                    && self.types_text()
                {
                    let text: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !text.is_empty() {
                        // Err only says no box of the screen takes it.
                        let _ = self.type_chars_into_screen(&text);
                    }
                }
                return;
            }
        }
        // Screen.keyPressed: Tab, the arrows and Enter move and press the
        // menu's focus, unless the focused widget or the screen keeps them.
        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &event
            && key_event.state == ElementState::Pressed
            && let PhysicalKey::Code(key) = key_event.physical_key
            && self.key_focus_key(event_loop, key)
        {
            // The keyboard's focus took the key; its characters are a separate
            // callback and still come (`KeyboardHandler.charTyped`).
            self.screen_chars_typed(typed.as_deref());
            return;
        }
        let result: Result<()> = (|| {
            if let WindowEvent::MouseInput { state, button, .. } = &event
                && !self.pad_synthetic_input()
            {
                self.track_drag_button(*button, *state == ElementState::Pressed);
            }
            // The emote key let go over the wheel, a click on it.
            if self.screen == Screen::EmoteWheel && self.emote_wheel_event(&event) {
                return Ok(());
            }
            if let WindowEvent::KeyboardInput { event, .. } = &event
                && event.state == ElementState::Released
                && let PhysicalKey::Code(key) = event.physical_key
                && self.screen_key_released(key)
            {
                // Screen cancellation must not leave a held keyboard mapping down.
                self.keys.remove(&key);
                self.mapped_key_event(key, false, false)?;
                return Ok(());
            }
            // ScreenEvent.MouseButtonPressed.Pre / MouseButtonReleased.Pre:
            // the open screen hears of the press before anything reads it, and
            // a cancelled one goes no further — no hotbar key, no slot, no
            // button, no sort, no drag. First in the event, so none of the
            // arms below (which hold the container hotkey and the sort and
            // header buttons too) see it. A click the pad or the keyboard's
            // focus made up is not the mouse: it fires nothing.
            if let WindowEvent::MouseInput { state, button, .. } = &event
                && !self.pad_synthetic_input()
                && let Some(screen) = self.screen_view_now()
            {
                let (button, modifiers) = (glfw_button(*button), glfw_modifiers(self.modifiers));
                let mut canceled = false;
                if *state == ElementState::Pressed {
                    self.native_screen_mouse_button_pressed_pre(
                        &screen,
                        button,
                        modifiers,
                        &mut canceled,
                    );
                } else {
                    self.native_screen_mouse_button_released_pre(
                        &screen,
                        button,
                        modifiers,
                        &mut canceled,
                    );
                }
                if canceled {
                    return Ok(());
                }
            }
            self.key_focus_mouse_press(&event);
            match event {
                WindowEvent::CloseRequested => {
                    if !self.settings.quit_shortcuts
                        && horizon_client::input::is_quit_shortcut_down(
                            self.modifiers,
                            &self.physical_keys,
                        )
                    {
                        return Ok(());
                    }
                    self.agent_cancel("the game window was closed; the game saves and exits");
                    self.begin_closing_save(true)?;
                }
                // Input method composition: the committed text types like a key's text.
                WindowEvent::Ime(winit::event::Ime::Commit(text)) => self.ime_commit(&text),
                WindowEvent::Ime(winit::event::Ime::Preedit(text, cursor)) => {
                    self.ime_preedit(&text, cursor)
                }
                WindowEvent::Ime(winit::event::Ime::Disabled) => {
                    horizon_client::preedit::set_preedit(None)
                }
                WindowEvent::Resized(size) => {
                    if let Some(r) = &mut self.renderer {
                        r.resize(size.width, size.height);
                    }
                    self.observe_fullscreen();
                }
                WindowEvent::Focused(focused) => {
                    self.focused = focused;
                    self.focus_lost = (!focused).then(Instant::now);
                    if focused {
                        if std::mem::take(&mut self.unclaimed_window) {
                            // A person clicked into the agent's window: from now on it is a
                            // window like any other, and their click takes the pointer
                            // (`MouseHandler.onPress`) instead of attacking.
                            self.captured = false;
                        }
                        self.regrab_on_refocus();
                    } else {
                        // A held pointer goes with the focus, wherever the
                        // system keeps the grab: the window asks for it back
                        // when the focus returns (`regrab_on_refocus`).
                        let held = self.captured;
                        if self.control.is_none() {
                            self.keys.clear();
                            // Latched sneak/sprint live in `toggles`, not in the
                            // physical keys: put them back so a short alt-tab
                            // (no pause screen) does not drop them. Only a mapping
                            // whose toggle option is on is latched; a held one has
                            // no state to keep, and an unfocused window never gets
                            // its key release, so leaving it down would keep the
                            // player sneaking or sprinting until the key is tapped.
                            for (slot, needs_toggle) in [
                                self.settings.toggle_crouch,
                                self.settings.toggle_sprint,
                                self.settings.toggle_attack,
                                self.settings.toggle_use,
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                self.toggles.0[slot].set_down(false, needs_toggle);
                            }
                            self.sync_toggle(0);
                            self.sync_toggle(1);
                            self.left = false;
                            self.right = false;
                            self.capture(false);
                        }
                        self.regrab_on_refocus = held;
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    if let Some(r) = &self.renderer {
                        let b = HudBatch::new([r.config.width, r.config.height]);
                        let previous = self.cursor;
                        self.cursor = (position.x as f32 / b.scale, position.y as f32 / b.scale);
                        if self.screen == Screen::Playing
                            && (self.studio_timeline.dragging_playhead
                                || self.studio_timeline.dragging_keyframe.is_some())
                        {
                            let action = self.studio_timeline.handle_mouse_drag(
                                self.cursor,
                                b.width,
                                &mut self.studio_freecam,
                            );
                            if matches!(
                                action,
                                horizon_client::studio::TimelineAction::KeyframeMoved
                            ) {
                                self.studio_camera_track_dirty = true;
                            }
                            return Ok(());
                        }
                        if self.screen_mouse_dragged(previous) {
                            return Ok(());
                        }
                        if self.dialog_open() {
                            self.dialog_drag();
                        }
                        if self.screen == Screen::BlockEditor {
                            self.block_editor_drag();
                        }
                        if self.screen == Screen::Advancements {
                            self.advancements_drag();
                        }
                        if self.screen == Screen::Stats {
                            self.stats_drag();
                        }
                        if self.screen == Screen::DebugOptions {
                            self.debug_options_drag();
                        }
                        if self.screen == Screen::HudEditor {
                            self.hud_editor_drag();
                        }
                        if self.screen == Screen::WorldOptions {
                            self.world_options_drag();
                        }
                        if self.screen == Screen::Creative {
                            let (_, oy) = Self::creative_origin(&b);
                            self.creative_screen.drag_scrollbar(self.cursor.1 - oy);
                        }
                        if self.screen == Screen::Inventory {
                            self.station_drag();
                            self.recipe_book_drag();
                        }
                        if matches!(self.screen, Screen::Inventory | Screen::Creative) {
                            self.inventory_drag_move();
                        }
                        if self.title_input_active()
                            && let Some(title) = &mut self.title
                        {
                            title.drag(self.cursor, (b.width, b.height));
                            title.drag_field(&self.gui.atlas, self.cursor.0);
                        }
                        if self.dragging && self.screen == Screen::Options {
                            let _ = b;
                            self.options_drag()?;
                        }
                    }
                }
                WindowEvent::MouseWheel { .. }
                    if self.screen == Screen::Playing && self.studio_replay.is_some() => {}
                WindowEvent::MouseWheel { delta, .. } => {
                    let (raw_x, raw_y) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => (f64::from(x), f64::from(y)),
                        MouseScrollDelta::PixelDelta(p) => (p.x / 30.0, p.y / 30.0),
                    };
                    // MouseHandler.onScroll: discrete scrolling and the wheel sensitivity.
                    let discrete = self.settings.discrete_scroll;
                    let wheel_sensitivity = f64::from(self.settings.mouse_wheel_sensitivity);
                    let scaled = |v: f64| {
                        (if discrete {
                            horizon_client::mouse::java_signum(v)
                        } else {
                            v
                        }) * wheel_sensitivity
                    };
                    let d = scaled(raw_y) as f32;
                    // ScreenEvent.MouseScrolled.Pre: the open screen hears of
                    // the wheel before anything scrolls, and a cancelled turn
                    // scrolls nothing — no bundle, no list, slider or
                    // scrollbar. The hotbar below reads the wheel only while
                    // playing, which the gate excludes. A wheel the pad or the
                    // keyboard's focus made up is not the mouse: it fires
                    // nothing.
                    if !self.pad_synthetic_input()
                        && let Some((id, mouse)) = self.screen_view_now().map(|s| (s.id, s.mouse))
                    {
                        let (scroll, modifiers) = (
                            [scaled(raw_x), scaled(raw_y)],
                            glfw_modifiers(self.modifiers),
                        );
                        let mut canceled = false;
                        // v2 lends the game mutably, then v1 hears the
                        // screen as v2 left it (gone, if v2 closed it).
                        self.native_screen_mouse_scrolled_pre_v2(
                            id,
                            mouse,
                            scroll,
                            modifiers,
                            &mut canceled,
                        );
                        if !canceled && let Some(screen) = self.screen_view_now() {
                            self.native_screen_mouse_scrolled_pre(
                                &screen,
                                scroll,
                                modifiers,
                                &mut canceled,
                            );
                        }
                        if canceled {
                            return Ok(());
                        }
                    }
                    // A prompt is vanilla's `Minecraft.screen`, the only one MouseHandler.onScroll
                    // tells: the top one hears the wheel, as the click arms below pick it, and
                    // nothing behind it does.
                    if self.prompt_open() {
                        if self.command_prompt.is_some() || self.link_prompt.is_some() {
                        } else if self.code_of_conduct.is_some() {
                            self.code_of_conduct_scroll(d);
                        } else if self.server_packs.prompt.is_none() {
                            self.dialog_scroll(d);
                        }
                        return Ok(());
                    }
                    let stick_scroll = self.pad_stick_scroll();
                    let step_scroll = self.pad_step_scroll();
                    if self.title_input_active()
                        && self.screen != Screen::Win
                        && let Some(title) = &mut self.title
                    {
                        let size = self
                            .window
                            .as_ref()
                            .map(|w| w.inner_size())
                            .unwrap_or_default();
                        let batch = HudBatch::new([size.width, size.height]);
                        let gui = (batch.width, batch.height);
                        if step_scroll {
                            title.step_scroll(d, self.cursor, gui);
                        } else if stick_scroll {
                            title.stick_scroll(d, self.cursor, gui);
                        } else {
                            title.scroll_worlds(d, self.cursor, gui);
                        }
                    }
                    // MouseHandler.onScroll: the hotbar (or the spectator
                    // menu) takes the wheel only when no screen is open; a
                    // book, lectern, sign or prompt drawn over gameplay is one.
                    if self.hotbar_takes_wheel()
                        && !self.native_wheel_event(d)
                        && let Some((wheel, (horizontal_steps, vertical_steps))) = self
                            .mouse
                            .on_scroll(raw_x, raw_y, discrete, wheel_sensitivity)
                    {
                        if !self.pad_synthetic_input()
                            && self.hotbar_scroll_canceled(
                                [scaled(raw_x), scaled(raw_y)],
                                [horizontal_steps, vertical_steps],
                            )
                        {
                            return Ok(());
                        }
                        self.hotbar_wheel_step(wheel, vertical_steps);
                    }
                    if self.screen == Screen::Chat && d != 0.0 {
                        self.chat_scroll(d);
                    }
                    if self.screen == Screen::Social && d != 0.0 {
                        self.social_scroll(d);
                    }
                    if self.screen == Screen::CommandBlock && d != 0.0 {
                        self.command_block_scroll(d);
                    }
                    if self.screen == Screen::Options && d != 0.0 {
                        self.options_scroll(d)?;
                    }
                    // AbstractContainerScreen.mouseScrolled: a bundle under the
                    // cursor takes the wheel before the screen's own scrolling.
                    let bundle = bundles::takes_bundle_wheel(self.screen)
                        && d != 0.0
                        && self.bundle_scroll(scaled(raw_x), scaled(raw_y));
                    if self.screen == Screen::Creative && d != 0.0 && !bundle {
                        self.creative_screen.wheel(f64::from(d), &self.pack.names);
                    }
                    if self.screen == Screen::Inventory
                        && d != 0.0
                        && !bundle
                        && !self.tweak_scroll(d, self.inventory_target().0)
                    {
                        self.station_scroll(d);
                    }
                    if self.screen == Screen::Stats && d != 0.0 {
                        self.stats_scroll(d);
                    }
                    if self.screen == Screen::WorldOptions && d != 0.0 {
                        self.world_options_scroll(d);
                    }
                    if self.screen == Screen::ServerLinks && d != 0.0 {
                        self.server_links_scroll(d);
                    }
                    if self.screen == Screen::DebugOptions && d != 0.0 {
                        self.debug_options_scroll(d);
                    }
                    if self.screen == Screen::HudEditor && d != 0.0 {
                        self.hud_editor_scroll(f64::from(d));
                    }
                    if self.screen == Screen::Advancements {
                        self.advancements_scroll(scaled(raw_x) as f32, d);
                    }
                }
                WindowEvent::MouseInput { state, button, .. } if self.command_prompt.is_some() => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        self.command_prompt_click();
                    }
                }
                WindowEvent::MouseInput { state, button, .. } if self.link_prompt.is_some() => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        self.link_prompt_click();
                    }
                }
                WindowEvent::MouseInput { state, button, .. } if self.code_of_conduct.is_some() => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        self.code_of_conduct_click()?;
                    }
                }
                WindowEvent::MouseInput { state, button, .. }
                    if self.server_packs.prompt.is_some() =>
                {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        self.pack_prompt_click()?;
                    }
                }
                WindowEvent::MouseInput { state, button, .. } if self.dialog_open() => {
                    if button == MouseButton::Left {
                        if state == ElementState::Pressed {
                            self.dialog_click()?;
                        } else {
                            self.dialog_release();
                        }
                    }
                }
                // ConnectScreen's Cancel, under the prompts and dialogs above.
                WindowEvent::MouseInput { state, button, .. } if self.remote_loading() => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        self.connect_screen_click()?;
                    }
                }
                WindowEvent::MouseInput { .. } if self.screen == Screen::Win => {}
                WindowEvent::MouseInput { state, button, .. } if self.screen == Screen::Options => {
                    if self.options_waiting_for_key() && state == ElementState::Pressed {
                        self.options_bind_mouse(button)?;
                    } else if matches!(button, MouseButton::Left | MouseButton::Right) {
                        self.options_mouse(
                            state == ElementState::Pressed,
                            button == MouseButton::Right,
                        )?;
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                } if self.title_input_active() && self.screen != Screen::Options => {
                    if let Some(title) = &mut self.title {
                        title.release();
                        title.release_field();
                    }
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } if self.title_input_active() && state == ElementState::Pressed => {
                    let size = self
                        .renderer
                        .as_ref()
                        .map(|r| [r.config.width, r.config.height])
                        .unwrap_or([1, 1]);
                    let b = HudBatch::new(size);
                    let atlas = &self.gui.atlas;
                    let shift = self.has_shift_down();
                    let (action, sound) = self.title.as_mut().map_or((Action::None, false), |t| {
                        let clicked = t.click(self.cursor.0, self.cursor.1, (b.width, b.height));
                        t.press_field(atlas, self.cursor, (b.width, b.height), shift);
                        let rules = t.press_rules(atlas, self.cursor, (b.width, b.height));
                        (clicked.0, clicked.1 || rules)
                    });
                    self.save_title_options();
                    if sound {
                        self.audio.play_button_click();
                    }
                    self.title_action(event_loop, action)?;
                    self.reopen_data_packs(event_loop)?;
                }
                // AbstractSelectionList.mouseClicked: the world list takes a
                // click of any button, so the middle one selects its row
                // without joining (only the left click's play icon or its
                // click on the selected row does that).
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Middle,
                    ..
                } if self.title_input_active() && self.screen != Screen::Options => {
                    let size = self
                        .renderer
                        .as_ref()
                        .map(|r| [r.config.width, r.config.height])
                        .unwrap_or([1, 1]);
                    let b = HudBatch::new(size);
                    if let Some(title) = self.title.as_mut() {
                        title.select_world_at(self.cursor.0, self.cursor.1, (b.width, b.height));
                    }
                }
                WindowEvent::MouseInput { .. } if self.title_input_active() => {}
                WindowEvent::MouseInput { state, button, .. } if self.book.is_some() => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        let size = self
                            .renderer
                            .as_ref()
                            .map(|r| [r.config.width, r.config.height])
                            .unwrap_or([1, 1]);
                        self.book_click(size);
                    }
                }
                WindowEvent::MouseInput { state, button, .. } if self.sign_edit.is_some() => {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        let size = self
                            .renderer
                            .as_ref()
                            .map(|r| [r.config.width, r.config.height])
                            .unwrap_or([1, 1]);
                        self.sign_edit_click(size);
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    let pressed = state == ElementState::Pressed;
                    if !pressed && button == MouseButton::Left {
                        let action = self.studio_timeline.handle_mouse_up();
                        if !matches!(action, horizon_client::studio::TimelineAction::None) {
                            self.handle_studio_timeline_action(action);
                        }
                    }
                    if (self.studio_freecam.active || self.studio_replay.is_some())
                        && button == MouseButton::Right
                    {
                        if let Some(renderer) = &self.renderer {
                            let batch =
                                HudBatch::new([renderer.config.width, renderer.config.height]);
                            if self.studio_timeline.visible
                                && self.cursor.1 >= batch.height - self.studio_timeline.height()
                            {
                                if pressed
                                    && let Some(index) = self.studio_timeline.keyframe_at_cursor(
                                        self.cursor,
                                        batch.width,
                                        batch.height,
                                        &self.studio_freecam,
                                    )
                                {
                                    if index < self.studio_freecam.track.keyframes.len() {
                                        self.studio_freecam.track.keyframes.remove(index);
                                        self.studio_camera_track_dirty = true;
                                    }
                                    self.studio_timeline.selected_keyframe = None;
                                }
                                return Ok(());
                            }
                        }
                        if pressed {
                            self.studio_topbar.close_dropdown();
                            self.studio_rmb_down = true;
                            self.capture(true);
                            return Ok(());
                        } else if self.studio_rmb_down {
                            self.studio_rmb_down = false;
                            self.capture(false);
                            return Ok(());
                        }
                    }
                    if pressed
                        && button == MouseButton::Left
                        && self.screen == Screen::Playing
                        && (self.studio_replay.is_some() || self.studio_freecam.active)
                    {
                        let size = self
                            .renderer
                            .as_ref()
                            .map(|renderer| [renderer.config.width, renderer.config.height])
                            .unwrap_or([1, 1]);
                        let batch = HudBatch::new(size);
                        let had_open_menu = self.studio_topbar.open_menu.is_some();
                        let topbar_action = self.studio_topbar.handle_mouse_click(
                            self.cursor,
                            batch.width,
                            batch.height,
                        );
                        if !matches!(topbar_action, horizon_client::studio::TopBarAction::None) {
                            self.handle_studio_topbar_action(topbar_action);
                            return Ok(());
                        }
                        if had_open_menu || self.cursor.1 <= self.studio_topbar.height {
                            return Ok(());
                        }
                        if self.studio_timeline.visible {
                            let action = self.studio_timeline.handle_mouse_down(
                                self.cursor,
                                batch.width,
                                batch.height,
                                &self.studio_freecam,
                            );
                            if !matches!(action, horizon_client::studio::TimelineAction::None) {
                                self.handle_studio_timeline_action(action);
                                return Ok(());
                            }
                            if self.cursor.1 >= batch.height - self.studio_timeline.height() {
                                return Ok(());
                            }
                        }
                    }
                    if self.screen == Screen::Playing && self.studio_replay.is_some() {
                        return Ok(());
                    }
                    if matches!(self.screen, Screen::Inventory | Screen::Creative)
                        && let Some(key) = self
                            .settings
                            .keys
                            .container_mouse_key(button, self.creative)
                    {
                        if pressed && self.carried.is_none() {
                            self.inventory_hotkey(key);
                        }
                        // Neither edge of a bound button is an ordinary slot click.
                        return Ok(());
                    }
                    if pressed
                        && (self.sort_mouse_press(button)
                            || self.hdr_mouse_press(button)
                            || self.hud_editor_key_press(&horizon_client::keybinds::mouse_name(
                                button,
                            )))
                    {
                        return Ok(());
                    }
                    match self.screen {
                        Screen::Playing => {
                            if pressed && !self.captured {
                                self.capture(true);
                                return Ok(());
                            }
                            self.extras_mouse(button, pressed);
                            if pressed
                                && self.settings.keys.get("key.smoothCamera")
                                    == horizon_client::keybinds::mouse_name(button)
                            {
                                self.smooth_camera_action();
                            }
                            let input = horizon_client::keybinds::mouse_name(button);
                            for mapped in self.settings.keys.translate_mouse_all(button) {
                                match mapped {
                                    Err(b)
                                        if !self.shared_default_acts(b, Some(input.as_str())) => {}
                                    Err(b) => self.mouse_mapping(b, pressed, Source::Keyboard)?,
                                    Ok(k) if pressed => {
                                        self.keys.insert(k);
                                        self.key_pressed(k, k)?;
                                    }
                                    Ok(k) => {
                                        self.keys.remove(&k);
                                    }
                                }
                            }
                        }
                        Screen::Inventory | Screen::Creative
                            if !matches!(button, MouseButton::Left | MouseButton::Right) =>
                        {
                            // AbstractContainerScreen: another button clicks
                            // a slot only as the pick block key (its hotbar
                            // keys were handled above).
                            if self.settings.keys.get("key.pickItem")
                                == horizon_client::keybinds::mouse_name(button)
                            {
                                if pressed {
                                    self.inventory_pick_press();
                                } else {
                                    self.inventory_pick_release();
                                }
                                if let Some(key) = self.container {
                                    self.container_changed(key);
                                }
                            }
                        }
                        Screen::Inventory if pressed => {
                            self.inventory_press(button == MouseButton::Right);
                            if let Some(key) = self.container {
                                self.container_changed(key);
                            }
                        }
                        Screen::Inventory => {
                            self.inventory_mouse_release(button == MouseButton::Right);
                            self.station_release();
                            self.recipe_book_release();
                        }
                        Screen::Chat if pressed && button == MouseButton::Left => self.chat_click(),
                        Screen::Creative if pressed => {
                            self.click_creative(button == MouseButton::Right);
                        }
                        Screen::Creative => {
                            if button == MouseButton::Left {
                                self.creative_screen.release();
                            }
                            self.inventory_mouse_release(button == MouseButton::Right);
                        }
                        Screen::Paused
                            if pressed && button == MouseButton::Left && self.pause_menu =>
                        {
                            self.pause_click()?
                        }
                        Screen::WorldOptions if pressed && button == MouseButton::Left => {
                            self.world_options_click()
                        }
                        Screen::ServerLinks if pressed && button == MouseButton::Left => {
                            self.server_links_click()?
                        }
                        Screen::DebugOptions if pressed && button == MouseButton::Left => {
                            self.debug_options_click()
                        }
                        Screen::DebugOptions if button == MouseButton::Left => {
                            self.debug_options_release()
                        }
                        Screen::HudEditor
                            if pressed
                                && matches!(button, MouseButton::Left | MouseButton::Right) =>
                        {
                            self.hud_editor_press(button == MouseButton::Right)
                        }
                        Screen::HudEditor if button == MouseButton::Left => {
                            self.hud_editor_release()
                        }
                        Screen::ConfirmLink if pressed && button == MouseButton::Left => {
                            self.confirm_link_click()
                        }
                        Screen::DialogWarning if pressed && button == MouseButton::Left => {
                            self.dialog_warning_click()?
                        }
                        Screen::WorldOptions if button == MouseButton::Left => {
                            self.world_options_release()
                        }
                        Screen::Friends if pressed && button == MouseButton::Left => {
                            self.friends_screen_click()
                        }
                        Screen::Social if pressed && button == MouseButton::Left => {
                            self.social_click()
                        }
                        Screen::CommandBlock if pressed && button == MouseButton::Left => {
                            self.command_block_click()
                        }
                        Screen::BlockEditor if pressed && button == MouseButton::Left => {
                            self.block_editor_click()
                        }
                        Screen::BlockEditor if button == MouseButton::Left => {
                            self.block_editor_release()
                        }
                        Screen::Dead if pressed && button == MouseButton::Left => {
                            self.death_click()?
                        }
                        Screen::Advancements if button == MouseButton::Left => {
                            let size = self
                                .renderer
                                .as_ref()
                                .map_or([1, 1], |r| [r.config.width, r.config.height]);
                            self.advancements_mouse(pressed, size);
                        }
                        Screen::Stats if pressed && button == MouseButton::Left => {
                            let size = self
                                .renderer
                                .as_ref()
                                .map_or([1, 1], |r| [r.config.width, r.config.height]);
                            self.stats_click(size);
                        }
                        Screen::Stats if button == MouseButton::Left => self.stats_release(),
                        Screen::GameModeSwitcher if !pressed => self.switcher_release(button),
                        _ => {}
                    }
                }
                // ConfirmScreen.keyPressed: Escape answers no, once its delay is over.
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen == Screen::Dead && self.ui.death_confirm.is_some() =>
                {
                    if event.state == ElementState::Pressed
                        && event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                        && self
                            .ui
                            .death_confirm
                            .is_some_and(|t| t.elapsed().as_millis() >= 1000)
                    {
                        self.death_confirm_answer(false)?;
                    }
                }
                // ConfirmScreen.keyPressed: Escape answers no.
                WindowEvent::KeyboardInput { event, .. } if self.command_prompt.is_some() => {
                    if event.state == ElementState::Pressed
                        && let PhysicalKey::Code(key) = event.physical_key
                    {
                        self.command_prompt_key(key);
                    }
                }
                WindowEvent::KeyboardInput { event, .. } if self.link_prompt.is_some() => {
                    if event.state == ElementState::Pressed
                        && event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                    {
                        self.answer_link_prompt(false);
                    }
                }
                // CodeOfConductScreen.shouldCloseOnEsc is false: no key leaves the screen.
                WindowEvent::KeyboardInput { .. } if self.code_of_conduct.is_some() => {}
                WindowEvent::KeyboardInput { event, .. } if self.server_packs.prompt.is_some() => {
                    if event.state == ElementState::Pressed
                        && event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                    {
                        self.answer_pack_prompt(false)?;
                    }
                }
                WindowEvent::KeyboardInput { event, .. } if self.dialog_open() => {
                    let shortcut = self.edit_shortcut(&event);
                    if event.state == ElementState::Pressed
                        && let PhysicalKey::Code(key) = event.physical_key
                    {
                        self.dialog_key(key, shortcut, event.text.as_deref())?
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen == Screen::Chat && self.title.is_none() =>
                {
                    if let PhysicalKey::Code(key) = event.physical_key {
                        if event.state == ElementState::Pressed {
                            self.keys.insert(key);
                            self.chat_key(&event);
                        } else {
                            self.keys.remove(&key);
                        }
                    }
                }
                WindowEvent::KeyboardInput { event, .. } if self.sign_edit.is_some() => {
                    self.sign_edit_key(&event)
                }
                WindowEvent::KeyboardInput { event, .. } if self.book.is_some() => {
                    self.book_key(&event)
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen == Screen::Creative
                        && self.creative_screen.tab().search
                        && !self.mod_browser_typing()
                        && event.state == ElementState::Pressed
                        && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape)) =>
                {
                    if let PhysicalKey::Code(key) = event.physical_key
                        && self.search_tab_hotbar_key(key)
                    {
                        // The hovered slot took the key, and its characters
                        // go nowhere (`ignoreTextInput`).
                        typed = None;
                    } else if let Some(shortcut) = self.edit_shortcut(&event) {
                        self.creative_search_shortcut(shortcut);
                    } else if let PhysicalKey::Code(key) = event.physical_key {
                        // EditBox.keyPressed: arrows, Home, End, Delete and Backspace.
                        let before = self.creative_screen.search.value.clone();
                        let (ctrl, shift) = (self.has_control_down(), self.has_shift_down());
                        if self.creative_screen.search.key(key, ctrl, shift) {
                            if self.creative_screen.search.value != before {
                                self.creative_screen.scroll = 0.0;
                            }
                        } else {
                            self.creative_search_edit(event.text.as_deref());
                        }
                    } else {
                        self.creative_search_edit(event.text.as_deref());
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.recipe_search_focused()
                        && event.state == ElementState::Pressed
                        && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape)) =>
                {
                    // The recipe book's search box takes every key but Escape.
                    self.recipe_key(&event);
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.mod_browser_typing()
                        && event.state == ElementState::Pressed
                        && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape)) =>
                {
                    // A mod's item panel's search box, as the recipe book's.
                    self.mod_browser_key(&event);
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.station_typing()
                        && event.state == ElementState::Pressed
                        && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape)) =>
                {
                    // The anvil rename box takes every key but Escape (AnvilScreen.keyPressed).
                    if let Some(shortcut) = self.edit_shortcut(&event) {
                        self.station_shortcut(shortcut);
                    } else if let PhysicalKey::Code(key) = event.physical_key
                        && self.station_key(key)
                    {
                    } else {
                        let backspace = event.physical_key == PhysicalKey::Code(KeyCode::Backspace);
                        self.station_text(event.text.as_deref(), backspace);
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen == Screen::WorldOptions
                        && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape)) =>
                {
                    self.world_options_key(&event)
                }
                WindowEvent::KeyboardInput { event, .. } if self.screen == Screen::Win => {
                    self.win_key(&event)?
                }
                // These screens consume presses; releases still reach the
                // bound gameplay mapping below, so a movement key pressed
                // before the screen opened doesn't stay held after it closes.
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen.takes_presses() && event.state == ElementState::Pressed =>
                {
                    self.screen_press(&event)
                }
                // Options consumes presses; releases still reach the bound
                // gameplay mapping below, including sneak/sprint toggles.
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen == Screen::Options && event.state == ElementState::Pressed =>
                {
                    if let Some(shortcut) = self.edit_shortcut(&event)
                        && self.options_shortcut(shortcut)
                    {
                        return Ok(());
                    }
                    if let PhysicalKey::Code(key) = event.physical_key {
                        self.options_key(key, event.text.as_deref())?;
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen == Screen::Paused
                        && event.state == ElementState::Pressed
                        && self.pause_menu
                        && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape)) =>
                {
                    if let PhysicalKey::Code(key) = event.physical_key {
                        self.native_key_event(key, true, event.repeat);
                    }
                    if let PhysicalKey::Code(key) = event.physical_key {
                        self.pause_key(key)?;
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if self.screen == Screen::Dead && event.state == ElementState::Pressed =>
                {
                    if let PhysicalKey::Code(key) = event.physical_key {
                        self.native_key_event(key, true, event.repeat);
                    }
                    if let PhysicalKey::Code(key) = event.physical_key {
                        self.death_key(key)?;
                    }
                }
                WindowEvent::KeyboardInput { event, .. } if self.title_input_active() => {
                    if event.state == ElementState::Pressed {
                        if let Some(shortcut) = self.edit_shortcut(&event) {
                            self.title_shortcut(shortcut);
                            return Ok(());
                        }
                        // `TabNavigationBar.keyPressed` comes before the open
                        // tab's own widgets: Ctrl plus a digit or Tab picks a
                        // tab instead of reaching them.
                        if let PhysicalKey::Code(key) = event.physical_key {
                            let ctrl = self.has_control_down_with_quirk();
                            let shift = self.has_shift_down();
                            let tab = self
                                .title
                                .as_mut()
                                .and_then(|t| t.tab_bar_key(key, ctrl, shift));
                            if let Some(sound) = tab {
                                if sound {
                                    self.audio.play_button_click();
                                }
                                return Ok(());
                            }
                        }
                        let (w, h) = self.gui_size();
                        match event.physical_key {
                            // `WorldListEntry.keyPressed`: Space joins too, with the button click sound.
                            PhysicalKey::Code(
                                code @ (KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space),
                            ) if code != KeyCode::Space
                                || self.title.as_mut().is_some_and(|t| {
                                    !t.has_caret_field()
                                        && (t.list_focused() || t.focused_button.is_some())
                                }) =>
                            {
                                let (action, sound) = self
                                    .title
                                    .as_mut()
                                    .map_or((Action::None, false), |t| t.confirm_key_size((w, h)));
                                self.save_title_options();
                                if sound {
                                    self.audio.play_button_click();
                                }
                                self.title_action(event_loop, action)?;
                            }
                            PhysicalKey::Code(
                                key @ (KeyCode::Backspace
                                | KeyCode::Delete
                                | KeyCode::ArrowLeft
                                | KeyCode::ArrowRight
                                | KeyCode::Home
                                | KeyCode::End),
                            ) if self.title.as_mut().is_some_and(|t| t.has_caret_field()) => {
                                let (ctrl, shift) =
                                    (self.has_control_down(), self.has_shift_down());
                                if let Some(t) = self.title.as_mut() {
                                    t.field_key(key, ctrl, shift);
                                }
                            }
                            PhysicalKey::Code(
                                key @ (KeyCode::ArrowLeft
                                | KeyCode::ArrowRight
                                | KeyCode::ArrowUp
                                | KeyCode::ArrowDown),
                            ) => {
                                let shift = self.has_shift_down();
                                if let Some(t) = self.title.as_mut() {
                                    t.arrow_key(key, (w, h), shift);
                                }
                            }
                            PhysicalKey::Code(KeyCode::Backspace) => self.title_edit(None, true),
                            PhysicalKey::Code(KeyCode::Tab) => {
                                let shift = self.has_shift_down();
                                if let Some(t) = self.title.as_mut() {
                                    t.tab_dir(shift, (w, h));
                                }
                            }
                            PhysicalKey::Code(KeyCode::Escape) => {
                                if let Some(t) = self.title.as_mut() {
                                    t.escape();
                                    self.reopen_data_packs(event_loop)?;
                                }
                                self.save_title_options();
                            }
                            PhysicalKey::Code(KeyCode::F5) => {
                                if let Some(t) = self.title.as_mut() {
                                    t.refresh_servers();
                                }
                            }
                            _ => self.title_edit(event.text.as_deref(), false),
                        }
                    }
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let PhysicalKey::Code(key) = event.physical_key else {
                        return Ok(());
                    };
                    self.native_key_event(key, event.state == ElementState::Pressed, event.repeat);
                    // Screen.keyPressed sees the physical key, repeats included.
                    if event.state == ElementState::Pressed
                        && self.screen == Screen::Inventory
                        && (self.merchant_key(key) || self.recipe_book_key(key))
                    {
                        return Ok(());
                    }
                    if event.state == ElementState::Pressed
                        && !event.repeat
                        && horizon_client::keybinds::key_name(key)
                            .is_some_and(|n| self.settings.keys.get("key.smoothCamera") == n)
                    {
                        self.smooth_camera_action();
                    }
                    self.mapped_key_event(key, event.state == ElementState::Pressed, event.repeat)?;
                }
                WindowEvent::RedrawRequested => {
                    horizon_client::spans::time("recover_device", || self.recover_device())?;
                    self.observe_fullscreen();
                    self.sync_ime();
                    let now = Instant::now();
                    let frame_cpu = self
                        .options
                        .spikes
                        .and_then(|_| horizon_client::spans::thread_cpu());
                    let elapsed = now.duration_since(self.previous);
                    let between = now.saturating_duration_since(self.frame_end);
                    self.frame_ticks = elapsed.as_secs_f32() * 20.0;
                    self.previous = now;
                    if self.studio_freecam.active
                        && self.screen == Screen::Playing
                        && !self.dialog_open()
                    {
                        let held = |key| f64::from(u8::from(self.keys.raw(&key)));
                        let forward = held(KeyCode::KeyW) - held(KeyCode::KeyS);
                        let strafe = held(KeyCode::KeyD) - held(KeyCode::KeyA);
                        let vertical =
                            held(KeyCode::KeyE) - held(KeyCode::KeyQ) + held(KeyCode::Space);
                        let fast = self.keys.raw(&KeyCode::ControlRight);
                        self.studio_freecam.update_flight_motion(
                            forward,
                            strafe,
                            vertical,
                            fast,
                            elapsed.as_secs_f64().min(0.1),
                        );
                    }
                    // Arrow-key camera controls belong to gameplay; chat and
                    // other screens use the same keys for their own navigation.
                    if self.screen == Screen::Playing
                        && self.title.is_none()
                        && self.sign_edit.is_none()
                        && self.book.is_none()
                        && !self.dialog_open()
                    {
                        let turn = |k| f32::from(u8::from(self.keys.raw(&k)));
                        let secs = elapsed.as_secs_f32().min(0.1);
                        let yaw = (turn(KeyCode::ArrowRight) - turn(KeyCode::ArrowLeft)) * 90.0;
                        let pitch = (turn(KeyCode::ArrowDown) - turn(KeyCode::ArrowUp)) * 90.0;
                        if yaw != 0.0 || pitch != 0.0 {
                            // 90 degrees a second, as `Entity.turn` degrees.
                            self.turn_player(
                                f64::from(yaw * secs / 0.15),
                                f64::from(pitch * secs / 0.15),
                            );
                        }
                    }
                    if self.title.is_none() {
                        horizon_client::spans::time("update_world", || self.update_world());
                        horizon_client::spans::time("spawn_player", || self.spawn_player())?;
                    }
                    let updated = Instant::now();
                    self.track_toggle_screens();
                    self.heal_hold_latches();
                    // For the hooks above the engine (the "I'm playing" ping): said when it changes.
                    horizon_hooks::engine::published(self.is_published());
                    #[cfg(feature = "discord")]
                    self.update_discord_presence();
                    let steps = self.clock.advance(elapsed);
                    for _ in 0..steps {
                        let started = Instant::now();
                        // Minecraft.runTick: the friends presence handler ticks before
                        // the world's own ticks.
                        horizon_client::social::set_presence_status(self.presence_status());
                        horizon_client::social::tick();
                        // Minecraft.tick: ClientTickEvent.Pre before any of it.
                        self.native_client_tick_pre();
                        if self.title.is_none() && !self.pause {
                            if self.tick_script.armed() {
                                self.tick_script_step(event_loop);
                            }
                            horizon_client::spans::time("tick", || self.tick())?;
                            // Cave mood (a shader pack's playerMood) samples on a random
                            // seeded from the clock: under `HORIZON_STILL` it waits for the
                            // world and draws from its tick.
                            if !self.clock.still() {
                                self.tick_ambience();
                            } else if self.clock.runs_normally() {
                                self.ambience.reseed(self.visual_ticks() as i64);
                                self.tick_ambience();
                            }
                            self.tick_fov();
                        }
                        self.tick_win_screen()?;
                        self.native_configs_tick_client();
                        // Gui.tick runs ChatListener.tick paused or not.
                        self.tick_chat_queue();
                        self.tick_music();
                        self.tick_sound_engine();
                        // ClientTickEvent.Post, the tick's work done.
                        self.native_client_tick_post();
                        self.clock.record(started.elapsed());
                        if let Some(report) = self.clock.take_sprint_report() {
                            self.sprint_report(&report);
                        }
                    }
                    self.extras_frame(elapsed);
                    self.extras_retry()?;
                    self.apply_mouse_look();
                    self.pad_look(elapsed.as_secs_f32());
                    self.advance_flight();
                    self.tick_world_icon();
                    let ticked = Instant::now();
                    let rendered = match self.render() {
                        // The callback can fire during this frame; rebuild on
                        // the next redraw rather than exiting on its error.
                        Err(_) if self.renderer.as_ref().is_some_and(Renderer::device_lost) => {
                            false
                        }
                        result => result?,
                    };
                    horizon_client::spans::time("pr_cursor", || self.apply_cursor_icon());
                    // The frame's spans, taken every frame they are recorded so they never
                    // pile up: the slow-frame line's, and the session's (`perf.frame`).
                    let frame_spans = if horizon_client::spans::recording_here() {
                        horizon_client::spans::take_raw()
                    } else {
                        Vec::new()
                    };
                    if horizon_client::spans::trace::due() {
                        horizon_client::spans::trace::stop();
                    }
                    if let Some(threshold) = self.options.spikes
                        && let Some(r) = self.renderer.as_ref()
                    {
                        let cpu = now.elapsed().saturating_sub(r.last_wait).as_secs_f32() * 1000.0;
                        // Spans timed between frames (`idle`) come first.
                        let spans = horizon_client::spans::merge(&frame_spans);
                        let between = between.as_secs_f32() * 1000.0;
                        if cpu > threshold || between > threshold {
                            // The render thread's own CPU time in the frame:
                            // far below the wall time, the frame waited.
                            let worked = horizon_client::spans::thread_cpu()
                                .zip(frame_cpu)
                                .map_or_else(String::new, |(end, start)| {
                                    format!(", cpu {:.1} ms", (end - start).as_secs_f32() * 1000.0)
                                });
                            let gap = if between > threshold {
                                format!(", {between:.1} ms before it")
                            } else {
                                String::new()
                            };
                            log::info!(
                                "Horizon: slow frame {cpu:.1} ms{worked}{gap} ({steps} ticks): {}",
                                horizon_client::spans::describe(&spans)
                            );
                        }
                    }
                    if let Some(r) = self.renderer.as_mut() {
                        let cpu = now.elapsed().saturating_sub(r.last_wait);
                        if rendered {
                            let ms = |d: Duration| d.as_secs_f32() * 1000.0;
                            self.frame_graph.record(
                                ms(elapsed),
                                ms(cpu),
                                r.gpu_times.last().copied(),
                            );
                            if let Some(&gpu) = r.gpu_times.last() {
                                self.gpu_utilization =
                                    f64::from(gpu) * 100.0 / f64::from(ms(cpu).max(0.001));
                            }
                            // GPU Culling's Auto: only frames of a world that has settled
                            // count, and only where it can be switched.
                            if self.settings.horizon.gpu_culling
                                == horizon_client::graphics::GpuCullingMode::Auto
                                && self.screen == Screen::Playing
                                && !self.streaming.terrain_loading()
                                && horizon_client::graphics::gpu_culling_offer()
                                    .reason()
                                    .is_none()
                                && let Some(on) = self.gpu_culling_auto.observe(
                                    ms(cpu),
                                    r.gpu_times.last().copied(),
                                    ms(elapsed),
                                )
                            {
                                log::info!(
                                    "Horizon: GPU Culling Auto turns it {} ({})",
                                    if on { "on" } else { "off" },
                                    self.gpu_culling_auto.state()
                                );
                                if r.set_gpu_culling(on).is_err() {
                                    self.gpu_culling_auto.reset();
                                }
                            }
                            self.frame_stats.record_cpu(ms(cpu));
                            let render = r.last_encode;
                            self.frame_stats.record_phases([
                                ms(updated - now),
                                ms(ticked - updated),
                                ms(ticked.elapsed().saturating_sub(r.last_wait + render)),
                                ms(render),
                            ]);
                            self.frame_stats
                                .record_block_entities(r.static_block_entity_frame_stats());
                        }
                        self.frame_stats.record_gpu(&r.gpu_times);
                        r.gpu_times.clear();
                    }
                    self.limit_frame();
                    self.pause = self.pauses_game();
                    if let Some(server) = &self.integrated {
                        server.set_paused(self.pause);
                        server.set_distances(
                            self.settings.render_distance,
                            self.settings.simulation_distance,
                            self.settings.entity_distance,
                        );
                        server.set_legacy_rules(self.settings.extras.pvp.combat.rules);
                    }
                    self.clock.end_frame(self.pause);
                    let mut consumed = false;
                    let finished = self
                        .renderer
                        .as_mut()
                        .and_then(|r| r.screenshot_result.take());
                    if let Some(result) = &finished
                        && self.ui.screenshot_message.is_some()
                    {
                        self.finish_screenshot(result);
                        consumed = true;
                    }
                    if let Some(result) = &finished
                        && let Some(reply) = self.pending_screenshot.take()
                    {
                        let _ = reply.send(
                            match result {
                                Ok([w, h]) => {
                                    serde_json::json!({ "ok": true, "width": w, "height": h })
                                }
                                Err(e) => serde_json::json!({ "error": e }),
                            }
                            .to_string(),
                        );
                        consumed = true;
                    }
                    if let Some(result) =
                        self.renderer.as_mut().and_then(|r| r.capture_result.take())
                        && let Some(reply) = self.pending_render_capture.take()
                    {
                        let value = result.unwrap_or_else(|e| serde_json::json!({ "error": e }));
                        let _ = reply.send(value.to_string());
                    }
                    // A result nobody asked for was the world icon's
                    // (`tick_world_icon`): a failure tries again a second
                    // later, as `tryTakeScreenshotIfNeeded` does.
                    if !consumed && let Some(Err(error)) = finished {
                        log::warn!("Horizon: couldn't save the world icon: {error}");
                    }
                    if rendered {
                        self.frame_stats.record(elapsed.as_secs_f32() * 1000.0);
                        self.debug_charts.frame_times.log(elapsed.as_nanos() as i64);
                        if self.frames == 0 {
                            // The launcher times Play to this line (launcher/src/perf.rs).
                            log::info!(
                                "Horizon: first frame after {} ms",
                                self.started.elapsed().as_millis()
                            );
                        }
                        if self.title.is_some() {
                            self.perf.title_shown();
                        } else if self.screen == Screen::Playing && self.player.is_some() {
                            self.perf
                                .frame(elapsed.as_secs_f32() * 1000.0, &frame_spans);
                            if startup::filling() {
                                let mut fill: Vec<(&'static str, usize)> =
                                    self.streaming.stages().to_vec();
                                fill.push(("resident", self.world.chunks.len()));
                                fill.push(("uploads", self.pending_uploads.len()));
                                fill.push(("backlog", self.remote_backlog()));
                                startup::counters("world.fill", &fill);
                            }
                            if startup::on() {
                                startup::view_filled(
                                    self.world.chunks.len(),
                                    self.streaming.pending()
                                        + self.pending_uploads.len()
                                        + self.remote_backlog(),
                                    self.streaming.radius.max(0) as u32,
                                );
                            }
                            self.world_frames += 1;
                        }
                        self.frames += 1;
                        self.fps_frames += 1;
                        horizon_client::crash::debug_crash_tick();
                        if self.fps_at.elapsed() >= Duration::from_secs(1) {
                            self.fps = self.fps_frames as f32 / self.fps_at.elapsed().as_secs_f32();
                            self.fps_frames = 0;
                            self.fps_at = Instant::now();
                        }
                        self.frame_end = Instant::now();
                        if self.options.frames.is_some_and(|f| self.frames >= f)
                            || self
                                .options
                                .world_frames
                                .is_some_and(|f| self.world_frames >= f)
                        {
                            event_loop.exit();
                        }
                    }
                }
                _ => {}
            }

            Ok(())
        })();
        // ScreenEvent.CharacterTyped.Pre: GLFW's char callback comes after its
        // key callback, so a key's characters reach the screen the key left
        // open, after the key's own effects (`KeyboardHandler.charTyped`).
        self.screen_chars_typed(typed.as_deref());
        if let Err(e) = result {
            self.error = Some(e);
            self.exit_on_error(event_loop);
        }
    }
}
/// clouds.png as filled cells (`CloudRenderer.prepare`: alpha 10 or more).
fn load_clouds(root: &std::path::Path) -> (u32, u32, Vec<bool>) {
    horizon_client::hud::decode(&root.join("assets/minecraft/textures/environment/clouds.png"))
        .map(|(w, h, px)| {
            (
                w,
                h,
                px.as_chunks::<4>().0.iter().map(|p| p[3] >= 10).collect(),
            )
        })
        .unwrap_or((1, 1, vec![false]))
}
/// misc/forcefield.png for the world border wall (square, power of two).
fn load_forcefield(root: &std::path::Path) -> (u32, u32, Vec<u8>) {
    horizon_client::hud::decode(&root.join("assets/minecraft/textures/misc/forcefield.png"))
        .ok()
        .filter(|(w, h, _)| w == h && w.is_power_of_two())
        .unwrap_or((1, 1, vec![0; 4]))
}
impl<'a> Game<'a> {
    /// A game with no world open, over the loaded resources: the window's,
    /// or an integrated server's on its own thread.
    fn new(
        res: &'a Arc<Resources>,
        pack: PackResources,
        gui: UiResources,
        options: Options,
        settings: Settings,
        audio: Audio,
        prepared: Option<GameCpuResources>,
    ) -> Result<Self> {
        let root = options.resources.clone();
        let registry = pack.assets.registry;
        // Until a world is opened the streamer is idle.
        let mut streaming = Streaming::new(
            Arc::new(TestTerrain::new(registry, res.biomes())?),
            pack.context.clone(),
            1,
        );
        streaming.paused = true;
        let level = Level::default();
        let save = Arc::new(SaveDir::new(
            std::env::current_dir()?
                .join("saves")
                .join(".horizon-title"),
            horizon_client::save::LevelShape::OVERWORLD,
        ));
        let prepared = prepared
            .map(Ok)
            .unwrap_or_else(|| GameCpuResources::load(&root, None))?;
        let world = World::new(res.facts().air, 0);
        let tutorial = tutorial::Tutorial::new(tutorial::Step::from_option(
            settings.file.int("tutorialStep"),
        ));
        let creative = options.creative;
        let inventory = vec![None; 36];
        let now = Instant::now();
        let look = options.look;
        let particles = horizon_client::particles::ParticleEngine::new(
            &root,
            &gui.atlas.sprites,
            &gui.atlas.translucent,
        );
        let mut game = Game {
            world_biomes: None,
            world_biomes_base: None,
            res: res.clone(),
            rest_loader: None,
            pending_entry: None,
            debug_density: RefCell::default(),
            pending_pack_reload: false,
            frame_attempted: render_settings::FrameAttempt::default(),
            pack_folder: None,
            pack_reload: None,
            server_packs: Default::default(),
            link_prompt: None,
            command_prompt: None,
            code_of_conduct: None,
            dialogs: Default::default(),
            integrated_starting: None,
            world_renderer_ready: false,
            collision: res.collision(),
            motion: res.motion(),
            health_resources: res.motion().health.clone(),
            damage_tag_generation: 1,
            pack,
            gui,
            fog: Default::default(),
            screen_fx: Default::default(),
            item_activation: Default::default(),
            weather_parameters: Default::default(),
            panorama_rotation: Default::default(),
            world,
            streaming,
            lod: None,
            lod_source_attempted: false,
            far_beacons: HashMap::new(),
            lod_observe: Default::default(),
            lod_held: Default::default(),
            lod_level: None,
            lod_blue_map: None,
            lod_covered_at: None,
            view_arrival: Default::default(),
            options,
            player: None,
            spawn: None,
            clock: FixedStep::default(),
            previous_position: Vec3::ZERO,
            rope_rot_old: (0.0, 10.0),
            eye: 1.62,
            previous_eye: 1.62,
            yaw: look.map_or(level.rotation[0], |l| l.0),
            pitch: look.map_or(level.rotation[1], |l| l.1),
            inventory,
            selected: 0,
            carried: None,
            craft_grid: vec![None; 9],
            craft_size: 2,
            craft_table: None,
            inv_drag: None,
            tweaks: Default::default(),
            bundle_mouse: Default::default(),
            inv_clicks: Default::default(),
            last_inv_click: None,
            last_quick_moved: None,
            container: None,
            open_container_slots: None,
            station: None,
            potions: Default::default(),
            scratch_slot: None,
            mining: None,
            destroying: HashMap::new(),
            destroy_delay: 0,
            use_delay: 0,
            creative,
            mode: if creative {
                horizon_client::command::args::GameMode::Creative
            } else {
                horizon_client::command::args::GameMode::Survival
            },
            keys: Default::default(),
            physical_keys: HashSet::new(),
            physical_buttons: HashSet::new(),
            drag_button: None,
            right_click_emulation: Default::default(),
            left: false,
            right: false,
            cursor: (0.0, 0.0),
            cursor_icon: horizon_client::widgets::Cursor::Arrow,
            captured: false,
            capture_mode: None,
            regrab_on_refocus: false,
            keyboard_mouse: true,
            split_window: false,
            view_at: [0, 0],
            split_hud: Default::default(),
            split_auth: None,
            focused: true,
            unclaimed_window: false,
            focus_after_run: None,
            ime_allowed: false,
            ime_area: None,
            focus_lost: None,
            modifiers: winit::keyboard::ModifiersState::empty(),
            fullscreen: horizon_client::window::FullscreenState::new(false),
            video_modes: Default::default(),
            mouse: MouseLook::default(),
            agent_pointer: false,
            smooth_camera: false,
            toggles: Default::default(),
            was_playing: false,
            pause: false,
            debug: false,
            gpu_passes: false,
            screen: Screen::Playing,
            observed_screen: None,
            window: None,
            renderer: None,
            last_device_recovery: None,
            entity_vertices: Vec::new(),
            dynamic_lights: Vec::new(),
            luminance: Default::default(),
            see_through_vertices: Vec::new(),
            shadow_only_vertices: Default::default(),
            entity_spans: Default::default(),
            entity_motion: Default::default(),
            entity_motion_posed: Default::default(),
            skin_models: Default::default(),
            skin_frame: Default::default(),
            glowing_vertices: Default::default(),
            pack_ids: None,
            health_hud: hud_layout::HealthHud::default(),
            started: now,
            spectator_gui: Default::default(),
            previous: now,
            frame_end: now,
            last_input: now,
            pad: Default::default(),
            frames: 0,
            glint_frame: Default::default(),
            kept_block_entities: Default::default(),
            item_level_cache: Default::default(),
            world_frames: 0,
            fps: 0.0,
            fps_frames: 0,
            fps_at: now,
            ticks: 0,
            texture_ticks: 0,
            error: None,
            message: None,
            save,
            level,
            player_autosaves: Default::default(),
            session_lock: None,
            audio,
            music: {
                // The manager starts with the saved option (MusicManager's constructor).
                let mut music = horizon_client::music::MusicManager::default();
                music.frequency = horizon_client::music::MusicFrequency::parse(
                    &settings.file.name("musicFrequency"),
                );
                music
            },
            music_layers: Default::default(),
            // ToastManager.initializeMusicToast: Pause and Pause and Toast make the toast.
            now_playing: (settings.file.int("musicToast").clamp(0, 2) != 0).then(Default::default),
            music_toast_state: settings.file.int("musicToast").clamp(0, 2) as u8,
            music_notes: Default::default(),
            ambience: Default::default(),
            sound_paused: false,
            cart_sounds: HashSet::new(),
            riding_cart_sounds: None,
            riding_sound: None,
            was_fall_flying: false,
            bee_sounds: HashMap::new(),
            guardian_sounds: HashSet::new(),
            guardian_look_scratch: Vec::new(),
            dropped: Default::default(),
            next_entity: 0,
            ticked_entities_through: u32::MAX,
            contact_drops: 0..0,
            particles,
            pickups: Default::default(),
            sign_edit: None,
            book: None,
            book_last_click: None,
            lectern_pulses: Vec::new(),
            loading_progress: 0.0,
            creation_job: None,
            loading_progress_at: None,
            moving_pistons_stale: false,
            spawn_origin: None,
            spawn_spiral: Default::default(),
            player_spawn: None,
            spawn_wait: Vec::new(),
            mobs: Default::default(),
            mob_index: Default::default(),
            creatures: Default::default(),
            sculk: Default::default(),
            raiding: Default::default(),
            attack_ticks: 100,
            attack_item: None,
            last_hurt_by_player: HashMap::new(),
            lead_killers: HashMap::new(),
            love_cause: HashMap::new(),
            died: Default::default(),
            be_anims: {
                let mut anims = horizon_client::block_entity_anim::BlockEntityAnims::default();
                anims.classify(registry);
                anims
            },
            using: None,
            use_duration: 0,
            use_ticks_set: false,
            crossbow_sounds: [false; 2],
            view: Default::default(),
            limiter: horizon_client::window::FramerateLimiter::new(0),
            frame_clock: Default::default(),
            scope_scale: 0.5,
            frame_ticks: 0.0,
            title: None,
            armor: [None, None, None, None],
            offhand: None,
            hotbar_pop: [0; 10],
            hand: Hand::Main,
            shield_cooldown: 0,
            cooldowns: Default::default(),
            command_source: None,
            titles: Default::default(),
            tool_highlight: Default::default(),
            saving_indicator: Default::default(),
            autosaving: false,
            // `computeNextAutosaveInterval` at the clock's 20 ticks a second.
            ticks_until_autosave: 6000,
            applied_modifiers: Vec::new(),
            experience: Experience::default(),
            orbs: Default::default(),
            xp_delay: 0,
            ordered: Default::default(),
            level_up_sound: Default::default(),
            swing: Default::default(),
            swings_started: 0,
            predicted_swings: 0,
            heights: Default::default(),
            sway: Default::default(),
            visible_hands: [(0, None), (0, None)],
            hands_replaced: [false; 2],
            clicks: Default::default(),
            attack_clicks: 0,
            use_clicks: 0,
            hand_strength: Default::default(),
            settings,
            tutorial,
            tutorial_toasts: Vec::new(),
            narrator_toast: None,
            narration: Default::default(),
            options_from_title: false,
            dragging: false,
            avatar: Default::default(),
            flash: 0,
            custom_sky: Default::default(),
            rain_sound_time: 0,
            lightning_seen: [0; 2],
            sleeping: None,
            sleep_timer: 0,
            player_beds: Vec::new(),
            sleep_seen: (0, 0),
            dim: Dim::Overworld,
            world_root: PathBuf::new(),
            owner_migration: None,
            world_flat: false,
            world_debug: false,
            server_trips: Vec::new(),
            level_teleports: Vec::new(),
            teleport_notes: Vec::new(),
            levels: Default::default(),
            level_pass: Default::default(),
            portals: Default::default(),
            respawn_config: None,
            end_portal_travel: false,
            win: None,
            end: Default::default(),
            block_ticks: Ticks::new(0),
            tick_scratch: TickScratch::default(),
            tick_chunks: Default::default(),
            tickets: Default::default(),
            random_tick_sections: Default::default(),
            level_events: Vec::new(),
            falling: Default::default(),
            transient_blocks: Default::default(),
            paintings: Vec::new(),
            painting_variants: prepared.painting_variants,
            item_frames: Vec::new(),
            frame_model: prepared.frame_model,
            glow_frame_model: prepared.glow_frame_model,
            map_frame_model: prepared.map_frame_model,
            glow_map_frame_model: prepared.glow_map_frame_model,
            armor_stands: Default::default(),
            location_armor: Default::default(),
            cushions: Vec::new(),
            others: Vec::new(),
            armor_stand_assets: prepared.armor_stand_assets,
            waypoint_styles: prepared.waypoint_styles,
            waypoints: Default::default(),
            locator_camera: None,
            native_camera_frame: None,
            posed_rotation: None,
            post_effects: Vec::new(),
            polytone_post_ticks: 0.0,
            spectated_post_effect: None,
            experience_display: Default::default(),
            boats: Default::default(),
            leash_boats: Vec::new(),
            boat_models: prepared.boat_models,
            crystal_model: prepared.crystal_model,
            cushion_model: prepared.cushion_model,
            entity_models: prepared.entity_models,
            carts: Default::default(),
            watched_vehicles: Vec::new(),
            native_moves: Vec::new(),
            jukebox_sounds: HashMap::new(),
            bolts: Vec::new(),
            maps: maps::Maps::default(),
            fight: dragon::Fight::default(),
            dragon: None,
            crystals: Vec::new(),
            breath_clouds: Vec::new(),
            dragon_model: prepared.dragon_model,
            map_patches: Vec::new(),
            name_tag_layouts: Default::default(),
            text_displays: Default::default(),
            interactions: Default::default(),
            remote_knots: Default::default(),
            knot_ids: Default::default(),
            text_display_vertices: 0,
            skin_downloads: RefCell::new({
                let mut downloads = horizon_client::skins::Downloader::new(
                    std::env::current_dir()
                        .unwrap_or_default()
                        .join("assets/skins"),
                );
                downloads.set_resource_root(root.clone());
                downloads
            }),
            skin_profiles: RefCell::new(horizon_client::skins::Resolver::new()),
            skin_slots: horizon_client::skins::SkinSlots::new(horizon_client::hud::SKIN_SLOTS),
            cosmetics: Default::default(),
            emotes: Default::default(),
            emote_wheel: Default::default(),
            skin_3d: Default::default(),
            trim_slots: Default::default(),
            cart_models: prepared.cart_models,
            riding: None,
            started_riding: Vec::new(),
            rotation_forced: false,
            posed_rider: None,
            pending_vehicle: None,
            ender_items: vec![None; 27],
            ender_open: None,
            pending: Default::default(),
            spill: Default::default(),
            recipe_ui: Default::default(),
            mount_menu: Default::default(),
            hook: None,
            remote_hooks: Vec::new(),
            tridents: Default::default(),
            rockets: Default::default(),
            ominous_spawners: Vec::new(),
            creaking: Default::default(),
            spawners: Default::default(),
            spawn_scratch: Default::default(),
            trident_model: prepared.trident_model,
            arrow_model: prepared.arrow_model,
            bee_stinger_model: prepared.bee_stinger_model,
            tnts: Default::default(),
            hoppers: HashMap::new(),
            mod_screen: None,
            mod_container_viewers: HashMap::new(),
            mod_chunk_tickets: HashMap::new(),
            item_columns: None,
            pickup_columns: None,
            pass_mobs: None,
            spawner_tick_positions: Vec::new(),
            trial_spawner_tick_positions: Vec::new(),
            creative_screen: prepared.creative_screen,
            creative_ignore_text: false,
            control: None,
            agent_control_on: false,
            agent_last: None,
            frame_stats: Default::default(),
            perf: Default::default(),
            pending_screenshot: None,
            screenshot_sequence: None,
            pending_render_capture: None,
            pending_bench: None,
            flight: None,
            ui_wait: Default::default(),
            events: Default::default(),
            input_trace: Default::default(),
            repro: Default::default(),
            tick_script: tick_script::TickScript::from_env(),
            agent: agent::Agent::default(),
            view_frustum: None,
            entity_section_visibility_valid: false,
            entity_section_camera: None,
            sync_occlusion_culler: None,
            async_occlusion_culler: None,
            async_occlusion_culling_failed: false,
            async_particle_culler: RefCell::new(Default::default()),
            pack_shadow_casters: (false, false),
            pending_uploads: Vec::new(),
            frame_graph: Default::default(),
            extras: Default::default(),
            pvp: Default::default(),
            mods: Default::default(),
            recipe_tables: Default::default(),
            panel: Default::default(),
            native_shaders: Default::default(),
            native_input: Default::default(),
            native_lifecycle: Default::default(),
            native_configs: Default::default(),
            native_net: Default::default(),
            native_player_fx: Default::default(),
            native_packets: Default::default(),
            native_mod_screen: None,
            sky_darken_memo: Default::default(),
            water_plane_memo: Default::default(),
            water_plane_scan: Default::default(),
            vehicle_mob_reach: Default::default(),
            persist: Default::default(),
            entity_attachments: Default::default(),
            remote: None,
            realm_session: false,
            studio_replay: None,
            studio_freecam: Default::default(),
            studio_timeline: Default::default(),
            studio_topbar: Default::default(),
            studio_rmb_down: false,
            studio_camera_track_dirty: false,
            studio_replay_previous_camera_type: None,
            studio_replay_standalone: false,
            last_replay_tick: None,
            studio_virtual_tick_ms: 0.0,
            lan: None,
            integrated: None,
            level_shapes: Default::default(),
            serving: None,
            world_options: None,
            options_to_world_options: false,
            options_to_game: false,
            command_block_screen: None,
            block_editor: None,
            structure_templates: HashMap::new(),
            test_defs: std::cell::RefCell::new(HashMap::new()),
            test_defs_snapshot: false,
            test_highlights: HashMap::new(),
            game_tests: game_tests::GameTests::default(),
            #[cfg(feature = "friends")]
            friends: None,
            social: Default::default(),
            debug_keys: Default::default(),
            debug_charts: Default::default(),
            debug_entries: horizon_client::debug_entries::DebugEntries::load(
                &std::env::current_dir().unwrap_or_default(),
            ),
            debug_options: None,
            memory_meter: Default::default(),
            debug_paint: Default::default(),
            debug_refresh: Default::default(),
            gpu_utilization: 0.0,
            gpu_culling_auto: horizon_client::autotune::GpuCullingAuto::new(),
            rendered_mobs: Default::default(),
            screen_blur_requested: Default::default(),
            spawn_counts: HashMap::new(),
            pause_menu: true,
            pause_focus: None,
            death_focus: None,
            focus_screen: 0,
            previous_mode: None,
            gamemode_switcher: None,
            clipboard: Default::default(),
            text_selection: Default::default(),
            saved_for_exit: false,
            native_client_stopped: false,
            pending_save: None,
            server_tick_deadline: None,
            frame_drawn: true,
            world_icon: None,
            forced_hit: None,
            tick_hit: None,
            tick_mob: None,
            hotbar_presses: Vec::new(),
            entity_reach: 3.0,
            block_reach: 4.5,
            guest_use: None,
            use_failed: false,
            guest_inventory_resend: false,
            lan_events: Vec::new(),
            lan_songs: Vec::new(),
            lan_cushion_breaks: Vec::new(),
            lan_block_events: Vec::new(),
            lan_entity_events: Vec::new(),
            lan_critical_hits: Vec::new(),
            lan_kinetic_hits: Vec::new(),
            stabbed: HashMap::new(),
            stab_players: Vec::new(),
            player_stabs: Vec::new(),
            known_movement: Vec3::ZERO,
            kinetic_feedback: HashMap::new(),
            lan_global_events: Vec::new(),
            lid_openers: HashMap::new(),
            lan_acting: None,
            native_commands_refresh: false,
            lan_building_boxes: Default::default(),
            save_after_guest_turn: false,
            player_attachments: Default::default(),
            guest_attachments: HashMap::new(),
            lan_break_pos: None,
            break_collect: None,
            guest_packets: Vec::new(),
            guest_cooldowns: Vec::new(),
            native_resend_trees: Vec::new(),
            guest_entity_events: Vec::new(),
            guest_equipment_breaks: Vec::new(),
            guest_block_sounds: Vec::new(),
            guest_player_sounds: Vec::new(),
            lan_acting_name: None,
            lan_acting_uuid: None,
            native_click_cursor: None,
            tnt_refused: None,
            lan_acting_permission: None,
            lan_acting_spawn_protection: None,
            lan_acting_entity: None,
            native_login_permission: None,
            chat_capture: None,
            admin_feedback: Vec::new(),
            console_broadcast: None,
            rcon_source: false,
            server_lists: None,
            world_tables: None,
            cmds: Default::default(),
            ui: chat::UiState::new(),
            combat: Default::default(),
            guest_combat: Default::default(),
            mount: Default::default(),
            stats: Default::default(),
            score: Default::default(),
            fns: Default::default(),
            bosses: Default::default(),
            boss_overlay: Default::default(),
            exec: Default::default(),
            rand_sequences: Default::default(),
            brushing: Default::default(),
            border: Default::default(),
            border_renderer: Default::default(),
            border_section: None,
            border_state: Default::default(),
            tab_list: Default::default(),
            subtitles: Default::default(),
            adv: Default::default(),
            restore_player_state: true,
            stats_ui: Default::default(),
            adv_ui: Default::default(),
            links_ui: Default::default(),
            camera_type: Default::default(),
            fangs: Default::default(),
            player_model: None,
        };
        if game.native_window() {
            game.native_client_start();
        }
        Ok(game)
    }
}

impl Game<'_> {}

/// `key.hotbar.1` to `key.hotbar.9` in a menu.
const HOTBAR_DIGITS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];
/// The other keys `InputConstants.Key.getNumericKeyValue` gives a digit.
const NUMERIC_KEYS: [KeyCode; 11] = [
    KeyCode::Digit0,
    KeyCode::Numpad0,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
];

fn load_world_rest(
    root: &std::path::Path,
    resources: &Resources,
    assets: &Assets,
    behaviour_json: &serde_json::Value,
    gameplay_projection: &[horizon_client::blocks::GameplayProjection],
    entity_models: Option<&horizon_client::mobs::EntityModelDocument>,
    item_defaults: &horizon_world::item_defaults::Defaults,
    visibility: horizon_world::block_render::FaceVisibility,
    progress: &Progress,
    started: Instant,
    // False on a dedicated server, which draws nothing: no block entity
    // renderer or animate tables (client-side display ticks).
    draws: bool,
) -> Result<WorldRest> {
    let facts = resources.facts();
    let biomes = resources.biomes();
    let threads = resources.threads();
    let registry = assets.registry;
    let overlay = &horizon_world::loot::PackOverlay::NONE;
    progress.set(6);
    // Independent tables load together, within the worker budget the settings
    // allow. Only the recipes and the stations read another table: the items.
    let load_items = || -> Result<_> {
        let items = startup::time("rest.items", || Items::load(root, registry, facts, overlay))?;
        log::info!("load items: {:.2}s", started.elapsed().as_secs_f32());
        progress.set(7);
        // The recipe tables, stations and recipe book share one filtered source
        // set so the same JSON files are decoded only once.
        let recipe_sources =
            startup::time("rest.recipe_sources", || Recipes::sources(root, overlay))?;
        let stations = startup::time("rest.stations", || {
            horizon_client::stations::StationData::load_with_recipe_sources(
                root,
                &items,
                overlay,
                None,
                &recipe_sources,
            )
        });
        let recipes = startup::time("rest.recipes", || {
            Recipes::from_sources(root, &items, overlay, recipe_sources)
        })?;
        Ok((items, recipes, stations))
    };
    let load_behaviours = || {
        startup::time("rest.behaviours", || {
            Behaviours::load_with_gameplay(registry, root, behaviour_json, gameplay_projection)
        })
    };
    let load_block_entity_render = || -> Result<_> {
        let _step = startup::step("rest.block_entity_render");
        if !draws {
            return Ok(Default::default());
        }
        let mut renderer = BlockEntityRenderer::load_with_visibility_models_and_defaults(
            root,
            registry,
            visibility,
            entity_models.as_ref().map(|document| document.value()),
            item_defaults,
        )?;
        renderer.add_mod_signs(facts, registry);
        Ok(renderer)
    };
    let load_animate = || -> Result<_> {
        let _step = startup::step("rest.animate_fog");
        let animate = if draws {
            horizon_client::animate_tick::AnimateTables::load(root, registry, biomes)?
        } else {
            Default::default()
        };
        Ok((animate, horizon_client::fog::FogTables::load(root, biomes)))
    };
    let load_mobs = || -> Result<_> {
        let _step = startup::step("rest.mobs");
        let mob_types: HashMap<_, _> = if draws {
            match entity_models {
                Some(document) => {
                    MobTypes::load_with_document_and_defaults(root, document, item_defaults)?
                }
                None => MobTypes::load_with_defaults(root, item_defaults)?,
            }
        } else {
            MobTypes::load_for_server(root)?
        }
        .types
        .into_iter()
        .map(|(k, v)| (k, Arc::new(v)))
        .collect();
        let mut mob_data = horizon_client::mobs::MobData::load(root, facts);
        mob_data.cem = std::sync::RwLock::new(Arc::new(if draws {
            horizon_client::mobs::cem::CemModels::load(root, &mob_types)
        } else {
            horizon_client::mobs::cem::CemModels::default()
        }));
        horizon_client::mobs::trial_spawner::load_registry(root);
        Ok((mob_types, mob_data))
    };
    let load_transformers = || -> Result<_> {
        let _step = startup::step("rest.transformers_models");
        let transformers = horizon_client::block_transform::Transformers::load(root, registry)?;
        let player_type = match entity_models {
            Some(document) => perspective::player_type_with_document(root, document),
            None => perspective::player_type(root),
        };
        let models = (
            player_type,
            player_arms(root, "minecraft:player#main", entity_models),
            player_arms(root, "minecraft:player_slim#main", entity_models),
            match entity_models {
                Some(document) => document.model("minecraft:spin_attack#main"),
                None => horizon_client::mobs::load_model(root, "minecraft:spin_attack#main"),
            },
        );
        Ok((transformers, models))
    };
    // The errors cross threads as text.
    fn text<T, E: std::fmt::Display>(
        result: std::result::Result<T, E>,
    ) -> std::result::Result<T, String> {
        result.map_err(|e| e.to_string())
    }
    let (
        items,
        behaviours,
        block_entity_render,
        animate,
        mobs,
        transformers,
        providers,
        music,
        ambient,
        nether_environment,
        end_environment,
    ) = if threads > 1 {
        std::thread::scope(|scope| {
            let items = named_scope(scope, "items", || text(load_items()));
            let behaviours = named_scope(scope, "behaviours", || text(load_behaviours()));
            let block_entity_render = named_scope(scope, "block_entity_render", || {
                text(load_block_entity_render())
            });
            let animate = named_scope(scope, "animate", || text(load_animate()));
            let mobs = named_scope(scope, "mobs", || text(load_mobs()));
            let transformers = named_scope(scope, "transformers", || text(load_transformers()));
            let providers = named_scope(scope, "providers", || {
                startup::time("rest.providers", || Providers::load(root))
            });
            let music = named_scope(scope, "music", || {
                startup::time("rest.music", || {
                    horizon_client::music::MusicTables::load(root)
                })
            });
            let ambient = named_scope(scope, "ambient", || {
                startup::time("rest.ambient", || {
                    horizon_client::ambience::AmbientTables::load(root)
                })
            });
            let nether_environment = named_scope(scope, "nether_environment", || {
                startup::time("rest.nether_environment", || {
                    text(Environment::load_dimension(root, "the_nether"))
                })
            });
            let end_environment = named_scope(scope, "end_environment", || {
                startup::time("rest.end_environment", || {
                    text(Environment::load_dimension(root, "the_end"))
                })
            });
            (
                items.join().unwrap(),
                behaviours.join().unwrap(),
                block_entity_render.join().unwrap(),
                animate.join().unwrap(),
                mobs.join().unwrap(),
                transformers.join().unwrap(),
                providers.join().unwrap(),
                music.join().unwrap(),
                ambient.join().unwrap(),
                nether_environment.join().unwrap(),
                end_environment.join().unwrap(),
            )
        })
    } else {
        (
            text(load_items()),
            text(load_behaviours()),
            text(load_block_entity_render()),
            text(load_animate()),
            text(load_mobs()),
            text(load_transformers()),
            Providers::load(root),
            startup::time("rest.music", || {
                horizon_client::music::MusicTables::load(root)
            }),
            startup::time("rest.ambient", || {
                horizon_client::ambience::AmbientTables::load(root)
            }),
            startup::time("rest.nether_environment", || {
                text(Environment::load_dimension(root, "the_nether"))
            }),
            startup::time("rest.end_environment", || {
                text(Environment::load_dimension(root, "the_end"))
            }),
        )
    };
    let (items, recipes, stations) = items?;
    let (animate, fog) = animate?;
    let (mob_types, mob_data) = mobs?;
    let (transformers, (player_type, player_arm, player_arm_slim, spin_attack)) = transformers?;
    let nether_environment = nether_environment?;
    let end_environment = end_environment?;
    log::info!(
        "Horizon: resources loaded in {:.1}s; {} block states, {} items, {} biomes; {} worker threads",
        started.elapsed().as_secs_f32(),
        facts.states.len(),
        items.items.len(),
        biomes.list.len(),
        threads
    );
    progress.set(8);
    Ok(WorldRest {
        stations,
        items,
        recipes,
        transformers,
        providers,
        block_entity_render: block_entity_render?,
        animate,
        fog,
        music,
        ambient,
        nether_environment,
        end_environment,
        behaviours: behaviours?,
        mob_types,
        mob_data,
        player_type,
        player_arm,
        player_arm_slim,
        spin_attack,
    })
}

/// A title-shaped frame: black clear and panorama, only the HUD is drawn.
/// The window's `HWND`, for the boot screen's GDI paint (`horizon_threads`):
/// the raw handle read is safe, the paint is not.
#[cfg(target_os = "windows")]
fn hwnd(window: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    match window.window_handle() {
        Ok(raw) => match raw.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
            _ => None,
        },
        Err(_) => None,
    }
}

pub use launch_api::{UsageError, exit_code};

pub fn run_with() -> Result<()> {
    let Some(mut options) = Options::read()? else {
        return Ok(());
    };
    if options.execution_role == execution_role::ExecutionRole::DedicatedServer
        && !dedicated::preflight(&mut options)?
    {
        return Ok(());
    }
    // The packets the server sends its players reach the mods that ask (native_packets.rs).
    let _ = horizon_client::lan::conn::SERVER_PACKET.set(Game::native_server_packet);
    let _ = horizon_client::lan::conn::ENTITY_DATA_TO.set(Game::native_entity_data_to);
    let _ = horizon_client::health::HURT_SERVER.set(native_hooks::hurt_server);
    let _ = horizon_client::health::DAMAGE_PRE.set(native_hooks::damage_pre);
    let _ = horizon_client::health::DAMAGE_POST.set(native_hooks::damage_post);
    let _ = horizon_client::block_contact::HOOK.set(native_hooks::block_contact);
    let _ = horizon_client::health::KNOCKBACK.set(native_hooks::knockback);
    let _ = horizon_client::lan::hub::PLAYER_MOVE.set(Game::native_player_move);
    let _ = horizon_client::lan::hub::PLAYER_TELEPORT.set(Game::native_player_teleport);
    let _ = horizon_client::block_contact::TRAMPLE.set(Game::native_trample);
    // Before any worker starts: on Linux they go behind the server thread
    // and the render thread.
    horizon_threads::put_helpers_behind();
    if options.execution_role.is_client() {
        // The render thread, like the server thread, ahead of the workers on
        // macOS and Windows. While terrain loads, the workers
        // (horizon_threads::jobs) and other programs outnumber the cores, and a frame that gave its core up (in a GPU submit) waited
        // 10 to 200 ms to get one back.
        horizon_threads::favour_current();
    }
    // A dedicated server too: vanilla's "Server Shutdown Thread" (`DedicatedServer.initServer`'s
    // shutdown hook) halts the server and saves its players and levels on SIGTERM, SIGINT or SIGHUP.
    if options.execution_role.is_client()
        || options.execution_role == execution_role::ExecutionRole::DedicatedServer
    {
        horizon_threads::catch_stop_signals();
    }
    let started = Instant::now();
    // A dedicated server writes its slow ticks to the log with the spans
    // they spent their time in (`HORIZON_SPIKES`'s line; the variable still
    // sets another threshold), so a lag spike players felt can be traced
    // from the log alone. Timing the spans costs microseconds a tick.
    if options.spikes.is_none()
        && options.execution_role == execution_role::ExecutionRole::DedicatedServer
    {
        options.spikes = Some(dedicated::SLOW_TICK_MS);
    }
    tick_profile::want_spans(tick_profile::SPANS_SPIKES, options.spikes.is_some());
    // A player who allows performance statistics also says what their slow frames spent their
    // time in: this thread, which draws the frames, times its steps (wall time only).
    if options.execution_role.is_client()
        && horizon_telemetry::Consent::load(&horizon_client::logging::game_home())
            .allows(horizon_telemetry::Kind::Perf)
    {
        horizon_client::spans::record_here(true);
    }
    let vanilla_root = options.resources.clone();
    // Saves from before 1.13 upgrade with vanilla's tables, prepared from the player's jar.
    horizon_world::datafix::set_resources(
        &std::path::absolute(&vanilla_root).unwrap_or_else(|_| vanilla_root.clone()),
    );
    if let Some(error) = missing_resources(&vanilla_root) {
        // The game build started by hand (the release's zip), not by the
        // launcher: a window says where to get it. Scripts and servers get
        // the error.
        if !options.offscreen && !synchronous_startup(&options) {
            log::error!("Horizon: {error}");
            let settings = Settings::load(&Game::options_path());
            return no_resources::show(&vanilla_root, &settings);
        }
        return Err(error.into());
    }
    // Someone is playing: the anonymous ping behind the launcher's "N playing now", and the
    // official server in their list (servers.dat in the game folder, the working directory). Not
    // for a server, a benchmark, a run that quits by itself, a harness's fixture or a run that
    // horizon-ctl drives: those start in a source checkout and wrote the list there.
    if !options.offscreen && !synchronous_startup(&options) {
        let version = horizon_client::telemetry::version();
        horizon_hooks::engine::started(&horizon_hooks::Started::new(&version));
        let player_dir = horizon_client::logging::player_dir();
        // horizon-extras: the official server is Horizon's (docs/forking.md).
        if cfg!(feature = "horizon-extras")
            && let Err(e) = horizon_client::multiplayer::add_official_server(&player_dir)
        {
            log::warn!("could not add the official server to servers.dat: {e}");
        }
        if let Err(e) = horizon_client::multiplayer::withdraw_partner_servers(&player_dir) {
            log::warn!("could not take the withdrawn partner server out of servers.dat: {e}");
        }
    }
    mods::register_components(&vanilla_root)?;
    mods::register_standalone_models();
    mods::register_layer_definitions();
    let mut early_settings = Settings::load(&Game::options_path());
    // The player's Polytone pack options (#11486), before any pack is read.
    horizon_client::packs::set_polytone_choices(horizon_client::packs::load_polytone_choices(
        &horizon_client::packs::polytone_choices_file(),
    ));
    // Minecraft.<init>: read the last start's flag, then mark this one as not
    // finished until onGameLoadFinished. A dedicated server has no options.txt.
    if options.execution_role.is_client() {
        if unclean_start_recovery(&mut early_settings) {
            log::warn!(
                "Detected unexpected shutdown during last game startup: resetting fullscreen mode"
            );
        }
        early_settings.file.set(
            "startedCleanly",
            horizon_client::options_file::V::Bool(false),
        );
        early_settings.save(&Game::options_path());
    }
    // Minecraft's constructor opens the narrator; a server has none.
    if options.execution_role.is_client() {
        narrator::start(&early_settings);
    }
    // "Playing Horizon" on Discord: never for a server; an automated run only talks to a test's fake Discord.
    #[cfg(feature = "discord")]
    if options.dedicated.is_none() {
        let automated = options.benchmark.is_some()
            || options.offscreen
            || options.frames.is_some()
            || options.world_frames.is_some();
        horizon_client::discord_presence::start(early_settings.extras.discord_presence, automated);
    }
    // Snapshots of derived tables live in the game folder, never beside the
    // shared resources.
    Behaviours::set_cache_dir(std::env::current_dir()?.join(".horizon-cache"));
    if !synchronous_startup(&options) {
        return run_loading(options, early_settings, vanilla_root, None, started);
    }
    let mut pack_error = None;
    let base = mods::content_root(
        &vanilla_root,
        &early_settings.mods.enabled,
        &options.dev_mods,
    );
    options.resources =
        packs::startup_root(&vanilla_root, &base, &early_settings.resource_packs, &[])
            .unwrap_or_else(|e| {
                log::warn!("Horizon: resource packs not applied: {e}");
                pack_error = Some(e.to_string());
                base.clone()
            });
    let loaded = load_resources(
        options.resources.clone(),
        launch_world(&options),
        &vanilla_root,
        &early_settings,
        started,
        false,
        options.dedicated.is_some(),
        &Progress::new(),
        &mut |_, _| {},
    )?;
    let gui = loaded.gui.ok_or("missing menu bundle")?;
    options.resources = loaded.root.clone();
    let audio = if !options.execution_role.is_client() || options.headless {
        Audio::silent(&options.resources)
    } else {
        Audio::with_device(&options.resources, &early_settings.sound_device)
    };
    let resources = loaded.resources;
    let game = start_game(
        &resources,
        loaded.pack,
        gui,
        options,
        loaded.pack_error.or(pack_error),
        audio,
        vanilla_root,
        started,
        loaded.rest_loader,
        None,
        None,
        loaded.entity_models,
    )?;
    if let Some(mut game) = game {
        // Minecraft.onGameLoadFinished: a start that made it this far is clean.
        if game.options.execution_role.is_client() {
            game.settings.file.set(
                "startedCleanly",
                horizon_client::options_file::V::Bool(true),
            );
            game.settings.save(&Game::options_path());
        }
        run_event_loop(game)?;
    }
    Ok(())
}

/// The right and left arms of a player model layer (`PlayerModel.rightArm`
/// and `leftArm`, with their sleeves).
fn player_arms(
    root: &std::path::Path,
    key: &str,
    document: Option<&horizon_client::mobs::EntityModelDocument>,
) -> [Option<horizon_client::mobs::Part>; 2] {
    let model = match document {
        Some(document) => document.model(key),
        None => horizon_client::mobs::load_model(root, key),
    };
    ["right_arm", "left_arm"].map(|name| {
        model
            .as_ref()
            .and_then(|m| m.children.iter().find(|c| c.name == name).cloned())
    })
}

// Player.tick: cap the sleeping fade at 100, then fade out over 10 waking ticks.
fn tick_sleep_timer(timer: u32, sleeping: bool) -> u32 {
    if sleeping {
        (timer + 1).min(100)
    } else if (1..109).contains(&timer) {
        timer + 1
    } else {
        0
    }
}

/// Where a ray entering `pos` first meets the block's outline boxes, if it
/// does: a hollow block (torch, fence, flower) lets the ray through.
fn outline_stop(
    origin: [f64; 3],
    dir: [f64; 3],
    reach: f64,
    pos: [i32; 3],
    face: [i32; 3],
    boxes: impl FnOnce() -> Vec<[f64; 6]>,
) -> Option<Hit> {
    let (t, face) = interaction_shapes::entry(origin, dir, &boxes(), reach, face)?;
    Some(Hit {
        pos,
        face,
        point: std::array::from_fn(|i| origin[i] + dir[i] * t),
    })
}
/// `VoxelShape.clip` of a fluid source's shape for a ray that starts in the
/// source's own block `cell`: the face and point it hits, in `pick_fluid`'s
/// `Hit` terms. `FlowingFluid.getShape` is the whole block under the same
/// fluid and `getOwnHeight` (8/9) tall otherwise. A start inside the shape
/// (tested 1e-7 of the way along) hits at the start, on the face opposite
/// `Direction.getApproximateNearest(dir)`; from above it, a ray going down
/// enters through the top while still in this block.
fn clip_start_source(
    eye: [f64; 3],
    dir: [f64; 3],
    reach: f64,
    cell: [i32; 3],
    same_above: bool,
) -> Option<([i32; 3], [f64; 3])> {
    let height = if same_above {
        1.0
    } else {
        f64::from(8.0f32 / 9.0f32)
    };
    let probe_y = eye[1] + dir[1] * reach * 1.0e-7;
    if probe_y - f64::from(cell[1]) < height {
        // Direction.values() order; the first of equal dot products wins.
        let normals: [[i32; 3]; 6] = [
            [0, -1, 0],
            [0, 1, 0],
            [0, 0, -1],
            [0, 0, 1],
            [-1, 0, 0],
            [1, 0, 0],
        ];
        let mut nearest = normals[2];
        let mut best = f64::NEG_INFINITY;
        for n in normals {
            let dot =
                f64::from(n[0]) * dir[0] + f64::from(n[1]) * dir[1] + f64::from(n[2]) * dir[2];
            if dot > best {
                best = dot;
                nearest = n;
            }
        }
        return Some((nearest.map(|v| -v), eye));
    }
    if dir[1] >= 0.0 {
        return None;
    }
    let t = (f64::from(cell[1]) + height - eye[1]) / dir[1];
    if t > reach {
        return None;
    }
    let point = [
        eye[0] + dir[0] * t,
        eye[1] + dir[1] * t,
        eye[2] + dir[2] * t,
    ];
    (point[0].floor() as i32 == cell[0] && point[2].floor() as i32 == cell[2])
        .then_some(([0, 1, 0], point))
}

fn reset_world_progress(adv: &mut advancements::AdvState, stats: &mut stats::Stats) {
    *adv = Default::default();
    *stats = Default::default();
}

#[cfg(test)]
mod startup_cpu_tests;

#[cfg(test)]
mod runtime_reset_tests;

#[cfg(test)]
mod entity_model_loading_tests;

/// `LocalPlayer.moveTowardsClosestSpace` at the body's four corners (aiStep, unless a spectator or a
/// passenger): a corner inside a suffocating block sets a 0.1 speed towards the nearest side that is
/// free, on that side's axis; a later corner overwrites an earlier one. Suffocating is a full
/// collision cube that is also a full render cube, as `in_wall` has it.
pub(crate) fn closest_space_nudge(
    p: &PlayerMotion,
    world: &World,
    facts: &BlockFacts,
) -> [Option<f64>; 2] {
    use horizon_core::geometry::Axis;
    let mut out = [None; 2];
    let body = p.bounds();
    let (min_y, max_y) = (body.min(Axis::Y) + 1.0e-7, body.max(Axis::Y) - 1.0e-7);
    let suffocates_at = |x: i32, z: i32| {
        ((min_y.floor() as i32 - 1)..=(max_y.ceil() as i32 + 1)).any(|y| {
            f64::from(y) < max_y && f64::from(y + 1) > min_y && {
                let f = facts.get(world.block(x, y, z));
                f.full_collision && f.solid_render
            }
        })
    };
    let side = f64::from(p.dimensions.width) * 0.35;
    let (px, pz) = (p.position.x, p.position.z);
    for (x, z) in [
        (px - side, pz + side),
        (px - side, pz - side),
        (px + side, pz - side),
        (px + side, pz + side),
    ] {
        let (bx, bz) = (x.floor() as i32, z.floor() as i32);
        if !suffocates_at(bx, bz) {
            continue;
        }
        let (xd, zd) = (x - f64::from(bx), z - f64::from(bz));
        // WEST, EAST, NORTH, SOUTH: (axis, step, distance to that edge).
        let mut closest = 1.797_693_134_862_315_7e308; // Double.MAX_VALUE
        let mut chosen = None;
        for (axis, step, distance) in [(0, -1, xd), (0, 1, 1.0 - xd), (1, -1, zd), (1, 1, 1.0 - zd)]
        {
            let (nx, nz) = if axis == 0 {
                (bx + step, bz)
            } else {
                (bx, bz + step)
            };
            if distance < closest && !suffocates_at(nx, nz) {
                closest = distance;
                chosen = Some((axis, step));
            }
        }
        if let Some((axis, step)) = chosen {
            out[axis] = Some(0.1 * f64::from(step));
        }
    }
    out
}

use subtitles::{Subtitles, in_game_ui, subtitles};

mod hud_render;

use hud_render::{block_outline_drawn, debug_crosshair_drawn, is_game_master_block};
mod subtitles;

#[cfg(test)]
#[path = "game_tests.rs"]
mod tests;

fn write_level_for_role(
    level: &mut Level,
    dir: &std::path::Path,
    role: execution_role::ExecutionRole,
) -> Result<()> {
    if role.writes_singleplayer_player() {
        level.write(dir)
    } else {
        level.write_without_player(dir)
    }
}

/// A landing's block game event, once the write it belongs to is applied.
struct LandingVibration {
    /// `minecraft:block_change` or `minecraft:block_destroy`.
    event: &'static str,
    position: BlockPos,
    /// The state the event affects (`GameEvent.Context.affectedState`).
    state: BlockStateId,
    /// Whether the entity that landed is its source
    /// (`GameEvent.Context.of(entity, state)`), else `Context.of(state)`.
    from_entity: bool,
}
#[cfg(test)]
mod allay_pickup_cooldown_tests;
#[cfg(test)]
mod dragon_fireball_motion_tests;
#[cfg(test)]
mod fake_player_tick_tests;
#[cfg(test)]
mod fence_leash_force_tests;
#[cfg(test)]
mod golem_defense_tests;
#[cfg(test)]
mod hoglin_repeat_attack_tests;
#[cfg(test)]
mod inline_feature_parse_tests;
#[cfg(test)]
mod inline_loot_reference_tests;
#[cfg(test)]
mod interaction_data_tests;
#[cfg(test)]
mod invulnerable_time_tests;
#[cfg(test)]
mod kill_die_cancel_tests;
#[cfg(test)]
mod kill_die_vibration_tests;
#[cfg(test)]
mod last_hurt_by_mob_tests;
#[cfg(test)]
mod loot_kill_guest_tests;
#[cfg(test)]
mod mannequin_save_tests;
#[cfg(test)]
mod mob_landing_support_tests;
#[cfg(test)]
mod orb_data_tests;
#[cfg(test)]
mod orb_score_store_tests;
#[cfg(test)]
mod recipe_book_variants_tests;
#[cfg(test)]
mod spawn_yaw_tests;
#[cfg(test)]
mod stand_break_loot_tests;
#[cfg(test)]
mod stand_command_hurt_tests;
#[cfg(test)]
mod storage_alias_tests;
#[cfg(test)]
mod sulfur_bucket_tests;
#[cfg(test)]
mod sulfur_prime_phase_tests;
#[cfg(test)]
mod test_pos_guest_tests;
#[cfg(test)]
mod vibration_source_tests;
#[cfg(test)]
mod vibration_tests;

#[cfg(test)]
mod falling_air_tick_tests;

#[cfg(test)]
mod player_save_projection_tests;

#[cfg(test)]
mod entity_die_tests;
