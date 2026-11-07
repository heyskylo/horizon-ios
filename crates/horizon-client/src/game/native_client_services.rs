//! Client services for trait mods, without exposing `Game` or the atlas.
use super::{Game, Screen};
use horizon_client::{
    client_resources::TexturePixels,
    collisions::SceneCollisions,
    hud::{HudBatch, rgba},
    hud_layout::GuiDraw,
    items::ItemStack,
    mod_api::Text,
    net::FRIEND,
};
use horizon_core::geometry::Vec3;
use horizon_friends::invite::{self, Parsed};
use std::borrow::Cow;
use std::net::SocketAddr;
use winit::keyboard::KeyCode;

/// This window's state; unavailable on a server or before a window is attached.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientState {
    pub window_id: u64,
    pub focused: bool,
    pub gui_hidden: bool,
    pub gui_size: [f32; 2],
    pub server_address: Option<String>,
    pub logged_in: bool,
    pub world_partial_tick: f64,
    pub eye_position: Option<[f64; 3]>,
    pub yaw: f32,
    pub pitch: f32,
    pub menu_blur: f32,
    /// What covers the world. API @API:session_state@.
    pub screen: NativeScreenKind,
    /// F3's debug screen is showing (`DebugScreenOverlay.showDebugScreen`).
    /// API @API:session_state@.
    pub debug_screen: bool,
    /// The world joined, None before one is. API @API:session_state@.
    pub world: Option<NativeWorldId>,
    /// The head of the bed the player sleeps in, as the server says
    /// (`LivingEntity.getSleepingPos`). API @API:session_state@.
    pub sleeping_pos: Option<[i32; 3]>,
}

/// What covers the world, in the kinds a HUD tells apart (vanilla's
/// `Minecraft.screen`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeScreenKind {
    /// No screen: the world is played.
    #[default]
    Playing,
    /// The chat box (`ChatScreen`), which a minimap still draws under.
    Chat,
    /// The death screen (`DeathScreen`).
    Death,
    /// Any other: the pause menu, an inventory or container, a sign, book,
    /// dialog or prompt, the loading screen, the title's screens.
    Other,
}

impl NativeScreenKind {
    /// `covered`: a screen Horizon keeps outside `Screen` is up over it.
    fn of(screen: Screen, covered: bool) -> Self {
        match screen {
            _ if covered => Self::Other,
            Screen::Playing => Self::Playing,
            Screen::Chat => Self::Chat,
            Screen::Dead => Self::Death,
            _ => Self::Other,
        }
    }
}

/// The world this window plays, for a mod that keeps data per world, as
/// Xaero's minimap and JourneyMap do. A later Horizon may add kinds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NativeWorldId {
    /// Single player, its LAN host and splitscreen seats: the save's folder
    /// in `saves/`, not its shown name.
    Singleplayer(String),
    /// A server, a LAN world joined included: its address as the player
    /// joined it (`mc.example.com:25565`). A friend's world is `friend:` and
    /// what its invite shows anyone (its host's addresses and its relay
    /// nameplate), never the invite's secret.
    Server(String),
}

impl NativeWorldId {
    /// `save` is set for single player's world joined through memory.
    fn of(save: Option<&str>, address: &str) -> Self {
        match (save, address.strip_prefix(FRIEND)) {
            (Some(folder), _) => Self::Singleplayer(folder.to_owned()),
            (None, Some(code)) => Self::Server(format!("{FRIEND}{}", invite_public_part(code))),
            (None, None) => Self::Server(address.to_owned()),
        }
    }
}

/// The part of a friend's invite anyone may see. A host's code is fresh each
/// time it shares, so a friend's world is known for that sharing only. The
/// relay is kept beside the addresses: two hosts on other networks can share
/// a private address and the fixed direct port.
fn invite_public_part(code: &str) -> String {
    match invite::parse(code) {
        Some(Parsed::Short(code)) => code.nameplate,
        Some(Parsed::Long(invite)) => {
            let relay = invite
                .relay
                .map(|r| format!("{}/{}", r.address, r.code.nameplate));
            let endpoints = invite.endpoints.iter().map(SocketAddr::to_string);
            endpoints.chain(relay).collect::<Vec<_>>().join(",")
        }
        None => String::new(),
    }
}

/// A living entity known to the client, before interpolation. No authoritative writes.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientEntity {
    pub id: u32,
    pub player_uuid: Option<u128>,
    pub player_name: Option<String>,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub bounds: [[f64; 3]; 2],
    pub height: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub alive: bool,
    pub invisible: bool,
    /// Its entity type (`minecraft:zombie`). API @API:client_entity_list@.
    pub kind: String,
    /// `Entity.getYRot` in degrees, 0 facing south and 90 west.
    pub yaw: f32,
    pub baby: bool,
    /// A player in spectator mode, whom vanilla draws only to spectators.
    pub spectator: bool,
    /// The texture its body draws with (`EntityRenderer.getTextureLocation`):
    /// `entity/cow/cow_temperate_baby`, or a player's skin. Pixels through
    /// `native_client_texture`.
    pub texture: Option<String>,
}

impl NativeClientEntity {
    /// Where this frame draws it (`Entity.getPosition(partialTick)`), given
    /// `NativeClientState::world_partial_tick`. API @API:client_entity_list@.
    pub fn position_at(&self, partial_tick: f64) -> [f64; 3] {
        position_at(self.previous_position, self.position, partial_tick)
    }
}

/// A dropped item the client shows. API @API:client_entity_list@.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct NativeClientItem {
    pub id: u32,
    pub stack: ItemStack,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
}

impl NativeClientItem {
    /// Where this frame draws it, as `NativeClientEntity::position_at`.
    pub fn position_at(&self, partial_tick: f64) -> [f64; 3] {
        position_at(self.previous_position, self.position, partial_tick)
    }
}

/// `Downloader::image` keeps every skin at 64x64.
const SKIN_SIDE: u32 = 64;

fn position_at(previous: [f64; 3], position: [f64; 3], partial_tick: f64) -> [f64; 3] {
    let vec = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    let at = vec(previous).lerp(vec(position), partial_tick);
    [at.x, at.y, at.z]
}

/// A posed model head, relative to its interpolated origin, in block units.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientHead {
    pub id: u32,
    pub origin: [f64; 3],
    pub transform: Option<[f32; 16]>,
    pub outer_layer_padding: f32,
    pub height: f32,
    pub name_anchor: [f64; 3],
    pub name_half_width: f32,
}

/// A player-list profile and the skin the client has ready for GUI faces.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct NativeClientProfile {
    pub uuid: u128,
    pub name: String,
    pub skin: String,
}

#[allow(dead_code)]
impl Game<'_> {
    /// A screenshot as F2 takes it (`ScreenShot.grab`): of the next frame, at
    /// the game's resolution times Screenshot Quality, into `screenshots/` with
    /// vanilla's file name, and the same "Saved screenshot as" line in chat.
    /// The file it will write, as a path string; an error without a window to draw, or while
    /// another screenshot waits for the frame (F2's, an agent's, the world
    /// icon's), which a mod's would replace. API @API:take_screenshot@.
    pub(super) fn native_take_screenshot(&mut self) -> Result<String, String> {
        let Some(renderer) = self.renderer.as_ref() else {
            return Err("no window to take it from".into());
        };
        if renderer.screenshot.is_some() {
            return Err("another screenshot is waiting for the frame".into());
        }
        self.take_screenshot()
            .map(|path| path.to_string_lossy().into_owned())
            .ok_or_else(|| "the screenshots folder can't be made".into())
    }
    /// Client/window/connection state, including when menus cover the world.
    pub(super) fn native_client_state(&self) -> Option<NativeClientState> {
        if !self.native_window() {
            return None;
        }
        let (w, h) = self.gui_size();
        Some(NativeClientState {
            window_id: self.window.as_ref()?.id().into(),
            focused: self.focused,
            gui_hidden: self.ui.hide_gui,
            gui_size: [w, h],
            server_address: self.remote.as_ref().map(|r| r.session.address.clone()),
            logged_in: self.remote.as_ref().is_some_and(|r| r.logged_in),
            world_partial_tick: self.clock.fraction(),
            eye_position: self.eye_position(),
            yaw: self.yaw,
            pitch: self.pitch,
            menu_blur: self.settings.menu_blur as f32,
            screen: NativeScreenKind::of(
                self.screen,
                self.title.is_some()
                    || self.screen_over_playing()
                    || self.remote_screen_up()
                    || self.pack_reload.is_some(),
            ),
            debug_screen: self.debug,
            world: self
                .remote
                .as_ref()
                .filter(|r| r.logged_in)
                .map(|r| NativeWorldId::of(r.save_folder.as_deref(), &r.session.address)),
            sleeping_pos: self.sleeping,
        })
    }
    /// Bulk client render data. Unlike EntityRef, this snapshot expires independently of handles.
    pub(super) fn native_client_entities(&self) -> Vec<NativeClientEntity> {
        if !self.native_window() || self.remote.is_none() {
            return Vec::new();
        }
        self.mobs
            .iter()
            .filter(|m| !m.removed)
            .map(|m| {
                let b = m.bounds();
                let (lo, hi) = (b.min_position(), b.max_position());
                NativeClientEntity {
                    id: m.id,
                    player_uuid: self.remote_player_uuid(m.id),
                    player_name: self.remote_player_name(m.id).map(str::to_owned),
                    position: [m.position.x, m.position.y, m.position.z],
                    previous_position: [m.previous.x, m.previous.y, m.previous.z],
                    bounds: [[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]],
                    height: m.height(),
                    head_yaw: m.head_yaw,
                    head_pitch: m.head_pitch,
                    alive: m.alive(),
                    invisible: m.invisible || m.has_effect("minecraft:invisibility"),
                    kind: m.kind.id.to_string(),
                    yaw: m.yaw,
                    baby: m.is_baby(),
                    spectator: m.extra.spectator,
                    texture: Some(horizon_client::mobs::render::body(m).1)
                        .filter(|t| !t.is_empty())
                        .map(Cow::into_owned),
                }
            })
            .collect()
    }
    /// The dropped items the server shows this window, by the same rules as
    /// `native_client_entities`. API @API:client_entity_list@.
    pub(super) fn native_client_items(&self) -> Vec<NativeClientItem> {
        if !self.native_window() || self.remote.is_none() {
            return Vec::new();
        }
        self.dropped
            .iter()
            .filter(|e| e.stack.count > 0)
            .map(|e| NativeClientItem {
                id: e.id,
                stack: e.stack.clone(),
                position: [e.position.x, e.position.y, e.position.z],
                previous_position: [e.previous.x, e.previous.y, e.previous.z],
            })
            .collect()
    }
    /// The pixels of a texture as this window draws it, by the name
    /// `NativeClientEntity::texture` and `NativeClientProfile::skin` give: a
    /// downloaded skin while the client holds it, else
    /// `native_client_resources().texture(name)`. Decodes on each call; None
    /// on a server or for a texture it can't read. API
    /// @API:client_entity_list@.
    pub(super) fn native_client_texture(&self, texture: &str) -> Option<TexturePixels> {
        if !self.native_window() {
            return None;
        }
        if let Some(rgba) = self.skin_downloads.borrow().image(texture) {
            return Some(TexturePixels {
                width: SKIN_SIDE,
                height: SKIN_SIDE,
                rgba: rgba.to_vec(),
            });
        }
        self.native_client_resources().texture(texture)
    }
    /// Requested model heads at a finite partial tick in 0..=1; at most 4096 IDs.
    /// Name bounds are vanilla's, without dispatching name hooks recursively.
    pub(super) fn native_client_heads(&self, ids: &[u32], partial: f64) -> Vec<NativeClientHead> {
        if !self.native_window()
            || !partial.is_finite()
            || !(0.0..=1.0).contains(&partial)
            || ids.len() > 4096
        {
            return Vec::new();
        }
        let ids: std::collections::HashSet<_> = ids.iter().copied().collect();
        let key = |p: glam::DVec3| p.to_array().map(f64::to_bits);
        let widths = if self.ui.hide_gui {
            Default::default()
        } else {
            self.client_name_tag_widths(glam::DVec3::from_array(self.view.position), partial)
        };
        self.mobs
            .iter()
            .filter(|m| !m.removed && ids.contains(&m.id))
            .map(|m| {
                let p = m.previous.lerp(m.position, partial);
                let at = glam::DVec3::new(p.x, p.y, p.z);
                let name_at = super::names::name_tag_at(m, partial);
                NativeClientHead {
                    id: m.id,
                    origin: at.to_array(),
                    transform: horizon_client::mobs::render::part_transform(
                        m,
                        partial as f32,
                        at,
                        self.res.mob_data(),
                        "head",
                    )
                    .map(|m| m.to_cols_array()),
                    outer_layer_padding: if m.extra.head.is_some() {
                        1.5 / 16.0
                    } else if m.skin_3d.is_some() {
                        1.25 / 16.0
                    } else {
                        0.5 / 16.0
                    },
                    height: m.height(),
                    name_anchor: name_at.to_array(),
                    name_half_width: widths.get(&key(name_at)).copied().unwrap_or(0.0),
                }
            })
            .collect()
    }
    /// Player-list profiles with skin/cosmetic resolution through the client's normal cache.
    pub(super) fn native_client_profiles(&mut self) -> Vec<NativeClientProfile> {
        if !self.native_window() {
            return Vec::new();
        }
        let Some((players, _, _)) = self.remote_tab_players() else {
            return Vec::new();
        };
        players
            .into_iter()
            .map(|p| NativeClientProfile {
                uuid: p.uuid,
                name: p.name,
                skin: self.face_skin(p.uuid, &p.skin),
            })
            .collect()
    }
    /// Whether a finite segment of at most 1024 blocks meets no visual block shape.
    /// Glass is transparent; partial blocks use their visual shapes. Unloaded paths refuse.
    pub(super) fn native_client_visual_clear(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        if !self.native_window()
            || !from.iter().chain(&to).all(|v| v.is_finite())
            || from
                .iter()
                .zip(to)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                > 1024.0f64.powi(2)
        {
            return false;
        }
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        horizon_client::mobs::visual_clip(
            self.res.facts(),
            self.collision,
            &mut scene,
            &|x, y, z| self.world.block(x, y, z),
            Vec3::new(from[0], from[1], from[2]),
            Vec3::new(to[0], to[1], to[2]),
        ) == Some(None)
    }
    /// Loaded block's full-collision and render-occlusion facts, in that order.
    pub(super) fn native_client_block_obstruction(&self, pos: [i32; 3]) -> Option<(bool, bool)> {
        if !self.native_window() || !self.world.loaded(pos[0], pos[2]) {
            return None;
        }
        let f = self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]));
        Some((f.full_collision, f.can_occlude))
    }
    /// Paint GUI extraction calls through this window's atlas, including player faces.
    pub(super) fn native_gui_paint(&self, batch: &mut HudBatch, draws: &[GuiDraw]) {
        horizon_client::hud_layout::paint(batch, &self.gui.atlas, draws);
    }
    /// Vanilla's nine-slice widget rendering. `color` is ARGB, as `fill` uses.
    pub(super) fn native_gui_nine_slice(
        &self,
        batch: &mut HudBatch,
        sprite: &str,
        rect: [f32; 4],
        color: u32,
    ) {
        let [a, r, g, b] = color.to_be_bytes();
        horizon_client::ui::nine_slice(
            batch,
            &self.gui.atlas,
            sprite,
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            rgba(r, g, b, a),
        );
    }
    /// A vanilla tooltip at the cursor, in GUI pixels.
    pub(super) fn native_gui_tooltip(&self, batch: &mut HudBatch, lines: &[Text], at: [f32; 2]) {
        horizon_client::ui::tooltip(batch, &self.gui.atlas, lines, (at[0], at[1]));
    }
    /// Save a mod binding, or restore its default with None. No write when unchanged.
    pub(super) fn native_set_mod_key(
        &mut self,
        mod_id: &str,
        name: &str,
        key: Option<&str>,
    ) -> bool {
        if !self.native_window()
            || mod_id.is_empty()
            || name.is_empty()
            || mod_id.len() > 256
            || name.len() > 256
            || key.is_some_and(|k| k.len() > 512)
        {
            return false;
        }
        let old = self
            .settings
            .mods
            .keys
            .get(mod_id)
            .and_then(|keys| keys.get(name))
            .map(String::as_str);
        if old == key {
            return false;
        }
        if let Some(key) = key {
            self.settings
                .mods
                .keys
                .entry(mod_id.into())
                .or_default()
                .insert(name.into(), key.into());
        } else if let Some(keys) = self.settings.mods.keys.get_mut(mod_id) {
            keys.remove(name);
        }
        self.settings.save(&Game::options_path());
        true
    }
    /// Set this client's action bar, with the normal timer; no server command or packet.
    pub(super) fn native_client_action_bar(&mut self, text: Text, animate: bool) {
        if self.native_window() {
            self.titles.set_overlay(text, animate);
        }
    }
    /// Draw an item stack with its default decorations at `at` in GUI units.
    pub(super) fn native_gui_draw_item(
        &self,
        batch: &mut HudBatch,
        stack: &ItemStack,
        at: [f32; 2],
    ) {
        self.draw_item(batch, stack, at[0], at[1]);
    }
    /// Draw an item stack with custom count/decoration text (or None) at `at` in GUI units.
    pub(super) fn native_gui_draw_item_with(
        &self,
        batch: &mut HudBatch,
        stack: &ItemStack,
        at: [f32; 2],
        text: Option<&str>,
    ) {
        self.draw_item_model(batch, stack, at[0], at[1]);
        self.draw_item_decorations(batch, stack, at[0], at[1], text.map(str::to_owned));
    }
    /// Whether Shift is pressed (live modifier state).
    pub(super) fn native_has_shift_down(&self) -> bool {
        self.has_shift_down()
    }
    /// Whether Control is pressed (live modifier state).
    pub(super) fn native_has_control_down(&self) -> bool {
        self.has_control_down()
    }
    /// Whether Alt is pressed (live modifier state).
    pub(super) fn native_has_alt_down(&self) -> bool {
        self.has_alt_down()
    }
    /// Whether a specific key is currently held down.
    pub(super) fn native_is_key_down(&self, key: KeyCode) -> bool {
        self.physical_keys.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_friends::code::Code;

    #[test]
    fn a_world_is_its_save_in_single_player_and_its_address_on_a_server() {
        assert_eq!(
            NativeWorldId::of(Some("New World"), "local"),
            NativeWorldId::Singleplayer("New World".into())
        );
        assert_eq!(
            NativeWorldId::of(None, "mc.example.com:25565"),
            NativeWorldId::Server("mc.example.com:25565".into())
        );
    }

    #[test]
    fn friends_worlds_are_told_apart_by_their_hosts_never_by_their_secrets() {
        const PORT: u16 = 50000;
        const RELAY: &str = "relay.example.com:443";
        // A short code: its nameplate, then its secret.
        let short = |nameplate: &str, secret: &str| Code {
            nameplate: nameplate.into(),
            secret: secret.into(),
        };
        let invite = |secret: u8, host: &str, relay: Option<Code>| {
            let code = invite::Invite {
                secret: [secret; invite::SECRET],
                port: PORT,
                endpoints: vec![format!("{host}:{PORT}").parse().unwrap()],
                relay: relay.map(|code| invite::RelayRef {
                    address: RELAY.into(),
                    code,
                }),
            }
            .encode();
            NativeWorldId::of(None, &format!("{FRIEND}{code}"))
        };
        assert_ne!(
            invite(1, "192.168.1.5", None),
            invite(1, "192.168.1.6", None),
            "two friends"
        );
        assert_ne!(
            invite(1, "192.168.1.5", Some(short("ABCD", "WXYZ"))),
            invite(1, "192.168.1.5", Some(short("EFGH", "WXYZ"))),
            "two friends on other networks behind the same private address"
        );
        assert_eq!(
            invite(1, "192.168.1.5", Some(short("ABCD", "WXYZ"))),
            invite(2, "192.168.1.5", Some(short("ABCD", "QRST"))),
            "the invite's and the relay code's secrets take no part"
        );
        let typed = |code: Code| NativeWorldId::of(None, &format!("{FRIEND}{code}"));
        assert_eq!(typed(short("ABCD", "WXYZ")), typed(short("ABCD", "QRST")));
        assert_eq!(
            typed(short("ABCD", "WXYZ")),
            NativeWorldId::Server(format!("{FRIEND}ABCD"))
        );
    }

    #[test]
    fn a_hud_tells_chat_and_death_from_other_screens() {
        assert_eq!(
            NativeScreenKind::of(Screen::Playing, false),
            NativeScreenKind::Playing
        );
        assert_eq!(
            NativeScreenKind::of(Screen::Chat, false),
            NativeScreenKind::Chat
        );
        assert_eq!(
            NativeScreenKind::of(Screen::Dead, false),
            NativeScreenKind::Death
        );
        assert_eq!(
            NativeScreenKind::of(Screen::Inventory, false),
            NativeScreenKind::Other
        );
        assert_eq!(
            NativeScreenKind::of(Screen::Chat, true),
            NativeScreenKind::Other,
            "a sign or dialog over chat covers the world"
        );
    }

    #[test]
    fn an_entity_is_drawn_between_its_last_two_tick_positions() {
        let entity = NativeClientEntity {
            previous_position: [0.0, 64.0, -2.0],
            position: [2.0, 65.0, 2.0],
            ..Default::default()
        };
        assert_eq!(entity.position_at(0.0), [0.0, 64.0, -2.0]);
        assert_eq!(entity.position_at(0.25), [0.5, 64.25, -1.0]);
        assert_eq!(entity.position_at(1.0), [2.0, 65.0, 2.0]);
    }
}

/// A window joined to a server through memory, fed the server's packets as
/// it would receive them: what a client mod lists through the mod `Host`.
#[cfg(test)]
mod client_tests {
    use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
    use horizon_client::mod_api::Host;
    use horizon_protocol::entity_data::{Value, serializer};
    use horizon_protocol::item::Stack;
    use horizon_protocol::packets::play::{self, Clientbound as C};

    /// `AgeableMob.DATA_BABY_ID` and `ItemEntity.DATA_ITEM`, the entity data
    /// a calf and a dropped item come with.
    const DATA_BABY_ID: u8 = 16;
    const DATA_ITEM: u8 = 8;
    /// Vanilla's `zombie.png`, like every skin, is 64x64 RGBA.
    const TEXTURE_SIDE: u32 = 64;
    const RGBA_BYTES: usize = 4;
    /// The network ids the server gives the window's player and the entities.
    const OWN_PLAYER: i32 = 7;
    const ZOMBIE: i32 = 20;
    const CALF: i32 = 21;
    const DIAMONDS: i32 = 22;
    const ALEX: i32 = 23;
    /// `UUID.hashCode` 2: `DefaultPlayerSkin.get` picks the third default
    /// skin, slim Efe.
    const ALEX_UUID: u128 = 2;
    /// `ClientboundPlayerInfoUpdatePacket`'s `ADD_PLAYER` and
    /// `UPDATE_GAME_MODE` actions, and `GameType.SPECTATOR`.
    const ADD_PLAYER: u8 = 1;
    const UPDATE_GAME_MODE: u8 = 4;
    const SPECTATOR: i32 = 3;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_client_lists_the_mobs_and_items_the_server_shows_it() {
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
        let (pipe, _server) = horizon_protocol::connection::Pipe::pair();
        game.connect_local(pipe, "Steve").unwrap();
        let builtin = game.remote.as_ref().unwrap().builtin.clone();
        let id = |registry: &str, name: &str| builtin.get(registry).unwrap().id(name).unwrap();
        let entity = |entity: i32, kind: &str, position: [f64; 3]| play::AddEntity {
            id: entity,
            uuid: entity as u128,
            kind: id("minecraft:entity_type", kind),
            position,
            velocity: [0.0; 3],
            pitch: 0,
            yaw: 64,
            head_yaw: 64,
            data: 0,
        };
        let add =
            |id: i32, kind: &str, position: [f64; 3]| C::AddEntity(entity(id, kind, position));
        let data = |entity: i32, value: (u8, i32, Value)| {
            C::SetEntityData(play::SetEntityData {
                id: entity,
                values: vec![value],
            })
        };
        game.handle_packet(C::Login(play::Login {
            player_id: OWN_PLAYER,
            hardcore: false,
            levels: vec!["minecraft:overworld".into()],
            max_players: 20,
            chunk_radius: 8,
            simulation_distance: 8,
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: play::SpawnInfo {
                dimension_type: 0,
                dimension: "minecraft:overworld".into(),
                seed: 1,
                game_type: 0,
                previous_game_type: None,
                debug: false,
                flat: false,
                last_death: None,
                portal_cooldown: 0,
                sea_level: 63,
            },
            online_mode: false,
            enforces_secure_chat: false,
        }))
        .unwrap();
        game.handle_packet(C::PlayerPosition(play::PlayerPosition {
            teleport: 1,
            position: [0.5, 65.0, 0.5],
            delta: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            relatives: 0,
        }))
        .unwrap();

        // What the server sends for `/summon zombie`, and for a calf.
        game.handle_packet(add(ZOMBIE, "minecraft:zombie", [1.5, 64.0, 2.5]))
            .unwrap();
        game.handle_packet(add(CALF, "minecraft:cow", [-3.5, 64.0, 0.5]))
            .unwrap();
        game.handle_packet(data(
            CALF,
            (DATA_BABY_ID, serializer::BOOLEAN, Value::Bool(true)),
        ))
        .unwrap();
        let entities = game.client_entities();
        let listed = |id: i32| entities.iter().find(|e| e.id == id as u32);
        assert_eq!(entities.len(), 2);
        assert!(listed(OWN_PLAYER).is_none(), "the window's own player");
        let zombie = listed(ZOMBIE).unwrap();
        assert_eq!(zombie.kind, "minecraft:zombie");
        assert_eq!(zombie.position_at(0.5), [1.5, 64.0, 2.5]);
        assert_eq!(zombie.yaw, 90.0);
        assert!(!zombie.baby);
        assert_eq!(zombie.texture.as_deref(), Some("entity/zombie/zombie"));
        let calf = listed(CALF).unwrap();
        assert!(calf.baby);
        assert_eq!(
            calf.texture.as_deref(),
            Some("entity/cow/cow_temperate_baby")
        );

        // A dropped item is listed once its stack arrives.
        game.handle_packet(add(DIAMONDS, "minecraft:item", [0.5, 65.0, 4.5]))
            .unwrap();
        assert!(game.client_items().is_empty());
        let diamonds = Stack::new(id("minecraft:item", "minecraft:diamond"), 3);
        game.handle_packet(data(
            DIAMONDS,
            (
                DATA_ITEM,
                serializer::ITEM_STACK,
                Value::Item(Some(diamonds)),
            ),
        ))
        .unwrap();
        let items = game.client_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, DIAMONDS as u32);
        assert_eq!(
            (&*items[0].stack.item, items[0].stack.count),
            ("minecraft:diamond", 3)
        );
        assert_eq!(items[0].position_at(1.0), [0.5, 65.0, 4.5]);

        // Another player: its list entry, a spectator, then its entity. The
        // frame's skin refresh (`draw_tab_list`) and a client tick
        // (`isSpectator` from the list) follow.
        game.handle_packet(C::PlayerInfoUpdate(play::PlayerInfoUpdate {
            actions: ADD_PLAYER | UPDATE_GAME_MODE,
            entries: vec![play::PlayerInfo {
                uuid: ALEX_UUID,
                name: Some("Alex".into()),
                game_mode: Some(SPECTATOR),
                ..Default::default()
            }],
        }))
        .unwrap();
        game.handle_packet(C::AddEntity(play::AddEntity {
            uuid: ALEX_UUID,
            ..entity(ALEX, "minecraft:player", [3.5, 64.0, -1.5])
        }))
        .unwrap();
        game.update_remote_skins();
        game.tick_remote().unwrap();
        let alex = game
            .client_entities()
            .into_iter()
            .find(|e| e.id == ALEX as u32)
            .unwrap();
        assert_eq!(alex.player_uuid, Some(ALEX_UUID));
        assert_eq!(alex.player_name.as_deref(), Some("Alex"));
        assert!(alex.spectator);
        assert_eq!(alex.texture.as_deref(), Some("entity/player/slim/efe"));

        let pixels = (TEXTURE_SIDE * TEXTURE_SIDE) as usize * RGBA_BYTES;
        let face = game.client_texture("entity/zombie/zombie").unwrap();
        assert_eq!((face.width, face.height), (TEXTURE_SIDE, TEXTURE_SIDE));
        assert_eq!(face.rgba.len(), pixels);
        // A downloaded skin comes from memory.
        let skin = vec![7; pixels];
        game.skin_downloads
            .borrow_mut()
            .remember_image("skins/0123abcd", &skin);
        assert_eq!(game.client_texture("skins/0123abcd").unwrap().rgba, skin);
        assert!(
            game.client_texture("entity/zombie/no_such_zombie")
                .is_none()
        );
        let skin = game.client_texture("entity/player/slim/efe").unwrap();
        assert_eq!((skin.width, skin.height), (TEXTURE_SIDE, TEXTURE_SIDE));

        // A name that climbs out of the packs is refused, even where a picture
        // is there.
        let packs = std::env::temp_dir().join(format!("horizon-texture-{}", std::process::id()));
        let textures = packs.join("assets/minecraft/textures");
        std::fs::create_dir_all(&textures).unwrap();
        let png =
            std::fs::read(root.join("assets/minecraft/textures/entity/zombie/zombie.png")).unwrap();
        std::fs::write(textures.join("inside.png"), &png).unwrap();
        std::fs::write(packs.join("outside.png"), &png).unwrap();
        game.options.resources = packs.clone();
        assert!(game.client_texture("inside").is_some());
        assert!(game.client_texture("../../../outside").is_none());
        std::fs::remove_dir_all(&packs).unwrap();
    }
}
