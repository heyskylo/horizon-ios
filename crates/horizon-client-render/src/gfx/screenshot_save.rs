//! Writing a screenshot's PNG once its frame is read back: on the render thread for the
//! requests that want the result with the frame (an agent's, a `screenshot-sequence`'s),
//! on a thread of its own for F2's and the world icon's (`ScreenshotRequest::background`).
//! The render thread then neither waits for the GPU to finish the frame nor encodes the
//! PNG, which at 4K or a Screenshot Quality above 1x froze the game for a second or more.
use super::{ScreenshotRequest, hdr_icc, hdr10_to_rgb16, padded_row, square_downscale};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// What a frame's readback holds and where its PNG goes.
pub(super) struct Shot {
    pub path: PathBuf,
    /// `ScreenshotRequest::size`: the world icon's square.
    pub size: Option<[u32; 2]>,
    /// The frame's size, which the readback's rows hold (padded to `padded_row`).
    pub frame: [u32; 2],
    /// Blue first (`Bgra8Unorm`), swapped to PNG's red first.
    pub bgra: bool,
    /// The HDR10 copy's readback and its PNG's path, beside the SDR one.
    pub hdr: Option<(PathBuf, wgpu::Buffer)>,
}
impl Shot {
    pub fn new(
        request: &ScreenshotRequest,
        frame: [u32; 2],
        format: wgpu::TextureFormat,
        hdr: Option<(PathBuf, wgpu::Buffer)>,
    ) -> Self {
        Self {
            path: request.path.clone(),
            size: request.size,
            frame,
            bgra: format.remove_srgb_suffix() == wgpu::TextureFormat::Bgra8Unorm,
            hdr,
        }
    }
}
/// A written screenshot: its path, its size or the error, and the HDR copy written beside it.
pub struct Saved {
    pub path: PathBuf,
    pub result: Result<[u32; 2], String>,
    pub hdr: Option<PathBuf>,
}
/// Waits for `buffer`'s mapping on the render thread and writes the PNG there.
pub(super) fn save_now(device: &wgpu::Device, shot: Shot, buffer: &wgpu::Buffer) -> Saved {
    let (sender, receiver) = channel();
    map(buffer, &sender);
    if let Some((_, hdr)) = &shot.hdr {
        map(hdr, &sender);
    }
    let waited = device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string());
    let count = 1 + usize::from(shot.hdr.is_some());
    let mapped = waited.and_then(|_| wait_mapped(&receiver, count));
    write(shot, buffer, mapped)
}
/// Screenshots being written on their thread.
pub(super) struct Saves {
    device: wgpu::Device,
    sender: Sender<Saved>,
    receiver: Receiver<Saved>,
    in_flight: usize,
}
impl Saves {
    pub fn new(device: &wgpu::Device) -> Self {
        let (sender, receiver) = channel();
        Self {
            device: device.clone(),
            sender,
            receiver,
            in_flight: 0,
        }
    }
    /// Maps `buffer` (and the HDR copy's), whose copy was just submitted, and writes the
    /// PNG on a thread of its own once the GPU has finished the frame.
    pub fn start(&mut self, shot: Shot, buffer: wgpu::Buffer) {
        let (mapped, on_mapped) = channel();
        map(&buffer, &mapped);
        if let Some((_, hdr)) = &shot.hdr {
            map(hdr, &mapped);
        }
        let done = self.sender.clone();
        let path = shot.path.clone();
        let spawned = std::thread::Builder::new()
            .name("Screenshot writer".into())
            .spawn(move || {
                let count = 1 + usize::from(shot.hdr.is_some());
                let ready = wait_mapped(&on_mapped, count);
                let _ = done.send(write(shot, &buffer, ready));
            });
        match spawned {
            Ok(_) => {}
            Err(e) => {
                let _ = self.sender.send(Saved {
                    path,
                    result: Err(format!("no thread to write it: {e}")),
                    hdr: None,
                });
            }
        }
        self.in_flight += 1;
    }
    /// The screenshots written since the last call. Polls the device so a finished frame's
    /// mapping reaches its writer: the render thread never waits for it.
    pub fn finished(&mut self) -> Vec<Saved> {
        if self.in_flight == 0 {
            return Vec::new();
        }
        let _ = self.device.poll(wgpu::PollType::Poll);
        let saved: Vec<Saved> = self.receiver.try_iter().collect();
        self.in_flight -= saved.len();
        saved
    }
}
impl Drop for Saves {
    /// A screenshot taken right before quitting is still written whole.
    fn drop(&mut self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while self.in_flight > 0 && std::time::Instant::now() < deadline {
            let _ = self.device.poll(wgpu::PollType::Poll);
            if let Ok(saved) = self.receiver.recv_timeout(Duration::from_millis(20)) {
                log_saved(&saved);
                self.in_flight -= 1;
            }
        }
    }
}
fn log_saved(saved: &Saved) {
    match &saved.result {
        Ok(_) => log::info!("Saved screenshot to {}", saved.path.display()),
        Err(e) => log::warn!("Horizon renderer: screenshot {}: {e}", saved.path.display()),
    }
}
fn map(buffer: &wgpu::Buffer, mapped: &Sender<Result<(), String>>) {
    let mapped = mapped.clone();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = mapped.send(r.map_err(|e| e.to_string()));
    });
}
/// `count` mappings done, or the first one that failed. A frame the GPU never finishes
/// (a lost device) gives up after a while instead of keeping its thread.
fn wait_mapped(mapped: &Receiver<Result<(), String>>, count: usize) -> Result<(), String> {
    for _ in 0..count {
        mapped
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| "the frame never came back from the GPU".to_string())??;
    }
    Ok(())
}
/// The PNG (and the HDR copy's beside it) from the mapped readbacks.
fn write(shot: Shot, buffer: &wgpu::Buffer, mapped: Result<(), String>) -> Saved {
    let mut hdr_written = None;
    let result = mapped.and_then(|()| {
        if let Some((hdr_path, hdr_buffer)) = &shot.hdr {
            let pixels = read_rows(hdr_buffer, shot.frame);
            match pixels.and_then(|p| write_hdr_png(hdr_path, shot.frame, &p)) {
                Ok(()) => {
                    log::info!("Saved HDR screenshot to {}", hdr_path.display());
                    hdr_written = Some(hdr_path.clone());
                }
                Err(e) => log::warn!(
                    "Horizon renderer: HDR screenshot {}: {e}",
                    hdr_path.display()
                ),
            }
        }
        let pixels = read_rows(buffer, shot.frame)?;
        write_png(&shot, &pixels)
    });
    Saved {
        path: shot.path,
        result,
        hdr: hdr_written,
    }
}
/// The pixels `copy_frame` read back into the mapped `buffer`, 4 bytes each, row after
/// row; unmaps it.
fn read_rows(buffer: &wgpu::Buffer, [width, height]: [u32; 2]) -> Result<Vec<[u8; 4]>, String> {
    let (width, height, row) = (width as usize, height as usize, padded_row(width) as usize);
    let mut pixels = Vec::with_capacity(width * height);
    {
        let data = buffer.get_mapped_range(..).map_err(|e| e.to_string())?;
        for y in 0..height {
            pixels.extend_from_slice(data[y * row..y * row + width * 4].as_chunks::<4>().0);
        }
    }
    buffer.unmap();
    Ok(pixels)
}
fn write_png(shot: &Shot, frame: &[[u8; 4]]) -> Result<[u32; 2], String> {
    let [frame_w, frame_h] = shot.frame;
    let mut pixels = Vec::with_capacity(frame.len() * 4);
    for px in frame {
        let (r, b) = if shot.bgra {
            (px[2], px[0])
        } else {
            (px[0], px[2])
        };
        pixels.extend_from_slice(&[r, px[1], b, 255]);
    }
    // NativeImage.resizeSubRectTo on takeAutoScreenshot's grab: the icon
    // is the frame's centre square averaged down to its size.
    let (width, height, pixels) = match shot.size {
        Some(size) => (
            size[0],
            size[1],
            square_downscale(&pixels, frame_w, frame_h, size),
        ),
        None => (frame_w, frame_h, pixels),
    };
    let file = std::fs::File::create(&shot.path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&pixels))
        .map_err(|e| e.to_string())?;
    Ok([width, height])
}
/// The HDR10 copy (`Hdr::capture`) as a 16-bit PNG tagged BT.2020 with PQ (cICP, PNG's
/// third edition), with the content light levels its pixels hold (cLLI), and
/// an ICC profile for the programs that don't read cICP (`hdr_icc`).
fn write_hdr_png(
    path: &std::path::Path,
    [width, height]: [u32; 2],
    frame: &[[u8; 4]],
) -> Result<(), String> {
    let pixels = hdr10_to_rgb16(frame);
    let mut info = png::Info::with_size(width, height);
    info.color_type = png::ColorType::Rgb;
    info.bit_depth = png::BitDepth::Sixteen;
    info.icc_profile = Some(hdr_icc::profile().into());
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let written = png::Encoder::with_info(std::io::BufWriter::new(file), info)
        .and_then(|encoder| encoder.write_header())
        .and_then(|mut writer| {
            // BT.2020 primaries, PQ, RGB, full range.
            writer.write_chunk(png::chunk::cICP, &[9, 16, 0, 1])?;
            writer.write_chunk(png::chunk::cLLI, &hdr_icc::content_light_levels(&pixels))?;
            writer.write_image_data(&pixels)
        });
    written.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_background_save_writes_the_png_off_the_render_thread() {
        let instance = crate::gfx::test_gpu::instance();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("horizon-shot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        let [w, h] = [3u32, 2];
        // A frame of blue-first pixels, each row padded as `copy_frame` reads them back.
        let row = padded_row(w) as usize;
        let mut bytes = vec![0u8; row * h as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                bytes[y * row + x * 4..][..4].copy_from_slice(&[10 * x as u8, 20, 200, 0]);
            }
        }
        let source = wgpu::util::DeviceExt::create_buffer_init(
            &device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &bytes,
                usage: wgpu::BufferUsages::COPY_SRC,
            },
        );
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&source, 0, &buffer, 0, bytes.len() as u64);
        queue.submit([encoder.finish()]);
        let request = ScreenshotRequest {
            path: path.clone(),
            size: None,
            scaled: false,
            background: true,
        };
        let mut saves = Saves::new(&device);
        saves.start(
            Shot::new(&request, [w, h], wgpu::TextureFormat::Bgra8Unorm, None),
            buffer,
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let saved = loop {
            if let Some(saved) = saves.finished().pop() {
                break saved;
            }
            assert!(std::time::Instant::now() < deadline, "never written");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(saved.path, path);
        assert_eq!(saved.result, Ok([w, h]));
        let decoder =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut out = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut out).unwrap();
        // Red first, opaque: the last pixel of the first row was [20, 20, 200] blue-first.
        assert_eq!(&out[8..12], &[200, 20, 20, 255]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
