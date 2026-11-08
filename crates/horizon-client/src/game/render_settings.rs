//! The renderer and its settings: building it for the window, applying the video settings and a
//! shader pack's translucent layers, and the far terrain (LOD) distances.
use super::*;

impl Game<'_> {
    pub(super) fn renderer_for(
        &self,
        window: Arc<Window>,
        pack: &PackResources,
        gui: &UiResources,
    ) -> Result<Renderer> {
        build_renderer(
            window,
            self.options.offscreen,
            pack,
            gui,
            self.renderer.as_ref(),
            None,
            self.settings.horizon.api,
            self.settings.horizon.translucent_batching,
            self.settings.horizon.wide_frame(),
            shader_packs::picked_pack_shape(&self.settings),
            (
                self.settings.file.int("textureFiltering"),
                self.settings.file.int("maxAnisotropyBit"),
            ),
        )
    }
    pub(super) fn apply_settings(&mut self) {
        // LocalPlayer.refreshChatAbilities: the chat shows what the Chat option allows.
        let abilities = self.chat_abilities();
        self.ui.chat.set_abilities(&self.gui.atlas.font, abilities);
        let render_distance_changed = self.streaming.radius != self.settings.render_distance;
        self.streaming.radius = self.settings.render_distance;
        // Options.renderDistance, biomeBlendRadius, ambientOcclusion,
        // cutoutLeaves and the leaf-culling mode: LevelExtractor.allChanged
        // when they change.
        let leaf_culling = self.effective_leaf_culling();
        let (s, st) = (&self.settings, &self.streaming);
        let reload = render_distance_changed
            || (
                s.biome_blend,
                s.smooth_lighting,
                s.cutout_leaves,
                leaf_culling,
            ) != (
                st.biome_blend,
                st.smooth_lighting,
                st.cutout_leaves,
                st.leaf_culling,
            );
        self.streaming.biome_blend = self.settings.biome_blend;
        self.streaming.smooth_lighting = self.settings.smooth_lighting;
        self.streaming.cutout_leaves = self.settings.cutout_leaves;
        self.streaming.leaf_culling = leaf_culling;
        if reload {
            self.all_sections_changed();
        }
        self.audio.master = self.settings.master_volume;
        if self.settings.sound_device != self.audio.device() {
            let _ = self.audio.set_device(&self.settings.sound_device);
        }
        horizon_client::hud::GUI_SCALE.store(
            self.settings.gui_scale,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.apply_ui_settings();
    }
    /// The sprite layers a shader pack draws in the translucent pass: every section is meshed
    /// again when they change.
    pub(super) fn set_translucent_layers(&mut self, layers: Vec<bool>) {
        if *layers != *self.streaming.translucent_layers {
            self.streaming.translucent_layers = layers.into();
            self.all_sections_changed();
        }
    }

    /// LOD Distance and Far Detail Reduced as the player chose them, the
    /// distance no farther than a server sampling far terrain for us allows.
    pub(super) fn far_terrain_choice(&self) -> (u32, bool) {
        let g = &self.settings.horizon;
        let reduced = g.far_detail == horizon_client::graphics::FarDetail::Reduced;
        let distance = match self.server_far_terrain() {
            Some(offer) => g.lod_distance.min(offer.distance),
            None => g.lod_distance,
        };
        (distance, reduced)
    }
    /// The far terrain another machine's server samples for us
    /// (`lod_net`): a client of a dedicated server or of a LAN world, not of
    /// its own server thread, which hands its generator over directly.
    pub(super) fn server_far_terrain(&self) -> Option<horizon_client::lod_net::Offer> {
        if self.integrated.is_some() {
            return None;
        }
        self.remote.as_ref()?.far_terrain
    }
    /// LOD Distance as far terrain uses it (`graphics::lod_distance_capped`).
    pub(super) fn lod_distance(&self) -> u32 {
        let (distance, _) = self.far_terrain_choice();
        match &self.renderer {
            Some(renderer) => {
                horizon_client::graphics::lod_distance_capped(distance, &renderer.info)
            }
            None => distance.min(horizon_client::lod::MAX_CHUNKS),
        }
    }
    /// Only enabled LOD worlds extend the renderer's range, never simulation.
    pub(super) fn terrain_distance(&self) -> i32 {
        if self.lod.is_some() {
            self.streaming.radius.max(self.lod_distance() as i32)
        } else {
            self.streaming.radius
        }
    }
}
