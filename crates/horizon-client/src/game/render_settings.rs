//! The renderer and its settings: building it for the window, applying the video settings and a
//! shader pack's translucent layers, and the far terrain (LOD) distances.
use super::*;

#[derive(Default)]
pub(super) struct FrameAttempt<K = FrameKey>(Option<K>);

/// The frame the renderer would be made with (`Renderer::requested_frame`), the pick's path,
/// its outcome.
type FrameKey = (
    Option<horizon_client::gfx::FrameConfiguration>,
    PathBuf,
    Option<Option<horizon_shader_api::Api>>,
);

impl<K: PartialEq> FrameAttempt<K> {
    // Remember attempts, including failures, until the requested configuration changes.
    fn changed(&mut self, key: K) -> bool {
        let changed = self.0.as_ref() != Some(&key);
        self.0 = Some(key);
        changed
    }
}

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
            &self.settings.horizon,
            shader_packs::picked_pack_shape(&self.settings),
            self.renderer
                .as_ref()
                .map(|r| r.display_reading())
                .unwrap_or_default(),
            (
                self.settings.file.int("textureFiltering"),
                self.settings.file.int("maxAnisotropyBit"),
            ),
        )
    }
    /// Under `HORIZON_HDR_FRAME=float` the frame follows what draws the picture
    /// (docs/design/hdr-linear-light.md, "When it changes"): when HDR, Smooth Gradients or the
    /// pick (unless refused) call for another frame than the renderer's, it is made again before
    /// this frame draws. Without the switch a frame stays until the next start, as before.
    pub(super) fn follow_frame(&mut self) {
        if !native_shaders::linear_frame_switch()
            || self.options.paced_by_split
            || self.options.split_guest
        {
            return;
        }
        let picked = shader_packs::picked_pack(&self.settings);
        let path = self.native_shaders.resolve_path(
            &Self::shader_packs_dir(),
            picked,
            shader_packs::dev_pack().is_some(),
        );
        let available = path.exists()
            && path.file_name().is_none_or(|f| {
                !shader_packs::active_pack(&f.to_string_lossy(), &self.settings.mods).is_empty()
            });
        let outcome = if available {
            native_shaders::outcome(&path)
        } else {
            Some(None)
        };
        // The renderer's offer can change while the game runs (Vulkan's formats follow desktop
        // HDR and the window's output), and the frame with it.
        let request = shader_packs::picked_pack_shape(&self.settings).frame();
        let wanted = self
            .renderer
            .as_ref()
            .map(|r| r.requested_frame(&self.settings.horizon, request));
        if !self.frame_attempted.changed((wanted, path, outcome)) {
            return;
        }
        if self
            .renderer
            .as_ref()
            .is_none_or(|r| r.frame_follows(&self.settings.horizon, request))
        {
            return;
        }
        if let Err(e) = self.rebuild_renderer() {
            log::warn!("Horizon: the renderer for the new frame could not be made: {e}");
        }
    }

    /// The renderer made again on the same device from the resources loaded, keeping the
    /// pipelines that come out the same (`adopt_renderer`, as after a lost device).
    fn rebuild_renderer(&mut self) -> Result<()> {
        let Some(window) = self.window.clone() else {
            return Ok(());
        };
        let started = std::time::Instant::now();
        if let Some(renderer) = &mut self.renderer {
            renderer.release_surface();
        }
        let mut renderer = match self.renderer_for(window, &self.pack, &self.gui) {
            Ok(renderer) => renderer,
            Err(error) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.recreate_surface()?;
                }
                return Err(error);
            }
        };
        if let Some(old) = &mut self.renderer {
            renderer.resize(old.config.width, old.config.height);
            renderer.screenshot = old.screenshot.take();
            renderer.screenshot_result = old.screenshot_result.take();
        }
        self.adopt_renderer(renderer);
        log::info!(
            "Horizon: the renderer was made again for the frame the settings call for in {} ms",
            started.elapsed().as_millis()
        );
        Ok(())
    }

    pub(super) fn apply_settings(&mut self) {
        // LocalPlayer.refreshChatAbilities: the chat shows what the Chat option allows.
        let abilities = self.chat_abilities();
        self.ui.chat.set_abilities(&self.gui.atlas.font, abilities);
        let render_distance_changed = self.streaming.radius != self.settings.render_distance;
        self.streaming.radius = self.settings.render_distance;
        // Options.renderDistance, biomeBlendRadius, ambientOcclusion,
        // cutoutLeaves, Opaque Ice and the leaf-culling mode:
        // LevelExtractor.allChanged when they change.
        let leaf_culling = self.effective_leaf_culling();
        let (s, st) = (&self.settings, &self.streaming);
        let reload = render_distance_changed
            || (
                s.biome_blend,
                s.smooth_lighting,
                s.cutout_leaves,
                s.horizon.opaque_ice,
                leaf_culling,
            ) != (
                st.biome_blend,
                st.smooth_lighting,
                st.cutout_leaves,
                st.opaque_ice,
                st.leaf_culling,
            );
        self.streaming.biome_blend = self.settings.biome_blend;
        self.streaming.smooth_lighting = self.settings.smooth_lighting;
        self.streaming.cutout_leaves = self.settings.cutout_leaves;
        self.streaming.opaque_ice = self.settings.horizon.opaque_ice;
        self.streaming.leaf_culling = leaf_culling;
        if reload {
            self.all_sections_changed();
        }
        self.audio.master = self.settings.master_volume;
        if self.settings.sound_device != self.audio.device() {
            let _ = self.audio.set_device(&self.settings.sound_device);
        }
        let old_gui = self.renderer.as_ref().map(|r| {
            let batch = HudBatch::new([r.config.width, r.config.height]);
            (batch.width, batch.height, batch.scale)
        });
        horizon_client::hud::GUI_SCALE.store(
            self.settings.gui_scale,
            std::sync::atomic::Ordering::Relaxed,
        );
        if let Some(renderer) = self.renderer.as_mut() {
            let batch = HudBatch::new([renderer.config.width, renderer.config.height]);
            if old_gui != Some((batch.width, batch.height, batch.scale)) {
                renderer.mark_gui_geometry_changed();
            }
        }
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

#[cfg(test)]
mod tests {
    use super::FrameAttempt;
    use horizon_shader_api::Api;

    #[test]
    fn failed_frame_attempt_waits_for_a_policy_or_setting_change() {
        let mut attempts = FrameAttempt::default();
        let mut rebuilds = 0;
        // The requested frame stands for itself here: the 8-bit one, then the float one.
        for key in [
            ("8-bit", "p", None),
            ("8-bit", "p", None),
            // HDR output offered later, with HDR On: the frame is looked at again.
            ("float", "p", None),
            ("float", "p", Some(Some(Api::V0_2))),
            ("float", "p", Some(Some(Api::V0_2))),
            ("float", "p", Some(None)),
            ("float", "p", Some(None)),
            ("float", "q", None),
            ("8-bit", "q", None),
        ] {
            if attempts.changed(key) {
                // A replacement fails: no success is recorded, just the attempted key.
                rebuilds += 1;
            }
        }
        assert_eq!(rebuilds, 6);
    }
}
