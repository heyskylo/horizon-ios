//! Runs a diagnostic gbuffers terrain program over arena-shaped page buffers through the real
//! `PackPost::run`, flat and non-flat, and reads back what `mc_Entity`, `at_midBlock`,
//! `mc_midTexCoord`, `at_tangent` and `gl_Normal` received. Page bind groups are cached per page
//! (set 3), so buffers that are replaced must show up in the next frame.
use super::super::terrain_metadata::encode;
use super::super::{PACK_INSTANCE_ATTRIBUTES, TERRAIN_ATTRIBUTES};
use super::pack_frame_tests::empty_frame;
use super::*;
use crate::mesher::{QuadState, TerrainVertex, UV_STEPS};
use horizon_shaders::{GameInputs, Macros, Pack, Properties, load_post};
use std::sync::atomic::{AtomicU64, Ordering};
use wgpu::util::DeviceExt;

const SIDE: u32 = 4;
/// Vertices before the first drawn quad of every page: a quad nobody draws, so a draw that
/// ignored its base vertex would read the wrong vertices and the wrong metadata.
const BASE_VERTEX: i32 = 4;
/// `mc_Entity` of block states 0 and 1: (model id, -1, fluid id, 1) per state.
const BLOCKS: [i32; 10] = [100, -1, -1, 1, 12, 200, -1, -1, 1, 3];

type Pixels = [[[u8; 4]; 4]; 4];

/// A throwaway pack folder, created with `create_dir` so that no two runs can share one.
struct ScratchPack(std::path::PathBuf);
impl ScratchPack {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "horizon-terrain-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("a fresh scratch folder");
        std::fs::create_dir(root.join("shaders")).unwrap();
        let files = [
            (
                // Corners are fixed by the vertex id, so the quads' winding and the mirrored UVs
                // cannot move them; the second block's quad is shifted to the right half.
                "gbuffers_terrain.vsh",
                "#version 330 compatibility\n\
                 in vec4 mc_Entity;\n\
                 in vec4 at_tangent;\n\
                 in vec4 at_midBlock;\n\
                 in vec2 mc_midTexCoord;\n\
                 out vec4 mrt0;\n\
                 out vec4 mrt1;\n\
                 out vec4 mrt2;\n\
                 out vec4 mrt3;\n\
                 void main() {\n\
                   int corner = gl_VertexID & 3;\n\
                   vec2 at = corner == 0 ? vec2(-1.0, -1.0) : corner == 1 ? vec2(0.0, -1.0) : corner == 2 ? vec2(0.0, 1.0) : vec2(-1.0, 1.0);\n\
                   bool second = mc_Entity.x > 150.0;\n\
                   if (second) at.x += 1.0;\n\
                   gl_Position = vec4(at, 0.0, 1.0);\n\
                   float mid = second ? at_midBlock.y : at_midBlock.z;\n\
                   mrt0 = vec4(mc_Entity.x / 255.0, gl_Normal.z, (mid / 64.0 + 2.0) / 4.0, 1.0);\n\
                   mrt1 = vec4(at_tangent.xyz * 0.25 + 0.4, at_tangent.w * 0.25 + 0.4);\n\
                   mrt2 = vec4(mc_midTexCoord, gl_Normal.y, mc_Entity.y < 0.0 ? 1.0 : 0.0);\n\
                   mrt3 = vec4(at_midBlock.w / 15.0, 0.0, 0.0, 1.0);\n\
                 }\n",
            ),
            (
                "gbuffers_terrain.fsh",
                "#version 330 compatibility\n\
                 in vec4 mrt0;\n\
                 in vec4 mrt1;\n\
                 in vec4 mrt2;\n\
                 in vec4 mrt3;\n\
                 void main() {\n\
                   /* DRAWBUFFERS:0123 */\n\
                   gl_FragData[0] = mrt0;\n\
                   gl_FragData[1] = mrt1;\n\
                   gl_FragData[2] = mrt2;\n\
                   gl_FragData[3] = mrt3;\n\
                 }\n",
            ),
            (
                "final.vsh",
                "#version 330 compatibility\n\
                 out vec2 texcoord;\n\
                 void main() {\n\
                   gl_Position = vec4(gl_Vertex.xy * 2.0 - 1.0, 0.0, 1.0);\n\
                   texcoord = gl_MultiTexCoord0.xy;\n\
                 }\n",
            ),
            (
                // GL row 0 (the bottom screen row) shows colortex0, 1 colortex1, 2 colortex2 and
                // 3 the real shadow vertex image writes, so all stages are observed.
                "final.fsh",
                "#version 330 compatibility\n\
                 uniform sampler2D colortex0;\n\
                 uniform sampler2D colortex1;\n\
                 uniform sampler2D colortex2;\n\
                 uniform sampler2D colortex3;\n\
                 uniform usampler3D shadowData;\n\
                 in vec2 texcoord;\n\
                 void main() {\n\
                   int row = int(gl_FragCoord.y);\n\
                   vec4 c = texture2D(colortex0, texcoord);\n\
                   if (row == 1) c = texture2D(colortex1, texcoord);\n\
                   else if (row == 2) c = texture2D(colortex2, texcoord);\n\
                   else if (row == 3) { uint v = texelFetch(shadowData, ivec3(texcoord.x < 0.5 ? 0 : 1, 0, 0), 0).r; c = vec4(float(v & 255u) / 255.0, float((v >> 8u) & 255u) / 15.0, float((v >> 16u) & 255u) / 16.0, 1.0); }\n\
                   gl_FragColor = c;\n\
                 }\n",
            ),
        ];
        for (name, source) in files {
            std::fs::write(root.join("shaders").join(name), source).unwrap();
        }
        // The shadow vertex stage must receive the same source data before voxel writes.
        std::fs::write(root.join("shaders/shaders.properties"),"shadow.enabled=true\nimage.shadow_data=shadowData red_integer r32ui unsigned_int true false 2 1 1\n").unwrap();
        let shadow=std::fs::read_to_string(root.join("shaders/gbuffers_terrain.vsh")).unwrap()
            .replace("void main() {","layout(r32ui) uniform writeonly uimage3D shadow_data;\nvoid main() {")
            .replace("mrt3 = vec4(at_midBlock.w / 15.0, 0.0, 0.0, 1.0);","mrt3 = vec4(at_midBlock.w / 15.0, 0.0, 0.0, 1.0);\nuint encoded = uint(mc_Entity.x) | (uint(at_midBlock.w) << 8u) | (uint(floor(gl_Vertex.z + at_midBlock.z / 64.0)) << 16u);\nimageStore(shadow_data, ivec3(second ? 1 : 0, 0, 0), uvec4(encoded));");
        std::fs::write(root.join("shaders/shadow.vsh"), shadow).unwrap();
        std::fs::write(
            root.join("shaders/shadow.fsh"),
            "#version 330 compatibility\n/* const int shadowMapResolution = 4; */\nvoid main(){}\n",
        )
        .unwrap();
        Self(root)
    }
}
impl Drop for ScratchPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn quad(positions: [[f32; 3]; 4], uvs: [[f32; 2]; 4]) -> [TerrainVertex; 4] {
    std::array::from_fn(|i| TerrainVertex::new(positions[i], uvs[i], 0, 0xffff_ffff, 0))
}
/// Block (3,7,12)'s vertical quad, facing +Z one block in front of its cell.
fn quad_a() -> [TerrainVertex; 4] {
    quad(
        [[3., 7., 14.], [4., 7., 14.], [4., 8., 14.], [3., 8., 14.]],
        [[0., 0.], [0.5, 0.], [0.5, 0.5], [0., 0.5]],
    )
}
/// Block (5,2,10)'s horizontal quad facing +Y; `mirrored` flips its U axis.
fn quad_b(mirrored: bool) -> [TerrainVertex; 4] {
    let uvs = if mirrored {
        [[0.5, 0.], [0., 0.], [0., 0.5], [0.5, 0.5]]
    } else {
        [[0., 0.], [0.5, 0.], [0.5, 0.5], [0., 0.5]]
    };
    quad(
        [
            [5., 2.25, 10.],
            [5., 2.25, 11.],
            [6., 2.25, 11.],
            [6., 2.25, 10.],
        ],
        uvs,
    )
}
fn dummy() -> [TerrainVertex; 4] {
    quad(
        [[9., 1., 9.], [9., 1., 10.], [10., 1., 10.], [10., 1., 9.]],
        [[0.25, 0.25], [1., 0.25], [1., 1.], [0.25, 1.]],
    )
}
fn meta_a(cell_z: i32) -> [u32; super::super::terrain_metadata::WORDS] {
    encode(QuadState::new(0, [3, 7, cell_z], false))
}
fn meta_b() -> [u32; super::super::terrain_metadata::WORDS] {
    encode(QuadState::new(1, [5, 2, 10], false))
}

/// Vertices of a page: the undrawn quad, then `quads`, then `spare` empty quads of capacity.
fn vertex_buffer(
    device: &wgpu::Device,
    quads: &[[TerrainVertex; 4]],
    spare: usize,
) -> wgpu::Buffer {
    let mut vertices = dummy().to_vec();
    vertices.extend(quads.iter().flatten());
    vertices.extend(std::iter::repeat_n(
        <TerrainVertex as bytemuck::Zeroable>::zeroed(),
        spare * 4,
    ));
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("diagnostic page vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
    })
}
/// The per-quad words of the same page (`terrain_metadata.rs`: 2 u32 per quad).
fn metadata_buffer(
    device: &wgpu::Device,
    quads: &[[u32; super::super::terrain_metadata::WORDS]],
    spare: usize,
) -> wgpu::Buffer {
    let unknown = encode(QuadState::UNKNOWN);
    let mut words = vec![unknown];
    words.extend(quads);
    words.extend(std::iter::repeat_n(unknown, spare));
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("diagnostic page metadata"),
        contents: bytemuck::cast_slice(&words),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

struct Page {
    id: u32,
    vertices: wgpu::Buffer,
    metadata: wgpu::Buffer,
    /// Quads drawn after the undrawn one.
    quads: u32,
}

/// What the game's terrain callback binds and draws for one mode.
struct Terrain {
    /// Flat: the fake set 1 (slots, origins); non-flat: nothing, the origin is an instance.
    set1: Option<wgpu::BindGroup>,
    instance: wgpu::Buffer,
    indices: wgpu::Buffer,
}
impl Terrain {
    fn draw(
        &self,
        pages: &[Page],
        rp: &mut wgpu::RenderPass<'_>,
        list: TerrainList,
        page: &dyn Fn(&mut wgpu::RenderPass<'_>, u32),
    ) {
        // Translucent terrain falls back to the same program: draw the geometry once.
        if !matches!(list, TerrainList::Solid | TerrainList::ShadowSolid) {
            return;
        }
        rp.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for p in pages {
            page(rp, p.id);
            rp.set_vertex_buffer(0, p.vertices.slice(..));
            match &self.set1 {
                Some(group) => rp.set_bind_group(1, group, &[]),
                None => rp.set_vertex_buffer(1, self.instance.slice(..)),
            }
            rp.draw_indexed(0..p.quads * 6, BASE_VERTEX, 0..1);
        }
    }
}

struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    atlas: super::super::pack_atlas::PackAtlas,
    screen: wgpu::Texture,
    readback: wgpu::Buffer,
}
impl Gpu {
    fn open() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("this execution regression requires a Metal adapter");
        // The renderer's own intersection (gfx.rs), without its timing and culling extras.
        let features = adapter.features()
            & (wgpu::Features::INDIRECT_FIRST_INSTANCE
                | wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
                | wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                | wgpu::Features::RG11B10UFLOAT_RENDERABLE
                | wgpu::Features::VERTEX_WRITABLE_STORAGE
                | wgpu::Features::CLEAR_TEXTURE);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("terrain diagnostic device"),
            required_features: features,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .unwrap();
        // Flat terrain reads 2 storage buffers of set 1 and 3 of set 3 in the vertex stage.
        assert!(device.limits().max_storage_buffers_per_shader_stage >= 5);
        // One opaque 2x2 sprite in one layer: the atlas is a single 2x2 cell.
        let sprite = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("diagnostic sprite"),
                size: wgpu::Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[255; 16],
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        let atlas =
            super::super::pack_atlas::PackAtlas::new(&device, &mut encoder, &[sprite], 1, 1, 2)
                .expect("a 2x2 atlas fits every device");
        queue.submit([encoder.finish()]);
        let screen = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain diagnostic screen"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain diagnostic readback"),
            size: 256 * u64::from(SIDE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            adapter,
            device,
            queue,
            atlas,
            screen,
            readback,
        }
    }

    /// The pipelines of the diagnostic pack for flat or non-flat draws, as `set_shader_pack`
    /// builds them: `TerrainVertex` alone, or with the origin as a second, per-instance buffer.
    fn post(&self, pack: &ScratchPack, flat: Option<&wgpu::BindGroupLayout>) -> PackPost {
        let pack = Pack::open(&pack.0).unwrap();
        let macros = Macros::standard();
        let properties = Properties::from_pack(&pack, "", &macros);
        let inputs = GameInputs {
            terrain: Some(self.atlas.terrain_inputs(flat.is_some())),
            entities: Some(super::super::pack_entities::inputs()),
            ..Default::default()
        };
        let parsed = load_post(&pack, "", &properties, &macros, &inputs);
        assert!(parsed.complete, "parse notes: {:?}", parsed.notes);
        assert!(
            parsed.terrain.is_some(),
            "no terrain program: {:?}",
            parsed.notes
        );
        let vertex = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &TERRAIN_ATTRIBUTES,
        };
        let instance = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &PACK_INSTANCE_ATTRIBUTES,
        };
        let buffers = if flat.is_some() {
            vec![Some(vertex)]
        } else {
            vec![Some(vertex), Some(instance)]
        };
        let world = World {
            adapter: &self.adapter,
            atlas: Some((self.atlas.view.clone(), self.atlas.size())),
            lightmap: texture(
                &self.device,
                &self.queue,
                "diagnostic lightmap",
                1,
                &[255; 4],
            ),
            terrain: Some(Geometry {
                buffers: &buffers,
                table: flat,
                depth_write: true,
                atlas: None,
                ids: None,
                cull: true,
            }),
            block_ids: &BLOCKS,
            celestial: None,
            clouds: None,
            weather: false,
            hand: false,
        };
        let post = PackPost::new(
            &self.device,
            &self.queue,
            parsed,
            wgpu::TextureFormat::Rgba8Unorm,
            world,
        );
        assert!(post.complete, "pipeline notes: {:?}", post.notes);
        assert!(post.terrain.is_some() && post.block_ids.is_some());
        post
    }

    /// One `PackPost::run` over `pages`; the screen's rows, top first.
    fn frame(&self, post: &mut PackPost, terrain: &Terrain, pages: &[Page], label: &str) -> Pixels {
        let world = |label, format| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: SIDE,
                        height: SIDE,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = world("diagnostic world", wgpu::TextureFormat::Rgba8Unorm);
        let depth = world("diagnostic world depth", wgpu::TextureFormat::Depth32Float);
        let screen = self.screen.create_view(&Default::default());
        let hud = crate::hud::HudBatch::new([SIDE; 2]);
        let frame = empty_frame(&hud);
        let inputs = PackInputs::default();
        let handles: Vec<_> = pages
            .iter()
            .map(|p| (p.id, p.vertices.clone(), p.metadata.clone()))
            .collect();
        checked(&self.device, || {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            // The world is empty: far in reverse Z, which the pack sees as GL depth 1.
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("diagnostic world clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            post.run(
                &self.device,
                &self.queue,
                &mut encoder,
                &color,
                &depth,
                &screen,
                &frame,
                &inputs,
                [SIDE; 2],
                [SIDE; 2],
                &handles,
                &mut |rp, list, page| terrain.draw(pages, rp, list, page),
            );
            encoder.copy_texture_to_buffer(
                self.screen.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &self.readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                self.screen.size(),
            );
            self.queue.submit([encoder.finish()]);
        })
        .unwrap_or_else(|e| panic!("{label}: wgpu rejected the frame: {e}"));
        assert!(post.complete, "{label}: notes {:?}", post.notes);
        // Every page the frame passed has its group over exactly these buffers, and no other.
        let mut cached: Vec<u32> = post.page_groups.keys().copied().collect();
        cached.sort_unstable();
        let mut ids: Vec<u32> = pages.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        assert_eq!(cached, ids, "{label}: cached page groups");
        for p in pages {
            let (vertices, metadata, _) = &post.page_groups[&p.id];
            assert!(
                *vertices == p.vertices && *metadata == p.metadata,
                "{label}: page {} is bound to stale buffers",
                p.id
            );
        }
        let (send, done) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        done.recv().unwrap().unwrap();
        let mut pixels = [[[0; 4]; 4]; 4];
        {
            let data = self.readback.slice(..).get_mapped_range().unwrap();
            for (y, row) in pixels.iter_mut().enumerate() {
                for (x, pixel) in row.iter_mut().enumerate() {
                    pixel.copy_from_slice(&data[y * 256 + x * 4..][..4]);
                }
            }
        }
        self.readback.unmap();
        pixels
    }

    /// The fake set 1 layout of flat draws (`flat.rs`'s `draw_layout`: block slots, origins).
    fn flat_layout(&self) -> wgpu::BindGroupLayout {
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        self.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("diagnostic flat draws"),
                entries: &[storage(0), storage(1)],
            })
    }

    fn terrain(&self, layout: Option<&wgpu::BindGroupLayout>) -> Terrain {
        // Slot 0 is block 0 of the page; its origin is the section's: no offset, UV scale 1/UV_STEPS.
        let origin = [0.0f32, 0.0, 0.0, 1.0 / UV_STEPS];
        let buffer = |label, contents: &[u8], usage| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage,
                })
        };
        let slots = buffer(
            "diagnostic slots",
            bytemuck::bytes_of(&0u32),
            wgpu::BufferUsages::STORAGE,
        );
        let origins = buffer(
            "diagnostic origins",
            bytemuck::bytes_of(&origin),
            wgpu::BufferUsages::STORAGE,
        );
        let instance = buffer(
            "diagnostic instance",
            bytemuck::bytes_of(&origin),
            wgpu::BufferUsages::VERTEX,
        );
        // Quad 0 then quad 1 of the drawn quads, each as two triangles, relative to the base vertex.
        let indices = buffer(
            "diagnostic indices",
            bytemuck::cast_slice(&[0u32, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]),
            wgpu::BufferUsages::INDEX,
        );
        let set1 = layout.map(|layout| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("diagnostic flat draws"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: slots.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: origins.as_entire_binding(),
                    },
                ],
            })
        });
        Terrain {
            set1,
            instance,
            indices,
        }
    }
}

/// The screen for a left quad whose `at_midBlock.z` byte is `left_blue` and a right quad whose
/// tangent bytes are `right_tangent`. GL row 0 is the bottom row (`pack_frame_tests`), and rows 0
/// to 3 show colortex0, 1, 2, 0:
/// - colortex0: (mc_Entity.x / 255, normal.z, (at_midBlock / 64 + 2) / 4, 1) with the left quad's
///   `.z` (-1.5 blocks: 32) and the right quad's `.y` (0.25 blocks: 143).
/// - colortex1: tangent.xyz * 0.25 + 0.4, tangent.w * 0.25 + 0.4. Left: +X, hand -1; mirrored
///   right: -Z, hand +1. (A tie-free encoding: 8-bit unorm rounds 127.5 either way.)
/// - colortex2: mc_midTexCoord (0.25, 0.25 -> 64), normal.y (left 0, right 1), mc_Entity.y == -1.
fn expected(left_blue: u8, right_tangent: [u8; 4]) -> Pixels {
    let rows = [
        [[100, 255, left_blue, 255], [200, 0, 143, 255]],
        [[166, 102, 102, 38], right_tangent],
        [[64, 64, 0, 255], [64, 64, 255, 255]],
        [
            [100, 204, if left_blue == 0 { 175 } else { 191 }, 255],
            [200, 51, 159, 255],
        ],
    ];
    let mut pixels = [[[0; 4]; 4]; 4];
    for (gl_row, sides) in rows.iter().enumerate() {
        for x in 0..4 {
            pixels[3 - gl_row][x] = sides[x / 2];
        }
    }
    pixels
}
const MIRRORED_TANGENT: [u8; 4] = [102, 102, 38, 166];
/// B with its UVs not mirrored: tangent +Z, hand -1.
const PLAIN_TANGENT: [u8; 4] = [102, 102, 166, 38];

/// Five frames of one `PackPost`; each checked against the exact screen.
fn run_mode(gpu: &Gpu, flat: bool) -> Vec<Pixels> {
    let mode = if flat { "flat" } else { "non-flat" };
    let layout = gpu.flat_layout();
    let terrain = gpu.terrain(flat.then_some(&layout));
    let pack = ScratchPack::new();
    let mut post = gpu.post(&pack, flat.then_some(&layout));
    let device = &gpu.device;
    let mut screens = Vec::new();
    let mut check = |post: &mut PackPost, pages: &[Page], name: &str, expect: Pixels| {
        let label = format!("{mode}, {name}");
        let pixels = gpu.frame(post, &terrain, pages, &label);
        for (y, (got, want)) in pixels.iter().zip(&expect).enumerate() {
            for (x, (got, want)) in got.iter().zip(want).enumerate() {
                assert_eq!(got, want, "{label}, pixel ({x},{y})");
            }
        }
        screens.push(pixels);
    };

    // The block table and metadata words as the arena stores them.
    assert_eq!(&meta_a(12)[..2], &[0, 3 | (7 << 4) | (12 << 8)]);
    assert_eq!(&meta_b()[..2], &[1, 5 | (2 << 4) | (10 << 8)]);

    // 1. Both quads on one page, drawn from vertex 4 on.
    let one = Page {
        id: 0,
        vertices: vertex_buffer(device, &[quad_a(), quad_b(true)], 0),
        metadata: metadata_buffer(device, &[meta_a(12), meta_b()], 0),
        quads: 2,
    };
    check(
        &mut post,
        &[one],
        "one page",
        expected(32, MIRRORED_TANGENT),
    );

    // 2. The page grew: new, larger buffers with the same quads, block A's source cell moved
    //    from z 12 to 11 (so its at_midBlock.z is -2.5 blocks, which clamps to 0).
    let grown_vertices = vertex_buffer(device, &[quad_a(), quad_b(true)], 6);
    let grown = Page {
        id: 0,
        vertices: grown_vertices.clone(),
        metadata: metadata_buffer(device, &[meta_a(11), meta_b()], 6),
        quads: 2,
    };
    check(
        &mut post,
        &[grown],
        "grown page",
        expected(0, MIRRORED_TANGENT),
    );

    // 3. Only the metadata buffer is replaced; the cell is back at z 12.
    let page = Page {
        id: 0,
        vertices: grown_vertices,
        metadata: metadata_buffer(device, &[meta_a(12), meta_b()], 6),
        quads: 2,
    };
    let metadata = page.metadata.clone();
    check(
        &mut post,
        &[page],
        "replaced metadata only",
        expected(32, MIRRORED_TANGENT),
    );

    // 4. Only the vertex buffer is replaced: B's UVs are no longer mirrored.
    let page = Page {
        id: 0,
        vertices: vertex_buffer(device, &[quad_a(), quad_b(false)], 6),
        metadata,
        quads: 2,
    };
    check(
        &mut post,
        &[page],
        "replaced vertices only",
        expected(32, PLAIN_TANGENT),
    );

    // 5. Two pages with other ids, each with one drawn quad after its undrawn one; page 0 goes.
    let pages = [
        Page {
            id: 5,
            vertices: vertex_buffer(device, &[quad_a()], 0),
            metadata: metadata_buffer(device, &[meta_a(12)], 0),
            quads: 1,
        },
        Page {
            id: 9,
            vertices: vertex_buffer(device, &[quad_b(true)], 0),
            metadata: metadata_buffer(device, &[meta_b()], 0),
            quads: 1,
        },
    ];
    check(
        &mut post,
        &pages,
        "two pages",
        expected(32, MIRRORED_TANGENT),
    );
    screens
}

/// The actual vertices and per-quad metadata reach the pack's vertex stage, and cached page
/// bind groups follow replaced buffers, with flat and non-flat inputs alike.
#[test]
#[ignore = "requires a Metal adapter; run explicitly on macOS"]
fn terrain_attributes_come_from_page_vertices_and_metadata_on_metal() {
    let gpu = Gpu::open();
    let flat = run_mode(&gpu, true);
    let non_flat = run_mode(&gpu, false);
    assert_eq!(flat, non_flat, "flat and non-flat draws must agree");
}

/// Real loader policy, metadata decoding and gl_Color output, without lighting in the fixture
/// shader. No alpha blending: alpha is observed as the separate AO channel itself.
fn lighting_pack(old: bool, separate: bool, strength: f32) -> ScratchPack {
    let pack = ScratchPack::new();
    let shaders = pack.0.join("shaders");
    std::fs::write(
        shaders.join("shaders.properties"),
        format!("shadow.enabled=false\noldLighting={old}\nseparateAo={separate}\n"),
    )
    .unwrap();
    std::fs::write(
        shaders.join("gbuffers_terrain.vsh"),
        format!(
            "#version 330 compatibility\n\
             const float ambientOcclusionLevel = {strength:?};\n\
             in vec4 mc_Entity;\nout vec4 vertexColor;\n\
             void main() {{\n\
               int corner = gl_VertexID & 3;\n\
               vec2 at = corner == 0 ? vec2(-1.0,-1.0) : corner == 1 ? vec2(0.0,-1.0) : corner == 2 ? vec2(0.0,1.0) : vec2(-1.0,1.0);\n\
               if (mc_Entity.x > 150.0) at.x += 1.0;\n\
               gl_Position = vec4(at,0.0,1.0);\nvertexColor = gl_Color;\n}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        shaders.join("gbuffers_terrain.fsh"),
        "#version 330 compatibility\nin vec4 vertexColor;\nvoid main() { /* DRAWBUFFERS:0 */ gl_FragData[0] = vertexColor; }\n",
    )
    .unwrap();
    std::fs::write(
        shaders.join("final.fsh"),
        "#version 330 compatibility\nuniform sampler2D colortex0;\nin vec2 texcoord;\nvoid main() { gl_FragColor = texture2D(colortex0, texcoord); }\n",
    )
    .unwrap();
    pack
}

fn lighting_page(gpu: &Gpu, ao: [u8; 4]) -> Page {
    let mut quads = [quad_a(), quad_b(false)];
    for vertex in quads.iter_mut().flatten() {
        vertex.color = u32::from_le_bytes([59, 29, 14, 255]);
    }
    let metadata = [0, 1].map(|state| {
        let mut source = QuadState::new(state, [3, 7, 12], false);
        source.lighting = Some(crate::mesher::QuadLighting {
            tint: u32::from_le_bytes([200, 100, 50, 255]),
            shaded_tint: u32::from_le_bytes([120, 60, 30, 255]),
            no_directional: ao.map(|alpha| u32::from_le_bytes([100, 50, 25, alpha])),
        });
        encode(source)
    });
    Page {
        id: 0,
        vertices: vertex_buffer(&gpu.device, &quads, 0),
        metadata: metadata_buffer(&gpu.device, &metadata, 0),
        quads: 2,
    }
}

/// All four policies must reach the real render pass in both terrain paths. The deliberately
/// different source bytes detect attempts to divide baked colours back into an invented tint.
#[test]
#[ignore = "requires a Metal adapter; run explicitly on macOS"]
fn terrain_lighting_policy_and_corner_ao_reach_gl_color_on_metal() {
    let gpu = Gpu::open();
    let layout = gpu.flat_layout();
    let base = lighting_page(&gpu, [128; 4]);
    for flat in [true, false] {
        let terrain = gpu.terrain(flat.then_some(&layout));
        for old in [true, false] {
            for separate in [false, true] {
                for strength in [1.0f32, 0.0, 0.375] {
                    let pack = lighting_pack(old, separate, strength);
                    let mut post = gpu.post(&pack, flat.then_some(&layout));
                    let label =
                        format!("flat={flat} old={old} separate={separate} strength={strength}");
                    let pixels =
                        gpu.frame(&mut post, &terrain, std::slice::from_ref(&base), &label);
                    let tint = if old {
                        [120.0, 60.0, 30.0]
                    } else {
                        [200.0, 100.0, 50.0]
                    };
                    let baked = if old {
                        [59.0, 29.0, 14.0]
                    } else {
                        [100.0, 50.0, 25.0]
                    };
                    let mut expected = [0u8; 4];
                    for channel in 0..3 {
                        expected[channel] = if separate {
                            tint[channel] as u8
                        } else {
                            (tint[channel] + (baked[channel] - tint[channel]) * strength).round()
                                as u8
                        };
                    }
                    expected[3] = if separate {
                        (255.0 + (128.0 - 255.0) * strength).round() as u8
                    } else {
                        255
                    };
                    for row in pixels {
                        for pixel in row {
                            for channel in 0..4 {
                                // Only mixed RGB at partial strength contains a half-integer;
                                // either legal unorm rounding result is accepted there.
                                let tolerance =
                                    u8::from(!separate && strength == 0.375 && channel < 3);
                                assert!(
                                    pixel[channel].abs_diff(expected[channel]) <= tolerance,
                                    "{label}: {pixel:?}, expected {expected:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        // For each quad, corners are BL, BR, TR, TL and triangles are 012 and 023.
        // Sampling interior pixel centres gives all four source words nonzero weight.
        let alphas = [64, 128, 192, 255];
        let page = lighting_page(&gpu, alphas);
        let pack = lighting_pack(false, true, 1.0);
        let mut post = gpu.post(&pack, flat.then_some(&layout));
        let pixels = gpu.frame(&mut post, &terrain, &[page], "per-corner AO");
        for (y, row) in pixels.iter().enumerate() {
            for (x, pixel) in row.iter().enumerate() {
                let u = (x % 2) as f32 * 0.5 + 0.25;
                let v = (3 - y) as f32 * 0.25 + 0.125;
                let weights = if v <= u {
                    [1.0 - u, u - v, v, 0.0]
                } else {
                    [1.0 - v, 0.0, u, v - u]
                };
                let alpha = weights
                    .iter()
                    .zip(alphas)
                    .map(|(w, a)| w * f32::from(a))
                    .sum::<f32>()
                    .round() as u8;
                assert_eq!(&pixel[..3], &[200, 100, 50], "flat={flat} pixel {x},{y}");
                assert!(
                    pixel[3].abs_diff(alpha) <= 1,
                    "flat={flat} pixel {x},{y}: alpha {}, expected {alpha}",
                    pixel[3]
                );
            }
        }
    }
}
