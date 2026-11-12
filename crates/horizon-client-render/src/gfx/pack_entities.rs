//! Entities, block entities, translucent entity parts and particles drawn by a shader pack's
//! `gbuffers_entities`, `gbuffers_block`, `gbuffers_entities_translucent` and
//! `gbuffers_particles` (spec 2.3, 5.3, 5.4): the runs of `Frame::entities` the game marked (`EntitySpan`), each its
//! own draw with its `entityId` or `blockEntityId` and `entityColor` in a per-draw slot of
//! `HzDraw` (set 2). Horizon draws whatever the pack does not.

use std::ops::Range;

use horizon_shaders::render_stage;

use super::Vertex;
use super::pack_draw::EntitySpan;

/// `HzDraw`'s size (pack_draw.rs).
const SLOT: u64 = 32;

const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x3, 1 => Float32x2, 2 => Uint32, 3 => Unorm8x4, 4 => Uint8x4, 5 => Snorm8x4
];

/// The entity vertex: Horizon's own (`Vertex`).
pub fn buffers() -> [Option<wgpu::VertexBufferLayout<'static>>; 1] {
    [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    })]
}

/// What `gbuffers_entities` and `gbuffers_block` read from an entity vertex: camera-relative
/// position, the shared atlas's UV, colour, light (block, sky in 16ths) and the world-oriented
/// normal. The quad's corners, read from the same vertices at set 1, give the face normal where
/// the vertex has none (custom geometry) and `at_tangent` along the texture's u, as for terrain.
pub fn inputs() -> horizon_shaders::GbuffersInputs {
    let words = std::mem::size_of::<Vertex>() / 4;
    let mut declarations = "layout(location = 0) in vec3 hz_v_pos;\nlayout(location = 1) in vec2 hz_v_uv;\n\
         layout(location = 2) in uint hz_v_overlay;\nlayout(location = 3) in vec4 hz_v_color;\n\
         layout(location = 4) in uvec4 hz_v_light;\nlayout(location = 5) in vec4 hz_v_normal;\n\
         layout(set = 1, binding = 0) readonly buffer HzEntityVertices { uint hz_entity_vertices[]; };\n\
         vec4 hz_tangent; vec2 hz_mid;\n"
        .to_owned();
    // `at_velocity`: the vertex's offset from last frame in view space, this frame's view
    // position less last frame's. Last frame's vertices stay at binding 2, relative to last
    // frame's camera, and binding 3 gives each vertex its index there (none: no velocity).
    declarations.push_str(&format!(
        "layout(set = 1, binding = 2) readonly buffer HzEntityPrevious {{ uint hz_entity_previous[]; }};\n\
         layout(set = 1, binding = 3) readonly buffer HzEntityPreviousIndex {{ uint hz_previous_index[]; }};\n\
         uint hz_vi;\n\
         vec3 hz_velocity(mat4 view, mat4 previous_view) {{\n    \
             uint p = hz_previous_index[hz_vi];\n    \
             if (p == 0xffffffffu) return vec3(0.0);\n    \
             uint w = p * {words}u;\n    \
             vec3 then = uintBitsToFloat(uvec3(hz_entity_previous[w], hz_entity_previous[w + 1u], hz_entity_previous[w + 2u]));\n    \
             return (view * vec4(hz_v_pos, 1.0)).xyz - (previous_view * vec4(then, 1.0)).xyz;\n\
         }}\n"
    ));
    let corner = |k: usize, field: &str| {
        let at = |o: usize| format!("hz_entity_vertices[hz_q + {}u]", k * words + o);
        match field {
            "pos" => format!("uintBitsToFloat(uvec3({}, {}, {}))", at(0), at(1), at(2)),
            _ => format!("uintBitsToFloat(uvec2({}, {}))", at(3), at(4)),
        }
    };
    let body = format!(
        "    hz_vi = uint(gl_VertexIndex);\n    \
         uint hz_q = (uint(gl_VertexIndex) & ~3u) * {words}u;\n    \
         vec3 hz_e1 = {p1} - {p0};\n    \
         vec3 hz_e2 = {p2} - {p0};\n    \
         vec2 hz_t0 = {t0};\n    \
         vec2 hz_d1 = {t1} - hz_t0;\n    \
         vec2 hz_d2 = {t2} - hz_t0;\n    \
         hz_mid = (hz_t0 + {t2}) * 0.5;\n    \
         vec3 hz_n = cross(hz_e1, hz_e2);\n    \
         hz_Normal = dot(hz_v_normal.xyz, hz_v_normal.xyz) > 0.01 ? normalize(hz_v_normal.xyz)\n        \
             : dot(hz_n, hz_n) > 1e-12 ? normalize(hz_n) : vec3(0.0, 1.0, 0.0);\n    \
         hz_tangent = vec4(1.0, 0.0, 0.0, 1.0);\n    \
         float hz_r = hz_d1.x * hz_d2.y - hz_d2.x * hz_d1.y;\n    \
         vec3 hz_t = (hz_e1 * hz_d2.y - hz_e2 * hz_d1.y) * sign(hz_r);\n    \
         vec3 hz_b = (hz_e2 * hz_d1.x - hz_e1 * hz_d2.x) * sign(hz_r);\n    \
         hz_t -= hz_Normal * dot(hz_Normal, hz_t);\n    \
         if (dot(hz_t, hz_t) > 1e-12) {{\n        \
             hz_t = normalize(hz_t);\n        \
             hz_tangent = vec4(hz_t, dot(cross(hz_t, hz_Normal), hz_b) < 0.0 ? -1.0 : 1.0);\n    \
         }}\n    \
         hz_Vertex = vec4(hz_v_pos, 1.0);\n    \
         hz_MultiTexCoord0 = vec4(hz_v_uv, 0.0, 1.0);\n    \
         hz_MultiTexCoord1 = vec4(float(hz_v_light.y), float(hz_v_light.x), 0.0, 1.0);\n    \
         hz_MultiTexCoord2 = hz_MultiTexCoord1;\n    \
         hz_Color = hz_v_color;\n",
        p0 = corner(0, "pos"),
        p1 = corner(1, "pos"),
        p2 = corner(2, "pos"),
        t0 = corner(0, "uv"),
        t1 = corner(1, "uv"),
        t2 = corner(2, "uv"),
    );
    let attributes = [
        ("at_tangent", "hz_tangent"),
        ("mc_midTexCoord", "vec4(hz_mid, 0.0, 1.0)"),
        ("vaPosition", "hz_Vertex.xyz"),
        ("vaUV0", "hz_v_uv"),
        ("vaUV1", "ivec2(0, 10)"),
        ("vaUV2", "ivec2(hz_MultiTexCoord1.xy)"),
        ("vaColor", "hz_Color"),
        ("vaNormal", "hz_Normal"),
        (
            "at_velocity",
            "hz_velocity(gbufferModelView, gbufferPreviousModelView)",
        ),
        // Horizon's own, as for terrain: the HDR tier of a light-giving item, particle or glowing
        // text (`mesher::hdr_tier`, the overlay's high bits), 0 for none, 1 to 4 for low to very
        // bright.
        ("horizonTier", "float((hz_v_overlay >> 16u) & 255u)"),
    ];
    horizon_shaders::GbuffersInputs {
        declarations,
        body,
        attributes: attributes
            .iter()
            .map(|&(n, v)| (n.to_owned(), v.to_owned()))
            .collect(),
        after: String::new(),
        // Cutout, as vanilla's entity types mostly are (OQ-23): the loader's test at 0.1.
        terrain_lighting: None,
        alpha_test: Some(horizon_shaders::AlphaTest {
            op: ">",
            reference: 0.1,
        }),
        parts: None,
        item_id_declared: false,
        far_depth: false,
        // Entities, items and block entities read the shared atlas: its own maps (paged_maps.rs).
        paged_maps: true,
        water_mask: false,
        water_patches: false,
    }
}

/// What a shadow program with a geometry stage reads of entities and block entities: the
/// inputs of [`inputs`], each vertex read from set 1 (`hz_entity_vertices`), as `Vertex` lays
/// it out. The draw's corners walk its quads two triangles each, as `quad_indices` does: corner
/// `c` of a draw from vertex `first` (a whole quad) is drawn as vertex `first / 4 * 6 + c`.
pub fn geometry_inputs() -> horizon_shaders::GeometryInputs {
    let words = std::mem::size_of::<Vertex>() / 4;
    let pull = format!(
        "        uint hz_pw = hz_vertex_index * {words}u;\n        \
         hz_v_pos = uintBitsToFloat(uvec3(hz_entity_vertices[hz_pw], hz_entity_vertices[hz_pw + 1u], hz_entity_vertices[hz_pw + 2u]));\n        \
         hz_v_uv = uintBitsToFloat(uvec2(hz_entity_vertices[hz_pw + 3u], hz_entity_vertices[hz_pw + 4u]));\n        \
         hz_v_overlay = hz_entity_vertices[hz_pw + 5u];\n        \
         hz_v_color = unpackUnorm4x8(hz_entity_vertices[hz_pw + 6u]);\n        \
         uint hz_pl = hz_entity_vertices[hz_pw + 7u];\n        \
         hz_v_light = uvec4(hz_pl & 255u, (hz_pl >> 8u) & 255u, (hz_pl >> 16u) & 255u, hz_pl >> 24u);\n        \
         hz_v_normal = unpackSnorm4x8(hz_entity_vertices[hz_pw + 8u]);\n"
    );
    horizon_shaders::GeometryInputs {
        inputs: inputs(),
        pull,
        // Quad `c / 6`, corner 0, 1, 2, 0, 2, 3 of it.
        index:
            "((@) / 6u * 4u + ((@) % 6u < 3u ? (@) % 6u : ((@) % 6u == 3u ? 0u : (@) % 6u - 2u)))"
                .to_owned(),
    }
}

/// What `gbuffers_clouds` reads from the clouds' mesh (entity vertices, `vs_cloud`): the
/// position moved by the clouds' offset, the colour of the face's shade from their palette,
/// full light, the face's normal. Set 1 holds the clouds' uniform (`clouds::Uniform`).
pub fn cloud_inputs() -> horizon_shaders::GbuffersInputs {
    let declarations = "layout(location = 0) in vec3 hz_v_pos;\nlayout(location = 1) in vec2 hz_v_uv;\n\
         layout(location = 2) in uint hz_v_overlay;\nlayout(location = 3) in vec4 hz_v_color;\n\
         layout(location = 4) in uvec4 hz_v_light;\nlayout(location = 5) in vec4 hz_v_normal;\n\
         layout(set = 1, binding = 0) uniform HzCloud { vec4 hz_cloud_offset; vec4 hz_cloud_colors[4]; };\n"
        .to_owned();
    let body = "    hz_Vertex = vec4(hz_v_pos + hz_cloud_offset.xyz, 1.0);\n    \
         hz_MultiTexCoord0 = vec4(hz_v_uv, 0.0, 1.0);\n    \
         hz_MultiTexCoord1 = vec4(240.0, 240.0, 0.0, 1.0);\n    \
         hz_MultiTexCoord2 = hz_MultiTexCoord1;\n    \
         hz_Color = hz_cloud_colors[min(hz_v_light.x, 3u)];\n    \
         hz_Normal = dot(hz_v_normal.xyz, hz_v_normal.xyz) > 0.01 ? normalize(hz_v_normal.xyz) : vec3(0.0, 1.0, 0.0);\n"
        .to_owned();
    let attributes = [
        ("vaPosition", "hz_Vertex.xyz"),
        ("vaUV0", "hz_v_uv"),
        ("vaUV2", "ivec2(240, 240)"),
        ("vaColor", "hz_Color"),
        ("vaNormal", "hz_Normal"),
    ];
    horizon_shaders::GbuffersInputs {
        declarations,
        body,
        attributes: attributes
            .iter()
            .map(|&(n, v)| (n.to_owned(), v.to_owned()))
            .collect(),
        after: String::new(),
        terrain_lighting: None,
        alpha_test: None,
        parts: None,
        item_id_declared: false,
        far_depth: false,
        paged_maps: false,
        water_mask: false,
        water_patches: false,
    }
}

/// `entityColor` for a run whose first quad carries overlay code `overlay`: the hurt red or a
/// white flash, its alpha how much of that colour shows over the entity (vanilla mixes the
/// overlay texel's colour in by one minus the texel's alpha).
pub fn entity_color(overlay: u32) -> [f32; 4] {
    let [r, g, b, a] = crate::overlay::HURT_TEXEL.map(|v| f32::from(v) / 255.0);
    let white = |u: u32| {
        let a = f32::from(crate::overlay::white_alpha(u)) / 255.0;
        [1.0, 1.0, 1.0, 1.0 - a]
    };
    match overlay {
        1 | 8 => [r, g, b, 1.0 - a],
        5 => white(15),
        16..32 => white(overlay - crate::overlay::WHITE),
        _ => [0.0; 4],
    }
}

/// One draw: its quads in `PackEntities`' buffer, its program and its slot of `HzDraw`.
pub(super) struct Draw {
    /// The first vertex.
    pub base: u32,
    pub quads: u32,
    /// The kind of program that draws it (`ENTITY`, `BLOCK`, `TRANSLUCENT`, `PARTICLE`).
    pub program: usize,
    /// The pack's gbuffers program draws it (else Horizon does).
    pub pack: bool,
    /// The pack's shadow program draws it into the shadow map too.
    pub shadow: bool,
    pub slot: u32,
    /// End portal or gateway faces: drawn with `end_portal.png` bound as the atlas.
    pub portal: bool,
    /// A boat's water patch (`WATER_MASK_OVERLAY`): depth only, with the program's pipeline that
    /// writes no colour.
    pub water_mask: bool,
}

/// Which entity runs a pack's shadow pass draws (2.4, 3.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Casters {
    /// `shadow_entities` (or `shadow`) draws entities...
    pub entities: bool,
    /// ...only the local player (`shadowPlayer` without `shadowEntities`).
    pub only_player: bool,
    /// `shadow_block` (or `shadow`) draws block entities...
    pub blocks: bool,
    /// ...only those whose block emits light (`shadowLightBlockEntities`).
    pub only_lit: bool,
}

impl Casters {
    /// Whether the shadow pass draws `span`'s run, of kind `program`.
    pub fn cast(&self, span: &EntitySpan, program: usize) -> bool {
        match program {
            ENTITY => self.entities && (!self.only_player || span.player),
            BLOCK | BLOCK_TRANSLUCENT => self.blocks && (!self.only_lit || span.emissive),
            _ => false,
        }
    }
}

/// The vertices and per-draw uniforms of the runs a pack draws, rebuilt each frame.
pub(super) struct PackEntities {
    vertices: wgpu::Buffer,
    /// Last frame's `vertices` (the two swap each frame), for `at_velocity`.
    previous: wgpu::Buffer,
    /// For each of `vertices`, its index in `previous` (`NO_PREVIOUS`: none).
    previous_index: wgpu::Buffer,
    /// This frame's entities (`Frame::entity_motion`) and last frame's.
    motion: Motion,
    previous_motion: Motion,
    index_scratch: Vec<u32>,
    /// Set 1: the vertices again, for the quads' corners; last frame's and the indices into them.
    pub table: wgpu::BindGroupLayout,
    pub table_group: wgpu::BindGroup,
    slots: wgpu::Buffer,
    pub group: wgpu::BindGroup,
    stride: u64,
    /// Two triangles per quad, for `quads` quads.
    pub indices: wgpu::Buffer,
    quads: u32,
    scratch: Vec<Vertex>,
    uniforms: Vec<u8>,
    pub draws: Vec<Draw>,
}

impl PackEntities {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let stride = SLOT.max(u64::from(
            device.limits().min_uniform_buffer_offset_alignment,
        ));
        let entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let table = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shader pack entity vertices"),
            entries: &[entry(0), entry(2), entry(3)],
        });
        let vertices = Self::vertex_buffer(device, 4096);
        let previous = Self::vertex_buffer(device, 4096);
        let previous_index = Self::index_buffer(device, 4096);
        let table_group = Self::table_group(device, &table, &vertices, &previous, &previous_index);
        let (slots, group) = Self::slot_buffer(device, layout, 64, stride);
        Self {
            vertices,
            previous,
            previous_index,
            motion: Motion::default(),
            previous_motion: Motion::default(),
            index_scratch: Vec::new(),
            table,
            table_group,
            slots,
            group,
            stride,
            indices: quad_indices(device, 1024),
            quads: 1024,
            scratch: Vec::new(),
            uniforms: Vec::new(),
            draws: Vec::new(),
        }
    }

    fn vertex_buffer(device: &wgpu::Device, vertices: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack entity vertices"),
            size: vertices * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn index_buffer(device: &wgpu::Device, vertices: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack entity previous indices"),
            size: vertices * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn table_group(
        device: &wgpu::Device,
        table: &wgpu::BindGroupLayout,
        vertices: &wgpu::Buffer,
        previous: &wgpu::Buffer,
        index: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader pack entity vertices"),
            layout: table,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: vertices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: previous.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: index.as_entire_binding(),
                },
            ],
        })
    }

    pub(super) fn slot_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        slots: u64,
        stride: u64,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack entity draws"),
            size: slots * stride,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader pack entity draws"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(SLOT),
                }),
            }],
        });
        (buffer, group)
    }

    pub fn skin_target(&self) -> &wgpu::Buffer {
        &self.vertices
    }

    pub fn vertices(&self) -> wgpu::BufferSlice<'_> {
        self.vertices.slice(..)
    }

    /// The dynamic offset of draw slot `slot`.
    pub fn offset(&self, slot: u32) -> u32 {
        (u64::from(slot) * self.stride) as u32
    }

    /// Splits `entities` between the pack, which draws the runs of `spans` its programs cover
    /// (`drawn`: whether it has `gbuffers_entities`, `gbuffers_block` or their fallbacks), and
    /// Horizon, which draws the rest (`rest`). Uploads the pack's part, and the runs its shadow
    /// programs draw (`shadow`). Runs hidden from the camera only ever cast shadows.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        entities: &[Vertex],
        spans: &[EntitySpan],
        drawn: [bool; PROGRAMS],
        crack: ([f32; 4], [u32; 2]),
        shadow: Casters,
        rest: &mut Vec<Vertex>,
        skin: Option<&super::entity_skin::SkinFrame>,
        motion: (&[(u32, u64)], &[u32]),
    ) {
        self.draws.clear();
        self.scratch.clear();
        self.uniforms.clear();
        rest.clear();
        // Last frame's vertices stay for `at_velocity`; this frame's go to the other buffer.
        std::mem::swap(&mut self.vertices, &mut self.previous);
        std::mem::swap(&mut self.motion, &mut self.previous_motion);
        self.motion.clear();
        let mut copied = Vec::new();
        let posed = skin.map_or(0, |s| s.vertices);
        if let Some(skin) = skin {
            for (span, range, overlay) in skin_runs(skin) {
                let program = material_program(&span, overlay, drawn).expect("tagged skin stage");
                let pack = drawn[program] && !span.hidden;
                let casts = shadow.cast(&span, program);
                if !pack && !casts {
                    continue;
                }
                self.push_draw(
                    span,
                    range.start,
                    range.len() as u32,
                    program,
                    pack,
                    casts,
                    overlay,
                    false,
                    false,
                );
            }
        }
        let first = spans.first().map_or(entities.len(), |s| s.start as usize);
        extend_without_portals(rest, &entities[..first.min(entities.len())]);
        let (mut other, mut portal_faces, mut beam_faces) = (Vec::new(), Vec::new(), Vec::new());
        let mut water_patches = Vec::new();
        let mut without_fluids = Vec::new();
        for (span, range, material) in material_runs(entities, spans, drawn) {
            let run_start = range.start as u32;
            let whole = &entities[range];
            // A native fluid carries sprite-array UVs, not shared-atlas UVs.
            // Keep it in its own material even when a shader pack draws the
            // model in the same block-entity run. Do not corrupt either UV space.
            let whole: &[Vertex] = if whole
                .chunks(4)
                .any(|q| super::is_native_terrain(q[0].layer))
            {
                without_fluids.clear();
                for quad in whole.chunks(4) {
                    if super::is_native_terrain(quad[0].layer) {
                        if !span.hidden {
                            rest.extend_from_slice(quad);
                        }
                    } else {
                        without_fluids.extend_from_slice(quad);
                    }
                }
                &without_fluids
            } else {
                whole
            };
            // A block entity's run can hold End portal faces and beams beside plain parts (an End
            // gateway: its cube and its beam): each goes to its own program.
            // Beacon beams come in a run of their own, at stage NONE (game.rs: no stage of 5.4
            // is theirs), for gbuffers_beaconbeam.
            let split = ((matches!(material, Some(BLOCK | BLOCK_TRANSLUCENT))
                || span.stage == render_stage("NONE"))
                && whole
                    .chunks(4)
                    .any(|q| matches!(q[0].layer & 0xffff, 6 | 9 | 10)))
                // A boat's water patch: depth only (`Part::WaterMask`).
                || whole
                    .chunks(4)
                    .any(|q| q[0].layer & 0xffff == super::entity_batch::WATER_MASK_OVERLAY);
            let parts: [(&[Vertex], Part); 4] = if split {
                other.clear();
                portal_faces.clear();
                beam_faces.clear();
                water_patches.clear();
                for quad in whole.chunks(4) {
                    match quad[0].layer & 0xffff {
                        9 | 10 => portal_faces.extend_from_slice(quad),
                        6 => beam_faces.extend_from_slice(quad),
                        super::entity_batch::WATER_MASK_OVERLAY => {
                            water_patches.extend_from_slice(quad)
                        }
                        _ => other.extend_from_slice(quad),
                    }
                }
                [
                    (&other, Part::Plain),
                    (&portal_faces, Part::Portal),
                    (&beam_faces, Part::Beam),
                    (&water_patches, Part::WaterMask),
                ]
            } else {
                [
                    (whole, Part::Plain),
                    (&[], Part::Plain),
                    (&[], Part::Plain),
                    (&[], Part::Plain),
                ]
            };
            for (run, part) in parts {
                if run.is_empty() {
                    continue;
                }
                let program = match part {
                    Part::Portal => Some(BLOCK),
                    Part::Beam => Some(BEAM),
                    // Drawn by the entity program, whose depth-only pipeline it takes.
                    Part::WaterMask => material.filter(|&p| p == ENTITY),
                    Part::Plain => material.filter(|&p| {
                        plain(run)
                            || (p == DESTROY && run.iter().all(|v| matches!(v.layer, 11 | 12)))
                    }),
                };
                let pack = program.is_some_and(|p| drawn[p]) && !span.hidden;
                // Portal faces cast too (a pack's shadow pass sees block entities whole, as Iris
                // draws them); beams do not.
                let casts = !matches!(part, Part::Beam | Part::WaterMask)
                    && program.is_some_and(|p| shadow.cast(&span, p));
                if !pack && !span.hidden {
                    extend_without_portals(rest, run);
                }
                let Some(program) = program.filter(|_| pack || casts) else {
                    continue;
                };
                let start = self.scratch.len() as u32;
                match part {
                    Part::Beam => beam_segments(run, &mut self.scratch),
                    _ => self.scratch.extend_from_slice(run),
                }
                // A run copied whole keeps its vertices' order, for `at_velocity`; so does the
                // start of a split one copied as it is (a boat's hull before its water patch).
                let in_order = !split
                    || (part == Part::Plain
                        && bytemuck::cast_slice::<_, u8>(&whole[..run.len().min(whole.len())])
                            == bytemuck::cast_slice::<_, u8>(run));
                if in_order {
                    copied.push((run_start, posed + start, run.len() as u32));
                }
                if program == DESTROY {
                    project_cracks(&mut self.scratch[start as usize..], crack.0, crack.1);
                }
                if part == Part::Portal {
                    as_portal_texture(&mut self.scratch[start as usize..]);
                }
                let vertices = self.scratch.len() as u32 - start;
                let overlay = self.scratch[start as usize].layer & 0xffff;
                let portal = part == Part::Portal;
                self.push_draw(
                    span,
                    posed + start,
                    vertices,
                    program,
                    pack,
                    casts,
                    overlay,
                    portal,
                    part == Part::WaterMask,
                );
            }
        }
        if self.draws.is_empty() && posed == 0 {
            return;
        }
        let total = posed + self.scratch.len() as u32;
        let posed_marks = if skin.is_some() { motion.1 } else { &[] };
        self.motion
            .collect(motion.0, posed_marks, entities.len() as u32, posed, &copied);
        self.index_scratch.clear();
        self.index_scratch.resize(total as usize, NO_PREVIOUS);
        self.motion
            .link(&self.previous_motion, &mut self.index_scratch);
        let vertex_size = std::mem::size_of::<Vertex>() as u64;
        let (need_vertices, need_slots) = (
            u64::from(posed) + self.scratch.len() as u64,
            self.draws.len() as u64,
        );
        let have_vertices = self.vertices.size() / vertex_size;
        let have_slots = self.slots.size() / self.stride;
        if need_vertices > have_vertices {
            self.vertices = Self::vertex_buffer(device, need_vertices.next_power_of_two());
        }
        if need_vertices * 4 > self.previous_index.size() {
            self.previous_index = Self::index_buffer(device, need_vertices.next_power_of_two());
        }
        queue.write_buffer(
            &self.previous_index,
            0,
            bytemuck::cast_slice(&self.index_scratch),
        );
        self.table_group = Self::table_group(
            device,
            &self.table,
            &self.vertices,
            &self.previous,
            &self.previous_index,
        );
        if need_slots > have_slots {
            (self.slots, self.group) =
                Self::slot_buffer(device, layout, need_slots.next_power_of_two(), self.stride);
        }
        let quads = self.draws.iter().map(|d| d.quads).max().unwrap_or(0);
        if quads > self.quads {
            self.quads = quads.next_power_of_two();
            self.indices = quad_indices(device, self.quads);
        }
        if !self.scratch.is_empty() {
            queue.write_buffer(
                &self.vertices,
                u64::from(posed) * vertex_size,
                bytemuck::cast_slice(&self.scratch),
            );
        }
        if !self.uniforms.is_empty() {
            queue.write_buffer(&self.slots, 0, &self.uniforms);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_draw(
        &mut self,
        span: EntitySpan,
        start: u32,
        vertices: u32,
        program: usize,
        pack: bool,
        casts: bool,
        overlay: u32,
        portal: bool,
        water_mask: bool,
    ) {
        let (entity_id, block_id) = match program {
            BLOCK | BLOCK_TRANSLUCENT => (0, span.id),
            _ => (span.id, 0),
        };
        let mut bytes = [0u8; SLOT as usize];
        bytes[0..4].copy_from_slice(&(span.stage as i32).to_le_bytes());
        bytes[4..8].copy_from_slice(&entity_id.to_le_bytes());
        bytes[8..12].copy_from_slice(&block_id.to_le_bytes());
        bytes[12..16].copy_from_slice(&span.item_id.to_le_bytes());
        // The overlay: the layer's low 16 bits (HDR output's tier rides in the high ones).
        for (i, c) in entity_color(overlay & 0xffff).iter().enumerate() {
            bytes[16 + 4 * i..20 + 4 * i].copy_from_slice(&c.to_le_bytes());
        }
        self.uniforms.extend_from_slice(&bytes);
        self.uniforms
            .resize(self.uniforms.len() + (self.stride - SLOT) as usize, 0);
        self.draws.push(Draw {
            base: start,
            quads: vertices / 4,
            program,
            pack,
            shadow: casts,
            slot: self.draws.len() as u32,
            portal,
            water_mask,
        });
    }
}

/// Two triangles for each of `quads` quads, from vertex 0.
pub(super) fn quad_indices(device: &wgpu::Device, quads: u32) -> wgpu::Buffer {
    let indices: Vec<u32> = (0..quads)
        .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
        .collect();
    wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("Shader pack entity indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        },
    )
}

/// Whether a run holds only geometry a gbuffers program can draw from the shared atlas: no
/// celestial body, cloud, beam, End portal, crack or energy swirl (overlay codes Horizon's entity
/// shader draws in its own way; entity.wgsl).
fn plain(run: &[Vertex]) -> bool {
    run.iter()
        .step_by(4)
        .all(|v| !matches!(v.layer & 0xffff, 2 | 4 | 6 | 9 | 10 | 11 | 12 | 34))
}

/// What part of a block entity's run goes to which program (`prepare`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Plain,
    Portal,
    Beam,
    /// A boat's water patch: depth only (`Draw::water_mask`).
    WaterMask,
}

/// Beam faces (`block_entity_render::beam`: layer 6, v counting the sprite's repeats up the
/// beam, the sprite's v range in the light bytes) for a pack: each face cut where its texture
/// repeats, so every piece holds one copy of the sprite in plain atlas coordinates (at most 64
/// pieces a face; a taller beam stretches its copies), white light and its colour kept.
fn beam_segments(run: &[Vertex], out: &mut Vec<Vertex>) {
    for quad in run.as_chunks::<4>().0 {
        let wrap = quad[0].light;
        let (v0, size) = (
            (wrap & 65535) as f32 / 65535.0,
            (wrap >> 16) as f32 / 65535.0,
        );
        // Corners 0 and 3 at the beam's end (v1), 1 and 2 at its start (v2); 0 and 1 on one
        // side, 2 and 3 on the other.
        let (v1, v2) = (quad[0].uv[1], quad[1].uv[1]);
        if (v1 - v2).abs() < 1e-6 {
            continue;
        }
        let (lo, hi) = (v1.min(v2), v1.max(v2));
        let first = lo.floor();
        let repeats = (hi.ceil() - first).max(1.0);
        let step = (repeats / 64.0).ceil().max(1.0);
        let pieces = (repeats / step).ceil() as u32;
        let at = |side: usize, v: f32| {
            let (a, b) = if side == 0 {
                (quad[1], quad[0])
            } else {
                (quad[2], quad[3])
            };
            let t = (v - v2) / (v1 - v2);
            glam::Vec3::from(a.pos)
                .lerp(glam::Vec3::from(b.pos), t)
                .to_array()
        };
        for k in 0..pieces {
            let base = first + k as f32 * step;
            let (a, b) = (base.max(lo), (base + step).min(hi));
            if b <= a {
                continue;
            }
            // The end-side corners take the piece's bound nearer v1.
            let (end_v, start_v) = if v1 > v2 { (b, a) } else { (a, b) };
            for (i, corner) in quad.iter().enumerate() {
                let side = usize::from(i >= 2);
                let v = if i == 0 || i == 3 { end_v } else { start_v };
                let mut out_v = *corner;
                out_v.pos = at(side, v);
                out_v.uv = [corner.uv[0], v0 + (v - base) / step * size];
                out_v.layer = 0;
                out_v.light = 0x0000_f0f0;
                out.push(out_v);
            }
        }
    }
}

/// `run` added to Horizon's own draw without End portal faces: under a shader pack Horizon's
/// entity shader would put its screen-projected starfield into the pack's buffers as if they
/// were its own frame (misplaced where the pack renders at another size), so a portal the pack
/// does not draw (its programs not there, or not yet) is better left out.
fn extend_without_portals(rest: &mut Vec<Vertex>, run: &[Vertex]) {
    for quad in run.chunks(4) {
        if !quad.iter().all(|v| matches!(v.layer & 0xffff, 6 | 9 | 10)) {
            rest.extend_from_slice(quad);
        }
    }
}

/// End portal faces for a pack's `gbuffers_block` (block entities): each face textured with the
/// whole of `end_portal.png`, which the draw binds as the atlas (a texture of its own, so a
/// pack's own portal effect can wrap it), white, at full light; the pack tells the portal by its
/// `blockEntityId`.
fn as_portal_texture(run: &mut [Vertex]) {
    const CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    for (i, v) in run.iter_mut().enumerate() {
        v.uv = CORNERS[i % 4];
        v.layer = 0;
        v.color = 0xFFFF_FFFF;
        v.light = 0x0000_f0f0;
    }
}

/// The programs of entity runs, in the order they draw.
pub const ENTITY: usize = 0;
pub const BLOCK: usize = 1;
pub const TRANSLUCENT: usize = 2;
pub const PARTICLE: usize = 3;
pub const DESTROY: usize = 4;
/// Beacon and End gateway beams (`gbuffers_beaconbeam`).
pub const BEAM: usize = 5;
pub const BLOCK_TRANSLUCENT: usize = 6;
pub const PARTICLE_TRANSLUCENT: usize = 7;
pub const PROGRAMS: usize = 8;

/// Translucent blocks retain the ordinary block shadow program and blockEntityId.
pub fn shadow_program(program: usize) -> usize {
    if program == BLOCK_TRANSLUCENT {
        BLOCK
    } else {
        program
    }
}

fn material_program(span: &EntitySpan, layer: u32, drawn: [bool; PROGRAMS]) -> Option<usize> {
    Some(match program_of(span)? {
        BLOCK
            if drawn[BLOCK_TRANSLUCENT]
                && (span.translucent
                    || layer & super::OIT_ENTITY != 0
                    || matches!(
                        layer & 0xffff,
                        super::TRANSLUCENT_ITEM_OVERLAY | super::TRANSLUCENT_GLINT_ITEM_OVERLAY
                    )) =>
        {
            BLOCK_TRANSLUCENT
        }
        PARTICLE
            if drawn[PARTICLE_TRANSLUCENT]
                && layer & 0xffff == super::TRANSLUCENT_PARTICLE_OVERLAY =>
        {
            PARTICLE_TRANSLUCENT
        }
        p => p,
    })
}

/// Split only where an available variant changes a quad's program. Keep contiguous runs,
/// their vertex order and the original single draw when no material variant is usable.
fn material_runs<'a>(
    vertices: &'a [Vertex],
    spans: &'a [EntitySpan],
    drawn: [bool; PROGRAMS],
) -> impl Iterator<Item = (EntitySpan, Range<usize>, Option<usize>)> + 'a {
    runs(vertices.len(), spans).flat_map(move |(span, range)| {
        let split = match program_of(&span) {
            Some(BLOCK) => drawn[BLOCK_TRANSLUCENT],
            Some(PARTICLE) => drawn[PARTICLE_TRANSLUCENT],
            _ => false,
        };
        let mut start = range.start;
        std::iter::from_fn(move || {
            if start >= range.end {
                return None;
            }
            let program = material_program(&span, vertices[start].layer, drawn);
            let end = if split {
                (start..range.end)
                    .step_by(4)
                    .skip(1)
                    .find(|&i| material_program(&span, vertices[i].layer, drawn) != program)
                    .unwrap_or(range.end)
            } else {
                range.end
            };
            let run = start..end;
            start = end;
            Some((span, run, program))
        })
    })
}

/// Which program draws `span`'s run, given which of them (or their fallbacks) the pack has;
/// `None`: Horizon draws it.
#[cfg(test)]
fn drawn_by_pack(span: &EntitySpan, drawn: [bool; PROGRAMS]) -> Option<usize> {
    program_of(span).filter(|&p| drawn[p])
}

/// The kind of program that draws `span`'s run, when a pack may.
/// A vertex with no copy last frame: `at_velocity` reads zero.
const NO_PREVIOUS: u32 = u32::MAX;

/// Where each entity's vertices are in a frame's pack buffer, to match them with last frame's
/// (`at_velocity`): the indices of each one's GPU-posed vertices, then of its CPU ones.
#[derive(Default)]
struct Motion {
    /// Entity id (`Frame::entity_motion`) → (first index in `indices`, posed count, CPU count).
    entities: std::collections::HashMap<u64, (u32, u32, u32)>,
    indices: Vec<u32>,
}

impl Motion {
    fn clear(&mut self) {
        self.entities.clear();
        self.indices.clear();
    }

    /// Records each marked entity (`marks`: first CPU vertex and id, each run up to the next;
    /// `posed_marks`: the posed count at each mark, empty without posing). A CPU vertex is in
    /// the pack buffer where `copied` (first vertex in `Frame::entities`, first index in the
    /// buffer, count, in order) has it, else `NO_PREVIOUS`.
    fn collect(
        &mut self,
        marks: &[(u32, u64)],
        posed_marks: &[u32],
        cpu_total: u32,
        posed_total: u32,
        copied: &[(u32, u32, u32)],
    ) {
        let at = |e: u32| {
            let i = copied.partition_point(|c| c.0 <= e);
            match i.checked_sub(1).map(|i| copied[i]) {
                Some((first, index, n)) if e < first + n => index + (e - first),
                _ => NO_PREVIOUS,
            }
        };
        for (i, &(cpu, id)) in marks.iter().enumerate() {
            if id == 0 {
                continue;
            }
            let cpu_end = marks.get(i + 1).map_or(cpu_total, |m| m.0).max(cpu);
            let posed = posed_marks.get(i).copied().unwrap_or(0).min(posed_total);
            let posed_end = posed_marks
                .get(i + 1)
                .copied()
                .unwrap_or(posed_total)
                .clamp(posed, posed_total);
            let first = self.indices.len() as u32;
            self.indices.extend(posed..posed_end);
            self.indices.extend((cpu..cpu_end).map(at));
            self.entities
                .insert(id, (first, posed_end - posed, cpu_end - cpu));
        }
    }

    /// For each entity drawn last frame with the same posed and CPU counts, each vertex's index
    /// last frame, written into `out` (indexed by this frame's buffer).
    fn link(&self, previous: &Motion, out: &mut [u32]) {
        for (id, &(first, posed, cpu)) in &self.entities {
            let Some(&(then, p, c)) = previous.entities.get(id) else {
                continue;
            };
            if (p, c) != (posed, cpu) {
                continue;
            }
            let n = (posed + cpu) as usize;
            let now = &self.indices[first as usize..][..n];
            let before = &previous.indices[then as usize..][..n];
            for (&now, &before) in now.iter().zip(before) {
                if now != NO_PREVIOUS
                    && before != NO_PREVIOUS
                    && let Some(slot) = out.get_mut(now as usize)
                {
                    *slot = before;
                }
            }
        }
    }
}

pub fn program_of(span: &EntitySpan) -> Option<usize> {
    Some(if span.stage == render_stage("ENTITIES") {
        if span.translucent {
            TRANSLUCENT
        } else {
            ENTITY
        }
    } else if span.stage == render_stage("BLOCK_ENTITIES") {
        BLOCK
    } else if span.stage == render_stage("PARTICLES") {
        PARTICLE
    } else if span.stage == render_stage("DESTROY") {
        DESTROY
    } else {
        return None;
    })
}

/// Each span with its vertex range: from its start to the next span's (or the end).
pub fn runs(
    len: usize,
    spans: &[EntitySpan],
) -> impl Iterator<Item = (EntitySpan, Range<usize>)> + '_ {
    spans.iter().enumerate().map(move |(i, s)| {
        let end = spans.get(i + 1).map_or(len, |n| n.start as usize);
        let start = (s.start as usize).min(len);
        (*s, start..end.clamp(start, len))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destroy_stage_uses_the_pack_only_when_a_program_is_available() {
        let destroy = EntitySpan {
            stage: render_stage("DESTROY"),
            ..Default::default()
        };
        assert!(drawn_by_pack(&destroy, [true; PROGRAMS]).is_some());
        assert_eq!(drawn_by_pack(&destroy, [false; PROGRAMS]), None);
    }

    #[test]
    fn runs_split_the_list_between_pack_and_horizon() {
        let v = |layer| Vertex {
            layer,
            ..bytemuck::Zeroable::zeroed()
        };
        let span = |start, stage: &str, id| EntitySpan {
            start,
            stage: render_stage(stage),
            id,
            ..Default::default()
        };
        let spans = [
            span(2, "ENTITIES", 7),
            span(5, "PARTICLES", 0),
            span(6, "BLOCK_ENTITIES", 9),
        ];
        let ranges: Vec<_> = runs(10, &spans).map(|(_, r)| r).collect();
        assert_eq!(ranges, [2..5, 5..6, 6..10]);
        let all = [true; PROGRAMS];
        assert_eq!(drawn_by_pack(&spans[0], all), Some(ENTITY));
        assert_eq!(drawn_by_pack(&spans[1], all), Some(PARTICLE));
        assert_eq!(
            drawn_by_pack(
                &spans[1],
                [true, true, true, false, true, true, false, false]
            ),
            None
        );
        assert_eq!(
            drawn_by_pack(
                &spans[2],
                [true, false, true, true, true, true, false, false]
            ),
            None
        );
        assert_eq!(drawn_by_pack(&spans[2], all), Some(BLOCK));
        let shadow = EntitySpan {
            translucent: true,
            ..spans[0]
        };
        assert_eq!(drawn_by_pack(&shadow, all), Some(TRANSLUCENT));
        assert_eq!(drawn_by_pack(&span(0, "DESTROY", 0), all), Some(DESTROY));
        // A beacon beam's quads stay Horizon's.
        assert!(plain(&[v(0), v(0), v(0), v(0), v(1)]));
        assert!(!plain(&[v(0), v(0), v(0), v(0), v(6)]));
        assert_eq!(entity_color(1)[0], 1.0);
        assert!((entity_color(1)[3] - (1.0 - 178.0 / 255.0)).abs() < 1e-6);
        assert_eq!(entity_color(0), [0.0; 4]);
        // The shadow pass: entities, the player alone, block entities, those that emit light.
        let all = Casters {
            entities: true,
            blocks: true,
            ..Default::default()
        };
        let player = EntitySpan {
            player: true,
            ..spans[0]
        };
        let lit = EntitySpan {
            emissive: true,
            ..spans[2]
        };
        assert!(all.cast(&spans[0], ENTITY) && all.cast(&spans[2], BLOCK));
        assert!(!all.cast(&spans[1], PARTICLE));
        let some = Casters {
            only_player: true,
            only_lit: true,
            ..all
        };
        assert!(!some.cast(&spans[0], ENTITY) && some.cast(&player, ENTITY));
        assert!(!some.cast(&spans[2], BLOCK) && some.cast(&lit, BLOCK));
    }
}

/// Decode the destroy-stage origin stored in the light bytes and project one continuous
/// sprite over each model face. Pack geometry uses ordinary atlas UVs, not Horizon's overlay codes.
fn project_cracks(vertices: &mut [Vertex], sprite: [f32; 4], atlas: [u32; 2]) {
    let dims = [atlas[0].max(1) as f32, atlas[1].max(1) as f32];
    for quad in vertices.as_chunks_mut::<4>().0 {
        let origin = [
            (quad[0].light & 65535) as f32 / 65535.0,
            (quad[0].light >> 16) as f32 / 65535.0,
        ];
        let origin = [0, 1].map(|i| (origin[i] * dims[i]).round() / dims[i]);
        let min = [0, 1]
            .map(|i| (quad.iter().map(|v| v.uv[i]).fold(f32::INFINITY, f32::min) + 0.0001).floor());
        for v in quad {
            v.uv =
                [0, 1].map(|i| origin[i] + (v.uv[i] - min[i]).clamp(0.001, 0.999) * sprite[i + 2]);
            v.layer = 0;
            v.light = 0x0000_f0f0;
        }
    }
}

#[cfg(test)]
mod portal_beam_tests {
    use super::*;

    fn quad(layer: u32, light: u32, corners: [([f32; 3], [f32; 2]); 4]) -> [Vertex; 4] {
        corners.map(|(pos, uv)| Vertex {
            pos,
            uv,
            layer,
            color: 0xFF20_40FF,
            light,
            ..bytemuck::Zeroable::zeroed()
        })
    }

    #[test]
    fn a_beam_face_becomes_one_sprite_copy_per_repeat() {
        // `block_entity_render::beam`'s corner order: 0 and 3 at the end (v1), 1 and 2 at the
        // start (v2); the sprite's v range (0.25, height 0.5) in the light bytes.
        let wrap = (0.25f32 * 65535.0).round() as u32 | ((0.5f32 * 65535.0).round() as u32) << 16;
        let face = quad(
            6,
            wrap,
            [
                ([0.0, 3.0, 0.0], [0.9, 3.0]),
                ([0.0, 0.0, 0.0], [0.9, 0.0]),
                ([1.0, 0.0, 0.0], [0.1, 0.0]),
                ([1.0, 3.0, 0.0], [0.1, 3.0]),
            ],
        );
        let mut out = Vec::new();
        beam_segments(&face, &mut out);
        assert_eq!(out.len(), 12, "three repeats, three pieces");
        for (k, piece) in out.chunks(4).enumerate() {
            let (bottom, top) = (k as f32, k as f32 + 1.0);
            assert_eq!([piece[0].pos[1], piece[1].pos[1]], [top, bottom]);
            assert_eq!([piece[2].pos[1], piece[3].pos[1]], [bottom, top]);
            for v in piece {
                assert!((0.25 - 1e-4..=0.75 + 1e-4).contains(&v.uv[1]), "{:?}", v.uv);
                assert_eq!(v.layer, 0);
                assert_eq!(v.light, 0x0000_f0f0);
                assert_eq!(v.color, 0xFF20_40FF);
            }
            assert!((piece[0].uv[1] - 0.75).abs() < 1e-4 && (piece[1].uv[1] - 0.25).abs() < 1e-4);
            assert_eq!([piece[0].uv[0], piece[2].uv[0]], [0.9, 0.1]);
        }
    }

    #[test]
    fn a_very_tall_beam_is_cut_into_at_most_64_pieces() {
        let face = quad(
            6,
            0xFFFF_0000,
            [
                ([0.0, 2000.0, 0.0], [1.0, 2000.0]),
                ([0.0, 0.0, 0.0], [1.0, 0.0]),
                ([1.0, 0.0, 0.0], [0.0, 0.0]),
                ([1.0, 2000.0, 0.0], [0.0, 2000.0]),
            ],
        );
        let mut out = Vec::new();
        beam_segments(&face, &mut out);
        assert!(out.len() / 4 <= 64, "{} pieces", out.len() / 4);
        let top = out.iter().map(|v| v.pos[1]).fold(f32::MIN, f32::max);
        let bottom = out.iter().map(|v| v.pos[1]).fold(f32::MAX, f32::min);
        assert_eq!(
            (bottom, top),
            (0.0, 2000.0),
            "the pieces still span the whole beam"
        );
    }

    #[test]
    fn portal_faces_take_the_whole_portal_texture_at_full_light() {
        let mut face = quad(9, 0, [([0.0; 3], [0.0; 2]); 4]).to_vec();
        as_portal_texture(&mut face);
        let uvs: Vec<[f32; 2]> = face.iter().map(|v| v.uv).collect();
        assert_eq!(uvs, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
        assert!(
            face.iter()
                .all(|v| v.layer == 0 && v.color == 0xFFFF_FFFF && v.light == 0x0000_f0f0)
        );
    }

    #[test]
    fn horizon_keeps_no_portal_or_beam_face_under_a_pack() {
        let mut run = Vec::new();
        for layer in [0, 6, 9, 10, 2] {
            run.extend(quad(layer, 0, [([0.0; 3], [0.0; 2]); 4]));
        }
        let mut rest = Vec::new();
        extend_without_portals(&mut rest, &run);
        let kept: Vec<u32> = rest.chunks(4).map(|q| q[0].layer).collect();
        assert_eq!(kept, [0, 2]);
    }
}

#[cfg(test)]
mod crack_projection_tests {
    use super::*;
    #[test]
    fn crack_uvs_keep_a_full_sprite_and_decode_each_stage() {
        let mut vertices: Vec<Vertex> = [[-1.0, -1.0], [-1.0, 0.0], [0.0, 0.0], [0.0, -1.0]]
            .into_iter()
            .map(|uv| Vertex {
                uv,
                layer: 11,
                light: crate::breaking::stage_light([0.25, 0.5]),
                ..bytemuck::Zeroable::zeroed()
            })
            .collect();
        project_cracks(
            &mut vertices,
            [0.0, 0.0, 16.0 / 4096.0, 16.0 / 9216.0],
            [4096, 9216],
        );
        assert!(
            vertices
                .iter()
                .all(|v| v.layer == 0 && v.uv[0] >= 0.25 && v.uv[0] < 0.25 + 16.0 / 4096.0)
        );
        assert!((vertices[2].uv[0] - vertices[0].uv[0]) > 15.0 / 4096.0);
        assert!((vertices[1].uv[1] - vertices[0].uv[1]) > 15.0 / 9216.0);
    }
}

/// Intersects shader identities with overlay runs, without reading posed vertices back.
fn skin_runs(skin: &super::entity_skin::SkinFrame) -> Vec<(EntitySpan, Range<u32>, u32)> {
    let mut out: Vec<(EntitySpan, Range<u32>, u32)> = Vec::new();
    let mut span_index = 0;
    for job in &skin.jobs {
        let mut at = job.dst;
        let end = at + job.count;
        while at < end {
            while span_index + 1 < skin.spans.len() && skin.spans[span_index + 1].start <= at {
                span_index += 1;
            }
            let Some(&span) = skin.spans.get(span_index) else {
                break;
            };
            let stop = end.min(skin.spans.get(span_index + 1).map_or(end, |s| s.start));
            if stop <= at {
                break;
            }
            if let Some((old, range, overlay)) = out.last_mut()
                && (EntitySpan { start: 0, ..*old }) == (EntitySpan { start: 0, ..span })
                && *overlay == job.overlay
                && range.end == at
            {
                range.end = stop;
            } else {
                out.push((span, at..stop, job.overlay));
            }
            at = stop;
        }
    }
    out
}

#[cfg(test)]
mod gpu_span_tests {
    use super::*;
    use crate::gfx::entity_skin::{SkinFrame, SkinJob};
    #[test]
    #[ignore = "requires a Vulkan/Metal/DX12 adapter"]
    fn native_fluid_stays_native_while_model_uses_pack() {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let layout = super::super::pack_draw::DrawUniforms::new(&device);
        let mut geometry = PackEntities::new(&device, &layout.layout);
        for overlay in [
            super::super::FLUID_OVERLAY,
            super::super::TERRAIN_MODEL_SOLID_OVERLAY,
            super::super::TERRAIN_MODEL_CUTOUT_OVERLAY,
            super::super::TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY,
        ] {
            let mut mixed = vec![
                Vertex {
                    layer: 0,
                    ..bytemuck::Zeroable::zeroed()
                };
                8
            ];
            for v in &mut mixed[4..] {
                v.layer = overlay | 279 << 16;
                v.uv = [0.25, 0.75];
                v.glint_uv = [17.0, 0.0];
            }
            let mut rest = Vec::new();
            for hidden in [false, true] {
                geometry.prepare(
                    &device,
                    &queue,
                    &layout.layout,
                    &mixed,
                    &[EntitySpan {
                        start: 0,
                        id: 77,
                        stage: render_stage("BLOCK_ENTITIES"),
                        hidden,
                        ..Default::default()
                    }],
                    [true; PROGRAMS],
                    ([0.; 4], [1; 2]),
                    Casters {
                        blocks: true,
                        ..Default::default()
                    },
                    &mut rest,
                    None,
                    (&[], &[]),
                );
                assert_eq!(
                    geometry.scratch.len(),
                    4,
                    "the model still uses the shader pack"
                );
                assert_eq!(geometry.draws.len(), 1);
                assert_eq!(
                    (geometry.draws[0].pack, geometry.draws[0].shadow),
                    (!hidden, true)
                );
                if hidden {
                    assert!(
                        rest.is_empty(),
                        "hidden native fluid must not leak into the visible pass"
                    );
                } else {
                    assert_eq!(
                        bytemuck::cast_slice::<_, u8>(&rest),
                        bytemuck::cast_slice::<_, u8>(&mixed[4..]),
                        "fluid UV/layer payload cannot be sampled from the entity atlas"
                    );
                }
            }
        }
    }

    #[test]
    fn posed_draws_preserve_subject_ids_overlays_and_hidden_shadow_casters() {
        let mut skin = SkinFrame {
            vertices: 24,
            ..Default::default()
        };
        skin.spans = vec![
            EntitySpan {
                start: 0,
                stage: render_stage("ENTITIES"),
                id: 40,
                ..Default::default()
            },
            EntitySpan {
                start: 12,
                stage: render_stage("ENTITIES"),
                id: 41,
                hidden: true,
                ..Default::default()
            },
        ];
        skin.jobs = vec![
            SkinJob {
                dst: 0,
                count: 4,
                overlay: 1,
                ..Default::default()
            },
            SkinJob {
                dst: 4,
                count: 4,
                overlay: 1,
                ..Default::default()
            },
            SkinJob {
                dst: 8,
                count: 12,
                overlay: 5,
                ..Default::default()
            },
            SkinJob {
                dst: 20,
                count: 4,
                overlay: 5,
                ..Default::default()
            },
        ];
        let runs = skin_runs(&skin);
        assert_eq!(
            runs.iter()
                .map(|(s, r, o)| (s.id, r.clone(), *o, s.hidden))
                .collect::<Vec<_>>(),
            [
                (40, 0..8, 1, false),
                (40, 8..12, 5, false),
                (41, 12..24, 5, true)
            ]
        );
        assert_eq!(entity_color(runs[0].2)[3], 0.30196077);
        assert!(
            Casters {
                entities: true,
                ..Default::default()
            }
            .cast(&runs[2].0, ENTITY)
        );
        skin.truncate((2, 0, 8));
        assert_eq!(skin.vertices, 8);
        assert_eq!(skin.spans.len(), 1);
    }

    #[test]
    fn end_portal_faces_reach_the_shadow_pass_with_their_block_entity() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let layout = super::super::pack_draw::DrawUniforms::new(&device);
        let mut geometry = PackEntities::new(&device, &layout.layout);
        // One block entity: a plain face, then an End portal face (overlay layer 9).
        let cpu: Vec<Vertex> = [0, 0, 0, 0, 9, 9, 9, 9]
            .into_iter()
            .map(|layer| Vertex {
                layer,
                ..bytemuck::Zeroable::zeroed()
            })
            .collect();
        let spans = [EntitySpan {
            start: 0,
            id: 99,
            stage: render_stage("BLOCK_ENTITIES"),
            ..Default::default()
        }];
        let mut rest = Vec::new();
        geometry.prepare(
            &device,
            &queue,
            &layout.layout,
            &cpu,
            &spans,
            [true; PROGRAMS],
            ([0.; 4], [1; 2]),
            Casters {
                blocks: true,
                ..Default::default()
            },
            &mut rest,
            None,
            (&[], &[]),
        );
        assert_eq!(
            (geometry.draws.iter())
                .map(|d| (d.portal, d.shadow))
                .collect::<Vec<_>>(),
            [(false, true), (true, true)],
            "the portal face casts as its block entity does"
        );
    }

    /// A boat's water patch (`RenderTypes.waterMask`) is a draw of its own, depth only, that casts
    /// no shadow; with no pack program for entities, Horizon draws it with the boat.
    #[test]
    fn a_boats_water_patch_draws_depth_only() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let layout = super::super::pack_draw::DrawUniforms::new(&device);
        let mut geometry = PackEntities::new(&device, &layout.layout);
        let mask = super::super::entity_batch::WATER_MASK_OVERLAY;
        // One boat: a hull face, then the water patch.
        let cpu: Vec<Vertex> = [0, 0, 0, 0, mask, mask, mask, mask]
            .into_iter()
            .map(|layer| Vertex {
                layer,
                ..bytemuck::Zeroable::zeroed()
            })
            .collect();
        let spans = [EntitySpan {
            start: 0,
            id: 7,
            stage: render_stage("ENTITIES"),
            ..Default::default()
        }];
        let mut rest = Vec::new();
        let shadows = Casters {
            entities: true,
            ..Default::default()
        };
        let mut prepare = |drawn, rest: &mut Vec<Vertex>| {
            geometry.prepare(
                &device,
                &queue,
                &layout.layout,
                &cpu,
                &spans,
                drawn,
                ([0.; 4], [1; 2]),
                shadows,
                rest,
                None,
                (&[], &[]),
            );
            let draws = (geometry.draws.iter())
                .map(|d| (d.base, d.program, d.pack, d.shadow, d.water_mask))
                .collect::<Vec<_>>();
            let heights: Vec<f32> = geometry.scratch.iter().map(|v| v.pos[1]).collect();
            (draws, heights)
        };
        assert_eq!(
            prepare([true; PROGRAMS], &mut rest),
            (
                vec![
                    (0, ENTITY, true, true, false),
                    (4, ENTITY, true, false, true)
                ],
                // Where they lie: the patch at the water line, not moved.
                vec![0.0; 8]
            )
        );
        assert!(rest.is_empty());
        let mut no_entities = [true; PROGRAMS];
        no_entities[ENTITY] = false;
        let (draws, _) = prepare(no_entities, &mut rest);
        assert!(draws.iter().all(|d| !d.2), "{draws:?}");
        assert_eq!(rest.len(), 8, "the boat and its patch stay Horizon's");
    }

    #[test]
    fn posed_and_cpu_draws_keep_offsets_uniforms_and_special_fallbacks() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let layout = super::super::pack_draw::DrawUniforms::new(&device);
        let mut geometry = PackEntities::new(&device, &layout.layout);
        let entity = |start, id| EntitySpan {
            start,
            id,
            stage: render_stage("ENTITIES"),
            ..Default::default()
        };
        let mut skin = SkinFrame {
            vertices: 12,
            spans: vec![
                entity(0, 40),
                EntitySpan {
                    hidden: true,
                    ..entity(4, 41)
                },
                EntitySpan {
                    start: 8,
                    id: 99,
                    stage: render_stage("BLOCK_ENTITIES"),
                    emissive: true,
                    ..Default::default()
                },
            ],
            jobs: [1 | (2 << 16), 5, 0]
                .into_iter()
                .enumerate()
                .map(|(i, overlay)| SkinJob {
                    dst: i as u32 * 4,
                    count: 4,
                    overlay,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let cpu = vec![
            Vertex {
                layer: 0,
                ..bytemuck::Zeroable::zeroed()
            };
            8
        ];
        let spans = [entity(4, 50)];
        let mut rest = Vec::new();
        let shadows = Casters {
            entities: true,
            blocks: true,
            ..Default::default()
        };
        geometry.prepare(
            &device,
            &queue,
            &layout.layout,
            &cpu,
            &spans,
            [true; PROGRAMS],
            ([0.; 4], [1; 2]),
            shadows,
            &mut rest,
            Some(&skin),
            (&[], &[]),
        );
        assert_eq!(rest.len(), 4, "untagged CPU prefix stays in Horizon's draw");
        assert_eq!(geometry.scratch.len(), 4, "only CPU geometry is uploaded");
        assert_eq!(
            geometry
                .draws
                .iter()
                .map(|d| (d.base, d.quads, d.pack, d.shadow))
                .collect::<Vec<_>>(),
            [
                (0, 1, true, true),
                (4, 1, false, true),
                (8, 1, true, true),
                (12, 1, true, true)
            ]
        );
        let slot = |i: usize| &geometry.uniforms[i * geometry.stride as usize..][..SLOT as usize];
        let word = |bytes: &[u8], at| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!((word(slot(0), 4), word(slot(0), 8)), (40, 0));
        assert_eq!((word(slot(1), 4), word(slot(1), 8)), (41, 0));
        assert_eq!((word(slot(2), 4), word(slot(2), 8)), (0, 99));
        assert_eq!((word(slot(3), 4), word(slot(3), 8)), (50, 0));
        let hurt: &[f32] = bytemuck::cast_slice(&slot(0)[16..32]);
        assert_eq!(&hurt[..3], &[1.0, 0.0, 0.0]);
        assert!((hurt[3] - 77.0 / 255.0).abs() < 1e-6);
        let flash: &[f32] = bytemuck::cast_slice(&slot(1)[16..32]);
        assert_eq!(&flash[..3], &[1.0, 1.0, 1.0]);
        assert!((flash[3] - 192.0 / 255.0).abs() < 1e-6);
        assert_eq!(word(slot(2), 0), render_stage("BLOCK_ENTITIES") as i32);

        let mut special = cpu.clone();
        special[4..].iter_mut().for_each(|v| v.layer = 2);
        geometry.prepare(
            &device,
            &queue,
            &layout.layout,
            &special,
            &spans,
            [true; PROGRAMS],
            ([0.; 4], [1; 2]),
            shadows,
            &mut rest,
            None,
            (&[], &[]),
        );
        assert!(geometry.draws.is_empty());
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&rest),
            bytemuck::cast_slice::<_, u8>(&special)
        );

        // Culling can retain hidden jobs for a pack whose entity caster is absent.
        // The compute output must still fit, even though no visible/shadow draw uses it.
        skin.vertices = 5000;
        skin.jobs.truncate(1);
        skin.jobs[0].count = 5000;
        skin.spans = vec![EntitySpan {
            hidden: true,
            ..entity(0, 60)
        }];
        geometry.prepare(
            &device,
            &queue,
            &layout.layout,
            &[],
            &[],
            [false; PROGRAMS],
            ([0.; 4], [1; 2]),
            Casters::default(),
            &mut rest,
            Some(&skin),
            (&[], &[]),
        );
        assert!(geometry.draws.is_empty());
        assert!(geometry.skin_target().size() >= 5000 * 44);
    }
}

#[cfg(test)]
mod velocity_tests {
    use super::{Motion, NO_PREVIOUS};

    /// Two mobs (ids 7 and 9) and a run of unmarked geometry (id 0); `copied` places CPU
    /// vertices `first..first + n` at buffer index `at`.
    fn frame(order: [(u64, u32); 2], copied: &[(u32, u32, u32)]) -> Motion {
        let mut marks = Vec::new();
        let mut cpu = 0;
        for (id, n) in order {
            marks.push((cpu, id));
            cpu += n;
        }
        marks.push((cpu, 0));
        let mut m = Motion::default();
        m.collect(&marks, &[], cpu + 2, 0, copied);
        m
    }

    /// The same mob's vertices find themselves however the frame's order and the buffer's
    /// layout moved between frames; geometry of no entity, and vertices not in the buffer,
    /// find nothing.
    #[test]
    fn each_vertex_finds_its_copy_from_last_frame() {
        // Last frame: mob 7 (4 vertices) then mob 9 (2), copied whole from buffer index 10.
        let before = frame([(7, 4), (9, 2)], &[(0, 10, 8)]);
        // This frame: mob 9 first, then mob 7, at buffer index 0; 9's last vertex not drawn.
        let now = frame([(9, 2), (7, 4)], &[(0, 0, 1), (2, 1, 6)]);
        let mut out = vec![NO_PREVIOUS; 8];
        now.link(&before, &mut out);
        // 9's first vertex (buffer 0) was 10 + 4; 7's four (buffer 1..5) were 10..14.
        assert_eq!(
            out,
            [14, 10, 11, 12, 13, NO_PREVIOUS, NO_PREVIOUS, NO_PREVIOUS]
        );
    }

    /// A mob whose vertex count changed (armor put on, an item picked up) gets no velocity
    /// this frame rather than the wrong vertices'.
    #[test]
    fn a_changed_model_gets_none() {
        let before = frame([(7, 4), (9, 2)], &[(0, 0, 8)]);
        let now = frame([(7, 6), (9, 2)], &[(0, 0, 10)]);
        let mut out = vec![NO_PREVIOUS; 10];
        now.link(&before, &mut out);
        assert!(out[..6].iter().all(|&i| i == NO_PREVIOUS));
        assert_eq!(out[6..8], [4, 5]);
    }

    /// GPU-posed vertices come first in the buffer: each mob's posed part is matched by its
    /// own posed count, then its CPU part.
    #[test]
    fn posed_and_cpu_parts_match_separately() {
        let marks = [(0, 7), (2, 9), (2, 0)];
        let mut before = Motion::default();
        // Mob 7: 3 posed (0..3) and 2 CPU; mob 9: 1 posed (3), no CPU.
        before.collect(&marks, &[0, 3, 4], 2, 4, &[(0, 4, 2)]);
        let mut now = Motion::default();
        // Mob 9 posed first this frame (0), then mob 7 (1..4).
        now.collect(&[(0, 9), (0, 7), (2, 0)], &[0, 1, 4], 2, 4, &[(0, 4, 2)]);
        let mut out = vec![NO_PREVIOUS; 6];
        now.link(&before, &mut out);
        assert_eq!(out, [3, 0, 1, 2, 4, 5]);
    }
}

#[cfg(test)]
mod velocity_translate_tests {
    /// A pack that reads `at_velocity` translates with the entity inputs (which read the
    /// previous view and last frame's vertices) and with the hand's (which read zero).
    #[test]
    fn a_pack_reading_at_velocity_translates_for_entities_and_the_hand() {
        let dir = std::env::temp_dir().join(format!("hz-at-velocity-{}", std::process::id()));
        let shaders = dir.join("shaders");
        std::fs::create_dir_all(&shaders).unwrap();
        std::fs::write(
            shaders.join("gbuffers_entities.vsh"),
            "#version 120\nattribute vec3 at_velocity;\nvarying vec3 v;\n\
             void main() {\n    gl_Position = ftransform();\n    v = at_velocity;\n}\n",
        )
        .unwrap();
        std::fs::write(
            shaders.join("gbuffers_entities.fsh"),
            "#version 120\nvarying vec3 v;\nvoid main() {\n    gl_FragData[0] = vec4(v, 1.0);\n}\n",
        )
        .unwrap();
        let pack = horizon_shaders::Pack::open(&dir).unwrap();
        let m = horizon_shaders::Macros::standard();
        let vsh = horizon_shaders::preprocess(&pack, "gbuffers_entities.vsh", &m);
        let fsh = horizon_shaders::preprocess(&pack, "gbuffers_entities.fsh", &m);
        for (who, inputs) in [
            ("entities", super::inputs()),
            ("hand", super::super::pack_hand::inputs()),
        ] {
            let p = horizon_shaders::translate_gbuffers(
                &vsh,
                &fsh,
                horizon_shaders::Target::Buffers,
                &inputs,
            )
            .unwrap_or_else(|e| panic!("{who}: {e:?}"));
            let previous = p
                .interface
                .uniforms
                .iter()
                .any(|u| u.name == "gbufferPreviousModelView");
            assert_eq!(
                previous,
                who == "entities",
                "{who}: previous view in the block"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tier_translate_tests {
    /// A pack's entity and hand programs may read `horizonTier`, the overlay's HDR tier.
    #[test]
    fn a_pack_reading_horizon_tier_translates_for_entities_and_the_hand() {
        let dir = std::env::temp_dir().join(format!("hz-entity-tier-{}", std::process::id()));
        let shaders = dir.join("shaders");
        std::fs::create_dir_all(&shaders).unwrap();
        std::fs::write(
            shaders.join("gbuffers_entities.vsh"),
            "#version 120\nattribute float horizonTier;\nvarying float t;\n\
             void main() {\n    gl_Position = ftransform();\n    t = horizonTier;\n}\n",
        )
        .unwrap();
        std::fs::write(
            shaders.join("gbuffers_entities.fsh"),
            "#version 120\nvarying float t;\nvoid main() {\n    gl_FragData[0] = vec4(t);\n}\n",
        )
        .unwrap();
        let pack = horizon_shaders::Pack::open(&dir).unwrap();
        let m = horizon_shaders::Macros::standard();
        let vsh = horizon_shaders::preprocess(&pack, "gbuffers_entities.vsh", &m);
        let fsh = horizon_shaders::preprocess(&pack, "gbuffers_entities.fsh", &m);
        for (who, inputs) in [
            ("entities", super::inputs()),
            ("hand", super::super::pack_hand::inputs()),
        ] {
            let attribute = inputs.attributes.iter().find(|(n, _)| n == "horizonTier");
            assert!(attribute.is_some(), "{who}");
            horizon_shaders::translate_gbuffers(
                &vsh,
                &fsh,
                horizon_shaders::Target::Buffers,
                &inputs,
            )
            .unwrap_or_else(|e| panic!("{who}: {e:?}"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod material_route_tests;
