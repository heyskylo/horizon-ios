//! Command execution (the `execute` side of each vanilla command class):
//! entity selectors resolved against the game, vanilla feedback and errors.
mod admin;
pub(super) mod compute;
mod data;
pub(super) use data::place_summoned;
mod effects;
pub(super) mod execute;
pub(super) mod extra;
pub(in crate::game) mod items;
mod misc;
mod place;
mod players;
pub(super) mod pregen;
mod profiles;
mod raid;
mod selector_bounds;
mod terrain_cmds;
mod test_locate;
mod test_runs;
mod text_commands;
mod world_cmds;

use super::{Dim, Game, Screen, chat::PLAYER_NAME};
use horizon_client::{
    command::{
        Arg, CommandError, Context, Reader,
        args::{BlockInput, Coordinates, GameMode, ItemInput, Selector, Sort},
        vanilla::{self, DIFFICULTIES},
    },
    gamerules::{self, Value},
    items::ItemStack,
    text::{self, Style, Text},
};
use horizon_core::{geometry::Vec3, random::RandomSource};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_world::registry::BlockStateId;

pub(super) type R<T> = Result<T, CommandError>;

/// An entity a selector found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Target {
    /// The player in place: the host's, or the guest whose turn it is.
    Player,
    /// Another player of a hosted world or server: a guest by connection
    /// (game/commands/players.rs).
    Guest(u32),
    Mob(u32),
    Item(u32),
    /// An experience orb, by its entity id.
    Orb(u32),
    /// A projectile, by its entity id.
    Projectile(u32),
    Stand(u32),
    Frame(u32),
    Painting(u32),
    Boat(u32),
    Cart(u32),
    /// An entity of a type Horizon does not simulate (markers, displays,
    /// interactions...): kept as its saved data at a position.
    Other(u32),
    /// Active evoker fangs, including command-loaded ones.
    Fangs(u32),
    /// An End crystal, by its entity id (`minecraft:end_crystal`).
    Crystal(u32),
    /// The Ender Dragon, by its entity id (`minecraft:ender_dragon`).
    Dragon(u32),
    /// A primed TNT, by its entity id (`minecraft:tnt`).
    Tnt(u32),
    /// A cushion, by its entity id (`minecraft:cushion`).
    Cushion(u32),
    /// A falling block, by its entity id (`minecraft:falling_block`).
    Falling(u32),
    /// A firework rocket, by its entity id (`minecraft:firework_rocket`).
    Rocket(u32),
    /// A thrown trident, by its entity id (`minecraft:trident`).
    Trident(u32),
    /// An ominous item spawner, by its entity id (`minecraft:ominous_item_spawner`).
    Spawner(u32),
    /// A simulated lingering, creeper or dragon area-effect cloud.
    Cloud(u32),
}
/// `Entity.setXRot`: reduce the angle before clamping the pitch.
fn entity_pitch(pitch: f32) -> f32 {
    (pitch % 360.0).clamp(-90.0, 90.0)
}

/// An entity of a type Horizon does not simulate: its compound lives in the
/// entity memory (`persist.raw`), its position and rotation here.
#[derive(Clone, Debug)]
pub(in crate::game) struct OtherEntity {
    pub id: u32,
    pub kind: String,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub item_transformation: Option<super::native_entities::ItemDisplayTransformation>,
    /// Only explicitly loaded leash knots own this BlockAttachedEntity check counter.
    pub knot_check: Option<u8>,
}
pub(super) fn err(key: &str, args: &[&dyn std::fmt::Display]) -> CommandError {
    CommandError::tr(key, args)
}
/// `EntityType.getDescriptionId`: `entity.<namespace>.<path>`.
fn entity_name_key(id: &str) -> String {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("entity.{namespace}.{}", path.replace('/', "."))
}
fn texts(args: &[Text]) -> Vec<Text> {
    args.to_vec()
}
/// Java's String.valueOf(float/double) for the values commands print.
fn java_float(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1.0e7 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}
/// String.format(Locale.ROOT, "%.<decimals>f"): Java rounds the shortest
/// decimal text half-up, which `{:.N}` (exact value, half-even) does not.
pub(super) fn percent_f(v: f64, decimals: usize) -> String {
    horizon_client::java_text::format_fixed(v, decimals)
}
/// String.format(Locale.ROOT, "%f").
fn format_double(v: f64) -> String {
    percent_f(v, 6)
}
/// `EntitySelectorParser.createRotationPredicate`: the wrapped angle lies in
/// the wrapped range, which runs through 180 when its minimum is above its
/// maximum (defaults 0 and 359).
fn rotation_in((lo, hi): (Option<f64>, Option<f64>), angle: f32) -> bool {
    let min = wrap_degrees(lo.unwrap_or(0.0) as f32);
    let max = wrap_degrees(hi.unwrap_or(359.0) as f32);
    let angle = wrap_degrees(angle);
    if min > max {
        angle >= min || angle <= max
    } else {
        angle >= min && angle <= max
    }
}
/// `Mth.wrapDegrees`.
fn wrap_degrees(angle: f32) -> f32 {
    let wrapped = angle % 360.0;
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else if wrapped < -180.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

/// The network id of a target that is no player: its entity id.
fn non_player_entity_id(t: Target) -> Option<i32> {
    let (Target::Mob(id)
    | Target::Item(id)
    | Target::Orb(id)
    | Target::Projectile(id)
    | Target::Stand(id)
    | Target::Frame(id)
    | Target::Painting(id)
    | Target::Boat(id)
    | Target::Cart(id)
    | Target::Other(id)
    | Target::Fangs(id)
    | Target::Crystal(id)
    | Target::Dragon(id)
    | Target::Tnt(id)
    | Target::Cushion(id)
    | Target::Falling(id)
    | Target::Rocket(id)
    | Target::Trident(id)
    | Target::Spawner(id)
    | Target::Cloud(id)) = t
    else {
        return None;
    };
    i32::try_from(id).ok()
}
impl Game<'_> {
    /// `native_player_level_change` for a level that was `from` before an experience change.
    pub(super) fn native_player_level_changed(&mut self, from: i32) {
        let to = self.experience.level;
        if from != to
            && !horizon_client::mod_api::mods().is_empty()
            && let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
        {
            self.native_player_level_change(player, &name, from, to);
        }
    }

    /// `PlayerXpEvent.XpChange` for a `Player.giveExperiencePoints` of `amount`
    /// points: the amount to give, or `None` when a mod cancelled it.
    pub(super) fn xp_change(&mut self, amount: i32) -> Option<i32> {
        if horizon_client::mod_api::mods().is_empty() {
            return Some(amount);
        }
        let (Some(player), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone())
        else {
            return Some(amount);
        };
        let (mut amount, mut cancel) = (amount, false);
        self.native_player_xp_change(player, &name, &mut amount, &mut cancel);
        (!cancel).then_some(amount)
    }

    /// Commands.performCommand for a line without its slash.
    pub(super) fn run_command(&mut self, command: &str) -> bool {
        if self.client_mod_command(command) {
            return true;
        }
        let d = vanilla::game_dispatcher_with_features(&self.level.enabled_features);
        let parse = d.parse(command, &self.ui.registries);
        let result = match parse.executable() {
            Err(e) => {
                self.syntax_failure(&e, command);
                return false;
            }
            Ok((ctx, _)) => {
                let ctx = ctx.clone();
                let root = parse.context.clone();
                self.exec = Default::default();
                self.run_chain(&root, &ctx)
            }
        };
        let ok = result.is_ok();
        if let Err(e) = result {
            if e.input.is_some() {
                self.syntax_failure(&e, command);
            } else {
                self.command_failure(e.message);
            }
        }
        // InventoryChangeTrigger fires as slots change, not on the next tick.
        if self.player.is_some() {
            self.check_inventory_trigger();
        }
        ok
    }
    /// `run_command` with its output suppressed
    /// (`CommandSourceStack.withSuppressedOutput`): the player's own source,
    /// rights and checks, no feedback and no failure in the chat.
    pub(super) fn run_command_quietly(&mut self, command: &str) -> bool {
        let quiet = execute::Source {
            silent: true,
            ..self.player_source()
        };
        let outer = self.command_source.replace(quiet);
        let ok = self.run_command(command);
        self.command_source = outer;
        ok
    }
    fn syntax_failure(&mut self, e: &CommandError, command: &str) {
        self.command_failure(e.message.clone());
        let Some(input) = &e.input else { return };
        let cursor = e.cursor.min(input.len());
        let suggest = Style {
            click: Some(text::ClickEvent::SuggestCommand(format!("/{command}"))),
            ..Style::default()
        };
        let mut context = Text::empty();
        if cursor > 10 {
            context.push("...");
        }
        let start = (0..=cursor.saturating_sub(10))
            .rev()
            .find(|&i| input.is_char_boundary(i))
            .unwrap_or(0);
        context.push(&input[start..cursor]);
        if cursor < input.len() {
            context.push(
                Text::literal(&input[cursor..])
                    .with_color(text::RED)
                    .underlined(),
            );
        }
        context.push(
            Text::translate("command.context.here", &[])
                .with_color(text::RED)
                .italic(),
        );
        self.command_failure(context.with_color(text::GRAY).with_style(&suggest));
    }
    /// CommandSourceStack.sendFailure: red text.
    pub(super) fn command_failure(&mut self, message: Text) {
        if self.command_source.is_some_and(|s| s.silent) {
            return;
        }
        let message = Text::empty().append(message).with_color(text::RED);
        if let Some(block) = self.command_source.and_then(|s| s.block) {
            self.command_block_output(block, message);
            return;
        }
        if let Some(cart) = self.command_source.and_then(|s| s.cart) {
            self.cart_command_output(cart, message);
            return;
        }
        self.system_message(message);
    }
    /// `sendSuccess(message, false)`: the source is told (when it takes
    /// success feedback), the operators are not.
    pub(super) fn feedback_unbroadcast(&mut self, message: Text) {
        if self.command_source.is_some_and(|s| s.silent) {
            return;
        }
        if let Some(block) = self.command_source.and_then(|s| s.block) {
            self.command_block_success(block, message, false);
            return;
        }
        if let Some(cart) = self.command_source.and_then(|s| s.cart) {
            self.cart_command_success(cart, message, false);
            return;
        }
        if self.accepts_success() {
            self.system_message(message);
        }
    }
    /// `CommandSource.acceptsSuccess`: a player's source takes success feedback while
    /// `send_command_feedback` is on; the server console's and an RCON client's always do.
    fn accepts_success(&self) -> bool {
        self.console_broadcast.is_some() || self.ui.rules.bool("send_command_feedback")
    }
    /// sendSuccess honouring `send_command_feedback`.
    pub(super) fn feedback(&mut self, message: Text) {
        if self.command_source.is_some_and(|s| s.silent) {
            return;
        }
        if let Some(block) = self.command_source.and_then(|s| s.block) {
            self.command_block_success(block, message, true);
            return;
        }
        if let Some(cart) = self.command_source.and_then(|s| s.cart) {
            self.cart_command_success(cart, message, true);
            return;
        }
        if self.accepts_success() {
            self.system_message(message.clone());
        }
        self.broadcast_to_admins(message);
    }
    /// `CommandSourceStack.broadcastToAdmins` for a command block or a command
    /// minecart called `name`: `[name: message]`, gray and italic, for every
    /// operator when `send_command_feedback` and for a dedicated server's console when
    /// `log_admin_commands`. Queued: `send_admin_feedback` sends it.
    pub(super) fn broadcast_source_to_admins(&mut self, name: Text, message: Text) {
        if message
            .translation_key()
            .is_some_and(|k| admin::QUIET.contains(&k))
        {
            return;
        }
        let line = Text::translate("chat.type.admin", &[name, message])
            .with_color(text::GRAY)
            .italic();
        let ops = self.ui.rules.bool("send_command_feedback");
        let console = self.options.dedicated.is_some() && self.ui.rules.bool("log_admin_commands");
        if ops || console {
            self.admin_feedback.push((line, ops, console));
        }
    }
    /// CommandSourceStack.broadcastToAdmins (`sendSuccess` with `broadcast`)
    /// for a guest's command, the host's in a world open to LAN and the
    /// dedicated server console's when `broadcast-console-to-ops` is on:
    /// `[name: message]`, gray and italic, for the other operators when
    /// `send_command_feedback`, and for a player's command on a dedicated
    /// server the console when `log_admin_commands`. Queued while the
    /// command runs; `send_admin_feedback` sends it.
    pub(super) fn broadcast_to_admins(&mut self, message: Text) {
        let informs = match self.console_broadcast {
            Some(broadcast) => broadcast,
            // While a guest acts the host's LanHost is out of `self.lan`.
            None => self.lan_acting.is_some() || self.lan.is_some(),
        };
        if !informs
            || message
                .translation_key()
                .is_some_and(|k| admin::QUIET.contains(&k))
        {
            return;
        }
        let ops = self.ui.rules.bool("send_command_feedback");
        // MinecraftServer.sendSystemMessage, unless the server is the source.
        let console = self.console_broadcast.is_none()
            && self.lan_acting.is_some()
            && self.options.dedicated.is_some()
            && self.ui.rules.bool("log_admin_commands");
        if !ops && !console {
            return;
        }
        // getDisplayName: the entity the command runs as (`execute as`), or
        // the server's own source, "Server".
        let who = match self.command_source.and_then(|s| s.entity) {
            Some(entity) => self.target_name(entity),
            None if self.console_broadcast.is_some() => {
                Text::literal(if self.rcon_source { "Rcon" } else { "Server" }.to_owned())
            }
            None => self.player_display_name(),
        };
        let line = Text::translate("chat.type.admin", &[who, message])
            .with_color(text::GRAY)
            .italic();
        self.admin_feedback.push((line, ops, console));
    }
    /// Player.getDisplayName: the name (team-formatted), suggesting a
    /// `/tell`, showing the player on hover, inserting the name.
    pub(super) fn player_display_name(&self) -> Text {
        let name = self.acting_player_name().to_owned();
        let Some(uuid) = self.lan_acting_uuid else {
            return self.target_name(Target::Player);
        };
        self.target_name(Target::Player).with_component_fields(vec![
            (
                "click_event".into(),
                serde_json::json!({"action": "suggest_command", "command": format!("/tell {name} ")}),
            ),
            (
                "hover_event".into(),
                serde_json::json!({"action": "show_entity", "id": "minecraft:player",
                    "uuid": horizon_protocol::crypto::uuid_string(uuid), "name": name}),
            ),
            ("insertion".into(), name.into()),
        ])
    }
    fn source_position(&self) -> [f64; 3] {
        if let Some(s) = &self.command_source {
            return s.pos;
        }
        match &self.player {
            Some(p) => [p.position.x, p.position.y, p.position.z],
            None => self.server_position(),
        }
    }
    /// MinecraftServer.createCommandSourceStack's position: the corner of
    /// the respawn block (`Vec3.atLowerCornerOf`).
    pub(super) fn server_position(&self) -> [f64; 3] {
        self.spawn.map_or([0.0, 64.0, 0.0], |s| {
            [s.x.floor(), s.y.floor(), s.z.floor()]
        })
    }
    fn source_rotation(&self) -> [f32; 2] {
        self.command_source
            .map_or([self.yaw, self.pitch], |s| s.rot)
    }
    /// LocalCoordinates use the anchored position; world coordinates the position.
    pub(super) fn resolve_coords(&self, c: &Coordinates) -> [f64; 3] {
        let origin = match (c, &self.command_source) {
            (Coordinates::Local(_), Some(s)) => self.anchor_position(s),
            _ => self.source_position(),
        };
        c.position(origin, self.source_rotation())
    }
    pub(super) fn coords(&self, ctx: &Context, name: &str) -> Option<Coordinates> {
        match ctx.get(name) {
            Some(Arg::Coords(c)) => Some(c.clone()),
            _ => None,
        }
    }
    pub(super) fn position_arg(&self, ctx: &Context, name: &str) -> Option<[f64; 3]> {
        self.coords(ctx, name).map(|c| self.resolve_coords(&c))
    }
    /// BlockPosArgument.getLoadedBlockPos.
    pub(super) fn block_pos_arg(&self, ctx: &Context, name: &str) -> R<[i32; 3]> {
        let c = self
            .coords(ctx, name)
            .ok_or_else(|| err("argument.pos.missing.int", &[]))?;
        // BlockPos.containing(getPosition)
        let p = self.resolve_coords(&c).map(|v| v.floor() as i32);
        let dimension = self.command_source.map_or(self.dim, |s| s.dimension);
        if dimension != self.dim || !self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4)) {
            return Err(err("argument.pos.unloaded", &[]));
        }
        if p[1] < self.world.bottom_y() || p[1] >= self.world.top_y() {
            return Err(err("argument.pos.outofworld", &[]));
        }
        Ok(p)
    }
    fn int(ctx: &Context, name: &str) -> Option<i32> {
        match ctx.get(name) {
            Some(Arg::Int(v)) => Some(*v),
            _ => None,
        }
    }
    // ---- entities -------------------------------------------------------

    /// The level `t` is in (`Entity.level`): a server's players have their
    /// own, which need not be the one in place; everything else is in the
    /// level in place.
    pub(super) fn target_level(&self, t: Target) -> Dim {
        let conn = match t {
            Target::Player => self.lan_acting,
            Target::Guest(conn) => Some(conn),
            _ => None,
        };
        conn.and_then(|c| self.command_guest(c))
            .map_or(self.dim, |g| Dim::from_id(&g.dimension))
    }
    pub(super) fn target_position(&self, t: Target) -> Option<[f64; 3]> {
        match t {
            Target::Player => self
                .player
                .as_ref()
                .map(|p| [p.position.x, p.position.y, p.position.z]),
            Target::Guest(conn) => self.command_guest(conn).map(|g| g.position),
            Target::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map(|m| [m.position.x, m.position.y, m.position.z]),
            Target::Item(id) => self
                .dropped
                .iter()
                .find(|i| i.id == id)
                .map(|i| [i.position.x, i.position.y, i.position.z]),
            Target::Orb(id) => self
                .orbs
                .iter()
                .find(|o| o.id == id)
                .map(|o| [o.position.x, o.position.y, o.position.z]),
            Target::Projectile(id) => self
                .creatures
                .projectiles
                .iter()
                .find(|a| a.id == id)
                .map(|a| [a.position.x, a.position.y, a.position.z]),
            Target::Stand(id) => self
                .armor_stands
                .iter()
                .find(|a| a.id == id)
                .map(|a| [a.position.x, a.position.y, a.position.z]),
            Target::Frame(id) => self.item_frames.iter().find(|f| f.id == id).map(|f| {
                let (lo, hi) = f.bounds();
                [
                    (lo[0] + hi[0]) / 2.0,
                    (lo[1] + hi[1]) / 2.0,
                    (lo[2] + hi[2]) / 2.0,
                ]
            }),
            Target::Painting(id) => self.paintings.iter().find(|p| p.id == id).map(|p| {
                let (lo, hi) = p.bounds();
                [
                    (lo[0] + hi[0]) / 2.0,
                    (lo[1] + hi[1]) / 2.0,
                    (lo[2] + hi[2]) / 2.0,
                ]
            }),
            Target::Boat(id) => self
                .boats
                .iter()
                .find(|b| b.id == id)
                .map(|b| [b.position.x, b.position.y, b.position.z]),
            Target::Cart(id) => self
                .carts
                .iter()
                .find(|c| c.id == id)
                .map(|c| [c.position.x, c.position.y, c.position.z]),
            Target::Other(id) => self
                .others
                .iter()
                .find(|o| o.id == id)
                .map(|o| [o.position.x, o.position.y, o.position.z]),
            Target::Crystal(id) => self
                .crystals
                .iter()
                .find(|c| c.id == id)
                .map(|c| [c.position.x, c.position.y, c.position.z]),
            Target::Dragon(id) => self
                .dragon
                .as_ref()
                .filter(|d| d.id == id)
                .map(|d| [d.position.x, d.position.y, d.position.z]),
            Target::Cushion(id) => self
                .cushions
                .iter()
                .find(|c| c.id == id)
                .map(|c| [c.position.x, c.position.y, c.position.z]),
            Target::Tnt(id) => self
                .tnts
                .iter()
                .find(|t| t.id == id)
                .map(|t| [t.position.x, t.position.y, t.position.z]),
            Target::Falling(id) => self
                .falling
                .iter()
                .find(|f| f.id == id)
                .map(|f| [f.position.x, f.position.y, f.position.z]),
            Target::Rocket(id) => self
                .rockets
                .iter()
                .find(|r| r.id == id)
                .map(|r| [r.position.x, r.position.y, r.position.z]),
            Target::Cloud(id) => self.breath_clouds.iter().find(|c| c.id == id).map(|c| c.at),
            Target::Trident(id) => self
                .tridents
                .iter()
                .find(|t| t.id == id)
                .map(|t| [t.position.x, t.position.y, t.position.z]),
            Target::Fangs(id) => self.fangs.list.iter().find(|f| f.id == id).map(|f| f.pos),
            Target::Spawner(id) => self
                .ominous_spawners
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.position),
        }
    }
    pub(super) fn target_type(&self, t: Target) -> String {
        match t {
            Target::Player | Target::Guest(_) => "minecraft:player".into(),
            Target::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map_or_else(String::new, |m| {
                    horizon_client::command::args::normalize(&m.kind.id)
                }),
            Target::Item(_) => "minecraft:item".into(),
            Target::Orb(_) => "minecraft:experience_orb".into(),
            Target::Projectile(id) => self
                .creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .map_or_else(String::new, |p| p.kind.id().into()),
            Target::Stand(_) => "minecraft:armor_stand".into(),
            Target::Frame(id) => {
                if self.item_frames.iter().any(|f| f.id == id && f.glow) {
                    "minecraft:glow_item_frame".into()
                } else {
                    "minecraft:item_frame".into()
                }
            }
            Target::Painting(_) => "minecraft:painting".into(),
            Target::Boat(id) => self
                .boats
                .iter()
                .find(|b| b.id == id)
                .map_or_else(|| "minecraft:oak_boat".into(), |b| b.entity_type()),
            Target::Cart(id) => self
                .carts
                .iter()
                .find(|c| c.id == id)
                .map_or_else(|| "minecraft:minecart".into(), |c| c.kind.id().into()),
            Target::Other(id) => self
                .others
                .iter()
                .find(|o| o.id == id)
                .map_or_else(String::new, |o| o.kind.clone()),
            Target::Crystal(_) => "minecraft:end_crystal".into(),
            Target::Dragon(_) => "minecraft:ender_dragon".into(),
            Target::Tnt(_) => "minecraft:tnt".into(),
            Target::Cushion(_) => "minecraft:cushion".into(),
            Target::Falling(_) => "minecraft:falling_block".into(),
            Target::Rocket(_) => "minecraft:firework_rocket".into(),
            Target::Trident(_) => "minecraft:trident".into(),
            Target::Fangs(_) => "minecraft:evoker_fangs".into(),
            Target::Spawner(_) => "minecraft:ominous_item_spawner".into(),
            Target::Cloud(_) => "minecraft:area_effect_cloud".into(),
        }
    }
    /// The name of the player the game acts for: a guest's own while their
    /// command or action runs (`Player.getName`).
    pub(super) fn acting_player_name(&self) -> &str {
        self.lan_acting_name.as_deref().unwrap_or(PLAYER_NAME)
    }
    /// Entity.getDisplayName.
    pub(super) fn target_name(&self, t: Target) -> Text {
        match t {
            Target::Player => {
                let name = self.acting_player_name();
                if let Some(uuid) = self.lan_acting_uuid {
                    return self.player_text(name, uuid);
                }
                match self.score.board.players_team(name) {
                    // PlayerTeam.formatNameForTeam.
                    Some(team) => team.formatted_name(Text::literal(name)),
                    None => Text::literal(name),
                }
            }
            Target::Guest(conn) => match self.command_guest(conn) {
                Some(g) => self.player_text(&g.name, g.uuid),
                None => Text::empty(),
            },
            // ItemEntity.getName: its CustomName, else the stack's item name
            // (`getItemName`: `minecraft:item_name`, else the item's own).
            Target::Item(id) => {
                if let Some(c) = self.persist_raw(id).and_then(|r| r.get("CustomName")) {
                    return horizon_client::scoreboard::comp_text(
                        &horizon_client::scoreboard::tag_to_json(c),
                    );
                }
                let stack = self.dropped.iter().find(|i| i.id == id).map(|i| &i.stack);
                match stack.and_then(|s| s.item_name()) {
                    Some(name) => Text::literal(name),
                    None => {
                        let item = stack.map(|s| s.item.to_string()).unwrap_or_default();
                        Text::literal(self.pack.names.display_item(&item))
                    }
                }
            }
            // FallingBlockEntity.getTypeName: `entity.minecraft.falling_block_type`
            // with the block's name.
            Target::Falling(id) => {
                let block = self
                    .falling
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| self.res.facts().get(f.state).name.replace(':', "."))
                    .unwrap_or_default();
                let block = Text::translate(&format!("block.{block}"), &[]);
                Text::translate("entity.minecraft.falling_block_type", &[block])
            }
            Target::Mob(id) => self.mobs.iter().find(|m| m.id == id).map_or_else(
                || Text::translate("entity.", &[]),
                |m| match self.mob_team_of(m) {
                    Some(team) => team.formatted_name(m.command_name()),
                    None => m.command_name(),
                },
            ),
            other => {
                // Entity.getDisplayName: the CustomName component, else the type.
                let custom = match other {
                    Target::Stand(id)
                    | Target::Frame(id)
                    | Target::Painting(id)
                    | Target::Boat(id)
                    | Target::Cart(id)
                    | Target::Other(id)
                    | Target::Fangs(id)
                    | Target::Cloud(id) => self.persist_raw(id).and_then(|r| r.get("CustomName")),
                    _ => None,
                };
                if let Some(c) = custom {
                    return horizon_client::scoreboard::comp_text(
                        &horizon_client::scoreboard::tag_to_json(c),
                    );
                }
                let ty = self.target_type(other);
                Text::translate(&format!("entity.{}", ty.replace(':', ".")), &[])
            }
        }
    }
    /// EntitySelector.findEntities.
    pub(super) fn select(&mut self, s: &Selector) -> Vec<Target> {
        if let Some(name) = &s.player_name {
            return self.player_named(name).into_iter().collect();
        }
        if let Some(uuid) = s.uuid {
            return self.target_by_uuid(uuid).into_iter().collect();
        }
        let origin = {
            let p = self.source_position();
            [
                s.pos[0].unwrap_or(p[0]),
                s.pos[1].unwrap_or(p[1]),
                s.pos[2].unwrap_or(p[2]),
            ]
        };
        let mut source_candidates: Vec<Target> = Vec::new();
        match (s.kind, self.command_source) {
            // @s: the executing entity (none for a server source).
            ('s', Some(source)) => source_candidates.extend(source.entity),
            ('s', None) => source_candidates.extend(self.source_entity()),
            // A guest in place is among the players, in join order.
            _ if self.player.is_some() && self.lan_acting.is_none() => {
                source_candidates.push(Target::Player);
            }
            _ => {}
        }
        if s.kind != 's' {
            source_candidates.extend(self.player_candidates(s));
        }
        let source_dim = self.dim;
        // A selector with a spatial constraint reaches the level the command
        // runs in only; an unbounded one reaches every loaded level
        // (EntitySelector.findEntities).
        let dims = if s.world_limited() {
            vec![source_dim]
        } else {
            self.loaded_dims()
        };
        let mut out: Vec<(Target, f64)> = Vec::new();
        for dim in dims {
            let back = self.enter_command_level(dim);
            let mut candidates = Vec::new();
            if dim == source_dim {
                candidates.append(&mut source_candidates);
            }
            if s.kind != 's' {
                candidates.extend(self.level_candidates());
            }
            out.extend(self.select_in_level(candidates, s, origin));
            if let Some(back) = back {
                self.enter_command_level(back);
            }
        }
        match s.sort {
            Sort::Nearest => out.sort_by(|a, b| a.1.total_cmp(&b.1)),
            Sort::Furthest => out.sort_by(|a, b| b.1.total_cmp(&a.1)),
            Sort::Random => {
                for i in (1..out.len()).rev() {
                    let j = self.world.random.next_int_bound(i as i32 + 1).unwrap_or(0) as usize;
                    out.swap(i, j);
                }
            }
            Sort::Arbitrary => {}
        }
        out.into_iter()
            .take(s.limit.max(1))
            .map(|(t, _)| t)
            .collect()
    }
    /// The candidates of the level in place that pass the selector
    /// (EntitySelector.getPredicate).
    fn select_in_level(
        &mut self,
        candidates: Vec<Target>,
        s: &Selector,
        origin: [f64; 3],
    ) -> Vec<(Target, f64)> {
        let level = self.experience.level;
        let creative = self.creative;
        let passing = self.selector_extra_filter(&candidates, s);
        candidates
            .into_iter()
            .filter_map(|t| {
                let p = self.target_position(t)?;
                let ty = self.target_type(t);
                // type=: every positive must match, no negative may match.
                for (want, neg) in &s.types {
                    let matches = if let Some(tag) = want.strip_prefix('#') {
                        self.entity_tag_matches(tag, &ty)
                    } else {
                        *want == ty
                    };
                    if matches == *neg {
                        return None;
                    }
                }
                if !s.include_entities
                    && !t.is_player()
                    && s.types.iter().all(|(w, n)| *n || w != &ty)
                    && s.kind != 'e'
                    && s.kind != 'n'
                {
                    return None;
                }
                for (name, neg) in &s.names {
                    let n = self.target_name(t).plain();
                    if (n == *name) == *neg {
                        return None;
                    }
                }
                if !passing.contains(&t) {
                    return None;
                }
                let player = match t {
                    Target::Player => Some((level, if creative { "creative" } else { "survival" })),
                    Target::Guest(conn) => self.guest_level_and_mode(conn),
                    _ => None,
                };
                if let Some((lo, hi)) = s.level
                    && player.is_none_or(|(level, _)| {
                        lo.is_some_and(|l| level < l) || hi.is_some_and(|h| level > h)
                    })
                {
                    return None;
                }
                for (mode, neg) in &s.gamemodes {
                    if player.is_none_or(|(_, current)| (current == mode) == *neg) {
                        return None;
                    }
                }
                let d2 = (p[0] - origin[0]).powi(2)
                    + (p[1] - origin[1]).powi(2)
                    + (p[2] - origin[2]).powi(2);
                if let Some((lo, hi)) = s.distance
                    && (lo.is_some_and(|l| d2 < l * l) || hi.is_some_and(|h| d2 > h * h))
                {
                    return None;
                }
                // x_rotation is the pitch, y_rotation the yaw (getXRot, getYRot).
                let [yaw, pitch] = self.target_rotation(t);
                if let Some(range) = s.x_rotation
                    && !rotation_in(range, pitch)
                {
                    return None;
                }
                if let Some(range) = s.y_rotation
                    && !rotation_in(range, yaw)
                {
                    return None;
                }
                if s.delta.iter().any(Option::is_some) {
                    let d = s.delta.map(|v| v.unwrap_or(0.0));
                    if let Some(bounds) = self.selector_target_bounds(t) {
                        if !selector_bounds::selector_box(origin, d).intersects(bounds) {
                            return None;
                        }
                    } else {
                        // Unmodeled boxes retain their previous point fallback.
                        for i in 0..3 {
                            let (a, b) = (
                                origin[i].min(origin[i] + d[i]),
                                origin[i].max(origin[i] + d[i]) + 1.0,
                            );
                            if p[i] < a || p[i] >= b {
                                return None;
                            }
                        }
                    }
                }
                Some((t, d2))
            })
            .collect()
    }
    /// The entities of the level in place a selector can find: one walk over
    /// the entity lists (a kind added there joins `@e` once it names its
    /// `Target`), then the kinds that are not on the walk. An
    /// unsorted selector lists them in that order.
    pub(in crate::game) fn level_candidates(&self) -> Vec<Target> {
        use super::entity::{Entity as _, entity_lists_of, for_each_entity};
        let mut candidates = Vec::new();
        for_each_entity!(entity_lists_of!(self), entity => {
            candidates.extend(entity.selector_target());
        });
        candidates.extend(self.item_frames.iter().map(|f| Target::Frame(f.id)));
        candidates.extend(self.paintings.iter().map(|p| Target::Painting(p.id)));
        candidates.extend(self.cushions.iter().map(|c| Target::Cushion(c.id)));
        candidates.extend(self.others.iter().map(|o| Target::Other(o.id)));
        candidates.extend(self.fangs.list.iter().map(|f| Target::Fangs(f.id)));
        // The End crystals and the dragon (EndCrystal and EnderDragon
        // are entities of their own).
        candidates.extend(self.crystals.iter().map(|c| Target::Crystal(c.id)));
        if let Some(d) = &self.dragon {
            candidates.push(Target::Dragon(d.id));
        }
        candidates.extend(self.ominous_spawners.iter().map(|s| Target::Spawner(s.id)));
        candidates.extend(
            self.breath_clouds
                .iter()
                .filter(|c| c.id != 0)
                .map(|c| Target::Cloud(c.id)),
        );
        candidates
    }
    /// The loaded levels, in the order the server holds them: the
    /// overworld, the nether, the end (MinecraftServer.getAllLevels).
    fn loaded_dims(&self) -> Vec<Dim> {
        let mut dims = vec![self.dim];
        dims.extend(self.levels.parked_dims());
        dims.sort_by_key(|d| match d {
            Dim::Overworld => 0,
            Dim::Nether => 1,
            Dim::End => 2,
        });
        dims
    }
    /// `Level.getEntity`: the entity of the level in place with a network
    /// id, players aside.
    pub(in crate::game) fn level_entity(&self, id: i32) -> Option<Target> {
        let id = u32::try_from(id).ok()?;
        [
            Target::Mob(id),
            Target::Item(id),
            Target::Orb(id),
            Target::Projectile(id),
            Target::Stand(id),
            Target::Frame(id),
            Target::Painting(id),
            Target::Boat(id),
            Target::Cart(id),
            Target::Other(id),
            Target::Fangs(id),
            Target::Crystal(id),
            Target::Dragon(id),
            Target::Tnt(id),
            Target::Cushion(id),
            Target::Falling(id),
            Target::Rocket(id),
            Target::Trident(id),
            Target::Spawner(id),
            Target::Cloud(id),
        ]
        .into_iter()
        .find(|t| self.level_holds(*t))
    }
    /// `ServerLevel.getEntity`: whether the level in place still holds the
    /// entity a target names. Discard removes mobs from lookup immediately.
    fn level_holds(&self, t: Target) -> bool {
        match t {
            // A player is in no level of its own.
            Target::Player | Target::Guest(_) => true,
            Target::Mob(id) => self.mobs.iter().any(|m| m.id == id && !m.removed),
            Target::Item(id) => self.dropped.iter().any(|i| i.id == id),
            Target::Orb(id) => self.orbs.iter().any(|o| o.id == id),
            Target::Projectile(id) => self.creatures.projectiles.iter().any(|p| p.id == id),
            Target::Stand(id) => self.armor_stands.iter().any(|a| a.id == id),
            Target::Frame(id) => self.item_frames.iter().any(|f| f.id == id),
            Target::Painting(id) => self.paintings.iter().any(|p| p.id == id),
            Target::Boat(id) => self.boats.iter().any(|b| b.id == id),
            Target::Cart(id) => self.carts.iter().any(|c| c.id == id),
            Target::Other(id) => self.others.iter().any(|o| o.id == id),
            Target::Fangs(id) => self.fangs.list.iter().any(|f| f.id == id),
            Target::Crystal(id) => self.crystals.iter().any(|c| c.id == id),
            Target::Dragon(id) => self.dragon.as_ref().is_some_and(|d| d.id == id),
            Target::Tnt(id) => self.tnts.iter().any(|t| t.id == id),
            Target::Cushion(id) => self.cushions.iter().any(|c| c.id == id),
            Target::Falling(id) => self.falling.iter().any(|f| f.id == id),
            Target::Rocket(id) => self.rockets.iter().any(|r| r.id == id),
            Target::Trident(id) => self.tridents.iter().any(|t| t.id == id),
            Target::Spawner(id) => self.ominous_spawners.iter().any(|s| s.id == id),
            Target::Cloud(id) => self.breath_clouds.iter().any(|c| c.id == id),
        }
    }
    /// Run `f` with the level that holds the entity a target names in place
    /// (the entity ids are unique across the loaded levels) and put the
    /// level back afterwards. `f` does not run when no level holds the
    /// entity.
    pub(in crate::game) fn in_target_level<R>(
        &mut self,
        t: Target,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if self.level_holds(t) {
            return Some(f(self));
        }
        for dim in self.levels.parked_dims() {
            let back = self.enter_command_level(dim);
            if self.level_holds(t) {
                let r = f(self);
                if let Some(back) = back {
                    self.enter_command_level(back);
                }
                return Some(r);
            }
            if let Some(back) = back {
                self.enter_command_level(back);
            }
        }
        None
    }
    fn entity_tag_matches(&self, tag: &str, ty: &str) -> bool {
        let path = self.options.resources.join(format!(
            "data/minecraft/tags/entity_type/{}.json",
            tag.trim_start_matches("minecraft:")
        ));
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap_or_default())
            .ok()
            .is_some_and(|v| {
                v["values"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|e| e.as_str() == Some(ty)))
            })
    }
    /// EntityArgument.getEntities / getPlayers / getEntity / getPlayer.
    pub(super) fn entities(&mut self, ctx: &Context, name: &str, players: bool) -> R<Vec<Target>> {
        let Some(Arg::Entity(s)) = ctx.get(name).cloned() else {
            return Ok(vec![self.implied_player()]);
        };
        let found: Vec<Target> = self
            .select(&s)
            .into_iter()
            .filter(|t| !players || t.is_player())
            .collect();
        if found.is_empty() {
            return Err(err(
                if players {
                    "argument.entity.notfound.player"
                } else {
                    "argument.entity.notfound.entity"
                },
                &[],
            ));
        }
        Ok(found)
    }
    // ---- dispatch -------------------------------------------------------

    fn execute_command(&mut self, id: &str, ctx: &Context) -> R<i32> {
        match id {
            "help" => self.cmd_help(ctx),
            "gamemode" => self.cmd_gamemode(ctx),
            "defaultgamemode" => {
                let Some(Arg::GameMode(m)) = ctx.get("gamemode") else {
                    return Ok(0);
                };
                self.set_default_game_mode(match m {
                    horizon_client::command::args::GameMode::Survival => 0,
                    horizon_client::command::args::GameMode::Creative => 1,
                    horizon_client::command::args::GameMode::Adventure => 2,
                    horizon_client::command::args::GameMode::Spectator => 3,
                });
                self.feedback(Text::translate(
                    "commands.defaultgamemode.success",
                    &[Text::translate(&format!("gameMode.{}", m.name()), &[])],
                ));
                Ok(0)
            }
            "give" => self.cmd_give(ctx),
            "teleport.location"
            | "teleport.destination"
            | "teleport.targets.location"
            | "teleport.targets.destination" => self.cmd_teleport(id, ctx),
            id if id.starts_with("time.") => self.cmd_time(id, ctx),
            id if id.starts_with("raid.") => self.cmd_raid(id, ctx),
            "weather" => self.cmd_weather(ctx),
            "kill" => self.cmd_kill(ctx),
            "summon" => self.cmd_summon(ctx),
            "effect.give" => self.cmd_effect_give(ctx),
            "effect.clear" => self.cmd_effect_clear(ctx),
            "enchant" => self.cmd_enchant(ctx),
            "tellraw" => self.cmd_tellraw(ctx),
            "title" => self.cmd_title(ctx),
            "setblock" => self.cmd_setblock(ctx),
            "fill" => self.cmd_fill(ctx),
            "clone" => self.cmd_clone(ctx),
            "experience" => self.cmd_experience(ctx),
            "advancement" => self.cmd_advancement(ctx),
            "gamerule" => self.cmd_gamerule(ctx),
            "difficulty" => self.cmd_difficulty(ctx),
            "seed" => {
                let seed = self.level.seed.to_string();
                let value = Text::copyable(seed);
                self.feedback_unbroadcast(Text::translate("commands.seed.success", &[value]));
                Ok(self.level.seed as i32)
            }
            "spawnpoint" => self.cmd_spawnpoint(ctx),
            "setworldspawn" => self.cmd_setworldspawn(ctx),
            "clear" => self.cmd_clear(ctx),
            "item" => self.cmd_item(ctx),
            "item.modify" => self.cmd_item_modify(ctx),
            "spectate" => self.cmd_spectate(ctx),
            id if id.starts_with("waypoint.") => self.cmd_waypoint(id, ctx),
            id if id.starts_with("dialog.") => self.cmd_dialog(id, ctx),
            id if id.starts_with("posteffect.") => self.cmd_posteffect(id, ctx),
            "kick" => self.cmd_kick(ctx),
            "op" | "deop" | "ban" | "pardon" => self.cmd_profiles(id, ctx),
            "ban-ip" | "pardon-ip" => self.cmd_ip_bans(id, ctx),
            id if id.starts_with("banlist") => self.cmd_banlist(id),
            id if id.starts_with("whitelist.") => self.cmd_whitelist(id, ctx),
            "setidletimeout" => self.cmd_setidle_timeout(ctx),
            "stop" => self.cmd_stop(),
            "save-all" => self.cmd_save_all(),
            "save-off" | "save-on" => self.cmd_auto_save(id == "save-on"),
            "transfer" => self.cmd_transfer(ctx),
            "fetchprofile" => self.cmd_fetchprofile(ctx),
            "publish" => self.cmd_publish(ctx),
            "unpublish" => self.cmd_unpublish(),
            "debug" => self.cmd_debug(ctx),
            "jfr" => self.cmd_jfr(ctx),
            "version" => self.cmd_version(),
            id if id.starts_with("test.") => self.cmd_test(id, ctx),
            "compute" => self.cmd_compute(ctx),
            "place" => self.cmd_place(ctx),
            "place.feature" | "place.jigsaw" | "place.structure" => {
                self.cmd_place_generated(id, ctx)
            }
            "recipe.give" | "recipe.take" => self.cmd_recipe(id, ctx),
            "damage" => self.cmd_damage(ctx),
            "rotate" => self.cmd_rotate(ctx),
            "swing" => self.cmd_swing(ctx),
            "playsound" => self.cmd_playsound(ctx),
            "stopsound" => self.cmd_stopsound(ctx),
            "particle" => self.cmd_particle(ctx),
            id if id.starts_with("attribute.") => {
                let result = self.cmd_attribute(id, ctx);
                // AttributeInstance.setDirty: the entity reacts when it ticks.
                if result.is_ok()
                    && matches!(ctx.get("attribute"), Some(Arg::Resource(a)) if a == "minecraft:waypoint_transmit_range")
                    && !id.ends_with("get")
                    && let Ok(t) = self.entities(ctx, "target", false)
                {
                    let target = self.waypoint_target(t[0]);
                    self.cmds.waypoint_dirty.push(target);
                }
                result
            }
            "ride.mount" | "ride.dismount" => self.cmd_ride(id, ctx),
            "spreadplayers" => self.cmd_spreadplayers(ctx),
            id if id.starts_with("forceload.") => self.cmd_forceload(id, ctx),
            id if id.starts_with("pregen.") => self.cmd_pregen(id, ctx),
            "fillbiome" => self.cmd_fillbiome(ctx),
            id if id.starts_with("worldborder.") => self.cmd_worldborder(id, ctx),
            id if id.starts_with("loot.") => self.cmd_loot(id, ctx),
            "teammsg" => self.cmd_teammsg(ctx),
            id if id.starts_with("stopwatch.") => self.cmd_stopwatch(id, ctx),
            id if id.starts_with("tick.") => self.cmd_tick(id, ctx),
            id if id.starts_with("random.") => self.cmd_random(id, ctx),
            "function" => self.cmd_function(ctx),
            "return.value" | "return.fail" => self.cmd_return(id, ctx),
            "schedule.function" | "schedule.clear" => self.cmd_schedule(id, ctx),
            "reload" => self.cmd_reload(),
            id if id.starts_with("datapack.") => self.cmd_datapack(id, ctx),
            id if id.starts_with("bossbar.") => self.cmd_bossbar(id, ctx),
            id if id.starts_with("data.") => self.cmd_data(id, ctx),
            id if id.starts_with("tag.") => self.cmd_tag(id, ctx),
            id if id.starts_with("scoreboard.")
                || id.starts_with("team.")
                || id.starts_with("trigger") =>
            {
                self.cmd_scoreboard(id, ctx)
            }
            "locate.structure" | "locate.biome" => self.cmd_locate(id, ctx),
            "locate.poi" => self.cmd_locate_poi(ctx),
            "warden_spawn_tracker.clear" | "warden_spawn_tracker.set" => {
                let level = Self::int(ctx, "warning_level");
                self.cmd_warden_spawn_tracker(level);
                Ok(1)
            }
            "say" | "me" | "msg" => self.cmd_say(id, ctx),
            "list" => self.cmd_list(ctx),
            _ => Err(err("command.failed", &[])),
        }
    }
    fn cmd_help(&mut self, ctx: &Context) -> R<i32> {
        let d = vanilla::game_dispatcher_with_features(&self.level.enabled_features);
        let level = self.ui.registries.permission;
        match ctx.get("command") {
            None => {
                let usage = d.smart_usage(0, level);
                let n = usage.len() as i32;
                for (_, line) in usage {
                    self.feedback_unbroadcast(Text::literal(format!("/{line}")));
                }
                Ok(n)
            }
            Some(Arg::Str(command)) => {
                let parse = d.parse(command, &self.ui.registries);
                if parse.context.nodes.is_empty() {
                    return Err(err("commands.help.failed", &[]));
                }
                let last = parse.context.nodes.last().map(|n| n.0).unwrap_or(0);
                let usage = d.smart_usage(last, level);
                let prefix = &command[..parse.context.range.1.min(command.len())];
                for (_, line) in &usage {
                    self.feedback_unbroadcast(Text::literal(format!("/{prefix} {line}")));
                }
                if usage.is_empty() {
                    self.feedback_unbroadcast(Text::literal(format!("/{prefix}")));
                }
                Ok(usage.len().max(1) as i32)
            }
            _ => Ok(0),
        }
    }
    pub(super) fn set_game_mode(&mut self, mode: GameMode) -> bool {
        if self.mode == mode {
            return false;
        }
        // A server player's change is heard by the mods first, who may keep it.
        if let Some(uuid) = self.lan_acting_uuid {
            let name = self.acting_player_name().to_owned();
            let mut cancel = false;
            self.native_player_game_mode_changed(
                uuid,
                &name,
                self.mode_id(),
                super::game_mode::mode_id_of(mode),
                &mut cancel,
            );
            if cancel {
                return false;
            }
        }
        self.set_mode(mode);
        if self.screen == Screen::Inventory && (self.creative || self.spectator()) {
            self.screen = Screen::Playing;
        }
        true
    }
    fn cmd_gamemode(&mut self, ctx: &Context) -> R<i32> {
        let Some(Arg::GameMode(mode)) = ctx.get("gamemode").cloned() else {
            return Ok(0);
        };
        let targets = if ctx.get("target").is_some() {
            self.entities(ctx, "target", true)?
        } else {
            vec![self.implied_player()]
        };
        let mut count = 0;
        for t in targets {
            let mode_name = Text::translate(&format!("gameMode.{}", mode.name()), &[]);
            // GameModeCommand.logGamemodeChange: the source's own entity.
            let own = self.source_entity() == Some(t);
            let told = !own && self.ui.rules.bool("send_command_feedback");
            let changed = self.for_player(t, |g| {
                if !g.set_game_mode(mode) {
                    return false;
                }
                if told {
                    g.system_message(Text::translate(
                        "gameMode.changed",
                        std::slice::from_ref(&mode_name),
                    ));
                }
                true
            });
            if changed != Some(true) {
                continue;
            }
            count += 1;
            if own {
                self.feedback(Text::translate(
                    "commands.gamemode.success.self",
                    &[mode_name],
                ));
            } else {
                self.feedback(Text::translate(
                    "commands.gamemode.success.other",
                    &[self.target_name(t), mode_name],
                ));
            }
        }
        Ok(count)
    }
    /// ItemStack.getDisplayName of one `item` with no components.
    pub(super) fn item_display_name(&self, item: &str) -> Text {
        self.stack_display_name(&ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        })
    }
    /// `ItemStack.getDisplayName`: the hover name (custom name in italics,
    /// a written book's title, a potion's effect name, else the item's
    /// translated name) in brackets, in the rarity colour and showing the
    /// stack on hover unless it is empty.
    pub(super) fn stack_display_name(&self, s: &ItemStack) -> Text {
        let item = &*s.item;
        let empty = item == "minecraft:air" || s.count == 0;
        let custom = s.data().and_then(|data| data.custom_name.as_ref());
        let hover = if let Some(c) = custom {
            horizon_client::component::name_text(c)
        } else if let Some(name) = s.data().and_then(|d| d.other("minecraft:item_name")) {
            // `ItemStack.getItemName`: the `item_name` component wins over the
            // item's own name (`Item.getName`), so a datapack's or a server's
            // renaming shows here as it does on the tooltip's first line.
            horizon_client::component::name_text(name)
        } else if let Some(title) = s
            .data()
            .and_then(|d| d.written.as_ref())
            .map(|w| w.title.clone())
            .filter(|t| !t.trim().is_empty())
        {
            Text::literal(title)
        } else if matches!(
            item,
            "minecraft:potion"
                | "minecraft:splash_potion"
                | "minecraft:lingering_potion"
                | "minecraft:tipped_arrow"
        ) && s.potion().is_some()
        {
            Text::translate(
                &horizon_client::brewing::potion_name_key(item, s.potion()),
                &[],
            )
        } else if item == "minecraft:shield"
            && let Some(color) = s.data().and_then(|d| d.base_color.as_deref())
        {
            // `ShieldItem.getName`: the banner's base colour names the shield.
            Text::translate(&format!("item.minecraft.shield.{color}"), &[])
        } else if let Some(head) = horizon_client::item_tooltip::player_head_name(s) {
            head.text()
        } else {
            // Item.getDescriptionId: a block item's is the block's.
            let (namespace, path) = item.split_once(':').unwrap_or(("minecraft", item));
            let key = format!("item.{namespace}.{path}");
            let key = if self.pack.names.get(&key).is_some() {
                key
            } else {
                format!("block.{namespace}.{path}")
            };
            Text::translate(&key, &[])
        };
        // Component.empty().append(getHoverName()), italic for a custom name.
        let mut name = Text::empty().append(hover);
        if custom.is_some() {
            name = name.italic();
        }
        let wrapped = Text::translate("chat.square_brackets", &[name]);
        if empty {
            return wrapped;
        }
        let mut rarity = self.item_rarity(item);
        if !s.enchantments().is_empty() {
            rarity = match rarity.as_str() {
                "rare" => "epic".into(),
                "epic" => rarity,
                _ => "rare".into(),
            };
        }
        let color = match rarity.as_str() {
            "uncommon" => text::YELLOW,
            "rare" => text::AQUA,
            "epic" => text::LIGHT_PURPLE,
            _ => text::WHITE,
        };
        // HoverEvent.ShowItem: the count only when it is not one.
        let mut shown = serde_json::json!({"action": "show_item", "id": item});
        if s.count != 1 {
            shown["count"] = s.count.into();
        }
        wrapped
            .with_color(color)
            .with_component_fields(vec![("hover_event".into(), shown)])
    }
    pub(super) fn item_rarity(&self, item: &str) -> String {
        static RARITY: std::sync::OnceLock<std::collections::HashMap<String, String>> =
            std::sync::OnceLock::new();
        let map = RARITY.get_or_init(|| {
            horizon_world::item_defaults::read(&self.options.resources)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(k, c)| c["minecraft:rarity"].as_str().map(|r| (k, r.to_owned())))
                .collect()
        });
        map.get(item).cloned().unwrap_or_else(|| "common".into())
    }
    fn cmd_give(&mut self, ctx: &Context) -> R<i32> {
        let Some(Arg::Item(ItemInput { id, components, .. })) = ctx.get("item").cloned() else {
            return Ok(0);
        };
        let (damage, extra) = match components
            .as_deref()
            .map(horizon_client::item_data::command_components)
        {
            Some(Ok(tag)) => horizon_client::item_data::decode_components(Some(&tag)),
            Some(Err(e)) => return Err(CommandError::new(Text::literal(e))),
            None => (0, None),
        };
        let count = Self::int(ctx, "count").unwrap_or(1);
        let targets = self.entities(ctx, "targets", true)?;
        let max_stack = self.items().get(&id).map_or(64, |i| i.max_stack) as i32;
        // A native mod's item can stack far past vanilla's 99: saturate rather
        // than overflow on a size near i32::MAX.
        let limit = max_stack.saturating_mul(100);
        if count > limit {
            return Err(CommandError::new(Text::translate(
                "commands.give.failed.toomanyitems",
                &[
                    Text::literal(limit.to_string()),
                    self.item_display_name(&id),
                ],
            )));
        }
        // An air stack is empty: nothing is given (but the message still is).
        let empty = id == "minecraft:air";
        for &t in targets.iter().filter(|_| !empty) {
            // The player's UUID, which an item they could not hold is set to.
            let conn = match t {
                Target::Guest(conn) => Some(conn),
                _ => self.lan_acting,
            };
            let uuid = match conn {
                Some(conn) => self
                    .command_guest(conn)
                    .map(|g| horizon_client::lan::playerdata::uuid_ints(g.uuid)),
                None => Some(self.level.ensure_player_uuid()),
            };
            self.for_player(t, |g| {
                let mut remaining = count;
                while remaining > 0 {
                    let size = remaining.min(max_stack);
                    remaining -= size;
                    let left = g.give(ItemStack {
                        item: id.as_str().into(),
                        count: size as u32,
                        damage,
                        extra: extra.clone(),
                    });
                    // At the player who gets them.
                    let p = g
                        .target_position(Target::Player)
                        .unwrap_or(g.source_position());
                    if let Some(rest) = left {
                        // Inventory full: Player.createItemStackToDrop (thrown from
                        // the eyes), then setNoPickUpDelay and setTarget(player).
                        let eye = g.eye_position().unwrap_or([
                            p[0],
                            p[1] + g.target_eye_height(Target::Player),
                            p[2],
                        ]);
                        g.throw_from_eyes(rest, eye, false);
                        if let Some(item) = g.just_spawned_item() {
                            item.thrower = None;
                            item.pickup_delay = 0;
                            item.target = uuid;
                        }
                    } else {
                        let r = &mut g.world.random;
                        let pitch = ((r.next_float() - r.next_float()) * 0.7 + 1.0) * 2.0;
                        g.audio
                            .play("minecraft:entity.item.pickup", Some(p), 0.2, pitch);
                    }
                }
                // containerMenu.broadcastChanges: InventoryChangeTrigger
                // (and its announcement) comes before the command's message.
                if g.player.is_some() {
                    g.check_inventory_trigger();
                }
            });
        }
        // The prototype stack: one item.
        let name = self.stack_display_name(&ItemStack {
            item: id.as_str().into(),
            count: 1,
            damage,
            extra: extra.clone(),
        });
        let message = if targets.len() == 1 {
            Text::translate(
                "commands.give.success.single",
                &[
                    Text::literal(count.to_string()),
                    name,
                    self.target_name(targets[0]),
                ],
            )
        } else {
            Text::translate(
                "commands.give.success.multiple",
                &texts(&[
                    Text::literal(count.to_string()),
                    name,
                    Text::literal(targets.len().to_string()),
                ]),
            )
        };
        self.feedback(message);
        Ok(targets.len() as i32)
    }
    pub(in crate::game) fn teleport_to(
        &mut self,
        t: Target,
        pos: [f64; 3],
        rotation: Option<[f32; 2]>,
        turn_riders: bool,
    ) {
        // An entity in another level is teleported in the level that holds it.
        let _ = self.in_target_level(t, |g| g.teleport_to_in_level(t, pos, rotation, turn_riders));
    }
    pub(super) fn teleport_to_in_level(
        &mut self,
        t: Target,
        pos: [f64; 3],
        rotation: Option<[f32; 2]>,
        turn_riders: bool,
    ) {
        use super::entity::Entity;
        let from = self.target_position(t);
        // Entity.calculatePassengerTransition compares a rider's angle with the
        // vehicle's own, so the vehicle's angles are read before the teleport,
        // beside its position.
        let from_rotation = self.target_rotation(t);
        let v = Vec3::new(pos[0], pos[1], pos[2]);
        // The riders turn from the unclamped angles (calculatePassengerTransition
        // adds their offset to the transition's, each rider's own setXRot clamps).
        let requested = rotation;
        // PositionMoveRotation.calculateAbsolute clamps pitch before setXRot.
        let rotation = rotation.map(|[yaw, pitch]| [yaw, pitch.clamp(-90.0, 90.0)]);
        match t {
            Target::Guest(_) => {
                self.for_player(t, |g| {
                    g.teleport_to(Target::Player, pos, rotation, turn_riders)
                });
            }
            Target::Player => {
                // ServerPlayer.teleport: removeVehicle before placing the player.
                self.stop_riding();
                // A server's player teleported within their level is placed
                // there at once: the arrival they were waiting for elsewhere
                // in it (whose chunks lose their tickets) is abandoned.
                if let Some(conn) = self.lan_acting {
                    horizon_client::lan::hub::teleport_cause(Some("command"));
                    self.server_trips.retain(|trip| !trip.is(conn));
                }
                let from = self
                    .player
                    .as_ref()
                    .map(|p| [p.position.x, p.position.y, p.position.z]);
                if let Some(p) = self.player.as_mut() {
                    p.position = v;
                    p.velocity = Vec3::new(0.0, 0.0, 0.0);
                    p.fall_distance = 0.0;
                    p.ground.on_ground = false;
                }
                self.previous_position = v;
                if let Some([yaw, pitch]) = rotation {
                    if yaw.is_finite() {
                        self.yaw = yaw;
                    }
                    if !pitch.is_nan() {
                        self.pitch = entity_pitch(pitch);
                    }
                }
                // A server player's teleport is told to the mods once their
                // move is in the hub (`leave_guest`).
                if let (Some(conn), Some(uuid), Some(name), Some(from)) = (
                    self.lan_acting,
                    self.lan_acting_uuid,
                    self.lan_acting_name.clone(),
                    from,
                ) {
                    self.teleport_notes
                        .push(super::server_travel::TeleportNote {
                            conn,
                            uuid,
                            name,
                            from,
                            to: pos,
                            dimension: self.dim.id().to_owned(),
                        });
                }
            }
            Target::Mob(id) => {
                // Entity.teleport: stopRiding() before the entity is placed,
                // which a command's transition always does (a command's
                // transition is never asPassenger). A rider moved without this
                // is seated again by the next sync, so nothing moves.
                self.dismount_mob(id);
                if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                    m.position = v;
                    m.previous = v;
                    // Entity.teleportSetPosition: setYRot, setYHeadRot, setXRot
                    // (clamped to -90..90), then setOldRot.
                    if let Some([yaw, pitch]) = rotation {
                        if yaw.is_finite() {
                            m.yaw = yaw;
                            m.body_yaw = yaw;
                            m.head_yaw = yaw;
                        }
                        if !pitch.is_nan() {
                            m.head_pitch = entity_pitch(pitch);
                        }
                        m.previous_yaw = m.yaw;
                        m.previous_head_pitch = m.head_pitch;
                    }
                }
            }
            Target::Item(id) => {
                if let Some(i) = self.dropped.iter_mut().find(|i| i.id == id) {
                    i.position = v;
                    i.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(i, r);
                    }
                }
            }
            Target::Orb(id) => {
                if let Some(o) = self.orbs.iter_mut().find(|o| o.id == id) {
                    o.position = v;
                    o.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(o, r);
                    }
                }
            }
            Target::Projectile(id) => {
                if let Some(a) = self.creatures.projectiles.iter_mut().find(|a| a.id == id) {
                    a.position = v;
                    a.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(a, r);
                    }
                }
            }
            Target::Stand(id) => {
                if let Some(a) = self.armor_stands.iter_mut().find(|a| a.id == id) {
                    a.position = v;
                    a.previous = v;
                    // Entity.teleport's stopRiding(), as for a mob: a stand a
                    // server seats on a vehicle (a name tag hologram) leaves it.
                    a.vehicle = None;
                    if let Some(r) = rotation {
                        Entity::set_rotation(a, r);
                    }
                }
            }
            Target::Boat(id) => {
                if let Some(b) = self.boats.iter_mut().find(|b| b.id == id) {
                    b.position = v;
                    b.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(b, r);
                    }
                }
            }
            Target::Cart(id) => {
                if let Some(c) = self.carts.iter_mut().find(|c| c.id == id) {
                    c.position = v;
                    c.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(c, r);
                    }
                }
            }
            Target::Fangs(id) => {
                if let Some(f) = self.fangs.list.iter_mut().find(|f| f.id == id) {
                    f.pos = [v.x, v.y, v.z];
                    if let Some([yaw, _]) = rotation.filter(|r| r[0].is_finite()) {
                        f.yaw = yaw;
                    }
                }
            }
            Target::Other(id) => {
                if let Some(o) = self.others.iter_mut().find(|o| o.id == id) {
                    o.position = v;
                    if let Some([yaw, pitch]) = rotation {
                        if yaw.is_finite() {
                            o.yaw = yaw;
                        }
                        if !pitch.is_nan() {
                            o.pitch = entity_pitch(pitch);
                        }
                    }
                }
            }
            Target::Crystal(id) => {
                if let Some(c) = self.crystals.iter_mut().find(|c| c.id == id) {
                    c.position = v;
                }
            }
            Target::Dragon(id) => {
                if let Some(d) = self.dragon.as_mut().filter(|d| d.id == id) {
                    d.position = v;
                    d.previous = v;
                }
            }
            Target::Tnt(id) => {
                if let Some(t) = self.tnts.iter_mut().find(|t| t.id == id) {
                    t.position = v;
                    t.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(t, r);
                    }
                }
            }
            Target::Cushion(id) => {
                // BlockAttachedEntity.setPos: the block it rests on follows the position.
                if let Some(c) = self.cushions.iter_mut().find(|c| c.id == id) {
                    c.position = v;
                    c.pos = [v.x.floor() as i32, v.y.floor() as i32, v.z.floor() as i32];
                    if let Some(r) = rotation {
                        c.yaw = r[0];
                    }
                }
            }
            Target::Falling(id) => {
                if let Some(f) = self.falling.iter_mut().find(|f| f.id == id) {
                    f.position = v;
                    f.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(f, r);
                    }
                }
            }
            Target::Rocket(id) => {
                if let Some(r) = self.rockets.iter_mut().find(|r| r.id == id) {
                    r.position = v;
                    r.previous = v;
                    if let Some(rot) = rotation {
                        Entity::set_rotation(r, rot);
                    }
                }
            }
            Target::Trident(id) => {
                if let Some(t) = self.tridents.iter_mut().find(|t| t.id == id) {
                    t.position = v;
                    t.previous = v;
                    if let Some(r) = rotation {
                        Entity::set_rotation(t, r);
                    }
                }
            }
            Target::Spawner(id) => {
                if let Some(s) = self.ominous_spawners.iter_mut().find(|s| s.id == id) {
                    s.position = [v.x, v.y, v.z];
                }
            }
            Target::Cloud(id) => {
                if let Some(c) = self.breath_clouds.iter_mut().find(|c| c.id == id) {
                    c.at = pos;
                }
            }
            // Wall-mounted entities do not move.
            Target::Frame(_) | Target::Painting(_) => {}
        }
        // Entity.teleportSameDimension: the riders go with their vehicle at
        // once, by the offset it took and the turn it was given.
        if let (Some(from), Some(to)) = (from, self.target_position(t)) {
            let placed = requested.filter(|_| turn_riders).map(|after| {
                super::vehicle_teleport::PlacedRotation {
                    before: from_rotation,
                    after,
                }
            });
            self.teleport_passengers(t, from, to, placed);
        }
        if self.options.dedicated.is_some()
            && let (Some(entity), Some(from), Some(to)) =
                (self.network_entity_id(t), from, self.target_position(t))
        {
            self.native_entity_teleport(self.dim.id(), entity as u32, from, to);
        }
    }

    /// A server's player teleported into another level than theirs
    /// (ServerPlayer.teleport): queued to move after the command. False
    /// when `t` is not a player in another level.
    fn teleport_across(
        &mut self,
        t: Target,
        level: super::Dim,
        pos: [f64; 3],
        rotation: Option<[f32; 2]>,
        turn_riders: bool,
    ) -> R<bool> {
        match self.teleport_vehicle_across(t, level, pos, rotation, turn_riders) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => {
                log::error!("entity teleport into {}: {error}", level.id());
                return Err(err("commands.generic.exception", &[]));
            }
        }
        let conn = match t {
            Target::Player => self.lan_acting,
            Target::Guest(conn) => Some(conn),
            _ => None,
        };
        let at = self.target_level(t);
        let Some(conn) = conn.filter(|_| at != level) else {
            return Ok(false);
        };
        self.level_teleports
            .push(super::server_travel::LevelTeleport {
                conn,
                dim: level,
                at: pos,
                rotation,
                vehicle: None,
                cause: Some("command"),
            });
        Ok(true)
    }
    /// `ServerPlayer.lookAt`: besides turning the player, tell their client
    /// with a `ClientboundPlayerLookAtPacket` (`from` anchor, the point, and
    /// the entity with its anchor when the target is one). Only a connected
    /// player has a client to tell. The entity form is sent whenever the
    /// target is an entity with a network id (`LookAt.LookAtEntity.perform`);
    /// a player whose id is not known here (a guest's turn, with the host
    /// out of reach) falls back to the point, which a client takes the same
    /// unless the entity moves first.
    /// `Entity.getId` as the players' clients know it: a player's is the one the
    /// hub gave it, any other entity's is its own id.
    pub(super) fn network_entity_id(&self, t: Target) -> Option<i32> {
        let host = self.lan.as_ref();
        match t {
            Target::Player => host?.hub.guest(self.lan_acting?).map(|g| g.entity_id),
            Target::Guest(conn) => host?.hub.guest(conn).map(|g| g.entity_id),
            other => non_player_entity_id(other),
        }
    }
    pub(super) fn send_player_look_at(
        &mut self,
        t: Target,
        from_eyes: bool,
        target: [f64; 3],
        entity: Option<(Target, bool)>,
    ) {
        let packet = C::PlayerLookAt(play::PlayerLookAt {
            from: i32::from(from_eyes),
            target,
            entity: entity.and_then(|(e, eyes)| {
                Some(play::LookAtEntity {
                    id: self.network_entity_id(e)?,
                    anchor: i32::from(eyes),
                })
            }),
        });
        match t {
            Target::Player if self.lan_acting.is_some() => self.guest_packets.push(packet),
            Target::Guest(conn) => {
                if let Some(host) = self.lan.as_ref() {
                    host.hub.send_to(conn, &packet);
                }
            }
            _ => {}
        }
    }
    fn cmd_teleport(&mut self, id: &str, ctx: &Context) -> R<i32> {
        let targets = if id.starts_with("teleport.targets") {
            self.entities(ctx, "targets", false)?
        } else {
            vec![
                self.source_entity()
                    .ok_or_else(|| err("permissions.requires.entity", &[]))?,
            ]
        };
        if id.ends_with("destination") {
            let dest = self.entities(ctx, "destination", false)?[0];
            let (pos, level, rotation, destination_name) = self
                .in_target_level(dest, |game| {
                    (
                        game.target_position(dest).unwrap_or(game.source_position()),
                        game.target_level(dest),
                        Some(game.target_rotation(dest).map(wrap_degrees)),
                        game.target_name(dest),
                    )
                })
                .ok_or_else(|| err("argument.entity.notfound.entity", &[]))?;
            let target_name = (targets.len() == 1).then(|| {
                self.in_target_level(targets[0], |game| game.target_name(targets[0]))
                    .unwrap_or_else(Text::empty)
            });
            for &t in &targets {
                if !self.teleport_across(t, level, pos, rotation, true)? {
                    // TeleportCommand.teleportToEntity: the destination's angles,
                    // which the riders turn to as well.
                    self.teleport_to(t, pos, rotation, true);
                }
            }
            let message = if targets.len() == 1 {
                Text::translate(
                    "commands.teleport.success.entity.single",
                    &[target_name.unwrap(), destination_name],
                )
            } else {
                Text::translate(
                    "commands.teleport.success.entity.multiple",
                    &[Text::literal(targets.len().to_string()), destination_name],
                )
            };
            self.feedback(message);
            return Ok(targets.len() as i32);
        }
        let pos = self
            .position_arg(ctx, "location")
            .unwrap_or(self.source_position());
        if !(-30_000_000.0..30_000_000.0).contains(&pos[0])
            || !(-30_000_000.0..30_000_000.0).contains(&pos[2])
            || !(-20_000_000.0..20_000_000.0).contains(&pos[1])
        {
            return Err(err("commands.teleport.invalidPosition", &[]));
        }
        let rotation = match ctx.get("rotation") {
            Some(Arg::Coords(c @ Coordinates::World(axes))) => Some((
                c.rotation(self.source_rotation()),
                [axes[0].relative, axes[1].relative],
            )),
            _ => None,
        };
        // facing: look from the new position at a location or entity.
        let facing = self.position_arg(ctx, "facingLocation").or_else(|| {
            let Some(Arg::Entity(_)) = ctx.get("facingEntity") else {
                return None;
            };
            None
        });
        let mut facing_entity = None;
        let facing = match (facing, ctx.get("facingEntity")) {
            (Some(f), _) => Some(f),
            (None, Some(_)) => {
                let e = self.entities(ctx, "facingEntity", false)?[0];
                // EntityAnchorArgument.Anchor.apply: the feet, or the eyes.
                let eyes = matches!(ctx.get("facingAnchor"), Some(Arg::Anchor(true)));
                facing_entity = Some((e, eyes));
                let eye_height = if eyes { self.target_eye_height(e) } else { 0.0 };
                self.target_position(e)
                    .map(|p| [p[0], p[1] + eye_height, p[2]])
            }
            _ => None,
        };
        // Entity.lookAt: from the source's anchor (the feet, unless `anchored
        // eyes`) on the teleported entity, so the eyes depend on the target.
        let anchored_eyes = self.command_source.is_some_and(|s| s.eyes);
        let look_at = |this: &Self, t: Target| {
            let f = facing?;
            let eye = if anchored_eyes {
                pos[1] + this.target_eye_height(t)
            } else {
                pos[1]
            };
            Some(world_cmds::look_at([pos[0], eye, pos[2]], f))
        };
        // A server's player in another level than the source's (`execute
        // in`, the console's Overworld) moves there (ServerPlayer.teleport
        // into another level), after the command.
        let source_level = match self.command_source.and_then(|s| s.level) {
            Some(level) => level,
            None if self.source_entity().is_none() => super::Dim::Overworld,
            None => self.dim,
        };
        let target_name = (targets.len() == 1).then(|| {
            self.in_target_level(targets[0], |game| game.target_name(targets[0]))
                .unwrap_or_else(Text::empty)
        });
        for &t in &targets {
            // `facing` teleports with the rotation unchanged (Relative.ROTATION,
            // no turn for the riders), then LookAt.perform turns the target alone.
            let looked = look_at(self, t);
            let turn_riders = looked.is_none();
            let rotation = looked.or_else(|| {
                let (angles, relative) = rotation?;
                let current = self
                    .in_target_level(t, |game| game.target_rotation(t))
                    .unwrap_or([0.0, 0.0]);
                // performTeleport wraps relative deltas before calculateAbsolute
                // adds them back; wrapping the resolved pitch changes its clamp.
                Some(std::array::from_fn(|axis| {
                    if relative[axis] {
                        current[axis] + wrap_degrees(angles[axis] - current[axis])
                    } else {
                        wrap_degrees(angles[axis])
                    }
                }))
            });
            if self.teleport_across(t, source_level, pos, rotation, turn_riders)? {
                continue;
            }
            self.teleport_to(t, pos, rotation, turn_riders);
            // LookAt.perform after a successful teleportTo.
            if let Some(f) = facing {
                self.send_player_look_at(t, anchored_eyes, f, facing_entity);
            }
        }
        let [x, y, z] = pos.map(|v| Text::literal(format_double(v)));
        let message = if targets.len() == 1 {
            Text::translate(
                "commands.teleport.success.location.single",
                &[target_name.unwrap(), x, y, z],
            )
        } else {
            Text::translate(
                "commands.teleport.success.location.multiple",
                &[Text::literal(targets.len().to_string()), x, y, z],
            )
        };
        self.feedback(message);
        Ok(targets.len() as i32)
    }
    /// `ServerClockManager.getInstance`: commands address clock-keyed state.
    /// Keep the existing Overworld clock tied to the world's canonical day time.
    fn command_clock_time(&self, clock: &str) -> i64 {
        if clock == "minecraft:overworld" {
            self.world.day_time
        } else {
            self.ui.command_clocks.get(clock).map_or(0, |c| c.0)
        }
    }
    fn set_command_clock_time(&mut self, clock: &str, total: i64) {
        if clock == "minecraft:overworld" {
            self.world.day_time = total;
        } else {
            self.ui
                .command_clocks
                .entry(clock.to_owned())
                .or_default()
                .0 = total;
        }
    }
    fn command_clock_state(&mut self, clock: &str) -> &mut super::chat::WorldClock {
        if clock == "minecraft:overworld" {
            &mut self.ui.clock
        } else {
            &mut self
                .ui
                .command_clocks
                .entry(clock.to_owned())
                .or_default()
                .1
        }
    }
    /// `TimeCommand.getDefaultClock`: explicit registered clocks and dimension defaults are
    /// separate; built-in timelines and markers belong only to Overworld.
    fn cmd_time(&mut self, id: &str, ctx: &Context) -> R<i32> {
        if id == "time.query.gametime" {
            let t = self.world.game_time;
            self.feedback_unbroadcast(Text::tr("commands.time.query.gametime", &[&t]));
            return Ok((t % i64::from(i32::MAX)) as i32);
        }
        // `time of <clock>` names the clock; otherwise the dimension's default clock.
        let clock = match ctx.get("clock") {
            Some(Arg::Resource(c)) => c.clone(),
            _ => match self.dimension_type().default_clock.clone() {
                Some(clock) => clock,
                None => return Err(err("commands.time.no_default_clock", &[&self.dim.id()])),
            },
        };
        let total = self.command_clock_time(&clock);
        match id {
            "time.set" => {
                let t = i64::from(Self::int(ctx, "time").unwrap_or(0));
                if total == t {
                    return Err(err("commands.time.set.already_at_time", &[&clock, &t]));
                }
                let mut cancel = false;
                self.native_time_skip(self.dim.id(), total, t, "command", &mut cancel);
                if !cancel {
                    self.set_command_clock_time(&clock, t);
                    self.command_clock_state(&clock).partial = 0.0;
                }
                self.feedback(Text::tr("commands.time.set.absolute", &[&clock, &t]));
                Ok(t as i32)
            }
            "time.set.marker" => {
                let Some(Arg::Resource(marker)) = ctx.get("timemarker").cloned() else {
                    return Ok(0);
                };
                let Some(m) = (clock == "minecraft:overworld")
                    .then(|| self.ui.markers.get(&marker).copied())
                    .flatten()
                else {
                    return Err(err(
                        "commands.time.no_time_marker_found",
                        &[&marker, &clock],
                    ));
                };
                if m.occurs_at(total) {
                    return Err(err(
                        "commands.time.set.already_at_time_marker",
                        &[&clock, &marker],
                    ));
                }
                let target = m.resolve(total);
                let mut cancel = false;
                self.native_time_skip(self.dim.id(), total, target, "command", &mut cancel);
                if !cancel {
                    self.set_command_clock_time(&clock, target);
                    self.command_clock_state(&clock).partial = 0.0;
                }
                self.feedback(Text::tr(
                    "commands.time.set.time_marker",
                    &[&clock, &marker],
                ));
                Ok((self.command_clock_time(&clock) % i64::from(i32::MAX)) as i32)
            }
            "time.add" => {
                let t = i64::from(Self::int(ctx, "time").unwrap_or(0));
                let target = (total + t).max(0);
                let mut cancel = false;
                self.native_time_skip(self.dim.id(), total, target, "command", &mut cancel);
                if !cancel {
                    self.set_command_clock_time(&clock, target);
                }
                let now = self.command_clock_time(&clock);
                self.feedback(Text::tr("commands.time.set.absolute", &[&clock, &now]));
                Ok((now % i64::from(i32::MAX)) as i32)
            }
            "time.pause" | "time.resume" => {
                let pause = id == "time.pause";
                if self.command_clock_state(&clock).paused == pause {
                    return Err(err(
                        if pause {
                            "commands.time.pause.already_paused"
                        } else {
                            "commands.time.pause.already_running"
                        },
                        &[&clock],
                    ));
                }
                self.command_clock_state(&clock).paused = pause;
                self.feedback(Text::tr(
                    if pause {
                        "commands.time.pause"
                    } else {
                        "commands.time.resume"
                    },
                    &[&clock],
                ));
                Ok(1)
            }
            "time.rate" => {
                let rate = match ctx.get("rate") {
                    Some(Arg::Float(r)) => *r,
                    _ => 1.0,
                };
                if self.command_clock_state(&clock).rate == rate {
                    return Err(err(
                        "commands.time.rate.already_same",
                        &[&clock, &java_float(f64::from(rate))],
                    ));
                }
                self.command_clock_state(&clock).rate = rate;
                self.feedback(Text::tr(
                    "commands.time.rate",
                    &[&clock, &java_float(f64::from(rate))],
                ));
                Ok(1)
            }
            "time.query.time" => {
                self.feedback_unbroadcast(Text::tr(
                    "commands.time.query.absolute",
                    &[&clock, &total],
                ));
                Ok((total % i64::from(i32::MAX)) as i32)
            }
            "time.query.timeline" | "time.query.repetition" => {
                let Some(Arg::Resource(timeline)) = ctx.get("timeline").cloned() else {
                    return Ok(0);
                };
                let Some(period) = (clock == "minecraft:overworld")
                    .then(|| self.ui.timelines.get(&timeline).copied())
                    .flatten()
                else {
                    return Err(err(
                        "commands.time.wrong_timeline_for_clock",
                        &[&timeline, &clock],
                    ));
                };
                if id == "time.query.timeline" {
                    let ticks = period.map_or(total, |p| total.rem_euclid(p));
                    self.feedback_unbroadcast(Text::tr(
                        "commands.time.query.timeline",
                        &[&timeline, &ticks],
                    ));
                    Ok(ticks as i32)
                } else {
                    let reps = period.map_or(0, |p| total / p);
                    self.feedback_unbroadcast(Text::tr(
                        "commands.time.query.timeline.repetitions",
                        &[&timeline, &reps],
                    ));
                    Ok(reps as i32)
                }
            }
            _ => Ok(0),
        }
    }
    fn cmd_weather(&mut self, ctx: &Context) -> R<i32> {
        let duration = Self::int(ctx, "duration");
        let sample = |min: i32, max: i32, r: &mut horizon_core::random::LegacyRandom| {
            min + r.next_int_bound(max - min + 1).unwrap_or(0)
        };
        let (key, clear, time, raining, thundering) = if ctx.has("clear") {
            (
                "commands.weather.set.clear",
                duration.unwrap_or_else(|| sample(12_000, 180_000, &mut self.world.random)),
                0,
                false,
                false,
            )
        } else if ctx.has("rain") {
            (
                "commands.weather.set.rain",
                0,
                duration.unwrap_or_else(|| sample(12_000, 24_000, &mut self.world.random)),
                true,
                false,
            )
        } else {
            (
                "commands.weather.set.thunder",
                0,
                duration.unwrap_or_else(|| sample(3_600, 15_600, &mut self.world.random)),
                true,
                true,
            )
        };
        let previous = (
            self.world.weather.clear_time,
            self.world.weather.rain_time,
            self.world.weather.thunder_time,
            self.world.weather.raining,
            self.world.weather.thundering,
        );
        let mut cancel = false;
        self.native_weather_change(self.dim.id(), raining, thundering, &mut cancel);
        let old_thundering = self.world.weather.thundering;
        let old_thunder_time = self.world.weather.thunder_time;
        let mut cancel_thunder = false;
        self.native_thunder_change(self.dim.id(), thundering, &mut cancel_thunder);
        if cancel {
            let w = &mut self.world.weather;
            (
                w.clear_time,
                w.rain_time,
                w.thunder_time,
                w.raining,
                w.thundering,
            ) = previous;
        } else {
            // ServerLevel.setWeatherParameters.
            let w = &mut self.world.weather;
            w.clear_time = clear;
            w.rain_time = time;
            w.thunder_time = time;
            w.raining = raining;
            w.thundering = thundering;
            if cancel_thunder {
                w.thunder_time = old_thunder_time;
                w.thundering = old_thundering;
            }
        }
        self.feedback(Text::translate(key, &[]));
        Ok(duration.unwrap_or(-1))
    }
    pub(super) fn kill_target(&mut self, t: Target) {
        // An entity in another level is killed in the level that holds it.
        let _ = self.in_target_level(t, |g| g.kill_target_in_level(t));
    }
    fn kill_target_in_level(&mut self, t: Target) {
        // Entity.kill (BlockAttachedEntity.kill; ArmorStand's generic_kill hit,
        // which bypasses its invulnerability; a vehicle is no LivingEntity):
        // removed, then ENTITY_DIE by the entity itself. Mobs post theirs as
        // they die, the crystal and the dragon on their own paths, and an item
        // entity can dampen it (ItemEntity.dampensVibrations), so it is not here.
        let died = matches!(
            t,
            Target::Stand(_)
                | Target::Frame(_)
                | Target::Painting(_)
                | Target::Boat(_)
                | Target::Cart(_)
                | Target::Orb(_)
                | Target::Projectile(_)
                | Target::Fangs(_)
                | Target::Tnt(_)
                | Target::Falling(_)
                | Target::Rocket(_)
                | Target::Trident(_)
                | Target::Spawner(_)
                | Target::Cloud(_)
        )
        .then(|| self.target_position(t))
        .flatten();
        match t {
            Target::Guest(_) => {
                self.for_player(t, |g| g.kill_target(Target::Player));
            }
            Target::Player => {
                if let Some(p) = self.player.as_mut() {
                    let _ = p.set_health(0.0, false);
                }
                self.ui.death_cause = Some("death.attack.genericKill".into());
            }
            Target::Mob(id) => {
                if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                    // LivingEntity.kill: a generic_kill hit of Float.MAX_VALUE,
                    // which its death message tells.
                    if m.alive() {
                        m.record_damage(
                            horizon_client::mobs::Damage::of(
                                horizon_client::mobs::DamageKind::GenericKill,
                            ),
                            f32::MAX,
                        );
                    }
                    m.health = 0.0;
                    if m.death_time == 0 {
                        m.death_time = 1;
                    }
                }
            }
            Target::Item(id) => {
                if let Some(i) = self.dropped.iter().position(|i| i.id == id) {
                    self.remove_dropped(i);
                }
            }
            Target::Orb(id) => {
                if let Some(o) = self.orbs.iter_mut().find(|o| o.id == id) {
                    o.age = u32::MAX / 2;
                }
            }
            Target::Projectile(id) => {
                self.creatures.projectiles.retain(|p| p.id != id);
            }
            Target::Stand(id) => self.armor_stands.retain(|a| a.id != id),
            Target::Frame(id) => {
                // ItemFrame.onKilled.
                if let Some(frame) = self.item_frames.iter().find(|f| f.id == id).cloned() {
                    self.remove_framed_map(&frame);
                }
                self.item_frames.retain(|f| f.id != id);
            }
            Target::Painting(id) => self.paintings.retain(|p| p.id != id),
            Target::Boat(id) => {
                // AbstractBoat.remove: a killed boat drops its lead.
                if let Some(i) = self.boats.iter().position(|b| b.id == id) {
                    self.release_vehicle_container(id);
                    let b = self.boats.remove(i);
                    self.spill_killed_vehicle_contents(b.position, &b.items);
                    self.boat_removed(&b);
                }
            }
            Target::Cart(id) => {
                self.release_vehicle_container(id);
                if let Some(i) = self.carts.iter().position(|c| c.id == id) {
                    let c = self.carts.remove(i);
                    self.spill_killed_vehicle_contents(c.position, &c.items);
                }
            }
            Target::Other(id) => self.others.retain(|o| o.id != id),
            Target::Fangs(id) => self.fangs.list.retain(|f| f.id != id),
            Target::Crystal(id) => self.kill_crystal(id),
            Target::Dragon(id) => self.kill_dragon(id),
            Target::Tnt(id) => self.tnts.retain(|t| t.id != id),
            Target::Cushion(id) => self.kill_cushion(id),
            Target::Falling(id) => self.falling.retain(|f| f.id != id),
            Target::Rocket(id) => self.rockets.retain(|r| r.id != id),
            Target::Trident(id) => self.tridents.retain(|t| t.id != id),
            Target::Spawner(id) => self.ominous_spawners.retain(|s| s.id != id),
            Target::Cloud(id) => self.breath_clouds.retain(|c| c.id != id),
        }
        // The orbs marked for removal are gone (Entity.kill removes them).
        self.orbs.retain(|o| o.age < u32::MAX / 2);
        if let Some(at) = died {
            self.entity_killed_event(at, false);
        }
        self.native_leaves_flush();
    }
    /// `AbstractChestBoat.remove` / `AbstractMinecartContainer.remove` for
    /// destroying removal (`Entity.kill`): `Containers.dropContents`, separate
    /// from hurt-time hull drops and after the live menu writes back its slots.
    fn spill_killed_vehicle_contents(&mut self, pos: Vec3, slots: &[Option<ItemStack>]) {
        for stack in slots {
            let count = stack.as_ref().map_or(0, |s| s.count);
            for piece in horizon_world::level::removal::drop_item_stack_at(
                &mut self.world.random,
                pos.components(),
                count,
            ) {
                let Some(stack) = stack else { continue };
                self.spawn_item(
                    ItemStack {
                        count: piece.count,
                        ..stack.clone()
                    },
                    piece.pos,
                    piece.motion,
                    0,
                );
            }
        }
    }
    fn cmd_kill(&mut self, ctx: &Context) -> R<i32> {
        let targets = if ctx.get("targets").is_some() {
            self.entities(ctx, "targets", false)?
        } else {
            vec![self.implied_player()]
        };
        let mut names: Vec<Text> = Vec::new();
        for &t in &targets {
            // The name of an entity in another level is read there.
            let name = match self.in_target_level(t, |g| g.target_name(t)) {
                Some(name) => name,
                None => self.target_name(t),
            };
            names.push(name);
        }
        for &t in &targets {
            self.kill_target(t);
        }
        let message = if targets.len() == 1 {
            Text::translate("commands.kill.success.single", &[names[0].clone()])
        } else {
            Text::tr("commands.kill.success.multiple", &[&targets.len()])
        };
        self.feedback(message);
        Ok(targets.len() as i32)
    }
    /// `SummonCommand.createEntity`: Peaceful admission uses the entity-type
    /// policy for both forms, independently of the mob category.
    fn cmd_summon(&mut self, ctx: &Context) -> R<i32> {
        let Some(Arg::Resource(kind)) = ctx.get("entity").cloned() else {
            return Ok(0);
        };
        // ResourceArgument.getSummonableEntityType.
        if matches!(
            kind.as_str(),
            "minecraft:player" | "minecraft:fishing_bobber"
        ) {
            return Err(err("entity.not_summonable", &[&kind]));
        }
        let pos = self
            .position_arg(ctx, "pos")
            .unwrap_or(self.source_position());
        let block_y = pos[1].floor() as i32;
        if !(-30_000_000.0..30_000_000.0).contains(&pos[0])
            || !(-20_000_000..20_000_000).contains(&block_y)
            || !(-30_000_000.0..30_000_000.0).contains(&pos[2])
        {
            return Err(err("commands.summon.invalidPosition", &[]));
        }
        if self.ui.difficulty == 0 && !horizon_client::mobs::finalize::allowed_in_peaceful(&kind) {
            return Err(err("commands.summon.failed.peaceful", &[]));
        }
        let short = kind.trim_start_matches("minecraft:").to_owned();
        if let Some(Arg::Str(nbt)) = ctx.get("nbt").cloned() {
            // With an NBT argument the entity is built from it, unfinalized.
            let before = self.spawned_mark();
            let name = match self.summon_with_nbt(&kind, pos, &nbt)? {
                Some(target) => self.target_name(target),
                // Spawned as a kind no command target stands for (primed
                // TNT, falling blocks, projectiles): its type's name.
                None if self.spawned_mark() != before => {
                    Text::translate(&entity_name_key(&kind), &[])
                }
                None => return Err(err("commands.summon.failed", &[])),
            };
            self.feedback(Text::translate("commands.summon.success", &[name]));
            return Ok(1);
        }
        let name;
        if self.res.mob_types().contains_key(&kind) {
            self.spawn_mob_spawned(&short, pos)
                .ok_or_else(|| err("commands.summon.failed.uuid", &[]))?;
            name = Text::translate(&entity_name_key(&kind), &[]);
        } else {
            match short.as_str() {
                "item" => {
                    // An item entity needs an item; vanilla spawns an empty one that despawns.
                    return Err(err("commands.summon.failed", &[]));
                }
                "experience_orb" => {
                    // `new ExperienceOrb(level, pos, Vec3.ZERO, 0)`, unstuck
                    // like any other orb spawn.
                    let at = self.unstuck_orb(pos);
                    self.next_entity += 1;
                    self.orbs.push(horizon_client::entities::ExperienceOrb {
                        rotation: [0.0, 0.0],
                        join: Default::default(),
                        no_gravity: false,
                        id: self.next_entity,
                        position: Vec3::new(at[0], at[1], at[2]),
                        previous: Vec3::new(at[0], at[1], at[2]),
                        velocity: Vec3::new(0.0, 0.0, 0.0),
                        value: 0,
                        age: 0,
                        on_ground: false,
                        count: 1,
                        ticks: 0,
                        health: 5,
                    });
                    // tryAddFreshEntityWithPassengers refused: ERROR_DUPLICATE_UUID.
                    if !self.native_joins_flush(false, self.next_entity) {
                        return Err(err("commands.summon.failed.uuid", &[]));
                    }
                    name = Text::translate("entity.minecraft.experience_orb", &[]);
                }
                "lightning_bolt" => {
                    let block = [
                        pos[0].floor() as i32,
                        pos[1].floor() as i32,
                        pos[2].floor() as i32,
                    ];
                    if !self.lightning_strike_allowed(self.lan_acting_uuid, block, "command") {
                        return Err(err("commands.summon.failed", &[]));
                    }
                    self.spawn_bolt(pos, false);
                    name = Text::translate("entity.minecraft.lightning_bolt", &[]);
                }
                "end_crystal" => {
                    // EndCrystal's constructor takes an id from the entity
                    // counter, and the crystal is shown with its base.
                    self.next_entity += 1;
                    let at = Vec3::new(pos[0], pos[1], pos[2]);
                    let drawn = self.world.random.next_int_bound(100_000).unwrap_or(0);
                    self.crystals.push(super::dragon::Crystal {
                        id: self.next_entity,
                        position: at,
                        show_bottom: true,
                        time: super::dragon::Crystal::start_time(at, drawn, self.clock.still()),
                        beam: None,
                        invulnerable: false,
                    });
                    // tryAddFreshEntityWithPassengers refused: ERROR_DUPLICATE_UUID.
                    if !self.native_joins_flush(false, self.next_entity) {
                        return Err(err("commands.summon.failed.uuid", &[]));
                    }
                    name = Text::translate("entity.minecraft.end_crystal", &[]);
                }
                k if super::minecarts::CartKind::from_id(&kind).is_some() => {
                    let cart_kind = super::minecarts::CartKind::from_id(&kind)
                        .unwrap_or(super::minecarts::CartKind::Rideable);
                    self.next_entity += 1;
                    let mut cart = super::minecarts::Minecart::new(
                        self.next_entity,
                        cart_kind,
                        Vec3::new(pos[0], pos[1], pos[2]),
                    );
                    // SummonCommand.createEntity loads even an empty compound, so a
                    // spawner minecart's BaseSpawner.load runs: SpawnPotentials becomes
                    // WeightedList.of(new SpawnData()).
                    if cart.spawner.is_some() {
                        use horizon_client::mobs::spawner::{Spawner, SpawnerExt};
                        let empty = horizon_world::nbt::Tag::Compound(Default::default());
                        cart.spawner = Some(Box::new(Spawner::load(&empty)));
                    }
                    let id = cart.id;
                    self.carts.push(cart);
                    if !self.native_joins_flush(false, id) {
                        return Err(err("commands.summon.failed.uuid", &[]));
                    }
                    name = Text::translate(&format!("entity.minecraft.{k}"), &[]);
                }
                // SummonCommand.createEntity: any other type from an empty compound.
                _ => {
                    let before = self.spawned_mark();
                    name = match self.summon_with_nbt(&kind, pos, "{}")? {
                        Some(target) => self.target_name(target),
                        // Spawned as a kind no command target stands for
                        // (primed TNT, falling blocks, projectiles).
                        None if self.spawned_mark() != before => {
                            Text::translate(&entity_name_key(&kind), &[])
                        }
                        None => return Err(err("commands.summon.failed", &[])),
                    };
                }
            }
        }
        self.feedback(Text::translate("commands.summon.success", &[name]));
        Ok(1)
    }
    /// What changes when an entity spawns, kinds that take their id only
    /// when first tracked (primed TNT, falling blocks, projectiles)
    /// included: a summon compares it before and after.
    fn spawned_mark(&self) -> (u32, usize) {
        (
            self.next_entity,
            self.tnts.len()
                + self.falling.len()
                + self.creatures.projectiles.len()
                + self.rockets.len()
                + self.tridents.len(),
        )
    }
    /// BlockStateArgument -> registry state.
    pub(super) fn block_state(&self, b: &BlockInput) -> R<BlockStateId> {
        let state = horizon_world::chunk::BlockState {
            name: b.id.clone(),
            properties: b.properties.iter().cloned().collect(),
        };
        self.pack
            .assets
            .registry
            .resolve(&state)
            .map_err(|_| err("argument.block.id.invalid", &[&b.id]))
    }
    /// BlockPredicateArgument test of the block at `p`: its state, then any
    /// `{...}` against the block entity's `saveWithFullMetadata`
    /// (`NbtUtils.compareNbt(…, true)`); no block entity fails.
    fn block_matches(&self, b: &BlockInput, p: [i32; 3]) -> bool {
        let state = self.world.block(p[0], p[1], p[2]);
        if !self.block_state_matches(b, state) {
            return false;
        }
        let Some(nbt) = &b.nbt else { return true };
        // (the argument's own grammar, `TagParser`, as the predicate was read)
        let Ok(pattern) = horizon_client::command::snbt::parse_fully(nbt) else {
            return false;
        };
        horizon_client::nbt_path::compare_nbt(&pattern, self.block_entity_nbt(p).as_ref())
    }
    /// The state part of `block_matches`: name or tag, then properties.
    fn block_state_matches(&self, b: &BlockInput, state: BlockStateId) -> bool {
        block_input_matches(
            self.pack.assets.registry,
            [&self.res.items().block_tags, &self.items().block_tags],
            b,
            state,
        )
    }
    /// `BlockInput.overwriteWithDefinedProperties`: restore explicit command
    /// properties after normalizing the state against neighbouring blocks.
    fn block_input_state(
        &mut self,
        pos: [i32; 3],
        input: &BlockInput,
        state: BlockStateId,
        strict: bool,
    ) -> R<BlockStateId> {
        if strict {
            return Ok(state);
        }
        let updated = self.with_level(|b, level, ticks| {
            let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
            let updated = b.update_from_neighbour_shapes(level, ticks, at, state);
            if b.is_air(updated) { state } else { updated }
        });
        let registry = self.pack.assets.registry;
        let mut descriptor = registry
            .state(updated)
            .expect("registered block state")
            .clone();
        for (key, value) in &input.properties {
            if descriptor.properties.contains_key(key) {
                descriptor.properties.insert(key.clone(), value.clone());
            }
        }
        registry
            .resolve(&descriptor)
            .map_err(|_| err("argument.block.id.invalid", &[&input.id]))
    }
    /// Place a state like BlockInput.place: false when nothing changed.
    /// `BlockInput.place` for a block of a command. `command`: `/setblock` and `/fill` place with
    /// `UPDATE_CLIENTS | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS`, so `Level.setBlock` notifies no neighbour,
    /// and send that notice themselves once the shape updates are done (`update_neighbours_on_block_set`):
    /// a block those shape updates pop off is gone before its neighbours are told of it.
    pub(super) fn place_state(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        strict: bool,
        command: bool,
    ) -> R<bool> {
        let previous = self.world.block(pos[0], pos[1], pos[2]);
        if previous == state {
            return Ok(false);
        }
        self.ensure_stateful_entity_table();
        // A different block replaces the block entity (Level.setBlock).
        let (old, new) = (
            self.res.facts().get(previous).name.clone(),
            self.res.facts().get(state).name.clone(),
        );
        // The command clears the old block entity without its side effects
        // (UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS); the new block gets a fresh one.
        let key = (pos[0], pos[1], pos[2]);
        if old != new {
            self.jukebox_removed(pos);
            self.world.block_entities.remove(&key);
        }
        if strict {
            self.with_level(|b, level, ticks| {
                b.set_block(
                    level,
                    ticks,
                    horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]),
                    state,
                    horizon_world::level::flags::UPDATE_CLIENTS
                        | horizon_world::level::flags::UPDATE_SKIP_ALL_SIDEEFFECTS,
                );
            });
            self.process_level_events()
                .map_err(|e| CommandError::new(Text::literal(e.to_string())))?;
        } else if command {
            self.level_set_with_flags(
                pos,
                state,
                horizon_world::level::flags::UPDATE_CLIENTS
                    | horizon_world::level::flags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS,
            )
            .map_err(|e| CommandError::new(Text::literal(e.to_string())))?;
        } else {
            self.level_set(pos, state)
                .map_err(|e| CommandError::new(Text::literal(e.to_string())))?;
        }
        // LevelChunk.setBlockState creates it when the block, not the state, is still
        // there after onPlace: a powered hopper turns itself `enabled=false`.
        let landed = self.world.block(pos[0], pos[1], pos[2]);
        let needs_entity = !self.world.block_entities.stateful_entity(landed.0) && old != new;
        if needs_entity
            && self.res.facts().get(landed).name == new
            && let Some(entity) = self.new_block_entity_for_state(landed)
        {
            self.world.block_entities.insert(key, entity);
        }
        Ok(true)
    }
    fn cmd_setblock(&mut self, ctx: &Context) -> R<i32> {
        let pos = self.block_pos_arg(ctx, "pos")?;
        let Some(Arg::Block(b)) = ctx.get("block").cloned() else {
            return Ok(0);
        };
        let state = self.block_state(&b)?;
        // SetBlockCommand.setBlock: `ServerLevel.isDebug` fails it.
        if self.world_debug {
            return Err(err("commands.setblock.failed", &[]));
        }
        let current = self.world.block(pos[0], pos[1], pos[2]);
        if ctx.has("keep") && current != self.world.air {
            return Err(err("commands.setblock.failed", &[]));
        }
        // SetBlockCommand.setBlock skips placement when destruction already
        // leaves an air state and the requested state is also air.
        let place_needed = if ctx.has("destroy") {
            self.destroy_block_dropping(pos)?;
            let remaining = self.world.block(pos[0], pos[1], pos[2]);
            !(self.res.facts().is_air(state) && self.res.facts().is_air(remaining))
        } else {
            true
        };
        if place_needed {
            let state = self.block_input_state(pos, &b, state, ctx.has("strict"))?;
            // SetBlockCommand.setBlock reads the old state after a destroy.
            let old_state = self.world.block(pos[0], pos[1], pos[2]);
            let placed = self.place_state(pos, state, ctx.has("strict"), true)?;
            let loaded = match &b.nbt {
                Some(nbt) => self.load_block_entity_nbt(pos, nbt),
                None => false,
            };
            if !placed && !loaded {
                return Err(err("commands.setblock.failed", &[]));
            }
            if !ctx.has("strict") {
                self.update_neighbours_on_block_set(pos, old_state)?;
            }
        } else if !ctx.has("strict") {
            // ServerLevel.updateNeighboursOnBlockSet still runs when placing
            // air was unnecessary. The unchanged air has no analog output or
            // removal side effects, so only its neighbours need notification.
            let remaining = self.world.block(pos[0], pos[1], pos[2]);
            self.with_level(|b, level, ticks| {
                b.update_neighbors_at(
                    level,
                    ticks,
                    horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]),
                    b.block(remaining),
                );
            });
            self.process_level_events()
                .map_err(|e| CommandError::new(Text::literal(e.to_string())))?;
        }
        self.feedback(Text::tr(
            "commands.setblock.success",
            &[&pos[0], &pos[1], &pos[2]],
        ));
        Ok(1)
    }
    /// `ServerLevel.updateNeighboursOnBlockSet` (`Behaviours::update_neighbours_on_block_set`) for the
    /// block now at `pos`, which replaced `old`.
    fn update_neighbours_on_block_set(&mut self, pos: [i32; 3], old: BlockStateId) -> R<()> {
        self.with_level(|b, level, ticks| {
            b.update_neighbours_on_block_set(
                level,
                ticks,
                horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]),
                old,
            );
        });
        self.process_level_events()
            .map_err(|e| CommandError::new(Text::literal(e.to_string())))
    }
    /// `Level.destroyBlock(pos, true)` through the behaviour library: the
    /// destroy event, the block's drops without a tool or breaker, then its
    /// fluid's block, the block entity spilling its contents on the way.
    pub(super) fn destroy_block_dropping(&mut self, pos: [i32; 3]) -> R<()> {
        self.destroy_block_dropping_with_breaker(pos, false)
    }
    /// `Level.destroyBlock` with entity presence for the direct target's loot.
    /// The context belongs to this level call and cannot reach later destroys.
    pub(super) fn destroy_block_dropping_with_breaker(
        &mut self,
        pos: [i32; 3],
        breaker: bool,
    ) -> R<()> {
        let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| {
            level.drop_breaker_at = breaker.then_some(at);
            b.destroy_block(level, ticks, at, true)
        });
        self.process_level_events()
            .map_err(|e| CommandError::new(Text::literal(e.to_string())))
    }
    /// The block entity part of `BlockInput.place`: load the block entity
    /// from the argument's tag; true when its saved data changed.
    pub(super) fn load_block_entity_nbt(&mut self, pos: [i32; 3], nbt: &str) -> bool {
        use horizon_world::nbt::Tag;
        if self.pot_data(pos).is_some() {
            let Ok((tag, _)) = horizon_world::nbt::parse_snbt(nbt) else {
                return false;
            };
            return self.set_pot_data(pos, &tag).unwrap_or(false);
        }
        let key = (pos[0], pos[1], pos[2]);
        let Some(e) = self.world.block_entities.get(&key) else {
            return false;
        };
        let name = self.data_block_name(pos);
        let before = horizon_client::save::encode_block_entity(key, e, &name);
        let Ok((mut tag, _)) = horizon_world::nbt::parse_snbt(nbt) else {
            return false;
        };
        if let Tag::Compound(c) = &mut tag {
            for k in ["id", "x", "y", "z"] {
                if let Some(v) = before.get(k) {
                    c.insert(k.into(), v.clone());
                }
            }
        }
        let Some((_, mut new)) = horizon_client::save::decode_block_entity(&tag) else {
            return false;
        };
        let rolled = self.take_loot_table(key, &tag, &mut new);
        let after = horizon_client::save::encode_block_entity(key, &new, &name);
        // The lock is outside the model: a tag that only changes it still
        // changes the block entity.
        let lock_changed = self.world.block_lock(key) != tag.get("lock");
        // So is the custom name.
        let name_changed = self.load_block_name(key, &new, &tag);
        let cooldown_changed = if name == "minecraft:hopper" {
            let state = super::machines::HopperState::load(&tag);
            let changed = self.hoppers.get(&key).map_or(-1, |s| s.cooldown) != state.cooldown;
            self.hoppers.insert(key, state);
            changed
        } else {
            false
        };
        if after == before && !rolled && !lock_changed && !name_changed && !cooldown_changed {
            return false;
        }
        self.world.block_entities.insert(key, new);
        self.world.set_block_lock(key, &tag);
        self.update_spawner_display(key);
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        true
    }
    /// `RandomizableContainer.unpackLootTable` for a player who opens a
    /// container: a roll the container still holds fires
    /// `player_generates_container_loot` for its table, and from then on the
    /// container saves its items, not the table.
    pub(super) fn unpack_opened_loot(&mut self, key: (i32, i32, i32), name: &str) {
        let Some(rolled) = self
            .world
            .chunks
            .get(&(key.0 >> 4, key.2 >> 4))
            .and_then(|c| c.saved.as_ref())
            .and_then(|s| s.rolled_loot.iter().find(|r| r.pos == key))
            .cloned()
        else {
            return;
        };
        // The roll is still there only while the container holds exactly its items.
        let still_rolled = self
            .world
            .block_entities
            .get_mut(&key)
            .is_some_and(|entity| {
                let mut tag = horizon_client::save::encode_block_entity(key, entity, name);
                match &mut tag {
                    horizon_world::nbt::Tag::Compound(map) => rolled.restore(map),
                    _ => false,
                }
            });
        if still_rolled {
            self.trigger_loot_table(&rolled.table);
        }
        let Some(chunk) = self.world.chunks.get_mut(&(key.0 >> 4, key.2 >> 4)) else {
            return;
        };
        let chunk = std::sync::Arc::make_mut(chunk);
        let mut extras = chunk
            .saved
            .take()
            .map(std::sync::Arc::unwrap_or_clone)
            .unwrap_or_default();
        horizon_client::save::consume_container_loot(&mut extras, key);
        chunk.saved = Some(std::sync::Arc::new(extras));
        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
        // `ChestBlock.getMenuProvider` unpacks both halves of a double chest:
        // the other half's table, if it still holds one, rolls too. The own roll
        // is consumed above, so the partner's call does not come back here.
        if let Some((other, _)) = self.chest_partner(key) {
            let other_name = self
                .res
                .facts()
                .get(self.world.block(other.0, other.1, other.2))
                .name
                .to_string();
            self.unpack_opened_loot(other, &other_name);
        }
    }
    /// RandomizableContainer.tryLoadLootTable for a tag a command gives a
    /// container (`setblock`, `data`): an empty container with a `LootTable`
    /// is rolled now, as a loaded chunk's are (`save::roll_loaded_loot`), and
    /// the roll is kept with its chunk, so that the container saves, and
    /// `data get` shows, its table and seed until its items change. A tag
    /// without one drops what the container had. True when it had a table.
    pub(super) fn take_loot_table(
        &mut self,
        key: (i32, i32, i32),
        tag: &horizon_world::nbt::Tag,
        entity: &mut horizon_client::containers::BlockEntity,
    ) -> bool {
        use horizon_client::{containers::BlockEntity, save, structure_blocks};
        let table = save::text(tag.get("LootTable"))
            .and_then(|id| horizon_client_core::identifier::normalize_id(&id));
        let rolled = match (table, entity) {
            (Some(table), BlockEntity::Chest(slots)) if slots.iter().all(Option::is_none) => {
                let loot = structure_blocks::loot_data(&self.options.resources, &self.pack_files());
                let at = Vec3::new(f64::from(key.0), f64::from(key.1), f64::from(key.2));
                let biome = self.biome_name_at(at).unwrap_or_default();
                Some(structure_blocks::roll_container(
                    slots,
                    loot.as_deref(),
                    table,
                    structure_blocks::loot_seed(tag),
                    key,
                    &biome,
                ))
            }
            _ => None,
        };
        let Some(chunk) = self.world.chunks.get_mut(&(key.0 >> 4, key.2 >> 4)) else {
            return false;
        };
        let held = |c: &horizon_client::chunk::Chunk| {
            c.saved
                .as_ref()
                .is_some_and(|s| s.rolled_loot.iter().any(|r| r.pos == key))
        };
        if rolled.is_none() && !held(chunk) {
            return false;
        }
        let chunk = std::sync::Arc::make_mut(chunk);
        let mut extras = chunk
            .saved
            .take()
            .map(std::sync::Arc::unwrap_or_clone)
            .unwrap_or_default();
        save::consume_container_loot(&mut extras, key);
        let taken = rolled.is_some();
        extras.rolled_loot.extend(rolled);
        chunk.saved = Some(std::sync::Arc::new(extras));
        taken
    }
    /// The loot table and seed a container still holding its roll saves
    /// (`RolledLoot::restore`), in place of its items.
    pub(super) fn restore_loot_table(
        &self,
        key: (i32, i32, i32),
        tag: &mut horizon_world::nbt::Tag,
    ) {
        let horizon_world::nbt::Tag::Compound(map) = tag else {
            return;
        };
        let rolled = self
            .world
            .chunks
            .get(&(key.0 >> 4, key.2 >> 4))
            .and_then(|c| c.saved.as_ref())
            .and_then(|s| s.rolled_loot.iter().find(|r| r.pos == key));
        if let Some(r) = rolled {
            r.restore(map);
        }
    }
    fn cmd_fill(&mut self, ctx: &Context) -> R<i32> {
        let a = self.block_pos_arg(ctx, "from")?;
        let b = self.block_pos_arg(ctx, "to")?;
        let Some(Arg::Block(block)) = ctx.get("block").cloned() else {
            return Ok(0);
        };
        let filter = match ctx.get("filter") {
            Some(Arg::Block(f)) => Some(f.clone()),
            _ => None,
        };
        let min = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let max = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        let area = (0..3)
            .map(|i| i64::from(max[i] - min[i] + 1))
            .product::<i64>();
        let limit = self.ui.rules.int("max_block_modifications");
        if area > i64::from(limit) {
            return Err(err("commands.fill.toobig", &[&limit, &area]));
        }
        // FillCommand.fillBlocks: `ServerLevel.isDebug` fails it.
        if self.world_debug {
            return Err(err("commands.fill.failed", &[]));
        }
        let state = self.block_state(&block)?;
        let air = self.world.air;
        let (hollow, outline, destroy, strict, keep) = (
            ctx.has("hollow"),
            ctx.has("outline"),
            ctx.has("destroy"),
            ctx.has("strict"),
            ctx.has("keep"),
        );
        let normalize_note = !strict
            && self
                .res
                .behaviours()
                .data(state)
                .is(horizon_world::level::classes::Class::Note);
        let mut count = 0;
        // What was placed, and what it replaced, for the notices after the last block.
        let mut updated: Vec<([i32; 3], BlockStateId)> = Vec::new();
        // Whether the filter takes a state, worked out once per state: a fill visits up to 32768 blocks of a handful
        // of states, and matching a tag is a name lookup.
        let mut matched: std::collections::HashMap<BlockStateId, bool> =
            std::collections::HashMap::new();
        let plain = !(destroy || hollow || outline);
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    let current = self.world.block(x, y, z);
                    if keep && current != air {
                        continue;
                    }
                    // Already the block it would become: nothing to place (place_state would do nothing either).
                    // `BlockInput.place` can change a note's instrument even
                    // when the raw input equals the current state.
                    if plain && current == state && !normalize_note {
                        continue;
                    }
                    if let Some(f) = &filter {
                        // (a filter with NBT reads each block's own block entity)
                        let ok = if f.nbt.is_some() {
                            self.block_matches(f, [x, y, z])
                        } else {
                            match matched.get(&current) {
                                Some(&ok) => ok,
                                None => {
                                    let ok = self.block_state_matches(f, current);
                                    matched.insert(current, ok);
                                    ok
                                }
                            }
                        };
                        if !ok {
                            continue;
                        }
                    }
                    let edge = x == min[0]
                        || x == max[0]
                        || y == min[1]
                        || y == max[1]
                        || z == min[2]
                        || z == max[2];
                    let mut affected = false;
                    if destroy && current != air {
                        self.destroy_block_dropping([x, y, z])?;
                        affected = true;
                    }
                    let target = if (outline || hollow) && !edge {
                        if outline {
                            if affected {
                                count += 1;
                            }
                            continue;
                        }
                        air
                    } else {
                        state
                    };
                    let target = if normalize_note && target == state {
                        self.block_input_state([x, y, z], &block, target, strict)?
                    } else {
                        target
                    };
                    let previous = self.world.block(x, y, z);
                    if self.place_state([x, y, z], target, strict, true)? {
                        count += 1;
                        if !strict {
                            updated.push(([x, y, z], previous));
                        }
                    } else if affected {
                        count += 1;
                    }
                }
            }
        }
        // FillCommand.fillBlocks: every block is in before any neighbour hears of one.
        for (pos, previous) in updated {
            self.update_neighbours_on_block_set(pos, previous)?;
        }
        if count == 0 {
            return Err(err("commands.fill.failed", &[]));
        }
        self.feedback(Text::tr("commands.fill.success", &[&count]));
        Ok(count)
    }
    fn cmd_clone(&mut self, ctx: &Context) -> R<i32> {
        let a = self.block_pos_arg(ctx, "begin")?;
        let b = self.block_pos_arg(ctx, "end")?;
        let dest = self.block_pos_arg(ctx, "destination")?;
        let min = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let max = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        let dmax = [dest[0] + size[0], dest[1] + size[1], dest[2] + size[2]];
        let overlap = (0..3).all(|i| dest[i] <= max[i] && dmax[i] >= min[i]);
        let moving = ctx.has("move");
        // Mode.canOverlap: only the normal mode refuses overlapping regions.
        if !ctx.has("force") && !moving && overlap {
            return Err(err("commands.clone.overlap", &[]));
        }
        let area = (0..3).map(|i| i64::from(size[i] + 1)).product::<i64>();
        let limit = self.ui.rules.int("max_block_modifications");
        if area > i64::from(limit) {
            return Err(err("commands.clone.toobig", &[&limit, &area]));
        }
        for p in [dest, dmax] {
            if !self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4)) {
                return Err(err("argument.pos.unloaded", &[]));
            }
        }
        // CloneCommands.clone: the destination level being a Debug world fails it.
        if self.world_debug {
            return Err(err("commands.clone.failed", &[]));
        }
        let filter = match ctx.get("filter") {
            Some(Arg::Block(f)) => Some(f.clone()),
            _ => None,
        };
        let air = self.world.air;
        let masked = ctx.has("masked");
        let mut copies = Vec::new();
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    let s = self.world.block(x, y, z);
                    if masked && s == air {
                        continue;
                    }
                    if let Some(f) = &filter
                        && !self.block_matches(f, [x, y, z])
                    {
                        continue;
                    }
                    copies.push((
                        [x, y, z],
                        [
                            dest[0] + x - min[0],
                            dest[1] + y - min[1],
                            dest[2] + z - min[2],
                        ],
                        s,
                        self.world.block_entities.get(&(x, y, z)).cloned(),
                        self.world.block_names.get(&(x, y, z)).cloned(),
                    ));
                }
            }
        }
        if moving {
            for (from, ..) in &copies {
                self.place_state(*from, air, true, false)?;
            }
        }
        let strict = ctx.has("strict");
        // CloneCommands.clone sets every destination to a barrier first, so
        // each copied state but a barrier counts as placed.
        let barrier = self.pack.assets.registry.default_state("minecraft:barrier");
        let mut count = 0;
        for (_, to, s, entity, name) in copies {
            self.place_state(to, s, strict, false)?;
            if Some(s) != barrier {
                count += 1;
            }
            if let Some(e) = entity {
                let key = (to[0], to[1], to[2]);
                self.world.block_entities.insert(key, e);
                // The copied tag carries the custom name.
                match name {
                    Some(name) => self.world.block_names.insert(key, name),
                    None => self.world.block_names.remove(&key),
                };
            }
        }
        // LevelTicks.copyAreaFrom: the block ticks pending anywhere in the
        // source box go to the same offset in the destination, before the
        // command can fail on an empty count.
        {
            use horizon_core::position::BlockPos;
            self.block_ticks.blocks.copy_area(
                horizon_core::ticks::TickArea::new(
                    BlockPos::new(min[0], min[1], min[2]),
                    BlockPos::new(max[0], max[1], max[2]),
                ),
                BlockPos::new(dest[0] - min[0], dest[1] - min[1], dest[2] - min[2]),
            );
        }
        if count == 0 {
            return Err(err("commands.clone.failed", &[]));
        }
        self.feedback(Text::tr("commands.clone.success", &[&count]));
        Ok(count)
    }
    /// Player.giveExperiencePoints including negative amounts.
    /// The caller reports the level change to the mods.
    fn give_points(&mut self, amount: i32) {
        let e = &mut self.experience;
        let mut sound_level = None;
        e.progress += amount as f32 / horizon_client::entities::Experience::needed(e.level) as f32;
        e.total = e.total.saturating_add(amount).clamp(0, i32::MAX);
        while e.progress < 0.0 {
            let f = e.progress * horizon_client::entities::Experience::needed(e.level) as f32;
            if e.level > 0 {
                e.level -= 1;
                e.progress = 1.0 + f / horizon_client::entities::Experience::needed(e.level) as f32;
            } else {
                e.level = 0;
                e.progress = 0.0;
            }
        }
        while e.progress >= 1.0 {
            e.progress =
                (e.progress - 1.0) * horizon_client::entities::Experience::needed(e.level) as f32;
            e.level += 1;
            if e.level % 5 == 0 {
                sound_level.get_or_insert(e.level);
            }
            e.progress /= horizon_client::entities::Experience::needed(e.level) as f32;
        }
        if let Some(level) = sound_level {
            self.play_level_up(1, level);
        }
    }
    /// AdvancementCommands.perform / performCriterion.
    fn cmd_advancement(&mut self, ctx: &Context) -> R<i32> {
        let targets = self.entities(ctx, "targets", true)?;
        self.load_advancements();
        let tree = self.adv.tree.clone();
        let grant = ctx.has("grant");
        let key = if grant { "grant" } else { "revoke" };
        let index = match ctx.get("advancement") {
            Some(Arg::Resource(id)) => Some(
                tree.find_id(id)
                    .ok_or_else(|| err("advancement.advancementNotFound", &[id]))?,
            ),
            _ => None,
        };
        if let (Some(i), Some(Arg::Str(criterion))) = (index, ctx.get("criterion")) {
            let a = &tree.list[i];
            let name = super::advancements::adv_name(a);
            if !a.criteria.iter().any(|c| &c.name == criterion) {
                return Err(CommandError::new(Text::translate(
                    "commands.advancement.criterionNotFound",
                    &[name, Text::literal(criterion.clone())],
                )));
            }
            // AdvancementCommands.performCriterion: each selected player's own turn.
            let results = self.advancement_turns(&targets, |g| {
                i32::from(if grant {
                    g.grant_advancement(i, Some(criterion))
                } else {
                    g.revoke_advancement(i, Some(criterion))
                })
            });
            let (who, many) = self.advancement_subject(&results);
            let to = if many { "many" } else { "one" };
            let args = [Text::literal(criterion.clone()), name, who];
            if results.iter().map(|r| r.1).sum::<i32>() == 0 {
                return Err(CommandError::new(Text::translate(
                    &format!("commands.advancement.{key}.criterion.to.{to}.failure"),
                    &args,
                )));
            }
            self.feedback(Text::translate(
                &format!("commands.advancement.{key}.criterion.to.{to}.success"),
                &args,
            ));
            return Ok(results.iter().map(|r| r.1).sum());
        }
        // getAdvancements: parents first (nearest up), the target, then
        // the subtree depth first.
        let list: Vec<usize> = match index {
            None => (0..tree.list.len()).collect(),
            Some(i) => {
                let mut out = Vec::new();
                if ctx.has("until") || ctx.has("through") {
                    let mut p = tree.list[i].parent.as_ref();
                    while let Some(pi) = p.and_then(|id| tree.find_id(id)) {
                        out.push(pi);
                        p = tree.list[pi].parent.as_ref();
                    }
                }
                out.push(i);
                if ctx.has("from") || ctx.has("through") {
                    let kids = tree.children();
                    fn add(kids: &[Vec<usize>], i: usize, out: &mut Vec<usize>) {
                        for &c in &kids[i] {
                            out.push(c);
                            add(kids, c, out);
                        }
                    }
                    add(&kids, i, &mut out);
                }
                out
            }
        };
        let quiet = index.is_none();
        // AdvancementCommands.perform: Action.perform for each selected player.
        let results = self.advancement_turns(&targets, |g| {
            let was_quiet = std::mem::replace(&mut g.adv.quiet, quiet);
            let mut count = 0;
            for &i in &list {
                let changed = if grant {
                    g.grant_advancement(i, None)
                } else {
                    g.revoke_advancement(i, None)
                };
                count += i32::from(changed);
            }
            g.adv.quiet = was_quiet;
            count
        });
        let total: i32 = results.iter().map(|r| r.1).sum();
        let (who, many) = self.advancement_subject(&results);
        let to = if many { "many" } else { "one" };
        let (what, first) = if list.len() == 1 {
            ("one", super::advancements::adv_name(&tree.list[list[0]]))
        } else {
            ("many", Text::literal(list.len().to_string()))
        };
        let args = [first, who];
        if total == 0 {
            return Err(CommandError::new(Text::translate(
                &format!("commands.advancement.{key}.{what}.to.{to}.failure"),
                &args,
            )));
        }
        self.feedback(Text::translate(
            &format!("commands.advancement.{key}.{what}.to.{to}.success"),
            &args,
        ));
        Ok(total)
    }
    /// Run `f` as each selected player's own turn and collect what it changed;
    /// a target without a turn counts as unchanged, like in `cmd_clear`.
    fn advancement_turns(
        &mut self,
        targets: &[Target],
        f: impl Fn(&mut Self) -> i32,
    ) -> Vec<(Target, i32)> {
        targets
            .iter()
            .map(|&t| (t, self.for_player(t, &f).unwrap_or(0)))
            .collect()
    }
    /// The player argument of the advancement feedback: with the tracker's
    /// NON_ZERO elements on success and every element on failure, the one
    /// player's name, else the count (CommandResponseTracker.dispatch).
    fn advancement_subject(&self, results: &[(Target, i32)]) -> (Text, bool) {
        let changed: Vec<Target> = results.iter().filter(|r| r.1 != 0).map(|r| r.0).collect();
        let picked: Vec<Target> = if changed.is_empty() {
            results.iter().map(|r| r.0).collect()
        } else {
            changed
        };
        match picked.as_slice() {
            [t] => (self.target_name(*t), false),
            _ => (Text::literal(picked.len().to_string()), true),
        }
    }
    fn cmd_experience(&mut self, ctx: &Context) -> R<i32> {
        let levels = ctx.has("levels");
        let unit = if levels { "levels" } else { "points" };
        if ctx.has("query") {
            let target = self.entities(ctx, "target", true)?[0];
            // ExperienceCommand.queryExperience: the target's own level or points.
            let value = self
                .for_player(target, |g| {
                    let e = &g.experience;
                    if levels {
                        e.level
                    } else {
                        (e.progress * horizon_client::entities::Experience::needed(e.level) as f32)
                            as i32
                    }
                })
                .unwrap_or(0);
            self.feedback_unbroadcast(Text::translate(
                &format!("commands.experience.query.{unit}"),
                &[self.target_name(target), Text::literal(value.to_string())],
            ));
            return Ok(value);
        }
        let targets = self.entities(ctx, "target", true)?;
        let amount = Self::int(ctx, "amount").unwrap_or(0);
        let add = ctx.has("add");
        let op = if add { "add" } else { "set" };
        // Every target changes (a set of points can be refused for some).
        let changed: Vec<Target> = targets
            .iter()
            .copied()
            .filter(|&t| {
                self.for_player(t, |g| g.change_experience(add, levels, amount)) == Some(true)
            })
            .collect();
        if changed.is_empty() {
            return Err(err("commands.experience.set.points.invalid", &[]));
        }
        let (key, who) = match changed[..] {
            [only] => ("single", self.target_name(only)),
            _ => ("multiple", Text::literal(changed.len().to_string())),
        };
        self.feedback(Text::translate(
            &format!("commands.experience.{op}.{unit}.success.{key}"),
            &[Text::literal(amount.to_string()), who],
        ));
        Ok(changed.len() as i32)
    }
    /// ExperienceCommand's add and set on the player in place: false when
    /// the points to set do not fit the level.
    fn change_experience(&mut self, add: bool, levels: bool, amount: i32) -> bool {
        let old_level = self.experience.level;
        if add {
            if levels {
                // Player.giveExperienceLevels handles the strict negative
                // boundary and emits the level-change hook itself.
                self.give_levels(amount);
                return true;
            } else if let Some(amount) = self.xp_change(amount) {
                self.give_points(amount);
            }
        } else if levels {
            self.experience.level = amount;
        } else {
            let needed = horizon_client::entities::Experience::needed(self.experience.level);
            if amount > needed - 1 {
                return false;
            }
            self.experience.progress = amount as f32 / needed as f32;
        }
        self.native_player_level_changed(old_level);
        true
    }
    /// `GameRules.set` as the `/gamerule` command and the management protocol call it: the rule changes, the
    /// world follows it, and a dedicated server tells the event stream (`MinecraftServer.onGameRuleChanged`
    /// tells the management protocol's clients).
    pub(super) fn set_game_rule(&mut self, rule: &gamerules::Rule, value: Value) {
        self.ui.rules.set(rule.name, value);
        self.sync_rules();
        if self.options.dedicated.is_some() {
            let (value_type, json) = match value {
                Value::Bool(b) => ("boolean", serde_json::json!(b)),
                Value::Int(i) => ("integer", serde_json::json!(i)),
            };
            super::dedicated::events::record_server(
                "gamerule",
                serde_json::json!({
                    "key": format!("minecraft:{}", rule.name),
                    "valueType": value_type,
                    "value": json,
                }),
            );
        }
    }
    fn cmd_gamerule(&mut self, ctx: &Context) -> R<i32> {
        let Some(name) = ctx.literals.get(1).cloned() else {
            return Ok(0);
        };
        let Some(rule) = gamerules::rule(&name) else {
            return Ok(0);
        };
        let current = self.ui.rules.get(rule.name).unwrap_or(rule.default);
        let value = match ctx.get("value") {
            Some(Arg::Bool(b)) => Value::Bool(*b),
            Some(Arg::Int(i)) => Value::Int(*i),
            _ => {
                self.feedback_unbroadcast(Text::tr(
                    "commands.gamerule.query",
                    &[&rule.name, &current],
                ));
                return Ok(match current {
                    Value::Bool(b) => i32::from(b),
                    Value::Int(i) => i,
                });
            }
        };
        if current == value {
            return Err(err("commands.gamerule.not_set", &[&rule.name, &value]));
        }
        self.set_game_rule(rule, value);
        self.feedback(Text::tr("commands.gamerule.set", &[&rule.name, &value]));
        Ok(match value {
            Value::Bool(b) => i32::from(b),
            Value::Int(i) => i,
        })
    }
    fn cmd_difficulty(&mut self, ctx: &Context) -> R<i32> {
        let name = |d: u8| {
            Text::translate(
                &format!("options.difficulty.{}", DIFFICULTIES[d as usize]),
                &[],
            )
        };
        let Some(i) = DIFFICULTIES.iter().position(|d| ctx.has(d)) else {
            let current = self.ui.difficulty;
            self.feedback_unbroadcast(Text::translate(
                "commands.difficulty.query",
                &[name(current)],
            ));
            return Ok(i32::from(current));
        };
        let i = i as u8;
        if self.ui.difficulty == i {
            return Err(CommandError::new(Text::translate(
                "commands.difficulty.failure",
                &[name(i)],
            )));
        }
        self.set_difficulty_forced(i);
        self.feedback(Text::translate("commands.difficulty.success", &[name(i)]));
        Ok(0)
    }
    /// `MinecraftServer.setDifficulty(difficulty, true)`.
    pub(super) fn set_difficulty_forced(&mut self, i: u8) {
        let from = self.ui.difficulty;
        self.ui.difficulty = i;
        // The level data holds the world's difficulty: it survives the
        // reopen, so it follows the command.
        self.level.set_difficulty(i);
        self.sync_rules();
        // MinecraftServer.setDifficulty: every player, guests included, is
        // sent the new difficulty.
        if let Some(host) = self.lan.as_ref() {
            host.hub
                .broadcast(&C::ChangeDifficulty(play::ChangeDifficulty {
                    difficulty: i32::from(i),
                    locked: self.level.difficulty_locked,
                }));
        }
        if from != i {
            let (from, to) = difficulty_names(from, i);
            self.native_difficulty_changed(from, to);
        }
    }
    fn cmd_spawnpoint(&mut self, ctx: &Context) -> R<i32> {
        let targets = if ctx.get("targets").is_some() {
            self.entities(ctx, "targets", true)?
        } else {
            vec![Target::Player]
        };
        let pos = if ctx.get("pos").is_some() {
            self.block_pos_arg(ctx, "pos")?
        } else {
            let p = self.source_position();
            [
                p[0].floor() as i32,
                p[1].floor() as i32,
                p[2].floor() as i32,
            ]
        };
        let rot = match ctx.get("rotation") {
            Some(Arg::Coords(c)) => c.rotation(self.source_rotation()),
            _ => [0.0, 0.0],
        };
        let yaw = ((rot[0] + 180.0).rem_euclid(360.0)) - 180.0;
        let pitch = rot[1].clamp(-90.0, 90.0);
        // SetSpawnCommand: a forced point in the source's level, for each target.
        let level = self.dim;
        for &t in &targets {
            self.for_player(t, |g| {
                let mut cancel = false;
                if let (Some(player), Some(name)) = (g.lan_acting_uuid, g.lan_acting_name.clone()) {
                    g.native_player_spawn_point_set(
                        player,
                        &name,
                        level.id(),
                        pos,
                        "command",
                        &mut cancel,
                    );
                }
                if cancel {
                    return;
                }
                g.spawn = None;
                g.respawn_config = Some(super::respawn_anchor::RespawnConfig {
                    dim: level,
                    pos,
                    yaw,
                    forced: true,
                });
            });
        }
        let dim = Text::literal(self.dim.id());
        let p = pos.map(|v| Text::literal(v.to_string()));
        let mut args = vec![
            p[0].clone(),
            p[1].clone(),
            p[2].clone(),
            Text::literal(java_float(f64::from(yaw))),
            Text::literal(java_float(f64::from(pitch))),
            dim,
        ];
        let key = if targets.len() == 1 {
            args.push(self.target_name(targets[0]));
            "commands.spawnpoint.success.single"
        } else {
            args.push(Text::literal(targets.len().to_string()));
            "commands.spawnpoint.success.multiple"
        };
        self.feedback(Text::translate(key, &args));
        Ok(targets.len() as i32)
    }
    fn cmd_setworldspawn(&mut self, ctx: &Context) -> R<i32> {
        let pos = if ctx.get("pos").is_some() {
            self.block_pos_arg(ctx, "pos")?
        } else {
            let p = self.source_position();
            [
                p[0].floor() as i32,
                p[1].floor() as i32,
                p[2].floor() as i32,
            ]
        };
        let rot = match ctx.get("rotation") {
            Some(Arg::Coords(c)) => c.rotation(self.source_rotation()),
            _ => [0.0, 0.0],
        };
        // LevelData.setRespawnData: the world's spawn (level.dat `spawn`).
        let old = self.level.world_spawn.replace(pos);
        self.level.world_spawn_angle = rot;
        self.native_spawn_change(pos, rot);
        // A player whose fallback is the world spawn (no spawnpoint of its
        // own; a bed or an anchor that is gone falls back to it) respawns at
        // the new one.
        let floor = |s: Vec3| [s.x.floor() as i32, s.y.floor() as i32, s.z.floor() as i32];
        if self.spawn.map(floor) == old {
            self.spawn = Some(Vec3::new(
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]),
                f64::from(pos[2]) + 0.5,
            ));
        }
        let args = [
            Text::literal(pos[0].to_string()),
            Text::literal(pos[1].to_string()),
            Text::literal(pos[2].to_string()),
            Text::literal(java_float(f64::from(rot[0]))),
            Text::literal(java_float(f64::from(rot[1]))),
            Text::literal(self.dim.id()),
        ];
        self.feedback(Text::translate("commands.setworldspawn.success", &args));
        Ok(1)
    }
    fn item_matches(&self, predicate: &ItemInput, item: &str) -> bool {
        if predicate.any {
            return true;
        }
        if predicate.tag {
            let tag = predicate.id.trim_start_matches("minecraft:");
            return self
                .res
                .items()
                .item_tags
                .get(tag)
                .or_else(|| self.items().item_tags.get(&predicate.id))
                .is_some_and(|s| {
                    s.contains(item) || s.contains(item.trim_start_matches("minecraft:"))
                });
        }
        predicate.id == item
    }
    /// `ItemPredicateArgument`: the item test and its `[...]` component
    /// tests (presence, `=` equality, `!` negation, `|` alternatives).
    pub(super) fn stack_matches(&self, predicate: &ItemInput, stack: &ItemStack) -> bool {
        if !self.item_matches(predicate, &stack.item) {
            return false;
        }
        let Some(tests) = predicate.components.as_deref() else {
            return true;
        };
        let inner = tests
            .trim()
            .strip_prefix('[')
            .and_then(|t| t.strip_suffix(']'))
            .unwrap_or("");
        let components = match horizon_client::item_data::encode_components(stack) {
            Some(horizon_world::nbt::Tag::Compound(c)) => c,
            _ => Default::default(),
        };
        let test = |term: &str| -> bool {
            let term = term.trim();
            let (negated, term) = match term.strip_prefix('!') {
                Some(t) => (true, t.trim()),
                None => (false, term),
            };
            let split = term.find(['=', '~']).unwrap_or(term.len());
            let id = term[..split].trim();
            let id = if id.contains(':') {
                id.to_owned()
            } else {
                format!("minecraft:{id}")
            };
            let present = components.get(&id.as_str().into());
            let result = match term[split..].chars().next() {
                Some('=') => {
                    let value = horizon_world::nbt::parse_snbt(term[split + 1..].trim())
                        .ok()
                        .map(|(t, _)| t);
                    match (present, value) {
                        (Some(have), Some(want)) => {
                            horizon_client::nbt_path::compare_nbt(&want, Some(have))
                                && horizon_client::nbt_path::compare_nbt(have, Some(&want))
                        }
                        _ => false,
                    }
                }
                Some('~') => {
                    // Sub-predicates: `count` and component predicates.
                    let value = horizon_world::nbt::parse_snbt(term[split + 1..].trim())
                        .ok()
                        .map(|(t, _)| t);
                    match (id.as_str(), value) {
                        ("minecraft:count", Some(v)) => {
                            let n = i64::from(stack.count);
                            match &v {
                                horizon_world::nbt::Tag::Compound(_) => {
                                    let get = |k: &str| match v.get(k) {
                                        Some(horizon_world::nbt::Tag::Int(i)) => {
                                            Some(i64::from(*i))
                                        }
                                        _ => None,
                                    };
                                    get("min").is_none_or(|m| n >= m)
                                        && get("max").is_none_or(|m| n <= m)
                                }
                                horizon_world::nbt::Tag::Int(i) => n == i64::from(*i),
                                _ => false,
                            }
                        }
                        // `MinMaxBounds.Ints.matches`: DamagePredicate's damage-only
                        // Int bounds shape; effective DAMAGE presence is required.
                        // Other fields keep the entire existing predicate path.
                        ("minecraft:damage", Some(ref v))
                            if matches!(v, horizon_world::nbt::Tag::Compound(c) if c.len() == 1)
                                && match v.get("damage") {
                                    Some(horizon_world::nbt::Tag::Int(_)) => true,
                                    Some(horizon_world::nbt::Tag::Compound(bounds)) => {
                                        bounds.iter().all(|(key, value)| {
                                            matches!(key.to_string_lossy().as_str(), "min" | "max")
                                                && matches!(value, horizon_world::nbt::Tag::Int(_))
                                        })
                                    }
                                    _ => false,
                                } =>
                        {
                            let removed = stack
                                .data()
                                .is_some_and(|data| data.other("!minecraft:damage").is_some());
                            let has_damage = !removed
                                && (present.is_some()
                                    || self.items().get(&stack.item).is_some_and(|item| {
                                        item.components.iter().any(|c| c == "minecraft:damage")
                                    }));
                            let n = i64::from(stack.damage);
                            has_damage
                                && match v.get("damage").unwrap() {
                                    horizon_world::nbt::Tag::Int(exact) => n == i64::from(*exact),
                                    bounds => {
                                        let get = |key| match bounds.get(key) {
                                            Some(horizon_world::nbt::Tag::Int(value)) => {
                                                Some(i64::from(*value))
                                            }
                                            _ => None,
                                        };
                                        get("min").is_none_or(|min| n >= min)
                                            && get("max").is_none_or(|max| n <= max)
                                    }
                                }
                        }
                        (_, Some(v)) => present.is_some_and(|have| {
                            horizon_client::nbt_path::compare_nbt(&v, Some(have))
                        }),
                        _ => false,
                    }
                }
                _ => present.is_some(),
            };
            result != negated
        };
        split_top_level(inner, ',')
            .iter()
            .filter(|t| !t.trim().is_empty())
            .all(|term| split_top_level(term, '|').iter().any(|alt| test(alt)))
    }
    fn cmd_clear(&mut self, ctx: &Context) -> R<i32> {
        let targets = if ctx.get("targets").is_some() {
            self.entities(ctx, "targets", true)?
        } else {
            vec![Target::Player]
        };
        let predicate = match ctx.get("item") {
            Some(Arg::Item(i)) => i.clone(),
            _ => ItemInput {
                id: "*".into(),
                tag: false,
                any: true,
                components: None,
            },
        };
        let max = Self::int(ctx, "maxCount").unwrap_or(-1);
        let counting = max == 0;
        // ClearInventoryCommands.clearInventory: each player on their own.
        let mut removed = 0i32;
        let mut found = Vec::new();
        for &t in &targets {
            let n = self
                .for_player(t, |g| g.clear_matching(&predicate, max))
                .unwrap_or(0);
            removed += n;
            if n != 0 {
                found.push(t);
            }
        }
        if removed == 0 {
            return Err(if targets.len() == 1 {
                CommandError::new(Text::translate(
                    "clear.failed.single",
                    &[self.target_name(targets[0])],
                ))
            } else {
                err("clear.failed.multiple", &[&targets.len()])
            });
        }
        let kind = if counting { "test" } else { "success" };
        let (key, who) = match found[..] {
            [only] => ("single", self.target_name(only)),
            _ => ("multiple", Text::literal(found.len().to_string())),
        };
        self.feedback(Text::translate(
            &format!("commands.clear.{kind}.{key}"),
            &[Text::literal(removed.to_string()), who],
        ));
        Ok(removed)
    }
    /// Inventory.clearOrCountMatchingItems for the player in place: `max`
    /// of the matching items go (all when negative, none and only counted
    /// when 0). Returns how many matched.
    fn clear_matching(&mut self, predicate: &ItemInput, max: i32) -> i32 {
        let counting = max == 0;
        // Inventory slots 0..40: main, armor feet to head, off hand;
        // then clearOrCountMatchingItems visits the crafting grid and carried stack.
        let mut removed = 0i32;
        let mut budget = if max < 0 { i32::MAX } else { max };
        let all = |g: &Self| -> Vec<Option<ItemStack>> {
            let mut v: Vec<Option<ItemStack>> = g.inventory.clone();
            v.extend(g.armor.iter().rev().cloned());
            v.push(g.offhand.clone());
            v.extend(g.craft_grid.iter().cloned());
            v.push(g.carried.clone());
            v
        };
        let matching: Vec<bool> = all(self)
            .iter()
            .map(|s| s.as_ref().is_some_and(|s| self.stack_matches(predicate, s)))
            .collect();
        let mut slots: Vec<&mut Option<ItemStack>> = Vec::new();
        slots.extend(self.inventory.iter_mut());
        slots.extend(self.armor.iter_mut().rev());
        slots.push(&mut self.offhand);
        slots.extend(self.craft_grid.iter_mut());
        slots.push(&mut self.carried);
        for (slot, matches) in slots.into_iter().zip(matching) {
            if !matches {
                continue;
            }
            let Some(stack) = slot.as_mut() else { continue };
            let take = if counting {
                stack.count as i32
            } else {
                (stack.count as i32).min(budget)
            };
            removed += take;
            if !counting {
                budget -= take;
                stack.count -= take as u32;
                if stack.count == 0 {
                    *slot = None;
                }
                if budget == 0 {
                    break;
                }
            }
        }
        removed
    }
    /// `PoiTypes`: the point of interest type of a block state.
    fn poi_type(&self, state: horizon_world::registry::BlockStateId) -> Option<&'static str> {
        let s = self.pack.assets.registry.state(state)?;
        horizon_client::save::poi_type(s).map(|(kind, _)| kind)
    }
    /// `LocateCommand.locatePoi`: `PoiManager.findClosestWithType` within
    /// 256 blocks of the source, over the loaded chunks.
    fn cmd_locate_poi(&mut self, ctx: &Context) -> R<i32> {
        let Some(Arg::Resource(poi)) = ctx.get("poi").cloned() else {
            return Ok(0);
        };
        let wanted: Vec<String> = match poi.strip_prefix('#') {
            Some(tag) => self.tag_entries("point_of_interest_type", tag),
            None => vec![poi.clone()],
        };
        let from = self.source_position().map(|v| v.floor() as i32);
        const RANGE: i32 = 256;
        let mut cache: std::collections::HashMap<u32, Option<&'static str>> =
            std::collections::HashMap::new();
        let mut best: Option<(i64, [i32; 3], &'static str)> = None;
        let (cx0, cz0) = ((from[0] - RANGE) >> 4, (from[2] - RANGE) >> 4);
        let (cx1, cz1) = ((from[0] + RANGE) >> 4, (from[2] + RANGE) >> 4);
        for cx in cx0..=cx1 {
            for cz in cz0..=cz1 {
                let Some(chunk) = self.world.chunks.get(&(cx, cz)) else {
                    continue;
                };
                for (i, section) in chunk.sections.iter().enumerate() {
                    let mut wanted_state = |state: u32| {
                        cache
                            .entry(state)
                            .or_insert_with(|| {
                                self.poi_type(horizon_world::registry::BlockStateId(state))
                                    .filter(|t| wanted.iter().any(|w| w == t))
                            })
                            .is_some()
                    };
                    if !section.blocks.maybe_has(&mut wanted_state) {
                        continue;
                    }
                    let base_y = self.world.bottom_y() + 16 * i as i32;
                    for j in 0..4096 {
                        let state = section.get(j);
                        let Some(kind) = cache.get(&state).copied().flatten() else {
                            continue;
                        };
                        let p = [
                            cx * 16 + (j & 15) as i32,
                            base_y + (j >> 8) as i32,
                            cz * 16 + ((j >> 4) & 15) as i32,
                        ];
                        let d = [p[0] - from[0], p[1] - from[1], p[2] - from[2]]
                            .map(|d| i64::from(d) * i64::from(d))
                            .iter()
                            .sum::<i64>();
                        if d <= i64::from(RANGE * RANGE) && best.is_none_or(|(b, _, _)| d < b) {
                            best = Some((d, p, kind));
                        }
                    }
                }
            }
        }
        let Some((_, p, kind)) = best else {
            return Err(CommandError::new(Text::tr(
                "commands.locate.poi.not_found",
                &[&poi],
            )));
        };
        let link = Text::teleport_link(p[0], None, p[2]);
        let (dx, dz) = (p[0] - from[0], p[2] - from[2]);
        let distance = ((dx * dx + dz * dz) as f32).sqrt().floor() as i32;
        let shown = if poi.starts_with('#') {
            format!("{poi} ({kind})")
        } else {
            poi
        };
        self.feedback(Text::translate(
            "commands.locate.poi.success",
            &[
                Text::literal(shown),
                link,
                Text::literal(distance.to_string()),
            ],
        ));
        Ok(distance)
    }
    fn cmd_locate(&mut self, id: &str, ctx: &Context) -> R<i32> {
        let origin = self.source_position();
        if id == "locate.biome" {
            let Some(Arg::Resource(biome)) = ctx.get("biome").cloned() else {
                return Ok(0);
            };
            // LocateCommand.locateBiome: BiomeSource.findClosestBiome3d with
            // a 6400-block radius, 32-block columns and 64-block layers.
            let from = origin.map(|v| v.floor() as i32);
            let allowed = |name: &str| self.biome_matches(&biome, name);
            let search = horizon_world::worldgen::generator::BiomeSearch {
                allowed: &allowed,
                origin: from,
                radius: 6400,
                horizontal: 32,
                vertical: 64,
                min_y: self.world.bottom_y(),
                max_y: self.world.top_y() - 1,
            };
            let found = self
                .end
                .locator()
                .and_then(|t| t.find_closest_biome(&search));
            let Some((name, p)) = found else {
                return Err(CommandError::new(Text::tr(
                    "commands.locate.biome.not_found",
                    &[&biome],
                )));
            };
            let d2 = [p[0] - from[0], p[1] - from[1], p[2] - from[2]]
                .map(|d| f64::from(d) * f64::from(d))
                .iter()
                .sum::<f64>();
            let shown = if biome.starts_with('#') {
                format!("{biome} ({name})")
            } else {
                biome.clone()
            };
            let link = Text::teleport_link(p[0], Some(p[1]), p[2]);
            let distance = (d2 as f32).sqrt().floor() as i32;
            self.feedback(Text::translate(
                "commands.locate.biome.success",
                &[
                    Text::literal(shown),
                    link,
                    Text::literal(distance.to_string()),
                ],
            ));
            return Ok(distance);
        }
        let Some(Arg::Resource(s)) = ctx.get("structure").cloned() else {
            return Ok(0);
        };
        // LocateCommand.locateStructure: an id or a #tag of structures.
        let names = match s.strip_prefix('#') {
            Some(tag) => self.structure_tag(tag),
            None => vec![s.clone()],
        };
        let from = origin.map(|v| v.floor() as i32);
        let found = self.end.locator().and_then(|t| t.locate(&names, from, 100));
        let Some((name, p)) = found else {
            return Err(CommandError::new(Text::tr(
                "commands.locate.structure.not_found",
                &[&s],
            )));
        };
        let link = Text::teleport_link(p[0], None, p[2]);
        let (dx, dz) = (f64::from(p[0] - from[0]), f64::from(p[2] - from[2]));
        let distance = (dx * dx + dz * dz).sqrt().floor() as i32;
        let shown = if s.starts_with('#') {
            format!("{s} ({name})")
        } else {
            name
        };
        self.feedback(Text::translate(
            "commands.locate.structure.success",
            &[
                Text::literal(shown),
                link,
                Text::literal(distance.to_string()),
            ],
        ));
        Ok(distance)
    }
    /// The structures of a `worldgen/structure` tag, nested tags expanded.
    pub(super) fn structure_tag(&self, tag: &str) -> Vec<String> {
        self.tag_entries("worldgen/structure", tag)
    }
    /// The entries of a data pack tag of `registry`, nested tags expanded.
    pub(super) fn tag_entries(&self, registry: &str, tag: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut pending = vec![tag.to_owned()];
        let mut seen = std::collections::HashSet::new();
        while let Some(t) = pending.pop() {
            if !seen.insert(t.clone()) {
                continue;
            }
            let (ns, path) = t.split_once(':').unwrap_or(("minecraft", &t));
            let file = self
                .options
                .resources
                .join(format!("data/{ns}/tags/{registry}/{path}.json"));
            // The game's file and the world's packs' both count, entries
            // joined in resource-stack order; a pack file with `replace`
            // drops what the lower sources put in (`TagLoader.load`).
            let packed = self
                .fns
                .store
                .files
                .get(&format!("tags/{ns}:{registry}/{path}"))
                .into_iter()
                .flatten()
                .filter_map(|b| serde_json::from_slice::<serde_json::Value>(b).ok());
            let parsed = std::fs::read(file)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .into_iter()
                .chain(packed);
            let mut entries = Vec::new();
            for v in parsed {
                if v["replace"].as_bool() == Some(true) {
                    entries.clear();
                }
                entries.extend(v["values"].as_array().into_iter().flatten().cloned());
            }
            for e in &entries {
                let id = e.as_str().or_else(|| e["id"].as_str()).unwrap_or_default();
                match id.strip_prefix('#') {
                    Some(nested) => pending.push(nested.to_owned()),
                    None if !id.is_empty() => out.push(id.to_owned()),
                    None => {}
                }
            }
        }
        out
    }
    /// ListPlayersCommand: every online player in join order (a single-player
    /// owner is one of them), with the count and the max.
    fn cmd_list(&mut self, ctx: &Context) -> R<i32> {
        let players: Vec<(u128, String)> = self
            .lan
            .iter()
            .flat_map(|h| h.hub.guests.iter())
            .filter(|g| g.placed)
            .map(|g| (g.uuid, g.name.clone()))
            .collect();
        let max = self
            .lan
            .as_ref()
            .map_or(horizon_client::lan::MAX_PLAYERS, |h| {
                h.hub.lan.shared.max_players()
            });
        let uuids = ctx.has("uuids");
        let mut names = Text::empty();
        for (k, (uuid, name)) in players.iter().enumerate() {
            if k > 0 {
                // ComponentUtils.formatList: gray commas.
                names.push(Text::colored(", ", text::GRAY));
            }
            if uuids {
                names.push(Text::translate(
                    "commands.list.nameAndId",
                    &[
                        Text::literal(name.as_str()),
                        Text::literal(horizon_protocol::crypto::uuid_string(*uuid)),
                    ],
                ));
            } else {
                names.push(self.player_text(name, *uuid));
            }
        }
        let count = players.len();
        self.feedback_unbroadcast(Text::translate(
            "commands.list.players",
            &[
                Text::literal(count.to_string()),
                Text::literal(max.to_string()),
                names,
            ],
        ));
        Ok(count as i32)
    }
    /// `SayCommand.register`: resolve the source name and broadcast unsigned announcements.
    fn cmd_say(&mut self, id: &str, ctx: &Context) -> R<i32> {
        let raw = match ctx.get(if id == "me" { "action" } else { "message" }) {
            Some(Arg::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let message = self.expand_selectors(&raw);
        // CommandSourceStack.getDisplayName: the executing entity's name.
        let name = match self.command_source {
            Some(source) => match source.entity {
                Some(target) => self.target_name(target),
                None => source
                    .block
                    .and_then(|pos| match self.world.block_entities.get(&pos) {
                        Some(horizon_client::containers::BlockEntity::CommandBlock(block)) => {
                            Some(block.name())
                        }
                        _ => None,
                    })
                    .or_else(|| {
                        source.cart.and_then(|id| {
                            self.carts
                                .iter()
                                .find(|cart| cart.id == id)
                                .map(|cart| cart.command.name())
                        })
                    })
                    .unwrap_or_else(|| Text::literal("Server")),
            },
            None => self.player_display_name(),
        };
        // A player's command whose output is captured (a function, `execute …
        // run say`, the command harness) gets the announcement with that
        // output, in order; everyone else gets the broadcast.
        let captured_for = self.chat_capture.as_ref().and(self.lan_acting);
        let line = match id {
            "say" => {
                // SayCommand broadcasts independently of the command source's feedback.
                if let Some(host) = &self.lan {
                    use horizon_client::lan::data::component_nbt;
                    host.hub.broadcast_source_chat(
                        "say_command",
                        component_nbt(&name.component()),
                        component_nbt(&message.component()),
                        captured_for,
                    );
                }
                Text::translate("chat.type.announcement", &[name, message])
            }
            "me" => Text::translate("chat.type.emote", &[name, message]),
            _ => {
                let _ = self.entities(ctx, "targets", true)?;
                // Whispering to yourself: the incoming and outgoing lines.
                let out = Text::translate(
                    "commands.message.display.outgoing",
                    &[self.player_display_name(), message.clone()],
                )
                .with_color(text::GRAY)
                .italic();
                self.system_message(out);
                Text::translate(
                    "commands.message.display.incoming",
                    &[self.player_display_name(), message],
                )
                .with_color(text::GRAY)
                .italic()
            }
        };
        if id == "say" {
            if self.lan.is_some() && self.options.dedicated.is_some() {
                self.lan_log_event(horizon_client::lan::hub::HostEvent::Chat {
                    line: line.plain(),
                    secure: false,
                });
            }
            if self.lan.is_some() && self.chat_capture.is_some() {
                // The broadcast skipped the acting player: their copy goes with
                // the captured output. Without one (the console), the broadcast
                // reached everyone and the console logged it above.
                if captured_for.is_some()
                    && let Some(capture) = self.chat_capture.as_mut()
                {
                    capture.push(line);
                }
                return Ok(1);
            }
            self.player_message(line);
            return Ok(1);
        }
        if let Some(capture) = self.chat_capture.as_mut() {
            capture.push(line.clone());
        }
        let focused = self.screen == Screen::Chat;
        self.ui.chat.add(
            &self.gui.atlas,
            line,
            None,
            horizon_client::chat::ChatSource::Player,
            self.visual_ticks(),
            focused,
        );
        Ok(1)
    }
    /// MessageArgument: `@p`-style selectors in a message become names.
    pub(super) fn expand_selectors(&mut self, raw: &str) -> Text {
        let mut out = Text::empty();
        let mut rest = raw;
        while let Some(at) = rest.find('@') {
            out.push(&rest[..at]);
            let mut reader = Reader::new(&rest[at..]);
            match (horizon_client::command::ArgType::Entity {
                single: false,
                players: false,
            })
            .parse(&mut reader, &self.ui.registries)
            {
                Ok(Arg::Entity(s)) if reader.cursor > 1 => {
                    let found = self.select(&s);
                    for (i, t) in found.iter().enumerate() {
                        if i > 0 {
                            out.push(", ");
                        }
                        out.push(self.target_name(*t));
                    }
                    rest = &rest[at + reader.cursor..];
                }
                _ => {
                    out.push("@");
                    rest = &rest[at + 1..];
                }
            }
        }
        out.push(rest);
        out
    }
}

/// Split at a separator outside brackets, braces and quotes.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut escaped, mut start) = (0i32, None::<char>, false, 0);
    for (i, ch) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// BlockPredicateArgument test, against the registry and the block tags:
/// a `#tag` is looked up by its path in `tags[0]`, then by its whole id in
/// `tags[1]` (as `Game::block_matches` reads them).
pub(super) fn block_input_matches(
    registry: &horizon_world::registry::BlockRegistry,
    tags: [&std::collections::HashMap<String, std::collections::HashSet<String>>; 2],
    b: &BlockInput,
    state: BlockStateId,
) -> bool {
    let Some(s) = registry.state(state) else {
        return false;
    };
    let name_ok = if b.tag {
        let tag = b.id.trim_start_matches("minecraft:");
        tags[0]
            .get(tag)
            .or_else(|| tags[1].get(&b.id))
            .is_some_and(|set| {
                set.contains(&s.name) || set.contains(s.name.trim_start_matches("minecraft:"))
            })
    } else {
        s.name == b.id
    };
    name_ok
        && b.properties
            .iter()
            .all(|(k, v)| s.properties.get(k) == Some(v))
}

/// The names `native_difficulty_changed` hears for a change from `from` to
/// `to`: the four difficulties of `options.difficulty.*`, the index kept in
/// range as the command and the options keep it.
fn difficulty_names(from: u8, to: u8) -> (&'static str, &'static str) {
    (
        DIFFICULTIES[usize::from(from) & 3],
        DIFFICULTIES[usize::from(to) & 3],
    )
}

#[cfg(test)]
mod teleport_pitch_tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn teleport_and_rotate_resolve_pitch_at_their_own_boundary() {
        use super::Target;
        use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        game.mobs.push(horizon_client::mobs::Mob::new(
            42,
            kind,
            horizon_core::geometry::Vec3::new(0.0, 0.0, 0.0),
            0.0,
        ));
        for (rotation, expected) in [
            ([40.0, f32::NAN], [40.0, 20.0]),
            ([f32::INFINITY, 30.0], [10.0, 30.0]),
            ([40.0, f32::INFINITY], [40.0, 90.0]),
            ([40.0, f32::NEG_INFINITY], [40.0, -90.0]),
        ] {
            game.yaw = 10.0;
            game.pitch = 20.0;
            game.teleport_to_in_level(Target::Player, [0.0; 3], Some(rotation), true);
            assert_eq!([game.yaw, game.pitch], expected);
            game.mobs[0].yaw = 10.0;
            game.mobs[0].head_pitch = 20.0;
            game.teleport_to_in_level(Target::Mob(42), [0.0; 3], Some(rotation), true);
            assert_eq!([game.mobs[0].yaw, game.mobs[0].head_pitch], expected);
        }
        game.build_registries();
        game.set_allow_commands(true);
        for (command, expected) in [
            ("rotate @e[type=minecraft:zombie,limit=1] 0 360", 90.0),
            ("tp @e[type=minecraft:zombie,limit=1] 0 0 0 0 360", 0.0),
            ("tp @e[type=minecraft:zombie,limit=1] 0 0 0 0 270", -90.0),
        ] {
            assert!(game.run_command(command), "{command}");
            assert_eq!(game.mobs[0].head_pitch, expected, "{command}");
        }
        for (source, target, angle, expected) in [
            (90.0, 90.0, "~100", 90.0),
            (-90.0, -90.0, "~-100", -90.0),
            (0.0, -90.0, "~100", -90.0),
            (0.0, 90.0, "~-100", 90.0),
        ] {
            game.pitch = source;
            game.mobs[0].head_pitch = target;
            assert!(game.run_command(&format!(
                "tp @e[type=minecraft:zombie,limit=1] 0 0 0 0 {angle}"
            )));
            assert_eq!(game.mobs[0].head_pitch, expected);
        }
        game.yaw = 190.0;
        game.mobs[0].yaw = 170.0;
        assert!(game.run_command("tp @e[type=minecraft:zombie,limit=1] 0 0 0 ~ ~"));
        assert_eq!(game.mobs[0].yaw, 190.0, "relative yaw wraps the delta");

        game.mobs[0].head_pitch = 360.0;
        assert!(
            game.run_command(
                "tp @e[type=minecraft:zombie,limit=1] @e[type=minecraft:zombie,limit=1]"
            )
        );
        assert_eq!(
            game.mobs[0].head_pitch, 0.0,
            "destination rotation wraps too"
        );

        let overflow = "9".repeat(400);
        for (pitch, expected) in [(overflow.clone(), 90.0), (format!("-{overflow}"), -90.0)] {
            assert!(game.run_command(&format!(
                "rotate @e[type=minecraft:zombie,limit=1] 0 {pitch}"
            )));
            assert_eq!(game.mobs[0].head_pitch, expected);
        }
        for (yaw, pitch, expected) in [
            ("40", overflow.as_str(), [40.0, 20.0]),
            (overflow.as_str(), "30", [10.0, 30.0]),
        ] {
            game.mobs[0].yaw = 10.0;
            game.mobs[0].head_pitch = 20.0;
            assert!(game.run_command(&format!(
                "tp @e[type=minecraft:zombie,limit=1] 0 0 0 {yaw} {pitch}"
            )));
            assert_eq!([game.mobs[0].yaw, game.mobs[0].head_pitch], expected);
        }
    }

    #[test]
    fn pitch_wraps_before_clamping() {
        assert_eq!(super::entity_pitch(360.0), 0.0);
        assert_eq!(super::entity_pitch(450.0), 90.0);
        assert_eq!(super::entity_pitch(-360.0), 0.0);
        assert_eq!(super::entity_pitch(-450.0), -90.0);
        assert_eq!(super::entity_pitch(super::wrap_degrees(270.0)), -90.0);
        assert_eq!(super::entity_pitch(super::wrap_degrees(-270.0)), 90.0);
        assert_eq!(super::entity_pitch(super::wrap_degrees(360.0)), 0.0);
        assert_eq!(super::entity_pitch(super::wrap_degrees(450.0)), 90.0);
        assert_eq!(super::wrap_degrees(190.0), -170.0);
        assert_eq!(super::wrap_degrees(540.0), -180.0);
    }
}

#[cfg(test)]
mod difficulty_tests {
    use super::difficulty_names;

    #[test]
    fn a_difficulty_change_names_both_ends() {
        assert_eq!(difficulty_names(0, 3), ("peaceful", "hard"));
        assert_eq!(difficulty_names(2, 1), ("normal", "easy"));
        // The four difficulties are the only indexes the command and the
        // options keep; an index outside them still names one.
        assert_eq!(difficulty_names(7, 4), ("hard", "peaceful"));
    }
}

#[cfg(test)]
mod summon_tests {
    #[test]
    fn a_summoned_entity_is_named_from_its_own_namespace() {
        use super::entity_name_key;
        assert_eq!(
            entity_name_key("minecraft:snowball"),
            "entity.minecraft.snowball"
        );
        assert_eq!(entity_name_key("ruby:bolt"), "entity.ruby.bolt");
        assert_eq!(entity_name_key("ruby:spell/bolt"), "entity.ruby.spell.bolt");
    }
}

#[cfg(test)]
mod look_at_tests {
    use super::{Target, non_player_entity_id};

    #[test]
    fn look_at_entity_form_names_any_entity_by_its_id() {
        for t in [
            Target::Mob(7),
            Target::Item(7),
            Target::Orb(7),
            Target::Projectile(7),
            Target::Boat(7),
            Target::Cart(7),
            Target::Crystal(7),
            Target::Dragon(7),
            Target::Tnt(7),
            Target::Cushion(7),
            Target::Falling(7),
            Target::Rocket(7),
            Target::Trident(7),
            Target::Spawner(7),
            Target::Cloud(7),
        ] {
            assert_eq!(non_player_entity_id(t), Some(7), "{t:?}");
        }
        // Players are the hub's to name; a dragon part's id is no entity id.
        assert_eq!(non_player_entity_id(Target::Player), None);
        assert_eq!(non_player_entity_id(Target::Guest(3)), None);
        assert_eq!(non_player_entity_id(Target::Mob(u32::MAX - 4096)), None);
    }
}

#[cfg(test)]
mod percent_f_tests {
    use super::percent_f;

    #[test]
    fn rounds_the_shortest_decimal_half_up_like_java() {
        // WorldBorderCommand: "%.1f" distance, "%.2f" buffer, "%.0f" size.
        assert_eq!(percent_f(10.35, 1), "10.4");
        assert_eq!(percent_f(5.125, 2), "5.13");
        assert_eq!(percent_f(10.5, 0), "11");
        assert_eq!(percent_f(2.5, 0), "3");
        // TeleportCommand.formatDouble: "%f".
        assert_eq!(percent_f(0.1234565, 6), "0.123457");
        assert_eq!(percent_f(-1.005, 2), "-1.01");
        assert_eq!(percent_f(3.0, 2), "3.00");
    }
}

#[cfg(test)]
mod teleport_rider_tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_vehicle_carries_its_riders_and_a_rider_leaves_its_vehicle() {
        use super::Target;
        use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
        use horizon_core::geometry::Vec3;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let horse = game.res.mob_types().get("minecraft:horse").unwrap().clone();
        let villager = game
            .res
            .mob_types()
            .get("minecraft:villager")
            .unwrap()
            .clone();
        // A villager riding a horse, 30 degrees off it.
        game.mobs.push(horizon_client::mobs::Mob::new(
            42,
            horse,
            Vec3::new(3.0, 64.0, 5.0),
            0.0,
        ));
        let mut rider =
            horizon_client::mobs::Mob::new(43, villager, Vec3::new(3.0, 64.5, 5.0), 30.0);
        rider.extra.mount = Some(42);
        game.mobs.push(rider);
        // Entity.teleportPassengers: the rider keeps its offset from the
        // vehicle and turns by what the vehicle turned.
        game.teleport_to_in_level(Target::Mob(42), [10.0, 70.0, -4.0], Some([90.0, 0.0]), true);
        let vehicle = game.mobs.iter().find(|m| m.id == 42).unwrap();
        assert_eq!(vehicle.position, Vec3::new(10.0, 70.0, -4.0));
        let rider = game.mobs.iter().find(|m| m.id == 43).unwrap();
        assert_eq!(rider.position, Vec3::new(10.0, 70.5, -4.0));
        assert_eq!(rider.yaw, 120.0, "the rider keeps its angle to the vehicle");
        assert_eq!(
            rider.extra.mount,
            Some(42),
            "a teleported vehicle keeps its riders"
        );
        // A relative yaw (`~90`) is the same turn: the transition carries the
        // 90-degree delta and the rider adds it to its own angle.
        game.teleport_to_in_level(
            Target::Mob(42),
            [10.0, 70.0, -4.0],
            Some([180.0, 0.0]),
            true,
        );
        let rider = game.mobs.iter().find(|m| m.id == 43).unwrap();
        assert_eq!(rider.yaw, 210.0, "the rider turns by the vehicle's turn");
        // `facing` keeps the transition's rotation: the vehicle looks, the rider does not turn.
        game.teleport_to_in_level(Target::Mob(42), [10.0, 70.0, -4.0], Some([0.0, 0.0]), false);
        let rider = game.mobs.iter().find(|m| m.id == 43).unwrap();
        assert_eq!(
            rider.yaw, 210.0,
            "a vehicle turned by `facing` leaves its rider"
        );
        // Entity.teleport's stopRiding(): a target that is itself a passenger
        // leaves its vehicle, or the next sync seats it where it was.
        game.teleport_to_in_level(Target::Mob(43), [0.0, 64.0, 0.0], None, false);
        let rider = game.mobs.iter().find(|m| m.id == 43).unwrap();
        assert_eq!(rider.extra.mount, None, "the teleported rider dismounted");
        assert_eq!(rider.position, Vec3::new(0.0, 64.0, 0.0));
        // #9335 (2): a rider keeps its angle to the vehicle through an
        // absolute turn, and a relative turn adds the delta: a villager 30
        // degrees off a horse turning 0 -> 40 ends at 70.
        let horse2 = game.res.mob_types().get("minecraft:horse").unwrap().clone();
        let villager2 = game
            .res
            .mob_types()
            .get("minecraft:villager")
            .unwrap()
            .clone();
        game.mobs.push(horizon_client::mobs::Mob::new(
            50,
            horse2,
            Vec3::new(10.0, 64.0, 10.0),
            0.0,
        ));
        let mut rider2 =
            horizon_client::mobs::Mob::new(51, villager2, Vec3::new(10.0, 64.5, 10.0), 30.0);
        rider2.extra.mount = Some(50);
        game.mobs.push(rider2);
        // Absolute turn: vehicle 0 -> 40, rider keeps its 30-degree offset.
        game.teleport_to_in_level(Target::Mob(50), [10.0, 64.0, 10.0], Some([40.0, 0.0]), true);
        let rider2 = game.mobs.iter().find(|m| m.id == 51).unwrap();
        assert_eq!(
            rider2.yaw, 70.0,
            "absolute turn keeps the rider's angle to the vehicle"
        );
        // Relative turn (~10 on the vehicle): the transition carries the
        // 10-degree delta and the rider adds it to its own angle (70 -> 80).
        game.teleport_to_in_level(Target::Mob(50), [10.0, 64.0, 10.0], Some([50.0, 0.0]), true);
        let rider2 = game.mobs.iter().find(|m| m.id == 51).unwrap();
        assert_eq!(
            rider2.yaw, 80.0,
            "relative turn shifts the rider by the delta"
        );
        // `facing` (turn_riders = false) leaves the rider's angle alone.
        game.teleport_to_in_level(Target::Mob(50), [10.0, 64.0, 10.0], Some([0.0, 0.0]), false);
        let rider2 = game.mobs.iter().find(|m| m.id == 51).unwrap();
        assert_eq!(
            rider2.yaw, 80.0,
            "a vehicle turned by `facing` leaves its rider"
        );
        // Native rotation preserves a rider's mount, position and motion.
        game.native_lifecycle
            .begin_open(horizon_client::mod_api::NativeAuthority::IntegratedServer);
        let rider = game.mobs.iter_mut().find(|m| m.id == 51).unwrap();
        rider.velocity = Vec3::new(0.2, 0.3, 0.4);
        let position = rider.position;
        let velocity = rider.velocity;
        let handle = game.native_entity(51).unwrap();
        assert!(game.native_entity_set_rotation(handle, -135.0, 120.0));
        let rider = game.mobs.iter().find(|m| m.id == 51).unwrap();
        assert_eq!(rider.extra.mount, Some(50));
        assert_eq!(rider.position, position);
        assert_eq!(rider.velocity, velocity);
        assert_eq!((rider.yaw, rider.head_pitch), (-135.0, 90.0));
        assert!(!game.native_entity_set_rotation(handle, f32::NAN, 0.0));
    }
}

#[cfg(test)]
mod selector_bounds_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{entities::ArmorStand, mobs::Mob};
    use horizon_core::geometry::Vec3;

    #[test]
    fn actual_selector_uses_body_overlap_but_distance_stays_at_the_origin() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        if !root.join("entity-types.json").exists() {
            eprintln!("Prepared resources unavailable; Game selector coverage did not run");
            return;
        }
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
        game.mobs
            .push(Mob::new(30, kind, Vec3::new(10.5, 220.0, 10.5), 0.0));
        let target = Target::Mob(30);
        let mut selector = Selector {
            kind: 'e',
            include_entities: true,
            limit: usize::MAX,
            delta: [Some(0.0); 3],
            ..Default::default()
        };
        let origin = [9.5, 220.0, 10.5];
        assert_eq!(
            game.select_in_level(vec![target], &selector, origin),
            vec![(target, 1.0)]
        );
        selector.distance = Some((None, Some(0.9)));
        assert!(
            game.select_in_level(vec![target], &selector, origin)
                .is_empty()
        );
        selector.distance = None;
        assert!(
            game.select_in_level(vec![target], &selector, [11.0, 220.0, 10.5])
                .is_empty()
        );
        selector.delta = [Some(-1.0), None, None];
        assert_eq!(
            game.select_in_level(vec![target], &selector, [11.5, 220.0, 10.5])
                .len(),
            1
        );
        game.armor_stands
            .push(ArmorStand::new(31, Vec3::new(30.5, 220.0, 10.5), 0.0));
        selector.delta = [Some(0.0); 3];
        for (x, count) in [(29.5, 1), (29.25, 0), (30.75, 0), (29.250000000000004, 1)] {
            assert_eq!(
                game.select_in_level(vec![Target::Stand(31)], &selector, [x, 220.0, 10.5])
                    .len(),
                count
            );
        }
    }
}

#[cfg(test)]
mod local_coordinate_command_tests {
    use super::super::native_machines::tests::server_game;
    use super::*;
    use horizon_core::position::BlockPos;
    use horizon_world::nbt::Tag;

    fn pos_bits(tag: &Tag) -> [u64; 3] {
        let Some(Tag::List(pos)) = tag.get("Pos") else {
            panic!("actual saved Double position missing")
        };
        assert_eq!(pos.len(), 3);
        std::array::from_fn(|i| match &pos[i] {
            Tag::Double(bits) => *bits,
            other => panic!("actual Pos must be Double: {other:?}"),
        })
    }
    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn h149_actual_registered_local_commands_preserve_exact_saved_positions() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("fill 0 199 0 15 199 15 stone"));
        assert!(game.run_command("scoreboard objectives add h149 dummy"));
        let clock = game.world.game_time;
        let mut failures = Vec::new();
        let cases = [
            (
                "south",
                "execute positioned 8.5 202.25 8.5 rotated 0.0 0.0 run tp 00000095-0000-0001-0000-000000000001 ^0.75 ^0.5 ^1.25",
                [
                    0x4022800000000000u64,
                    0x4069580000000000u64,
                    0x4023800000000000u64,
                ],
                false,
            ),
            (
                "west",
                "execute positioned 8.5 202.25 8.5 rotated 90.0 0.0 run tp 00000095-0000-0001-0000-000000000002 ^0.75 ^0.5 ^1.25",
                [
                    0x401d000000000000u64,
                    0x4069580000000000u64,
                    0x4022800000000000u64,
                ],
                false,
            ),
            (
                "north",
                "execute positioned 8.5 202.25 8.5 rotated 180.0 0.0 run tp 00000095-0000-0001-0000-000000000003 ^0.75 ^0.5 ^1.25",
                [
                    0x401f000000000000u64,
                    0x4069580000000000u64,
                    0x401d000000000000u64,
                ],
                false,
            ),
            (
                "east",
                "execute positioned 8.5 202.25 8.5 rotated 270.0 0.0 run tp 00000095-0000-0001-0000-000000000004 ^0.75 ^0.5 ^1.25",
                [
                    0x4023800000000000u64,
                    0x4069580000000000u64,
                    0x401f000000000000u64,
                ],
                false,
            ),
            (
                "oblique",
                "execute positioned 8.5 202.25 8.5 rotated 37.5 -22.25 run tp 00000095-0000-0001-0000-000000000005 ^0.1 ^0.1 ^0.1",
                [
                    0x402117934a6314b9u64,
                    0x40694c2c5ee98f58u64,
                    0x40213562572e889au64,
                ],
                false,
            ),
            (
                "positioned_oblique",
                "execute positioned 8.5 202.25 8.5 rotated 123.75 47.125 positioned ^0.1 ^0.1 ^0.1 run tp 00000095-0000-0001-0000-000000000006 ~ ~ ~",
                [
                    0x4020a7648c7aa86bu64,
                    0x406947d514129374u64,
                    0x4021025e8b180036u64,
                ],
                false,
            ),
            (
                "world",
                "execute positioned 8.5 202.25 8.5 rotated 37.5 -22.25 run tp 00000095-0000-0001-0000-000000000007 9.25 202.75 9.75",
                [
                    0x4022800000000000u64,
                    0x4069580000000000u64,
                    0x4023800000000000u64,
                ],
                true,
            ),
            (
                "relative_eyes",
                "execute positioned 8.5 202.25 8.5 rotated 37.5 -22.25 anchored eyes run tp 00000095-0000-0001-0000-000000000008 ~0.75 ~0.5 ~1.25",
                [
                    0x4022800000000000u64,
                    0x4069580000000000u64,
                    0x4023800000000000u64,
                ],
                true,
            ),
            (
                "feet",
                "execute as 00000095-0000-0001-0000-000000000009 at @s anchored feet rotated 37.5 -22.25 run tp @s ^ ^ ^",
                [
                    0x4012000000000000u64,
                    0x4069000000000000u64,
                    0x4012000000000000u64,
                ],
                true,
            ),
            (
                "eyes",
                "execute as 00000095-0000-0001-0000-00000000000a at @s anchored eyes rotated 37.5 -22.25 run tp @s ^ ^ ^",
                [
                    0x4012000000000000u64,
                    0x4069187ae1400000u64,
                    0x4012000000000000u64,
                ],
                true,
            ),
        ];
        for (i, (name, command, want, control)) in cases.into_iter().enumerate() {
            let number = i as i32 + 1;
            let actor = format!("00000095-0000-0001-0000-{number:012x}");
            let uuid = [149, 1, 0, number];
            assert!(game.run_command(&format!(r#"summon pig 4.5 200 4.5 {{UUID:[I;149,1,0,{number}],Tags:["h149"],Health:10f,NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Motion:[0d,0d,0d],Rotation:[11f,7f]}}"#)));
            let id = *game
                .persist
                .uuids
                .iter()
                .find(|(_, u)| **u == uuid)
                .unwrap()
                .0;
            assert!(game.mobs.iter().any(|m| m.id == id));
            assert!(game.run_command(&format!("execute if entity @e[type=pig,nbt={{UUID:[I;149,1,0,{number}],Health:10f,NoAI:1b,NoGravity:1b,Pos:[4.5d,200d,4.5d],Motion:[0d,0d,0d],Rotation:[11f,7f]}}]")));
            assert!(game.world.loaded(4, 4));
            let before = game.entity_nbt(Target::Mob(id)).unwrap();
            assert_eq!(pos_bits(&before), [4.5f64, 200.0, 4.5].map(f64::to_bits));
            assert_eq!(before.get("UUID"), Some(&Tag::IntArray(uuid.to_vec())));
            assert_eq!(before.get("Health"), Some(&Tag::Float(10f32.to_bits())));
            // Discard retained raw input: only actual modeled position may prove this.
            game.persist.raw.remove(&id);
            assert!(game.run_command("scoreboard players set #r h149 91"));
            assert!(game.run_command("scoreboard players set #s h149 17"));
            assert!(
                game.run_command(&format!(
                    "execute store result score #r h149 store success score #s h149 run {command}"
                )),
                "actual command accepted: {name}"
            );
            assert_eq!(game.score.board.score("#r", "h149").unwrap().value, 1);
            assert_eq!(game.score.board.score("#s", "h149").unwrap().value, 1);
            let model = game.mobs.iter().find(|m| m.id == id).unwrap();
            let model_bits =
                [model.position.x, model.position.y, model.position.z].map(f64::to_bits);
            let saved = game.entity_nbt(Target::Mob(id)).unwrap();
            let got = pos_bits(&saved);
            assert_eq!(
                model_bits, got,
                "actual model and typed canonical save agree: {name}"
            );
            assert_eq!(saved.get("UUID"), before.get("UUID"));
            for key in ["Health", "NoAI", "NoGravity", "Rotation", "Motion"] {
                assert_eq!(saved.get(key), before.get(key), "unchanged {key}: {name}");
            }
            assert!(game.run_command(&format!("execute as {actor} at @s if block ~ 199 ~ stone")));
            assert!(game.world.loaded(
                f64::from_bits(got[0]).floor() as i32,
                f64::from_bits(got[2]).floor() as i32,
            ));
            if control {
                assert_eq!(
                    got, want,
                    "independent world/relative/anchor control {name}"
                );
            } else if got != want {
                failures.push((name, got, want));
            }
            assert_eq!(game.world.game_time, clock, "zero observation ticks");
        }
        assert!(
            failures.is_empty(),
            "actual registered local-coordinate bit mismatches: {failures:#?}"
        );
    }
}

#[cfg(test)]
mod query_feedback_tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_silent_source_drops_query_results_but_still_stores_them() {
        use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command("time set 1234"));
        assert!(game.run_command("scoreboard objectives add q dummy"));
        let queries = [
            "time query time",
            "time query day",
            "time query day repetition",
            "time query gametime",
            "seed",
            "gamerule keep_inventory",
            "difficulty",
            "list",
            "help seed",
        ];
        // A function's source (silent, like `run_command_quietly`) is told nothing.
        for command in queries {
            game.chat_capture = Some(Vec::new());
            assert!(game.run_command_quietly(command), "{command}");
            let lines = game.chat_capture.take().unwrap();
            assert!(lines.is_empty(), "{command}: {:?}", lines[0].plain());
        }
        // The player's own source still sees each result.
        for command in queries {
            game.chat_capture = Some(Vec::new());
            assert!(game.run_command(command), "{command}");
            assert_eq!(game.chat_capture.take().unwrap().len(), 1, "{command}");
        }
        // The result still reaches `execute store`.
        assert!(game.run_command_quietly("execute store result score #t q run time query day"));
        game.chat_capture = Some(Vec::new());
        assert!(game.run_command("scoreboard players get #t q"));
        let lines = game.chat_capture.take().unwrap();
        assert!(lines[0].plain().contains("1234"), "{:?}", lines[0].plain());
    }
}

#[cfg(test)]
mod forced_respawn_tests {
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn overworld_spawnpoint_is_validated_at_both_player_blocks() {
        use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
        use horizon_client::chunk::Chunk;
        use horizon_core::geometry::Vec3;
        use std::sync::Arc;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.player = Some(
            horizon_client::motion::PlayerMotion::new(game.motion, Vec3::new(5.5, 64.0, 5.5))
                .unwrap(),
        );
        game.build_registries();
        game.set_allow_commands(true);
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.level.world_spawn = Some([0, 80, 0]);
        let registry = game.pack.assets.registry;
        let air = registry.default_state("minecraft:air").unwrap();
        // Liquid is a block property: aquatic plants such as kelp still permit
        // a forced point, while a bubble column does not, at feet or head.
        for (block, allowed) in [
            ("air", true),
            ("water", false),
            ("lava", false),
            ("bubble_column", false),
            ("kelp", true),
            ("stone", false),
        ] {
            let state = registry
                .default_state(&format!("minecraft:{block}"))
                .unwrap();
            for height in [0, 1] {
                game.world.set(5, 64, 5, air).unwrap();
                game.world.set(5, 65, 5, air).unwrap();
                game.world.set(5, 64 + height, 5, state).unwrap();
                assert!(game.run_command("spawnpoint @s 5 64 5"));
                let config = game
                    .respawn_config
                    .expect("Overworld commands must retain a forced configuration");
                assert!(config.forced);
                assert_eq!(config.pos, [5, 64, 5]);
                let actual = game.resolve_server_respawn(config);
                assert_eq!(actual.is_some(), allowed, "{block} at height {height}");
                assert_eq!(game.respawn_config.is_some(), allowed);
                assert_eq!(game.level.world_spawn, Some([0, 80, 0]));
                if let Some((at, yaw)) = actual {
                    assert_eq!(at, Vec3::new(5.5, 64.1, 5.5));
                    assert_eq!(yaw, 0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod fill_note_instrument_tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, motion::PlayerMotion};
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;
    fn with_game(test: impl FnOnce(&mut Game<'_>)) {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.game_dir = Some(
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
                .join(format!("h164-{:?}", std::thread::current().id())),
        );
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);

        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(0, 0))
            .unwrap();
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
        game.creative = true;
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("tick freeze"));
        assert!(game.run_command("fill 0 198 0 15 199 15 stone"));
        test(&mut game);
    }

    fn instrument(game: &Game<'_>, x: i32, z: i32) -> String {
        let state = game
            .pack
            .assets
            .registry
            .state(game.world.block(x, 200, z))
            .unwrap();
        assert_eq!(state.name, "minecraft:note_block");
        state.properties["instrument"].clone()
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn ordinary_fill_normalizes_a_note_block_against_real_stone() {
        with_game(|game| {
            let clock = game.world.game_time;
            assert!(
                game.world.chunks.contains_key(&(0, 0)),
                "actual fill chunk is resident"
            );
            assert!(game.run_command("execute if block 4 199 4 stone"));
            assert!(game.run_command("execute if block 4 200 4 air"));
            assert!(game.run_command("setblock 8 200 4 note_block"));
            assert_eq!(
                instrument(game, 8, 4),
                "basedrum",
                "positive setblock counterpart"
            );
            assert!(game.run_command("fill 4 200 4 4 200 4 note_block"));
            assert_eq!(
                instrument(game, 4, 4),
                "basedrum",
                "actual registered fill must normalize instrument"
            );
            assert!(game.run_command("execute if block 4 200 4 note_block[note=0,powered=false]"));
            assert_eq!(game.world.game_time, clock);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn an_existing_default_state_does_not_skip_note_normalization() {
        with_game(|game| {
            let clock = game.world.game_time;
            assert!(game.run_command("setblock 4 200 4 note_block strict"));
            assert_eq!(instrument(game, 4, 4), "harp");
            assert!(
                game.run_command("fill 4 200 4 4 200 4 note_block"),
                "valid fill changes existing harp to basedrum"
            );
            assert_eq!(instrument(game, 4, 4), "basedrum");
            assert_eq!(game.world.game_time, clock);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn strict_explicit_and_filtered_out_inputs_keep_their_existing_contracts() {
        with_game(|game| {
            let clock = game.world.game_time;
            assert!(game.run_command("fill 4 200 4 4 200 4 note_block strict"));
            assert_eq!(instrument(game, 4, 4), "harp");
            assert!(game.run_command("fill 8 200 4 8 200 4 note_block[instrument=banjo,note=7]"));
            assert_eq!(instrument(game, 8, 4), "banjo");
            assert!(game.run_command("execute if block 8 200 4 note_block[note=7,powered=false]"));
            assert!(game.run_command("fill 12 200 4 12 200 4 note_block[instrument=harp,note=7]"));
            assert_eq!(instrument(game, 12, 4), "harp");
            assert!(!game.run_command("fill 4 200 4 4 200 4 note_block[instrument=banjo] keep"));
            assert!(
                !game
                    .run_command("fill 4 200 4 4 200 4 note_block[instrument=banjo] replace glass")
            );
            assert_eq!(instrument(game, 4, 4), "harp");
            assert_eq!(game.world.game_time, clock);
        });
    }
}

#[cfg(test)]
mod item_damage_range_tests {
    use super::super::native_machines::tests::server_game;
    use super::*;
    use horizon_core::position::BlockPos;

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn actual_chest_damage_range_uses_live_damage_and_effective_presence() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        game.block_ticks
            .add_empty_chunk(BlockPos::new(0, 0, 0).chunk())
            .unwrap();
        assert!(game.run_command("forceload add 0 0"));
        assert!(game.run_command("setblock 4 199 4 stone"));
        assert!(game.run_command(r#"setblock 4 200 4 chest{Items:[{Slot:0b,id:"minecraft:diamond_sword",count:1,components:{"minecraft:custom_data":{marker:195}}},{Slot:1b,id:"minecraft:diamond_sword",count:1,components:{"minecraft:damage":3,"minecraft:custom_data":{marker:195}}},{Slot:2b,id:"minecraft:diamond_sword",count:1,components:{"minecraft:damage":4,"minecraft:custom_data":{marker:195}}},{Slot:3b,id:"minecraft:diamond_sword",count:1,components:{"minecraft:damage":5,"minecraft:custom_data":{marker:195}}},{Slot:4b,id:"minecraft:stone",count:1,components:{"minecraft:custom_data":{marker:195}}},{Slot:5b,id:"minecraft:diamond_sword",count:1,components:{"!minecraft:damage":{},"minecraft:custom_data":{marker:195}}}]}"#));
        assert!(game.run_command("scoreboard objectives add h195 dummy"));
        assert!(game.world.loaded(4, 4));
        assert!(game.run_command("execute if block 4 199 4 stone"));
        let clock = game.world.game_time;
        let stacks = match game.world.block_entities.get(&(4, 200, 4)).unwrap() {
            horizon_client::containers::BlockEntity::Chest(slots) => slots.clone(),
            _ => panic!("actual loaded chest missing"),
        };
        for (slot, damage) in [0, 3, 4, 5, 0, 0].into_iter().enumerate() {
            let stack = stacks[slot].as_ref().unwrap();
            assert_eq!(stack.damage, damage);
            assert_eq!(stack.count, 1);
            assert_eq!(
                &*stack.item,
                if slot == 4 {
                    "minecraft:stone"
                } else {
                    "minecraft:diamond_sword"
                }
            );
            assert_eq!(
                stack.custom_data().unwrap().get(&"marker".into()),
                Some(&horizon_world::nbt::Tag::Int(195))
            );
            assert!(game.run_command(&format!(
                "execute if items block 4 200 4 container.{slot} {}",
                stack.item
            )));
        }
        assert!(stacks[6].is_none());
        assert!(
            game.items()
                .get("minecraft:diamond_sword")
                .unwrap()
                .components
                .iter()
                .any(|c| c == "minecraft:damage")
        );
        assert!(
            !game
                .items()
                .get("minecraft:stone")
                .unwrap()
                .components
                .iter()
                .any(|c| c == "minecraft:damage")
        );
        let removed =
            horizon_client::item_data::encode_components(stacks[5].as_ref().unwrap()).unwrap();
        assert!(
            removed.get("!minecraft:damage").is_some(),
            "actual removal patch must survive load before evaluating it"
        );
        assert!(game.run_command(
            "execute unless data block 4 200 4 Items[{Slot:0b}].components.\"minecraft:damage\""
        ));
        assert!(game.run_command(
            "execute if data block 4 200 4 Items[{Slot:2b,components:{\"minecraft:damage\":4}}]"
        ));
        assert!(game.run_command(
            "execute if data block 4 200 4 Items[{Slot:5b}].components.\"!minecraft:damage\""
        ));
        let mut cases = Vec::new();
        for (slot, damage) in [(1, 3), (2, 4), (3, 5)] {
            for (shape, expected) in [
                ("4", damage == 4),
                ("{min:4}", damage >= 4),
                ("{max:4}", damage <= 4),
                ("{min:3,max:4}", (3..=4).contains(&damage)),
            ] {
                cases.push((
                    slot,
                    format!("minecraft:damage~{{damage:{shape}}}"),
                    expected,
                ));
            }
        }
        cases.extend([
            (0, "damage~{damage:0}".into(), true),
            (0, "damage~{damage:{min:1}}".into(), false),
            (4, "damage~{damage:0}".into(), false),
            (5, "damage~{damage:0}".into(), false),
            (2, "!damage~{damage:4}".into(), false),
            (1, "!damage~{damage:4}".into(), true),
            (1, "damage=3".into(), true),
            (1, "damage=4".into(), false),
            (1, "count~1".into(), true),
            (1, "custom_data~{marker:195}".into(), true),
        ]);
        let mut mismatches = Vec::new();
        for (slot, term, expected) in cases {
            let stack = stacks[slot].as_ref().unwrap();
            let predicate = ItemInput {
                id: stack.item.to_string(),
                tag: false,
                any: false,
                components: Some(format!("[{term}]")),
            };
            let actual = game.stack_matches(&predicate, stack);
            if actual != expected {
                mismatches.push(format!(
                    "direct slot{slot} {term}: {actual}, expected{expected}"
                ));
            }
            assert!(game.run_command("scoreboard players set #r h195 91"));
            assert!(game.run_command("scoreboard players set #s h195 17"));
            let command = format!(
                "execute store result score #r h195 store success score #s h195 if items block 4 200 4 container.{slot} {}[{term}]",
                stack.item
            );
            game.run_command(&command);
            let r = game.score.board.score("#r", "h195").map(|s| s.value);
            let s = game.score.board.score("#s", "h195").map(|s| s.value);
            if (r, s) != (Some(i32::from(expected)), Some(i32::from(expected))) {
                mismatches.push(format!("command {term}: {r:?}/{s:?}, expected{expected}"));
            }
            assert!(game.run_command("scoreboard players set #branch h195 23"));
            game.run_command(&format!("execute unless items block 4 200 4 container.{slot} {}[{term}] run scoreboard players set #branch h195 7",stack.item));
            let branch = game.score.board.score("#branch", "h195").map(|s| s.value);
            if branch != Some(if expected { 23 } else { 7 }) {
                mismatches.push(format!("unless {term}: {branch:?}"));
            }
        }
        assert!(!game.run_command(
            "execute if items block 4 200 4 container.4 diamond_sword[damage~{damage:4}]"
        ));
        assert!(!game.run_command(
            "execute if items block 4 200 4 container.6 diamond_sword[damage~{damage:4}]"
        ));
        let actual = match game.world.block_entities.get(&(4, 200, 4)).unwrap() {
            horizon_client::containers::BlockEntity::Chest(slots) => slots,
            _ => panic!("source chest changed"),
        };
        assert_eq!(
            actual, &stacks,
            "queries must leave the authoritative loaded stacks unchanged"
        );
        assert_eq!(game.world.game_time, clock);
        assert!(
            mismatches.is_empty(),
            "damage-only command predicate mismatches: {mismatches:#?}"
        );
    }
}

#[cfg(test)]
mod vehicle_kill_tests {
    use super::*;
    use crate::game::BlockEntity;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_world::nbt::parse_snbt;

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn kill_spills_explicit_vehicle_contents_after_menu_writeback() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let clock = game.world.game_time;
        let mut differences = Vec::new();
        for (index, kind) in [
            "oak_chest_boat",
            "bamboo_chest_raft",
            "chest_minecart",
            "hopper_minecart",
            "oak_boat",
        ]
        .iter()
        .enumerate()
        {
            let uuid = [51, 0, 0, index as i32 + 1];
            let source = r#"{Pos:[5.25d,200d,4.75d],NoGravity:1b,Items:[{Slot:0b,id:"minecraft:diamond",count:3,components:{"minecraft:custom_data":{h051:1}}},{Slot:1b,id:"minecraft:dirt",count:5},{Slot:2b,id:"minecraft:stick",count:7}]}"#.to_string();
            let (tag, _) = parse_snbt(&source).unwrap();
            let boat = kind.contains("boat") || kind.contains("raft");
            let (id, mut slots) = if boat {
                game.load_boat(tag, Some(uuid), &format!("minecraft:{kind}"));
                let b = game.boats.last().unwrap();
                assert_eq!(b.position, Vec3::new(5.25, 200.0, 4.75));
                (b.id, b.items.clone())
            } else {
                game.load_cart(tag, Some(uuid), &format!("minecraft:{kind}"));
                let c = game.carts.last().unwrap();
                (c.id, c.items.clone())
            };
            let container = index < 4;
            assert_eq!(
                slots.iter().flatten().map(|s| s.count).sum::<u32>(),
                if container { 15 } else { 0 }
            );
            let target = if boat {
                Target::Boat(id)
            } else {
                Target::Cart(id)
            };
            assert_eq!(
                game.entity_nbt(target).unwrap().get("UUID"),
                Some(&horizon_world::nbt::Tag::IntArray(uuid.to_vec()))
            );
            // The active menu's actual edited slot must win over the older model.
            let key = (i32::MAX - 15, crate::game::minecarts::CART_KEY_Y, id as i32);
            if container {
                slots[0].as_mut().unwrap().count = 4;
                game.world
                    .block_entities
                    .insert(key, BlockEntity::Chest(slots.clone()));
                game.container = Some(key);
            }
            let old_len = game.dropped.len();
            let actor = format!("00000033-0000-0000-0000-{:012x}", index + 1);
            assert!(
                game.run_command(&format!("kill {actor}")),
                "{kind}: actual command admitted"
            );
            assert!(!game.boats.iter().any(|b| b.id == id));
            assert!(!game.carts.iter().any(|c| c.id == id));
            assert!(!game.world.block_entities.contains_key(&key));
            assert_ne!(game.container, Some(key));
            let dropped = &game.dropped[old_len..];
            let expected: Vec<_> = slots.into_iter().flatten().collect();
            if dropped.len() != expected.len()
                || expected
                    .iter()
                    .any(|s| !dropped.iter().any(|d| &d.stack == s))
            {
                differences.push(format!(
                    "{kind}: expected {expected:?}, actual {:?}",
                    dropped.iter().map(|d| &d.stack).collect::<Vec<_>>()
                ));
            }
        }
        assert_eq!(game.world.game_time, clock);
        assert!(
            differences.is_empty(),
            "kill must spill authoritative explicit contents: {differences:#?}"
        );
    }
}
