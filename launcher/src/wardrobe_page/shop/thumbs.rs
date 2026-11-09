//! The Shop's pictures: each item seen alone (a shirt on a plain figure, an emote on the player
//! at its fullest move), a look on the player, the pack as its looks side by side. Drawn by the
//! Wardrobe's own renderer (`horizon_cosmetics::preview`) at the screen's pixels, twice as fine
//! and then halved so that edges stay smooth, on a background thread; kept by the files' hashes,
//! so a picture is drawn again only when an item's file, the skin or the screen changes.
use super::*;
use glam::{Mat4, Vec3};
use horizon_cosmetics::preview::{self, Draw, Image};
use horizon_cosmetics::{Emote, Pose, BODY};
use std::collections::HashMap;

/// What a picture shows.
#[derive(Clone)]
pub(super) enum Subject {
    /// One item: alone (a trail's particles grown), on a plain figure (a shirt) or played by
    /// the player (an emote).
    Item(Arc<Item>),
    /// The player wearing every item (posed by the emote among them, if any).
    Look(Vec<Arc<Item>>),
    /// Each look on a player of its own, side by side.
    Pack(Vec<Vec<Arc<Item>>>),
}

/// A picture to draw: what, its key and hash (`Thumbs`), its size in points.
pub(super) struct Ask {
    pub key: String,
    pub hash: String,
    pub subject: Subject,
    pub size: (f32, f32),
}

/// The pictures drawn so far, by key, each with the hash of what it was drawn from.
#[derive(Default)]
pub(crate) struct Thumbs {
    scale: f32,
    baked: HashMap<String, (String, Arc<RenderImage>)>,
    /// Being drawn: key → hash.
    drawing: HashMap<String, String>,
    /// Pictures no longer shown, the atlas's to free.
    replaced: Vec<Arc<RenderImage>>,
    /// Asked for during this frame.
    asks: Vec<Ask>,
}

impl Thumbs {
    /// The picture for `key` drawn from `hash`, or the last one drawn under `key` while the new
    /// one is on its way; None (and asked for) the first time.
    pub(super) fn get(&mut self, ask: Ask, scale: f32) -> Option<Arc<RenderImage>> {
        if self.scale != scale {
            self.scale = scale;
            self.replaced
                .extend(self.baked.drain().map(|(_, (_, i))| i));
            self.drawing.clear();
        }
        let have = self.baked.get(&ask.key);
        let fresh = have.is_some_and(|(h, _)| *h == ask.hash);
        let image = have.map(|(_, i)| i.clone());
        if !fresh && self.drawing.get(&ask.key) != Some(&ask.hash) {
            self.drawing.insert(ask.key.clone(), ask.hash.clone());
            self.asks.push(ask);
        }
        image
    }

    pub(super) fn take_asks(&mut self) -> Vec<Ask> {
        std::mem::take(&mut self.asks)
    }

    /// A drawn picture is back (dropped if the screen changed since it was asked for).
    pub(super) fn put(&mut self, key: String, hash: String, scale: f32, image: Arc<RenderImage>) {
        if scale != self.scale || self.drawing.get(&key) != Some(&hash) {
            self.replaced.push(image);
            return;
        }
        self.drawing.remove(&key);
        if let Some((_, old)) = self.baked.insert(key, (hash, image)) {
            self.replaced.push(old);
        }
    }

    pub(crate) fn free_replaced(&mut self, window: &mut gpui::Window) {
        for image in self.replaced.drain(..) {
            let _ = window.drop_image(image);
        }
    }
}

/// The poses of the player model's six bones at rest, `BODY`'s order.
fn rest(body: &Part) -> [Pose; 6] {
    BODY.map(|b| (body.children.iter().find(|p| p.name == b.part())).map_or([0.0; 9], |p| p.pose))
}

/// The emote's most telling moment: of 48 points through it (its fades left out), the one where
/// the bones turn and move farthest from rest.
fn fullest(emote: &Emote, rest: &[Pose; 6]) -> f32 {
    let (from, to) = (emote.blend, (emote.length - emote.blend).max(emote.blend));
    let reach = |t: f32| {
        let mut poses = *rest;
        emote.pose_body(t, 1.0, rest, &mut poses);
        (poses.iter().zip(rest))
            .map(|(p, r)| {
                let turn: f32 = (3..6).map(|i| (p[i] - r[i]).abs()).sum();
                let shift: f32 = (0..3).map(|i| (p[i] - r[i]).abs()).sum();
                turn + shift / 16.0
            })
            .sum::<f32>()
    };
    (0..48)
        .map(|k| from + (to - from) * k as f32 / 47.0)
        .max_by(|a, b| reach(*a).total_cmp(&reach(*b)))
        .unwrap_or(0.0)
}

/// Poses by part name.
type Poses = Vec<(String, Pose)>;

/// An emote's body poses and its props' at `t`.
fn emote_poses(item: &Item, body: &Part, t: f32) -> (Poses, Poses) {
    let Some(emote) = &item.emote else {
        return (Vec::new(), Vec::new());
    };
    let rest = rest(body);
    let mut poses = rest;
    emote.pose_body(t, 1.0, &rest, &mut poses);
    let body = (BODY.iter().zip(poses))
        .map(|(b, p)| (b.part().to_owned(), p))
        .collect();
    let mut props = Vec::new();
    emote.pose_props(t, |n| item.rest(n), |n, p| props.push((n.to_owned(), p)));
    (body, props)
}

/// The draws of a player with `skin` wearing `items` (name tags left out), moved `at` blocks;
/// the first emote among them poses it.
fn dressed<'a>(
    body: &'a Part,
    skin: &'a Texture,
    items: &'a [Arc<Item>],
    at: Vec3,
) -> Vec<Draw<'a>> {
    let emote = items.iter().find(|i| i.emote.is_some());
    let (poses, props) = emote.map_or_else(Default::default, |e| {
        let t = e.emote.as_ref().map_or(0.0, |m| fullest(m, &rest(body)));
        emote_poses(e, body, t)
    });
    let mut draws = vec![Draw {
        poses: poses.clone(),
        ..Draw::model(body, skin)
    }];
    for item in items.iter().filter(|i| i.slot != Slot::NameTag) {
        let mut more = Draw::item_posed(item, body, &poses, 0.0);
        if item.emote.is_some() {
            for d in &mut more {
                d.poses.extend(props.iter().cloned());
            }
        }
        draws.extend(more);
    }
    let shift = Mat4::from_translation(at);
    for d in &mut draws {
        d.base = shift * d.base;
    }
    draws
}

/// The angle a slot's item is best seen from: (yaw, pitch) in degrees.
fn angle(slot: Slot) -> (f32, f32) {
    match slot {
        // Its face is the side away from the player.
        Slot::Back => (205.0, 15.0),
        Slot::Aura => (25.0, 40.0),
        Slot::Shoulder => (-30.0, 12.0),
        _ => (30.0, 15.0),
    }
}

/// `subject` drawn `width`×`height` pixels, transparent around it: twice as large, then each
/// 2×2 block averaged.
pub(super) fn draw(
    subject: &Subject,
    skin: &Texture,
    slim: bool,
    width: u32,
    height: u32,
) -> Image {
    let (w2, h2) = (width * 2, height * 2);
    let body = preview::player_model(slim);
    let figure = super::super::plain();
    let (draws, angle, margin): (Vec<Draw>, _, _) = match subject {
        Subject::Item(item) => match item.slot {
            Slot::Emote => (
                dressed(&body, skin, std::slice::from_ref(item), Vec3::ZERO),
                (25.0, 8.0),
                0.06,
            ),
            Slot::Trail => {
                let Some(trail) = preview::trail_part(item).map(grown) else {
                    return blank(width, height);
                };
                let d = vec![Draw {
                    root: std::borrow::Cow::Owned(trail),
                    ..Draw::model(&body, &item.texture)
                }];
                let image = match preview::fitted_view(&d, (w2, h2), (40.0, 10.0), 0.1) {
                    Some(view) => preview::render_view(&d, w2, h2, view),
                    None => blank(w2, h2),
                };
                return halve(&image);
            }
            Slot::Torso => {
                let mut d = dressed(&body, &figure, std::slice::from_ref(item), Vec3::ZERO);
                // Framed on the shirt: the figure only fills it.
                let shirt = d.split_off(1);
                let view = preview::fitted_view(&shirt, (w2, h2), (30.0, 12.0), 0.1);
                d.extend(shirt);
                return halve(&match view {
                    Some(view) => preview::render_view(&d, w2, h2, view),
                    None => blank(w2, h2),
                });
            }
            _ => (Draw::item(item, &body, 0.0), angle(item.slot), 0.1),
        },
        Subject::Look(items) => (dressed(&body, skin, items, Vec3::ZERO), (25.0, 8.0), 0.05),
        Subject::Pack(looks) => {
            let n = looks.len() as f32;
            let draws = (looks.iter().enumerate())
                .flat_map(|(k, items)| {
                    // A block and a half apart, the middle one a little nearer.
                    let x = (k as f32 - (n - 1.0) / 2.0) * 1.5;
                    let z = if (k as f32 - (n - 1.0) / 2.0).abs() < 0.5 {
                        -0.4
                    } else {
                        0.0
                    };
                    dressed(&body, skin, items, Vec3::new(x, 0.0, z))
                })
                .collect();
            (draws, (12.0, 6.0), 0.04)
        }
    };
    let image = match preview::fitted_view(&draws, (w2, h2), angle, margin) {
        Some(view) => preview::render_view(&draws, w2, h2, view),
        None => blank(w2, h2),
    };
    halve(&image)
}

/// A trail's particles three times their size, each about its centre: in a picture of the
/// trail alone they are what there is to see.
fn grown(mut trail: Part) -> Part {
    for face in &mut trail.faces {
        let n = face.vertices.len() as f32;
        let centre = (0..3).map(|k| face.vertices.iter().map(|v| v[k]).sum::<f32>() / n);
        let centre: Vec<f32> = centre.collect();
        for v in &mut face.vertices {
            for k in 0..3 {
                v[k] = centre[k] + (v[k] - centre[k]) * 3.0;
            }
        }
    }
    trail
}

fn blank(width: u32, height: u32) -> Image {
    Image {
        width,
        height,
        rgba: vec![0; (width * height * 4) as usize],
    }
}

/// Each 2×2 block of `image` as one pixel, its colour weighted by alpha.
fn halve(image: &Image) -> Image {
    let (w, h) = (image.width / 2, image.height / 2);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let at = |x: u32, y: u32| ((y * image.width + x) * 4) as usize;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = &image.rgba[at(x * 2 + dx, y * 2 + dy)..][..4];
                let a = u32::from(p[3]);
                for c in 0..3 {
                    sum[c] += u32::from(p[c]) * a;
                }
                sum[3] += a;
            }
            let out = &mut rgba[((y * w + x) * 4) as usize..][..4];
            for c in 0..3 {
                out[c] = sum[c].checked_div(sum[3]).unwrap_or(0) as u8;
            }
            out[3] = (sum[3] / 4) as u8;
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

/// The picture baked for the screen: `size` points at `scale` device pixels each.
pub(super) fn bake(
    subject: &Subject,
    skin: &Texture,
    slim: bool,
    size: (f32, f32),
    scale: f32,
) -> Arc<RenderImage> {
    let (w, h) = (
        (size.0 * scale).round() as u32,
        (size.1 * scale).round() as u32,
    );
    let image = draw(subject, skin, slim, w.max(1), h.max(1));
    let texels = image.rgba.as_chunks::<4>().0;
    art::bake(
        texels,
        w.max(1) as usize,
        h.max(1) as usize,
        1. / scale,
        scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(slot: &str, extra: &str) -> Arc<Item> {
        let png = {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(32, 32, image::Rgba([200, 120, 40, 255]))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
        };
        let file = format!(
            r#"{{"format": 1, "id": "x", "slot": "{slot}", "texture": {{"png": "{png}"}},
              "parts": [{{"name": "c", "cubes": [{{"origin": [-4, -12, -4], "size": [8, 4, 8], "uv": [0, 0]}}]}}]{extra}}}"#
        );
        Arc::new(Item::parse(file.as_bytes()).unwrap())
    }

    /// The share of the picture's pixels something covers.
    fn covered(image: &Image) -> f32 {
        let seen = image.rgba.chunks(4).filter(|p| p[3] > 0).count();
        seen as f32 / (image.width * image.height) as f32
    }

    #[test]
    fn an_item_alone_fills_its_picture() {
        let skin = super::super::super::plain();
        let image = draw(&Subject::Item(item("head", "")), &skin, false, 120, 120);
        assert_eq!((image.width, image.height), (120, 120));
        // A flat box seen from above at an angle: as wide as the square less its margins, and
        // none of the margin.
        let columns =
            (0..120).filter(|x| (0..120).any(|y| image.rgba[((y * 120 + x) * 4 + 3) as usize] > 0));
        assert!(columns.count() >= 90);
        assert!(covered(&image) > 0.2, "{}", covered(&image));
        let edge = (0..120).flat_map(|i| [(i, 0), (0, i), (i, 119), (119, i)]);
        assert!(edge
            .into_iter()
            .all(|(x, y)| image.rgba[((y * 120 + x) * 4 + 3) as usize] == 0));
    }

    #[test]
    fn a_look_shows_the_whole_player_and_a_pack_each_look() {
        let skin = super::super::super::plain();
        let hat = item("head", "");
        let look = draw(&Subject::Look(vec![hat.clone()]), &skin, false, 90, 160);
        let pack = draw(
            &Subject::Pack(vec![vec![hat.clone()]; 3]),
            &skin,
            false,
            270,
            160,
        );
        // Three players side by side: drawn pixels in each third of the picture.
        for third in 0..3 {
            let any = (0..160).any(|y| {
                (third * 90..third * 90 + 90)
                    .any(|x| pack.rgba[((y * 270 + x) * 4 + 3) as usize] > 0)
            });
            assert!(any, "third {third} is empty");
        }
        // The whole figure is taller than wide: it fills the height, not the width.
        let rows =
            (0..160).filter(|y| (0..90).any(|x| look.rgba[((y * 90 + x) * 4 + 3) as usize] > 0));
        assert!(rows.count() > 140);
    }

    #[test]
    fn an_emote_is_shown_at_its_fullest_move() {
        // The right arm goes up over 10 ticks, stays up to 30, comes down by 40.
        let wave = r#", "emote": {"length": 40, "blend": 4, "tracks": [{"part": "right_arm", "keys": [
            {"t": 0}, {"t": 10, "rotation": [0, 0, 150]}, {"t": 30, "rotation": [0, 0, 150]}, {"t": 40}]}]}"#;
        let emote = item("emote", wave);
        let body = preview::player_model(false);
        let t = fullest(emote.emote.as_ref().unwrap(), &rest(&body));
        assert!((10.0..=30.0).contains(&t), "{t}");
    }

    #[test]
    fn halving_keeps_colour_under_partial_cover() {
        let image = Image {
            width: 2,
            height: 2,
            rgba: vec![200, 100, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 200, 100, 0, 255],
        };
        // Half covered: half as opaque, the colour of what covers it (not darkened by the gaps).
        assert_eq!(halve(&image).rgba, vec![200, 100, 0, 127]);
    }

    #[test]
    fn a_picture_is_drawn_once_per_hash_and_scale() {
        let mut thumbs = Thumbs::default();
        let ask = |hash: &str| Ask {
            key: "fox_kit".into(),
            hash: hash.into(),
            subject: Subject::Look(Vec::new()),
            size: (10., 10.),
        };
        assert!(thumbs.get(ask("a"), 2.).is_none());
        assert!(thumbs.get(ask("a"), 2.).is_none());
        assert_eq!(thumbs.take_asks().len(), 1, "asked once while it is drawn");
        let image = bake(
            &Subject::Look(Vec::new()),
            &super::super::super::plain(),
            false,
            (4., 4.),
            2.,
        );
        thumbs.put("fox_kit".into(), "a".into(), 2., image);
        assert!(thumbs.get(ask("a"), 2.).is_some());
        assert!(thumbs.take_asks().is_empty());
        // A new file: the old picture shows while the new one is drawn.
        assert!(thumbs.get(ask("b"), 2.).is_some());
        assert_eq!(thumbs.take_asks().len(), 1);
        // Another screen: drawn again, and a late picture for the old one is dropped.
        assert!(thumbs.get(ask("b"), 1.).is_none());
        assert_eq!(thumbs.take_asks().len(), 1);
        let late = bake(
            &Subject::Look(Vec::new()),
            &super::super::super::plain(),
            false,
            (4., 4.),
            2.,
        );
        thumbs.put("fox_kit".into(), "b".into(), 2., late);
        assert!(thumbs.get(ask("b"), 1.).is_none());
    }
}
