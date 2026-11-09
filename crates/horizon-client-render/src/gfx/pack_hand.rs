//! The first-person hand and held items drawn by a shader pack's `gbuffers_hand` (spec 2.3):
//! `Frame::hand`'s entity vertices, read as entities are (`pack_entities::inputs`), with the
//! hand's own projection as `gl_ProjectionMatrix` (5.2: `gbufferProjection` stays the world's).

use glam::Mat4;

use super::Vertex;

/// `MC_HAND_DEPTH`'s value (1.5): the hand's depth is scaled by it through its projection, so
/// that it sits in front of the world in the shared depth buffer.
const HAND_DEPTH: f32 = 0.125;

/// `HzHand`: the hand's projection and its inverse.
const UNIFORM: u64 = 2 * 64;

/// `HzDraw`'s size (pack_draw.rs).
const SLOT: u64 = 32;

/// What `gbuffers_hand` reads: the entity inputs, and the hand's projection from set 1.
pub fn inputs() -> horizon_shaders::GbuffersInputs {
    let mut inputs = super::pack_entities::inputs();
    // No last frame's hand to measure from: the hand's `at_velocity` reads zero (and its set 1
    // has no previous vertices).
    for (name, value) in &mut inputs.attributes {
        if name == "at_velocity" {
            *value = "vec3(0.0)".to_owned();
        }
    }
    // Iris 1.11.7 declares `currentRenderedItemId` in the hand's programs (observed).
    inputs.item_id_declared = true;
    inputs.declarations.push_str(
        "layout(set = 1, binding = 1) uniform HzHand { mat4 hz_hand_projection; mat4 hz_hand_projection_inverse; };\n",
    );
    inputs.body.push_str(
        "    hz_ProjectionMatrix = hz_hand_projection;\n    \
         hz_ProjectionMatrixInverse = hz_hand_projection_inverse;\n    \
         hz_ModelViewProjectionMatrix = hz_hand_projection * hz_ModelViewMatrix;\n",
    );
    inputs
}

/// The hand's projection in GL's conventions: `hud_fov` degrees, depth scaled by
/// `MC_HAND_DEPTH` (the view bob is in `gbufferModelView`).
pub fn projection(hud_fov: f32, aspect: f32, far: f32) -> Mat4 {
    Mat4::from_scale(glam::Vec3::new(1.0, 1.0, HAND_DEPTH))
        * Mat4::perspective_rh_gl(hud_fov.to_radians(), aspect, 0.05, far)
}

/// The hand's vertices (also read at set 1 for normals and tangents), its uniform and quad
/// indices.
pub struct PackHand {
    pub layout: wgpu::BindGroupLayout,
    vertices: wgpu::Buffer,
    uniform: wgpu::Buffer,
    pub group: wgpu::BindGroup,
    pub indices: wgpu::Buffer,
    capacity: u32,
    /// Quads uploaded this frame.
    pub quads: u32,
    /// `HzDraw` (set 2) of each run: the arm or one held item, and its `currentRenderedItemId`.
    slots: wgpu::Buffer,
    pub slot_group: wgpu::BindGroup,
    stride: u64,
    /// This frame's runs: first quad, quads, and the dynamic offset of their slot. The opaque
    /// quads come first in the buffer, then the translucent ones (`translucent_runs`), which a
    /// pack's `gbuffers_hand_water` draws after deferred.
    pub runs: Vec<(u32, u32, u32)>,
    pub translucent_runs: Vec<(u32, u32, u32)>,
    order: Vec<Vertex>,
}

impl PackHand {
    /// `draws`: the layout of `HzDraw` (pack_draw.rs).
    pub fn new(device: &wgpu::Device, draws: &wgpu::BindGroupLayout) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shader pack hand"),
            entries: &[
                entry(0, wgpu::BufferBindingType::Storage { read_only: true }),
                entry(1, wgpu::BufferBindingType::Uniform),
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack hand"),
            size: UNIFORM,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let capacity = 256;
        let (vertices, group) = Self::vertex_buffer(device, &layout, &uniform, capacity);
        let stride = SLOT.max(u64::from(
            device.limits().min_uniform_buffer_offset_alignment,
        ));
        // The arm and the two hands' items: a few runs a frame.
        let slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack hand draws"),
            size: SLOTS * stride,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let slot_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader pack hand draws"),
            layout: draws,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &slots,
                    offset: 0,
                    size: wgpu::BufferSize::new(SLOT),
                }),
            }],
        });
        Self {
            indices: super::pack_entities::quad_indices(device, capacity),
            layout,
            vertices,
            uniform,
            group,
            capacity,
            quads: 0,
            slots,
            slot_group,
            stride,
            runs: Vec::new(),
            translucent_runs: Vec::new(),
            order: Vec::new(),
        }
    }

    fn vertex_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        quads: u32,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack hand vertices"),
            size: u64::from(quads) * 4 * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader pack hand"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        (buffer, group)
    }

    /// Uploads this frame's hand, its projection, and the `HzDraw` of each run `items` starts
    /// (first vertex, `currentRenderedItemId`; the slots past [`SLOTS`] share the last).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hand: &[Vertex],
        items: &[(u32, i32)],
        projection: Mat4,
    ) {
        self.quads = (hand.len() / 4) as u32;
        self.runs.clear();
        self.translucent_runs.clear();
        if self.quads == 0 {
            return;
        }
        let split = split(hand, items);
        self.order = split.order;
        let mut contents = Vec::new();
        for (i, &(first, quads, id, translucent)) in split.runs.iter().enumerate() {
            let slot = (i as u64).min(SLOTS - 1);
            if slot == i as u64 {
                contents.extend_from_slice(&draw_slot(id, translucent));
                contents.resize(((slot + 1) * self.stride) as usize, 0);
            }
            let run = (first, quads, (slot * self.stride) as u32);
            if translucent {
                self.translucent_runs.push(run);
            } else {
                self.runs.push(run);
            }
        }
        queue.write_buffer(&self.slots, 0, &contents);
        if self.quads > self.capacity {
            self.capacity = self.quads.next_power_of_two();
            (self.vertices, self.group) =
                Self::vertex_buffer(device, &self.layout, &self.uniform, self.capacity);
            self.indices = super::pack_entities::quad_indices(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&self.order));
        let matrices = [projection, projection.inverse()].map(|m| m.to_cols_array());
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&matrices));
    }

    pub fn vertices(&self) -> wgpu::BufferSlice<'_> {
        self.vertices.slice(..)
    }
}

/// The hand's runs a frame keeps slots for.
const SLOTS: u64 = 8;

/// The hand's quads reordered, opaque first and translucent after (each in their order), and
/// their runs: first quad, quads, `currentRenderedItemId`, translucent.
struct Split {
    order: Vec<Vertex>,
    runs: Vec<(u32, u32, i32, bool)>,
}

fn split(hand: &[Vertex], items: &[(u32, i32)]) -> Split {
    let quads = (hand.len() / 4) as u32;
    let mut ids = vec![0; quads as usize];
    for (start, n, id) in runs(quads, items) {
        ids[start as usize..(start + n) as usize].fill(id);
    }
    let kind = |q: usize| translucent_quad(&hand[q * 4..q * 4 + 4]);
    let opaque: Vec<usize> = (0..quads as usize).filter(|&q| !kind(q)).collect();
    let clear: Vec<usize> = (0..quads as usize).filter(|&q| kind(q)).collect();
    let mut out = Split {
        order: Vec::with_capacity(hand.len()),
        runs: Vec::new(),
    };
    for &q in opaque.iter().chain(&clear) {
        out.order.extend_from_slice(&hand[q * 4..q * 4 + 4]);
    }
    for (translucent, list, base) in [(false, &opaque, 0), (true, &clear, opaque.len())] {
        let mut i = 0;
        while i < list.len() {
            let id = ids[list[i]];
            let mut n = 1;
            while i + n < list.len() && ids[list[i + n]] == id {
                n += 1;
            }
            out.runs
                .push(((base + i) as u32, n as u32, id, translucent));
            i += n;
        }
    }
    out
}

/// A held item's quad drawn from a translucent sheet (`item_render`: its overlay layer).
fn translucent_quad(quad: &[Vertex]) -> bool {
    matches!(
        quad[0].layer & 0xffff,
        super::TRANSLUCENT_ITEM_OVERLAY | super::TRANSLUCENT_GLINT_ITEM_OVERLAY
    )
}

/// `HzDraw` of a hand run: `renderStage` the solid or translucent hand's, `blockEntityId` -1 as
/// outside block entities, and `currentRenderedItemId`.
fn draw_slot(id: i32, translucent: bool) -> [u8; SLOT as usize] {
    let mut bytes = [0u8; SLOT as usize];
    let stage = horizon_shaders::render_stage(if translucent {
        "HAND_TRANSLUCENT"
    } else {
        "HAND_SOLID"
    }) as i32;
    bytes[0..4].copy_from_slice(&stage.to_le_bytes());
    bytes[8..12].copy_from_slice(&(-1i32).to_le_bytes());
    bytes[12..16].copy_from_slice(&id.to_le_bytes());
    bytes
}

/// The runs of `quads` quads that `items` starts (first vertex, id), as first quad, quads and
/// id; geometry before the first start, or with no starts at all, reads 0, as the arm does.
fn runs(quads: u32, items: &[(u32, i32)]) -> Vec<(u32, u32, i32)> {
    let mut starts: Vec<(u32, i32)> = vec![(0, 0)];
    for &(start, id) in items {
        let q = (start / 4).min(quads);
        match starts.last_mut() {
            Some(last) if last.0 == q => last.1 = id,
            _ => starts.push((q, id)),
        }
    }
    (0..starts.len())
        .map(|i| {
            let end = starts.get(i + 1).map_or(quads, |s| s.0);
            (starts[i].0, end - starts[i].0, starts[i].1)
        })
        .filter(|r| r.1 > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arm, then an item whose middle quad is translucent, then a second item: the opaque
    /// quads keep their order first, the translucent one comes after them, and each run keeps
    /// its item's id.
    #[test]
    fn translucent_quads_go_after_the_opaque_ones() {
        let quad = |layer: u32, tag: f32| {
            [Vertex {
                layer,
                pos: [tag, 0.0, 0.0],
                ..bytemuck::Zeroable::zeroed()
            }; 4]
        };
        let glass = super::super::TRANSLUCENT_ITEM_OVERLAY;
        let hand: Vec<Vertex> = [
            quad(0, 0.),
            quad(0, 1.),
            quad(glass, 2.),
            quad(0, 3.),
            quad(0, 4.),
        ]
        .concat();
        // The arm is quad 0; item 7 quads 1..4; item 9 quad 4.
        let split = split(&hand, &[(4, 7), (16, 9)]);
        let order: Vec<f32> = split.order.chunks(4).map(|q| q[0].pos[0]).collect();
        assert_eq!(order, [0., 1., 3., 4., 2.]);
        assert_eq!(
            split.runs,
            [
                (0, 1, 0, false),
                (1, 2, 7, false),
                (3, 1, 9, false),
                (4, 1, 7, true)
            ]
        );
    }

    /// Each item's quads draw with its id; geometry before the first item is the arm's (0).
    #[test]
    fn runs_split_the_hand_by_item() {
        assert_eq!(runs(3, &[]), [(0, 3, 0)]);
        assert_eq!(runs(5, &[(0, 11), (8, 65535)]), [(0, 2, 11), (2, 3, 65535)]);
        assert_eq!(runs(4, &[(4, 21)]), [(0, 1, 0), (1, 3, 21)]);
        // An item that drew nothing gives way to the next.
        assert_eq!(runs(2, &[(0, 11), (0, 22)]), [(0, 2, 22)]);
    }

    /// The hand's window depth lies within `MC_HAND_DEPTH` of the middle (packs tell the hand
    /// apart by it), and x, y match the plain projection.
    #[test]
    fn hand_depth_is_scaled_around_the_middle() {
        let p = projection(70.0, 16.0 / 9.0, 512.0);
        let plain = Mat4::perspective_rh_gl(70f32.to_radians(), 16.0 / 9.0, 0.05, 512.0);
        for z in [-0.06f32, -0.5, -3.0, -400.0] {
            let v = glam::Vec4::new(0.1, -0.2, z, 1.0);
            let (a, b) = (p * v, plain * v);
            let window = (a.z / a.w) * 0.5 + 0.5;
            assert!(
                (window - 0.5).abs() <= HAND_DEPTH * 0.5 + 1e-6,
                "{z}: {window}"
            );
            assert!((a.x / a.w - b.x / b.w).abs() < 1e-6);
        }
    }
}
