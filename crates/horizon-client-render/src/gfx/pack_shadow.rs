//! A shader pack's shadow buffers (spec 6.3): the shadow map's depth (`shadowtex0`, and
//! `shadowtex1` without translucent geometry), and the colour buffers `shadowcolor0` to `7`, each
//! with two sides that `shadowcomp` programs flip as composite programs flip colortex (6.1.6).
//! Square, `shadowMapResolution` pixels.

use horizon_shaders::SHADOW_COLORS;

type Ops<T> = wgpu::Operations<T>;
type Color = wgpu::Color;

/// Loaded and stored.
pub(super) fn load<T>() -> Ops<T> {
    wgpu::Operations {
        load: wgpu::LoadOp::Load,
        store: wgpu::StoreOp::Store,
    }
}

fn clear(c: [f32; 4]) -> Ops<Color> {
    wgpu::Operations {
        load: wgpu::LoadOp::Clear(wgpu::Color {
            r: c[0].into(),
            g: c[1].into(),
            b: c[2].into(),
            a: c[3].into(),
        }),
        store: wgpu::StoreOp::Store,
    }
}

/// What the shadow pass writes and the programs read.
pub(super) struct ShadowTargets {
    pub size: u32,
    /// `shadowtex0`, written by the shadow pass.
    pub depth: (wgpu::Texture, wgpu::TextureView),
    /// `shadowtex1`: a copy of `shadowtex0` taken before translucent terrain, when read.
    pub depth1: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// `shadowtex0`/`1` as a plain sampler filters them: Iris 1.11.7 reads them linearly unless
    /// a pack asks for the nearest texel (measured). wgpu filters a depth format only for a
    /// comparison, so these are `R32Float` copies of the final depths, when read that way.
    pub linear: [Option<wgpu::TextureView>; 2],
    /// The rows a depth copy goes through to `linear` (a depth format copies only to a buffer),
    /// and their stride in bytes.
    staging: Option<(wgpu::Buffer, u32)>,
    /// Both sides of each shadowcolor buffer used; one texture for both when no `shadowcomp`
    /// program draws into or flips the buffer.
    pub color: [Option<[wgpu::TextureView; 2]>; SHADOW_COLORS],
    /// The clear colour of each buffer cleared every frame.
    pub clears: [Option<[f32; 4]>; SHADOW_COLORS],
    /// Bit n set: B is shadowcolor n's "main".
    pub main: u16,
    /// Nothing cleared yet this frame: the first pass into the shadow buffers clears them as it
    /// begins (`clear_ops`), which saves a pass that loads and stores them all on tiled GPUs.
    fresh: bool,
    /// Bit n set: shadowcolor n was cleared that way.
    cleared: u8,
    /// Depth comparison for `sampler2DShadow` (hardware filtering): linear, as GL filters it.
    pub comparison: wgpu::Sampler,
}

fn target(
    device: &wgpu::Device,
    label: &str,
    size: u32,
    format: wgpu::TextureFormat,
    copy: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | copy,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

impl ShadowTargets {
    /// The buffers of `shadow` the pack uses: `colors` (bit n: shadowcolor n), `depth1` when
    /// some program reads `shadowtex1`, `linear` (bit n: shadowtex n read through a filtering
    /// sampler), `doubled` (bit n: shadowcolor n needs a B texture). The resolution is kept
    /// within the device's limit.
    pub fn new(
        device: &wgpu::Device,
        shadow: &horizon_shaders::Shadow,
        formats: [wgpu::TextureFormat; SHADOW_COLORS],
        colors: u16,
        doubled: u16,
        depth1: bool,
        linear: u8,
    ) -> Self {
        let size = shadow
            .resolution
            .clamp(1, device.limits().max_texture_dimension_2d);
        let copy = wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        let depth = wgpu::TextureFormat::Depth32Float;
        let color = std::array::from_fn(|n| {
            (colors & 1 << n != 0).then(|| {
                let side = |side: &str| {
                    let label = format!("Shader pack shadowcolor{n} {side}");
                    target(
                        device,
                        &label,
                        size,
                        formats[n],
                        wgpu::TextureUsages::empty(),
                    )
                    .1
                };
                let a = side("A");
                let b = if doubled & 1 << n != 0 {
                    side("B")
                } else {
                    a.clone()
                };
                [a, b]
            })
        });
        let linear = [0, 1].map(|n| {
            (linear & 1 << n != 0).then(|| {
                let label = format!("Shader pack shadowtex{n} (filtered)");
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(&label),
                        size: wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::R32Float,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            })
        });
        let staging = linear.iter().any(Option::is_some).then(|| {
            let stride = (size * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Shader pack shadow depth rows"),
                size: u64::from(stride) * u64::from(size),
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (buffer, stride)
        });
        Self {
            size,
            depth: target(device, "Shader pack shadowtex0", size, depth, copy),
            depth1: depth1.then(|| target(device, "Shader pack shadowtex1", size, depth, copy)),
            linear,
            staging,
            color,
            clears: std::array::from_fn(|n| {
                let b = &shadow.colors[n];
                b.clear.then(|| b.clear_color.unwrap_or([1.0; 4]))
            }),
            main: 0,
            fresh: false,
            cleared: 0,
            comparison: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Shader pack shadow comparison"),
                compare: Some(wgpu::CompareFunction::LessEqual),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    /// Side `alt` (of "main" and "alt") of shadowcolor `n`.
    pub fn color(&self, n: usize, alt: bool) -> Option<&wgpu::TextureView> {
        let side = usize::from(self.main & 1 << n != 0) ^ usize::from(alt);
        self.color.get(n)?.as_ref().map(|c| &c[side])
    }

    /// A new frame: the shadow map and the shadowcolor buffers cleared every frame (6.3) are
    /// cleared by the first pass that begins (`clear_ops`), or else by `clear_rest`.
    pub fn begin_frame(&mut self) {
        self.fresh = true;
        self.cleared = 0;
    }

    /// The load operations of a pass into the shadow map and shadowcolor buffers `writes`:
    /// clears in the frame's first one, loads after.
    pub fn clear_ops(
        &mut self,
        writes: impl Iterator<Item = usize>,
    ) -> (Ops<f32>, [Ops<Color>; SHADOW_COLORS]) {
        if !std::mem::take(&mut self.fresh) {
            return (load(), [load(); SHADOW_COLORS]);
        }
        let mut colors = [load(); SHADOW_COLORS];
        for n in writes.filter(|&n| n < SHADOW_COLORS) {
            if let Some(c) = self.clears[n] {
                colors[n] = clear(c);
                self.cleared |= 1 << n;
            }
        }
        let depth = wgpu::Operations {
            load: wgpu::LoadOp::Clear(1.0),
            store: wgpu::StoreOp::Store,
        };
        (depth, colors)
    }

    /// Clears what no pass has cleared this frame: all of it when no shadow program drew, the
    /// cleared shadowcolor buffers the first one did not write. One buffer a pass: eight wide
    /// formats together would pass the device's bytes-per-sample limit.
    pub fn clear_rest(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let mut depth = std::mem::take(&mut self.fresh);
        let cleared = std::mem::replace(&mut self.cleared, u8::MAX);
        let attachments = (0..SHADOW_COLORS)
            .filter(|n| cleared & 1 << n == 0)
            .filter_map(|n| {
                Some(Some(wgpu::RenderPassColorAttachment {
                    view: self.color(n, false)?,
                    depth_slice: None,
                    resolve_target: None,
                    ops: clear(self.clears[n]?),
                }))
            });
        let passes: Vec<Vec<_>> = attachments.map(|a| vec![a]).collect();
        let passes = if passes.is_empty() && depth {
            vec![Vec::new()]
        } else {
            passes
        };
        for attachments in &passes {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader pack shadow clear"),
                color_attachments: attachments,
                depth_stencil_attachment: std::mem::take(&mut depth).then_some(
                    wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth.1,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                ),
                ..Default::default()
            });
        }
    }

    /// `shadowtex1`: the shadow map as it is now (before translucent geometry).
    pub fn copy_depth1(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some((copy, _)) = &self.depth1 {
            encoder.copy_texture_to_texture(
                self.depth.0.as_image_copy(),
                copy.as_image_copy(),
                self.depth.0.size(),
            );
        }
    }

    /// `linear`: the filtered copies of the depths the shadow pass left (`shadowtex1` from its
    /// copy before translucent geometry).
    pub fn copy_linear(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some((buffer, stride)) = &self.staging else {
            return;
        };
        for (n, linear) in self.linear.iter().enumerate() {
            let Some(linear) = linear else {
                continue;
            };
            let depth = match (n, &self.depth1) {
                (1, Some(d)) => &d.0,
                _ => &self.depth.0,
            };
            let rows = wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(*stride),
                    rows_per_image: None,
                },
            };
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    aspect: wgpu::TextureAspect::DepthOnly,
                    ..depth.as_image_copy()
                },
                rows,
                depth.size(),
            );
            encoder.copy_buffer_to_texture(rows, linear.texture().as_image_copy(), depth.size());
        }
    }
}
