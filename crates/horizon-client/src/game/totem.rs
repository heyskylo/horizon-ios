//! The totem of undying (`LivingEntity.checkTotemDeathProtection`: a hand's
//! item carrying the `death_protection` component, whatever item it is, and
//! entity event 35 on the client).
use super::Game;
use horizon_assets::item_model::DisplayContext;
use horizon_client::{
    block_entity_render::vertex_light,
    items::ItemStack,
    mesher::Vertex,
    particles::{ParticleOptions, ParticleType},
};

const TOTEM: &str = "minecraft:totem_of_undying";

/// `EntityEvent.TOTEM_OF_UNDYING`.
pub(super) const TOTEM_EVENT: i8 = 35;

impl Game<'_> {
    /// Whether the stack has `DataComponents.DEATH_PROTECTION`, whatever
    /// item it is: the totem of undying is only one item that carries it.
    fn has_death_protection(&self, s: &ItemStack) -> bool {
        self.pack
            .item_renderer
            .models
            .components(&horizon_client::item_render::stack_view(s))
            .get("minecraft:death_protection")
            .is_some()
    }
    /// The `death_effects` of the stack's `minecraft:death_protection`.
    fn death_effects(&self, s: &ItemStack) -> Vec<horizon_client::items::ConsumeEffect> {
        death_effects_of(&self.pack.item_renderer.models, s)
    }
    /// What each armed mob's hands carry as `minecraft:death_protection`, for
    /// `Mob::use_death_protection` (the mob has no item data of its own): any
    /// item with the component counts, not only the totem.
    pub(super) fn sync_mob_death_protection(&mut self) {
        let models = &self.pack.item_renderer.models;
        let of = |id: Option<&std::sync::Arc<str>>, stack: Option<&ItemStack>| {
            let stack = match (stack, id) {
                (Some(s), _) => s.clone(),
                (None, Some(id)) => ItemStack::new(id, 1),
                (None, None) => return None,
            };
            models
                .components(&horizon_client::item_render::stack_view(&stack))
                .get("minecraft:death_protection")
                .map(horizon_client::items::death_effects)
        };
        for m in self.mobs.iter_mut().filter(|m| !m.removed) {
            if m.extra.main_hand.is_none() && m.extra.off_hand.is_none() {
                m.extra.hand_protection = None;
                continue;
            }
            m.extra.hand_protection = Some(horizon_client::mobs::HandProtection {
                main: of(m.extra.main_hand.as_ref(), m.extra.main_hand_stack()),
                off: of(m.extra.off_hand.as_ref(), None),
            });
        }
    }
    /// Whether either hand holds a death-protection item, for the next hurt:
    /// the window's player once a tick, a server's player for each of its
    /// turns.
    pub(super) fn update_totem_ready(&mut self) {
        let ready = self.held().is_some_and(|s| self.has_death_protection(s))
            || self
                .offhand
                .as_ref()
                .is_some_and(|s| self.has_death_protection(s));
        if let Some(p) = self.player.as_mut() {
            p.health.totem_ready = ready;
        }
    }
    /// The server's half once a totem fired: use it up (main hand first),
    /// count it, set health to 1, then clear and apply its effects. Returns
    /// the player's body, for entity event 35, which the caller broadcasts,
    /// with the protector copy the activation animation shows.
    pub(super) fn apply_totem(&mut self) -> Option<Body> {
        if !self.player.as_ref()?.health.totem_used {
            return None;
        }
        // The mods hear a server player's totem before it is used up.
        if let (Some(uuid), Some(name)) = (self.lan_acting_uuid, self.lan_acting_name.clone()) {
            self.native_player_totem(uuid, &name);
        }
        let p = self.player.as_mut()?;
        p.health.totem_used = false;
        let _ = p.set_health(1.0, false);
        let body = Body {
            at: [p.position.x, p.position.y, p.position.z],
            width: f64::from(p.dimensions.width),
            height: f64::from(p.dimensions.height),
            protector: None,
        };
        // `checkTotemDeathProtection`: the selected slot first, then the
        // offhand. The copy of that stack is the protector (`protectionItem`)
        // the use is counted and the activation animation shown with, taken
        // before the use takes one off the stack.
        let main = self.held().is_some_and(|s| self.has_death_protection(s));
        let cell = if main {
            &mut self.inventory[self.selected]
        } else {
            &mut self.offhand
        };
        let protector = cell
            .as_ref()
            .cloned()
            .unwrap_or_else(|| ItemStack::new(TOTEM, 1));
        if let Some(s) = cell.as_mut() {
            s.count -= 1;
            if s.count == 0 {
                *cell = None;
            }
        }
        self.award_stat("used", &protector.item, 1);
        self.trigger_item("minecraft:used_totem", &protector);
        // ItemStack.causeUseVibration: the totem's use effects make one.
        let ctx = self.player_context(None);
        self.post_game_event("minecraft:item_interact_finish", body.at, ctx);
        // `DeathProtection.applyEffects`: the component's own `death_effects`,
        // read from the protector copy (the totem's clear-then-effects, or a
        // custom list; `death_protection={}` has none).
        for effect in self.death_effects(&protector) {
            self.apply_consume_effect(&effect);
        }
        Some(Body {
            protector: Some(protector),
            ..body
        })
    }
    /// The window's player's totem: the server's half, then the event to
    /// the guests and to this window's own client.
    pub(super) fn tick_totem(&mut self) {
        let Some(body) = self.apply_totem() else {
            return;
        };
        self.totem_event(body, true);
    }
    /// Entity event 35 from a server, for this client's own player or an
    /// entity it draws.
    pub(super) fn remote_totem(&mut self, id: i32) {
        let own = self.remote.as_ref().is_some_and(|r| r.entity_id == id);
        let body = if own {
            self.player.as_ref().map(|p| Body {
                at: [p.position.x, p.position.y, p.position.z],
                width: f64::from(p.dimensions.width),
                height: f64::from(p.dimensions.height),
                protector: None,
            })
        } else {
            self.mobs.iter().find(|m| m.id == id as u32).map(|m| Body {
                at: [m.position.x, m.position.y, m.position.z],
                width: f64::from(m.width()),
                height: f64::from(m.height()),
                protector: None,
            })
        };
        if let Some(body) = body {
            self.totem_event(body, own);
        }
    }
    /// `ClientPacketListener.handleEntityEvent` 35: the totem particles (a
    /// 30-tick emitter) and its sound, heard here only (`playLocalSound`),
    /// and the player's own rumble.
    pub(super) fn totem_event(&mut self, body: Body, own: bool) {
        self.particles.tracking_emitter_for(
            ParticleOptions::simple(ParticleType::TotemOfUndying),
            body.at,
            body.width,
            body.height,
            30,
        );
        self.audio
            .play_unshared("minecraft:item.totem.use", Some(body.at), 1.0, 1.0);
        if own {
            self.pad_rumble(horizon_client::gamepad::Rumble::Totem);
            // findTotem, then LocalPlayer.displayItemActivation. The offsets
            // come from the player's own random in vanilla, not the level's:
            // the client-only particle random stands in for it. The server
            // half hands over the stack it used up; only an event from a
            // remote server, whose item is still in the hand, looks one up.
            let totem = body.protector.unwrap_or_else(|| self.find_totem());
            self.item_activation
                .activate(totem, self.particles.random());
        }
    }
    /// `ClientPacketListener.findTotem`: the first hand's item with
    /// `death_protection`, else a new totem (a used single totem is gone by
    /// the time the event arrives).
    fn find_totem(&self) -> ItemStack {
        [self.held(), self.offhand.as_ref()]
            .into_iter()
            .flatten()
            .find(|s| self.has_death_protection(s))
            .cloned()
            .unwrap_or_else(|| ItemStack::new(TOTEM, 1))
    }
    /// `GameRenderer.tick`: the activated item counts down.
    pub(super) fn tick_item_activation(&mut self) {
        if self.player.is_some() {
            self.item_activation.tick();
        }
    }
    /// `ScreenEffectRenderer.renderItemActivationAnimation`, for the hand
    /// pass: the item in view space with the hand's projection but none of
    /// the camera's turn or bob (a fresh `PoseStack`), so those are undone
    /// here. `FIXED` display, no owner, seed 0 (`LevelExtractor`), full
    /// brightness and `Lighting.Entry.ITEMS_3D`. Only F1 hides it.
    pub(super) fn item_activation_geometry(
        &self,
        partial: f32,
        view: glam::Mat4,
        aspect: f32,
        out: &mut Vec<Vertex>,
    ) {
        if self.ui.hide_gui {
            return;
        }
        let (Some(stack), Some(pose)) = (
            self.item_activation.active(),
            self.item_activation.pose(partial, aspect),
        ) else {
            return;
        };
        let state = self.resolve_item(stack, DisplayContext::Fixed, None, 0);
        let to_world = view.inverse();
        let (a, b) = horizon_client::item_render::gui::items_3d_lights();
        let lights = (
            to_world.transform_vector3(a).normalize(),
            to_world.transform_vector3(b).normalize(),
        );
        // PvP & HUD's totem animation size (100% is vanilla's), about the item's centre.
        let size = f32::from(self.settings.extras.pvp.tweaks.totem_size) / 100.0;
        let pose = pose * glam::Mat4::from_scale(glam::Vec3::splat(size));
        self.submit_item_lit(&state, to_world * pose, vertex_light(0xFF), lights, out);
    }
}

/// The `death_effects` of the stack's `minecraft:death_protection`, if it has
/// the component.
fn death_effects_of(
    models: &horizon_assets::item_model::ItemModels,
    s: &ItemStack,
) -> Vec<horizon_client::items::ConsumeEffect> {
    models
        .components(&horizon_client::item_render::stack_view(s))
        .get("minecraft:death_protection")
        .map(horizon_client::items::death_effects)
        .unwrap_or_default()
}

/// Where an entity event's entity stands and its size, and, for the event
/// this window's own player fires after the server half used a stack up, the
/// protector copy the activation animation shows.
#[derive(Clone)]
pub(super) struct Body {
    pub at: [f64; 3],
    width: f64,
    height: f64,
    protector: Option<ItemStack>,
}
