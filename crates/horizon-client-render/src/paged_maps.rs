//! The shared atlas's LabPBR maps (`_n`, `_s` beside an item's, entity's or block's texture),
//! for shader packs: a map the size of the atlas would be mostly empty (only some textures have
//! maps) and as large as the atlas itself, so each is a paged map instead. The atlas is cut into
//! 16x16 pages; only pages holding a mapped texture get one in the map's pool, found through a
//! page table at the map's start. Packs read the maps with the colour's coordinates, as for
//! terrain: the translator turns their reads into `hz_pagedTexture` (horizon-shaders,
//! `PAGED_READ`, which documents the layout this writes).

/// A page's side, in texels.
pub const PAGE: u32 = 16;
/// Texels before the page table: the atlas's size, and the map's value where it has none.
const HEADER: u32 = 2;

/// One paged map, RGBA8.
#[derive(Clone, Debug, PartialEq)]
pub struct PagedMap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A texture's maps: its place in the atlas (x, y, width, height, in texels) and its normal and
/// specular images at that size (RGBA8), each where it has one.
pub struct SpriteMaps {
    pub rect: [u32; 4],
    pub maps: [Option<Vec<u8>>; 2],
}

/// The normal and specular paged maps of an atlas of `size` texels, each `None` where no texture
/// has that map; `max_side` is the GPU's largest texture side.
pub fn build(size: (u32, u32), sprites: &[SpriteMaps], max_side: u32) -> [Option<PagedMap>; 2] {
    [
        (0, crate::sprites::FLAT_NORMAL),
        (1, crate::sprites::NO_SPECULAR),
    ]
    .map(|(kind, default)| {
        let mapped = sprites
            .iter()
            .filter_map(|s| s.maps[kind].as_deref().map(|m| (s.rect, m)));
        build_one(size, mapped, default, max_side)
    })
}

fn build_one<'a>(
    (width, height): (u32, u32),
    sprites: impl Iterator<Item = ([u32; 4], &'a [u8])>,
    default: [u8; 4],
    max_side: u32,
) -> Option<PagedMap> {
    let (pages_x, pages_y) = (width.div_ceil(PAGE), height.div_ceil(PAGE));
    // Each atlas page's place in the pool, plus one (0: none).
    let mut table = vec![0u32; (pages_x * pages_y) as usize];
    let mut pool: Vec<Vec<u8>> = Vec::new();
    let blank: Vec<u8> = default.repeat((PAGE * PAGE) as usize);
    for ([x, y, w, h], map) in sprites {
        if w == 0 || h == 0 || map.len() < (w * h * 4) as usize || x + w > width || y + h > height {
            continue;
        }
        for py in y / PAGE..=(y + h - 1) / PAGE {
            for px in x / PAGE..=(x + w - 1) / PAGE {
                let entry = &mut table[(py * pages_x + px) as usize];
                if *entry == 0 {
                    pool.push(blank.clone());
                    *entry = pool.len() as u32;
                }
                let page = &mut pool[*entry as usize - 1];
                // The sprite's texels on this page.
                let (x0, x1) = (x.max(px * PAGE), (x + w).min((px + 1) * PAGE));
                let (y0, y1) = (y.max(py * PAGE), (y + h).min((py + 1) * PAGE));
                for ty in y0..y1 {
                    let src = (((ty - y) * w + (x0 - x)) * 4) as usize;
                    let dst = (((ty - py * PAGE) * PAGE + (x0 - px * PAGE)) * 4) as usize;
                    let len = ((x1 - x0) * 4) as usize;
                    page[dst..dst + len].copy_from_slice(&map[src..src + len]);
                }
            }
        }
    }
    if pool.is_empty() || pool.len() >= 1 << 24 {
        return None;
    }
    let entries = HEADER + pages_x * pages_y;
    // The narrowest map width (whole pages) that keeps it within the GPU's side.
    let (side, rows, map_height) = [1024, 2048, 4096, 8192, 16384]
        .into_iter()
        .filter(|&s| s <= max_side)
        .map(|s| {
            let rows = entries.div_ceil(s).next_multiple_of(PAGE);
            let per_row = s / PAGE;
            (s, rows, rows + (pool.len() as u32).div_ceil(per_row) * PAGE)
        })
        .find(|&(_, _, h)| h <= max_side)?;
    let mut pixels = vec![0u8; (side * map_height * 4) as usize];
    let header = [
        width as u8,
        (width >> 8) as u8,
        height as u8,
        (height >> 8) as u8,
    ];
    pixels[..4].copy_from_slice(&header);
    pixels[4..8].copy_from_slice(&default);
    for (i, &k) in table.iter().enumerate() {
        let at = (HEADER as usize + i) * 4;
        pixels[at..at + 4].copy_from_slice(&[k as u8, (k >> 8) as u8, (k >> 16) as u8, 255]);
    }
    let per_row = side / PAGE;
    for (k, page) in pool.iter().enumerate() {
        let k = k as u32;
        let (ox, oy) = (k % per_row * PAGE, rows + k / per_row * PAGE);
        for row in 0..PAGE {
            let dst = (((oy + row) * side + ox) * 4) as usize;
            let src = (row * PAGE * 4) as usize;
            pixels[dst..dst + (PAGE * 4) as usize]
                .copy_from_slice(&page[src..src + (PAGE * 4) as usize]);
        }
    }
    Some(PagedMap {
        width: side,
        height: map_height,
        pixels,
    })
}

/// `map` (RGBA8, `mw` by `mh`) at `w` by `h`, nearest: a map drawn for a strip of animation
/// frames keeps its first, the colour's shape from its top.
pub fn fit(map: &[u8], (mw, mh): (u32, u32), (w, h): (u32, u32)) -> Option<Vec<u8>> {
    if mw == 0 || mh == 0 || w == 0 || h == 0 || map.len() < (mw * mh * 4) as usize {
        return None;
    }
    // The first frame of a strip as tall as the colour's frame is wide, scaled.
    let frame_h = (u64::from(mw) * u64::from(h) / u64::from(w)).clamp(1, u64::from(mh)) as u32;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let sy = (u64::from(y) * u64::from(frame_h) / u64::from(h)) as u32;
        for x in 0..w {
            let sx = (u64::from(x) * u64::from(mw) / u64::from(w)) as u32;
            let at = ((sy * mw + sx) * 4) as usize;
            out.extend_from_slice(&map[at..at + 4]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `PAGED_READ` returns at texel (x, y) of the atlas.
    fn read(map: &PagedMap, (width, height): (u32, u32), (x, y): (u32, u32)) -> [u8; 4] {
        let texel = |at: u32| {
            let at = (at * 4) as usize;
            [0, 1, 2, 3].map(|c| map.pixels[at + c])
        };
        let at = |x: u32, y: u32| texel(y * map.width + x);
        let h = texel(0);
        assert_eq!(
            (
                u32::from(h[0]) | u32::from(h[1]) << 8,
                u32::from(h[2]) | u32::from(h[3]) << 8
            ),
            (width, height)
        );
        let pages = (width.div_ceil(PAGE), height.div_ceil(PAGE));
        let e = texel(2 + (y / PAGE) * pages.0 + x / PAGE);
        let k = u32::from(e[0]) | u32::from(e[1]) << 8 | u32::from(e[2]) << 16;
        if k == 0 {
            return texel(1);
        }
        let rows = (2 + pages.0 * pages.1)
            .div_ceil(map.width)
            .next_multiple_of(16);
        let per_row = map.width / 16;
        at(
            (k - 1) % per_row * 16 + x % 16,
            rows + (k - 1) / per_row * 16 + y % 16,
        )
    }

    #[test]
    fn mapped_textures_read_back_and_the_rest_reads_the_default() {
        let size = (4096, 2048);
        // A 20x20 texture across four pages, and one without a specular map.
        let normal: Vec<u8> = (0..20 * 20)
            .flat_map(|i: u32| [i as u8, (i >> 8) as u8, 7, 255])
            .collect();
        let sprites = [
            SpriteMaps {
                rect: [30, 40, 20, 20],
                maps: [Some(normal.clone()), Some(vec![9; 20 * 20 * 4])],
            },
            SpriteMaps {
                rect: [1000, 1000, 16, 16],
                maps: [Some(vec![3; 16 * 16 * 4]), None],
            },
        ];
        let [normals, specular] = build(size, &sprites, 16384);
        let (normals, specular) = (normals.unwrap(), specular.unwrap());
        for y in 0..20 {
            for x in 0..20 {
                let i = ((y * 20 + x) * 4) as usize;
                assert_eq!(read(&normals, size, (30 + x, 40 + y)), normal[i..i + 4]);
                assert_eq!(read(&specular, size, (30 + x, 40 + y)), [9; 4]);
            }
        }
        // The texels beside it on its pages, and pages without a map.
        assert_eq!(read(&normals, size, (29, 40)), crate::sprites::FLAT_NORMAL);
        assert_eq!(read(&normals, size, (2000, 5)), crate::sprites::FLAT_NORMAL);
        assert_eq!(
            read(&specular, size, (1005, 1005)),
            crate::sprites::NO_SPECULAR
        );
        assert_eq!(read(&normals, size, (1005, 1005)), [3; 4]);
        // Five pages: a table of 2 + 256 x 128 entries, 33 rows at 1024 wide (48, whole pages),
        // then a row of pages.
        assert_eq!((normals.width, normals.height), (1024, 64));
    }

    #[test]
    fn nothing_mapped_makes_no_map() {
        let sprites = [SpriteMaps {
            rect: [0, 0, 16, 16],
            maps: [None, None],
        }];
        assert_eq!(build((1024, 1024), &sprites, 16384), [None, None]);
    }

    #[test]
    fn a_map_fits_its_texture() {
        // A 2x map is sampled down; a strip of frames keeps its first.
        let map: Vec<u8> = (0..4 * 8).flat_map(|i: u8| [i, 0, 0, 255]).collect();
        let fitted = fit(&map, (4, 8), (2, 2)).unwrap();
        assert_eq!(
            fitted.chunks(4).map(|p| p[0]).collect::<Vec<_>>(),
            [0, 2, 8, 10]
        );
    }
}
