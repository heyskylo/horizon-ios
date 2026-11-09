//! Screenshots of a swapchain that cannot be copied from. OpenGL's surfaces
//! offer `RENDER_ATTACHMENT` only, so a frame whose pixels are read back
//! (the screenshot key, a world's icon) is drawn into a texture that can be,
//! then drawn onto the swapchain image as a fullscreen triangle. A 16-bit frame
//! (`Renderer::frame_format`) with HDR output off goes the same way every frame.

/// The texture a frame to read back is drawn into, and the pass that puts it
/// on the swapchain afterwards.
#[derive(Default)]
pub struct Readback {
    texture: Option<wgpu::Texture>,
    /// The 8-bit texture `finish` put the frame on, where the swapchain image can't be read
    /// back; `present` then draws it instead of `texture`.
    finished: Option<wgpu::Texture>,
    blitter: Option<(wgpu::TextureFormat, wgpu::util::TextureBlitter)>,
    /// A frame drew into `texture` (or `finished`) and still has to reach the swapchain.
    drawn: bool,
}

impl Readback {
    /// The texture this frame draws into instead of the swapchain image
    /// `surface`, in the frame's `format`: `None` unless that differs from the
    /// swapchain's, or the frame is read back (`screenshot`) and the image
    /// cannot be copied from.
    pub fn target(
        &mut self,
        device: &wgpu::Device,
        surface: &wgpu::Texture,
        format: wgpu::TextureFormat,
        screenshot: bool,
    ) -> Option<wgpu::Texture> {
        if format == surface.format().remove_srgb_suffix()
            && (!screenshot || surface.usage().contains(wgpu::TextureUsages::COPY_SRC))
        {
            return None;
        }
        if self
            .texture
            .as_ref()
            .is_none_or(|t| t.size() != surface.size() || t.format() != format)
        {
            self.texture = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Horizon screenshot frame"),
                size: surface.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }));
        }
        self.drawn = true;
        self.texture.clone()
    }

    /// The frame `target` gave, finished onto 8 bits of the swapchain image `surface`'s format,
    /// clipped and rounded as an 8-bit frame holds it, for what draws over it from here on
    /// (`Renderer::overlay_format`): `surface` itself, or, for a screenshot it can't be copied
    /// from, a texture of its own that `present` then draws there.
    pub fn finish(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        surface: &wgpu::Texture,
        screenshot: bool,
    ) -> Option<wgpu::Texture> {
        let frame = self.texture.clone().filter(|_| self.drawn)?;
        let format = surface.format().remove_srgb_suffix();
        let target = if !screenshot || surface.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            self.drawn = false;
            surface.clone()
        } else {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Horizon finished frame"),
                size: frame.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.finished = Some(texture.clone());
            texture
        };
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            ..Default::default()
        });
        self.blit(device, encoder, &frame, &view, format);
        Some(target)
    }

    /// Draws the frame `target` (or `finish`) gave onto the swapchain image `surface`; nothing
    /// when this frame drew straight into it.
    pub fn present(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        surface: &wgpu::Texture,
    ) {
        let finished = self.finished.take();
        if !std::mem::take(&mut self.drawn) {
            return;
        }
        let Some(texture) = finished.or_else(|| self.texture.clone()) else {
            return;
        };
        self.draw_onto(device, encoder, &texture, surface);
    }

    /// Draws `frame` onto the swapchain image (or offscreen target) `surface`.
    pub fn draw_onto(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::Texture,
        surface: &wgpu::Texture,
    ) {
        // The frame's passes write the swapchain through its non-sRGB view
        // (`view_formats`), so the copy goes there unconverted too.
        let format = surface.format().remove_srgb_suffix();
        let target = surface.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            ..Default::default()
        });
        self.blit(device, encoder, frame, &target, format);
    }

    /// `frame` (a 16-bit frame) in an 8-bit texture of `format`, clamped to 0..1
    /// as an 8-bit frame holds it, for a screenshot to read back.
    pub fn unorm_copy(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::Texture,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        let copy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Horizon screenshot copy"),
            size: frame.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = copy.create_view(&Default::default());
        self.blit(device, encoder, frame, &target, format);
        copy
    }

    fn blit(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) {
        if self.blitter.as_ref().is_none_or(|b| b.0 != format) {
            self.blitter = Some((format, wgpu::util::TextureBlitter::new(device, format)));
        }
        let source = source.create_view(&Default::default());
        if let Some((_, blitter)) = &self.blitter {
            blitter.copy(device, encoder, &source, target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = crate::gfx::test_gpu::instance();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    fn texture(device: &wgpu::Device, usage: wgpu::TextureUsages) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 70,
                height: 3,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: F,
            usage,
            view_formats: &[],
        })
    }

    fn clear(encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture, color: wgpu::Color) {
        let view = texture.create_view(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }

    fn read(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
        buffer.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        buffer.get_mapped_range(..).unwrap().to_vec()
    }

    /// The last pixel of the last row of a 70x3 frame read back by `copy_frame`.
    fn last_pixel(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
        let pixels = read(device, buffer);
        let row = 70usize * 4;
        let last = row.next_multiple_of(256) * 2 + row - 4;
        pixels[last..last + 4].to_vec()
    }

    /// The renderer's screenshot path on a surface like OpenGL's (#3510): the
    /// frame is drawn into `target`, read back with `copy_frame` and drawn on
    /// the swapchain, with no validation error (the copy used to need the
    /// swapchain's missing `COPY_SRC`), and the swapchain shows the frame.
    #[test]
    fn screenshot_of_a_surface_without_copy_src() {
        let Some((device, queue)) = device() else {
            return;
        };
        // Sampled only so the test can look at what reached it.
        let surface = texture(
            &device,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let check = texture(
            &device,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let blitter = wgpu::util::TextureBlitter::new(&device, F);
        let mut readback = Readback::default();
        let colors = [
            (1.0, [0, 0, 255, 255]),
            // A second screenshot reuses the texture.
            (0.0, [255, 0, 0, 255]),
        ];
        for (r, bgra) in colors {
            let color = wgpu::Color {
                r,
                g: 0.0,
                b: 1.0 - r,
                a: 1.0,
            };
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut encoder = device.create_command_encoder(&Default::default());
            let frame = readback
                .target(&device, &surface, F, true)
                .unwrap_or_else(|| surface.clone());
            clear(&mut encoder, &frame, color);
            let buffer = crate::gfx::copy_frame(&device, &mut encoder, &frame, [70, 3]);
            readback.present(&device, &mut encoder, &surface);
            queue.submit([encoder.finish()]);
            if let Some(error) = pollster::block_on(scope.pop()) {
                panic!("{error}");
            }
            assert_eq!(last_pixel(&device, &buffer), bgra);
            let mut encoder = device.create_command_encoder(&Default::default());
            let view = |t: &wgpu::Texture| t.create_view(&Default::default());
            blitter.copy(&device, &mut encoder, &view(&surface), &view(&check));
            let buffer = crate::gfx::copy_frame(&device, &mut encoder, &check, [70, 3]);
            queue.submit([encoder.finish()]);
            assert_eq!(last_pixel(&device, &buffer), bgra);
        }
        assert!(!readback.drawn);
    }

    /// A surface that can be copied from, or a frame nobody reads back,
    /// draws straight into the swapchain image.
    #[test]
    fn frames_draw_straight_to_a_readable_surface() {
        let Some((device, _)) = device() else {
            return;
        };
        let mut readback = Readback::default();
        let readable = texture(
            &device,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        assert!(readback.target(&device, &readable, F, true).is_none());
        let surface = texture(&device, wgpu::TextureUsages::RENDER_ATTACHMENT);
        assert!(readback.target(&device, &surface, F, false).is_none());
        assert!(readback.texture.is_none() && !readback.drawn);
    }

    /// A 16-bit frame with HDR output off draws into a 16-bit texture of its own, and both
    /// the swapchain and a screenshot's 8-bit copy get it clamped as an 8-bit frame holds it.
    fn reaches_an_8_bit_swapchain(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        color: wgpu::Color,
        expected: [u8; 4],
    ) {
        let surface = texture(
            device,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let mut readback = Readback::default();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = device.create_command_encoder(&Default::default());
        let frame = readback.target(device, &surface, format, false).unwrap();
        assert_eq!(frame.format(), format);
        clear(&mut encoder, &frame, color);
        let copy = readback.unorm_copy(device, &mut encoder, &frame, F);
        let screenshot = crate::gfx::copy_frame(device, &mut encoder, &copy, [70, 3]);
        readback.present(device, &mut encoder, &surface);
        let presented = crate::gfx::copy_frame(device, &mut encoder, &surface, [70, 3]);
        queue.submit([encoder.finish()]);
        if let Some(error) = pollster::block_on(scope.pop()) {
            panic!("{error}");
        }
        assert_eq!(last_pixel(device, &screenshot), expected);
        assert_eq!(last_pixel(device, &presented), expected);
    }

    /// `Rgba16Unorm`, the frame HDR output draws into where the device can
    /// (`Renderer::frame_format`).
    #[test]
    fn a_16_bit_unorm_frame_reaches_an_8_bit_swapchain() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let wide = wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        if !adapter.features().contains(wide) {
            return;
        }
        let descriptor = wgpu::DeviceDescriptor {
            required_features: wide,
            ..Default::default()
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&descriptor)) else {
            return;
        };
        let color = wgpu::Color {
            r: 1.0,
            g: 0.2,
            b: 0.0,
            a: 1.0,
        };
        let unorm = wgpu::TextureFormat::Rgba16Unorm;
        reaches_an_8_bit_swapchain(&device, &queue, unorm, color, [0, 51, 255, 255]);
    }

    /// `Rgba16Float`, the frame of a shader API 0.2 pack, or of HDR on without a pack, under
    /// `HORIZON_HDR_FRAME=float` (`Renderer::frame_format`): what is left over 1 is clipped on
    /// the way.
    #[test]
    fn a_float_frame_reaches_an_8_bit_swapchain() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let color = wgpu::Color {
            r: 1.5,
            g: 0.2,
            b: -0.5,
            a: 1.0,
        };
        let float = wgpu::TextureFormat::Rgba16Float;
        reaches_an_8_bit_swapchain(&device, &queue, float, color, [0, 51, 255, 255]);
    }

    /// `finish` puts a float frame on 8 bits before the overlays draw: on the swapchain image
    /// itself, which `present` then leaves alone, or, for a screenshot of an image that can't
    /// be copied from, on a texture of its own that the screenshot reads and `present` draws.
    #[test]
    fn a_float_frame_finishes_on_8_bits_before_the_overlays() {
        let Some((device, queue)) = device() else {
            return;
        };
        let float = wgpu::TextureFormat::Rgba16Float;
        let color = wgpu::Color {
            r: 1.5,
            g: 0.2,
            b: -0.5,
            a: 1.0,
        };
        let check = texture(
            &device,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let blitter = wgpu::util::TextureBlitter::new(&device, F);
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        for (usage, screenshot) in [
            (wgpu::TextureUsages::COPY_SRC, false),
            (wgpu::TextureUsages::COPY_SRC, true),
            (wgpu::TextureUsages::empty(), false),
            (wgpu::TextureUsages::empty(), true),
        ] {
            let surface = texture(
                &device,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | usage,
            );
            let mut readback = Readback::default();
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut encoder = device.create_command_encoder(&Default::default());
            let frame = readback
                .target(&device, &surface, float, screenshot)
                .unwrap();
            clear(&mut encoder, &frame, color);
            let finished = readback
                .finish(&device, &mut encoder, &surface, screenshot)
                .unwrap();
            assert_eq!(finished.format(), F);
            let own = screenshot && usage.is_empty();
            assert_eq!(finished == surface, !own, "{usage:?} {screenshot}");
            assert_eq!(readback.drawn, own);
            let shot = screenshot
                .then(|| crate::gfx::copy_frame(&device, &mut encoder, &finished, [70, 3]));
            readback.present(&device, &mut encoder, &surface);
            blitter.copy(&device, &mut encoder, &view(&surface), &view(&check));
            let shown = crate::gfx::copy_frame(&device, &mut encoder, &check, [70, 3]);
            queue.submit([encoder.finish()]);
            if let Some(error) = pollster::block_on(scope.pop()) {
                panic!("{usage:?} {screenshot}: {error}");
            }
            assert_eq!(last_pixel(&device, &shown), [0, 51, 255, 255]);
            if let Some(shot) = shot {
                assert_eq!(last_pixel(&device, &shot), [0, 51, 255, 255]);
            }
            assert!(!readback.drawn && readback.finished.is_none());
        }
    }
}
