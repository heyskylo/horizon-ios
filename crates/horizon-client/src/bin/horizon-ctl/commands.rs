//! Every command horizon-ctl knows: its usage, help, examples and arguments.
//! The same table prints `help`, checks `COMMAND --help` and describes the
//! MCP tools, so the CLI and the MCP server always offer the same commands.
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Int,
    Num,
    Str,
    Bool,
    /// One of these words.
    Enum(&'static [&'static str]),
    /// The rest of the line, spaces included.
    Rest,
}

#[derive(Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    /// Sent as `--name value` (`--name` alone for booleans), not by position.
    pub flag: bool,
    pub help: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Session,
    Observe,
    Move,
    Act,
    Screens,
    Multiplayer,
    World,
    Scripting,
}
impl Group {
    pub const ALL: [Group; 8] = [
        Group::Session,
        Group::Observe,
        Group::Move,
        Group::Act,
        Group::Screens,
        Group::Multiplayer,
        Group::World,
        Group::Scripting,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Group::Session => "Session",
            Group::Observe => "Observe (JSON state)",
            Group::Move => "Move and look",
            Group::Act => "Act on the world",
            Group::Screens => "Screens, menus and chat",
            Group::Multiplayer => "Multiplayer, LAN and friends",
            Group::World => "World and cheats",
            Group::Scripting => "Scripting and benchmarks",
        }
    }
}

/// One field of a command's JSON reply. `ty` is a JSON type (`string`, `integer`, `number`,
/// `boolean`, `array`, `object`); `a|null` marks a field that is `null` when it does not apply.
#[derive(Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    pub ty: &'static str,
    pub help: &'static str,
}

const fn f(name: &'static str, ty: &'static str, help: &'static str) -> Field {
    Field { name, ty, help }
}

/// `hello`: always these three fields.
const HELLO_REPLY: &[Field] = &[
    f(
        "protocol",
        "string",
        "the control protocol, MAJOR.MINOR.PATCH (docs/api/stability.md)",
    ),
    f(
        "release",
        "string",
        "the game's version (MCP serverInfo carries the same string)",
    ),
    f(
        "features",
        "array",
        "the command groups the game answers, each a string",
    ),
];

/// `status`: the top-level fields the reply always carries. It holds more (sounds,
/// projectiles, maps and the like), so a reader must ignore the keys it does not know.
const STATUS_REPLY: &[Field] = &[
    f(
        "screen",
        "string",
        "the open screen (playing, inventory, paused...)",
    ),
    f(
        "debug",
        "object",
        "the F3 overlay flags: overlay, hitboxes, chunk_borders, pause_menu, advanced_tooltips, pause_on_lost_focus, clipboard",
    ),
    f("dimension", "string", "the dimension the player is in"),
    f("creative", "boolean", "creative mode"),
    f(
        "player",
        "object|null",
        "position, velocity (blocks), on_ground, health, max_health, food; null without a player",
    ),
    f("yaw", "number", "view yaw, degrees"),
    f("pitch", "number", "view pitch, degrees"),
    f(
        "target",
        "object|null",
        "the block under the crosshair as pos and block; null when none",
    ),
    f("selected", "integer", "the selected hotbar slot, 1 to 9"),
    f(
        "held",
        "object|null",
        "the held stack as item and count; null when empty",
    ),
    f(
        "offhand",
        "object|null",
        "the offhand stack as item, count and damage; null when empty",
    ),
    f(
        "keys",
        "array",
        "the keys the game holds down, each a string",
    ),
    f("fps", "number", "frames per second"),
    f("frames", "integer", "frames drawn so far"),
    f("ticks", "integer", "game ticks so far"),
    f("day_time", "integer", "the world's time of day, ticks"),
    f("uptime_seconds", "number", "seconds since the game started"),
    f("render_distance", "integer", "chunks"),
    f("chunks", "integer", "chunks loaded"),
    f("mobs", "integer", "mobs this side knows"),
    f("block_entities", "integer", "block entities loaded"),
    f(
        "pending_chunks",
        "integer",
        "chunks still being generated, meshed, uploaded or received",
    ),
    f(
        "streaming",
        "object",
        "chunk streaming stages and their counts",
    ),
    f(
        "lod",
        "object",
        "far terrain: distances, tiles, quads and sampling progress",
    ),
    f(
        "renderer",
        "object|null",
        "GPU, driver, backend, size, draw counts and GPU memory; null without a renderer",
    ),
    f(
        "display",
        "object|null",
        "the monitor: width, height, scale, refresh_hz; null without a window",
    ),
    f(
        "server",
        "object|null",
        "the connected server's address, name and state; null when not connected",
    ),
];

pub struct Cmd {
    pub name: &'static str,
    /// What the game receives instead of `name` (aliases such as look-at-block).
    pub wire: Option<&'static str>,
    pub group: Group,
    pub usage: &'static str,
    pub summary: &'static str,
    pub details: &'static str,
    pub examples: &'static [&'static str],
    pub params: &'static [Param],
    /// Offered as an MCP tool.
    pub mcp: bool,
    /// Runs in horizon-ctl itself, not in the game.
    pub local: bool,
    /// The reply's fields; empty until the command's reply is described (`REPLY_GROUP`).
    pub reply: &'static [Field],
}

const fn p(name: &'static str, ty: Ty, help: &'static str) -> Param {
    Param {
        name,
        ty,
        required: true,
        flag: false,
        help,
    }
}
const fn opt(name: &'static str, ty: Ty, help: &'static str) -> Param {
    Param {
        name,
        ty,
        required: false,
        flag: false,
        help,
    }
}
const fn flag(name: &'static str, ty: Ty, help: &'static str) -> Param {
    Param {
        name,
        ty,
        required: false,
        flag: true,
        help,
    }
}
const XYZ: [Param; 3] = [
    p("x", Ty::Num, "X coordinate"),
    p("y", Ty::Num, "Y coordinate"),
    p("z", Ty::Num, "Z coordinate"),
];
const BLOCK_XYZ: [Param; 3] = [
    p("x", Ty::Int, "block X"),
    p("y", Ty::Int, "block Y"),
    p("z", Ty::Int, "block Z"),
];
const OVER: Param = flag(
    "over",
    Ty::Num,
    "turn with mouse motion spread over this many seconds (eased), as a player would",
);
const FACE: Ty = Ty::Enum(&["down", "up", "north", "south", "west", "east"]);
const BOX: [Param; 6] = [
    p("x1", Ty::Int, "corner X"),
    p("y1", Ty::Int, "corner Y"),
    p("z1", Ty::Int, "corner Z"),
    p("x2", Ty::Int, "opposite X"),
    p("y2", Ty::Int, "opposite Y"),
    p("z2", Ty::Int, "opposite Z"),
];
const PAGES: Ty = Ty::Enum(&[
    "main",
    "video",
    "sound",
    "controls",
    "keybinds",
    "controller",
    "controller-binds",
    "mouse",
    "chat",
    "accessibility",
    "language",
    "font",
    "packs",
    "mods",
    "shader-packs",
    "graphics",
    "graphics-advanced",
    "graphics-hdr",
    "extras",
    "horizon",
    "agent",
    "video-vanilla",
]);

/// `--confirm`, required by destructive commands: the player agreed.
const CONFIRM: Param = Param {
    name: "confirm",
    ty: Ty::Bool,
    required: true,
    flag: true,
    help: "true once the player agreed: it cannot be undone",
};

/// MCP tools that delete or overwrite the player's things (`destructiveHint`).
pub const DESTRUCTIVE: &[&str] = &["world-delete", "fill", "setblock"];
/// MCP tools that reach other players or the network (`openWorldHint`): only when the player asks.
pub const OPEN_WORLD: &[&str] = &["connect", "lan", "friends", "chat", "command"];

macro_rules! cmd {
    ($name:expr, $group:ident, $usage:expr, $summary:expr, $details:expr, [$($ex:expr),* $(,)?], $params:expr, mcp: $mcp:expr, local: $local:expr, reply: $reply:expr) => {
        Cmd {
            reply: $reply,
            ..cmd!($name, $group, $usage, $summary, $details, [$($ex),*], $params, mcp: $mcp, local: $local)
        }
    };
    ($name:expr, $group:ident, $usage:expr, $summary:expr, $details:expr, [$($ex:expr),* $(,)?], $params:expr, mcp: $mcp:expr, local: $local:expr $(, wire: $wire:expr)?) => {
        Cmd {
            name: $name,
            wire: cmd!(@wire $($wire)?),
            group: Group::$group,
            usage: $usage,
            summary: $summary,
            details: $details,
            examples: &[$($ex),*],
            params: $params,
            mcp: $mcp,
            local: $local,
            reply: &[],
        }
    };
    (@wire) => { None };
    (@wire $w:expr) => { Some($w) };
}

pub const COMMANDS: &[Cmd] = &[
    // Session
    cmd!("launch", Session, "launch [--world NAME] [--seed N] [--creative] [--version TAG] [--game PATH] [--wait] [GAME ARGS...]",
        "start Horizon with the control socket and wait until it answers",
        "Finds the game in this order: --game PATH; a horizon-client next to horizon-ctl (a build folder, a release folder or a launcher install); the newest version installed by the Horizon launcher (--version TAG picks one). Launcher installs run with their prepared resources and keep saves in the game home ($HORIZON_HOME, else ~/.local/share/horizon). --wait also waits until the player has spawned and chunks are loaded. Other arguments go to the game (horizon-client --help lists them). The game's output goes to a .log file beside the socket.",
        ["horizon-ctl launch --world Survival --seed 42 --wait", "horizon-ctl launch --world Flat --creative --render-distance 8"],
        &[flag("world", Ty::Str, "world folder to open or create"), flag("seed", Ty::Str, "seed for a new world"), flag("creative", Ty::Bool, "creative mode for a new world"),
          flag("version", Ty::Str, "an installed launcher version tag such as v2026.10.13"), flag("game", Ty::Str, "path to a horizon-client binary"),
          flag("wait", Ty::Bool, "also wait until the world is ready (default in MCP)"),
          flag("offscreen", Ty::Bool, "render without showing a window, splitscreen's too (screenshots still work): locked or headless sessions"),
          flag("headless", Ty::Bool, "no display, window or GPU: a player that plays through these commands only (no screenshots)")],
        mcp: true, local: true),
    cmd!("wait-for", Session, "wait-for screen NAME|widget ID|loaded|chat TEXT|event KIND [--timeout S] [--since SEQ]", "wait until the game reaches a state, then reply",
        "screen takes the names state shows (playing, inventory, paused, dead, title...); widget an id from ui; loaded means a world is open, the player is in it and no chunk is pending; chat a new line containing TEXT; event a kind from events (death, respawn, item, screen...). chat and event count from --since SEQ (take last from events before acting), else from now. A timeout (default 30 s) is an error with kind timeout.",
        ["horizon-ctl wait-for screen inventory", "horizon-ctl wait-for loaded --timeout 120", "horizon-ctl wait-for chat joined the game", "horizon-ctl wait-for event death --timeout 600"],
        &[flag("timeout", Ty::Num, "seconds (default 30)"), flag("since", Ty::Int, "chat and event: count from this event seq"), p("condition", Ty::Enum(&["screen", "widget", "loaded", "chat", "event"]), "what to wait for"), opt("value", Ty::Rest, "the screen name, widget id, chat text or event kind")], mcp: true, local: false),
    cmd!("events", Observe, "events [--since SEQ] [--limit N]", "what happened since SEQ, oldest first: screens, chat, deaths, respawns, items gained or lost, damage taken (hurt, with its cause), healing, food, dimensions, worlds",
        "Every event has seq and tick; pass the reply's last as --since next time. --limit keeps the newest N and skipped counts the older ones left out. hurt and death carry the player position (a death drops the inventory there). The game keeps the newest 256.",
        ["horizon-ctl events", "horizon-ctl events --since 42", "horizon-ctl events --limit 10"], &[flag("since", Ty::Int, "only events after this seq"), flag("limit", Ty::Int, "newest N events")], mcp: true, local: false),
    cmd!("input-log", Observe, "input-log [--since SEQ] [--limit N] [--origin user|agent|game]", "recent input and who caused it: a person, an agent or the game",
        "Every event has seq, tick and origin. `user` is the window's own keyboard, mouse or pad; `agent` is the control socket (this tool); `game` is input the game made up. A `user-input` event in `events` means a person stepped in while an agent was driving. The game keeps the newest 512.",
        ["horizon-ctl input-log", "horizon-ctl input-log --origin user --limit 20", "horizon-ctl input-log --since 120"], &[flag("since", Ty::Int, "only events after this seq"), flag("limit", Ty::Int, "newest N events"), flag("origin", Ty::Enum(&["user", "agent", "game"]), "user, agent or game")], mcp: true, local: false),
    cmd!("wait-ready", Session, "wait-ready [SECONDS]", "wait until the player has spawned and no chunks are pending",
        "Polls status every 250 ms; replies with the status, or an error after SECONDS (default 120).",
        ["horizon-ctl wait-ready 300"], &[opt("seconds", Ty::Num, "how long to wait (default 120)")], mcp: true, local: true),
    cmd!("logs", Session, "logs [--lines N] [--crash]", "the end of the game's log, or its newest crash report",
        "Reads the game home ($HORIZON_HOME, else ~/.local/share/horizon): logs/latest.log (the last N lines, default 100) or, with --crash, the newest file in crash-reports. Read-only; works whether the game runs or not.",
        ["horizon-ctl logs", "horizon-ctl logs --lines 300", "horizon-ctl logs --crash"],
        &[flag("lines", Ty::Int, "how many lines of latest.log (default 100)"), flag("crash", Ty::Bool, "the newest crash report instead")],
        mcp: true, local: true),
    cmd!("status", Session, "status", "game, renderer and streaming state (the scripting socket's original summary)",
        "Also what F3 shows: the overlay flags, the renderer, chunk streaming and memory.", ["horizon-ctl status"], &[], mcp: true, local: false, reply: STATUS_REPLY),
    cmd!("hello", Session, "hello", "the control protocol's version, the game's release and the command groups it serves",
        "Replies protocol (MAJOR.MINOR.PATCH of the control protocol, docs/api/stability.md), release (the game's version, the string MCP serverInfo carries too) and features (the command groups the game answers, so a script can test for one before calling it).",
        ["horizon-ctl hello"], &[], mcp: true, local: false, reply: HELLO_REPLY),
    cmd!("memory", Session, "memory", "what holds the game's memory: resident set, allocator, client and server columns by part, renderer, GPU memory by resource",
        "Columns count blocks, biomes, light, heightmaps and records, each shared section once; server_columns are the single-player server thread's own levels. GPU memory by label needs Vulkan or DirectX 12. What the census doesn't reach is the resident set less its parts. A build with the alloc-census feature adds heap_by_tag_mb: the live heap by startup step, timed span, chunk job and thread.", ["horizon-ctl memory"], &[], mcp: true, local: false),
    cmd!("worlds", Session, "worlds", "the saved worlds, newest first: folder, name, mode, last played, version",
        "folder is what open, world-rename, world-backup and world-delete take.", ["horizon-ctl worlds"], &[], mcp: true, local: false),
    cmd!("world-rename", Session, "world-rename FOLDER NAME", "rename a world, as Edit World does (its folder stays)",
        "Needs the title screen (no world open).", ["horizon-ctl world-rename Test \"My Base\""],
        &[p("folder", Ty::Str, "world folder from worlds"), p("name", Ty::Rest, "the new name")], mcp: true, local: false),
    cmd!("world-backup", Session, "world-backup FOLDER", "zip a world into backups/, as Edit World's Make Backup does",
        "Needs the title screen. Replies once the zip is written, with its size.", ["horizon-ctl world-backup Test"],
        &[p("folder", Ty::Str, "world folder from worlds")], mcp: true, local: false),
    cmd!("world-delete", Session, "world-delete FOLDER --confirm", "delete a world for good, as Select World's Delete does",
        "Needs the title screen and --confirm: ask the player first, it cannot be undone (world-backup keeps a copy).",
        ["horizon-ctl world-delete Test --confirm"],
        &[p("folder", Ty::Str, "world folder from worlds"), CONFIRM], mcp: true, local: false),
    cmd!("open", Session, "open WORLD [SEED] [creative]", "open or create a world from the title screen",
        "Opens saves/WORLD, creating it with SEED (and creative mode) when it doesn't exist.",
        ["horizon-ctl open Test 12345", "horizon-ctl open Build 0 creative"],
        &[p("world", Ty::Str, "world folder"), opt("seed", Ty::Int, "seed for a new world"), opt("mode", Ty::Enum(&["creative"]), "creative for a creative world")], mcp: true, local: false),
    cmd!("save", Session, "save", "save the world", "", ["horizon-ctl save"], &[], mcp: true, local: false),
    cmd!("quit", Session, "quit", "save and exit the game", "", ["horizon-ctl quit"], &[], mcp: true, local: false),
    cmd!("screenshot", Session, "screenshot PATH", "write the next rendered frame as a PNG",
        "Relative paths are taken from horizon-ctl's directory. Over MCP the image is returned directly.",
        ["horizon-ctl screenshot view.png"], &[], mcp: true, local: false),
    // Observe
    cmd!("state", Observe, "state", "player, world, screen and crosshair target in one reply",
        "position, block_pos, eye, yaw, pitch, velocity, on_ground, health, food, saturation, xp, effects, gamemode, dimension, time, weather, the screen or title menu, the held item, what the crosshair points at and whether an action is running.",
        ["horizon-ctl state"], &[], mcp: true, local: false),
    cmd!("looking-at", Observe, "looking-at", "the block or entity under the crosshair",
        "kind is block, entity or none; a block has pos, state, face, point and distance; an entity has id, type, health and distance.",
        ["horizon-ctl looking-at"], &[], mcp: true, local: false),
    cmd!("entities", Observe, "entities [RADIUS] [TYPE]", "mobs, dropped items and experience orbs near the player, nearest first",
        "RADIUS defaults to 32. TYPE filters by entity type (pig, minecraft:item, ...). Mobs have an id for look-at entity and attack.",
        ["horizon-ctl entities", "horizon-ctl entities 16 cow", "horizon-ctl entities 8 item"],
        &[opt("radius", Ty::Num, "search radius in blocks (default 32)"), opt("type", Ty::Str, "entity type filter")], mcp: true, local: false),
    cmd!("block", Observe, "block X Y Z", "the block state at a position", "",
        ["horizon-ctl block 10 64 -3"], &BLOCK_XYZ, mcp: true, local: false),
    cmd!("light", Observe, "light X1 Y1 Z1 X2 Y2 Z2", "the stored sky and block light of a box",
        "At most 32768 blocks per request. One hex digit per block in `sky` and `block`, x fastest, then z, then y; `.` where no chunk is loaded.",
        ["horizon-ctl light 0 64 0 15 70 15"],
        &BOX, mcp: true, local: false),
    cmd!("blocks", Observe, "blocks X1 Y1 Z1 X2 Y2 Z2", "the non-air blocks of a box, with counts per block",
        "At most 32768 blocks per request.", ["horizon-ctl blocks 0 60 0 15 70 15"],
        &BOX, mcp: true, local: false),
    cmd!("find-block", Observe, "find-block BLOCK [RADIUS]", "the nearest loaded blocks of a kind",
        "Searches a cube of RADIUS (default 32, at most 64) around the player; returns the count and the 16 nearest.",
        ["horizon-ctl find-block oak_log", "horizon-ctl find-block diamond_ore 48"],
        &[p("block", Ty::Str, "block id such as oak_log"), opt("radius", Ty::Int, "search radius (default 32)")], mcp: true, local: false),
    cmd!("inventory", Observe, "inventory", "hotbar, main inventory, armor, offhand, carried stack and item counts",
        "Slots carry the ids click-slot takes: 0-8 hotbar, 9-35 inventory, 500-503 armor (head to feet), 504 offhand. Stacks with components list them as SNBT.",
        ["horizon-ctl inventory"], &[], mcp: true, local: false),
    cmd!("screen", Observe, "screen", "the open screen or title menu, its buttons and its slots",
        "buttons are what click-button takes; slots are what click-slot takes (crafting grid 100-103 in the inventory, 100-108 at a crafting table, result 200, containers from 300, stations from 600). On the title screen, menu has the mode, worlds and the create-world fields.",
        ["horizon-ctl screen"], &[], mcp: true, local: false),
    cmd!("messages", Observe, "messages [N]", "recent chat and system messages, the title and the action bar",
        "N defaults to 20; chat is newest first.", ["horizon-ctl messages 5"], &[opt("n", Ty::Int, "how many chat lines (default 20)")], mcp: true, local: false),
    cmd!("menu", Observe, "menu", "the open menu's slots and carried stack, with station details", "",
        ["horizon-ctl menu"], &[], mcp: false, local: false),
    cmd!("narration", Observe, "narration", "what the narrator was last asked to say, oldest first, and its status",
        "recent holds the last 32 messages the narrator's status let through, said aloud or not; status is the Narrator option (off, all, chat or system) and active whether text-to-speech is there (with HORIZON_NO_AUDIO it is, silently).",
        ["horizon-ctl narration"], &[], mcp: true, local: false),
    // Move
    cmd!("move", Move, "move forward|back|left|right SECONDS [--sprint] [--sneak] [--jump]", "walk in a direction relative to the view for a time",
        "Replies when done with the start and end positions.",
        ["horizon-ctl move forward 2", "horizon-ctl move forward 1.5 --sprint --jump"],
        &[p("direction", Ty::Enum(&["forward", "back", "left", "right"]), "direction relative to the view"), p("seconds", Ty::Num, "how long to walk"),
          flag("sprint", Ty::Bool, "sprint"), flag("sneak", Ty::Bool, "sneak"), flag("jump", Ty::Bool, "keep jumping")], mcp: true, local: false),
    cmd!("goto", Move, "goto X Y Z [--near R] [--sprint] [--direct] [--timeout S]", "walk to a position, finding a path through loaded terrain",
        "A* over standable blocks (steps up one block, drops up to three, avoids lava, swims when it must), then steering with jumps when stuck. Whole-number X and Z name a block and aim at its center (8 64 3 is 8.5 64 3.5); decimals are exact positions. Without a path it fails at once (--direct walks straight at the target anyway, falls included). --near R stops within R blocks (use 1.5-3 to reach a block you then break). Replies with arrived, position and distance, or an error when stuck or out of time (default 60 s).",
        ["horizon-ctl goto 120 70 -40", "horizon-ctl goto 8 64 3 --near 2 --sprint"],
        &[XYZ[0], XYZ[1], XYZ[2], flag("near", Ty::Num, "arrive within this many blocks (default 0.6)"), flag("sprint", Ty::Bool, "sprint"), flag("direct", Ty::Bool, "walk straight when no path is found"), flag("timeout", Ty::Num, "give up after this many seconds (default 60)")], mcp: true, local: false),
    cmd!("jump", Move, "jump", "jump once and reply when landed", "", ["horizon-ctl jump"], &[], mcp: true, local: false),
    cmd!("look", Move, "look YAW PITCH", "set the view direction in degrees",
        "Yaw 0 faces south (+Z), 90 west (-X), 180 north, -90 east; pitch 90 looks straight down.",
        ["horizon-ctl look 90 10"], &[p("yaw", Ty::Num, "yaw in degrees"), p("pitch", Ty::Num, "pitch in degrees (-90 up .. 90 down)")], mcp: true, local: false),
    cmd!("turn", Move, "turn DYAW DPITCH [--over S]", "rotate the view by an amount",
        "Without --over the view jumps. With --over S the turn is mouse motion fed over S seconds with ease-in-out, through the same path as a real mouse (sensitivity, inverted axes, cinematic camera), and the reply comes once it is done.",
        ["horizon-ctl turn 45 0", "horizon-ctl turn -90 10 --over 0.5"],
        &[p("dyaw", Ty::Num, "yaw change in degrees"), p("dpitch", Ty::Num, "pitch change in degrees"), OVER], mcp: true, local: false),
    cmd!("mouse-move", Move, "mouse-move DX DY", "move the mouse: relative motion that turns the view like a player's mouse",
        "DX and DY are mouse units (pixels of relative motion) added to the frame's accumulated movement and turned by MouseHandler.turnPlayer: degrees = units x 0.15 x (sensitivity x 0.6 + 0.2)^3 x 8 (without the 8 while scoping), inverted axes and the cinematic camera included. Positive DX turns right, positive DY looks down. Works in the world with no screen open; replies with yaw and pitch after the frame that turned.",
        ["horizon-ctl mouse-move 100 0", "horizon-ctl mouse-move -25 40"],
        &[p("dx", Ty::Num, "horizontal motion (positive turns right)"), p("dy", Ty::Num, "vertical motion (positive looks down)")], mcp: true, local: false),
    cmd!("look-at", Move, "look-at X Y Z | look-at block X Y Z [FACE] | look-at entity ID  [--over S]", "turn to face a point, a block (a visible face of it) or an entity",
        "look-at block aims at a face the crosshair can reach and fails when none is visible. --over S turns there with mouse motion over S seconds (the shorter way round) instead of jumping.",
        ["horizon-ctl look-at 10.5 65 3.5", "horizon-ctl look-at block 10 64 3 up", "horizon-ctl look-at entity 42 --over 0.4"],
        &[XYZ[0], XYZ[1], XYZ[2], OVER], mcp: true, local: false),
    cmd!("look-at-block", Move, "look-at-block X Y Z [FACE] [--over S]", "aim the crosshair at a block (same as look-at block)", "",
        ["horizon-ctl look-at-block 10 64 3", "horizon-ctl look-at-block 10 64 3 up --over 0.3"], &[BLOCK_XYZ[0], BLOCK_XYZ[1], BLOCK_XYZ[2], opt("face", FACE, "which face to aim at"), OVER], mcp: true, local: false, wire: "look-at block"),
    cmd!("look-at-entity", Move, "look-at-entity ID [--over S]", "face an entity (same as look-at entity)", "",
        ["horizon-ctl look-at-entity 42"], &[p("id", Ty::Int, "entity id from entities"), OVER], mcp: true, local: false, wire: "look-at entity"),
    cmd!("tp", Move, "tp X Y Z", "teleport the player", "", ["horizon-ctl tp 0 80 0"], &XYZ, mcp: true, local: false),
    cmd!("emote", Act, "emote [ID|stop]", "play one of the player's emotes, stop it, or list them",
        "Without an argument: the emotes the player owns (the wheel's) and the one playing. With an ID it plays that emote as the emote wheel does (the player must stand still: moving, attacking or using an item cancels it); stop fades it out. Visual only: nothing is sent to the game server (docs/cosmetics.md).",
        ["horizon-ctl emote", "horizon-ctl emote wave_hello", "horizon-ctl emote stop"],
        &[opt("id", Ty::Str, "an owned emote's id, or stop")], mcp: true, local: false),
    // Act
    cmd!("break", Act, "break X Y Z [--timeout S]", "mine a block, holding the attack button until it breaks",
        "Aims at a visible face (the block must be within reach: use goto --near 2 first), mines with the held item, then waits up to a second for the drops. Replies with block_before, block, ticks, drops, picked_up (inventory change, where a tool that broke shows as -1) and held (the item in hand afterwards, also on a timeout: a broken tool leaves the bare hand).",
        ["horizon-ctl break 10 64 3"], &[BLOCK_XYZ[0], BLOCK_XYZ[1], BLOCK_XYZ[2], flag("timeout", Ty::Num, "give up after this many seconds (default 30)")], mcp: true, local: false),
    cmd!("place", Act, "place X Y Z [--item ITEM] [--sneak]", "place the held item (or ITEM from the hotbar) at an empty position",
        "Clicks the face of a solid neighbour touching X Y Z. Replies with the block placed and the inventory change.",
        ["horizon-ctl place 10 64 4 --item oak_planks"], &[BLOCK_XYZ[0], BLOCK_XYZ[1], BLOCK_XYZ[2], flag("item", Ty::Str, "hotbar item to select first"), flag("sneak", Ty::Bool, "sneak while clicking: place against a crafting table or chest instead of opening it")], mcp: true, local: false),
    cmd!("interact", Act, "interact X Y Z [FACE] [--sneak]", "right-click a block: open a chest or crafting table, press a button, open a door",
        "Replies with the screen afterwards and whether the block changed.", ["horizon-ctl interact 5 64 2"],
        &[BLOCK_XYZ[0], BLOCK_XYZ[1], BLOCK_XYZ[2], opt("face", FACE, "which face to click"), flag("sneak", Ty::Bool, "sneak while clicking: use the held item on the block instead of opening it")], mcp: true, local: false),
    cmd!("use-item", Act, "use-item [--hold SECONDS]", "right-click with the held item: eat, drink, throw, draw a bow",
        "Hold for 1.7 s to eat.", ["horizon-ctl use-item --hold 1.7"], &[flag("hold", Ty::Num, "hold the button this long")], mcp: true, local: false),
    cmd!("attack", Act, "attack [ID]", "swing at what the crosshair targets (after facing entity ID)",
        "Replies with the target's health before and after.", ["horizon-ctl attack", "horizon-ctl attack 42"],
        &[opt("id", Ty::Int, "entity id to face first")], mcp: true, local: false),
    cmd!("select", Act, "select SLOT|ITEM", "select a hotbar slot 1-9, or the hotbar slot holding ITEM", "",
        ["horizon-ctl select 3", "horizon-ctl select oak_planks"], &[p("slot", Ty::Str, "1-9 or an item id")], mcp: true, local: false),
    cmd!("drop", Act, "drop [all]", "drop one of the held item, or the whole stack", "", ["horizon-ctl drop", "horizon-ctl drop all"],
        &[opt("all", Ty::Bool, "drop the whole stack")], mcp: true, local: false),
    cmd!("hunt", Act, "hunt ID [--timeout S]", "walk up to a mob and hit it until it is gone, then pick up its drops",
        "For animals and monsters that move between calls: it re-reads the entity's position, walks within reach (sprinting), attacks with the held item, waits 12 ticks for the weapon to recharge, and repeats. Once the entity is gone (killed or despawned) it walks over its last position to collect the drops. Replies with hits, last_seen and inventory_change; times out with kind timeout (default 30 s).",
        ["horizon-ctl hunt 90", "horizon-ctl hunt 90 --timeout 60"],
        &[p("id", Ty::Int, "entity id from entities"), flag("timeout", Ty::Num, "give up after this many seconds (default 30)")], mcp: true, local: true),
    cmd!("collect", Act, "collect [RADIUS]", "walk over every dropped item within RADIUS blocks (default 8) to pick it up",
        "Drops from break, hunt or a death often land out of pickup range, in a hole or behind a corner. collect walks to each item nearest first, trying each once, and replies with inventory_change and left: the items it could not reach (with why, the walk's error, when it had one). It stops with kind hurt, listing the hurt events, as soon as the player takes damage.",
        ["horizon-ctl collect", "horizon-ctl collect 16"],
        &[opt("radius", Ty::Num, "search radius in blocks (default 8)")], mcp: true, local: true),
    cmd!("hold", Act, "hold KEY SECONDS", "press a key, wait, release it", "Keys as for key.", ["horizon-ctl hold w 2"],
        &[p("key", Ty::Str, "key name"), p("seconds", Ty::Num, "how long")], mcp: false, local: true),
    cmd!("key", Act, "key KEY down|up|tap|repeat", "press, release or auto-repeat a key (w a s d space shift ctrl e q esc f3 1-9 arrows ...)",
        "down holds the key until key ... up, release or stop; wait does not release it. move, goto, jump, break and the other actions release the movement keys when they end.",
        ["horizon-ctl key space tap"], &[p("key", Ty::Str, "key name"), p("action", Ty::Enum(&["down", "up", "tap", "repeat"]), "what to do")], mcp: true, local: false),
    cmd!("mouse", Act, "mouse left|right|middle down|up|click", "press or release a mouse button in the world", "",
        ["horizon-ctl mouse right click"], &[p("button", Ty::Enum(&["left", "right", "middle"]), "button"), p("action", Ty::Enum(&["down", "up", "click"]), "what to do")], mcp: true, local: false),
    cmd!("interact-entity", Act, "interact-entity ID", "right-click an entity: trade with a villager, mount a horse, ride a boat, shear a sheep",
        "Faces the entity (from entities), right-clicks it with the held item, waits two ticks and replies with the screen and inventory_change (items gained or lost). Fails without clicking when the entity is not under the crosshair, naming what is in the way.",
        ["horizon-ctl interact-entity 42"], &[p("id", Ty::Int, "entity id from entities")], mcp: true, local: true),
    cmd!("pad", Act, "pad BUTTON down|up|tap | pad stick left|right X Y | pad trigger left|right V | pad release | pad status | pad connection connected [FAMILY]|disconnected | pad family FAMILY", "a virtual controller: buttons, sticks and triggers as a real pad would send them",
        "Buttons by position: south east west north lb rb lt rt select start ls rs up down left right (south is Xbox A / PlayStation cross). Sticks run -1..1 with up positive and stay tilted until centred; triggers 0..1. Merged with any real controller; status lists the connected ones and what is held. FAMILY is xbox, playstation, nintendo or auto: connection connected raises the notice of that family's controller (PlayStation unless given), and family makes the pad playing that family's (its glyphs; a Nintendo one goes by its letters when Nintendo Buttons says so).",
        ["horizon-ctl pad south tap", "horizon-ctl pad stick left 0 1", "horizon-ctl pad trigger right 1", "horizon-ctl pad status"],
        &[p("what", Ty::Str, "a button name, stick, trigger, release, status, connection or family"), opt("a", Ty::Str, "down|up|tap, left|right, connected|disconnected, or a family"), opt("b", Ty::Num, "stick X or trigger value"), opt("c", Ty::Num, "stick Y")], mcp: true, local: false),
    cmd!("release", Act, "release", "release every key and mouse button", "", ["horizon-ctl release"], &[], mcp: true, local: false),
    cmd!("stop", Act, "stop", "cancel the running action (goto, break...) and release every key", "", ["horizon-ctl stop"], &[], mcp: true, local: false),
    // Screens
    cmd!("open-inventory", Screens, "open-inventory", "open the inventory screen (crafting grid 100-103, result 200)", "",
        ["horizon-ctl open-inventory"], &[], mcp: true, local: false),
    cmd!("click-slot", Screens, "click-slot SLOT [right] [shift]", "click a slot of the open inventory or menu",
        "SLOT ids as listed by screen: 0-8 hotbar, 9-35 inventory, 100+ crafting grid, 200 result, 300+ container, 500-503 armor, 504 offhand, 600+ station. A left click picks up or puts down the carried stack; right puts down one item; shift moves the stack across.",
        ["horizon-ctl click-slot 36", "horizon-ctl click-slot 100 right", "horizon-ctl click-slot 200 shift"],
        &[p("slot", Ty::Int, "slot id"), opt("right", Ty::Bool, "right-click"), opt("shift", Ty::Bool, "shift-click")], mcp: true, local: false),
    cmd!("close", Screens, "close", "close the open menu and return to the game", "", ["horizon-ctl close"], &[], mcp: true, local: false),
    cmd!("ui", Screens, "ui", "every widget of the open screen: id, kind, label, bounds, value",
        "Kinds: button, slider (value 0..1), field, row (worlds, servers, packs, languages), tab (selected), slot (slot-ID, with its stack). Ids come from labels (\"FOV: 70\" is fov, a repeat gets -2) or kind-N without one, and stay put when a value changes. Reflects the last drawn frame (frame counts them).",
        ["horizon-ctl ui"], &[], mcp: true, local: false),
    cmd!("click", Screens, "click ID [right|middle] [shift]", "click a widget of ui by its id, as the mouse would",
        "A field takes focus (then type); slot-ID clicks a slot like click-slot. middle presses the middle button (pick block: a creative player clones the stack).",
        ["horizon-ctl click done", "horizon-ctl click video-settings", "horizon-ctl click row-0", "horizon-ctl click slot-0 shift"],
        &[p("id", Ty::Str, "widget id from ui"), opt("right", Ty::Bool, "right-click"), opt("middle", Ty::Bool, "middle-click"), opt("shift", Ty::Bool, "shift-click (slots)")], mcp: true, local: false),
    cmd!("set", Screens, "set ID VALUE", "move a slider of ui to VALUE (0..1), as a click on its track",
        "The option behind it rounds to its own steps; read the new label with ui.",
        ["horizon-ctl set fov 0.5", "horizon-ctl set render-distance 0"],
        &[p("id", Ty::Str, "slider id from ui"), p("value", Ty::Num, "0..1 along the track")], mcp: true, local: false),
    cmd!("options-get", Screens, "options-get [KEY...]", "the game's options (options.txt) by key, with their type and range or choices",
        "Keys are vanilla's: fov, renderDistance, guiScale, gamma, maxFps, lang, resourcePacks, soundCategory_music, key_key.jump... Enums are given and set by name, key bindings by key name (key.keyboard.w).",
        ["horizon-ctl options-get", "horizon-ctl options-get fov renderDistance"], &[opt("keys", Ty::Rest, "option keys (default: all)")], mcp: true, local: false),
    cmd!("options-set", Screens, "options-set KEY VALUE", "set one option, apply it at once and save options.txt, as the options screen does",
        "A value the option refuses is an error saying what it accepts. Lists are JSON: options-set resourcePacks [\"vanilla\"].",
        ["horizon-ctl options-set fov 90", "horizon-ctl options-set renderClouds fast", "horizon-ctl options-set key_key.jump key.keyboard.j"],
        &[p("key", Ty::Str, "option key from options-get"), p("value", Ty::Rest, "the new value")], mcp: true, local: false),
    cmd!("click-button", Screens, "click-button LABEL", "click a button of the open screen or title menu by its label",
        "Case-insensitive; a unique part of the label is enough. screen lists the buttons.",
        ["horizon-ctl click-button Singleplayer", "horizon-ctl click-button \"Create New World\"", "horizon-ctl click-button respawn"],
        &[p("label", Ty::Rest, "button label")], mcp: true, local: false),
    cmd!("click-at", Screens, "click-at X Y [right]", "click at GUI pixel X Y of the window (screen gives button rectangles)", "",
        ["horizon-ctl click-at 200 120"], &[p("x", Ty::Num, "GUI x"), p("y", Ty::Num, "GUI y"), opt("right", Ty::Bool, "right-click")], mcp: true, local: false),
    cmd!("drag-at", Screens, "drag-at [right] X Y X Y...", "press at the first GUI point, move through the others, release at the last", "",
        ["horizon-ctl drag-at 130 100 148 100 166 100", "horizon-ctl drag-at right 130 100 148 100"], &[p("points", Ty::Rest, "right, then GUI x y pairs")], mcp: true, local: false),
    cmd!("scroll-at", Screens, "scroll-at X Y STEPS", "turn the mouse wheel at GUI pixel X Y (positive scrolls up)", "",
        ["horizon-ctl scroll-at 130 100 -1"], &[p("x", Ty::Num, "GUI x"), p("y", Ty::Num, "GUI y"), p("steps", Ty::Num, "wheel steps")], mcp: true, local: false),
    cmd!("type", Screens, "type [--field name|seed] [--replace] TEXT", "type into the focused text field (chat box, world name, anvil, sign...)",
        "On the title's create-world screen, --field name|seed picks the field and --replace clears it first; type \\b deletes one character.",
        ["horizon-ctl type hello everyone", "horizon-ctl type --field name --replace Agent World", "horizon-ctl type --field seed 12345"], &[flag("field", Ty::Enum(&["name", "seed"]), "create-world field to type into"), flag("replace", Ty::Bool, "clear the field first"), p("text", Ty::Rest, "text to type")], mcp: true, local: false),
    cmd!("chat", Screens, "chat TEXT", "send a chat line (or a /command) as if typed", "", ["horizon-ctl chat hello"],
        &[p("text", Ty::Rest, "message")], mcp: true, local: false),
    cmd!("command", Screens, "command TEXT", "run a command (the leading / is optional)", "",
        ["horizon-ctl command time set day", "horizon-ctl command /give @s oak_log 4"], &[p("command", Ty::Rest, "command line")], mcp: true, local: false),
    cmd!("respawn", Screens, "respawn", "respawn from the death screen", "", ["horizon-ctl respawn"], &[], mcp: true, local: false),
    cmd!("pause", Screens, "pause", "open the pause menu", "", ["horizon-ctl pause"], &[], mcp: true, local: false),
    cmd!("resume", Screens, "resume", "leave the pause menu", "", ["horizon-ctl resume"], &[], mcp: true, local: false),
    cmd!("options", Screens, "options [PAGE]", "open the options screen, or one of its pages (Horizon's too)",
        "Then ui lists its widgets and click, set and type change them, as with the mouse. options-set changes an options.txt value without the screen.",
        ["horizon-ctl options video", "horizon-ctl options horizon"], &[opt("page", PAGES, "the page (default: the main options)")], mcp: true, local: false),
    cmd!("book", Screens, "book [next|back|take|done]", "the open book or lectern: its page text, or press one of its buttons",
        "", ["horizon-ctl book", "horizon-ctl book next"], &[opt("action", Ty::Enum(&["next", "back", "take", "done"]), "a button to press")], mcp: true, local: false),
    cmd!("signedit", Screens, "signedit X Y Z [front|back]", "open the sign editor of the sign at X Y Z (then type and close)",
        "", ["horizon-ctl signedit 3 64 1"], &[BLOCK_XYZ[0], BLOCK_XYZ[1], BLOCK_XYZ[2], opt("side", Ty::Enum(&["front", "back"]), "which side")], mcp: true, local: false),
    cmd!("mount-menu", Screens, "mount-menu", "open the inventory of the horse, donkey, mule or llama being ridden",
        "As the inventory key does while riding; also opens the nearest one's.", ["horizon-ctl mount-menu"], &[], mcp: true, local: false),
    cmd!("chat-log", Screens, "chat-log [N]", "the newest N chat messages", "", ["horizon-ctl chat-log 5"], &[], mcp: false, local: false),
    // Multiplayer
    cmd!("connect", Multiplayer, "connect ADDRESS [NAME]", "join a multiplayer server, or a friend's world by its invite code, from the title screen",
        "ADDRESS is HOST[:PORT], or friend:CODE for an invite code. Only when the player asks: it reaches the network.",
        ["horizon-ctl connect play.example.net", "horizon-ctl connect localhost:25565", "horizon-ctl connect friend:0B33K-Q405Q-..."],
        &[p("address", Ty::Str, "HOST[:PORT] or friend:CODE"), opt("name", Ty::Str, "player name when offline")], mcp: true, local: false),
    cmd!("disconnect", Multiplayer, "disconnect", "leave the server, back to the server list", "", ["horizon-ctl disconnect"], &[], mcp: true, local: false),
    cmd!("servers", Multiplayer, "servers", "the Multiplayer screen's server list: name and address",
        "Add, edit and remove them on the Multiplayer screen (click-button Multiplayer, then ui).", ["horizon-ctl servers"], &[], mcp: true, local: false),
    cmd!("lan", Multiplayer, "lan open [PORT] [commands] [force] | lan close | lan status", "open the world to LAN (Open to LAN), stop, or see the guests",
        "PORT 0 or none picks one; commands lets guests use cheats; force forces the game mode. Only when the player asks.",
        ["horizon-ctl lan open", "horizon-ctl lan status", "horizon-ctl lan close"],
        &[p("action", Ty::Enum(&["open", "close", "status"]), "what to do"), opt("port", Ty::Int, "port for open"), opt("commands", Ty::Bool, "allow cheats"), opt("force", Ty::Bool, "force the game mode")], mcp: true, local: false),
    cmd!("friends", Multiplayer, "friends host|stop|status", "Play with Friends: share the open world behind an invite code, stop, or see who joined",
        "status gives the code: share it only when the player asks. Friends join with connect friend:CODE.",
        ["horizon-ctl friends host", "horizon-ctl friends status"], &[p("action", Ty::Enum(&["host", "stop", "status"]), "what to do")], mcp: true, local: false),
    // World
    cmd!("give", World, "give ITEM [COUNT]", "add items to the inventory", "", ["horizon-ctl give oak_log 16"],
        &[p("item", Ty::Str, "item id"), opt("count", Ty::Int, "how many (default 1)")], mcp: true, local: false),
    cmd!("time", World, "time TICKS", "set the time of day (0 sunrise, 6000 noon, 13000 night)", "", ["horizon-ctl time 6000"],
        &[p("ticks", Ty::Int, "day time")], mcp: true, local: false),
    cmd!("setblock", World, "setblock X Y Z BLOCK[props]", "set a block (with neighbour updates)", "", ["horizon-ctl setblock 0 64 0 stone"],
        &[BLOCK_XYZ[0], BLOCK_XYZ[1], BLOCK_XYZ[2], p("block", Ty::Str, "block state such as oak_stairs[facing=east]")], mcp: true, local: false),
    cmd!("summon", World, "summon KIND [X Y Z] [baby]", "spawn a mob (in front of the player without a position)", "Replies with its id.", ["horizon-ctl summon cow", "horizon-ctl summon pig 10 64 3 baby"],
        &[p("kind", Ty::Str, "mob type"), opt("x", Ty::Num, "X"), opt("y", Ty::Num, "Y"), opt("z", Ty::Num, "Z"), opt("baby", Ty::Bool, "a baby")], mcp: true, local: false),
    cmd!("effect", World, "effect ID SECONDS [AMPLIFIER] | effect clear", "give a status effect, or clear them all", "", ["horizon-ctl effect speed 30 1", "horizon-ctl effect clear"],
        &[p("id", Ty::Str, "effect id such as speed, or clear"), opt("seconds", Ty::Num, "duration"), opt("amplifier", Ty::Int, "level - 1 (default 0)")], mcp: true, local: false),
    cmd!("effects", World, "effects", "the player's active status effects", "", ["horizon-ctl effects"], &[], mcp: true, local: false),
    cmd!("xp", World, "xp LEVELS", "add experience levels (negative removes)", "", ["horizon-ctl xp 30"], &[p("levels", Ty::Int, "levels to add")], mcp: true, local: false),
    cmd!("enchant", World, "enchant ID LEVEL", "enchant the held item (a book stores it)", "", ["horizon-ctl enchant sharpness 5"],
        &[p("id", Ty::Str, "enchantment id"), p("level", Ty::Int, "level")], mcp: true, local: false),
    cmd!("potion", World, "potion ITEM POTION", "give a potion item", "", ["horizon-ctl potion splash_potion healing"],
        &[p("item", Ty::Enum(&["potion", "splash_potion", "lingering_potion", "tipped_arrow"]), "which item"), p("potion", Ty::Str, "potion id")], mcp: true, local: false),
    cmd!("fill", World, "fill X1 Y1 Z1 X2 Y2 Z2 BLOCK", "set a box of loaded blocks", "", ["horizon-ctl fill 0 64 0 4 64 4 glass"],
        &[BOX[0], BOX[1], BOX[2], BOX[3], BOX[4], BOX[5], p("block", Ty::Str, "block state")], mcp: true, local: false),
    cmd!("container", World, "container X Y Z [ITEM COUNT]", "list a container's items, or add items to it", "", ["horizon-ctl container 3 64 1", "horizon-ctl container 3 64 1 diamond 3"],
        &[BLOCK_XYZ[0], BLOCK_XYZ[1], BLOCK_XYZ[2], opt("item", Ty::Str, "item to add"), opt("count", Ty::Int, "how many")], mcp: true, local: false),
    cmd!("render-distance", World, "render-distance N", "chunks, 2..128", "", ["horizon-ctl render-distance 8"], &[p("chunks", Ty::Int, "render distance")], mcp: false, local: false),
    cmd!("wait", World, "wait TICKS", "wait until TICKS game ticks have passed (20 per second)",
        "Keys and mouse buttons held with key ... down or mouse ... down stay held while it waits. Release them with key ... up, release or stop.",
        ["horizon-ctl wait 40", "horizon-ctl key w down && horizon-ctl wait 40 && horizon-ctl key w up"],
        &[p("ticks", Ty::Int, "game ticks")], mcp: true, local: false),
    // Scripting
    cmd!("script", Scripting, "script [FILE] [--keep-going]", "run commands from FILE (or stdin), one per line",
        "Lines are split like a shell (quotes keep spaces); # starts a comment; sleep SECONDS waits. Stops at the first error unless --keep-going. horizon-ctl - is the older form: read stdin and keep going.",
        ["horizon-ctl script play.txt", "printf 'state\\ngoto 0 64 0\\n' | horizon-ctl script"], &[], mcp: false, local: true),
    cmd!("sleep", Scripting, "sleep SECONDS", "wait (in scripts)", "", ["sleep 0.5"], &[], mcp: false, local: true),
    cmd!("mcp", Scripting, "mcp [--paced]", "serve the Model Context Protocol on stdin/stdout, one tool per command",
        "Add it to an MCP client as a stdio server: command horizon-ctl, args [\"mcp\"] (with --socket PATH first if needed). The screenshot tool returns the image. --paced stops a single-player world between tool calls: the pause menu opens after each action and closes before the next, so time passes only while an action (or wait) runs, not while the agent thinks; reading tools (state, inventory, entities, events...) leave it paused.",
        ["horizon-ctl mcp", "horizon-ctl mcp --paced"], &[], mcp: false, local: true),
    cmd!("bench", Scripting, "bench SECONDS [raw]", "reset frame stats, wait, reply with the summary", "", ["horizon-ctl bench 10", "horizon-ctl bench 4 raw"], &[p("seconds", Ty::Num, "how long to measure"), opt("raw", Ty::Bool, "every frame time too (frame_ms, cpu_ms, gpu_ms; alloc_count and alloc_bytes in an alloc-count build)")], mcp: true, local: false),
    cmd!("stats", Scripting, "stats [reset] [raw]", "frame-time summary since the last reset", "A build with the alloc-count feature adds alloc: the window thread's allocations and KiB per frame and all threads' allocations per second, else null.", ["horizon-ctl stats"], &[opt("reset", Ty::Bool, "start counting again"), opt("raw", Ty::Bool, "every frame time too (frame_ms, cpu_ms, gpu_ms; alloc_count and alloc_bytes in an alloc-count build)")], mcp: true, local: false),
    cmd!("gpu-passes", Scripting, "gpu-passes [on|off|detail]", "time Horizon's own render passes on the GPU, shown in F3 and in stats and bench; detail adds dispatch times and, on Vulkan, fragment and vertex counts per world pass", "", ["horizon-ctl gpu-passes on"], &[opt("mode", Ty::Enum(&["on", "off", "detail"]), "on, off or detail; without it, only reports")], mcp: false, local: false),
    cmd!("trace", Scripting, "trace [FILE [SECONDS] | stop]", "write every thread's timed spans to FILE as a .hzperf capture holding a timeline, for SECONDS or until stop",
        "Each span (ticks by system, streaming, uploads, far terrain, lighting, the renderer) is recorded on the thread that ran it, with its depth; the timeline stops after 60 seconds. trace stop replies with the file's path once it is whole. Without arguments, only reports whether a trace is being written. HORIZON_TRACE=FILE writes one from the start; horizon-trace on a dedicated server's console writes one to reports/. Open the file in the web UI. See docs/graphics.md.",
        ["horizon-ctl trace /tmp/frame.hzperf 10", "horizon-ctl trace stop"], &[], mcp: false, local: false),
    cmd!("profile", Scripting, "profile [client|server] start [--seconds S] [--hz N] [--over MS] [--threads main|all] [--wall] [--gpu[=detail]] [--no-samples] [--timeline] | stop [--bench REPORT] | status | mark NAME | open FILE | share FILE", "capture the game's performance to a .hzperf file in reports/ in the game home",
        "start records the frame times, the health history and the render thread's call trees until stop or --seconds (at most 600). stop replies with the file's path once the file is whole, and with the viewer's address (open_at) only in a build that has a part to open captures; open FILE serves the file once on 127.0.0.1 and opens the viewer on it, share FILE uploads it and replies with the link; both fail in a build without that part and work with every telemetry switch off; status says whether a capture runs, its age and its size; mark NAME puts a label at the current time. F3+O does start and stop from the keyboard; HORIZON_PROFILE=DIR records the first 10 minutes of a run into DIR. --gpu adds each render pass's median GPU time and --gpu=detail the dispatch times and, on Vulkan, fragment and vertex counts per pass; HORIZON_SHADER_PACK_TIMING and HORIZON_GPU_PASS_DETAIL start a capture with them. stop --bench REPORT embeds a benchmark report (benchmarks/*.json) in the capture; --benchmark --profile does this by itself. Read the file with horizon-perf (query gpu). With no side a single-player run captures both sides in one file (the window, and the server thread as its own section); client or server captures one. Connected to a Horizon server, server asks that server to capture its side: it needs operator level 4, only one server capture runs at a time, and the server keeps the file in its reports/ and logs who asked; a refusal is a warning in the window's own capture.",
        ["horizon-ctl profile start --seconds 30", "horizon-ctl profile mark loaded-the-nether", "horizon-ctl profile stop", "horizon-ctl profile open reports/profile-20251107-143844Z.hzperf"],
        &[p("action", Ty::Enum(&["start", "stop", "status", "mark", "open", "share"]), "what to do"), opt("name", Ty::Rest, "mark: the label; open and share: the file"),
          flag("side", Ty::Enum(&["client", "server"]), "client or server, for the window or the server (the single-player server thread, or the server joined); both when omitted"), flag("seconds", Ty::Int, "start: stop after this many seconds, 1 to 600"),
          flag("hz", Ty::Int, "start: samples per second, 10 to 1000 (default 250)"), flag("over", Ty::Num, "start: sample only frames slower than this many milliseconds"),
          flag("threads", Ty::Enum(&["main", "all"]), "start: the render thread (default) or every thread that opted in"), flag("wall", Ty::Bool, "start: also sample on the wall clock, to find waits"),
          flag("gpu", Ty::Enum(&["passes", "detail"]), "start: also record each render pass's GPU time (detail: its parts and counters too)"),
          flag("no-samples", Ty::Bool, "start: record frame times and health only"), flag("timeline", Ty::Bool, "start: also record every thread's spans, for 60 seconds at most"),
          flag("bench", Ty::Str, "stop: the path of a benchmark report to embed in the capture")], mcp: true, local: false),
    cmd!("screenshot-sequence", Scripting, "screenshot-sequence FRAMES STEP_MS PATH|cancel", "draw a shader pack's first FRAMES frames on a set clock and write the last as a PNG",
        "Needs HORIZON_STILL and a frozen world (docs/agent-control.md). The pack starts over, frame n's clock is n frames and (n - 1) × STEP_MS milliseconds, the PNG is written as screenshot PATH writes it, and the reply comes once it is. Relative paths are taken from horizon-ctl's directory. cancel stops a running sequence.",
        ["horizon-ctl screenshot-sequence 64 50 frame64.png", "horizon-ctl screenshot-sequence 64 16.666667 frame64.png", "horizon-ctl screenshot-sequence cancel"],
        &[p("frames", Ty::Str, "frames to draw, 1 to 10000, or cancel"), opt("step_ms", Ty::Num, "milliseconds between frames on the pack's clock, more than 0 (16.666667 for 60 fps)"), opt("path", Ty::Rest, "the PNG to write")], mcp: false, local: false),
    cmd!("render-capture", Scripting, "render-capture on|off|DIR", "keep CPU copies of the meshes the renderer draws (on), or write the next frame's scene to DIR for the scene_replay example",
        "on meshes every section and far tile again so the copies are complete (HORIZON_RENDER_CAPTURE=1 keeps them from the start). DIR gets the camera, frame inputs, settings and every resident near section and far tile mesh; entities, particles, weather, the hand and the HUD are left out. See docs/graphics.md.",
        ["horizon-ctl render-capture on", "horizon-ctl render-capture /tmp/scene"], &[], mcp: false, local: false),
    cmd!("fly", Scripting, "fly X1 Y1 Z1 X2 Y2 Z2 SECONDS [PITCH [raw]]", "fly the camera along a line, then reply with the frame summary",
        "The reply's tick_ms and tick_ms_max are the world's smoothed tick time over the flight (the server thread's when single player runs on one). With raw, the reply also lists every frame time in milliseconds (frame_ms).",
        ["horizon-ctl fly 0 100 0 500 100 0 20"], &[], mcp: false, local: false),
    // Repro recordings
    cmd!("record-start", Scripting, "record-start [NAME] [--no-world]", "record input, state and the world for a bug repro",
        "Writes recordings/<stamp>-NAME/: inputs.jsonl (who caused each input: user, agent or game), state.jsonl (the player and world every second), events.jsonl and, in single player, a copy of the world. The recording starts from a fresh save. Replies once the world copy is done.",
        ["horizon-ctl record-start repro-bug", "horizon-ctl record-start --no-world"], &[opt("name", Ty::Str, "folder name after the timestamp (default session)"), flag("no-world", Ty::Bool, "do not copy the world (a server session)")], mcp: true, local: false),
    cmd!("record-stop", Scripting, "record-stop [--export]", "stop the recording (and write the repro zip)",
        "Without --export only the recording folder is closed; repro-export zips it later. With --export the reply carries the zip path.",
        ["horizon-ctl record-stop", "horizon-ctl record-stop --export"], &[flag("export", Ty::Bool, "write the repro bundle too")], mcp: true, local: false),
    cmd!("record-status", Scripting, "record-status", "the running recording and replay, if any", "",
        ["horizon-ctl record-status"], &[], mcp: true, local: false),
    cmd!("record-list", Scripting, "record-list", "the recordings and repro bundles on disk", "", ["horizon-ctl record-list"], &[], mcp: true, local: false),
    cmd!("record-replay", Scripting, "record-replay NAME [--stop] [--in-place] [--keep-world] [--timeout S]", "replay a recording, best effort",
        "Loads the recording's world copy into saves/, opens it and feeds the recorded input back at its ticks. Only input and movement commands replay (never world, network or account ones). --in-place feeds the input into the world already open (a server recording needs it); --keep-world leaves the copied world behind when it ends. --stop ends a running replay. Same build and machine only; state.jsonl is the oracle, not a promise.",
        ["horizon-ctl record-replay repro-bug", "horizon-ctl record-replay repro-bug --stop", "horizon-ctl record-replay server-fight --in-place"],
        &[opt("name", Ty::Str, "recording from record-list"), flag("stop", Ty::Bool, "stop the running replay"), flag("in-place", Ty::Bool, "feed the open world instead of opening the copy"), flag("keep-world", Ty::Bool, "leave the copied world in saves/"), flag("timeout", Ty::Int, "give up after this many seconds (default 300)")], mcp: true, local: false),
    cmd!("repro-export", Scripting, "repro-export [NAME] [PATH] [--no-world]", "zip a recording with the logs, crash report and status",
        "Writes repro/<stamp>-NAME.zip: the recording, logs/latest.log, the newest crash report, a status.json and a README. The player attaches it to the bug report; nothing is uploaded. Stops a running recording first.",
        ["horizon-ctl repro-export", "horizon-ctl repro-export repro-bug out.zip"], &[opt("name", Ty::Str, "recording from record-list (default the newest)"), opt("path", Ty::Str, "where to write the zip"), flag("no-world", Ty::Bool, "leave the world out")], mcp: true, local: false),
];

pub fn find(name: &str) -> Option<&'static Cmd> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// `horizon-ctl help`.
pub fn overview() -> String {
    let mut out = String::from(
        "horizon-ctl: play and script a running Horizon from the command line.\n\n\
         usage: horizon-ctl [--socket PATH] [--timeout SECONDS] [--api LINE] [--pretty] COMMAND [ARGS...]\n\
         \x20      horizon-ctl COMMAND --help      (or: horizon-ctl help COMMAND)\n",
    );
    for group in Group::ALL {
        out.push_str(&format!("\n{}:\n", group.title()));
        for c in COMMANDS.iter().filter(|c| c.group == group) {
            out.push_str(&format!("  {:<16} {}\n", c.name, c.summary));
        }
    }
    out.push_str(
        "\nEvery reply is one JSON line. Errors are {\"error\": \"...\"}; the exit status is 0 on\n\
         success, 1 when the game replied with an error, 2 for a usage error, 3 when the\n\
         game cannot be reached or started, 4 while No AI is on (the launcher's settings) and 5 when\n\
         --api LINE (such as --api 0.1) names a protocol line the game's hello does not report.\n\
         Actions (goto, break, place...) block until done.\n\
         horizon-ctl help --game lists the running game's own commands, test hooks included.\n\
         The socket is $HORIZON_CONTROL, else $XDG_RUNTIME_DIR/horizon-control.sock (on Windows\n\
         %TEMP%\\horizon-control.json, a localhost TCP port with a token; any path ending in .json\n\
         uses that transport).\n\n\
         First steps:\n\
         \x20 horizon-ctl launch --world Test --seed 1 --wait\n\
         \x20 horizon-ctl state\n\
         \x20 horizon-ctl find-block oak_log\n\
         \x20 horizon-ctl goto X Y Z --near 2 && horizon-ctl break X Y Z\n\
         \x20 horizon-ctl inventory\n",
    );
    out
}

/// `horizon-ctl COMMAND --help`.
pub fn detail(c: &Cmd) -> String {
    let mut out = format!(
        "usage: horizon-ctl {}\n\n{}.\n",
        c.usage,
        capitalize(c.summary)
    );
    if !c.details.is_empty() {
        out.push('\n');
        out.push_str(&wrap(c.details, 78));
    }
    if !c.examples.is_empty() {
        out.push_str("\nExamples:\n");
        for e in c.examples {
            out.push_str(&format!("  {e}\n"));
        }
    }
    out
}

/// The commands whose replies are described so far, one group at a time: the test
/// requires each to name its fields.
#[cfg(test)]
pub const REPLY_GROUP: &[&str] = &["hello", "status"];

/// The JSON schema of a command's reply, or none while its reply is undescribed.
pub fn reply_schema(c: &Cmd) -> Option<Value> {
    if c.reply.is_empty() {
        return None;
    }
    let mut props = Map::new();
    let mut required = Vec::new();
    for fld in c.reply {
        let types: Vec<&str> = fld.ty.split('|').collect();
        let ty = if types.len() == 1 {
            json!(types[0])
        } else {
            json!(types)
        };
        props.insert(
            fld.name.to_owned(),
            json!({ "type": ty, "description": fld.help }),
        );
        required.push(fld.name);
    }
    Some(json!({ "type": "object", "properties": props, "required": required }))
}

/// The generated reference, docs/horizon-ctl-commands.md (`horizon-ctl help --markdown`).
pub fn markdown() -> String {
    let cell = |s: &str| s.replace('|', "\\|");
    let mut out = String::from(
        "<!-- Generated by `horizon-ctl help --markdown` from the command table in \
         crates/horizon-client/src/bin/horizon-ctl/commands.rs: edit the table, then regenerate. -->\n\
         # horizon-ctl commands\n\n\
         Every command `horizon-ctl` takes, its MCP tool in `horizon-ctl mcp` (the launcher's \
         `horizon-launcher mcp` offers the same tools as `game_<tool>`, all but `launch`) and its \
         arguments. Positional arguments come first, in order; `--flags` after them. \
         `horizon-ctl help --game` lists the game's own commands, test hooks included. \
         Design rules and the full inventory: [agent-control.md](agent-control.md).\n",
    );
    for group in Group::ALL {
        out.push_str(&format!(
            "\n## {}\n\n| Command | MCP tool | What it does | Arguments |\n| --- | --- | --- | --- |\n",
            group.title()
        ));
        for c in COMMANDS.iter().filter(|c| c.group == group) {
            let tool = if c.mcp {
                let mut t = format!("`{}`", tool_name(c));
                if DESTRUCTIVE.contains(&c.name) {
                    t.push_str(" (destructive)");
                } else if OPEN_WORLD.contains(&c.name) {
                    t.push_str(" (network)");
                }
                t
            } else {
                "CLI only".to_owned()
            };
            let args: Vec<String> = c
                .params
                .iter()
                .map(|p| {
                    let ty = match p.ty {
                        Ty::Int => "integer".to_owned(),
                        Ty::Num => "number".to_owned(),
                        Ty::Str => "string".to_owned(),
                        Ty::Rest => "text".to_owned(),
                        Ty::Bool => "boolean".to_owned(),
                        Ty::Enum(v) => v.join(", "),
                    };
                    let name = if p.flag {
                        format!("--{}", p.name)
                    } else {
                        p.name.to_owned()
                    };
                    let need = if p.required { ", required" } else { "" };
                    format!("`{name}` ({ty}{need}): {}", cell(p.help))
                })
                .collect();
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                cell(c.usage),
                tool,
                cell(&capitalize(c.summary)),
                if args.is_empty() {
                    "none".to_owned()
                } else {
                    args.join("<br>")
                }
            ));
        }
    }
    out.push_str(
        "\n## Replies\n\nThe reply fields of the commands described so far (more follow, a group at a \
         time). Each reply is one JSON object; a type ending in `null` may be `null` when the field \
         does not apply. A reader ignores keys it does not know: replies may gain fields within a \
         protocol line. The MCP tool list declares the same fields as `outputSchema`.\n",
    );
    for c in COMMANDS.iter().filter(|c| !c.reply.is_empty()) {
        out.push_str(&format!(
            "\n### `{}`\n\n| Field | Type | Meaning |\n| --- | --- | --- |\n",
            c.name
        ));
        for fld in c.reply {
            out.push_str(&format!(
                "| `{}` | {} | {} |\n",
                fld.name,
                fld.ty.replace('|', ", "),
                cell(fld.help)
            ));
        }
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The MCP tool name of a command (`look-at-block` gives `look_at_block`).
pub fn tool_name(c: &Cmd) -> String {
    c.name.replace('-', "_")
}

/// The JSON schema of a command's arguments.
pub fn schema(c: &Cmd) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for p in c.params {
        let mut s = match p.ty {
            Ty::Int => json!({ "type": "integer" }),
            Ty::Num => json!({ "type": "number" }),
            Ty::Str | Ty::Rest => json!({ "type": "string" }),
            Ty::Bool => json!({ "type": "boolean" }),
            Ty::Enum(values) => json!({ "type": "string", "enum": values }),
        };
        s["description"] = json!(p.help);
        props.insert(p.name.to_owned(), s);
        if p.required {
            required.push(p.name);
        }
    }
    json!({ "type": "object", "properties": props, "required": required })
}

/// The command line for an MCP call's arguments: positional values in order,
/// then `--flag value` options.
pub fn line_for(c: &Cmd, args: &Value) -> Result<Vec<String>, String> {
    let mut words: Vec<String> = c
        .wire
        .unwrap_or(c.name)
        .split(' ')
        .map(str::to_owned)
        .collect();
    let mut flags = Vec::new();
    let mut gap: Option<&str> = None;
    let mut rest: Option<String> = None;
    for p in c.params {
        let value = args.get(p.name).filter(|v| !v.is_null());
        let text = match value {
            None => {
                if p.required {
                    return Err(format!("{} is required", p.name));
                }
                if !p.flag {
                    gap.get_or_insert(p.name);
                }
                continue;
            }
            Some(Value::Bool(b)) => {
                if p.ty != Ty::Bool {
                    return Err(format!("{} must be a {}", p.name, ty_name(p.ty)));
                }
                if *b {
                    if p.flag {
                        flags.push(format!("--{}", p.name));
                    } else {
                        words.push(p.name.to_owned());
                    }
                }
                continue;
            }
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(other) => {
                return Err(format!(
                    "{} must be a {}, not {other}",
                    p.name,
                    ty_name(p.ty)
                ));
            }
        };
        match p.ty {
            Ty::Int if text.parse::<i64>().is_err() => {
                return Err(format!("{} must be an integer, not {text}", p.name));
            }
            Ty::Num if text.parse::<f64>().is_err() => {
                return Err(format!("{} must be a number, not {text}", p.name));
            }
            Ty::Enum(values) if !values.contains(&text.as_str()) => {
                return Err(format!("{} must be one of {values:?}, not {text}", p.name));
            }
            _ => {}
        }
        if p.flag {
            flags.push(format!("--{}", p.name));
            flags.push(text);
        } else {
            if let Some(missing) = gap {
                return Err(format!("{} needs {missing} to be given too", p.name));
            }
            if p.ty == Ty::Rest {
                rest = Some(text);
            } else if text.contains(char::is_whitespace) || text.is_empty() {
                return Err(format!("{} must be a single word", p.name));
            } else {
                words.push(text);
            }
        }
    }
    // Options go before free text, which runs to the end of the line.
    words.extend(flags);
    words.extend(rest);
    Ok(words)
}

fn ty_name(ty: Ty) -> &'static str {
    match ty {
        Ty::Int => "integer",
        Ty::Num => "number",
        Ty::Bool => "boolean",
        _ => "string",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_documented() {
        let mut seen = std::collections::HashSet::new();
        for c in COMMANDS {
            assert!(seen.insert(c.name), "duplicate command {}", c.name);
            assert!(
                c.usage.starts_with(c.name.split('-').next().unwrap()),
                "{}",
                c.name
            );
            assert!(
                !c.summary.is_empty() && !c.examples.is_empty(),
                "{}",
                c.name
            );
        }
    }

    #[test]
    fn mcp_arguments_become_command_lines() {
        let goto = find("goto").unwrap();
        let words = line_for(
            goto,
            &json!({ "x": 1.5, "y": 64, "z": -3, "near": 2, "sprint": true }),
        )
        .unwrap();
        assert_eq!(
            words,
            ["goto", "1.5", "64", "-3", "--near", "2", "--sprint"]
        );
        assert!(
            line_for(goto, &json!({ "x": 1, "y": 2 }))
                .unwrap_err()
                .contains("z is required")
        );
        assert!(line_for(goto, &json!({ "x": "a", "y": 2, "z": 3 })).is_err());
        let block = find("look-at-block").unwrap();
        assert_eq!(
            line_for(block, &json!({ "x": 1, "y": 2, "z": 3, "face": "up" })).unwrap(),
            ["look-at", "block", "1", "2", "3", "up"]
        );
        assert!(
            line_for(
                block,
                &json!({ "x": 1, "y": 2, "z": 3, "face": "sideways" })
            )
            .is_err()
        );
        let turn = find("turn").unwrap();
        assert_eq!(
            line_for(turn, &json!({ "dyaw": -90, "dpitch": 10.5, "over": 0.5 })).unwrap(),
            ["turn", "-90", "10.5", "--over", "0.5"]
        );
        let mouse = find("mouse-move").unwrap();
        assert_eq!(tool_name(mouse), "mouse_move");
        assert_eq!(
            line_for(mouse, &json!({ "dx": -25, "dy": 40 })).unwrap(),
            ["mouse-move", "-25", "40"]
        );
        assert_eq!(
            line_for(block, &json!({ "x": 1, "y": 2, "z": 3, "over": 0.3 })).unwrap(),
            ["look-at", "block", "1", "2", "3", "--over", "0.3"]
        );
        let button = find("click-button").unwrap();
        assert_eq!(
            line_for(button, &json!({ "label": "Create New World" })).unwrap(),
            ["click-button", "Create New World"]
        );
        let slot = find("click-slot").unwrap();
        assert_eq!(
            line_for(slot, &json!({ "slot": 200, "shift": true })).unwrap(),
            ["click-slot", "200", "shift"]
        );
        let open = find("open").unwrap();
        assert!(
            line_for(open, &json!({ "world": "A", "mode": "creative" }))
                .unwrap_err()
                .contains("seed")
        );
    }

    #[test]
    fn command_schema_enum_choices_are_unique() {
        for command in COMMANDS {
            for parameter in command.params {
                if let Ty::Enum(choices) = parameter.ty {
                    let unique: std::collections::BTreeSet<_> = choices.iter().collect();
                    assert_eq!(
                        unique.len(),
                        choices.len(),
                        "duplicate choice in {}.{}",
                        command.name,
                        parameter.name
                    );
                }
            }
        }
    }

    #[test]
    fn schemas_list_required_parameters() {
        let s = schema(find("break").unwrap());
        assert_eq!(s["required"], json!(["x", "y", "z"]));
        assert_eq!(s["properties"]["timeout"]["type"], "number");
        assert_eq!(tool_name(find("look-at-entity").unwrap()), "look_at_entity");
    }

    #[test]
    fn every_command_in_the_reply_group_names_its_reply_fields() {
        for name in REPLY_GROUP {
            let c = find(name).unwrap_or_else(|| panic!("{name} is not a command"));
            assert!(!c.reply.is_empty(), "{name} names no reply fields");
            let mut seen = std::collections::HashSet::new();
            for fld in c.reply {
                assert!(seen.insert(fld.name), "{name}: {} twice", fld.name);
                assert!(!fld.help.is_empty(), "{name}.{} has no meaning", fld.name);
                for t in fld.ty.split('|') {
                    assert!(
                        [
                            "string", "integer", "number", "boolean", "array", "object", "null"
                        ]
                        .contains(&t),
                        "{name}.{}: unknown type {t}",
                        fld.name
                    );
                }
            }
            assert_eq!(
                reply_schema(c).unwrap()["required"]
                    .as_array()
                    .unwrap()
                    .len(),
                c.reply.len()
            );
        }
        // Nothing outside the group claims a reply the group list does not cover.
        for c in COMMANDS.iter().filter(|c| !c.reply.is_empty()) {
            assert!(
                REPLY_GROUP.contains(&c.name),
                "{} has fields but is not in REPLY_GROUP",
                c.name
            );
        }
        assert_eq!(
            reply_schema(find("hello").unwrap()).unwrap()["properties"]["features"]["type"],
            "array"
        );
        assert_eq!(
            reply_schema(find("status").unwrap()).unwrap()["properties"]["player"]["type"],
            json!(["object", "null"])
        );
    }

    #[test]
    fn the_generated_reference_is_up_to_date() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/horizon-ctl-commands.md"
        );
        let on_disk = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            on_disk == markdown(),
            "docs/horizon-ctl-commands.md is out of date: run `horizon-ctl help --markdown > docs/horizon-ctl-commands.md`"
        );
        for c in COMMANDS {
            assert!(
                on_disk.contains(&format!("`{}", c.usage.replace('|', "\\|"))),
                "{} undocumented",
                c.name
            );
        }
    }

    #[test]
    fn destructive_and_network_tools_exist() {
        for name in DESTRUCTIVE.iter().chain(OPEN_WORLD) {
            assert!(find(name).is_some_and(|c| c.mcp), "{name}");
        }
        let delete = find("world-delete").unwrap();
        assert_eq!(
            line_for(delete, &json!({ "folder": "A", "confirm": true })).unwrap(),
            ["world-delete", "A", "--confirm"]
        );
        let lan = find("lan").unwrap();
        assert_eq!(
            line_for(lan, &json!({ "action": "open", "commands": true })).unwrap(),
            ["lan", "open", "commands"]
        );
    }

    #[test]
    fn help_mentions_every_command() {
        let text = overview();
        for c in COMMANDS {
            assert!(text.contains(c.name));
            assert!(detail(c).contains("usage: horizon-ctl"));
        }
    }
}
