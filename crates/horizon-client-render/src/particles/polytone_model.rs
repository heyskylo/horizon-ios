//! Polytone model particles (`"model"` on a custom particle type, #11486): a particle drawn as
//! the faces of a block model (`assets/<ns>/models/<path>.json`) instead of one sprite quad.
//! Visual Effects+ draws its block glows this way: a glow texture on a plane in front of the
//! block, turned to the camera and blended additively.
//!
//! The model is read the vanilla way (elements, faces, `#texture` references, parents, element
//! rotations) into quads around the model's centre, in blocks; the particle's size scales them
//! and its rotation mode turns them as it turns a sprite quad (`+Z` facing the camera for
//! `look_at_xyz`, world axes for `none`).
use glam::{Quat, Vec3};
use serde_json::{Map, Value as Json};
use std::collections::HashMap;
use std::path::Path;

/// One face of a model particle: corners in blocks around the model's centre, in drawing order,
/// with their texture coordinates inside the sprite (0 to 1).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelQuad {
    pub(crate) corners: [Vec3; 4],
    pub(crate) uv: [[f32; 2]; 4],
    /// The sprite's atlas name (`davidparticles:block/glow_test_1`, `block/stone`).
    pub(crate) sprite: String,
}

/// A model particle's faces.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParticleModel {
    pub(crate) quads: Vec<ModelQuad>,
}

/// `ns:path` with `minecraft` when the namespace is left out.
fn split_id(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("minecraft", id))
}

/// The atlas name of a texture id: `minecraft:block/stone` is `block/stone`.
fn sprite_name(texture: &str) -> String {
    match split_id(texture) {
        ("minecraft", path) => path.to_owned(),
        (namespace, path) => format!("{namespace}:{path}"),
    }
}

/// A model JSON and its parents' `textures` and `elements`, the child's winning.
fn read_model(pack_root: &Path, id: &str, depth: u32) -> Option<(Map<String, Json>, Vec<Json>)> {
    if depth > 8 {
        return None;
    }
    let (namespace, path) = split_id(id);
    let file = pack_root
        .join("assets")
        .join(namespace)
        .join("models")
        .join(format!("{path}.json"));
    let json: Json = horizon_world::json_file::read(&file).ok()?;
    let (mut textures, mut elements) = match json.get("parent").and_then(Json::as_str) {
        Some(parent) => read_model(pack_root, parent, depth + 1).unwrap_or_default(),
        None => Default::default(),
    };
    if let Some(own) = json.get("textures").and_then(Json::as_object) {
        textures.extend(own.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    if let Some(own) = json.get("elements").and_then(Json::as_array) {
        elements.clone_from(own);
    }
    Some((textures, elements))
}

/// A `#name` texture reference followed through the model's textures to an id.
fn resolve_texture(textures: &Map<String, Json>, reference: &str) -> Option<String> {
    let mut name = reference;
    for _ in 0..8 {
        let Some(key) = name.strip_prefix('#') else {
            return Some(name.to_owned());
        };
        name = textures.get(key)?.as_str()?;
    }
    None
}

fn vec3(v: &Json) -> Option<Vec3> {
    let a = v.as_array()?;
    Some(Vec3::new(
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
        a.get(2)?.as_f64()? as f32,
    ))
}

/// A face's corners (in model pixels) in vanilla's order: top left, bottom left, bottom right,
/// top right, as seen from outside the face.
fn face_corners(face: &str, f: Vec3, t: Vec3) -> Option<[Vec3; 4]> {
    let v = Vec3::new;
    Some(match face {
        "south" => [
            v(f.x, t.y, t.z),
            v(f.x, f.y, t.z),
            v(t.x, f.y, t.z),
            v(t.x, t.y, t.z),
        ],
        "north" => [
            v(t.x, t.y, f.z),
            v(t.x, f.y, f.z),
            v(f.x, f.y, f.z),
            v(f.x, t.y, f.z),
        ],
        "east" => [
            v(t.x, t.y, t.z),
            v(t.x, f.y, t.z),
            v(t.x, f.y, f.z),
            v(t.x, t.y, f.z),
        ],
        "west" => [
            v(f.x, t.y, f.z),
            v(f.x, f.y, f.z),
            v(f.x, f.y, t.z),
            v(f.x, t.y, t.z),
        ],
        "up" => [
            v(f.x, t.y, f.z),
            v(f.x, t.y, t.z),
            v(t.x, t.y, t.z),
            v(t.x, t.y, f.z),
        ],
        "down" => [
            v(f.x, f.y, t.z),
            v(f.x, f.y, f.z),
            v(t.x, f.y, f.z),
            v(t.x, f.y, t.z),
        ],
        _ => return None,
    })
}

/// The `uv` a face has when its JSON gives none: the element's extent on that face.
fn default_uv(face: &str, f: Vec3, t: Vec3) -> [f32; 4] {
    match face {
        "north" => [16.0 - t.x, 16.0 - t.y, 16.0 - f.x, 16.0 - f.y],
        "south" => [f.x, 16.0 - t.y, t.x, 16.0 - f.y],
        "east" => [16.0 - t.z, 16.0 - t.y, 16.0 - f.z, 16.0 - f.y],
        "west" => [f.z, 16.0 - t.y, t.z, 16.0 - f.y],
        "up" => [f.x, f.z, t.x, t.z],
        _ => [f.x, 16.0 - t.z, t.x, 16.0 - f.z],
    }
}

/// An element's `rotation` (`angle` degrees around `axis` through `origin`, in pixels).
fn element_rotation(element: &Json) -> Option<(Quat, Vec3)> {
    let r = element.get("rotation")?;
    let angle = (r.get("angle")?.as_f64()? as f32).to_radians();
    if angle == 0.0 {
        return None;
    }
    let axis = match r.get("axis")?.as_str()? {
        "x" => Vec3::X,
        "y" => Vec3::Y,
        _ => Vec3::Z,
    };
    let origin = r.get("origin").and_then(vec3).unwrap_or(Vec3::splat(8.0));
    Some((Quat::from_axis_angle(axis, angle), origin))
}

impl ParticleModel {
    /// The model `id` (`davidparticles:block/fullblock_glow`) under a pack overlay root; `None`
    /// when it's missing or has no face to draw.
    pub(crate) fn read(pack_root: &Path, id: &str) -> Option<Self> {
        let (textures, elements) = read_model(pack_root, id, 0)?;
        let mut quads = Vec::new();
        for element in &elements {
            let (Some(f), Some(t)) = (
                element.get("from").and_then(vec3),
                element.get("to").and_then(vec3),
            ) else {
                continue;
            };
            let rotation = element_rotation(element);
            let faces = element.get("faces").and_then(Json::as_object);
            for (face, spec) in faces.into_iter().flatten() {
                let Some(corners) = face_corners(face, f, t) else {
                    continue;
                };
                // A face with no area (a plane's edges) draws nothing.
                let span = (corners[0] - corners[2]).abs();
                if [span.x, span.y, span.z]
                    .iter()
                    .filter(|&&s| s > 1.0e-4)
                    .count()
                    < 2
                {
                    continue;
                }
                let Some(sprite) = spec
                    .get("texture")
                    .and_then(Json::as_str)
                    .and_then(|r| resolve_texture(&textures, r))
                else {
                    continue;
                };
                let uv = match spec.get("uv").and_then(Json::as_array) {
                    Some(a) if a.len() == 4 => {
                        let n = |i: usize| a[i].as_f64().unwrap_or(0.0) as f32;
                        [n(0), n(1), n(2), n(3)]
                    }
                    _ => default_uv(face, f, t),
                };
                if uv[0] == uv[2] || uv[1] == uv[3] {
                    continue;
                }
                let corners = corners.map(|c| {
                    let c = match rotation {
                        Some((q, origin)) => q * (c - origin) + origin,
                        None => c,
                    };
                    c / 16.0 - Vec3::splat(0.5)
                });
                let [u0, v0, u1, v1] = uv.map(|x| x / 16.0);
                quads.push(ModelQuad {
                    corners,
                    uv: [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
                    sprite: sprite_name(&sprite),
                });
            }
        }
        (!quads.is_empty()).then_some(Self { quads })
    }
}

/// The packs' model particles: each custom particle type with a `model`, its faces.
pub(crate) fn read_models<'a>(
    pack_root: &Path,
    types: impl Iterator<Item = (&'a String, &'a Json)>,
) -> HashMap<String, std::sync::Arc<ParticleModel>> {
    types
        .filter_map(|(id, model)| {
            let model_id = model.as_str()?;
            match ParticleModel::read(pack_root, model_id) {
                Some(m) => Some((id.clone(), std::sync::Arc::new(m))),
                None => {
                    log::warn!("custom particle {id}: model {model_id} not found or empty");
                    None
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str, json: serde_json::Value) {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, json.to_string()).unwrap();
    }

    fn root() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("horizon-particle-model-{}-{n}", std::process::id()))
    }

    #[test]
    fn a_glow_plane_keeps_its_one_face_in_front_of_the_centre() {
        let root = root();
        // Visual Effects+'s glow, as Blockbench writes it: a plane at z 18.25 whose edges have
        // no area, and faces without a texture.
        write(
            &root,
            "assets/fx/models/block/glow.json",
            serde_json::json!({
                "textures": {"2": "fx:block/glow", "particle": "fx:block/glow"},
                "elements": [{
                    "from": [1.25, 1.25, 18.25], "to": [14.75, 14.75, 18.25],
                    "faces": {
                        "north": {"uv": [0, 0, 0, 0], "texture": "#2"},
                        "east": {"uv": [0, 0, 0, 4], "texture": "#missing"},
                        "south": {"uv": [0, 0, 16, 16], "texture": "#2"}
                    }
                }]
            }),
        );
        let model = ParticleModel::read(&root, "fx:block/glow").unwrap();
        assert_eq!(model.quads.len(), 1);
        let quad = &model.quads[0];
        assert_eq!(quad.sprite, "fx:block/glow");
        // 18.25 px is 0.640625 blocks in front of the centre; the plane spans ±0.421875.
        let z = 18.25 / 16.0 - 0.5;
        let e = 6.75 / 16.0;
        assert_eq!(
            quad.corners,
            [
                Vec3::new(-e, e, z),
                Vec3::new(-e, -e, z),
                Vec3::new(e, -e, z),
                Vec3::new(e, e, z)
            ]
        );
        assert_eq!(quad.uv, [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parents_textures_and_default_uvs_resolve_as_vanilla() {
        let root = root();
        write(
            &root,
            "assets/minecraft/models/block/base.json",
            serde_json::json!({
                "textures": {"all": "block/stone"},
                "elements": [{"from": [0, 0, 0], "to": [16, 8, 16], "faces": {"up": {"texture": "#all"}}}]
            }),
        );
        write(
            &root,
            "assets/fx/models/slab.json",
            serde_json::json!({
                "parent": "block/base", "textures": {"all": "fx:block/sand"}
            }),
        );
        let model = ParticleModel::read(&root, "fx:slab").unwrap();
        assert_eq!(model.quads.len(), 1);
        // The child's texture wins; `up` at y 8 px, seen from above with north on top.
        assert_eq!(model.quads[0].sprite, "fx:block/sand");
        assert_eq!(model.quads[0].corners[0], Vec3::new(-0.5, 0.0, -0.5));
        assert_eq!(model.quads[0].corners[2], Vec3::new(0.5, 0.0, 0.5));
        assert_eq!(model.quads[0].uv[2], [1.0, 1.0]);
        assert!(ParticleModel::read(&root, "fx:missing").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
