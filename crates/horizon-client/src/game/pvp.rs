//! The PvP & HUD page's options in the game (`horizon_client::pvp`, `docs/pvp.md`). Each
//! does nothing while it is off.
use super::{Dim, Game};
use horizon_client::{
    extras::Corner,
    extras_hud::Corners,
    hud::{HudBatch, rgba},
    mesher::Vertex,
    pvp::{Colour, CrosshairCache},
    pvp_hud::{self, Combo, Durability, Icon, Key, Line, Widget},
    text::Text,
};
use std::sync::Arc;
use winit::event::MouseButton;

#[derive(Default)]
pub(super) struct PvpState {
    crosshair: CrosshairCache,
    /// Each armour slot's item, damage and maximum damage, and whether durability is in percent.
    armour: Widget<([Option<(Arc<str>, u32, u32)>; 4], bool)>,
    /// Each effect: id, amplifier, whole seconds left (-1 infinite).
    effects: Widget<Vec<(&'static str, i32, i32)>>,
    /// The counted item's slot and the total.
    held: Widget<Option<(usize, Arc<str>, u32)>>,
    ping: Widget<Option<i32>>,
    /// The block position and the direction faced.
    coords: Widget<Option<([i32; 3], &'static str)>>,
    server: Widget<Option<String>>,
    combo: Combo,
    combo_count: Widget<u32>,
    fps: Widget<u32>,
    /// Left and right presses, counted while the CPS or keystrokes widget is on.
    clicks: [pvp_hud::Clicks; 2],
    cps: Widget<(u32, u32)>,
    keys: Keystrokes,
    /// The local hour and minute, and when they were read.
    clock: Widget<(u8, u8)>,
    clock_read: f64,
    reach: pvp_hud::Reach,
    reach_shown: Widget<Option<u32>>,
    speed: pvp_hud::Speed,
    speed_shown: Widget<u32>,
    /// Each widget drawn last frame and its box on screen, for the HUD editor.
    pub(super) boxes: Vec<(pvp_hud::WidgetId, [f32; 4])>,
    /// The HUD editor, while it is open.
    pub(super) editor: Option<super::hud_editor::HudEditor>,
    /// The open chat's box holds a chat search's query (Ctrl+F).
    pub(super) chat_search: bool,
    /// What the chat box held when the search borrowed it.
    pub(super) chat_search_aside: String,
}
/// `GuiGraphicsExtractor`'s grey for a detail line (an effect's time left).
const DETAIL: u32 = 0xFF7F_7F7F;
/// How the widgets look: their boxes' colour (none without boxes), text colour and shadow.
struct Style {
    background: Option<u32>,
    text: u32,
    shadow: bool,
}
/// A held key's box in the keystrokes widget, and its text.
const KEY_HELD: u32 = 0xB4FF_FFFF;
const KEY_HELD_TEXT: u32 = 0xFF00_0000;

/// The keystrokes widget: each key's label, made again only when its binding changes, which
/// keys are held, and the clicks per second under the mouse buttons.
#[derive(Default)]
struct Keystrokes {
    bound: [String; 7],
    labels: Vec<(Text, f32)>,
    held: [bool; 7],
    cps: Widget<(u32, u32)>,
}
const ARROWS: [&str; 3] = [
    "minecraft:arrow",
    "minecraft:spectral_arrow",
    "minecraft:tipped_arrow",
];

impl Game<'_> {
    /// Whether the custom crosshair replaces vanilla's sprite.
    pub(super) fn custom_crosshair(&self) -> bool {
        self.settings.extras.pvp.crosshair.custom
    }
    /// The custom crosshair, centred where vanilla's 15x15 sprite is (`Hud.extractCrosshair`):
    /// its outline in black, then the shape, inverting what is under it when its colour is
    /// `Inverted`. The runs are cached, so a frame only adds their quads.
    pub(super) fn draw_custom_crosshair(&mut self, batch: &mut HudBatch, w: i32, h: i32) {
        let mut c = self.settings.extras.pvp.crosshair;
        if let Some(hover) = c.hover
            && self.crosshair_living() == Some(true)
        {
            c.colour = hover;
        }
        let (cx, cy) = ((w - 15) / 2 + 7, (h - 15) / 2 + 7);
        let atlas = &self.gui.atlas;
        let (shape, rim) = self.pvp.crosshair.runs(&c);
        let fill = |batch: &mut HudBatch, &(x0, y, x1): &(i32, i32, i32), colour: u32| {
            let (x, y) = ((cx + x0) as f32, (cy + y) as f32);
            batch.fill(atlas, x, y, (x1 - x0) as f32, 1.0, colour);
        };
        for run in rim {
            fill(batch, run, rgba(0, 0, 0, 255));
        }
        let start = batch.vertices.len();
        let [a, r, g, b] = c.colour.argb().to_be_bytes();
        for run in shape {
            fill(batch, run, rgba(r, g, b, a));
        }
        if c.colour == Colour::Inverted {
            batch.invert_since(start);
        }
    }
    /// The block outline's fill: each box of the targeted block's shape (`getShape`, the boxes
    /// the outline's edges come from) as translucent faces drawn after translucent terrain,
    /// pushed out a little so they don't fight the block's own faces. Only while the outline
    /// shows (`outline` is its edges).
    pub(super) fn outline_fill(
        &self,
        outline: &[[f64; 6]],
        camera: glam::DVec3,
        out: &mut Vec<Vertex>,
    ) {
        let Some(argb) = self.settings.extras.pvp.outline.fill() else {
            return;
        };
        let Some(hit) = self.pick().filter(|_| !outline.is_empty()) else {
            return;
        };
        let [x, y, z] = hit.pos;
        let state = self.world.block(x, y, z);
        let facts = self.res.facts().get(state);
        let offset = match facts.outline_offset {
            true => self.pack.context.tables.offset(state, x, z),
            false => None,
        }
        .unwrap_or([0.0; 3]);
        let base = [f64::from(x), f64::from(y), f64::from(z)];
        let context = self.context_outline(hit.pos, state);
        let boxes = context.as_slice().iter().chain(match context {
            Some(_) => &[][..],
            None => &facts.outline[..],
        });
        let uv = horizon_client::gfx::lightning::white_uv(&self.gui.atlas.font.white());
        let [a, r, g, b] = argb.to_be_bytes();
        let color = u32::from_le_bytes([r, g, b, a]);
        const OUT: f64 = 0.002;
        for shape in boxes {
            let corner = |i: usize, high: bool| {
                let v = if high {
                    shape[i + 3] + OUT
                } else {
                    shape[i] - OUT
                };
                (base[i] + offset[i] + v - camera[i]) as f32
            };
            // Each face: the axis it faces along, its side, and its corners' other two axes.
            for axis in 0..3 {
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                for high in [false, true] {
                    for (hu, hv) in [(false, false), (true, false), (true, true), (false, true)] {
                        let mut pos = [0.0; 3];
                        pos[axis] = corner(axis, high);
                        pos[u] = corner(u, hu);
                        pos[v] = corner(v, hv);
                        out.push(Vertex {
                            pos,
                            uv,
                            layer: horizon_client::gfx::TRANSLUCENT_PARTICLE_OVERLAY,
                            color,
                            light: 240 | 240 << 8,
                            normal: [0; 4],
                            glint_uv: [0.0; 2],
                        });
                    }
                }
            }
        }
    }
    /// Once per frame: the options that live outside the game's state.
    pub(super) fn pvp_frame(&mut self) {
        let tweaks = self.settings.extras.pvp.tweaks;
        horizon_client::enchant::set_numeric_levels(tweaks.numeric_enchants);
    }
    /// Whether this window may draw PvP & HUD's own time and weather: only a client of a server
    /// (single player's included) draws them, never the server that ticks the world.
    fn own_view(&self) -> bool {
        self.remote.is_some() && self.dim == Dim::Overworld
    }
    /// The day time the frame draws (sky, sun, moon, the sky's light): PvP & HUD's time of day,
    /// else the world's. The world's own `day_time` and everything that plays by it stay.
    pub(super) fn view_day_time(&self) -> i64 {
        let fixed = self.settings.extras.pvp.tweaks.time;
        horizon_client::pvp::view_day_time(self.world.day_time, fixed.filter(|_| self.own_view()))
    }
    /// The rain and thunder levels the frame draws (rain, the sky's colour and light, fog): PvP &
    /// HUD's weather, else the world's. Gameplay (riptide, mobs, crops) keeps the world's.
    pub(super) fn view_weather(&self, partial: f32) -> (f32, f32) {
        match self.settings.extras.pvp.tweaks.weather {
            Some(w) if self.own_view() && self.can_have_weather() => w.levels(),
            _ => self.weather_levels(partial),
        }
    }
    /// This player attacked entity `id`, `distance` from the eyes to where the crosshair's
    /// ray met its box (the combo counter waits for the server's report; the reach widget
    /// shows the distance).
    pub(super) fn pvp_attacked(&mut self, id: i32, distance: f64) {
        let now = self.started.elapsed().as_secs_f64();
        self.pvp.combo.attacked(id, now);
        self.pvp.reach.hit(distance, now);
    }
    /// A mouse press, counted for clicks per second while playing with a widget that shows them.
    pub(super) fn pvp_click(&mut self, button: MouseButton) {
        let hud = &self.settings.extras.pvp.hud;
        if !(hud.cps || hud.keys) || self.screen != super::Screen::Playing {
            return;
        }
        let i = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            _ => return,
        };
        let now = self.started.elapsed().as_secs_f64();
        self.pvp.clicks[i].press(now);
    }
    /// Whether the key `mapping` is bound to is held: a keyboard key or a mouse button.
    fn binding_held(&self, mapping: &str) -> bool {
        let bound = self.settings.keys.get(mapping);
        if let Some(button) = horizon_client::keybinds::mouse_button(bound) {
            return self.physical_buttons.contains(&button);
        }
        self.mapped_key_down(mapping)
    }
    /// The server reports entity `id` hurt; `own` when it is this player,
    /// `by_me` whether this player was the damage's direct entity (if named).
    pub(super) fn pvp_hurt(&mut self, id: i32, own: bool, by_me: Option<bool>) {
        if own {
            self.pvp.combo.broken();
        } else {
            let now = self.started.elapsed().as_secs_f64();
            self.pvp.combo.hurt(id, by_me, now);
        }
    }
    /// Whether the effects widget replaces vanilla's effect icons (`Hud.extractEffects`).
    pub(super) fn pvp_effects_shown(&self) -> bool {
        self.settings.extras.pvp.hud.effects
    }
    /// The HUD widgets that are on, after the minimap and before mods' boxes. Each rebuilds its
    /// lines only when what it shows changed.
    pub(super) fn draw_pvp_hud(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        let hud = self.settings.extras.pvp.hud;
        if self.player.is_none() || !hud.any() {
            // The editor frames only what was drawn.
            self.pvp.boxes.clear();
            return;
        }
        // What each widget shows, read before its cache is borrowed.
        let armour: [Option<(Arc<str>, u32, u32)>; 4] = std::array::from_fn(|i| {
            let s = self.armor[i].as_ref()?;
            let max = self.items().get(&s.item).map_or(0, |i| i.max_damage);
            Some((s.item.clone(), s.damage, max))
        });
        let held = if hud.held { self.held_count() } else { None };
        let multiplayer = self.remote.is_some() && !self.has_singleplayer_server();
        let ping = self.own_latency().filter(|_| multiplayer);
        let coords = self.player.as_ref().filter(|_| !self.reduced_debug_info());
        let coords = coords.map(|p| {
            let at = [p.position.x, p.position.y, p.position.z].map(|v| v.floor() as i32);
            (at, pvp_hud::facing(self.yaw))
        });
        // The address is copied only when it changed (`server_address` borrows the whole game).
        let server = self.server_address().filter(|_| multiplayer);
        let server = (hud.server && self.pvp.server.next.as_deref() != server)
            .then(|| server.map(str::to_owned));
        let now = self.started.elapsed().as_secs_f64();
        let fps = self.fps.round() as u32;
        let position = self.player.as_ref().map(|p| (p.position.x, p.position.z));
        if hud.keys {
            let held = Key::ALL.map(|k| self.binding_held(k.mapping()));
            self.pvp.keys.held = held;
            self.keystroke_labels();
        }
        let atlas = &self.gui.atlas;
        let width = |s: &str| HudBatch::text_width(atlas, s);
        let state = &mut self.pvp;
        if hud.armour {
            let w = &mut state.armour;
            w.next = (armour, hud.armour_percent);
            w.refresh(|(pieces, percent), lines| {
                let mode = if *percent {
                    Durability::Percent
                } else {
                    Durability::Number
                };
                // `Game::armor` holds the head first: it is drawn at the top.
                for (i, piece) in pieces.iter().enumerate() {
                    let Some((_, damage, max)) = piece else {
                        continue;
                    };
                    let text = pvp_hud::durability(*damage, *max, mode).unwrap_or_default();
                    let mut line = Line::new(Icon::Armour(i as u8), text, &width);
                    line.colour = pvp_hud::durability_colour(*damage, *max);
                    lines.push(line);
                }
            });
        } else {
            state.armour.clear();
        }
        if hud.effects {
            let w = &mut state.effects;
            let rate = self.clock.rate();
            w.next.clear();
            w.next.extend(self.potions.effects.list.iter().map(|e| {
                let secs = if e.duration == -1 {
                    -1
                } else {
                    (e.duration as f32 / rate) as i32
                };
                (e.id, e.amplifier, secs)
            }));
            let names = &self.pack.names;
            w.refresh(|effects, lines| {
                for &(id, amplifier, secs) in effects {
                    let key = horizon_client_core::potions::description_id(id);
                    let mut name = names.display_get(&key).unwrap_or_else(|| id.to_owned());
                    // EffectsInInventory.getEffectName: the level from II to X.
                    if (1..=9).contains(&amplifier) {
                        let level = amplifier + 1;
                        let suffix = names
                            .display_get(&format!("enchantment.level.{level}"))
                            .unwrap_or_else(|| horizon_client::enchant::level_name(level as u32));
                        name = format!("{name} {suffix}");
                    }
                    let time = if secs == -1 {
                        names
                            .display_get("effect.duration.infinite")
                            .unwrap_or_else(|| "∞".into())
                    } else {
                        horizon_client::brewing::format_tick_duration(secs, 1.0)
                    };
                    let sprite = match id.split_once(':') {
                        Some((ns, path)) => format!("{ns}:mob_effect/{path}"),
                        None => format!("mob_effect/{id}"),
                    };
                    let line = Line::new(Icon::Sprite(sprite), name, &width);
                    lines.push(line.with_detail(time, &width));
                }
            });
        } else {
            state.effects.clear();
        }
        if hud.held {
            let w = &mut state.held;
            w.next = held;
            w.refresh(|held, lines| {
                if let Some((slot, _, total)) = held {
                    lines.push(Line::new(Icon::Slot(*slot), total.to_string(), &width));
                }
            });
        } else {
            state.held.clear();
        }
        let text = |key: &str, args: &[&dyn std::fmt::Display]| {
            let line = horizon_client::lang::tr(key, args);
            Line::new(Icon::None, line, &width)
        };
        if hud.ping {
            state.ping.next = ping;
            state.ping.refresh(|ping, lines| {
                lines.extend(ping.map(|ms| text("horizon.pvp.hud.ping", &[&ms])));
            });
        } else {
            state.ping.clear();
        }
        if hud.coords {
            state.coords.next = coords;
            state.coords.refresh(|coords, lines| {
                if let Some(([x, y, z], facing)) = coords {
                    lines.push(text("horizon.pvp.hud.coords", &[x, y, z]));
                    let key = format!("horizon.pvp.hud.direction.{facing}");
                    let name = horizon_client::lang::get_or(&key, facing);
                    lines.push(text("horizon.pvp.hud.facing", &[&name]));
                }
            });
        } else {
            state.coords.clear();
        }
        if hud.server {
            // A new String only when the address changes.
            let w = &mut state.server;
            if let Some(address) = server {
                w.next = address;
            }
            w.refresh(|address, lines| {
                if let Some(address) = address {
                    let shown = horizon_client::net::shown_address(address);
                    lines.push(Line::new(Icon::None, shown, &width));
                }
            });
        } else {
            state.server.clear();
        }
        if hud.combo {
            state.combo_count.next = state.combo.count(now);
            state.combo_count.refresh(|count, lines| {
                lines.push(text("horizon.pvp.hud.combo", &[count]));
            });
        } else {
            state.combo_count.clear();
        }
        let cps = if hud.cps || hud.keys {
            [0, 1].map(|i| state.clicks[i].per_second(now))
        } else {
            [0; 2]
        };
        if hud.fps {
            state.fps.next = fps;
            state
                .fps
                .refresh(|fps, lines| lines.push(text("horizon.pvp.hud.fps", &[fps])));
        } else {
            state.fps.clear();
        }
        if hud.cps {
            state.cps.next = (cps[0], cps[1]);
            state.cps.refresh(|(l, r), lines| {
                lines.push(text("horizon.pvp.hud.cps", &[l, r]));
            });
        } else {
            state.cps.clear();
        }
        if hud.keys {
            state.keys.cps.next = (cps[0], cps[1]);
            state.keys.cps.refresh(|(l, r), lines| {
                for n in [l, r] {
                    lines.push(text("horizon.pvp.hud.keys.cps", &[n]));
                }
            });
        }
        if hud.clock {
            // The time is read once a second, not every frame.
            if now - state.clock_read >= 1.0 || state.clock.lines.is_empty() {
                use chrono::Timelike;
                let t = chrono::Local::now();
                state.clock.next = (t.hour() as u8, t.minute() as u8);
                state.clock_read = now;
            }
            state.clock.refresh(|&(h, m), lines| {
                lines.push(Line::new(Icon::None, pvp_hud::clock(h, m), &width));
            });
        } else {
            state.clock.clear();
        }
        if hud.reach {
            state.reach_shown.next = state.reach.hundredths(now);
            state.reach_shown.refresh(|reach, lines| {
                if let Some(n) = reach {
                    lines.push(text("horizon.pvp.hud.reach", &[&pvp_hud::hundredths(*n)]));
                }
            });
        } else {
            state.reach_shown.clear();
        }
        if hud.speed
            && let Some((x, z)) = position
        {
            state.speed_shown.next = state.speed.sample(x, z, now);
            state.speed_shown.refresh(|n, lines| {
                lines.push(text("horizon.pvp.hud.speed", &[&pvp_hud::tenths(*n)]));
            });
        } else if !hud.speed {
            state.speed_shown.clear();
        }
        let style = Style {
            background: hud.background.then(|| hud.background_argb()),
            text: hud.text.argb(),
            shadow: hud.shadow,
        };
        let layout = self.settings.extras.pvp.layout;
        let screen = (batch.width, batch.height);
        let mut boxes = std::mem::take(&mut self.pvp.boxes);
        boxes.clear();
        let p = &self.pvp;
        use pvp_hud::WidgetId as W;
        let widgets = [
            (
                W::Armour,
                hud.armour,
                hud.armour_corner,
                &p.armour.lines,
                p.armour.size,
            ),
            (
                W::Effects,
                hud.effects,
                hud.effects_corner,
                &p.effects.lines,
                p.effects.size,
            ),
            (
                W::Held,
                hud.held,
                hud.held_corner,
                &p.held.lines,
                p.held.size,
            ),
            (
                W::Ping,
                hud.ping,
                hud.ping_corner,
                &p.ping.lines,
                p.ping.size,
            ),
            (
                W::Coords,
                hud.coords,
                hud.coords_corner,
                &p.coords.lines,
                p.coords.size,
            ),
            (
                W::Server,
                hud.server,
                hud.server_corner,
                &p.server.lines,
                p.server.size,
            ),
            (
                W::Combo,
                hud.combo,
                hud.combo_corner,
                &p.combo_count.lines,
                p.combo_count.size,
            ),
            (W::Fps, hud.fps, hud.fps_corner, &p.fps.lines, p.fps.size),
            (W::Cps, hud.cps, hud.cps_corner, &p.cps.lines, p.cps.size),
            (
                W::Clock,
                hud.clock,
                hud.clock_corner,
                &p.clock.lines,
                p.clock.size,
            ),
            (
                W::Reach,
                hud.reach,
                hud.reach_corner,
                &p.reach_shown.lines,
                p.reach_shown.size,
            ),
            (
                W::Speed,
                hud.speed,
                hud.speed_corner,
                &p.speed_shown.lines,
                p.speed_shown.size,
            ),
        ];
        // The editor shows a widget with nothing to show yet by its name.
        let editing = self.hud_editing();
        for (id, on, corner, lines, size) in widgets {
            if !on {
                continue;
            }
            let stand_in;
            let (lines, size) = match lines.is_empty() {
                false => (lines, size),
                true if editing => {
                    let key = format!("horizon.pvp.hud_{}", id.name());
                    let name = horizon_client::lang::get_or(&key, id.name());
                    let width = |s: &str| HudBatch::text_width(&self.gui.atlas, s);
                    stand_in = vec![Line::new(Icon::None, name, &width)];
                    (&stand_in, pvp_hud::size(&stand_in))
                }
                true => continue,
            };
            let place = layout.get(id);
            if let Some(rect) = self.widget_rect(corners, place, corner, size, screen) {
                self.paint_widget(batch, rect, lines, size, &style);
                boxes.push((id, rect));
            }
        }
        if hud.keys {
            let place = layout.get(W::Keys);
            let size = pvp_hud::keys_size();
            if let Some(rect) = self.widget_rect(corners, place, hud.keys_corner, size, screen) {
                self.paint_keys(batch, rect, &style);
                boxes.push((W::Keys, rect));
            }
        }
        self.pvp.boxes = boxes;
    }
    /// Where a widget of `size` goes at its `place`'s scale: where the player put it, else in
    /// its corner (unless a mod hides that corner). `[x, y, w, h]`, scaled.
    fn widget_rect(
        &self,
        corners: &mut Corners,
        place: pvp_hud::Place,
        corner: Corner,
        (w, h): (f32, f32),
        screen: (f32, f32),
    ) -> Option<[f32; 4]> {
        let scale = f32::from(place.scale) / 100.0;
        let size = (w * scale, h * scale);
        let (x, y) = match place.at {
            Some(at) => pvp_hud::from_fractions(at, size, screen),
            None if self.native_corner_shown(corner) => corners.place(corner, size.0, size.1),
            None => return None,
        };
        Some([x, y, size.0, size.1])
    }
    /// The keystrokes' labels: each key's name, made again only when a binding changed.
    fn keystroke_labels(&mut self) {
        let keys = &self.settings.keys;
        let k = &mut self.pvp.keys;
        let same = Key::ALL
            .iter()
            .zip(&k.bound)
            .all(|(key, b)| keys.get(key.mapping()) == b);
        if same && k.labels.len() == Key::ALL.len() {
            return;
        }
        let atlas = &self.gui.atlas;
        k.labels.clear();
        for (i, key) in Key::ALL.into_iter().enumerate() {
            let bound = keys.get(key.mapping());
            k.bound[i] = bound.to_owned();
            let label = match horizon_client::keybinds::mouse_button(bound) {
                Some(MouseButton::Left) => Text::tr("horizon.pvp.hud.keys.attack", &[]),
                Some(MouseButton::Right) => Text::tr("horizon.pvp.hud.keys.use", &[]),
                _ => horizon_client::keybinds::display_name(bound),
            };
            let w = HudBatch::text_width(atlas, &label.plain());
            k.labels.push((label, w));
        }
    }
    /// The keystrokes widget at `(x, y)`: a box per key, light while held, its name centred,
    /// and under each mouse button its clicks per second.
    fn paint_keys(&self, batch: &mut HudBatch, [x, y, sw, _]: [f32; 4], style: &Style) {
        let start = batch.vertices.len();
        let atlas = &self.gui.atlas;
        let k = &self.pvp.keys;
        let colour = |c: u32| {
            let [a, r, g, b] = c.to_be_bytes();
            rgba(r, g, b, a)
        };
        for (i, key) in Key::ALL.into_iter().enumerate() {
            let (kx, ky, w, h) = pvp_hud::key_rect(key);
            let (kx, ky) = (x + kx, y + ky);
            let held = k.held[i];
            if held {
                batch.fill(atlas, kx, ky, w, h, colour(KEY_HELD));
            } else if let Some(background) = style.background {
                batch.fill(atlas, kx, ky, w, h, colour(background));
            }
            let text_colour = colour(if held { KEY_HELD_TEXT } else { style.text });
            let Some((label, lw)) = k.labels.get(i) else {
                continue;
            };
            let cps = match key {
                Key::Attack => k.cps.lines.first(),
                Key::Use => k.cps.lines.get(1),
                _ => None,
            };
            let top = match cps {
                Some(_) => ky + 2.0,
                None => ky + (h - 8.0) / 2.0,
            };
            let shadow = style.shadow && !held;
            batch.text_styled(atlas, label, kx + (w - lw) / 2.0, top, text_colour, shadow);
            if let Some(line) = cps {
                // The count at half size under the name.
                let start = batch.vertices.len();
                let (cx, cy) = (kx + w / 2.0, ky + h - 6.0);
                let tx = cx - line.text_width / 2.0;
                batch.text_styled(atlas, &line.text, tx, cy, text_colour, false);
                batch.scale_since(start, cx, cy, 0.5, 0.5);
            }
        }
        let scale = sw / pvp_hud::keys_size().0;
        if scale != 1.0 {
            batch.scale_since(start, x, y, scale, scale);
        }
    }
    /// The held item count: the first slot holding what is counted, its item, and the total in
    /// the inventory and off hand (arrows for a bow or crossbow). None for a single item.
    fn held_count(&self) -> Option<(usize, Arc<str>, u32)> {
        let held = self.held()?;
        let ranged = matches!(&*held.item, "minecraft:bow" | "minecraft:crossbow");
        let counts = |s: &horizon_client::items::ItemStack| match ranged {
            true => ARROWS.contains(&&*s.item),
            false => s.item == held.item,
        };
        let slots = self.inventory.iter().map(Option::as_ref).enumerate();
        let offhand = std::iter::once((super::shield::OFFHAND_SLOT, self.offhand.as_ref()));
        let mut found = slots
            .chain(offhand)
            .filter_map(|(i, s)| Some((i, s?)))
            .filter(|(_, s)| counts(s));
        let (first, stack) = found.next()?;
        let total = stack.count + found.map(|(_, s)| s.count).sum::<u32>();
        (ranged || total > 1).then(|| (first, stack.item.clone(), total))
    }
    /// One widget's box in `corner`: icons in a column, texts beside them.
    fn paint_widget(
        &self,
        batch: &mut HudBatch,
        [x, y, sw, _]: [f32; 4],
        lines: &[Line],
        (w, h): (f32, f32),
        style: &Style,
    ) {
        let start = batch.vertices.len();
        let atlas = &self.gui.atlas;
        if let Some(background) = style.background {
            let [a, r, g, b] = background.to_be_bytes();
            batch.fill(atlas, x, y, w, h, rgba(r, g, b, a));
        }
        let icons = pvp_hud::icon_column(lines);
        let mut top = y + pvp_hud::PAD;
        for line in lines {
            let inner = line.height() - 2.0;
            let size = line.icon.size();
            let (ix, iy) = (x + pvp_hud::PAD, top + (inner - size) / 2.0);
            match &line.icon {
                Icon::None => {}
                Icon::Armour(i) => {
                    if let Some(stack) = &self.armor[usize::from(*i)] {
                        self.draw_item(batch, stack, ix, iy);
                    }
                }
                Icon::Slot(slot) => {
                    if let Some(stack) = self.stack_in(*slot) {
                        self.draw_item_model(batch, stack, ix, iy);
                    }
                }
                Icon::Sprite(name) => {
                    if let Some(r) = atlas.sprite_or_missing(name) {
                        batch.rect_uv(ix, iy, size, size, r, u32::MAX);
                    }
                }
            }
            let text_h = if line.detail.is_some() { 18.0 } else { 8.0 };
            let tx = x + pvp_hud::PAD + line.text_x(icons);
            let ty = top + (inner - text_h) / 2.0;
            let argb = |c: u32| {
                let [a, r, g, b] = c.to_be_bytes();
                rgba(r, g, b, a)
            };
            // White lines take the widgets' text colour; durability keeps its own.
            let colour = match line.colour {
                0xFFFF_FFFF => style.text,
                c => c,
            };
            batch.text_styled(atlas, &line.text, tx, ty, argb(colour), style.shadow);
            if let Some(detail) = &line.detail {
                batch.text_styled(atlas, detail, tx, ty + 10.0, argb(DETAIL), style.shadow);
            }
            top += line.height();
        }
        if sw != w {
            batch.scale_since(start, x, y, sw / w, sw / w);
        }
    }
}
