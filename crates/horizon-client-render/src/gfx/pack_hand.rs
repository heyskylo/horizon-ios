//! The first-person hand and held items drawn by a shader pack's `gbuffers_hand` (spec 2.3):
//! `Frame::hand`'s entity vertices, read as entities are (`pack_entities::inputs`), with the
//! hand's own projection as `gl_ProjectionMatrix` (5.2: `gbufferProjection` stays the world's).

use glam::Mat4;

use super::Vertex;
use super::pack_draw::EntitySpan;

/// `MC_HAND_DEPTH`'s value (1.5): the hand's depth is scaled by it through its projection, so
/// that it sits in front of the world in the shared depth buffer.
const HAND_DEPTH: f32 = 0.125;

/// `HzHand`: the hand's projection and its inverse.
const UNIFORM: u64 = 2 * 64;

/// What `gbuffers_hand` reads: the entity inputs, and the hand's projection from set 1.
pub fn inputs() -> horizon_shaders::GbuffersInputs {
    let mut inputs = super::pack_entities::inputs();
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

/// The hand's projection in GL's conventions: `hud_fov` degrees and depth scaled by
/// `MC_HAND_DEPTH`. The shared gbuffer model-view already supplies the camera bob.
pub fn projection(hud_fov: f32, aspect: f32, far: f32) -> Mat4 {
    Mat4::from_scale(glam::Vec3::new(1.0, 1.0, HAND_DEPTH))
        * Mat4::perspective_rh_gl(hud_fov.to_radians(), aspect, 0.05, far)
}

/// The hand's vertices (also read at set 1 for normals and tangents), its uniform and quad
/// indices.
pub struct HandDraw {
    pub indices: std::ops::Range<u32>,
    pub offset: u32,
}

pub struct PackHand {
    pub layout: wgpu::BindGroupLayout,
    vertices: wgpu::Buffer,
    uniform: wgpu::Buffer,
    pub group: wgpu::BindGroup,
    pub indices: wgpu::Buffer,
    capacity: u32,
    /// Quads uploaded this frame.
    pub quads: u32,
    pub draws: Vec<HandDraw>,
    pub draw_group: Option<wgpu::BindGroup>,
    draw_buffer: Option<wgpu::Buffer>,
    draw_capacity: usize,
    draw_stride: u64,
}

impl PackHand {
    pub fn new(device: &wgpu::Device) -> Self {
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
        Self {
            indices: super::pack_entities::quad_indices(device, capacity),
            layout,
            vertices,
            uniform,
            group,
            capacity,
            quads: 0,
            draws: Vec::new(),
            draw_group: None,
            draw_buffer: None,
            draw_capacity: 0,
            draw_stride: 32.max(u64::from(
                device.limits().min_uniform_buffer_offset_alignment,
            )),
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

    /// Uploads this frame's hand and its projection.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hand: &[Vertex],
        spans: &[EntitySpan],
        draw_layout: &wgpu::BindGroupLayout,
        projection: Mat4,
    ) {
        self.draws.clear();
        self.quads = (hand.len() / 4) as u32;
        if self.quads == 0 {
            return;
        }
        if self.quads > self.capacity {
            self.capacity = self.quads.next_power_of_two();
            (self.vertices, self.group) =
                Self::vertex_buffer(device, &self.layout, &self.uniform, self.capacity);
            self.indices = super::pack_entities::quad_indices(device, self.capacity);
        }
        let runs = item_draws(hand.len(), spans);
        if runs.len() > self.draw_capacity {
            self.draw_capacity = runs.len().next_power_of_two();
            let (buffer, group) = super::pack_entities::PackEntities::slot_buffer(
                device,
                draw_layout,
                self.draw_capacity as u64,
                self.draw_stride,
            );
            self.draw_buffer = Some(buffer);
            self.draw_group = Some(group);
        }
        let mut bytes = vec![0u8; runs.len() * self.draw_stride as usize];
        for (i, (span, range)) in runs.into_iter().enumerate() {
            let offset = i * self.draw_stride as usize;
            bytes[offset..offset + 32].copy_from_slice(&span.draw_bytes([0.0; 4]));
            self.draws.push(HandDraw {
                indices: range,
                offset: offset as u32,
            });
        }
        if let Some(buffer) = &self.draw_buffer {
            queue.write_buffer(buffer, 0, &bytes);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(hand));
        let matrices = [projection, projection.inverse()].map(|m| m.to_cols_array());
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&matrices));
    }

    pub fn vertices(&self) -> wgpu::BufferSlice<'_> {
        self.vertices.slice(..)
    }
}

/// First-person subdraws use their own actual stack IDs. Ranges are index-buffer
/// ranges over the original hand buffer, so splitting never duplicates geometry.
fn item_draws(len: usize, spans: &[EntitySpan]) -> Vec<(EntitySpan, std::ops::Range<u32>)> {
    let fallback = [EntitySpan {
        stage: horizon_shaders::render_stage("HAND_SOLID"),
        ..Default::default()
    }];
    let spans = if spans.is_empty() {
        &fallback[..]
    } else {
        spans
    };
    super::pack_entities::runs(len, spans)
        .filter(|(_, range)| !range.is_empty())
        .map(|(span, range)| {
            (
                span,
                (range.start as u32 / 4 * 6)..(range.end as u32 / 4 * 6),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_hands_keep_distinct_item_ids_and_cover_each_index_once() {
        let spans = [
            EntitySpan {
                start: 0,
                item_id: 42,
                ..Default::default()
            },
            EntitySpan {
                start: 8,
                item_id: 81,
                ..Default::default()
            },
        ];
        let draws = item_draws(12, &spans);
        assert_eq!(
            draws
                .iter()
                .map(|(span, range)| (span.item_id, range.clone()))
                .collect::<Vec<_>>(),
            [(42, 0..12), (81, 12..18)]
        );
        assert_eq!(item_draws(8, &[])[0].1, 0..12);
        assert!(item_draws(0, &[]).is_empty());
    }

    #[test]
    fn hand_model_view_applies_walking_bob_exactly_once() {
        let (fov, aspect, far) = (70.0_f32, 16.0 / 9.0, 512.0);
        let bob = Mat4::from_translation(glam::Vec3::new(0.04, -0.07, 0.0))
            * Mat4::from_rotation_z(0.035)
            * Mat4::from_rotation_x(0.02);
        let rotation = crate::camera::view_matrix(28.0, 17.0);
        let view = bob * rotation;
        let hand_projection = projection(fov, aspect, far);
        let native = Mat4::from_scale(glam::Vec3::new(1.0, 1.0, HAND_DEPTH))
            * Mat4::perspective_rh_gl(fov.to_radians(), aspect, 0.05, far)
            * bob
            * rotation;
        assert!((hand_projection * view).abs_diff_eq(native, 1e-5));
        assert!(
            !(hand_projection * bob * view).abs_diff_eq(native, 1e-4),
            "double bob should change the regression fixture"
        );
        assert!((hand_projection * hand_projection.inverse()).abs_diff_eq(Mat4::IDENTITY, 1e-5));
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
