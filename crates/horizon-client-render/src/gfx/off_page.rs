use super::*;

pub(super) fn move_off_page<'a>(
    arena: &mut arena::Arena,
    encoder: &mut wgpu::CommandEncoder,
    mut flat: Option<(&mut flat::Flat, &wgpu::Device, &mut upload::Uploads)>,
    mut records: Option<&mut gpu_culling::Records>,
    meshes: impl Iterator<Item = &'a mut SectionGpu>,
    page: u32,
    part: Part,
    budget: &mut u64,
    mut metadata: Option<(&mut terrain_metadata::TerrainMetadata, &wgpu::Device)>,
) -> bool {
    debug_assert!(
        records.is_none() || matches!(part, Part::Section { .. }),
        "records follow the section arena's ranges"
    );
    let mut moved = false;
    for s in meshes {
        // Nothing of a mesh here: split, one without solid quads (its (0, 0) is no
        // range of page 0); in the other arenas, one without translucent quads.
        let Some((_, start, vertices)) = part.of(s).filter(|r| r.0 == page) else {
            continue;
        };
        let bytes = vertices * arena.stride();
        if moved && bytes > *budget {
            break;
        }
        let Some(to) = arena.alloc_moved(vertices) else {
            return false;
        };
        if let Some((metadata, device)) = &mut metadata
            && !metadata.copy_range(
                device,
                encoder,
                (page, start),
                to,
                arena.buffer(to.0).size() / arena.stride(),
                vertices,
            )
        {
            arena.release(to.0, to.1, vertices);
            return false;
        }
        arena.copy(encoder, (page, start), to, vertices);
        arena.release(page, start, vertices);
        if let (Some((flat, device, uploads)), Some(id)) = (&mut flat, s.flat_id) {
            let key = match part {
                Part::Translucent => to.0 + flat::TRANSLUCENT_KEYS,
                _ => to.0,
            };
            flat.place(device, encoder, uploads, id, key, to.1, vertices);
        }
        part.set(s, to);
        // Its solid quads start the range in either layout.
        if let (Part::Section { .. }, Some(records), Some(id)) =
            (part, records.as_deref_mut(), s.gpu_id)
        {
            records.relocate(id, to.0, u64::from(s.solid_base()));
        }
        *budget = budget.saturating_sub(bytes);
        moved = true;
    }
    true
}
