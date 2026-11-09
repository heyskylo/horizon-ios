//! The renderer and its settings: building it for the window, applying the video settings and a
//! shader pack's translucent layers, and the far terrain (LOD) distances.
use super::*;

#[derive(Default)]
pub(super) struct FrameAttempt(Option<FrameKey>);

/// HDR wanted, HDR output offered, Smooth Gradients' wide frame, the pick's path, its outcome.
type FrameKey = (
    bool,
    bool,
    bool,
    PathBuf,
    Option<Option<horizon_shader_api::Api>>,
);

impl FrameAttempt {
    // Remember attempts, including failures, until the requested configuration changes.
    fn changed(&mut self, key: FrameKey) -> bool {
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
            self.settings.horizon.wide_frame(),
            shader_packs::picked_pack_shape(&self.settings),
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
        let horizon = &self.settings.horizon;
        let wide_frame = horizon.wide_frame();
        // On wants HDR whether or not it is offered; the offer can change while the game runs
        // (Vulkan's formats follow desktop HDR and the window's output), and the frame with it.
        let offered = horizon_client::graphics::hdr_offered();
        let key = (horizon.hdr_wanted(), offered, wide_frame, path, outcome);
        if !self.frame_attempted.changed(key) {
            return;
        }
        let shape = shader_packs::picked_pack_shape(&self.settings);
        if self
            .renderer
            .as_ref()
            .is_none_or(|r| r.frame_follows(wide_frame, shape.linear, shape.hdr_float))
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

#[cfg(test)]
mod tests {
    use super::FrameAttempt;
    use horizon_shader_api::Api;

    #[test]
    fn failed_frame_attempt_waits_for_a_policy_or_setting_change() {
        let mut attempts = FrameAttempt::default();
        let mut rebuilds = 0;
        for key in [
            (true, false, true, "p", None),
            (true, false, true, "p", None),
            // HDR output offered later, with HDR On: the frame is looked at again.
            (true, true, true, "p", None),
            (true, true, true, "p", Some(Some(Api::V0_2))),
            (true, true, true, "p", Some(Some(Api::V0_2))),
            (true, true, true, "p", Some(None)),
            (true, true, true, "p", Some(None)),
            (true, true, true, "q", None),
            (false, true, false, "q", None),
        ] {
            if attempts.changed((key.0, key.1, key.2, key.3.into(), key.4)) {
                // A replacement fails: no success is recorded, just the attempted key.
                rebuilds += 1;
            }
        }
        assert_eq!(rebuilds, 6);
    }
}
