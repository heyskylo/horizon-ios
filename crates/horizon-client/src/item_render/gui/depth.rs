//! Back-to-front ordering for the orthographic GUI item view (near is +Z).
//! Whole-face average depths fail when a long, tilted face overlaps a short
//! one, such as a banner's cloth and support. Partition faces by their planes
//! instead, splitting intersecting faces with their UVs. Coplanar faces keep
//! submission order so banner/shield pattern layers still paint bottom first.
use super::super::ItemQuad;
use glam::{Vec3, Vec4};

#[derive(Clone, Copy)]
pub(super) struct Corner {
    pub pos: Vec3,
    pub uv: [f32; 2],
    pub glint_uv: [f32; 2],
    pub cit_uv: [f32; 2],
}

pub(super) struct Face {
    pub quad: usize,
    pub corners: Vec<Corner>,
}

const EPSILON: f32 = 1e-4;

fn split(face: Face, distances: &[f32]) -> (Face, Face) {
    let mut back = Vec::new();
    let mut front = Vec::new();
    for i in 0..face.corners.len() {
        let a = face.corners[i];
        let b = face.corners[(i + 1) % face.corners.len()];
        let da = distances[i];
        let db = distances[(i + 1) % face.corners.len()];
        if da <= EPSILON {
            back.push(a);
        }
        if da >= -EPSILON {
            front.push(a);
        }
        if (da < -EPSILON && db > EPSILON) || (da > EPSILON && db < -EPSILON) {
            let t = da / (da - db);
            let corner = Corner {
                pos: a.pos.lerp(b.pos, t),
                uv: std::array::from_fn(|k| a.uv[k] + (b.uv[k] - a.uv[k]) * t),
                cit_uv: std::array::from_fn(|k| a.cit_uv[k] + (b.cit_uv[k] - a.cit_uv[k]) * t),
                glint_uv: std::array::from_fn(|k| {
                    a.glint_uv[k] + (b.glint_uv[k] - a.glint_uv[k]) * t
                }),
            };
            back.push(corner);
            front.push(corner);
        }
    }
    (
        Face {
            quad: face.quad,
            corners: back,
        },
        Face {
            quad: face.quad,
            corners: front,
        },
    )
}

pub(super) fn sort(quads: &[ItemQuad]) -> Vec<Face> {
    let mut planes = vec![Vec4::ZERO; quads.len()];
    let faces: Vec<_> = quads
        .iter()
        .enumerate()
        .filter_map(|(i, q)| {
            let normal = (q.pos[2] - q.pos[0]).cross(q.pos[3] - q.pos[1]);
            // Front faces wind counter-clockwise on screen with Y up.
            if normal.z >= 0.0 {
                return None;
            }
            let normal = -normal.normalize();
            planes[i] = normal.extend(-normal.dot(q.pos[0]));
            Some(Face {
                quad: i,
                corners: std::array::from_fn::<_, 4, _>(|k| Corner {
                    pos: q.pos[k],
                    uv: q.uv[k],
                    glint_uv: q.gui_glint_uv.unwrap_or(q.foil_uv)[k],
                    cit_uv: q.foil_uv[k],
                })
                .to_vec(),
            })
        })
        .collect();
    let mut out = Vec::new();
    // An explicit stack avoids recursion for models with many parallel faces.
    let mut pending = vec![(faces, false)];
    while let Some((faces, emit)) = pending.pop() {
        if emit {
            out.extend(faces);
            continue;
        }
        let mut faces = faces.into_iter();
        let Some(first) = faces.next() else { continue };
        let plane = planes[first.quad];
        let mut coplanar = vec![first];
        let mut back = Vec::new();
        let mut front = Vec::new();
        for face in faces {
            let distances: Vec<_> = face
                .corners
                .iter()
                .map(|c| plane.dot(c.pos.extend(1.0)))
                .collect();
            let behind = distances.iter().any(|d| *d < -EPSILON);
            let ahead = distances.iter().any(|d| *d > EPSILON);
            match (behind, ahead) {
                (false, false) => coplanar.push(face),
                (true, false) => back.push(face),
                (false, true) => front.push(face),
                (true, true) => {
                    let (b, f) = split(face, &distances);
                    back.push(b);
                    front.push(f);
                }
            }
        }
        pending.push((front, false));
        pending.push((coplanar, true));
        pending.push((back, false));
    }
    out
}
