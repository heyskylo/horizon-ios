use super::Game;
use super::native_hooks::{NativeBlockRim, NativeColoredLight};
use horizon_client::{dynamic_light, gizmos};

/// What one mod's visual requests may use of a frame, whatever other mods ask for: its first
/// `RIMS_PER_MOD` rims and `LIGHTS_PER_MOD` lights are examined, and its valid rims draw at most
/// `RIM_EDGES_PER_MOD` outline edges. Requests bundles' injections push share one budget.
pub(super) const RIMS_PER_MOD: usize = 256;
pub(super) const RIM_EDGES_PER_MOD: usize = 4096;
pub(super) const LIGHTS_PER_MOD: usize = 8;

fn rim_target_valid(
    loaded: bool,
    state: horizon_world::registry::BlockStateId,
    air: horizon_world::registry::BlockStateId,
) -> bool {
    loaded && state != air
}

/// One mod's rims within its budget: the first `RIMS_PER_MOD`, of which those `edges` accepts
/// (it says how many outline edges a rim draws, None to refuse it), while they fit in
/// `RIM_EDGES_PER_MOD` edges. A rim too big for what is left is skipped whole.
fn budget_rims(
    rims: &mut Vec<NativeBlockRim>,
    edges: impl Fn(&NativeBlockRim) -> Option<usize>,
    out: &mut Vec<NativeBlockRim>,
) {
    let mut used = 0;
    for rim in rims.drain(..).take(RIMS_PER_MOD) {
        if let Some(n) = edges(&rim)
            && used + n <= RIM_EDGES_PER_MOD
        {
            used += n;
            out.push(rim);
        }
    }
}

/// A rim's outline: its block's edges, placed in the world.
struct RimShape<'a> {
    origin: [f64; 3],
    offset: [f64; 3],
    /// A context outline's box (light, scaffolding), whose edges are the unit cube's scaled.
    box_shape: Option<[f64; 6]>,
    segments: &'a [[f64; 6]],
}

impl Game<'_> {
    /// Moves one mod's requests (`rims`, `lights`, emptied) into the frame's, within its own
    /// budget (`RIMS_PER_MOD`, `RIM_EDGES_PER_MOD`, `LIGHTS_PER_MOD`).
    pub(super) fn budget_visuals(
        &self,
        rims: &mut Vec<NativeBlockRim>,
        lights: &mut Vec<NativeColoredLight>,
        frame_rims: &mut Vec<NativeBlockRim>,
        frame_lights: &mut Vec<NativeColoredLight>,
    ) {
        let Some(player) = self.native_player() else {
            rims.clear();
            lights.clear();
            return;
        };
        let player = [player.x, player.y, player.z];
        let cube = horizon_client::blocks::cube_edges();
        budget_rims(
            rims,
            |rim| self.rim_shape(rim, player, &cube).map(|s| s.segments.len()),
            frame_rims,
        );
        frame_lights.extend(
            lights
                .drain(..)
                .take(LIGHTS_PER_MOD)
                .filter(|l| self.light_valid(l, player)),
        );
    }

    /// The outline a rim draws, or None when it is refused: a bad width, too far from the
    /// player, or not on a loaded, non-air block.
    fn rim_shape<'a>(
        &'a self,
        rim: &NativeBlockRim,
        player: [f64; 3],
        cube: &'a [[f64; 6]],
    ) -> Option<RimShape<'a>> {
        let pos = rim.pos;
        if !rim.width.is_finite()
            || !(0.5..=4.0).contains(&rim.width)
            || !close(pos.map(|v| f64::from(v) + 0.5), player)
        {
            return None;
        }
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let loaded = self
            .world
            .chunks
            .contains_key(&(pos[0].div_euclid(16), pos[2].div_euclid(16)));
        if !rim_target_valid(loaded, state, self.world.air) {
            return None;
        }
        let facts = self.res.facts().get(state);
        let offset = if facts.outline_offset {
            self.pack
                .context
                .tables
                .offset(state, pos[0], pos[2])
                .unwrap_or([0.0; 3])
        } else {
            [0.0; 3]
        };
        let box_shape = self.context_outline(pos, state);
        Some(RimShape {
            origin: pos.map(f64::from),
            offset,
            box_shape,
            segments: if box_shape.is_some() {
                cube
            } else {
                &facts.outline_edges
            },
        })
    }

    fn light_valid(&self, l: &NativeColoredLight, player: [f64; 3]) -> bool {
        close(l.at, player)
            && l.rgb <= 0xffffff
            && l.radius.is_finite()
            && (0.25..=16.0).contains(&l.radius)
            && l.intensity.is_finite()
            && (0.0..=1.0).contains(&l.intensity)
            && self.world.chunks.contains_key(&(
                (l.at[0].floor() as i32).div_euclid(16),
                (l.at[2].floor() as i32).div_euclid(16),
            ))
    }

    /// Frame-local requests never persist after a mod is removed or a world changes.
    pub(super) fn native_frame_visuals(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        lines: &mut Vec<gizmos::Line>,
    ) -> Vec<dynamic_light::Colored> {
        let mut particle_lights = self
            .renderer
            .as_ref()
            .and_then(|r| r.particle_lights())
            .map_or_else(Vec::new, |entries| {
                self.particles
                    .lights(entries, self.clock.camera_fraction() as f32)
            });
        // Held and dropped items whose block the pack's `[[lights]]` colour shine in that colour,
        // as far as their dynamic light reaches.
        if let Some(renderer) = self.renderer.as_ref().filter(|r| r.has_light_colors()) {
            for (source, item) in self.dynamic_light_sources(alpha) {
                let Some(color) = item.and_then(|i| renderer.light_color(i)) else {
                    continue;
                };
                let top = color[0].max(color[1]).max(color[2]);
                if source.level == 0 || top <= 0.0 {
                    continue;
                }
                let [r, g, b] = color.map(|c| (c / top * 255.0).round() as u32);
                particle_lights.push((
                    source.at.to_array(),
                    r << 16 | g << 8 | b,
                    f32::from(source.level),
                    1.0,
                ));
            }
        }
        let Some(player) = self.native_player() else {
            return dynamic_light::select_colored(particle_lights.into_iter(), camera);
        };
        let player = [player.x, player.y, player.z];
        // Already within each mod's budget (`budget_visuals`); checked again for what a
        // bundle's injection may add after it.
        let mut rims = Vec::new();
        let mut lights = Vec::new();
        self.native_visuals(&mut rims, &mut lights);
        let cube = horizon_client::blocks::cube_edges();
        for rim in &rims {
            let Some(shape) = self.rim_shape(rim, player, &cube) else {
                continue;
            };
            for edge in shape.segments {
                let point = |end: usize| {
                    std::array::from_fn(|i| {
                        let v = edge[i + 3 * end];
                        if let Some(b) = shape.box_shape {
                            shape.origin[i] + b[i] + v * (b[i + 3] - b[i])
                        } else {
                            shape.origin[i] + shape.offset[i] + v
                        }
                    })
                };
                lines.push(gizmos::Line {
                    start: point(0),
                    end: point(1),
                    color: rim.argb,
                    width: rim.width,
                });
            }
        }
        let sources = lights.into_iter().filter(|l| self.light_valid(l, player));
        dynamic_light::select_colored(
            sources
                .map(|l| (l.at, l.rgb, l.radius, l.intensity))
                .chain(particle_lights),
            camera,
        )
    }
}

/// Within 32 blocks of the player, and finite.
fn close(at: [f64; 3], player: [f64; 3]) -> bool {
    at.iter().all(|v| v.is_finite())
        && (at[0] - player[0]).powi(2) + (at[1] - player[1]).powi(2) + (at[2] - player[2]).powi(2)
            <= 1024.0
}

#[cfg(test)]
mod tests {
    use super::{RIM_EDGES_PER_MOD, RIMS_PER_MOD, budget_rims, rim_target_valid};
    use crate::game::native_hooks::NativeBlockRim;
    use horizon_world::registry::BlockStateId;

    #[test]
    fn ordinary_loaded_blocks_can_have_rims() {
        let air = BlockStateId(0);
        let stone = BlockStateId(1); // No emission requirement.
        assert!(rim_target_valid(true, stone, air));
        assert!(!rim_target_valid(false, stone, air));
        assert!(!rim_target_valid(true, air, air));
    }

    fn rims(n: i32) -> Vec<NativeBlockRim> {
        (0..n)
            .map(|x| NativeBlockRim {
                pos: [x, 64, 0],
                argb: 0xFFFF_0000,
                width: 1.0,
            })
            .collect()
    }

    #[test]
    fn each_mod_gets_its_own_rims_and_edges() {
        let mut frame = Vec::new();
        // Two mods asking for 300 full cubes each: each keeps 256 of its own,
        // the second untouched by what the first used.
        for _ in 0..2 {
            let mut mine = rims(300);
            budget_rims(&mut mine, |_| Some(12), &mut frame);
            assert!(mine.is_empty());
        }
        assert_eq!(frame.len(), 2 * 256);
        assert_eq!(frame[255].pos[0], 255);
        assert_eq!(frame[256].pos[0], 0);
    }

    #[test]
    fn a_mods_edges_are_bounded_and_refused_rims_cost_nothing() {
        let mut frame = Vec::new();
        // 30-edge outlines: 4096 / 30 = 136 fit; refused odd rims don't count.
        let mut mine = rims(RIMS_PER_MOD as i32);
        budget_rims(&mut mine, |r| (r.pos[0] % 2 == 0).then_some(30), &mut frame);
        assert_eq!(frame.len(), 128);
        assert!(frame.iter().all(|r| r.pos[0] % 2 == 0));
        frame.clear();
        let mut mine = rims(RIMS_PER_MOD as i32);
        budget_rims(&mut mine, |_| Some(30), &mut frame);
        assert_eq!(frame.len(), RIM_EDGES_PER_MOD / 30);
    }
}
