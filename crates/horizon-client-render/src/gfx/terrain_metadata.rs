//! Block identity and source cell beside each arena quad, independent of its texture.
use super::upload::Uploads;
use crate::mesher::QuadState;
use std::collections::HashMap;
#[derive(Default)]
pub(super) struct TerrainMetadata {
    pages: HashMap<u32, wgpu::Buffer>,
}
pub(super) const WORDS: usize = 8;
pub(super) const BYTES: u64 = WORDS as u64 * 4;
pub(super) fn encode(state: QuadState) -> [u32; WORDS] {
    let mut words = [0; WORDS];
    words[0] = state.state;
    if state.state == u32::MAX {
        return words;
    }
    debug_assert!(state.block.iter().all(|&v| v < 16));
    words[1] = u32::from(state.block[0])
        | (u32::from(state.block[1]) << 4)
        | (u32::from(state.block[2]) << 8)
        | (u32::from(state.fluid) << 12);
    if let Some(light) = state.lighting {
        words[1] |= 1 << 13;
        words[2..6].copy_from_slice(&light.no_directional);
        words[6] = light.tint;
        words[7] = light.shaded_tint;
    }
    words
}

impl TerrainMetadata {
    /// `(page, first vertex, page capacity in vertices)` addresses the same arena as position data.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uploads: &mut Uploads,
        allocation: (u32, u64, u64),
        states: impl IntoIterator<Item = QuadState>,
    ) -> bool {
        let (page, start, capacity) = allocation;
        debug_assert_eq!(start % 4, 0);
        let bytes = capacity.div_ceil(4).max(1) * BYTES;
        if self
            .pages
            .get(&page)
            .is_none_or(|buffer| buffer.size() < bytes)
        {
            let Some(new) = super::arena::try_create(
                device,
                bytes,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
            ) else {
                return false;
            };
            if let Some(old) = self.pages.get(&page) {
                encoder.copy_buffer_to_buffer(old, 0, &new, 0, old.size());
            }
            self.pages.insert(page, new);
        }
        let words: Vec<[u32; WORDS]> = states.into_iter().map(encode).collect();
        if !words.is_empty() {
            uploads.buffer(
                encoder,
                &self.pages[&page],
                (start / 4) * BYTES,
                bytemuck::cast_slice(&words),
            );
        }
        true
    }
    pub fn page(&self, page: u32) -> &wgpu::Buffer {
        self.pages
            .get(&page)
            .expect("quad metadata accompanies every terrain page")
    }
    pub fn retain(&mut self, live: impl Fn(u32) -> bool) {
        self.pages.retain(|&page, _| live(page));
    }
    pub fn clear(&mut self) {
        self.pages.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_and_origin_are_independent_of_shared_texture_layers() {
        assert_eq!(
            &encode(QuadState::new(40, [3, 7, 12], false))[..2],
            &[40, 3 | (7 << 4) | (12 << 8)]
        );
        assert_eq!(
            &encode(QuadState::new(41, [3, 7, 12], true))[..2],
            &[41, 3 | (7 << 4) | (12 << 8) | (1 << 12)]
        );
        assert_eq!(&encode(QuadState::UNKNOWN)[..2], &[u32::MAX, 0]);
    }
    #[test]
    #[ignore = "requires Metal; run explicitly"]
    fn quad_sources_survive_arena_page_growth_and_release_on_metal() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut uploads = Uploads::new(&device);
        let mut metadata = TerrainMetadata::default();
        let mut encoder = device.create_command_encoder(&Default::default());
        let first = QuadState {
            state: 111,
            fluid: false,
            block: [3, 4, 5],
            lighting: None,
        };
        let second = QuadState {
            state: 222,
            fluid: true,
            block: [6, 7, 8],
            lighting: None,
        };
        assert!(metadata.upload(&device, &mut encoder, &mut uploads, (9, 4, 8), [first]));
        let original = metadata.page(9).clone();
        assert!(metadata.upload(&device, &mut encoder, &mut uploads, (9, 8, 16), [second]));
        assert_ne!(&original, metadata.page(9));
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 4 * BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(metadata.page(9), 0, &read, 0, 4 * BYTES);
        uploads.finish(&encoder);
        queue.submit([encoder.finish()]);
        read.slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = read.slice(..).get_mapped_range().unwrap();
        let words: &[[u32; WORDS]] = bytemuck::cast_slice(&data);
        assert_eq!(words[1], encode(first));
        assert_eq!(words[2], encode(second));
        drop(data);
        read.unmap();
        metadata.retain(|page| page != 9);
        assert!(metadata.pages.is_empty());
        let mut encoder = device.create_command_encoder(&Default::default());
        assert!(metadata.upload(
            &device,
            &mut encoder,
            &mut uploads,
            (9, 0, 4),
            [QuadState::UNKNOWN]
        ));
        uploads.finish(&encoder);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        metadata.clear();
        assert!(metadata.pages.is_empty());
    }
}
