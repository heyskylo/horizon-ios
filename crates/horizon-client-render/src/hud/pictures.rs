//! On-demand GUI pictures. The atlas keeps weak references; only a frame and
//! the renderer keep decoded pixels/textures alive, so leaving the screen frees both.
use super::*;
use std::sync::{
    Weak,
    atomic::{AtomicU64, Ordering},
};

pub struct Picture {
    pub(super) id: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pixels: Vec<u8>,
    pub(super) flags: TextureFlags,
}

#[derive(Default)]
pub(super) struct Cache {
    roots: [PathBuf; 2],
    entries: HashMap<String, Option<Weak<Picture>>>,
}

impl Cache {
    pub(super) fn new(root: &Path, defaults: &Path) -> Self {
        Self {
            roots: [root.into(), defaults.into()],
            entries: HashMap::new(),
        }
    }
    fn get(&mut self, name: &str) -> Option<Arc<Picture>> {
        if let Some(cached) = self.entries.get(name) {
            let weak = cached.as_ref()?;
            if let Some(image) = weak.upgrade() {
                return Some(image);
            }
        }
        let image = self
            .roots
            .iter()
            .find_map(|root| {
                let path = root.join(format!("assets/minecraft/textures/{name}.png"));
                let image = decode(&path).ok()?;
                let flags = horizon_core::prepared_files::read_to_string(
                    &path.with_extension("png.mcmeta"),
                )
                .ok()
                .as_deref()
                .and_then(TextureFlags::from_mcmeta)
                .unwrap_or_default();
                Some((image, flags))
            })
            .map(|((width, height, pixels), flags)| {
                static NEXT: AtomicU64 = AtomicU64::new(1);
                Arc::new(Picture {
                    id: NEXT.fetch_add(1, Ordering::Relaxed),
                    width,
                    height,
                    pixels,
                    flags,
                })
            });
        self.entries
            .insert(name.into(), image.as_ref().map(Arc::downgrade));
        image
    }
}

impl HudBatch {
    /// Draw a full GUI picture at its exact place in the HUD's ordered runs.
    pub fn picture(
        &mut self,
        atlas: &HudAtlas,
        name: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        tint: u32,
    ) {
        let Some(picture) = atlas.pictures.lock().unwrap().get(name) else {
            return;
        };
        let start = self.vertices.len();
        self.rect_uv(
            x,
            y,
            w,
            h,
            Rect {
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
                w: picture.width,
                h: picture.height,
            },
            tint,
        );
        self.blend_since(start, Blend::Picture(picture.id));
        if !self.pictures.iter().any(|p| p.id == picture.id) {
            self.pictures.push(picture);
        }
    }
}

impl HudRenderer {
    pub(super) fn draw_picture(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        id: u64,
        range: std::ops::Range<u32>,
        coverage: bool,
    ) {
        if let Some((_, group)) = self.pictures.get(&id) {
            pass.set_bind_group(0, group, &[]);
            pass.set_pipeline(self.pipeline.get(coverage));
            pass.draw(range, 0..1);
            pass.set_bind_group(0, &self.group, &[]);
        }
    }

    pub(super) fn prepare_pictures(
        &mut self,
        device: &wgpu::Device,
        uploads: &mut crate::gfx::upload::Uploads,
        encoder: &mut wgpu::CommandEncoder,
        batch: &HudBatch,
    ) {
        self.pictures
            .retain(|id, _| batch.pictures.iter().any(|p| p.id == *id));
        for picture in &batch.pictures {
            if picture.width > device.limits().max_texture_dimension_2d
                || picture.height > device.limits().max_texture_dimension_2d
            {
                continue;
            }
            self.pictures.entry(picture.id).or_insert_with(|| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("GUI picture"),
                    size: wgpu::Extent3d {
                        width: picture.width,
                        height: picture.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                uploads.texture(
                    encoder,
                    texture.as_image_copy(),
                    picture.width,
                    picture.height,
                    &picture.pixels,
                );
                let view = texture.create_view(&Default::default());
                let filter = if picture.flags.blur {
                    wgpu::FilterMode::Linear
                } else {
                    wgpu::FilterMode::Nearest
                };
                let address = if picture.flags.clamp {
                    wgpu::AddressMode::ClampToEdge
                } else {
                    wgpu::AddressMode::Repeat
                };
                let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                    mag_filter: filter,
                    min_filter: filter,
                    address_mode_u: address,
                    address_mode_v: address,
                    ..Default::default()
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("GUI picture"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&self.atlas_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::TextureView(&self.atlas_view),
                        },
                    ],
                });
                (picture.clone(), group)
            });
        }
    }
}

#[cfg(test)]
mod tests;
