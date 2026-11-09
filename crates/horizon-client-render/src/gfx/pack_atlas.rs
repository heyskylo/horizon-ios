//! The block atlas of shader packs (spec 5.1, `atlasSize` in 5.2): packs sample one 2D atlas
//! with atlas coordinates, Horizon keeps block sprites in texture arrays. With a pack, every
//! layer is copied into the atlas, and an animated layer again whenever its frame changes.
//!
//! Flat draws, which read a table per sprite layer, get the game's own layout
//! ([`PackAtlas::stitched`]): every sprite at its own size, placed by vanilla's `Stitcher` with its
//! padding; this gives the `atlasSize` and spans our probes read in Iris 1.11.7. Other draws get
//! a grid of array-sized cells ([`PackAtlas::new`]).

#[path = "pack_atlas/stitch.rs"]
mod stitch;

pub(super) struct PackAtlas {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// Sprites per row.
    pub columns: u32,
    /// A sprite's edge in pixels, at the first level.
    pub sprite: u32,
    /// Layers per sprite array.
    split: u32,
    /// Shader policy: expose the per-quad ambient occlusion in `hz_Color.a`.
    pub separate_ao: bool,
    /// The stitched layout, when the atlas has one.
    stitched: Option<Stitched>,
}

/// A stitched atlas: where each layer's sprite is, and the pipeline that draws them in.
struct Stitched {
    /// Each layer's sprite at the first level: its top left corner and its side.
    rects: Vec<[u32; 3]>,
    padding: u32,
    material: bool,
    pipeline: wgpu::RenderPipeline,
    /// One per sprite array.
    groups: Vec<wgpu::BindGroup>,
}

/// One sprite drawn into one level of a stitched atlas (`BLIT`'s instance).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlitSprite {
    /// The sprite's top left corner, side and padding at that level.
    rect: [i32; 4],
    /// Its layer in its array, the array's level it comes from, the atlas level's size.
    source: [u32; 4],
}

/// Draws sprites into a stitched atlas level, padding included: a pixel of the padding repeats
/// the sprite's nearest edge pixel, as vanilla's `animate_sprite` shaders do with a clamped sampler.
const BLIT: &str = r#"
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) rect: vec4<i32>,
    @location(1) @interpolate(flat) source: vec2<u32>,
}
@group(0) @binding(0) var sprites: texture_2d_array<f32>;
@vertex
fn vs(@builtin(vertex_index) v: u32, @location(0) rect: vec4<i32>, @location(1) source: vec4<u32>) -> Out {
    var corners = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    let pixel = vec2<f32>(rect.xy - rect.w) + corners[v] * f32(rect.z + 2 * rect.w);
    let size = vec2<f32>(source.zw);
    var out: Out;
    out.position = vec4(pixel.x / size.x * 2.0 - 1.0, 1.0 - pixel.y / size.y * 2.0, 0.0, 1.0);
    out.rect = rect;
    out.source = source.xy;
    return out;
}
fn sprite_texel(in: Out) -> vec2<i32> {
    return clamp(vec2<i32>(floor(in.position.xy)) - in.rect.xy, vec2(0), vec2(in.rect.z - 1));
}
@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    return textureLoad(sprites, sprite_texel(in), i32(in.source.x), i32(in.source.y));
}
@fragment
fn fs_material(in: Out) -> @location(0) vec4<f32> {
    let texel = sprite_texel(in);
    let edge = i32(textureDimensions(sprites, 0).x);
    let sampled = min((texel * 2 + 1) * edge / (2 * in.rect.z), vec2(edge - 1));
    return textureLoad(sprites, sampled, i32(in.source.x), i32(in.source.y));
}
"#;

/// The side a layer of `sprite`-wide arrays has in a stitched atlas: its own, when the arrays
/// hold it scaled up by a power of two (else the array's).
fn atlas_side(native: u32, sprite: u32) -> u32 {
    if native == 0
        || native > sprite
        || !sprite.is_multiple_of(native)
        || !(sprite / native).is_power_of_two()
    {
        sprite
    } else {
        native
    }
}

/// `SpriteLoader.stitch`'s mip level: the option, lowered to fit the smallest sprite.
fn stitch_mip_level(sides: &[u32], option: u32) -> u32 {
    let mut lowest = 1u32 << option.min(16);
    let mut smallest = u32::MAX;
    for &side in sides {
        smallest = smallest.min(side);
        lowest = lowest.min(side & side.wrapping_neg());
    }
    let level = smallest.min(lowest).max(1).ilog2();
    level.min(option)
}

impl PackAtlas {
    /// Copies `layers` layers of `arrays` (`split` per array, `sprite` pixels wide, `levels`
    /// mip levels); `None` when the grid is larger than the device allows.
    pub fn new(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arrays: &[wgpu::Texture],
        split: u32,
        layers: u32,
        sprite: u32,
    ) -> Option<Self> {
        Self::with_levels(
            device,
            encoder,
            arrays,
            split,
            layers,
            sprite,
            arrays[0].mip_level_count(),
        )
    }

    /// Encoded material channels must select an existing texel, including at distance.
    pub fn material(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arrays: &[wgpu::Texture],
        split: u32,
        layers: u32,
    ) -> Option<Self> {
        Self::with_levels(device, encoder, arrays, split, layers, arrays[0].width(), 1)
    }

    fn with_levels(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arrays: &[wgpu::Texture],
        split: u32,
        layers: u32,
        sprite: u32,
        levels: u32,
    ) -> Option<Self> {
        let layers = layers.max(1);
        let columns = (f64::from(layers).sqrt().ceil() as u32).max(1);
        let rows = layers.div_ceil(columns);
        let max = device.limits().max_texture_dimension_2d;
        if columns * sprite > max || rows * sprite > max {
            return None;
        }
        // Levels past a sprite's single pixel would mix neighbours: stop there.
        let levels = levels.min(sprite.ilog2() + 1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shader pack block atlas"),
            size: wgpu::Extent3d {
                width: columns * sprite,
                height: rows * sprite,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let atlas = Self {
            texture,
            view,
            columns,
            sprite,
            split,
            separate_ao: false,
            stitched: None,
        };
        atlas.copy(device, encoder, arrays, 0..layers);
        Some(atlas)
    }

    /// Every layer of `arrays` at its own size (`native`, its side before the arrays scaled it up
    /// to `sprite`), placed as vanilla's `Stitcher` places them with the padding of the Mipmap
    /// Levels option and the anisotropy bit; `None` when they don't fit the device.
    #[allow(clippy::too_many_arguments)]
    pub fn stitched(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arrays: &[wgpu::Texture],
        split: u32,
        sprite: u32,
        native: &[u32],
        mip_option: u32,
        anisotropy_bit: u32,
    ) -> Option<Self> {
        let sides: Vec<u32> = if native.is_empty() {
            vec![sprite]
        } else {
            native.iter().map(|&n| atlas_side(n, sprite)).collect()
        };
        // A level of the atlas comes from the arrays' level as many halvings further down.
        let array_levels = arrays[0].mip_level_count();
        let deepest = sides
            .iter()
            .map(|&s| (sprite / s).ilog2())
            .max()
            .unwrap_or(0);
        let mip =
            stitch_mip_level(&sides, mip_option).min(array_levels.saturating_sub(deepest + 1));
        let max = device.limits().max_texture_dimension_2d;
        let layout = stitch::stitch(&sides, mip, anisotropy_bit, max)?;
        let rects = layout
            .holders
            .iter()
            .zip(&sides)
            .map(|(&[x, y], &side)| [x + layout.padding, y + layout.padding, side])
            .collect();
        Some(Self::with_rects(
            device,
            encoder,
            arrays,
            split,
            sprite,
            [layout.width.max(1), layout.height.max(1)],
            rects,
            layout.padding,
            mip + 1,
            false,
        ))
    }

    /// Companion maps use the colour atlas's coordinates, not a second packing.
    pub fn material_for(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arrays: &[wgpu::Texture],
        layers: u32,
    ) -> Option<Self> {
        let Some(stitched) = &self.stitched else {
            return Self::material(device, encoder, arrays, self.split, layers);
        };
        Some(Self::with_rects(
            device,
            encoder,
            arrays,
            self.split,
            arrays[0].width(),
            self.size(),
            stitched.rects.clone(),
            stitched.padding,
            1,
            true,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn with_rects(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arrays: &[wgpu::Texture],
        split: u32,
        sprite: u32,
        size: [u32; 2],
        rects: Vec<[u32; 3]>,
        padding: u32,
        levels: u32,
        material: bool,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shader pack block atlas"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader pack atlas sprites"),
            source: wgpu::ShaderSource::Wgsl(BLIT.into()),
        });
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shader pack atlas sprites"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shader pack atlas sprites"),
            bind_group_layouts: &[Some(&group_layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shader pack atlas sprites"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BlitSprite>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Sint32x4, 1 => Uint32x4],
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(if material { "fs_material" } else { "fs" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let groups = arrays
            .iter()
            .map(|array| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Shader pack atlas sprites"),
                    layout: &group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&array.create_view(
                            &wgpu::TextureViewDescriptor {
                                dimension: Some(wgpu::TextureViewDimension::D2Array),
                                ..Default::default()
                            },
                        )),
                    }],
                })
            })
            .collect();
        let layers = rects.len() as u32;
        let atlas = Self {
            texture,
            view,
            columns: 1,
            sprite,
            split,
            separate_ao: false,
            stitched: Some(Stitched {
                rects,
                padding,
                material,
                pipeline,
                groups,
            }),
        };
        atlas.copy(device, encoder, arrays, 0..layers);
        atlas
    }

    /// Whether the sprites keep their own sizes ([`PackAtlas::stitched`]).
    pub fn is_stitched(&self) -> bool {
        self.stitched.is_some()
    }

    /// The table flat terrain reads (`hz_sprite_rects`): per layer, its sprite's top left corner
    /// (x | y << 16) and side, in atlas pixels.
    pub fn rects_table(&self) -> Vec<u32> {
        if let Some(stitched) = &self.stitched {
            return stitched
                .rects
                .iter()
                .flat_map(|&[x, y, side]| [x | (y << 16), side])
                .collect();
        }
        (0..self.columns * self.texture.height() / self.sprite)
            .flat_map(|layer| {
                let (x, y) = (layer % self.columns, layer / self.columns);
                [(x * self.sprite) | ((y * self.sprite) << 16), self.sprite]
            })
            .collect()
    }

    /// The atlas's size in pixels (`atlasSize`).
    pub fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }

    /// Copies `layers` from the sprite arrays again, every level.
    pub fn copy(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        arrays: &[wgpu::Texture],
        layers: impl IntoIterator<Item = u32>,
    ) {
        if let Some(stitched) = &self.stitched {
            return self.draw_sprites(stitched, device, encoder, layers);
        }
        for layer in layers {
            let Some(array) = arrays.get((layer / self.split) as usize) else {
                continue;
            };
            let (column, row) = (layer % self.columns, layer / self.columns);
            for level in 0..self.texture.mip_level_count() {
                let edge = self.sprite >> level;
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: array,
                        mip_level: level,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer % self.split,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.texture,
                        mip_level: level,
                        origin: wgpu::Origin3d {
                            x: column * edge,
                            y: row * edge,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: edge,
                        height: edge,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }

    /// Draws `layers` into every level of a stitched atlas, from the arrays' level where each
    /// sprite has its own size.
    fn draw_sprites(
        &self,
        stitched: &Stitched,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        layers: impl IntoIterator<Item = u32>,
    ) {
        let mut layers: Vec<u32> = layers
            .into_iter()
            .filter(|&l| (l as usize) < stitched.rects.len())
            .collect();
        if layers.is_empty() {
            return;
        }
        layers.sort_unstable_by_key(|&l| (l / self.split, l));
        let levels = self.texture.mip_level_count();
        let mut sprites = Vec::with_capacity(layers.len() * levels as usize);
        for level in 0..levels {
            let size = [
                self.texture.width() >> level,
                self.texture.height() >> level,
            ];
            for &layer in &layers {
                let [x, y, side] = stitched.rects[layer as usize];
                sprites.push(BlitSprite {
                    rect: [x, y, side, stitched.padding].map(|v| (v >> level) as i32),
                    source: [
                        layer % self.split,
                        if stitched.material {
                            0
                        } else {
                            (self.sprite / side).ilog2() + level
                        },
                        size[0],
                        size[1],
                    ],
                });
            }
        }
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Shader pack atlas sprites"),
                contents: bytemuck::cast_slice(&sprites),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let per_level = layers.len() as u32;
        for level in 0..levels {
            let view = self.texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack atlas sprites"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&stitched.pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            let first = level * per_level;
            let mut start = 0;
            while start < layers.len() {
                let array = layers[start] / self.split;
                let end = start
                    + layers[start..]
                        .iter()
                        .take_while(|&&l| l / self.split == array)
                        .count();
                if let Some(group) = stitched.groups.get(array as usize) {
                    pass.set_bind_group(0, group, &[]);
                    pass.draw(0..6, first + start as u32..first + end as u32);
                }
                start = end;
            }
        }
    }
}

/// A `TerrainVertex` of flat draws' arena page (`hz_vertices`, six words each) into the vertex
/// inputs, for vertex `hz_vertex_index`.
pub const PULL: &str = "        uint hz_pw = hz_vertex_index * 6u;\n        \
    hz_v_pos = uintBitsToFloat(uvec3(hz_vertices[hz_pw], hz_vertices[hz_pw + 1u], hz_vertices[hz_pw + 2u]));\n        \
    hz_v_uv = uvec2(hz_vertices[hz_pw + 3u] & 65535u, hz_vertices[hz_pw + 3u] >> 16u);\n        \
    hz_v_layer = hz_vertices[hz_pw + 4u] & 65535u;\n        \
    uint hz_pl = hz_vertices[hz_pw + 4u] >> 16u;\n        \
    hz_v_light = uvec2(hz_pl & 255u, hz_pl >> 8u);\n        \
    hz_v_color = unpackUnorm4x8(hz_vertices[hz_pw + 5u]);\n";

/// GLSL: the face normal and tangent of a flat-drawn vertex, from its quad's first three corners
/// (quads are four consecutive vertices of the page; position words 0-2, UV word 3).
const FACE: &str = "    uint hz_q = (uint(gl_VertexIndex) & ~3u) * 6u;\n    \
    vec3 hz_p0 = uintBitsToFloat(uvec3(hz_vertices[hz_q], hz_vertices[hz_q + 1u], hz_vertices[hz_q + 2u]));\n    \
    vec3 hz_p1 = uintBitsToFloat(uvec3(hz_vertices[hz_q + 6u], hz_vertices[hz_q + 7u], hz_vertices[hz_q + 8u]));\n    \
    vec3 hz_p2 = uintBitsToFloat(uvec3(hz_vertices[hz_q + 12u], hz_vertices[hz_q + 13u], hz_vertices[hz_q + 14u]));\n    \
    uvec3 hz_w = uvec3(hz_vertices[hz_q + 3u], hz_vertices[hz_q + 9u], hz_vertices[hz_q + 15u]);\n    \
    vec2 hz_t0 = vec2(hz_w.x & 65535u, hz_w.x >> 16u);\n    \
    vec2 hz_d1 = vec2(hz_w.y & 65535u, hz_w.y >> 16u) - hz_t0;\n    \
    vec2 hz_d2 = vec2(hz_w.z & 65535u, hz_w.z >> 16u) - hz_t0;\n    \
    uint hz_w3 = hz_vertices[hz_q + 21u];\n    \
    vec2 hz_t3 = vec2(hz_w3 & 65535u, hz_w3 >> 16u);\n    \
    hz_mid = (clamp(hz_t0 * hz_uv_scale, 0.0, 1.0) + clamp((hz_t0 + hz_d1) * hz_uv_scale, 0.0, 1.0)\n        \
        + clamp((hz_t0 + hz_d2) * hz_uv_scale, 0.0, 1.0) + clamp(hz_t3 * hz_uv_scale, 0.0, 1.0)) * 0.25;\n    \
    vec3 hz_e1 = hz_p1 - hz_p0;\n    \
    vec3 hz_e2 = hz_p2 - hz_p0;\n    \
    vec3 hz_n = cross(hz_e1, hz_e2);\n    \
    if (dot(hz_n, hz_n) > 1e-12) {\n        \
        hz_Normal = normalize(hz_n);\n        \
        float hz_r = hz_d1.x * hz_d2.y - hz_d2.x * hz_d1.y;\n        \
        vec3 hz_t = (hz_e1 * hz_d2.y - hz_e2 * hz_d1.y) * sign(hz_r);\n        \
        vec3 hz_b = (hz_e2 * hz_d1.x - hz_e1 * hz_d2.x) * sign(hz_r);\n        \
        if (dot(hz_t, hz_t) > 1e-12) {\n            \
            hz_t = normalize(hz_t - hz_Normal * dot(hz_Normal, hz_t));\n            \
            hz_tangent = vec4(hz_t, dot(cross(hz_t, hz_Normal), hz_b) < 0.0 ? -1.0 : 1.0);\n        \
        }\n    \
    }\n";

/// `terrain` (`PackAtlas::terrain_inputs`) drawing only solid terrain, then only cutout terrain:
/// each leaves the other part's quads outside the clip volume. Solid: the quads over opaque
/// texels and opaque leaves (`mesher::OPAQUE_LAYER`), as vanilla picks `ChunkSectionLayer.SOLID`
/// per quad (`FaceBakery.computeMaterialTransparency`, `SectionCompiler.compile`), and lava's
/// faces (sprite layers `lava`, its still and flowing sprites), which Iris 1.11.7 drew with
/// `gbuffers_terrain_solid` in our probes, with and without `separateAo`.
///
/// The test reads only the vertex's sprite layer, which all four vertices of a quad share: a
/// quad lands whole in one part. The colour's alpha can't decide it, as with `separateAo` it
/// holds each vertex's own ambient occlusion.
pub fn terrain_parts(
    terrain: &horizon_shaders::GbuffersInputs,
    lava: &[u16],
) -> [horizon_shaders::GbuffersInputs; 2] {
    let opaque = crate::mesher::OPAQUE_LAYER;
    let lava: String = lava
        .iter()
        .map(|l| format!(" || hz_v_layer == {l}u"))
        .collect();
    let part = |solid: bool| {
        let mut inputs = terrain.clone();
        inputs.after.push_str(&format!(
            "    bool hz_solid = (hz_v_layer & {opaque}u) != 0u{lava};\n    \
             if (hz_solid != {solid}) gl_Position = vec4(2.0, 2.0, 2.0, 1.0);\n"
        ));
        inputs
    };
    [part(true), part(false)]
}

impl PackAtlas {
    /// The vertex inputs of a gbuffers program drawing terrain (`TerrainVertex`, terrain.wgsl's
    /// `vs_terrain` or, for `flat` draws, `vs_terrain_flat`), in atlas coordinates.
    pub fn terrain_inputs(&self, flat: bool) -> horizon_shaders::GbuffersInputs {
        let mut declarations = String::from(
            "layout(location = 0) in vec3 hz_v_pos;\nlayout(location = 1) in uvec2 hz_v_uv;\n\
             layout(location = 2) in uint hz_v_layer;\nlayout(location = 3) in vec4 hz_v_color;\n\
             layout(location = 4) in uvec2 hz_v_light;\n",
        );
        let origin = if flat {
            declarations.push_str(
                "layout(set = 1, binding = 0) readonly buffer HzBlockSlots { uint hz_block_slots[]; };\n\
                 layout(set = 1, binding = 1) readonly buffer HzSlotOrigins { vec4 hz_slot_origins[]; };\n",
            );
            "hz_slot_origins[hz_block_slots[uint(gl_VertexIndex) >> 8u]]"
        } else {
            declarations.push_str("layout(location = 5) in vec4 hz_v_origin;\n");
            "hz_v_origin"
        };
        declarations.push_str("layout(set = 3, binding = 0) readonly buffer HzBlockIds { int hz_block_ids[]; };\nlayout(set = 3, binding = 1) readonly buffer HzVertices { uint hz_vertices[]; };\nlayout(set = 3, binding = 2) readonly buffer HzQuadStates { uint hz_quad_states[]; };\nlayout(set = 3, binding = 3) readonly buffer HzSpriteRects { uvec2 hz_sprite_rects[]; };\nvec4 hz_mc_entity; vec3 hz_mid_block; vec2 hz_mid_uv; float hz_emission;\n");
        declarations.push_str(&format!(
            "const uint hz_state_words = {}u;\n",
            super::TERRAIN_STATE_WORDS
        ));
        declarations.push_str(&format!(
            "const uint hz_quad_words = {}u;\n",
            super::terrain_metadata::WORDS
        ));
        let [width, height] = self.size();
        // A sprite's cell in its own sides, and the share of the atlas a side is: from the table
        // in flat draws (`rects_table`), from the grid in others.
        let columns = self.columns;
        let (cell, side, scale) = if flat {
            (
                "uvec2 hz_rect = hz_sprite_rects[hz_layer];\n    \
                 hz_cell = vec2(float(hz_rect.x & 65535u), float(hz_rect.x >> 16u)) / float(hz_rect.y);\n    "
                    .to_owned(),
                "float(hz_rect.y)",
                [1.0 / width as f32, 1.0 / height as f32],
            )
        } else {
            (
                format!(
                    "hz_cell = vec2(float(hz_layer % {columns}u), float(hz_layer / {columns}u));\n    "
                ),
                "1.0",
                [
                    self.sprite as f32 / width as f32,
                    self.sprite as f32 / height as f32,
                ],
            )
        };
        // UVs past a sprite (models up to 2, distant tiles) cannot repeat in an atlas: clamped.
        // The vertex alpha retains HDR flags for normal rendering; shader AO is
        // reconstructed exactly from the sidecar below, not borrowed from those bits.
        // A vertex's place in its section goes down to the 32768th of a block below it: read back
        // with a probe pack of ours, Iris 1.11.7's terrain `gl_Vertex` has that for every position
        // we read (water tops at 8/9 - 0.001, plants' offsets), on all three axes (#8464). A fit,
        // not Iris's code. Whole and half blocks, and every 1/16, are on that grid already. A place
        // up to 1/32 of a 32768th (one float step below 16) under a grid line is on it: plants whose
        // random offset is 9/15 of a block read exactly on a half block in Iris, and a 32768th
        // under it in Horizon (our guess, not tested: Horizon's float sum lands just under the
        // line) (#8464). Also a fit.
        let body = format!(
            "    vec4 hz_origin = {origin};\n    \
             uint hz_layer = hz_v_layer & 32767u;\n    \
             {cell}\
             hz_scale = vec2({:?}, {:?}) * {side};\n    \
             hz_Vertex = vec4(floor(hz_v_pos * 32768.0 + 0.03125) / 32768.0 + hz_origin.xyz, 1.0);\n    \
             hz_uv_scale = hz_origin.w > 0.0 ? uintBitsToFloat(floatBitsToUint(hz_origin.w) & 0xff800000u) : fract(abs(hz_origin.w));\n    \
             hz_mid = vec2(0.5);\n    \
             hz_MultiTexCoord0 = vec4((hz_cell + clamp(vec2(hz_v_uv) * hz_uv_scale, 0.0, 1.0)) * hz_scale, 0.0, 1.0);\n    \
             hz_MultiTexCoord1 = vec4(float(hz_v_light.y), float(hz_v_light.x), 0.0, 1.0);\n    \
             hz_MultiTexCoord2 = hz_MultiTexCoord1;\n    \
             hz_Color = vec4(hz_v_color.rgb, 1.0);\n    \
             hz_Normal = vec3(0.0, 1.0, 0.0);\n    \
             hz_tangent = vec4(1.0, 0.0, 0.0, 1.0);\n",
            scale[0], scale[1],
        );
        let mut body = body;
        body.push_str(FACE);
        body.push_str(r#"    hz_mc_entity=vec4(-1.0,0.0,0.0,0.0); hz_mid_block=vec3(0.0); hz_emission=0.0;
    uint hz_meta=(uint(gl_VertexIndex)>>2u)*hz_quad_words;
    uvec2 hz_source=uvec2(hz_quad_states[hz_meta],hz_quad_states[hz_meta+1u]);
    if(hz_source.x!=0xffffffffu){
        vec3 hz_cell_origin=vec3(uvec3(hz_source.y&15u,(hz_source.y>>4u)&15u,(hz_source.y>>8u)&15u));
        hz_mid_block=(hz_cell_origin+vec3(0.5)-hz_v_pos)*64.0;
        if(hz_source.x<uint(hz_block_ids.length())/hz_state_words){
            uint hz_id=hz_state_words*hz_source.x+2u*((hz_source.y>>12u)&1u);
            hz_mc_entity.xy=vec2(float(hz_block_ids[hz_id]),float(hz_block_ids[hz_id+1u]));
            hz_emission=float(hz_block_ids[hz_state_words*hz_source.x+4u]);
        }
        if((hz_source.y&8192u)!=0u){
            vec4 hz_no_dir=unpackUnorm4x8(hz_quad_states[hz_meta+2u+(uint(gl_VertexIndex)&3u)]);
            vec4 hz_tint=unpackUnorm4x8(hz_quad_states[hz_meta+6u]);
            vec4 hz_shaded_tint=unpackUnorm4x8(hz_quad_states[hz_meta+7u]);
            float hz_ao=mix(1.0,hz_no_dir.a,hz_ao_strength);
            if(hz_separate_ao){hz_Color=vec4(hz_old_lighting?hz_shaded_tint.rgb:hz_tint.rgb,hz_ao);}
            else{hz_Color=vec4(hz_old_lighting?mix(hz_shaded_tint.rgb,hz_v_color.rgb,hz_ao_strength):mix(hz_tint.rgb,hz_no_dir.rgb,hz_ao_strength),1.0);}
        }
    }
    vec2 hz_avg_uv=vec2(0.0);
    for(uint hz_c=0u;hz_c<4u;++hz_c){
        uint hz_uv_word=hz_vertices[hz_q+hz_c*6u+3u];
        hz_avg_uv+=clamp(vec2(float(hz_uv_word&65535u),float(hz_uv_word>>16u))*(hz_origin.w > 0.0 ? uintBitsToFloat(floatBitsToUint(hz_origin.w) & 0xff800000u) : fract(abs(hz_origin.w))),0.0,1.0)*0.25;
    }
    hz_mid_uv=(hz_cell+hz_avg_uv)*hz_scale;
"#);
        declarations.push_str(
            "vec2 hz_cell; vec2 hz_scale; vec2 hz_mid; float hz_uv_scale; vec4 hz_tangent;\n",
        );
        let attributes = [
            // section_instance stores 1 - visibility in a positive origin's mantissa.
            // Negative origins encode a distant-terrain handover, fully visible here
            // just as terrain.wgsl's instance_visibility treats them.
            (
                "mc_chunkFade",
                "(hz_origin.w > 0.0 ? 1.0 - float(floatBitsToUint(hz_origin.w) & 0x7fffffu) / 8388608.0 : 1.0)",
            ),
            ("mc_Entity", "hz_mc_entity"),
            ("at_midBlock", "vec4(hz_mid_block, hz_emission)"),
            ("mc_midTexCoord", "vec4(hz_mid_uv, 0.0, 1.0)"),
            ("at_tangent", "hz_tangent"),
            ("vaPosition", "hz_Vertex.xyz"),
            ("vaUV0", "hz_MultiTexCoord0.xy"),
            ("vaUV2", "hz_MultiTexCoord1.xy"),
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
            terrain_lighting: Some(horizon_shaders::TerrainLighting {
                separate_ao: self.separate_ao,
                ..Default::default()
            }),
            alpha_test: None,
            item_id_declared: false,
            far_depth: false,
        }
    }
}

#[cfg(test)]
#[path = "pack_atlas/fade_test.rs"]
mod fade_test;

#[cfg(test)]
#[path = "pack_atlas/midpoint_test.rs"]
mod midpoint_test;

#[cfg(test)]
mod tests {
    use super::*;

    /// Two arrays of 2x2 sprites, one layer split; each layer a colour of its own.
    #[test]
    fn layers_land_in_their_cells() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let (split, layers, sprite) = (3u32, 5u32, 2u32);
        let arrays: Vec<wgpu::Texture> = (0..2)
            .map(|a| {
                let count = (layers - a * split).min(split);
                let pixels: Vec<u8> = (0..count)
                    .flat_map(|l| [(a * split + l) as u8 * 40, 0, 0, 255].repeat(4))
                    .collect();
                wgpu::util::DeviceExt::create_texture_with_data(
                    &device,
                    &queue,
                    &wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: sprite,
                            height: sprite,
                            depth_or_array_layers: count,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &pixels,
                )
            })
            .collect();
        let mut encoder = device.create_command_encoder(&Default::default());
        let atlas = PackAtlas::new(&device, &mut encoder, &arrays, split, layers, sprite).unwrap();
        assert_eq!((atlas.columns, atlas.size()), (3, [6, 4]));
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            atlas.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 6,
                height: 4,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = read.slice(..).get_mapped_range().unwrap();
        let red = |x: usize, y: usize| data[y * 256 + x * 4];
        // Layer 4 (second array, its second layer) is in row 1, column 1.
        assert_eq!([red(0, 0), red(2, 0), red(4, 0)], [0, 40, 80]);
        assert_eq!([red(1, 3), red(3, 2), red(5, 2)], [120, 160, 0]);
    }

    /// An array of `sprite`-pixel layers (`sprite` the largest of `native`), as the game builds
    /// them: each layer scaled up by whole pixels at level 0, its own pixels from the level where
    /// it has its own size. Every pixel of a layer's own image differs: (layer, x, y, 255).
    pub(super) fn upscaled_array(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        native: &[u32],
    ) -> wgpu::Texture {
        let sprite = native.iter().copied().max().unwrap();
        let levels = sprite.ilog2() + 1;
        let mut data = Vec::new();
        for (layer, &side) in native.iter().enumerate() {
            for level in 0..levels {
                let edge = sprite >> level;
                // Above its own size a level repeats each pixel; below, it shrinks to the first.
                let scale = (edge / side).max(1);
                for y in 0..edge {
                    for x in 0..edge {
                        let (x, y) = if edge >= side {
                            (x / scale, y / scale)
                        } else {
                            (0, 0)
                        };
                        data.extend([layer as u8 * 50, x as u8 * 16, y as u8 * 16, 255]);
                    }
                }
            }
        }
        wgpu::util::DeviceExt::create_texture_with_data(
            device,
            queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: sprite,
                    height: sprite,
                    depth_or_array_layers: native.len() as u32,
                },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &data,
        )
    }

    /// Sprites of 2, 4 and 1 px in a 4 px array, no mipmaps (one pixel of padding). Worked through
    /// vanilla's `Stitcher` by hand: holders of 6, 4 and 3 px at (0, 0), (8, 0) and (8, 4) in a 16
    /// by 8 atlas. Each sprite has its own pixels, and its padding repeats its edge.
    #[test]
    fn stitched_sprites_keep_their_own_size_and_padding() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let array = upscaled_array(&device, &queue, &[2, 4, 1]);
        let mut encoder = device.create_command_encoder(&Default::default());
        let atlas =
            PackAtlas::stitched(&device, &mut encoder, &[array], 3, 4, &[2, 4, 1], 0, 0).unwrap();
        assert_eq!(atlas.size(), [16, 8]);
        assert_eq!(
            atlas.rects_table(),
            [9 | 1 << 16, 2, 1 | 1 << 16, 4, 9 | 5 << 16, 1]
        );
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 8,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            atlas.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 16,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = read.slice(..).get_mapped_range().unwrap();
        let at = |x: usize, y: usize| {
            let i = y * 256 + x * 4;
            [data[i], data[i + 1], data[i + 2], data[i + 3]]
        };
        // Layer 0's own 2 px: (layer 0, x, y); its padding repeats the nearest edge pixel.
        assert_eq!(
            [at(9, 1), at(10, 1), at(10, 2)],
            [[0, 0, 0, 255], [0, 16, 0, 255], [0, 16, 16, 255]]
        );
        assert_eq!(
            [at(8, 0), at(11, 3), at(11, 1)],
            [[0, 0, 0, 255], [0, 16, 16, 255], [0, 16, 0, 255]]
        );
        // Layer 1 fills its 4 px, padded; layer 2 is one pixel in the middle of its padding.
        assert_eq!(
            [at(1, 1), at(4, 4), at(0, 5), at(5, 0)],
            [
                [50, 0, 0, 255],
                [50, 48, 48, 255],
                [50, 0, 48, 255],
                [50, 48, 0, 255]
            ]
        );
        assert_eq!([at(9, 5), at(8, 4), at(10, 6)], [[100, 0, 0, 255]; 3]);
    }

    fn material_fixture(animated: bool) {
        let instance = crate::gfx::test_gpu::instance();
        // No adapter (a CI runner without a GPU): skip, as the module's other readback tests do.
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let colour_array = upscaled_array(&device, &queue, &[2, 4, 1]);
        let mut encoder = device.create_command_encoder(&Default::default());
        let colour = PackAtlas::stitched(
            &device,
            &mut encoder,
            &[colour_array],
            3,
            4,
            &[2, 4, 1],
            0,
            0,
        )
        .unwrap();
        for kind in 0..2 {
            // Different source resolution, with encoded channel boundaries and defaults.
            let markers = if kind == 0 {
                [
                    [128, 126, 255, 19],
                    crate::sprites::FLAT_NORMAL,
                    [130, 125, 254, 21],
                ]
            } else {
                [
                    [17, 240, 65, 255],
                    [18, 0, 66, 0],
                    crate::sprites::NO_SPECULAR,
                ]
            };
            let pixels: Vec<_> = markers.iter().flat_map(|m| m.repeat(4)).collect();
            let array = wgpu::util::DeviceExt::create_texture_with_data(
                &device,
                &queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 2,
                        height: 2,
                        depth_or_array_layers: 3,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &pixels,
            );
            let arrays = [array];
            let material = colour
                .material_for(&device, &mut encoder, &arrays, 3)
                .unwrap();
            assert_eq!(material.size(), [16, 8]);
            assert_eq!(material.rects_table(), colour.rects_table());
            assert_eq!(material.texture.mip_level_count(), 1);
            queue.submit([encoder.finish()]);
            encoder = device.create_command_encoder(&Default::default());
            let changed = [243, 1, 229, 0];
            if animated {
                // Alter every source layer, but copy only the animated layer.
                queue.write_texture(
                    arrays[0].as_image_copy(),
                    &changed.repeat(12),
                    wgpu::TexelCopyBufferLayout {
                        bytes_per_row: Some(8),
                        rows_per_image: Some(2),
                        offset: 0,
                    },
                    arrays[0].size(),
                );
                material.copy(&device, &mut encoder, &arrays, [0]);
            }
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 256 * 8,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                material.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &read,
                    layout: wgpu::TexelCopyBufferLayout {
                        bytes_per_row: Some(256),
                        ..Default::default()
                    },
                },
                material.texture.size(),
            );
            queue.submit([encoder.finish()]);
            encoder = device.create_command_encoder(&Default::default());
            read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let data = read.slice(..).get_mapped_range().unwrap();
            // Independent, hand-worked colour rectangles, including all padding pixels.
            for (layer, [x, y, side]) in [[9usize, 1, 2], [1, 1, 4], [9, 5, 1]]
                .into_iter()
                .enumerate()
            {
                let expected = if animated && layer == 0 {
                    changed
                } else {
                    markers[layer]
                };
                for py in y - 1..=y + side {
                    for px in x - 1..=x + side {
                        let at = py * 256 + px * 4;
                        assert_eq!(
                            &data[at..at + 4],
                            &expected,
                            "kind={kind}, layer={layer}, ({px},{py})"
                        );
                    }
                }
            }
        }
        queue.submit([encoder.finish()]);
    }

    #[test]
    fn stitched_material_maps_share_colour_rectangles() {
        material_fixture(false);
    }

    #[test]
    fn animated_stitched_material_updates_only_its_rectangle() {
        material_fixture(true);
    }

    #[test]
    fn terrain_inputs_separate_ao_uses_metadata_not_hdr_alpha() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let (device, _) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut atlas = PackAtlas {
            texture,
            view,
            columns: 1,
            sprite: 16,
            split: 1,
            separate_ao: false,
            stitched: None,
        };
        let inputs_default = atlas.terrain_inputs(false);
        assert!(
            inputs_default
                .body
                .contains("hz_Color = vec4(hz_v_color.rgb, 1.0);")
        );

        atlas.separate_ao = true;
        let inputs_separate = atlas.terrain_inputs(false);
        assert!(inputs_separate.terrain_lighting.unwrap().separate_ao);
        assert!(inputs_separate.body.contains("if(hz_separate_ao)"));
        assert!(
            inputs_separate
                .body
                .contains("hz_Color = vec4(hz_v_color.rgb, 1.0);")
        );
    }
}
