//! A native mod's world warp ([`native_world_warp`](super::native_render::native_world_warp),
//! docs/native-mods.md "World warps"): WGSL that every terrain vertex, far terrain's tiles
//! included, and entity vertex goes through, so a mod can bend the drawn world itself (its real
//! blocks, light and depth, seen from any side) rather than a picture of it.
//!
//! The stock shaders hold only comment markers where a warp goes in (terrain.wgsl's
//! `[world warp ...]`, entity.wgsl's and static_entity.wgsl's): a frame without a warp draws
//! with exactly the shaders it always did. For a warp, copies of them get the markers filled,
//! the declarations of `world_warp.wgsl`, the mod's WGSL and, for the terrain, the pixel stage of
//! `world_warp_fragment.wgsl` with the mod's `world_fragment` or [`DEFAULT_FRAGMENT`]; the
//! pipelines built from them read the warp's uniform at group 2.
//!
//! Those pipelines don't cull back faces (a warp may turn a face over), and a frame drawn with
//! them leaves nothing out by where it was (`Renderer::render`): what is out of view, hidden or
//! facing away may be brought into view. It also keeps to the per-section draws, which these
//! pipelines are built for.
use super::native_render::WorldWarp;
use super::{SolidPipeline, TERRAIN_ATTRIBUTES, TerrainVertex, Vertex};
use glam::DVec3;
use std::sync::Arc;

/// The line in a warp's source after which the terrain's pixel stage begins.
const FRAGMENTS: &str = "// [fragments]";
/// The pixel stage of a warp without one: the terrain as the game draws it, less faces it cut.
pub const DEFAULT_FRAGMENT: &str = "fn world_fragment(f: Fragment) -> Painted {
    if f.cut > 0.0 { return Painted(vec4(0.0, 0.0, 0.0, -1.0), 1.0); }
    return Painted(sprite(f.uv, f.layer) * f.color, 1.0);
}
";
/// What terrain.wgsl's markers are filled with: the warp's two values in `TerrainOut`, and how
/// they cross to the pixel stage in `TerrainWire`, after its five locations.
const VARYINGS: &str = "warp_home: vec4<f32>,
    warp_at: vec3<f32>,";
const WIRE: &str = "@location(5) warp_home: vec4<f32>,
    @location(6) warp_at: vec3<f32>,";
const TO_WIRE: &str = "w.warp_home = o.warp_home;
    w.warp_at = o.warp_at;";
const FROM_WIRE: &str = "o.warp_home = w.warp_home;
    o.warp_at = w.warp_at;";
const VERTEX: &str = "let warp_home = position + origin;
    let warped = world_warp(warp_home, origin);
    let pos = warped.pos;";
const VERTEX_OUT: &str = "out.color = out.color * vec4(max(warped.tint, vec3(0.0)), 1.0);
    out.warp_home = vec4(warp_home, select(0.0, 1.0, warped.tint.r < 0.0));
    out.warp_at = pos;";
/// And entity.wgsl's and static_entity.wgsl's: an entity's vertex is its own `section`; a block
/// entity's is its section's corner.
const ENTITY: &str = "let at = world_warp(position, position).pos;";
const STATIC_ENTITY: &str =
    "let section_corner = vec3<f32>(block_delta) - static_entity.camera_fraction.xyz;
    let camera_relative = world_warp(section_corner + position, section_corner).pos;";

/// The plainest warp, for the tests of the shaders it makes.
#[cfg(test)]
pub(super) const IDENTITY: &str = "fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped { return Warped(pos, vec3(1.0)); }\n";

/// `shader` with what stands between the markers `// [name]` and `// [end name]` replaced.
fn fill(shader: &str, name: &str, with: &str) -> Result<String, String> {
    let (from, to) = (format!("// [{name}]"), format!("// [end {name}]"));
    let a = shader
        .find(&from)
        .ok_or_else(|| format!("no `{from}` in the shader"))?;
    let b = shader[a..]
        .find(&to)
        .ok_or_else(|| format!("no `{to}` in the shader"))?
        + a;
    Ok(format!("{}{with}{}", &shader[..a], &shader[b + to.len()..]))
}

/// A warp's source split into what every warped shader takes and the terrain's pixel stage.
fn split(source: &str) -> (&str, Option<&str>) {
    match source.find(FRAGMENTS) {
        Some(at) => (&source[..at], Some(&source[at + FRAGMENTS.len()..])),
        None => (source, None),
    }
}

/// The warp's copy of terrain.wgsl.
pub(super) fn terrain_shader(source: &str) -> Result<String, String> {
    let (vertex, fragment) = split(source);
    let fragment = fragment
        .filter(|f| f.contains("fn world_fragment"))
        .unwrap_or(DEFAULT_FRAGMENT);
    let shader = fill(&super::terrain_shader(), "world warp varyings", VARYINGS)?;
    let shader = fill(&shader, "world warp wire", WIRE)?;
    let shader = fill(&shader, "world warp to wire", TO_WIRE)?;
    let shader = fill(&shader, "world warp from wire", FROM_WIRE)?;
    let shader = fill(&shader, "world warp vertex", VERTEX)?;
    let shader = fill(&shader, "world warp out", VERTEX_OUT)?;
    Ok(format!(
        "{shader}\n{}\n{vertex}\n{}\n{fragment}",
        include_str!("world_warp.wgsl"),
        include_str!("world_warp_fragment.wgsl"),
    ))
}

/// The warp's copy of entity.wgsl with static_entity.wgsl, for every entity pipeline it draws.
pub(super) fn entity_shader(source: &str) -> Result<String, String> {
    let (vertex, _) = split(source);
    let shader = fill(&super::static_entity_shader(), "world warp entity", ENTITY)?;
    let shader = fill(&shader, "world warp static entity", STATIC_ENTITY)?;
    Ok(format!(
        "{shader}\n{}\n{vertex}",
        include_str!("world_warp.wgsl")
    ))
}

fn validate(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(source))?;
    Ok(())
}

/// The warp's uniform (`world_warp.wgsl`'s `WorldWarpUniform`): the mod's sixteen floats, then
/// the camera split into f32 and what f32 left out of it, with the frame's partial tick.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    params: [[f32; 4]; 4],
    camera: [f32; 4],
    camera_rest: [f32; 4],
}

fn uniform(warp: &WorldWarp, camera: DVec3, partial_tick: f32) -> Uniform {
    let (params, _) = warp.params.as_chunks::<4>();
    let params: [[f32; 4]; 4] = params.try_into().expect("sixteen floats");
    let at = camera.as_vec3();
    let rest = camera - at.as_dvec3();
    Uniform {
        params,
        camera: [at.x, at.y, at.z, partial_tick],
        camera_rest: [rest.x as f32, rest.y as f32, rest.z as f32, 0.0],
    }
}

/// What the warp's pipelines are built with: the renderer's own layouts and formats.
pub(super) struct Kit {
    /// terrain.wgsl's group 0, and with flat draws the per-section draws' page group 1
    /// (`vs_section` reads its vertices from it).
    pub terrain: wgpu::BindGroupLayout,
    pub pages: Option<wgpu::BindGroupLayout>,
    /// entity.wgsl's group 0 and static_entity.wgsl's group 1.
    pub entity: wgpu::BindGroupLayout,
    pub static_entity: wgpu::BindGroupLayout,
    pub format: wgpu::TextureFormat,
    /// The frame holds linear light (`linear_blend`).
    pub linear: bool,
    pub sprite_constants: [(&'static str, f64); 2],
    /// Whether the display offers HDR output: the solid terrain is built lit by it too.
    pub hdr_output: bool,
}

/// The warp's pipelines, in place of the renderer's while it is on.
pub(super) struct Pipelines {
    pub solid: SolidPipeline,
    pub translucent: wgpu::RenderPipeline,
    pub entities: wgpu::RenderPipeline,
    pub static_entities: wgpu::RenderPipeline,
    pub translucent_items: wgpu::RenderPipeline,
    pub text: wgpu::RenderPipeline,
    pub entity_shadows: wgpu::RenderPipeline,
    pub hdr_mark: wgpu::RenderPipeline,
    pub hdr_entity_mark: wgpu::RenderPipeline,
}

/// The warp's uniform, and what binds it.
struct Bound {
    layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// The renderer's world warp: nothing made until a mod gives one.
pub(super) struct WorldWarps {
    kit: Kit,
    bound: Option<Bound>,
    built: Option<(Arc<str>, Pipelines)>,
    /// The last source that didn't compile, so it is tried once.
    refused: Option<Arc<str>>,
    /// Whether the log said a warp went undrawn under a shader pack drawing the terrain.
    told_undrawable: bool,
}

impl WorldWarps {
    pub fn new(kit: Kit) -> Self {
        Self {
            kit,
            bound: None,
            built: None,
            refused: None,
            told_undrawable: false,
        }
    }

    /// Readies this frame's warp: builds its pipelines when its source is new and writes its
    /// uniform. Whether the frame draws with it: not when it is off, didn't compile, or the
    /// frame can't take it (`drawable`: a shader pack draws the terrain).
    pub fn frame(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut super::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        warp: &WorldWarp,
        camera: DVec3,
        partial_tick: f32,
        drawable: bool,
    ) -> bool {
        let Some(source) = &warp.source else {
            return false;
        };
        let same = |s: &Arc<str>| Arc::ptr_eq(s, source) || **s == **source;
        if !self.built.as_ref().is_some_and(|(s, _)| same(s)) {
            if self.refused.as_ref().is_some_and(same) {
                return false;
            }
            match self.build(device, source) {
                Ok(pipelines) => self.built = Some((source.clone(), pipelines)),
                Err(e) => {
                    log::warn!("Horizon: a mod's world warp did not compile: {e}");
                    self.refused = Some(source.clone());
                    self.built = None;
                    return false;
                }
            }
        }
        if !warp.on {
            return false;
        }
        if !drawable {
            if !self.told_undrawable {
                log::info!(
                    "Horizon: a mod's world warp isn't drawn while a shader pack draws the terrain"
                );
                self.told_undrawable = true;
            }
            return false;
        }
        let bound = self.bound(device);
        uploads.buffer(
            encoder,
            &bound.buffer,
            0,
            bytemuck::bytes_of(&uniform(warp, camera, partial_tick)),
        );
        true
    }

    /// The pipelines of the warp `frame` readied.
    pub fn pipelines(&self) -> Option<&Pipelines> {
        self.built.as_ref().map(|(_, p)| p)
    }

    /// The warp's group 2, once a frame has drawn with it.
    pub fn group(&self) -> Option<&wgpu::BindGroup> {
        self.bound.as_ref().map(|b| &b.group)
    }

    fn bound(&mut self, device: &wgpu::Device) -> &Bound {
        self.bound.get_or_insert_with(|| bind(device))
    }

    fn build(&mut self, device: &wgpu::Device, source: &str) -> Result<Pipelines, String> {
        let started = std::time::Instant::now();
        let terrain = terrain_shader(source)?;
        let entity = entity_shader(source)?;
        validate(&terrain)?;
        validate(&entity)?;
        // Validation failures above are the mod's; past here wgpu's are the game's.
        let warp = &self.bound.get_or_insert_with(|| bind(device)).layout;
        let kit = &self.kit;
        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                ..Default::default()
            })
        };
        let terrain_layout = layout(
            "Warped terrain layout",
            &[Some(&kit.terrain), None, Some(warp)],
        );
        let pages_layout = kit.pages.as_ref().map(|pages| {
            layout(
                "Warped terrain layout (pulled)",
                &[Some(&kit.terrain), Some(pages), Some(warp)],
            )
        });
        let entity_layout = layout(
            "Warped entity layout",
            &[Some(&kit.entity), None, Some(warp)],
        );
        let static_layout = layout(
            "Warped static block-entity layout",
            &[Some(&kit.entity), Some(&kit.static_entity), Some(warp)],
        );
        let module = |label, source: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let terrain = module("Warped terrain", terrain);
        let entity = module("Warped entities", entity);

        let instance_attributes = wgpu::vertex_attr_array![5=>Float32x4];
        let instances = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attributes,
        };
        let vertices = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &TERRAIN_ATTRIBUTES,
        };
        let buffered = [Some(vertices), Some(instances.clone())];
        let pulled = [None, Some(instances)];
        // As gfx.rs's: a float frame takes light where others take marks.
        let [write_light, linear] = super::linear_blend::frame_constants(kit.format, kit.linear);
        let lit = [
            kit.sprite_constants[0],
            kit.sprite_constants[1],
            write_light,
            linear,
        ];
        let plain = [
            kit.sprite_constants[0],
            kit.sprite_constants[1],
            ("HDR_LIGHTING", 0.0),
            write_light,
            linear,
        ];
        let depth = |write| wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(write),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        };
        // As gfx.rs's terrain pipelines, without back-face culling.
        let terrain_pipeline = |label,
                                layout: &wgpu::PipelineLayout,
                                vertex,
                                buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                                fragment,
                                constants: &[(&str, f64)],
                                blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &terrain,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(depth(true)),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &terrain,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants,
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: kit.format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // The per-section solid draws read the arena page with flat draws (`vs_section`), and
        // vertex buffers without.
        let (solid_layout, solid_vertex, solid_buffers): (_, _, &[_]) = match &pages_layout {
            Some(pages) => (pages, "vs_section", &pulled),
            None => (&terrain_layout, "vs_terrain", &buffered),
        };
        let solid = |constants: &[(&str, f64)]| {
            terrain_pipeline(
                "Warped terrain solid",
                solid_layout,
                solid_vertex,
                solid_buffers,
                "fs_warp_solid",
                constants,
                None,
            )
        };
        let solid = SolidPipeline {
            plain: solid(&plain),
            hdr: kit.hdr_output.then(|| solid(&lit)),
        };
        let translucent = terrain_pipeline(
            "Warped terrain translucent",
            &terrain_layout,
            "vs_terrain",
            &buffered,
            "fs_warp_translucent",
            &lit,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );

        let entity_attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Uint8x4,5=>Snorm8x4,6=>Float32x2];
        let entity_vertices = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &entity_attributes,
        };
        let origin_attributes = wgpu::vertex_attr_array![7=>Sint32x3];
        let static_buffers = [
            Some(entity_vertices.clone()),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<[i32; 3]>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &origin_attributes,
            }),
        ];
        let entity_buffers = [Some(entity_vertices)];
        // As gfx.rs's `entity_pipeline_with`, for the pipelines a warped frame swaps.
        let entity_pipeline = |look: &EntityLook| {
            let (layout, vertex, buffers): (_, _, &[_]) = if look.static_mesh {
                (&static_layout, "vs_static_entity", &static_buffers)
            } else {
                (&entity_layout, "vs_entity", &entity_buffers)
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(look.label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &entity,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: look.cull,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    bias: look.bias,
                    ..depth(look.depth_write)
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &entity,
                    entry_point: Some(look.fragment),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[write_light, linear],
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: kit.format,
                        blend: look.blend,
                        write_mask: look.write_mask,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = Pipelines {
            solid,
            translucent,
            entities: entity_pipeline(&ENTITIES),
            static_entities: entity_pipeline(&STATIC_ENTITIES),
            translucent_items: entity_pipeline(&TRANSLUCENT_ITEMS),
            text: entity_pipeline(&TEXT),
            entity_shadows: entity_pipeline(&ENTITY_SHADOWS),
            hdr_mark: entity_pipeline(&HDR_MARK),
            hdr_entity_mark: entity_pipeline(&HDR_ENTITY_MARK),
        };
        log::info!(
            "Horizon: a mod's world warp built in {} ms",
            started.elapsed().as_millis()
        );
        Ok(pipelines)
    }
}

fn bind(device: &wgpu::Device) -> Bound {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("World warp"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("World warp"),
        size: std::mem::size_of::<Uniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("World warp"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    Bound {
        layout,
        buffer,
        group,
    }
}

/// How one of the entity pipelines a warped frame swaps draws: as gfx.rs builds the stock one
/// of the same name.
struct EntityLook {
    label: &'static str,
    static_mesh: bool,
    cull: Option<wgpu::Face>,
    depth_write: bool,
    bias: wgpu::DepthBiasState,
    blend: Option<wgpu::BlendState>,
    fragment: &'static str,
    write_mask: wgpu::ColorWrites,
}
const NO_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: 0,
    slope_scale: 0.0,
    clamp: 0.0,
};
const ENTITIES: EntityLook = EntityLook {
    label: "Warped entities",
    static_mesh: false,
    cull: None,
    depth_write: true,
    bias: wgpu::DepthBiasState {
        constant: 128,
        slope_scale: 0.0,
        clamp: 0.0,
    },
    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
    fragment: "fs_entity",
    write_mask: wgpu::ColorWrites::ALL,
};
const STATIC_ENTITIES: EntityLook = EntityLook {
    label: "Warped static block entities",
    static_mesh: true,
    bias: NO_BIAS,
    blend: None,
    fragment: "fs_entity_unmarked",
    ..ENTITIES
};
const TRANSLUCENT_ITEMS: EntityLook = EntityLook {
    label: "Warped translucent items",
    bias: NO_BIAS,
    ..ENTITIES
};
const TEXT: EntityLook = EntityLook {
    label: "Warped polygon offset text",
    cull: Some(wgpu::Face::Back),
    bias: wgpu::DepthBiasState {
        constant: 10,
        slope_scale: 1.0,
        clamp: 0.0,
    },
    fragment: "fs_entity_unmarked",
    ..ENTITIES
};
const ENTITY_SHADOWS: EntityLook = EntityLook {
    label: "Warped entity shadows",
    cull: Some(wgpu::Face::Back),
    depth_write: false,
    bias: wgpu::DepthBiasState {
        constant: 128,
        slope_scale: 0.015,
        clamp: 0.0,
    },
    fragment: "fs_entity_shadow",
    ..ENTITIES
};
const HDR_MARK: EntityLook = EntityLook {
    label: "Warped HDR particle mark",
    depth_write: false,
    bias: NO_BIAS,
    blend: Some(wgpu::BlendState::REPLACE),
    fragment: "fs_hdr_mark",
    write_mask: wgpu::ColorWrites::ALPHA,
    ..ENTITIES
};
/// `HDR_MARK` at the entities' depth bias, for what they drew.
const HDR_ENTITY_MARK: EntityLook = EntityLook {
    label: "Warped HDR entity mark",
    bias: ENTITIES.bias,
    ..HDR_MARK
};

#[cfg(test)]
mod tests {
    use super::*;

    const BEND: &str = "fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped {
        let lift = sin(pos.x * 0.1 + warp.params[0].x) * warp.params[0].y + section.y * 0.0;
        return Warped(pos + vec3(0.0, lift, 0.0), vec3(1.0));
    }
";

    #[test]
    fn a_warp_without_fragments_draws_the_terrain_as_the_game_does() {
        let terrain = terrain_shader(BEND).unwrap();
        validate(&terrain).unwrap();
        assert!(terrain.contains(DEFAULT_FRAGMENT));
        let entity = entity_shader(BEND).unwrap();
        validate(&entity).unwrap();
    }

    #[test]
    fn a_warps_own_fragments_replace_the_default_and_stay_out_of_the_entities() {
        let source = format!(
            "{BEND}{FRAGMENTS}
fn world_fragment(f: Fragment) -> Painted {{
    if !f.front || f.light.x < 1.0 {{ return Painted(vec4(0.0, 0.0, 0.0, -1.0), 1.0); }}
    let far = length(f.home - f.at) + f.cut;
    return Painted(sprite(f.uv * 2.0, f.layer) * f.color, 1.0 - min(far, 1.0));
}}
"
        );
        let terrain = terrain_shader(&source).unwrap();
        validate(&terrain).unwrap();
        assert!(!terrain.contains(DEFAULT_FRAGMENT));
        let entity = entity_shader(&source).unwrap();
        validate(&entity).unwrap();
        assert!(!entity.contains("world_fragment"));
    }

    #[test]
    fn the_stock_shaders_hold_nothing_of_a_warp() {
        for (name, shader) in [
            ("terrain", super::super::terrain_shader()),
            ("entity", super::super::static_entity_shader()),
        ] {
            validate(&shader).unwrap();
            for word in ["world_warp(", "warp_home", "Warped", "group(2)"] {
                assert!(!shader.contains(word), "{name}.wgsl holds {word}");
            }
        }
    }

    #[test]
    fn every_marker_is_found() {
        let terrain = terrain_shader(BEND).unwrap();
        let entity = entity_shader(BEND).unwrap();
        for shader in [&terrain, &entity] {
            assert!(!shader.contains("// [world warp"), "a marker left unfilled");
        }
        assert!(terrain.contains("let warped = world_warp(warp_home, origin);"));
        assert!(entity.contains("let at = world_warp(position, position).pos;"));
        assert!(entity.contains("world_warp(section_corner + position, section_corner)"));
        assert_eq!(
            fill("a // [x] b // [end x] c", "x", "B"),
            Ok("a B c".to_owned())
        );
        assert!(fill("a // [x] b", "x", "B").is_err());
    }

    #[test]
    fn a_warp_that_does_not_compile_is_refused_with_naga_s_reason() {
        let terrain =
            terrain_shader("fn world_warp(pos: vec3<f32>) -> Warped { return 1; }").unwrap();
        let e = validate(&terrain).unwrap_err();
        assert!(e.contains("world_warp"), "{e}");
    }

    #[test]
    fn the_uniform_keeps_the_camera_exact_far_out() {
        let mut warp = WorldWarp::default();
        warp.params[5] = 2.0;
        let camera = DVec3::new(29_999_984.3, 70.25, -12_345_678.9);
        let u = uniform(&warp, camera, 0.25);
        assert_eq!(u.params[1][1], 2.0);
        assert_eq!(u.camera[3], 0.25);
        for i in 0..3 {
            let whole = f64::from(u.camera[i]) + f64::from(u.camera_rest[i]);
            assert!((whole - camera[i]).abs() < 1e-6, "{whole} {}", camera[i]);
        }
        assert_eq!(std::mem::size_of::<Uniform>(), 96);
    }
}
