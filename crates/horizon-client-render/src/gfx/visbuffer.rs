//! A visibility buffer for solid terrain, opt-in with `HORIZON_VISBUFFER=1` (read once, at
//! startup): pass 1 rasterises the solid flat draws (near sections and far tiles, phase 1 and,
//! with occlusion culling, phase 2) writing only depth and the ID of the quad each pixel shows
//! (`visbuffer.wgsl`'s `vis_id`: arena page and quad); cutout and fading
//! quads keep their alpha test and dither there, the rest never discard. Pass 2 is one
//! full-screen draw with up to `PAGE_SLOTS` arena pages bound at once (more pages, more draws)
//! that reads the quad back from its page and its section's origin (through a copy of every
//! drawn page's block table in one buffer), and runs terrain.wgsl's
//! vertex work and solid shading once per pixel with analytic screen gradients. Every solid
//! pixel is shaded once, however many quads were drawn over it and however small they are.
//! A draw per page instead made each GPU thread group run the shading once for every page
//! among its pixels, with most lanes idle: the pages' sections lie all over the screen.
//!
//! It runs only in frames drawn flat (`flat`: Metal past `flat::MIN_SECTIONS`, or
//! `HORIZON_FLAT_DRAWS=1`) and falls back to the normal solid passes in frames with a shader
//! pack drawing terrain, one of Horizon's own packs with a material stage or a shadow pass, a
//! pack's surface target, and on the title screen. The water mirror's own pass stays as it is.
use super::arena::Arena;
use super::flat::Flat;
use super::pack_timing::PassTiming;

/// Arena pages one resolve draw reads (`visbuffer.wgsl`'s `vis_page_0`...).
pub const PAGE_SLOTS: u32 = 16;
/// The resolve binds that many pages, the block tables and the origins by section id.
const STORAGE_BUFFERS: u32 = PAGE_SLOTS + 2;
const IDS: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

/// Whether the visibility buffer is asked for, and the device binds storage buffers enough.
pub fn wanted(device: &wgpu::Device) -> bool {
    std::env::var("HORIZON_VISBUFFER").as_deref() == Ok("1")
        && device.limits().max_storage_buffers_per_shader_stage >= STORAGE_BUFFERS
}

pub struct Visbuffer {
    /// Pass 1 over the quads `fs_opaque` draws (no discard) and the rest.
    opaque: wgpu::RenderPipeline,
    cutout: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    /// Pass 2's group 1: the block tables (binding 0), the origins by section id (1),
    /// `PAGE_SLOTS` pages' vertices (from 2), the IDs (after them).
    page_layout: wgpu::BindGroupLayout,
    /// Every page's block table, page after page (`Flat::blocks_per_page` entries each),
    /// copied each frame from the drawn pages' own, and its size in pages.
    blocks: Option<(wgpu::Buffer, u32)>,
    blocks_per_page: u64,
    /// The IDs, the size of the world's depth.
    ids: Option<(wgpu::TextureView, [u32; 2])>,
}

impl Visbuffer {
    /// `terrain_layout`: terrain.wgsl's group 0; `flat_layout`: the flat draws' group 1;
    /// `color`: the world pass's colour format; `constants`: the sprite arrays' pipeline
    /// constants.
    pub fn new(
        device: &wgpu::Device,
        terrain_layout: &wgpu::BindGroupLayout,
        flat: &Flat,
        color: wgpu::TextureFormat,
        constants: &[(&str, f64)],
    ) -> Self {
        let flat_layout = &flat.draw_layout;
        let blocks_per_page = flat.blocks_per_page();
        let resolve_constants: Vec<(&str, f64)> = constants
            .iter()
            .copied()
            .chain([("VIS_BLOCKS_PER_PAGE", blocks_per_page as f64)])
            .collect();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Horizon visibility buffer"),
            source: wgpu::ShaderSource::Wgsl(
                (include_str!("../terrain.wgsl").to_owned() + include_str!("../visbuffer.wgsl"))
                    .into(),
            ),
        });
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: fragment,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let entries: Vec<_> = (0..2 + PAGE_SLOTS)
            .map(storage)
            .chain([wgpu::BindGroupLayoutEntry {
                binding: 2 + PAGE_SLOTS,
                visibility: fragment,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }])
            .collect();
        let page_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Visibility buffer pages"),
            entries: &entries,
        });
        let id_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Visibility buffer IDs"),
            bind_group_layouts: &[Some(terrain_layout), Some(flat_layout)],
            ..Default::default()
        });
        let resolve_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Visibility buffer resolve"),
            bind_group_layouts: &[Some(terrain_layout), Some(&page_layout)],
            ..Default::default()
        });
        let vertices = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<super::TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &super::TERRAIN_ATTRIBUTES,
        })];
        let ids = |label: &'static str, vertex: &'static str, fragment: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&id_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &vertices,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants,
                        ..Default::default()
                    },
                    targets: &[Some(wgpu::ColorTargetState {
                        format: IDS,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = ids("Visibility buffer opaque", "vs_vis_opaque", "fs_vis_id");
        let cutout = ids("Visibility buffer cutout", "vs_vis_cutout", "fs_vis_cutout");
        let resolve = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Visibility buffer resolve"),
            layout: Some(&resolve_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_vis_resolve"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_vis_resolve"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &resolve_constants,
                    ..Default::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: color,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            opaque,
            cutout,
            resolve,
            page_layout,
            blocks: None,
            blocks_per_page,
            ids: None,
        }
    }

    /// Makes the IDs the world depth's `size`, and the block tables hold the pages `flat`
    /// draws this frame.
    pub fn fit(&mut self, device: &wgpu::Device, size: [u32; 2], flat: &Flat) {
        let pages = flat.calls[..flat.solid_calls]
            .iter()
            .map(|c| c.page + 1)
            .max()
            .unwrap_or(1);
        if self.blocks.as_ref().is_none_or(|(_, n)| *n < pages) {
            let pages = pages.next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Visibility buffer block tables"),
                size: u64::from(pages) * self.blocks_per_page * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.blocks = Some((buffer, pages));
        }
        if self.ids.as_ref().is_some_and(|(_, s)| *s == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Visibility buffer IDs"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IDS,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.ids = Some((texture.create_view(&Default::default()), size));
    }

    /// Pass 1's render pass over the world's `depth`: the IDs cleared (`clear`, phase 1) or
    /// kept (phase 2 of occlusion culling).
    pub fn id_pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        clear: bool,
        timing: Option<&mut PassTiming>,
    ) -> wgpu::RenderPass<'e> {
        let (ids, _) = self.ids.as_ref().expect("fitted before the world pass");
        let name = if clear {
            "Visibility IDs"
        } else {
            "Visibility IDs after culling"
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Horizon visibility buffer"),
            timestamp_writes: timing.and_then(|t| t.writes(name)),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ids,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        })
    }

    /// Phase 1's solid calls into the IDs, as the normal passes draw them: the opaque
    /// pipeline over `calls[..pass_calls[0]]`, the cutout one over
    /// `calls[pass_calls[1]..phase1_calls]` (each leaves out the other's quads).
    pub fn draw_ids<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        flat: &Flat,
        vertices: impl Fn(u32) -> wgpu::BufferSlice<'a> + Copy,
    ) {
        let [opaque, cutout, _] = flat.pass_calls;
        pass.set_pipeline(&self.opaque);
        flat.draw_paged(pass, &flat.calls[..opaque], vertices);
        pass.set_pipeline(&self.cutout);
        flat.draw_paged(pass, &flat.calls[cutout..flat.phase1_calls], vertices);
    }

    /// Phase 2's calls as `Flat::flatten_phase2` left them (their first instance is their
    /// page: `Flat::page_instances`), through both pipelines.
    pub fn draw_ids_phase2<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        flat: &Flat,
        vertices: impl Fn(u32) -> wgpu::BufferSlice<'a> + Copy,
    ) {
        // Phase 2 lists each section's opaque runs first, its cutout runs after.
        let split = flat.pass_calls[2] - flat.phase1_calls;
        let total = flat.solid_calls - flat.phase1_calls;
        pass.set_pipeline(&self.opaque);
        flat.draw_phase2(pass, 0..split, vertices);
        pass.set_pipeline(&self.cutout);
        flat.draw_phase2(pass, split..total, vertices);
    }

    /// Pass 2: shades the solid terrain the IDs show into `target`, the world's colour,
    /// over the sky drawn before. `terrain`: terrain.wgsl's group 0.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        terrain: &wgpu::BindGroup,
        flat: &Flat,
        arena: &Arena,
        timing: Option<&mut PassTiming>,
    ) {
        let (Some((ids, _)), Some((blocks, _))) = (&self.ids, &self.blocks) else {
            return;
        };
        let mut pages: Vec<u32> = flat.calls[..flat.solid_calls]
            .iter()
            .map(|c| c.page)
            .collect();
        pages.sort_unstable();
        pages.dedup();
        let Some(&any) = pages.first() else {
            return;
        };
        let table = self.blocks_per_page * 4;
        for &page in &pages {
            if let Some(from) = flat.block_table(page) {
                encoder.copy_buffer_to_buffer(from, 0, blocks, u64::from(page) * table, table);
            }
        }
        // Pages in groups of `PAGE_SLOTS` by number: a page's slot is its number's remainder.
        // Slots no drawn page fills take one that is, which no pixel's ID names.
        let mut chunks: Vec<u32> = pages.iter().map(|p| p / PAGE_SLOTS).collect();
        chunks.dedup();
        let groups: Vec<(u32, wgpu::BindGroup)> = chunks
            .into_iter()
            .map(|chunk| {
                let mut entries = vec![
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: blocks.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: flat.origins().as_entire_binding(),
                    },
                ];
                entries.extend((0..PAGE_SLOTS).map(|slot| {
                    let page = chunk * PAGE_SLOTS + slot;
                    let page = if pages.binary_search(&page).is_ok() {
                        page
                    } else {
                        any
                    };
                    wgpu::BindGroupEntry {
                        binding: 2 + slot,
                        resource: arena.buffer(page).as_entire_binding(),
                    }
                }));
                entries.push(wgpu::BindGroupEntry {
                    binding: 2 + PAGE_SLOTS,
                    resource: wgpu::BindingResource::TextureView(ids),
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Visibility buffer pages"),
                    layout: &self.page_layout,
                    entries: &entries,
                });
                (chunk, group)
            })
            .collect();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Horizon visibility buffer resolve"),
            timestamp_writes: timing.and_then(|t| t.writes("Visibility resolve")),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&self.resolve);
        pass.set_bind_group(0, terrain, &[]);
        // The group of pages goes as the instance: each draw shades the pixels whose quad is
        // in one of its pages (one draw, up to `PAGE_SLOTS` pages).
        for (chunk, group) in &groups {
            pass.set_bind_group(1, group, &[]);
            pass.draw(0..3, *chunk..*chunk + 1);
        }
    }
}
