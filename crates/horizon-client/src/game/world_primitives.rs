//! Native mods' world primitives (`native_world_primitives`): asked once a frame, tessellated
//! here camera-relative into quads from the shared atlas and gizmo lines, and drawn by the
//! renderer's own pipelines (`gfx::world_primitives`) after the entities, before the hand.

use super::Game;
use horizon_client::block_entity_render::{BeamShape, beam};
use horizon_client::gfx::world_primitives::Batch;
use horizon_client::gizmos::{self, Gizmos, Line};
use horizon_client::mesher::Vertex;
use horizon_client::text::Text;
use horizon_client::world_primitives::{
    MAX_BEAM_HEIGHT, PrimitiveShape, WorldPrimitives, text_unit, vertex_color,
};

/// A frame's primitives as the renderer draws them, depth tested and through walls.
#[derive(Default)]
pub(super) struct PrimitiveGeometry {
    quads: [Vec<Vertex>; 2],
    lines: [Vec<[f32; 8]>; 2],
}

impl PrimitiveGeometry {
    pub(super) fn batch(&self) -> Batch<'_> {
        Batch {
            quads: &self.quads[0],
            quads_through: &self.quads[1],
            lines: &self.lines[0],
            lines_through: &self.lines[1],
        }
    }
}

impl Game<'_> {
    /// Asks the mods for this frame's primitives and tessellates them, `alpha` ticks past the
    /// last tick, seen from `camera` through `view` (the camera's rotation).
    pub(super) fn world_primitive_geometry(
        &mut self,
        camera: glam::DVec3,
        alpha: f64,
        view: glam::Mat4,
    ) -> PrimitiveGeometry {
        let mut primitives = WorldPrimitives::default();
        self.native_world_primitives(alpha as f32, camera.to_array(), &mut primitives);
        let mut out = PrimitiveGeometry::default();
        if primitives.is_empty() {
            return out;
        }
        let atlas = &self.gui.atlas;
        let white = horizon_client::gfx::lightning::white_uv(&atlas.font.white());
        let beam_sprite = atlas.sprites.get("entity/beacon/beacon_beam").copied();
        // BeaconRenderer's animation time: game time mod 40 plus the partial tick.
        let time = self.world.game_time.rem_euclid(40) as f32 + alpha as f32;
        let relative = |p: [f64; 3]| (glam::DVec3::from_array(p) - camera).as_vec3();
        let mut lines: [Vec<Line>; 2] = Default::default();
        let mut texts = Vec::new();
        for primitive in primitives.iter() {
            let through = usize::from(primitive.through_walls);
            let quads = &mut out.quads[through];
            match &primitive.shape {
                PrimitiveShape::Line {
                    from,
                    to,
                    color,
                    width,
                } if finite(from) && finite(to) && width.is_finite() => {
                    lines[through].push(Line {
                        start: *from,
                        end: *to,
                        color: *color,
                        width: *width,
                    });
                }
                PrimitiveShape::BoxOutline {
                    min,
                    max,
                    color,
                    width,
                } if finite(min) && finite(max) && width.is_finite() => {
                    let mut edges = Gizmos::default();
                    edges.cuboid(*min, *max, *color, *width);
                    lines[through].extend(edges.lines);
                }
                PrimitiveShape::FilledBox { min, max, color } if finite(min) && finite(max) => {
                    let color = vertex_color(*color, 255);
                    for quad in gizmos::cuboid_fill_quads(*min, *max) {
                        quads.extend(quad.map(|corner| plain(relative(corner), white, color)));
                    }
                }
                PrimitiveShape::Beam {
                    base,
                    height,
                    color,
                } if finite(base) => {
                    let Some(sprite) = beam_sprite else {
                        continue;
                    };
                    // `beam` draws around the block's centre, half a block in from its corner.
                    let corner = relative(*base) - glam::Vec3::new(0.5, 0.0, 0.5);
                    let start = quads.len();
                    beam(
                        glam::Mat4::from_translation(corner),
                        BeamShape {
                            scale: 1.0,
                            time,
                            start: 0,
                            height: (*height).min(MAX_BEAM_HEIGHT) as i32,
                            color: color & 0xFF_FFFF,
                            solid_radius: 0.2,
                            glow_radius: 0.25,
                        },
                        sprite,
                        quads,
                    );
                    // The colour's alpha scales the beacon's own (the core's 255, the glow's 32).
                    for v in &mut quads[start..] {
                        let a = (v.color >> 24) * (color >> 24) / 255;
                        v.color = a << 24 | v.color & 0xFF_FFFF;
                    }
                }
                PrimitiveShape::Text {
                    at,
                    text,
                    color,
                    scale,
                    scale_with_distance,
                    background,
                } if finite(at) => {
                    let o = glam::DVec3::from_array(*at) - camera;
                    let unit = text_unit(*scale, *scale_with_distance, o.length());
                    if unit > 0.0 {
                        texts.push((through, o, text.clone(), *color, unit, *background));
                    }
                }
                _ => {}
            }
        }
        for (through, o, text, color, unit, background) in texts {
            // A line of text is 9 font pixels tall, hung below its anchor: lift it by half.
            let line = [(
                Text::colored(text, color & 0xFF_FFFF),
                f64::from(unit * 4.5),
            )];
            let patches = self.name_tag_quads(
                o,
                &line,
                color >> 24,
                background,
                0,
                0,
                unit,
                &mut out.quads[through],
            );
            self.map_patches.extend(patches);
        }
        let at = camera.to_array();
        out.lines = lines.map(|lines| gizmos::camera_relative(&lines, at, view));
        out
    }
}

fn finite(p: &[f64; 3]) -> bool {
    p.iter().all(|v| v.is_finite())
}

/// A vertex of a plain colour: the atlas's white texel.
fn plain(pos: glam::Vec3, white: [f32; 2], color: u32) -> Vertex {
    Vertex {
        pos: pos.to_array(),
        uv: white,
        layer: 0,
        color,
        light: 0,
        normal: [0; 4],
        glint_uv: [0.0, 0.0],
    }
}
