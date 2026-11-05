//! Pregeneration (`/pregen`): every chunk of a square or a circle generated,
//! lit and saved ahead of the players. The area is cut into strips of columns
//! and each strip is swept north to south by a [`Band`] a few rows deep:
//! streaming generates the band's chunks and keeps them until they are lit,
//! then the band moves on and the rows behind it unload and are saved, so
//! memory stays bounded whatever the radius.
use crate::streaming::Band;

/// The opt-in command's report-format tree, beside vanilla's exported report.
/// The server sends this to clients so they can complete the command too.
pub fn command_tree() -> serde_json::Value {
    use serde_json::json;
    let center = json!({"type": "argument", "parser": "minecraft:vec2", "executable": true});
    let shape = json!({"type": "literal", "executable": true, "children": {"center": center}});
    let mut children = serde_json::Map::new();
    children.insert(
        "start".into(),
        json!({
            "type": "literal", "children": {"radius": {
                "type": "argument", "parser": "brigadier:integer",
                "properties": {"min": 0, "max": 30_000_000}, "executable": true,
                "children": {"square": shape, "circle": shape}
            }}
        }),
    );
    for name in ["status", "pause", "continue", "cancel"] {
        children.insert(name.into(), json!({"type": "literal", "executable": true}));
    }
    json!({"type": "root", "children": {"pregen": {
        "type": "literal",
        "permissions": {"type": "require", "permission": {"level": "gamemasters"}},
        "children": children
    }}})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Square,
    Circle,
}

/// The widest strip, in chunks. Strips are swept one after another; the few
/// columns along the edge between two are generated with both.
const STRIP_WIDTH: i32 = 256;
/// How many chunks a band holds at least: enough to keep every worker busy
/// while the rows before it finish.
const BAND_CHUNKS: i32 = 1024;

pub struct Pregen {
    /// The centre chunk and the radius in chunks.
    pub center: (i32, i32),
    pub radius: i32,
    pub shape: Shape,
    /// The strips' columns, west to east.
    strips: Vec<(i32, i32)>,
    strip: usize,
    /// The band's first row: every row of the strip before it is done.
    row: i32,
    /// Chunks of the area lit so far, and in all.
    pub done: u64,
    pub total: u64,
}

impl Pregen {
    pub fn new(center: (i32, i32), radius: i32, shape: Shape) -> Self {
        let radius = radius.max(0);
        let (x0, width) = (center.0 - radius, 2 * radius + 1);
        let count = (width + STRIP_WIDTH - 1) / STRIP_WIDTH;
        // The columns `i * width / count` on, in i64: the product overflows
        // for a radius of a few hundred thousand chunks.
        let edge = |i: i32| x0 + (i64::from(width) * i64::from(i) / i64::from(count)) as i32;
        let strips = (0..count).map(|i| (edge(i), edge(i + 1) - 1)).collect();
        let mut pregen = Self {
            center,
            radius,
            shape,
            strips,
            strip: 0,
            row: 0,
            done: 0,
            total: 0,
        };
        pregen.total = (center.1 - radius..=center.1 + radius)
            .map(|z| pregen.row_chunks(x0, x0 + width - 1, z))
            .sum();
        pregen.row = pregen.rows().map_or(0, |r| r.0);
        pregen
    }

    /// How far from the centre column the area reaches at `d` chunks from
    /// the centre row (or, the same, from the centre column), if it does.
    fn half_width(&self, d: i32) -> Option<i32> {
        let (r, d) = (i64::from(self.radius), i64::from(d));
        match self.shape {
            Shape::Square => (d.abs() <= r).then_some(self.radius),
            Shape::Circle => (d * d <= r * r).then(|| (r * r - d * d).isqrt() as i32),
        }
    }

    fn contains(&self, x: i32, z: i32) -> bool {
        self.half_width(z - self.center.1)
            .is_some_and(|h| (x - self.center.0).abs() <= h)
    }

    /// The area's chunks in columns `x0..=x1` of row `z`.
    fn row_chunks(&self, x0: i32, x1: i32, z: i32) -> u64 {
        self.half_width(z - self.center.1).map_or(0, |h| {
            let (lo, hi) = (x0.max(self.center.0 - h), x1.min(self.center.0 + h));
            (i64::from(hi) - i64::from(lo) + 1).max(0) as u64
        })
    }

    /// The first and last rows of the current strip that hold chunks of the
    /// area.
    fn rows(&self) -> Option<(i32, i32)> {
        let &(x0, x1) = self.strips.get(self.strip)?;
        let nearest = (x0 - self.center.0).max(self.center.0 - x1).max(0);
        let h = self.half_width(nearest)?;
        Some((self.center.1 - h, self.center.1 + h))
    }

    /// The band streaming should hold now; none once every strip is done.
    pub fn band(&self) -> Option<Band> {
        let &(x0, x1) = self.strips.get(self.strip)?;
        let (_, last) = self.rows()?;
        let rows = (BAND_CHUNKS + (x1 - x0)) / (x1 - x0 + 1);
        Some(Band {
            x0,
            x1,
            z0: self.row,
            z1: (self.row + rows - 1).min(last),
            circle: (self.shape == Shape::Circle).then_some((self.center, self.radius)),
        })
    }

    /// Move past the band's first rows once all their chunks are `lit`.
    /// Returns whether the band moved.
    pub fn advance(&mut self, lit: impl Fn(i32, i32) -> bool) -> bool {
        let mut moved = false;
        while let (Some(&(x0, x1)), Some((_, last))) = (self.strips.get(self.strip), self.rows()) {
            let z = self.row;
            if !(x0..=x1).all(|x| !self.contains(x, z) || lit(x, z)) {
                break;
            }
            self.done += self.row_chunks(x0, x1, z);
            self.row += 1;
            if self.row > last {
                self.strip += 1;
                self.row = self.rows().map_or(self.row, |r| r.0);
            }
            moved = true;
        }
        moved
    }

    /// The chunks of the band's first row, which it waits for.
    pub fn first_row(&self) -> Vec<(i32, i32)> {
        let Some(&(x0, x1)) = self.strips.get(self.strip) else {
            return Vec::new();
        };
        (x0..=x1)
            .filter(|&x| self.contains(x, self.row))
            .map(|x| (x, self.row))
            .collect()
    }

    pub fn finished(&self) -> bool {
        self.strip >= self.strips.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_every_chunk_of_the_area_once() {
        for shape in [Shape::Square, Shape::Circle] {
            for (center, radius) in [((0, 0), 0), ((3, -7), 5), ((-40, 12), 130), ((0, 0), 300)] {
                let mut p = Pregen::new(center, radius, shape);
                let r = i64::from(radius);
                let area = (-radius..=radius)
                    .flat_map(|dz| (-radius..=radius).map(move |dx| (dx, dz)))
                    .filter(|&(dx, dz)| {
                        shape == Shape::Square
                            || i64::from(dx).pow(2) + i64::from(dz).pow(2) <= r * r
                    })
                    .count() as u64;
                assert_eq!(p.total, area, "{shape:?} {radius}");
                let mut seen = std::collections::HashSet::new();
                while !p.finished() {
                    let band = p.band().unwrap();
                    assert!(band.z0 <= band.z1);
                    let row = p.first_row();
                    for &(x, z) in &row {
                        assert!(seen.insert((x, z)), "{x} {z} twice");
                    }
                    assert!(
                        p.advance(|x, z| z == band.z0 && row.contains(&(x, z))) || row.is_empty()
                    );
                }
                assert_eq!(
                    (p.done, seen.len() as u64),
                    (area, area),
                    "{shape:?} {radius}"
                );
            }
        }
    }

    #[test]
    fn a_huge_area_is_planned_at_once() {
        let started = std::time::Instant::now();
        let p = Pregen::new((0, 0), 30_000_000 >> 4, Shape::Circle);
        assert!(p.total > 11_000_000_000_000);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
