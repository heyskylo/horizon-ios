//! A mob's baked model: the parts of `entity-models.json` as polygons, and the normal stack that packs their
//! normals the way `ModelPart` does.
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Polygon {
    pub normal: [f32; 3],
    pub vertices: [[f32; 5]; 4],
    /// The model's render type culls back faces (`entityCutoutCull`: the bat,
    /// the baby turtle).
    pub cull_back: bool,
}
impl Polygon {
    pub(super) fn vertex_normal(&self, pose: ModelNormals) -> [i8; 4] {
        let n = pose.matrix * glam::Vec3::from_array(self.normal);
        let n = if pose.trusted {
            n
        } else {
            n.normalize_or_zero()
        };
        let packed = n.to_array().map(|v| (v.clamp(-1.0, 1.0) * 127.0) as i8);
        let w = if self.cull_back {
            crate::mesher::SHADED_CULL_W
        } else {
            -127
        };
        [packed[0], packed[1], packed[2], w]
    }
}

/// ModelPart uses a separate normal stack: uniform scaling keeps normal lengths,
/// while nonuniform scaling requires normalization after the complete part transform.
#[derive(Clone, Copy)]
pub(super) struct ModelNormals {
    pub(super) matrix: glam::Mat3,
    pub(super) trusted: bool,
}
impl ModelNormals {
    // shortcut: infer the root normal pose from its geometry matrix; pass an explicit
    // root normal stack when root-transform rounding is ported as well.
    pub(super) fn new(base: glam::Mat4) -> Self {
        let m = glam::Mat3::from_mat4(base);
        let lengths = glam::Vec3::new(m.x_axis.length(), m.y_axis.length(), m.z_axis.length());
        let uniform =
            lengths.min_element() > 0.0 && lengths.max_element() - lengths.min_element() < 1.0e-6;
        Self {
            matrix: if uniform {
                m / lengths.x
            } else {
                m.inverse().transpose()
            },
            trusted: uniform,
        }
    }
    pub(super) fn rotation(x: f32, y: f32, z: f32) -> glam::Mat3 {
        // JOML derives cosine from the rounded sine, then retains quaternion length.
        // Assuming an exactly unit quaternion changes a packed normal by one byte.
        let sin_cos = |angle: f32| {
            let angle = angle * 0.5;
            let sin = f64::from(angle).sin() as f32;
            let cos = (1.0 - sin * sin).sqrt();
            let a = angle + std::f32::consts::FRAC_PI_2;
            let tau = std::f32::consts::TAU;
            let mut quadrant = a - (a / tau) as i32 as f32 * tau;
            if quadrant < 0.0 {
                quadrant += tau;
            }
            (
                sin,
                if quadrant >= std::f32::consts::PI {
                    -cos
                } else {
                    cos
                },
            )
        };
        let (sx, cx) = sin_cos(x);
        let (sy, cy) = sin_cos(y);
        let (sz, cz) = sin_cos(z);
        let (cycz, sysz, sycz, cysz) = (cy * cz, sy * sz, sy * cz, cy * sz);
        let (w, x, y, z) = (
            cx * cycz + sx * sysz,
            sx * cycz - cx * sysz,
            cx * sycz + sx * cysz,
            cx * cysz - sx * sycz,
        );
        let (ww, xx, yy, zz) = (w * w, x * x, y * y, z * z);
        let (xy, xz, xw, yz, yw, zw) = (x * y, x * z, x * w, y * z, y * w, z * w);
        glam::Mat3::from_cols_array(&[
            ww + xx - zz - yy,
            (xy + xy) + (zw + zw),
            (xz + xz) - (yw + yw),
            (xy + xy) - (zw + zw),
            yy - zz + ww - xx,
            (yz + yz) + (xw + xw),
            (yw + yw) + (xz + xz),
            (yz + yz) - (xw + xw),
            zz - yy - xx + ww,
        ])
    }
    pub(super) fn part(mut self, pose: [f32; 9]) -> Self {
        self.matrix *= Self::rotation(pose[3], pose[4], pose[5]);
        let scale = glam::Vec3::new(pose[6], pose[7], pose[8]);
        if scale.x.abs() == scale.y.abs() && scale.x.abs() == scale.z.abs() {
            if scale.min_element() < 0.0 {
                self.matrix *= glam::Mat3::from_diagonal(scale.signum());
            }
        } else {
            self.matrix *= glam::Mat3::from_diagonal(scale.recip());
            self.trusted = false;
        }
        self
    }
    pub(super) fn part_rotated(mut self, pose: [f32; 9], rotation: glam::Mat3) -> Self {
        self.matrix *= rotation;
        let scale = glam::Vec3::new(pose[6], pose[7], pose[8]);
        if scale.x.abs() == scale.y.abs() && scale.x.abs() == scale.z.abs() {
            if scale.min_element() < 0.0 {
                self.matrix *= glam::Mat3::from_diagonal(scale.signum());
            }
        } else {
            self.matrix *= glam::Mat3::from_diagonal(scale.recip());
            self.trusted = false;
        }
        self
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    /// x, y, z, xRot, yRot, zRot, xScale, yScale, zScale
    pub pose: [f32; 9],
    pub polygons: Vec<Polygon>,
    pub children: Vec<Part>,
}
impl Part {
    /// Marks every polygon as drawn by a back-face-culling render type.
    pub(super) fn set_cull_back(&mut self) {
        for poly in &mut self.polygons {
            poly.cull_back = true;
        }
        for child in &mut self.children {
            child.set_cull_back();
        }
    }
    pub(super) fn parse(name: &str, v: &Value) -> Self {
        let pose: Vec<f32> = v["pose"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| x.as_f64().unwrap_or(0.0) as f32)
            .collect();
        let mut p = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        for (i, x) in pose.iter().enumerate().take(9) {
            p[i] = *x;
        }
        let polygons: Vec<Polygon> = v["polygons"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, poly)| {
                let verts: Vec<[f32; 5]> = poly
                    .as_array()?
                    .iter()
                    .map(|vx| {
                        let a: Vec<f32> = vx
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|x| x.as_f64().unwrap_or(0.0) as f32)
                            .collect();
                        [a[0], a[1], a[2], a[3], a[4]]
                    })
                    .collect();
                (verts.len() == 4).then(|| Polygon {
                    normal: v["normals"][index].as_array().map_or_else(
                        || {
                            // shortcut: collapsed legacy faces have no recoverable normal; re-prepare resources.
                            let p = verts
                                .iter()
                                .map(|v| glam::Vec3::new(v[0], v[1], v[2]))
                                .collect::<Vec<_>>();
                            (p[1] - p[0])
                                .cross(p[2] - p[0])
                                .normalize_or_zero()
                                .to_array()
                        },
                        |n| std::array::from_fn(|i| n[i].as_f64().unwrap_or(0.0) as f32),
                    ),
                    vertices: [verts[0], verts[1], verts[2], verts[3]],
                    cull_back: false,
                })
            })
            .collect();
        let mut children: Vec<Part> = v["children"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, c)| Part::parse(k, c))
            .collect();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        // A model whose root holds a part also named "root" (allay, vex,
        // frog...): that inner part is what `root.getChild("root")` and the
        // keyframe bones mean, so the outer one gives up the name.
        let name = if name == "root" && children.iter().any(|c| c.name == "root") {
            ""
        } else {
            name
        };
        Self {
            name: name.to_owned(),
            pose: p,
            polygons,
            children,
        }
    }
}
