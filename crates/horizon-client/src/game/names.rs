//! Custom names: `NameTagItem.interactLivingEntity` and the name tag
//! (`EntityRenderer.submitNameTag`): above the head, facing the camera,
//! on a translucent black background, shown when the name is visible or the
//! mob is under the crosshair within 64 blocks, as its team's name tag
//! visibility allows, in the team's colours, with its score in the
//! objective displayed below names. Other players' names (on a LAN host
//! or a server) show whenever `LivingEntityRenderer.shouldShowName` allows,
//! and the player's own in third person with the `ownNameTag` extra.
use super::{Game, view_vector};
use horizon_client::{
    block_entity_render::vertex_light,
    hud::{HudAtlas, HudBatch, HudVertex},
    mesher::Vertex,
    mobs::Mob,
    mod_api,
    save::uuid_text,
    scoreboard as sb,
    text::Text,
};

/// The world size of one text pixel in a name tag: `Matrix4f.scale(0.025F, -0.025F, 0.025F)`.
pub(super) const NAME_TAG_UNIT: f32 = 0.025;

/// Local glyph quads only: camera transforms, lighting and cosmetic marks stay per frame.
/// Slots follow submission order; comparing the whole styled text also handles entity reorder,
/// team/score changes and native name hooks without keeping entity IDs alive.
#[derive(Default)]
pub(super) struct NameTagLayouts {
    epoch: (u64, u64, bool),
    next: usize,
    lines: Vec<Option<NameTagLine>>,
}

struct NameTagLine {
    text: Text,
    alpha: u32,
    background: bool,
    y: f32,
    behind: usize,
    vertices: Vec<HudVertex>,
}

impl NameTagLayouts {
    fn begin_frame(&mut self, atlas: &HudAtlas) {
        let epoch = (
            atlas.revision,
            atlas.font.epoch(),
            horizon_client::lang::bidirectional(),
        );
        if self.epoch != epoch {
            self.lines.clear();
            self.epoch = epoch;
        }
        self.next = 0;
    }

    fn end_frame(&mut self) {
        self.lines.truncate(self.next);
    }

    fn name_tag(
        &mut self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        text: &Text,
        y: f32,
        alpha: u32,
        background: bool,
    ) -> usize {
        // At most 512 * 512 HUD vertices (~10 MiB), plus short text keys. Longer tags and
        // excess lines still render normally. Animated/random glyphs and live inline skins
        // must go through the font each time, including legacy formatting supplied by hooks.
        const MAX_LINES: usize = 512;
        const MAX_VERTICES: usize = 512;
        let slot = self.next;
        self.next += 1;
        let stable =
            text.resolved().spans.iter().all(|span| {
                span.style.obfuscated != Some(true) && span.style.inline_object.is_none()
            }) && text.spans.iter().map(|s| s.text.len()).sum::<usize>() <= 1024;
        if slot < MAX_LINES
            && stable
            && let Some(Some(line)) = self.lines.get(slot)
            && line.text == *text
            && line.alpha == alpha
            && line.background == background
            && line.y == y
        {
            batch.vertices.extend_from_slice(&line.vertices);
            return line.behind;
        }
        let behind = batch.name_tag(
            atlas,
            text,
            y,
            alpha << 24 | 0xFF_FFFF,
            if background { 0x3F00_0000 } else { 0 },
        );
        if slot < MAX_LINES {
            if self.lines.len() <= slot {
                self.lines.resize_with(slot + 1, || None);
            }
            self.lines[slot] =
                (stable && batch.vertices.len() <= MAX_VERTICES).then(|| NameTagLine {
                    text: text.clone(),
                    alpha,
                    background,
                    y,
                    behind,
                    vertices: batch.vertices.clone(),
                });
        }
        behind
    }
}

impl Game<'_> {
    /// A named name tag names the mob under the crosshair.
    pub(super) fn name_mob(&mut self, index: usize) -> bool {
        let Some(name) = self
            .held()
            .filter(|h| &*h.item == "minecraft:name_tag")
            .and_then(|h| h.custom_name())
        else {
            return false;
        };
        let m = &mut self.mobs[index];
        if !m.alive() {
            return false;
        }
        let entity_id = m.id;
        let entity_name = name.clone();
        m.custom_name = Some(name);
        m.persistent = true;
        // itemStack.shrink(1), in creative too.
        self.consume_held();
        self.swing_arm();
        if let (Some(player), Some(player_name)) =
            (self.lan_acting_uuid, self.lan_acting_name.clone())
        {
            self.native_player_name_entity(player, &player_name, entity_id, &entity_name);
        }
        true
    }
    /// `Entity.belowNameDisplay`: the score holder's score in the objective
    /// shown below names, then a space and the objective's name.
    fn below_name_text(&self, holder: &str) -> Option<Text> {
        let board = &self.score.board;
        let objective = board.display_objective(sb::SLOT_BELOW_NAME)?;
        let score = board.score(holder, &objective.name)?;
        // numberFormatOrDefault(StyledFormat.NO_STYLE).
        let format = score
            .number_format
            .clone()
            .or_else(|| objective.number_format.clone())
            .unwrap_or(sb::NumberFormat::Styled(serde_json::json!({})));
        let mut t = Text::empty();
        t.push(format.format(score.value));
        t.push(Text::literal(" "));
        t.push(objective.display_text());
        Some(t)
    }
    /// The other players' models and names: the players a server sends.
    fn other_players(&self) -> Vec<(&Mob, String)> {
        let mut out = Vec::new();
        let indices = first_mob_indices(&self.mobs);
        for (id, name) in self.remote_player_names() {
            if let Some(&index) = indices.get(&id) {
                out.push((&self.mobs[index], name));
            }
        }
        out
    }
    /// Horizon's own name tag extra (`ownNameTag`): the player's own model and
    /// name, while that model shows in third person. Vanilla never names the
    /// camera entity (`LivingEntityRenderer.shouldShowName`).
    fn own_name_tag(&self) -> Option<(&Mob, String)> {
        if !self.settings.extras.own_name_tag
            || self.camera_type.first_person()
            || self.sleeping.is_some()
            || self.spectator()
            || self.own_invisible()
        {
            return None;
        }
        let m = self.player_model.as_ref()?;
        Some((m, self.own_profile_name()))
    }
    /// The local player's profile name as others see it: the session's on a
    /// server (single player's server thread included), else the local player's.
    pub(super) fn own_profile_name(&self) -> String {
        match self.remote.as_ref() {
            Some(r) if !r.profile_name.is_empty() => r.profile_name.clone(),
            _ => self.acting_player_name().to_owned(),
        }
    }
    /// The names the name tags show this frame (the control socket's status).
    pub(super) fn shown_name_tags(&self, camera: glam::DVec3) -> Vec<String> {
        self.name_tags(camera, 1.0)
            .into_iter()
            .filter_map(|t| t.lines.last().map(|(t, _)| t.plain()))
            .collect()
    }
    /// Vanilla name bounds for a native render snapshot; never re-enters name hooks.
    pub(super) fn client_name_tag_widths(
        &self,
        camera: glam::DVec3,
        alpha: f64,
    ) -> std::collections::HashMap<[u64; 3], f32> {
        self.name_tags_with_hooks(camera, alpha, false)
            .into_iter()
            .map(|tag| {
                let width = tag
                    .lines
                    .iter()
                    .map(|(text, _)| self.gui.atlas.font.width_text(text) * NAME_TAG_UNIT * 0.5)
                    .fold(0.0, f32::max);
                (tag.at.to_array().map(f64::to_bits), width)
            })
            .collect()
    }
    /// Each name tag this frame.
    fn name_tags(&self, camera: glam::DVec3, alpha: f64) -> Vec<NameTag> {
        self.name_tags_with_hooks(camera, alpha, !mod_api::mods().is_empty())
    }
    fn name_tags_with_hooks(&self, camera: glam::DVec3, alpha: f64, hooks: bool) -> Vec<NameTag> {
        self.collect_name_tags(camera, alpha, hooks, None)
    }
    fn collect_name_tags(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        hooks: bool,
        frustum: Option<&horizon_client::gfx::Frustum>,
    ) -> Vec<NameTag> {
        let target = self.mob_hit().map(|(i, _, _)| self.mobs[i].id);
        let viewer = self.player_team();
        let mut tags = Vec::new();
        let players = self.other_players();
        let player_ids = named_by_player_list(&players);
        let named = self.mobs.iter().filter_map(|m| {
            // Already named by the player list: a second tag would sit on the
            // first (`named_by_player_list`).
            if player_ids.contains(&m.id) {
                return None;
            }
            // A mannequin with no name still shows its description.
            let name = match &m.custom_name {
                Some(name) => name.clone(),
                None if hooks || m.species() == horizon_client::mobs::Species::Mannequin => {
                    String::new()
                }
                None => return None,
            };
            // The tag's 64 block distance is measured to the entity: copy the
            // name only for a mob the frame can show it on.
            let at = glam::DVec3::new(m.position.x, m.position.y, m.position.z);
            (at.distance_squared(camera) < 64.0 * 64.0).then_some((m, name, false))
        });
        let own = self.own_name_tag().map(|(m, _)| m.id);
        let players = players
            .into_iter()
            .chain(self.own_name_tag())
            .map(|(m, name)| (m, name, true));
        // Who is riding whom: asked once per tag below, and rescanning every entity
        // for each tag made this quadratic in the number of loaded entities.
        let local_entity_id = self.remote.as_ref().map(|r| r.entity_id as u32);
        let ridden = collect_vehicle_ids(
            &self.mobs,
            self.remote_vehicle_ids(),
            self.riding,
            local_entity_id,
        );
        for (m, name, player) in named.chain(players) {
            if !m.alive() {
                continue;
            }
            // EntityAttachment.NAME_TAG: 0.5 above the box of the current pose.
            let p = name_tag_at(m, alpha);
            // The name tag distance (64) is measured to the entity.
            let distance_sqr =
                glam::DVec3::new(m.position.x, m.position.y, m.position.z).distance_squared(camera);
            // Match the model's renderer-specific box, not the tag anchor: the
            // name must survive when only the entity's feet are in view.
            let (lo, hi) = super::entity_culling::cull_box(m, alpha);
            let inflate = glam::DVec3::splat(0.5);
            if !name_tag_in_view(
                frustum,
                distance_sqr,
                (lo - camera - inflate).as_vec3(),
                (hi - camera + inflate).as_vec3(),
            ) {
                continue;
            }
            let marks = if player || self.cosmetics_local() {
                self.cosmetics_marks(m, Some(m.id) == own)
            } else {
                Marks::default()
            };
            let at = match marks.top {
                Some(top) => p + glam::DVec3::Y * cosmetic_lift(top, m.model_scale()),
                None => p,
            };
            // A player's scores and team go by name, a mob's by UUID.
            let holder = if player {
                Some(name.clone())
            } else {
                self.persist.uuids.get(&m.id).map(|u| uuid_text(*u))
            };
            let team = if player {
                self.score.board.players_team(&name)
            } else {
                self.mob_team_of(m)
            };
            // LivingEntityRenderer.shouldShowName (team rules included), then
            // MobRenderer: a visible name, or the mob under the crosshair.
            let invisible = sb::invisible_to(
                team,
                viewer,
                self.spectator(),
                m.invisible || m.has_effect("minecraft:invisibility"),
            );
            // Entity.isDiscrete: a crouching player.
            let discrete = player && m.extra.crouching;
            let view = sb::NameView {
                discrete,
                distance_sqr,
                invisible_to_viewer: invisible,
                hud_hidden: self.ui.hide_gui,
                is_camera: false,
                is_vehicle: ridden.contains(&m.id),
            };
            // MobRenderer.shouldShowName: a visible name, or the mob under the
            // crosshair; players (LivingEntityRenderer) need neither.
            // `ClientMannequin.belowNameDisplay`: the description is `scoreText`, drawn
            // within the below-name distance (10) whether the name shows or not.
            let description = (m.species() == horizon_client::mobs::Species::Mannequin
                && distance_sqr < 10.0 * 10.0)
                .then(|| m.extra.description.component())
                .flatten()
                .map(|tag| horizon_client::entity_nbt::name_text(&tag).resolve_legacy());
            let is_own = Some(m.id) == own;
            let name_shown = sb::name_shown(team, viewer, view)
                && (!is_own || !view.is_vehicle)
                && (player || m.name_visible || Some(m.id) == target)
                && (player || m.custom_name.is_some());
            let mut visible = name_shown;
            let mut text = None;
            if hooks {
                self.native_render_name_tag(
                    m.id,
                    if player {
                        "minecraft:player"
                    } else {
                        &m.kind.id
                    },
                    name_shown,
                    &mut visible,
                    &mut text,
                );
            }
            if !visible {
                if let Some(text) = description {
                    let l = self.world.light(
                        m.position.x.floor() as i32,
                        (m.position.y + f64::from(m.height())).floor() as i32,
                        m.position.z.floor() as i32,
                    );
                    tags.push(NameTag {
                        at: p,
                        lines: vec![(text, 0.0)],
                        light: l,
                        discrete,
                        marks: Marks::default(),
                    });
                }
                continue;
            }
            // getDisplayName: the name in the team's prefix, suffix and colour.
            let member = if player {
                Text::literal(&name)
            } else {
                m.custom_name_text
                    .clone()
                    .unwrap_or_else(|| Text::literal(&name))
            };
            // StringDecomposer.iterateFormatted: `§` codes a server leaves
            // inside the team prefix or the name itself apply at draw time.
            let shown = match team {
                Some(t) => t.formatted_name(member),
                None => member,
            }
            .resolve_legacy();
            let shown = text
                .map(|text| Text::literal(text).resolve_legacy())
                .unwrap_or(shown);
            let mut lines = Vec::new();
            // EntityRenderer.extractNameTags: the score shows within the
            // below-name distance (10), under the name.
            let mut lift = 0.0;
            if m.species() == horizon_client::mobs::Species::Mannequin {
                if let Some(text) = description {
                    lines.push((text, 0.0));
                    lift = f64::from(9.0f32 * 1.15 * 0.025);
                }
            } else if distance_sqr < 10.0 * 10.0
                && let Some(score) = holder.and_then(|h| self.below_name_text(&h))
            {
                lines.push((score.resolve_legacy(), 0.0));
                lift = f64::from(9.0f32 * 1.15 * 0.025);
            }
            lines.push((shown, lift));
            let l = self.world.light(
                m.position.x.floor() as i32,
                (m.position.y + f64::from(m.height())).floor() as i32,
                m.position.z.floor() as i32,
            );
            tags.push(NameTag {
                at,
                lines,
                light: l,
                discrete,
                marks,
            });
        }
        drop(player_ids);
        // Armor stands name themselves: an invisible one with a visible name
        // is a hologram (its model is skipped, its name tag is not).
        for a in &self.armor_stands {
            if !hooks {
                let Some(_) = &a.custom_name else {
                    continue;
                };
                if !a.name_visible {
                    continue;
                }
            }
            let height = match (a.marker, a.small) {
                (true, _) => 0.0,
                (false, true) => 0.9875,
                (false, false) => 1.975,
            };
            let distance_sqr =
                glam::DVec3::new(a.position.x, a.position.y, a.position.z).distance_squared(camera);
            let position = a.render_position(alpha);
            // Same dimensions and dragon-head growth as armor_stand_geometry.
            let width = if a.marker {
                0.0
            } else if a.small {
                0.25
            } else {
                0.5
            };
            let dragon = a.equipment[5]
                .as_ref()
                .is_some_and(|i| &*i.item == "minecraft:dragon_head");
            let grow = if dragon { 1.0 } else { 0.5 };
            let half = glam::DVec3::new(width / 2.0 + grow, grow, width / 2.0 + grow);
            let origin = super::entity_culling::at(position) - camera;
            if !name_tag_in_view(
                frustum,
                distance_sqr,
                (origin - half).as_vec3(),
                (origin + glam::DVec3::new(half.x, height + grow, half.z)).as_vec3(),
            ) {
                continue;
            }
            let team = self.mob_team(a.id);
            let invisible = sb::invisible_to(team, viewer, self.spectator(), false);
            let view = sb::NameView {
                discrete: false,
                distance_sqr,
                invisible_to_viewer: invisible,
                hud_hidden: self.ui.hide_gui,
                is_camera: false,
                is_vehicle: false,
            };
            let name_shown =
                a.custom_name.is_some() && a.name_visible && sb::name_shown(team, viewer, view);
            let mut visible = name_shown;
            let mut text = None;
            if hooks {
                self.native_render_name_tag(
                    a.id,
                    "minecraft:armor_stand",
                    name_shown,
                    &mut visible,
                    &mut text,
                );
            }
            if !visible {
                continue;
            }
            let name = a.custom_name.clone().unwrap_or_else(Text::empty);
            let shown = match team {
                Some(t) => t.formatted_name(name.clone()),
                None => name.clone(),
            }
            .resolve_legacy();
            let shown = text
                .map(|text| Text::literal(text).resolve_legacy())
                .unwrap_or(shown);
            let mut lines = Vec::new();
            let mut lift = 0.0;
            let holder = self.persist.uuids.get(&a.id).map(|u| uuid_text(*u));
            if distance_sqr < 10.0 * 10.0
                && let Some(score) = holder.and_then(|h| self.below_name_text(&h))
            {
                lines.push((score.resolve_legacy(), 0.0));
                lift = f64::from(9.0f32 * 1.15 * 0.025);
            }
            lines.push((shown, lift));
            // The tag rides with the body, between last tick's position and this one's.
            let at = a.render_position(alpha);
            let l = self.world.light(
                at.x.floor() as i32,
                (at.y + height).floor() as i32,
                at.z.floor() as i32,
            );
            tags.push(NameTag {
                at: glam::DVec3::new(at.x, at.y + height + 0.5, at.z),
                lines,
                light: l,
                discrete: false,
                marks: Marks::default(),
            });
        }
        // ItemFrameRenderer.shouldShowName/getNameTag: only the targeted
        // frame's custom-named item, with the HUD visible (also glow frames).
        let targeted_frame = (hooks || !self.ui.hide_gui)
            .then(|| self.frame_hit())
            .flatten()
            .filter(|(_, distance)| {
                self.before_block(*distance) && self.mob_hit().is_none_or(|(_, d, _)| d > *distance)
            })
            .map(|(index, _)| index);
        let frames = if hooks {
            0..self.item_frames.len()
        } else {
            targeted_frame.map_or(0..0, |index| index..index + 1)
        };
        for index in frames {
            let frame = &self.item_frames[index];
            let name = frame.item.as_ref().and_then(|item| item.custom_name());
            if !hooks && name.is_none() {
                continue;
            }
            let (lo, hi) = frame.bounds();
            let center = glam::DVec3::from_array(std::array::from_fn(|a| (lo[a] + hi[a]) / 2.0));
            if center.distance_squared(camera) < 64.0 * 64.0 {
                let name_shown =
                    !self.ui.hide_gui && targeted_frame == Some(index) && name.is_some();
                let mut visible = name_shown;
                let mut text = None;
                if hooks {
                    self.native_render_name_tag(
                        frame.id,
                        frame.kind(),
                        name_shown,
                        &mut visible,
                        &mut text,
                    );
                }
                if !visible {
                    continue;
                }
                // NAME_TAG is at the entity height (0.5), then the name
                // submitter lifts it 0.5. ItemFrameRenderer offsets rendering
                // 0.3 toward its facing and -0.25 vertically.
                let step = frame.step();
                tags.push(NameTag {
                    at: center
                        + glam::DVec3::new(
                            f64::from(step[0] as f32 * 0.3),
                            0.75,
                            f64::from(step[2] as f32 * 0.3),
                        ),
                    lines: vec![(
                        Text::literal(text.or(name).unwrap_or_default()).resolve_legacy(),
                        0.0,
                    )],
                    light: self.world.light(
                        center.x.floor() as i32,
                        center.y.floor() as i32,
                        center.z.floor() as i32,
                    ),
                    discrete: false,
                    marks: Marks::default(),
                });
            }
        }
        tags
    }
    /// `SubmitNodeCollection.submitNameTag`: a discrete name is drawn once,
    /// faded, on its background; any other in full over the world, its faded
    /// copy and background in `see_through`, over everything.
    pub(super) fn name_tag_geometry(
        &mut self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
        see_through: &mut Vec<Vertex>,
    ) {
        // The text alpha that goes with getBackgroundOpacity(0.25):
        // max((0.25 + 0.75) / 2, 0.5), as ARGB.color (the floor of 255 times it).
        const FADED: u32 = 0x7F;
        let mut patches = Vec::new();
        let mut layouts = std::mem::take(&mut self.name_tag_layouts);
        layouts.begin_frame(&self.gui.atlas);
        for tag in self.collect_name_tags(
            camera,
            alpha,
            !mod_api::mods().is_empty(),
            self.view_frustum.as_ref(),
        ) {
            let o = tag.at - camera;
            // `gbuffers_entities` draws name tag text (2.3); a sneaking player's faded tag, on its
            // background, blends.
            if tag.discrete {
                self.mark_translucent_entity(out.len(), "minecraft:name_tag");
                let light = vertex_light(tag.light);
                patches.extend(self.styled_name_tag_quads(
                    o,
                    &tag.lines,
                    FADED,
                    true,
                    0,
                    light,
                    NAME_TAG_UNIT,
                    out,
                    tag.marks,
                    Some(&mut layouts),
                ));
                continue;
            }
            // LightCoordsUtil.lightCoordsWithEmission(lightCoords, 2).
            let lit = (tag.light >> 4).max(2) << 4 | (tag.light & 15).max(2);
            self.mark_entity(out.len(), "minecraft:name_tag");
            let light = vertex_light(lit);
            patches.extend(self.styled_name_tag_quads(
                o,
                &tag.lines,
                0xFF,
                false,
                0,
                light,
                NAME_TAG_UNIT,
                out,
                tag.marks,
                Some(&mut layouts),
            ));
            // RenderPipelines.TEXT_SEE_THROUGH: no depth test, no lightmap.
            patches.extend(self.styled_name_tag_quads(
                o,
                &tag.lines,
                FADED,
                true,
                2,
                0,
                NAME_TAG_UNIT,
                see_through,
                tag.marks,
                Some(&mut layouts),
            ));
        }
        layouts.end_frame();
        self.name_tag_layouts = layouts;
        self.map_patches.extend(patches);
    }
    /// The lines of one name tag, `o` from the camera: the text at
    /// `text_alpha` in its layer, on the translucent black background
    /// (in `text_layer` like the glyphs, a hair behind) when `background`.
    /// Returns the glyph pixels the atlas still needs.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn name_tag_quads(
        &self,
        o: glam::DVec3,
        lines: &[(Text, f64)],
        text_alpha: u32,
        background: bool,
        text_layer: u32,
        light: u32,
        unit: f32,
        out: &mut Vec<Vertex>,
    ) -> Vec<horizon_client::font::Patch> {
        self.styled_name_tag_quads(
            o,
            lines,
            text_alpha,
            background,
            text_layer,
            light,
            unit,
            out,
            Marks::default(),
            None,
        )
    }

    /// [`Self::name_tag_quads`], with Horizon's marks on the last line (the name): the Horizon
    /// player's mark before the text, a name tag style's badge after it and its line under the
    /// background.
    #[allow(clippy::too_many_arguments)]
    fn styled_name_tag_quads(
        &self,
        o: glam::DVec3,
        lines: &[(Text, f64)],
        text_alpha: u32,
        background: bool,
        text_layer: u32,
        light: u32,
        unit: f32,
        out: &mut Vec<Vertex>,
        marks: Marks,
        mut layouts: Option<&mut NameTagLayouts>,
    ) -> Vec<horizon_client::font::Patch> {
        // Options.getBackgroundOpacity(0.25) black, as ARGB.color.
        const BACKGROUND: u32 = 0x3F00_0000;
        // The camera's facing (Camera.rotation()), turned round in third-person
        // front view, so the text reads the right way from where it is seen.
        let (view_yaw, view_pitch) = match self.posed_rotation {
            // The frame's camera, already turned round in front view.
            Some(rotation) => rotation,
            None if self.camera_type == super::perspective::CameraType::ThirdPersonFront => {
                (self.yaw + 180.0, -self.pitch)
            }
            None => (self.yaw, self.pitch),
        };
        let forward = glam::DVec3::from_array(view_vector(view_yaw, view_pitch));
        let right = forward.cross(glam::DVec3::Y).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let away = forward.as_vec3() * 0.002;
        let mut batch = HudBatch::new([1, 1]);
        let last = lines.len().saturating_sub(1);
        for (n, (text, lift)) in lines.iter().enumerate() {
            let o = (o + glam::DVec3::new(0.0, *lift, 0.0)).as_vec3();
            let (r, u) = (right.as_vec3() * unit, up.as_vec3() * unit);
            // Text space: x right, y down (scale 0.025, -0.025).
            let at = |x: f32, y: f32| o + r * x - u * y;
            let y0 = if text.plain() == "deadmau5" {
                -10.0
            } else {
                0.0
            };
            batch.vertices.clear();
            // Font.prepareText, as any text: bold, italic, fonts and each
            // glyph's own box (accented.png's are 12 pixels tall).
            let behind = match layouts.as_deref_mut() {
                Some(layouts) => layouts.name_tag(
                    &mut batch,
                    &self.gui.atlas,
                    text,
                    y0,
                    text_alpha,
                    background,
                ),
                None => batch.name_tag(
                    &self.gui.atlas,
                    text,
                    y0,
                    text_alpha << 24 | 0xFF_FFFF,
                    if background { BACKGROUND } else { 0 },
                ),
            };
            if n == last && marks.horizon {
                let x = -self.gui.atlas.font.width_text(text) / 2.0 - 10.0;
                // 8 text pixels square, a pixel left of the background.
                for (mx, my, w, h, argb) in horizon_cosmetics::mark::MARK {
                    let a = text_alpha * (argb >> 24) / 255;
                    batch.fill(
                        &self.gui.atlas,
                        x + f32::from(mx),
                        y0 + f32::from(my),
                        f32::from(w),
                        f32::from(h),
                        horizon_client::hud_layout::abgr(a << 24 | argb & 0xFF_FFFF),
                    );
                }
            }
            if let Some(style) = marks.style.filter(|_| n == last) {
                let width = self.gui.atlas.font.width_text(text);
                let x = -width / 2.0;
                let alpha = text_alpha << 24;
                // The badge, 8 text pixels square, a pixel right of the background.
                batch.rect_uv(x + width + 2.0, y0, 8.0, 8.0, style.mark, alpha | 0xFF_FFFF);
                // The line, a pixel tall, under the background (which runs from -1 to 9).
                batch.fill(
                    &self.gui.atlas,
                    x - 1.0,
                    y0 + 9.0,
                    width + 1.0,
                    1.0,
                    // The batch takes RGBA: the line as ARGB, swapped.
                    horizon_client::hud_layout::abgr(alpha | style.line),
                );
            }
            for (i, quad) in batch.vertices.as_chunks::<6>().0.iter().enumerate() {
                let shift = if i * 6 < behind {
                    away
                } else {
                    glam::Vec3::ZERO
                };
                // The corners from the bottom left, as the world draws quads.
                for v in [quad[5], quad[2], quad[1], quad[0]] {
                    out.push(Vertex {
                        normal: [0; 4],
                        pos: (at(v.pos[0], v.pos[1]) + shift).to_array(),
                        uv: v.uv,
                        layer: text_layer,
                        color: v.color,
                        light,
                        glint_uv: [0.0, 0.0],
                    });
                }
            }
        }
        // Glyphs rasterized on first use (Unifont's) upload with the frame.
        batch.patches
    }
}

/// Use the renderer's camera-relative clip test before measuring or emitting text.
fn name_tag_in_view(
    frustum: Option<&impl horizon_client::gfx::Cull>,
    distance_sqr: f64,
    min: glam::Vec3,
    max: glam::Vec3,
) -> bool {
    distance_sqr < 64.0 * 64.0 && frustum.is_none_or(|view| view.intersects(min, max))
}

fn first_mob_indices(mobs: &[Mob]) -> std::collections::HashMap<u32, usize> {
    let mut indices = std::collections::HashMap::with_capacity(mobs.len());
    for (index, mob) in mobs.iter().enumerate() {
        // Match iter().find even if a transient snapshot contains duplicate IDs.
        indices.entry(mob.id).or_insert(index);
    }
    indices
}

/// Collect entity IDs of all vehicles: entities that have passengers
/// (`passenger_vehicles`: what every entity the server sent rides, text displays,
/// armor stands, item displays and interactions included; vanilla's `isVehicle` is
/// any passenger at all).
///
/// On multiplayer servers (e.g. `mcpvp.club`), server plugins hang custom nametags
/// (ranks, prefixes, stats) over players using `TextDisplay` or `ArmorStand` passengers.
/// In vanilla `LivingEntityRenderer.shouldShowName`, a vehicle does not show its default
/// name tag (`!entity.isVehicle()`).
///
/// If the local player's remote entity ID is in the set, ID 0 (`player_model`) is also
/// added so that Horizon's own name tag extra (`ownNameTag`) recognizes that the player
/// is ridden and suppresses the duplicate tag (#9477).
fn collect_vehicle_ids(
    mobs: &[Mob],
    passenger_vehicles: impl IntoIterator<Item = u32>,
    local_riding: Option<u32>,
    local_entity_id: Option<u32>,
) -> std::collections::HashSet<u32> {
    let mut ridden: std::collections::HashSet<u32> = mobs
        .iter()
        .filter_map(|o| o.extra.mount)
        .chain(mobs.iter().filter_map(|m| m.extra.rider.map(|_| m.id)))
        .chain(passenger_vehicles)
        .collect();
    if let Some(riding) = local_riding {
        ridden.insert(riding);
    }
    if let Some(id) = local_entity_id
        && ridden.contains(&id)
    {
        // The local player's model has id 0 (player_model).
        ridden.insert(0);
    }
    ridden
}

/// The entity ids the player-list pass names, which the custom-name pass must
/// therefore skip.
///
/// Vanilla draws one tag per entity, and names a player from its profile:
/// `Player.getName` overrides `Entity.getName` and ignores `customName`. So a
/// server that sets a custom name on a player entity -- a rank, a nickname, a
/// prefix -- must not get a tag of its own drawn over the one the player list
/// already draws.
///
/// Built whether or not native mods are loaded. It was briefly built only with
/// them, as a fast path, and without them both tags were drawn on top of each
/// other on every server that names its players this way. The set is small --
/// the players in view -- and `other_players` has already allocated a name
/// string for each, so skipping it saved very little.
fn named_by_player_list(players: &[(&Mob, String)]) -> std::collections::HashSet<u32> {
    players.iter().map(|(player, _)| player.id).collect()
}
/// `EntityAttachment.NAME_TAG`: `(0, bbHeight, 0)` of the box of the entity's
/// current pose, which the submitter lifts 0.5 more, between the last two tick
/// positions. The player box is `Pose::dimensions(1.0)`.
/// shortcut: the scale attribute is not in the tag height, add `model_scale`
/// when tags on scaled players matter.
pub(super) fn name_tag_at(m: &Mob, alpha: f64) -> glam::DVec3 {
    glam::DVec3::new(
        m.previous.x + (m.position.x - m.previous.x) * alpha,
        m.previous.y + (m.position.y - m.previous.y) * alpha + f64::from(m.height()) + 0.5,
        m.previous.z + (m.position.z - m.previous.z) * alpha,
    )
}

/// Horizon: how far a player's name tag goes up so that it clears the tallest cosmetic drawn on
/// them, `top` pixels above the model's neck (`Item::top`) on a player of `scale`
/// (`LivingEntity.getScale`); 0 when the item stays under vanilla's tag. Measured standing: the
/// model's neck is 1.501 blocks up, times `AvatarRenderer.scale`'s 0.9375 and `scale`; the tag's
/// lowest pixel (the style's line under the background, 10 text pixels under the anchor) is at
/// 1.8 + 0.5 - 10 × 0.025 = 2.05 blocks, and the lift leaves one text pixel between them. Other
/// poses keep the same lift: crouching lowers the model more (0.39 blocks) than the tag (0.3).
pub(super) fn cosmetic_lift(top: f32, scale: f32) -> f64 {
    let item = f64::from(scale) * 0.9375 * (1.501 + f64::from(top) / 16.0);
    let unit = f64::from(NAME_TAG_UNIT);
    let bottom = 1.8 + 0.5 - 10.0 * unit;
    (item + unit - bottom).max(0.0)
}

/// A name tag: where it hangs, its lines (text, lift), the packed light at
/// the entity and whether it is discrete.
struct NameTag {
    at: glam::DVec3,
    lines: Vec<(Text, f64)>,
    light: u8,
    discrete: bool,
    marks: Marks,
}

/// What Horizon draws by a player's name: its mark (they play Horizon) and their name tag style.
#[derive(Clone, Copy, Default)]
pub(super) struct Marks {
    pub horizon: bool,
    pub style: Option<TagStyle>,
    /// How high the cosmetics drawn on their body get above the neck, in model pixels: the tag
    /// goes up over a tall one (`cosmetic_lift`).
    pub top: Option<f32>,
}

/// A cosmetic name tag style: a badge after the name and a line under it.
#[derive(Clone, Copy)]
pub(super) struct TagStyle {
    /// The mark's place in the GUI atlas.
    pub mark: horizon_client::hud::Rect,
    /// The line's colour, `0xrrggbb`.
    pub line: u32,
}

/// `Entity.DATA_CUSTOM_NAME` of a mob named `name`: its component with its style (a name
/// summoned or set as `{text: "..", color: "red"}` keeps its colour), else the plain text.
pub(super) fn name_component(m: &Mob, name: &str) -> horizon_world::nbt::Tag {
    match &m.custom_name_text {
        Some(text) => horizon_client::scoreboard::json_to_tag(&text.component()),
        None => horizon_protocol::text::literal(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::mobs::{Anim, Brain, MobType, Part, Species};
    use horizon_client::pose::Pose;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    #[test]
    fn a_mobs_name_is_sent_with_its_style() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let Ok(types) = horizon_client::mobs::MobTypes::load(&root) else {
            return;
        };
        let cow = Arc::new(types.types["minecraft:cow"].clone());
        let mut m = Mob::new(1, cow, Vec3::new(0.0, 64.0, 0.0), 0.0);
        m.custom_name = Some("RED NAME".into());
        let plain = horizon_protocol::text::literal("RED NAME");
        assert_eq!(name_component(&m, "RED NAME"), plain);
        let styled = horizon_world::nbt::Tag::compound([
            ("text", horizon_world::nbt::Tag::String("RED NAME".into())),
            ("color", horizon_world::nbt::Tag::String("red".into())),
        ]);
        m.custom_name_text = Some(horizon_client::entity_nbt::name_text(&styled));
        let sent = horizon_client::entity_nbt::name_text(&name_component(&m, "RED NAME"));
        assert_eq!(sent, horizon_client::entity_nbt::name_text(&styled));
        assert_ne!(sent, horizon_client::entity_nbt::name_text(&plain));
    }

    fn tag_font() -> HudAtlas {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "horizon-tag-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let textures = root.join("assets/minecraft/textures/font");
        std::fs::create_dir_all(root.join("assets/minecraft/font")).unwrap();
        std::fs::create_dir_all(&textures).unwrap();
        std::fs::write(root.join("assets/minecraft/font/default.json"),
            r#"{"providers":[{"type":"bitmap","file":"minecraft:font/cache.png","ascent":7,"chars":["ab"]}]}"#).unwrap();
        let file = std::fs::File::create(textures.join("cache.png")).unwrap();
        let mut encoder = png::Encoder::new(file, 16, 8);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..128)
            .flat_map(|i| [255, 255, 255, if i % 8 < 4 { 255 } else { 0 }])
            .collect();
        writer.write_image_data(&pixels).unwrap();
        drop(writer);
        let mut atlas = HudAtlas::boot();
        atlas.font = horizon_client::font::Font::load(
            &root,
            &Default::default(),
            &[],
            (1024, 1024),
            (512, 512),
            atlas.sprites["horizon/white"],
        );
        std::fs::remove_dir_all(root).unwrap();
        atlas
    }

    fn tag_bytes(
        cache: &mut NameTagLayouts,
        atlas: &HudAtlas,
        text: &Text,
        alpha: u32,
        background: bool,
    ) -> Vec<u8> {
        cache.begin_frame(atlas);
        let mut batch = HudBatch::new([1, 1]);
        let behind = cache.name_tag(&mut batch, atlas, text, 0.0, alpha, background);
        cache.end_frame();
        let mut fresh = HudBatch::new([1, 1]);
        assert_eq!(
            behind,
            fresh.name_tag(
                atlas,
                text,
                0.0,
                alpha << 24 | 0xFF_FFFF,
                if background { 0x3F00_0000 } else { 0 }
            )
        );
        let bytes: &[u8] = bytemuck::cast_slice(&batch.vertices);
        assert_eq!(bytes, bytemuck::cast_slice::<_, u8>(&fresh.vertices));
        bytes.to_vec()
    }

    #[test]
    fn cached_tag_quads_follow_text_style_pass_and_font_changes() {
        let mut atlas = tag_font();
        let mut cache = NameTagLayouts::default();
        let a = Text::literal("a");
        let first = tag_bytes(&mut cache, &atlas, &a, 255, false);
        assert!(!first.is_empty());
        assert_eq!(tag_bytes(&mut cache, &atlas, &a, 255, false), first);
        for text in [
            Text::literal("b"),
            a.clone().bold(),
            Text::colored("a", 0x12AB34),
        ] {
            assert_ne!(tag_bytes(&mut cache, &atlas, &text, 255, false), first);
            tag_bytes(&mut cache, &atlas, &text, 255, false);
        }
        assert_ne!(tag_bytes(&mut cache, &atlas, &a, 127, true), first);
        tag_bytes(&mut cache, &atlas, &a, 127, true);
        tag_bytes(&mut cache, &atlas, &a, 255, false);
        // Reset and occupy the first glyph slot with b: a's old UV is now wrong.
        atlas.font.reset_cache();
        atlas.font.glyph('b');
        assert_ne!(tag_bytes(&mut cache, &atlas, &a, 255, false), first);
        // Font options may change without replacing the atlas.
        atlas.font.uniform = true;
        tag_bytes(&mut cache, &atlas, &a, 255, false);
    }

    #[test]
    fn obfuscated_tag_glyphs_keep_animating_after_cache_warmup() {
        let atlas = tag_font();
        let mut cache = NameTagLayouts::default();
        let text = Text::legacy("§kaaaaaaaaaaaaaaaa");
        let mut images = std::collections::HashSet::new();
        for seed in 0..8 {
            cache.begin_frame(&atlas);
            atlas.font.reseed(seed);
            let mut batch = HudBatch::new([1, 1]);
            cache.name_tag(&mut batch, &atlas, &text, 0.0, 255, false);
            cache.end_frame();
            let bytes = bytemuck::cast_slice::<_, u8>(&batch.vertices).to_vec();
            atlas.font.reseed(seed);
            let mut fresh = HudBatch::new([1, 1]);
            fresh.name_tag(&atlas, &text, 0.0, u32::MAX, 0);
            assert_eq!(bytes, bytemuck::cast_slice::<_, u8>(&fresh.vertices));
            images.insert(bytes);
        }
        assert!(images.len() > 1, "the fixture must actually animate");
    }

    #[test]
    fn inline_tag_objects_follow_live_sprite_updates() {
        let mut atlas = HudAtlas::boot();
        let mut sprite = atlas.sprites["horizon/white"];
        atlas.sprites.insert("item/probe".into(), sprite);
        let text = horizon_client::component::to_text(
            &serde_json::json!({"object":"atlas", "sprite":"minecraft:item/probe"}),
            &mut |_| Text::empty(),
        );
        let mut cache = NameTagLayouts::default();
        let first = tag_bytes(&mut cache, &atlas, &text, 255, false);
        assert!(!first.is_empty());
        sprite.u0 += 0.1;
        atlas.sprites.insert("item/probe".into(), sprite);
        assert_ne!(tag_bytes(&mut cache, &atlas, &text, 255, false), first);
    }

    #[test]
    fn name_tag_culling_keeps_intersecting_entity_boxes_within_distance() {
        // A 90-degree camera looking down -Z, with near/far at 1/100.
        struct View;
        impl horizon_client::gfx::Cull for View {
            fn intersects(&self, lo: glam::Vec3, hi: glam::Vec3) -> bool {
                lo.z <= -1.0
                    && hi.z >= -100.0
                    && lo.x + lo.z <= 0.0
                    && hi.x - lo.z >= 0.0
                    && lo.y + lo.z <= 0.0
                    && hi.y - lo.z >= 0.0
            }
            fn contains(&self, lo: glam::Vec3, hi: glam::Vec3) -> bool {
                self.intersects(lo, hi)
            }
        }
        // The head/anchor at y=12 is outside the top plane, but the
        // entity's feet are inside: vanilla still submits its name.
        assert!(name_tag_in_view(
            Some(&View),
            181.0,
            glam::Vec3::new(-0.8, 8.5, -10.8),
            glam::Vec3::new(0.8, 11.3, -9.2)
        ));
        for (offset, distance, expected) in [
            ([0.0, 0.0, -10.0], 100.0, true),
            ([0.0, 0.0, 10.0], 100.0, false),
            ([12.0, 0.0, -10.0], 244.0, false),
            ([0.0, 12.0, -10.0], 244.0, false),
            ([0.0, 0.0, -65.0], 4225.0, false),
            ([0.0, 0.0, -64.0], 4096.0, false),
            ([0.0, 0.0, -63.0], 3969.0, true),
        ] {
            assert_eq!(
                name_tag_in_view(
                    Some(&View),
                    distance,
                    glam::Vec3::from_array(offset) - glam::Vec3::splat(0.5),
                    glam::Vec3::from_array(offset) + glam::Vec3::splat(0.5)
                ),
                expected
            );
        }
    }

    #[test]
    fn name_tag_id_lookup_matches_scan_on_randomized_duplicate_lists() {
        let mut seed = 4664_u32;
        for len in [0, 1, 10, 150, 300] {
            let mut mobs = Vec::new();
            for _ in 0..len {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let mut mob = player_mob();
                mob.id = seed % 80;
                mobs.push(mob);
            }
            let indices = first_mob_indices(&mobs);
            for id in 0..100 {
                assert_eq!(
                    indices.get(&id).copied(),
                    mobs.iter().position(|m| m.id == id)
                );
            }
        }
    }

    /// The player model `perspective::player_type` builds from the prepared
    /// resources, without them: its box only needs the id and the pointer.
    fn player_mob() -> Mob {
        let part = Part {
            name: "root".into(),
            pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            polygons: Vec::new(),
            children: Vec::new(),
        };
        let kind = MobType {
            id: "minecraft:player".into(),
            species: Species::Zombie,
            width: 0.6,
            height: 1.8,
            eye_height: 1.62,
            category: "misc".into(),
            max_health: 20.0,
            speed: 0.1,
            flying_speed: 0.02,
            attack_damage: 1.0,
            attack_knockback: 0.0,
            follow_range: 0.0,
            step_height: 0.6,
            armor: 0.0,
            armor_toughness: 0.0,
            knockback_resistance: 0.0,
            jump_strength: 0.42,
            attributes: Default::default(),
            item_stack_limits: Default::default(),
            gravity: 0.08,
            safe_fall_distance: 3.0,
            fall_damage_multiplier: 1.0,
            water_efficiency: 0.0,
            water_slowdown: 0.8,
            air_speed: 0.02,
            baby_scale: 0.5,
            head_rot_speed: 10.0,
            max_head_x_rot: 40.0,
            max_head_y_rot: 75.0,
            ambient_interval: 80,
            flags: Default::default(),
            model: Arc::new(part),
            layers: Default::default(),
            texture: String::new(),
            brain: Brain::Passive,
            anim: Anim::Humanoid { zombie_arms: false },
            sounds: [
                "entity.player.hurt",
                "entity.player.hurt",
                "entity.player.death",
            ],
            step_sound: None,
            food: Default::default(),
            tempt: Default::default(),
        };
        Mob::new(0, Arc::new(kind), Vec3::ZERO, 0.0)
    }

    /// A server that names its players -- a rank prefix, a nickname -- sets a
    /// custom name on the player entity. The player list already names that
    /// player, so the custom-name pass has to skip it or the two tags are drawn
    /// on top of each other. The set of already-named players was briefly built
    /// only when native mods were loaded, so without mods every such server
    /// showed the pair overlapping.
    ///
    /// An entity the player list does not name keeps its own tag: that is how a
    /// shopkeeper NPC spawned as a player entity is named at all.
    #[test]
    fn a_listed_player_is_named_once_whatever_custom_name_a_server_sets() {
        let mut listed = player_mob();
        listed.id = 7;
        listed.custom_name = Some("[MVP+] Steve".to_owned());
        let mut unlisted = player_mob();
        unlisted.id = 9;
        unlisted.custom_name = Some("Shopkeeper".to_owned());
        // The player list names 7 and knows nothing of 9.
        let players = vec![(&listed, "Steve".to_owned())];
        let named = named_by_player_list(&players);
        assert!(
            named.contains(&listed.id),
            "the player list names 7, so its custom name must not add a second tag"
        );
        assert!(
            !named.contains(&unlisted.id),
            "9 is named by nothing else, so its custom name is its only tag"
        );
    }

    /// #4129: the tag hangs from the box of the pose (0.6 swimming, 1.5
    /// crouching, 1.8 standing) and never from the sneak flag, which a
    /// sprint-swimming or gliding player still has set. Today's
    /// `if player && m.extra.crouching { 1.5 }` gives 2.0 for the first.
    #[test]
    fn a_name_tag_hangs_from_the_pose_box_not_the_sneak_flag() {
        let mut m = player_mob();
        let at_y = |m: &Mob| name_tag_at(m, 1.0).y;
        m.extra.crouching = true;
        m.extra.avatar_pose = Pose::Swimming;
        assert!((at_y(&m) - 1.1).abs() < 1e-6, "0.6 + 0.5, not 2.0");
        // A crouching player's tag keeps the height it had: 1.5 + 0.5.
        m.extra.avatar_pose = Pose::Crouching;
        assert!((at_y(&m) - 2.0).abs() < 1e-6, "1.5 + 0.5, as before");
        m.extra.avatar_pose = Pose::Standing;
        m.extra.crouching = false;
        assert!((at_y(&m) - 2.3).abs() < 1e-6, "1.8 + 0.5");
    }

    /// #9477: When server plugins attach a TextDisplay or ArmorStand to a player
    /// (e.g. rank prefixes on multiplayer lobbies), vanilla's `LivingEntityRenderer.shouldShowName`
    /// hides the default nametag because `entity.isVehicle()` is true.
    ///
    /// For the local player with `ownNameTag: true`, `player_model` has ID 0 while
    /// the server sends passengers for `local_entity_id`. `collect_vehicle_ids`
    /// maps `local_entity_id -> 0` so that `ownNameTag` is not drawn twice over the
    /// server's rider nametag.
    #[test]
    fn vehicle_with_text_display_or_armor_stand_passenger_is_marked_ridden() {
        let mut remote_player = player_mob();
        remote_player.id = 42;
        let mobs = vec![remote_player];

        // 1. TextDisplay riding remote player 42
        let ridden = collect_vehicle_ids(&mobs, vec![42], None, Some(100));
        assert!(
            ridden.contains(&42),
            "remote player 42 is ridden by text display"
        );
        assert!(!ridden.contains(&100), "local player 100 has no passenger");
        assert!(!ridden.contains(&0), "local model 0 has no passenger");

        // 2. ArmorStand riding local player 100 -> model 0 must be in ridden
        let ridden_local = collect_vehicle_ids(&mobs, vec![100], None, Some(100));
        assert!(
            ridden_local.contains(&100),
            "local player 100 is ridden by armor stand"
        );
        assert!(
            ridden_local.contains(&0),
            "local player model (id 0) must be mapped to vehicle so ownNameTag is suppressed (#9477)"
        );

        // 3. Name tag is suppressed for vehicles when team is None
        let view_vehicle = sb::NameView {
            is_vehicle: true,
            ..Default::default()
        };
        assert!(
            !sb::name_shown(None, None, view_vehicle),
            "vanilla shouldShowName hides tag for vehicles with no team"
        );
    }
}
