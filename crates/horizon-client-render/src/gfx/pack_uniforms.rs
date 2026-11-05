// `invert_generic`, `invert_affine` and the formulas of `pack_projection` and `shadow_ortho` in
// this file are adapted from JOML 1.10.9's `Matrix4f` (https://github.com/JOML-CI/JOML, MIT; see
// THIRD_PARTY_NOTICES.md).
//
// Copyright (c) 2015-2026 Richard Greenlees
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! The uniforms a shader pack's post-processing programs read once per frame (spec 5.2, class
//! F), from the renderer's `Frame` and the few values only the game knows (`PackInputs`).
//! Matrices follow GL's conventions (5.5): `gbufferProjection` maps view depth to clip z in
//! [-w, w], and the depth buffers packs read hold GL window depth.

use glam::{DMat4, DVec3, Mat4, Vec3, Vec4};
use horizon_shaders::Shadow;

use super::Frame;

/// What the game passes for the pack's uniforms, beyond what `Frame` carries.
#[derive(Clone, Copy, Debug, Default)]
pub struct PackInputs {
    /// `Level.getDayTime`.
    pub day_time: i64,
    /// The interpolated rain and thunder levels, 0..1.
    pub rain: f32,
    pub thunder: f32,
    /// The fluid at the camera: 0 none, 1 water, 2 lava, 3 powder snow.
    pub eye_in: i32,
    pub blindness: f32,
    /// Block and sky light at the camera, 0..15.
    pub eye_light: [i32; 2],
    pub hide_gui: bool,
    /// The brightness option, 0..1.
    pub brightness: f32,
    /// The biome at the camera (3.4): registry id, `CAT_*`, `PPT_*`, temperature, downfall.
    pub biome: i32,
    pub biome_category: i32,
    pub biome_precipitation: i32,
    pub temperature: f32,
    pub rainfall: f32,
    /// Whether the biome at the camera is known (its chunk is loaded).
    pub biome_known: bool,
    /// `heldItemId`, `heldItemId2` (item.properties, 5.3) and `heldBlockLightValue(2)`.
    pub held_items: [i32; 2],
    pub held_light: [i32; 2],
    /// `isRightHanded` (the Main Hand option) and `is_on_ground`.
    pub right_handed: bool,
    pub on_ground: bool,
    /// The dimension type's lowest block (`bedrockLevel`), build height (`heightLimit`),
    /// logical height, sky light and ceiling.
    pub min_y: i32,
    pub height: i32,
    pub logical_height: i32,
    pub has_skylight: bool,
    pub has_ceiling: bool,
    /// `isElytraFlying`: the player is fall flying.
    pub elytra_flying: bool,
    /// `cloudHeight`, see [`cloud_height`].
    pub cloud_height: f32,
    /// `cloudTime`: how far the game's clouds have moved along X, in blocks (`clouds::drift`
    /// times vanilla's 0.03 blocks a tick). Iris 1.11.7 gives it, though ShaderDoc doesn't list
    /// it: observed with a probe pack, three game times 0.03 apart per tick.
    pub cloud_time: f32,
    /// The values native mods hand their packs (`frame.mods`, `native_shader_params`).
    pub mod_params: [[f32; 4]; 8],
    /// The player's state, for Iris's status uniforms.
    pub status: PlayerStatus,
    /// The End flash: its intensity, 0..1 (0 outside the End), and its direction from the
    /// camera in world space, of length 100 (`endFlashIntensity`, `endFlashPosition`).
    pub end_flash: f32,
    pub end_flash_direction: Vec3,
    /// The dimension type's `ambient_light` (`ambientLight`).
    pub ambient_light: f32,
    /// The player's cave mood, 0..1 (`BiomeAmbientSoundsHandler`'s moodiness):
    /// `playerMood` and `constantMood`.
    pub mood: f32,
    /// Under `HORIZON_STILL`, the world's ticks: the pack's clock (`frameTimeCounter`,
    /// `frameCounter`) instead of the wall clock and the frames drawn, and smoothed values
    /// (`wetness`, `eyeBrightnessSmooth`, `centerDepthSmooth`) at their targets.
    pub still_ticks: Option<u64>,
}

/// The `cloudHeight` uniform. Vanilla reads `visual/cloud_height` only while the cloud color
/// has alpha (`LevelExtractor`); a dimension without clouds has no height, which the uniform
/// reports as NaN. NaN is not vanilla's own value: packs map it to "no clouds".
pub fn cloud_height(cloud_color_alpha: f32, height: f32) -> f32 {
    if crate::clouds::has_color(cloud_color_alpha) {
        height
    } else {
        f32::NAN
    }
}

/// The local player's state behind Iris's status uniforms (`currentPlayerHealth`,
/// `isSpectator`, `is_burning`...).
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerStatus {
    /// Survival or adventure: the vitals are -1 in creative and spectator.
    pub survival: bool,
    pub health: f32,
    pub max_health: f32,
    pub food: f32,
    /// Air supply ticks, of 300.
    pub air: f32,
    /// Armor points (the `armor` attribute).
    pub armor: f32,
    pub spectator: bool,
    pub riding: bool,
    pub feet_in_water: bool,
    pub swimming: bool,
    pub burning: bool,
    pub hurt: bool,
    pub invisible: bool,
    pub sneaking: bool,
    pub sprinting: bool,
}

/// `maxPlayerHunger`, `maxPlayerAir` and `maxPlayerArmor`: Minecraft's own maximums.
const MAX_HUNGER: f32 = 20.0;
const MAX_AIR: f32 = 300.0;
const MAX_ARMOR: f32 = 50.0;

/// One uniform's value: up to 16 floats or 3 integers.
#[derive(Clone, Copy, Debug)]
pub enum Value {
    F([f32; 16]),
    I([i32; 3]),
}

impl Value {
    fn f(v: &[f32]) -> Self {
        let mut a = [0.0; 16];
        a[..v.len()].copy_from_slice(v);
        Value::F(a)
    }
    fn i(v: &[i32]) -> Self {
        let mut a = [0; 3];
        a[..v.len()].copy_from_slice(v);
        Value::I(a)
    }

    /// Whether a program that declares the built-in `name` as `ty` reads this value. Iris 1.11.7
    /// disables a built-in declared with another GLSL type than the one it gives ("Wrong uniform
    /// type ... Disabling that uniform"), and the program reads 0. Our probe packs saw it for a
    /// float declared `int` or `vec2`, an int `float`, an `ivec2` `vec2`, `int` or `ivec3`, a
    /// `vec3` `vec2` or `vec4`, a `mat4` `mat3`, and any `uint`; an int or a bool built-in
    /// declared `bool` or `int` was kept. A float declared `bool` is left as it was: Iris warned,
    /// but our probe read true.
    pub fn read_as(&self, name: &str, ty: &str) -> bool {
        let int = matches!(self, Value::I(_));
        let (declared_int, size) = match ty {
            "float" => (false, 1),
            "bool" if !int => return true,
            "int" | "bool" => (true, 1),
            "mat2" => (false, 4),
            "mat3" => (false, 9),
            "mat4" => (false, 16),
            t if t.starts_with('u') => return false,
            t if let Some(n) = t.strip_prefix("ivec") => (true, n.parse().unwrap_or(0)),
            t if let Some(n) = t.strip_prefix("vec") => (false, n.parse().unwrap_or(0)),
            _ => return true,
        };
        let name = name.strip_prefix("gbuffers.").unwrap_or(name);
        declared_int == int && size == components(name)
    }

    /// Writes the value as the type the pack declared, at `offset` of a std140 block.
    pub fn write(&self, ty: &str, out: &mut [u8], offset: usize) {
        let ints = matches!(ty, "int" | "bool" | "uint") || ty.starts_with(['i', 'u']);
        let (floats, integers): ([f32; 16], [i32; 16]) = match *self {
            Value::F(f) => (f, f.map(|v| v as i32)),
            Value::I(i) => {
                let mut f = [0.0; 16];
                let mut v = [0; 16];
                for k in 0..3 {
                    f[k] = i[k] as f32;
                    v[k] = i[k];
                }
                (f, v)
            }
        };
        let count = match ty {
            "mat4" => 16,
            "mat3" => {
                // std140: three columns, each padded to a vec4, from the mat4's upper left.
                for c in 0..3 {
                    let column: [f32; 3] = std::array::from_fn(|r| floats[c * 4 + r]);
                    put(out, offset + c * 16, bytemuck::cast_slice(&column));
                }
                return;
            }
            t => match t.as_bytes().last() {
                Some(n @ b'2'..=b'4') if t.contains("vec") => (n - b'0') as usize,
                _ => 1,
            },
        };
        if ints {
            put(out, offset, bytemuck::cast_slice(&integers[..count]));
        } else {
            put(out, offset, bytemuck::cast_slice(&floats[..count]));
        }
    }
}

fn put(out: &mut [u8], offset: usize, bytes: &[u8]) {
    if let Some(dst) = out.get_mut(offset..offset + bytes.len()) {
        dst.copy_from_slice(bytes);
    }
}

/// The uniforms provided, in the order of [`FrameUniforms::values`].
pub const NAMES: &[&str] = &[
    "cameraPosition",
    "previousCameraPosition",
    "eyeAltitude",
    "upPosition",
    "sunPosition",
    "moonPosition",
    "shadowLightPosition",
    "sunAngle",
    "shadowAngle",
    "worldTime",
    "worldDay",
    "moonPhase",
    "frameCounter",
    "frameTime",
    "frameTimeCounter",
    "viewWidth",
    "viewHeight",
    "aspectRatio",
    "near",
    "far",
    "rainStrength",
    "wetness",
    "thunderStrength",
    "fogColor",
    "skyColor",
    "fogMode",
    "fogStart",
    "fogEnd",
    "fogDensity",
    "fogShape",
    "isEyeInWater",
    "blindness",
    "nightVision",
    "darknessFactor",
    "darknessLightFactor",
    "screenBrightness",
    "hideGUI",
    "eyeBrightness",
    "eyeBrightnessSmooth",
    "heldItemId",
    "heldItemId2",
    "heldBlockLightValue",
    "heldBlockLightValue2",
    "renderStage",
    "biome",
    "biome_category",
    "biome_precipitation",
    "temperature",
    "rainfall",
    "gbufferModelView",
    "gbufferModelViewInverse",
    "gbufferProjection",
    "gbufferProjectionInverse",
    "gbufferPreviousModelView",
    "gbufferPreviousProjection",
    "atlasSize",
    "shadowModelView",
    "shadowModelViewInverse",
    "shadowProjection",
    "shadowProjectionInverse",
    "cameraPositionInt",
    "cameraPositionFract",
    "previousCameraPositionInt",
    "previousCameraPositionFract",
    "eyePosition",
    "relativeEyePosition",
    "playerLookVector",
    "playerBodyVector",
    "firstPersonCamera",
    "currentPlayerHealth",
    "maxPlayerHealth",
    "currentPlayerHunger",
    "maxPlayerHunger",
    "currentPlayerAir",
    "maxPlayerAir",
    "currentPlayerArmor",
    "maxPlayerArmor",
    "isSpectator",
    "isRiding",
    "feetInWater",
    "inSwimmingAnimation",
    "is_burning",
    "is_hurt",
    "is_invisible",
    "is_sneaking",
    "is_sprinting",
    "endFlashIntensity",
    "endFlashPosition",
    "ambientLight",
    "playerMood",
    "constantMood",
    "centerDepthSmooth",
    "alphaTestRef",
    "entityId",
    "blockEntityId",
    "entityColor",
    GBUFFERS_FOG_MODE,
    GBUFFERS_FOG_SHAPE,
    GBUFFERS_CENTER_DEPTH,
    "isRightHanded",
    "is_on_ground",
    "bedrockLevel",
    "heightLimit",
    "logicalHeightLimit",
    "hasSkylight",
    "hasCeiling",
    "cloudTime",
    "isElytraFlying",
    "cloudHeight",
];

/// What programs that draw the game's geometry (gbuffers, shadow) read as `fogMode`,
/// `fogShape` and `centerDepthSmooth`: no GLSL name can hold a dot, so no pack reads these
/// directly.
const GBUFFERS_FOG_MODE: &str = "gbuffers.fogMode";
const GBUFFERS_FOG_SHAPE: &str = "gbuffers.fogShape";
const GBUFFERS_CENTER_DEPTH: &str = "gbuffers.centerDepthSmooth";

/// Values carried from frame to frame.
pub struct FrameUniforms {
    pub values: Vec<Value>,
    /// The block atlas's size (`atlasSize`), 0 without one.
    pub atlas: [u32; 2],
    start: std::time::Instant,
    last: Option<std::time::Instant>,
    frame: u32,
    /// The last frame's `PackInputs::still_ticks`, for `eval_custom`.
    still_ticks: Option<u64>,
    previous: Option<(glam::DVec3, Mat4, Mat4)>,
    wetness: f32,
    eye_smooth: [f32; 2],
    /// `depthtex0` at the screen's centre as last read back, and `centerDepthSmooth`.
    pub center_depth_raw: Option<f32>,
    center_depth: Option<f32>,
    /// The pack's shadow settings (7.1).
    pub shadow: Shadow,
}

impl Default for FrameUniforms {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            atlas: [0; 2],
            start: std::time::Instant::now(),
            last: None,
            frame: 0,
            still_ticks: None,
            previous: None,
            wetness: 0.0,
            eye_smooth: [0.0; 2],
            center_depth_raw: None,
            center_depth: None,
            shadow: Shadow::default(),
        }
    }
}

/// `(0, 100, 0)` on the frame the sky draws a body on: Y -90°, then X by its angle in degrees;
/// `sunPathRotation` (`tilt`, degrees) then turns the path about the world's X axis (7.1, OQ-6).
fn body(angle: f32, tilt: f32) -> Vec3 {
    (tilt_matrix(tilt)
        * Mat4::from_rotation_y((-90.0f32).to_radians())
        * Mat4::from_rotation_x(angle.to_radians()))
    .transform_vector3(Vec3::new(0.0, 100.0, 0.0))
}

/// `sunPathRotation`'s turn of the sky's bodies: `tilt` degrees about the world's X axis, so a
/// positive angle moves the noon sun toward -Z (north), as Iris does.
pub fn tilt_matrix(tilt: f32) -> Mat4 {
    Mat4::from_rotation_x(-tilt.to_radians())
}

/// The world direction of the shadow light (7.1): the sun by day, the moon by night.
pub fn shadow_light(frame: &Frame, shadow: &Shadow) -> Vec3 {
    let sun_angle = (frame.sky.sun_angle / 360.0 + 0.25).rem_euclid(1.0);
    let angle = if sun_angle < 0.5 {
        frame.sky.sun_angle
    } else {
        frame.sky.moon_angle
    };
    body(angle, shadow.sun_path_rotation).normalize()
}

/// A sky angle in degrees as the fraction of a day Iris 1.11.7 gives packs (`sunAngle`,
/// `shadowAngle`): 0 at sunrise, 0.25 at noon. Our probes' readings fit `((angle + 90) % 360)
/// / 360` in floats to the last bit, by day and by night.
fn day_fraction(degrees: f32) -> f32 {
    (degrees + 90.0) % 360.0 / 360.0
}

/// `shadowAngle`: the day fraction of the body that casts the shadows, the sun by day and the
/// moon (taken below 360) at night.
fn shadow_angle(frame: &Frame) -> f32 {
    let sun = day_fraction(frame.sky.sun_angle);
    if sun < 0.5 {
        sun
    } else {
        day_fraction(frame.sky.moon_angle % 360.0)
    }
}

/// The angle in degrees `shadowModelView` turns the light's path by, from `shadowAngle`:
/// `(shadowAngle - 0.25) * 360` in floats, plus 360 when that is negative. Our probes' readings
/// of Iris 1.11.7 (50: 25 ticks of day and night, among them 7295 to 7305 one by one, with
/// `sunPathRotation` 0 and 30) fit this to the last bit; the body's own sky angle, which the
/// fraction is made from, misses 16 of them by 1 to 6 ulps (#8464).
fn shadow_matrix_degrees(frame: &Frame) -> f32 {
    let degrees = (shadow_angle(frame) - 0.25) * 360.0;
    if degrees < 0.0 {
        degrees + 360.0
    } else {
        degrees
    }
}

/// `shadowModelView` and its inverse to the last bit, as Iris 1.11.7 gives them: our probes'
/// readings (times of day and night, `sunPathRotation` 0 and 30, cameras off the block grid)
/// fit these JOML float operations, in this order: `rotateX(90°)`, `rotateZ(-angle)` with
/// `shadow_matrix_degrees`, `rotateX(sunPathRotation)`, then `translate` by minus the origin
/// `shadow_camera` describes, from the camera's position as a float; and JOML's `invert()`.
/// `shadow_camera`'s view, built in doubles, differs in the last bits (6e-17 for 0), and on a
/// texel boundary of the map that is enough to read the other texel (#8464).
pub fn shadow_model_view(frame: &Frame, shadow: &Shadow) -> (Mat4, Mat4) {
    use horizon_core::geometry::trig::DEG_TO_RAD;
    let mut m = crate::camera::joml::Mat4::IDENTITY;
    m.rotate_x(90.0 * DEG_TO_RAD);
    m.rotate_z(-(shadow_matrix_degrees(frame) * DEG_TO_RAD));
    m.rotate_x(shadow.sun_path_rotation * DEG_TO_RAD);
    let step = shadow.interval;
    if step > 0.0 {
        let camera = frame.camera.as_vec3();
        let remainder = if shadow.euclidean_snap {
            camera.rem_euclid(Vec3::splat(step))
        } else {
            camera % step
        };
        let origin = Vec3::splat(step / 2.0) - remainder;
        m.translate(-origin.x, -origin.y, -origin.z);
    }
    let inverse = m
        .invert_orthonormal()
        .expect("rotations and a translation are orthonormal");
    (m.to_glam(), inverse.to_glam())
}

/// `gbufferProjection` and its inverse to the last bit, as Iris 1.11.7 gives them: our probes'
/// readings fit JOML's `setPerspective` formula worked in doubles from the float field of view
/// in radians, the float aspect ratio and the near plane 0.05 as a float, narrowed to floats;
/// and JOML's general `invert()` of that, in floats. glam's perspective differs in the last
/// bits.
pub fn pack_projection(fov_degrees: f32, aspect: f32, far: f32) -> (Mat4, Mat4) {
    use horizon_core::geometry::trig::DEG_TO_RAD;
    let fov = f64::from(fov_degrees * DEG_TO_RAD);
    let (aspect, near, far) = (f64::from(aspect), f64::from(0.05f32), f64::from(far));
    let h = (fov * 0.5).tan();
    let m = glam::DMat4::from_cols(
        glam::DVec4::new(1.0 / (h * aspect), 0.0, 0.0, 0.0),
        glam::DVec4::new(0.0, 1.0 / h, 0.0, 0.0),
        glam::DVec4::new(0.0, 0.0, (far + near) / (near - far), -1.0),
        glam::DVec4::new(0.0, 0.0, (far + far) * near / (near - far), 0.0),
    )
    .as_mat4();
    (m, invert_generic(m))
}

/// JOML's `Matrix4f.invert()` of a matrix with no known shape (`invertGeneric`), operation by
/// operation (JOML's `Math.fma` without `joml.useMathFma` is a product and a sum).
fn invert_generic(m: Mat4) -> Mat4 {
    let g = |c: usize, r: usize| m.col(c)[r];
    let (m00, m01, m02, m03) = (g(0, 0), g(0, 1), g(0, 2), g(0, 3));
    let (m10, m11, m12, m13) = (g(1, 0), g(1, 1), g(1, 2), g(1, 3));
    let (m20, m21, m22, m23) = (g(2, 0), g(2, 1), g(2, 2), g(2, 3));
    let (m30, m31, m32, m33) = (g(3, 0), g(3, 1), g(3, 2), g(3, 3));
    let a = m00 * m11 - m01 * m10;
    let b = m00 * m12 - m02 * m10;
    let c = m00 * m13 - m03 * m10;
    let d = m01 * m12 - m02 * m11;
    let e = m01 * m13 - m03 * m11;
    let f = m02 * m13 - m03 * m12;
    let gg = m20 * m31 - m21 * m30;
    let h = m20 * m32 - m22 * m30;
    let i = m20 * m33 - m23 * m30;
    let j = m21 * m32 - m22 * m31;
    let k = m21 * m33 - m23 * m31;
    let l = m22 * m33 - m23 * m32;
    let det = 1.0 / (a * l - b * k + c * j + d * i - e * h + f * gg);
    // `Math.fma(x, y, z)` unfused.
    let fma = |x: f32, y: f32, z: f32| x * y + z;
    Mat4::from_cols_array(&[
        fma(m11, l, fma(-m12, k, m13 * j)) * det,
        fma(-m01, l, fma(m02, k, -m03 * j)) * det,
        fma(m31, f, fma(-m32, e, m33 * d)) * det,
        fma(-m21, f, fma(m22, e, -m23 * d)) * det,
        fma(-m10, l, fma(m12, i, -m13 * h)) * det,
        fma(m00, l, fma(-m02, i, m03 * h)) * det,
        fma(-m30, f, fma(m32, c, -m33 * b)) * det,
        fma(m20, f, fma(-m22, c, m23 * b)) * det,
        fma(m10, k, fma(-m11, i, m13 * gg)) * det,
        fma(-m00, k, fma(m01, i, -m03 * gg)) * det,
        fma(m30, e, fma(-m31, c, m33 * a)) * det,
        fma(-m20, e, fma(m21, c, -m23 * a)) * det,
        fma(-m10, j, fma(m11, h, -m12 * gg)) * det,
        fma(m00, j, fma(-m01, h, m02 * gg)) * det,
        fma(-m30, d, fma(m31, b, -m32 * a)) * det,
        fma(m20, d, fma(-m21, b, m22 * a)) * det,
    ])
}

/// `shadow_camera`'s view and projection in wgpu's clip space (depth 0..1), for culling.
pub fn shadow_culling(frame: &Frame, shadow: &Shadow) -> Mat4 {
    let (view, gl) = shadow_camera(shadow_light(frame, shadow), frame.camera, shadow);
    // GL's depth -w..w to wgpu's 0..w.
    let depth = Mat4::from_cols(
        Vec4::X,
        Vec4::Y,
        Vec4::new(0.0, 0.0, 0.5, 0.0),
        Vec4::new(0.0, 0.0, 0.5, 1.0),
    );
    depth * gl * view
}

/// How far toward the light a `shadowMapFov` projection starts, in blocks.
const SHADOW_EYE: f32 = 100.0;

/// The orthographic shadow projection's planes when the pack sets none, as JOML's `zNear` and
/// `zFar`: Iris 1.11.7's, measured with a probe pack (each unset plane on its own).
const SHADOW_NEAR: f32 = -100.05;
const SHADOW_FAR: f32 = 156.0;

/// `shadowProjection` and its inverse to the last bit, as Iris 1.11.7 gives them: our probes'
/// readings (13 settings of `shadowDistance`, `shadowNearPlane` and `shadowFarPlane`, each
/// plane set or not) fit JOML's `setOrtho` over `shadowDistance` each side with `zZeroToOne`,
/// its depth terms then taken to GL's -1..1 (`m22 * 2`, `m32 * 2 - 1`), in floats; and JOML's
/// `invertAffine` of that. The planes are the pack's, or `SHADOW_NEAR`/`SHADOW_FAR`: a pack
/// that sets one gets it as it is, with no 100-block offset (#8464).
fn shadow_ortho(shadow: &Shadow) -> (Mat4, Mat4) {
    let d = shadow.distance;
    let near = shadow.near.unwrap_or(SHADOW_NEAR);
    let far = shadow.far.unwrap_or(SHADOW_FAR);
    let side = 2.0 / (d - -d);
    let projection = Mat4::from_cols(
        Vec4::new(side, 0.0, 0.0, 0.0),
        Vec4::new(0.0, side, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0 / (near - far) * 2.0, 0.0),
        Vec4::new(0.0, 0.0, near / (near - far) * 2.0 - 1.0, 1.0),
    );
    (projection, invert_affine(projection))
}

/// JOML's `Matrix4f.invertAffine()`, operation by operation.
fn invert_affine(m: Mat4) -> Mat4 {
    let g = |c: usize, r: usize| m.col(c)[r];
    let (m00, m01, m02) = (g(0, 0), g(0, 1), g(0, 2));
    let (m10, m11, m12) = (g(1, 0), g(1, 1), g(1, 2));
    let (m20, m21, m22) = (g(2, 0), g(2, 1), g(2, 2));
    let (m30, m31, m32) = (g(3, 0), g(3, 1), g(3, 2));
    let (m11m00, m10m01, m10m02) = (m00 * m11, m01 * m10, m02 * m10);
    let (m12m00, m12m01, m11m02) = (m00 * m12, m01 * m12, m02 * m11);
    let det = (m11m00 - m10m01) * m22 + (m10m02 - m12m00) * m21 + (m12m01 - m11m02) * m20;
    let s = 1.0 / det;
    let (m10m22, m10m21, m11m22) = (m10 * m22, m10 * m21, m11 * m22);
    let (m11m20, m12m21, m12m20) = (m11 * m20, m12 * m21, m12 * m20);
    let (m20m02, m20m01, m21m02) = (m20 * m02, m20 * m01, m21 * m02);
    let (m21m00, m22m01, m22m00) = (m21 * m00, m22 * m01, m22 * m00);
    Mat4::from_cols_array(&[
        (m11m22 - m12m21) * s,
        (m21m02 - m22m01) * s,
        (m12m01 - m11m02) * s,
        0.0,
        (m12m20 - m10m22) * s,
        (m22m00 - m20m02) * s,
        (m10m02 - m12m00) * s,
        0.0,
        (m10m21 - m11m20) * s,
        (m20m01 - m21m00) * s,
        (m11m00 - m10m01) * s,
        0.0,
        (m10m22 * m31 - m10m21 * m32 + m11m20 * m32 - m11m22 * m30 + m12m21 * m30 - m12m20 * m31)
            * s,
        (m20m02 * m31 - m20m01 * m32 + m21m00 * m32 - m21m02 * m30 + m22m01 * m30 - m22m00 * m31)
            * s,
        (m11m02 * m30 - m12m01 * m30 + m12m00 * m31 - m10m02 * m31 + m10m01 * m32 - m11m00 * m32)
            * s,
        1.0,
    ])
}

/// The shadow camera as Iris places it (7.1; measured from the outside): its view (player
/// space to shadow view space) and GL projection. The view undoes `sunPathRotation`, turns the
/// light's path about Z by the light's angle from the zenith, then X by 90 degrees, so the
/// light lies on the view's +Z. Its origin is `shadowIntervalSize / 2 - camera % interval`
/// (the remainder takes the camera's sign) from the player on each world axis, so the map moves
/// in whole steps. The orthographic projection is `shadow_ortho`'s; or perspective with
/// `shadowMapFov`, from `shadowNearPlane` (0.05) to `shadowFarPlane` (256) counted from
/// `SHADOW_EYE` blocks toward the light.
pub fn shadow_camera(light: Vec3, camera: DVec3, shadow: &Shadow) -> (Mat4, Mat4) {
    let tilt = f64::from(shadow.sun_path_rotation).to_radians();
    let untilted = DMat4::from_rotation_x(tilt).transform_vector3(light.as_dvec3());
    let zenith = untilted.x.atan2(untilted.y);
    let rotation = DMat4::from_rotation_x(std::f64::consts::FRAC_PI_2)
        * DMat4::from_rotation_z(zenith)
        * DMat4::from_rotation_x(tilt);
    let step = f64::from(shadow.interval);
    let origin = if step > 0.0 {
        let remainder = if shadow.euclidean_snap {
            camera.rem_euclid(DVec3::splat(step))
        } else {
            camera % step
        };
        DVec3::splat(step / 2.0) - remainder
    } else {
        DVec3::ZERO
    };
    let view = (rotation * DMat4::from_translation(-origin)).as_mat4();
    let projection = match shadow.fov {
        Some(fov) => {
            let (near, far) = (shadow.near.unwrap_or(0.05), shadow.far.unwrap_or(256.0));
            let eye = Mat4::from_translation(Vec3::new(0.0, 0.0, -SHADOW_EYE));
            Mat4::perspective_rh_gl(fov.to_radians(), 1.0, near, far) * eye
        }
        None => shadow_ortho(shadow).0,
    };
    (view, projection)
}

/// The seconds a tick of a smoothing half-life (`wetnessHalflife`, `eyeBrightnessHalflife`,
/// `centerDepthHalflife`) lasts in Iris 1.11.7: measured, twice a game tick.
const SECONDS_A_TICK: f32 = 0.1;

/// `value` moved toward `target` with a half-life of `half_life` seconds over `dt` (5.6).
fn smooth(value: f32, target: f32, half_life: f32, dt: f32) -> f32 {
    target + (value - target) * 0.5f32.powf(dt / half_life)
}

impl FrameUniforms {
    /// This frame's values for a render size of `size` pixels.
    pub fn update(&mut self, frame: &Frame, inputs: &PackInputs, size: [u32; 2]) {
        let now = std::time::Instant::now();
        let measured = self.last.map_or(0.0, |l| (now - l).as_secs_f32());
        self.last = Some(now);
        self.still_ticks = inputs.still_ticks;
        // Smoothing over an endless step lands on its target.
        let dt = inputs.still_ticks.map_or(measured, |_| f32::INFINITY);
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        // Packs take gbufferProjection for a plain perspective (they invert only its perspective
        // terms): the view bob and the nausea spin belong to the model-view, as GL drew them.
        // Vanilla's view rotation to the last bit (Iris 1.11.7's gbufferModelView and its inverse
        // match it bit for bit in our probes): fragments on a texel boundary of the shadow map
        // then read the same texel as in Iris (#8464).
        let (rotation, rotation_inverse) =
            crate::camera::view_rotation_matrix(frame.yaw, frame.pitch);
        let (view, view_inverse) = if frame.bob == Mat4::IDENTITY && frame.spin == Mat4::IDENTITY {
            (rotation, rotation_inverse)
        } else {
            let view = frame.bob * frame.spin * rotation;
            (view, view.inverse())
        };
        let (projection, projection_inverse) =
            pack_projection(frame.fov_degrees, aspect, frame.depth_far);
        let camera = frame.camera.as_vec3();
        let first_frame = self.previous.is_none();
        let (previous_exact, previous_view, previous_projection) =
            self.previous.unwrap_or((frame.camera, view, projection));
        let previous_camera = previous_exact.as_vec3();
        self.previous = Some((frame.camera, view, projection));
        // The camera's whole blocks and the rest, exact however far from the origin.
        let whole = |p: glam::DVec3| p.floor();
        let int = |p: glam::DVec3| Value::i(&whole(p).as_ivec3().to_array());
        let fract = |p: glam::DVec3| Value::f(&(p - whole(p)).as_vec3().to_array());
        // The player's, not the camera's: in a third-person view the camera is behind (or in
        // front of, turned around) the eye the pack lights from.
        let [yaw, pitch] = frame.eye_rotation.map(f32::to_radians);
        let look = Vec3::new(
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        );

        let to_view = |v: Vec3| view.transform_vector3(v);
        let sun_angle = day_fraction(frame.sky.sun_angle);
        let day = sun_angle < 0.5;
        let tilt = self.shadow.sun_path_rotation;
        let sun = to_view(body(frame.sky.sun_angle, tilt));
        let moon = to_view(body(frame.sky.moon_angle, tilt));
        // Half-lives are given in ticks (7.2); smoothing runs per frame in seconds (OQ-26), at
        // a tenth of a second a tick: Iris 1.11.7 smooths that slowly (measured frame by frame
        // with a probe pack: wetness 60 s rising, 20 s falling; eyeBrightnessSmooth 1 s;
        // centerDepthSmooth 0.1 s).
        let half_life = if inputs.rain > self.wetness {
            600.0
        } else {
            200.0
        };
        self.wetness = smooth(self.wetness, inputs.rain, half_life * SECONDS_A_TICK, dt);
        let eye = inputs.eye_light.map(|l| (l * 16) as f32);
        if first_frame {
            self.wetness = inputs.rain;
            self.eye_smooth = eye;
        }
        for (smoothed, target) in self.eye_smooth.iter_mut().zip(eye) {
            *smoothed = smooth(*smoothed, target, 10.0 * SECONDS_A_TICK, dt);
        }
        // `centerDepthHalflife`'s default, 1 tick (OptiFine's shaders.txt); the first value
        // read back is taken as it is.
        if let Some(raw) = self.center_depth_raw {
            self.center_depth = Some(match self.center_depth {
                Some(v) => smooth(v, raw, SECONDS_A_TICK, dt),
                None => raw,
            });
        }
        let center_depth = self.center_depth.unwrap_or(1.0);
        // Vanilla fades by the stronger of two linear fogs; a pack gets the environmental one
        // (`FogData::environmental_start`/`environmental_end`), as Iris 1.11.7 gives it in every
        // program kind: 0 to 1024 on a clear day, -160 to 768 in rain, -8 to 96 under water, 10
        // to 96 in the Nether (measured with a probe pack).
        let (fog_color, fog_start, fog_end) = match &frame.fog {
            Some(f) => (
                [f.color[0], f.color[1], f.color[2]],
                f.environmental_start,
                f.environmental_end,
            ),
            None => (frame.sky.fog_color, frame.distance * 0.9, frame.distance),
        };
        let light = shadow_light(frame, &self.shadow);
        let (shadow_projection, shadow_projection_inverse) = match self.shadow.fov {
            Some(_) => {
                let (_, projection) = shadow_camera(light, frame.camera, &self.shadow);
                (projection, projection.inverse())
            }
            None => shadow_ortho(&self.shadow),
        };
        let (shadow_view, shadow_view_inverse) = shadow_model_view(frame, &self.shadow);
        let day_time = inputs.day_time.rem_euclid(24000) as i32;
        let world_day = inputs.day_time.div_euclid(24000);
        let (seconds, frame_time, frame_counter) = match inputs.still_ticks {
            Some(ticks) => (
                (ticks as f32 / 20.0) % 3600.0,
                0.05,
                (ticks % 720_720) as i32,
            ),
            None => (
                (now - self.start).as_secs_f32() % 3600.0,
                measured,
                self.frame as i32,
            ),
        };

        let value = |name: &str| -> Value {
            let v3 = |v: Vec3| Value::f(&v.to_array());
            let m4 = |m: Mat4| Value::f(&m.to_cols_array());
            match name {
                "cameraPosition" => v3(camera),
                "previousCameraPosition" => v3(previous_camera),
                "cameraPositionInt" => int(frame.camera),
                "cameraPositionFract" => fract(frame.camera),
                "previousCameraPositionInt" => int(previous_exact),
                "previousCameraPositionFract" => fract(previous_exact),
                // The camera is the eye in first person.
                "eyePosition" => v3(frame.eye.as_vec3()),
                "relativeEyePosition" => v3((frame.camera - frame.eye).as_vec3()),
                "playerLookVector" | "playerBodyVector" => v3(look),
                "firstPersonCamera" => Value::i(&[i32::from(frame.first_person)]),
                "isRightHanded" => Value::i(&[i32::from(inputs.right_handed)]),
                "is_on_ground" => Value::i(&[i32::from(inputs.on_ground)]),
                "bedrockLevel" => Value::i(&[inputs.min_y]),
                "heightLimit" => Value::i(&[inputs.height]),
                "logicalHeightLimit" => Value::i(&[inputs.logical_height]),
                "hasSkylight" => Value::i(&[i32::from(inputs.has_skylight)]),
                "hasCeiling" => Value::i(&[i32::from(inputs.has_ceiling)]),
                "eyeAltitude" => Value::f(&[frame.camera.y as f32]),
                "upPosition" => v3(to_view(Vec3::new(0.0, 100.0, 0.0))),
                "sunPosition" => v3(sun),
                "endFlashPosition" => v3(to_view(inputs.end_flash_direction)),
                "centerDepthSmooth" => Value::f(&[center_depth]),
                "moonPosition" => v3(moon),
                "shadowLightPosition" => v3(if day { sun } else { moon }),
                "sunAngle" => Value::f(&[sun_angle]),
                "shadowAngle" => Value::f(&[shadow_angle(frame)]),
                "worldTime" => Value::i(&[day_time]),
                "worldDay" => Value::i(&[world_day as i32]),
                "moonPhase" => Value::i(&[world_day.rem_euclid(8) as i32]),
                "frameCounter" => Value::i(&[frame_counter]),
                "shadowModelView" => m4(shadow_view),
                "shadowModelViewInverse" => m4(shadow_view_inverse),
                "shadowProjection" => m4(shadow_projection),
                "shadowProjectionInverse" => m4(shadow_projection_inverse),
                "atlasSize" => Value::i(&[self.atlas[0] as i32, self.atlas[1] as i32]),
                "frameTime" => Value::f(&[frame_time]),
                "frameTimeCounter" => Value::f(&[seconds]),
                "viewWidth" => Value::f(&[size[0] as f32]),
                "viewHeight" => Value::f(&[size[1] as f32]),
                "aspectRatio" => Value::f(&[aspect]),
                "near" => Value::f(&[0.05]),
                // The chunk render distance, not the distant terrain's reach.
                "far" => Value::f(&[frame.detail_distance]),
                "rainStrength" => Value::f(&[inputs.rain]),
                "wetness" => Value::f(&[self.wetness]),
                "thunderStrength" => Value::f(&[inputs.thunder]),
                "fogColor" => Value::f(&fog_color),
                "skyColor" => Value::f(&frame.sky.sky_color),
                // Iris 1.11.7, measured: programs that draw no geometry read no fog mode (0) and
                // shape (-1); those that do read GL_LINEAR and a cylinder (1). The density is 0.
                "fogMode" => Value::i(&[0]),
                GBUFFERS_FOG_MODE => Value::i(&[9729]),
                "fogStart" => Value::f(&[fog_start]),
                "fogEnd" => Value::f(&[fog_end]),
                "fogDensity" => Value::f(&[0.0]),
                "fogShape" => Value::i(&[-1]),
                GBUFFERS_FOG_SHAPE => Value::i(&[1]),
                // Iris 1.11.7 gives gbuffers programs 0 (measured in 16 scenes).
                GBUFFERS_CENTER_DEPTH => Value::f(&[0.0]),
                // Outside a draw (gbuffers programs get these per draw), as Iris 1.11.7 gives
                // them to composite and final programs (measured).
                "alphaTestRef" => Value::f(&[0.1]),
                "blockEntityId" => Value::i(&[-1]),
                "entityColor" => Value::f(&[0.0; 4]),
                "isEyeInWater" => Value::i(&[inputs.eye_in]),
                "blindness" => Value::f(&[inputs.blindness]),
                "nightVision" => Value::f(&[frame.night_vision]),
                "darknessFactor" => Value::f(&[frame.darkness.0]),
                "darknessLightFactor" => Value::f(&[frame.darkness.1]),
                "screenBrightness" => Value::f(&[inputs.brightness]),
                "hideGUI" => Value::i(&[inputs.hide_gui as i32]),
                "eyeBrightness" => Value::i(&eye.map(|v| v as i32)),
                "biome" => Value::i(&[inputs.biome]),
                "biome_category" => Value::i(&[inputs.biome_category]),
                "biome_precipitation" => Value::i(&[inputs.biome_precipitation]),
                "temperature" => Value::f(&[inputs.temperature]),
                "rainfall" => Value::f(&[inputs.rainfall]),
                // Rounded down, as Iris 1.11.7 gives it (probe pack).
                "eyeBrightnessSmooth" => Value::i(&self.eye_smooth.map(|v| v.floor() as i32)),
                "gbufferModelView" => m4(view),
                "gbufferModelViewInverse" => m4(view_inverse),
                "gbufferProjection" => m4(projection),
                "gbufferProjectionInverse" => m4(projection_inverse),
                "gbufferPreviousModelView" => m4(previous_view),
                "gbufferPreviousProjection" => m4(previous_projection),
                "heldItemId" => Value::i(&[inputs.held_items[0]]),
                "heldItemId2" => Value::i(&[inputs.held_items[1]]),
                "heldBlockLightValue" => Value::i(&[inputs.held_light[0]]),
                "heldBlockLightValue2" => Value::i(&[inputs.held_light[1]]),
                "isElytraFlying" => Value::i(&[inputs.elytra_flying as i32]),
                "cloudHeight" => Value::f(&[inputs.cloud_height]),
                "cloudTime" => Value::f(&[inputs.cloud_time]),
                "ambientLight" => Value::f(&[inputs.ambient_light]),
                // Iris 1.11.7 gave both the same value in every scene measured.
                "playerMood" | "constantMood" => Value::f(&[inputs.mood]),
                // renderStage is NONE outside gbuffers draws.
                _ => status_value(name, inputs).unwrap_or(Value::i(&[0])),
            }
        };
        self.values.clear();
        self.values.extend(NAMES.iter().map(|n| value(n)));
        self.frame = (self.frame + 1) % 720_720;
    }

    /// `hz_fog_color` and `hz_fog` of the translator's block (the compatibility `gl_Fog`).
    /// Its density is `fogDensity`'s (0): Iris 1.11.7 gave `gl_Fog.density` 0, not 1, in every
    /// program kind our probe pack read it in (gbuffers_terrain and deferred, in a cave and in
    /// the Nether), where `fogDensity` read 0 too.
    pub fn fog(&self) -> [Vec4; 2] {
        let get = |name| match index(name).and_then(|i| self.values.get(i)) {
            Some(Value::F(f)) => *f,
            _ => [0.0; 16],
        };
        let (color, start, end) = (get("fogColor"), get("fogStart")[0], get("fogEnd")[0]);
        [
            Vec4::new(color[0], color[1], color[2], 1.0),
            Vec4::new(
                get("fogDensity")[0],
                start,
                end,
                1.0 / (end - start).max(1e-3),
            ),
        ]
    }
}

impl FrameUniforms {
    /// Evaluates the pack's custom uniforms with this frame's values and puts them after
    /// `NAMES`' values, in `custom.uniforms()` order.
    pub fn eval_custom(&mut self, custom: &mut horizon_shaders::Custom) {
        if let Some(tick) = self.still_ticks {
            custom.restart_at(tick);
        }
        self.values.truncate(NAMES.len());
        let values = &self.values;
        let builtin = |name: &str| {
            let v = values.get(index(name)?)?;
            let n = components(name);
            Some(match *v {
                Value::F(f) => horizon_shaders::Val::vector(&f.map(f64::from)[..n]),
                Value::I(i) => horizon_shaders::Val::vector(&i.map(f64::from)[..n.min(3)]),
            })
        };
        let dt = match index("frameTime").and_then(|i| values.get(i)) {
            Some(Value::F(f)) => f64::from(f[0]),
            _ => 0.0,
        };
        let out = custom.eval(dt, &builtin);
        self.values
            .extend(out.iter().map(|v| Value::f(&v.v.map(|x| x as f32)[..v.n])));
    }
}

/// Iris's player status and dimension uniforms, from `inputs`: the vitals are fractions of
/// their maximum, and -1 (with the maximums of health and air) in creative and spectator.
fn status_value(name: &str, inputs: &PackInputs) -> Option<Value> {
    let status = inputs.status;
    let vital = |v: f32| {
        Value::f(&[if status.survival {
            v.clamp(0.0, 1.0)
        } else {
            -1.0
        }])
    };
    let flag = |b: bool| Value::i(&[i32::from(b)]);
    Some(match name {
        "currentPlayerHealth" => vital(status.health / status.max_health.max(1.0)),
        "maxPlayerHealth" => Value::f(&[if status.survival {
            status.max_health
        } else {
            -1.0
        }]),
        "currentPlayerHunger" => vital(status.food / MAX_HUNGER),
        "maxPlayerHunger" => Value::f(&[MAX_HUNGER]),
        "currentPlayerAir" => vital(status.air / MAX_AIR),
        "maxPlayerAir" => Value::f(&[if status.survival { MAX_AIR } else { -1.0 }]),
        "currentPlayerArmor" => vital(status.armor / MAX_ARMOR),
        "maxPlayerArmor" => Value::f(&[MAX_ARMOR]),
        "isSpectator" => flag(status.spectator),
        "isRiding" => flag(status.riding),
        "feetInWater" => flag(status.feet_in_water),
        "inSwimmingAnimation" => flag(status.swimming),
        "is_burning" => flag(status.burning),
        "is_hurt" => flag(status.hurt),
        // Spectators count as invisible.
        "is_invisible" => flag(status.invisible || status.spectator),
        "is_sneaking" => flag(status.sneaking),
        "is_sprinting" => flag(status.sprinting),
        "endFlashIntensity" => Value::f(&[inputs.end_flash]),
        _ => return None,
    })
}

/// How many components the uniform `name` has (16 for a matrix).
fn components(name: &str) -> usize {
    match name {
        "cameraPosition"
        | "previousCameraPosition"
        | "cameraPositionInt"
        | "cameraPositionFract"
        | "previousCameraPositionInt"
        | "previousCameraPositionFract"
        | "eyePosition"
        | "relativeEyePosition"
        | "playerLookVector"
        | "playerBodyVector"
        | "upPosition"
        | "sunPosition"
        | "moonPosition"
        | "shadowLightPosition"
        | "fogColor"
        | "skyColor"
        | "endFlashPosition" => 3,
        "eyeBrightness" | "eyeBrightnessSmooth" | "atlasSize" => 2,
        "entityColor" => 4,
        n if n.starts_with("gbuffer") || n.starts_with("shadow") && n != "shadowAngle" => 16,
        _ => 1,
    }
}

/// What `name` gives custom uniforms before the first frame: zeros, at its size, so their
/// shapes check out as on a frame (#9086).
pub fn zero(name: &str) -> Option<horizon_shaders::Val> {
    index(name)?;
    Some(horizon_shaders::Val::vector(&[0.0; 16][..components(name)]))
}

/// The position of `name` in [`NAMES`].
pub fn index(name: &str) -> Option<usize> {
    NAMES.iter().position(|n| *n == name)
}

/// The position in [`NAMES`] of the value a program reads as `name`; `geometry`: the program
/// draws the game's geometry (gbuffers, shadow), which reads its own fog mode and shape and no
/// centre depth.
pub fn index_in(name: &str, geometry: bool) -> Option<usize> {
    match name {
        "fogMode" if geometry => index(GBUFFERS_FOG_MODE),
        "fogShape" if geometry => index(GBUFFERS_FOG_SHAPE),
        "centerDepthSmooth" if geometry => index(GBUFFERS_CENTER_DEPTH),
        _ => index(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_follow_the_declared_type() {
        let mut out = vec![0u8; 64];
        Value::i(&[7]).write("float", &mut out, 0);
        assert_eq!(f32::from_ne_bytes(out[0..4].try_into().unwrap()), 7.0);
        Value::f(&[1.5, 2.5]).write("ivec2", &mut out, 4);
        assert_eq!(i32::from_ne_bytes(out[8..12].try_into().unwrap()), 2);
        Value::i(&[1]).write("bool", &mut out, 16);
        assert_eq!(i32::from_ne_bytes(out[16..20].try_into().unwrap()), 1);
        Value::f(&[1.0; 16]).write("mat4", &mut out, 60);
        assert_eq!(out[60..64], [0; 4], "a value past the block is dropped");
    }

    /// The declarations our probe packs (#9307) ran in Iris 1.11.7: kept, or read as 0.
    #[test]
    fn built_ins_are_read_only_as_iris_gives_them() {
        let int = Value::i(&[1]);
        let float = Value::f(&[0.25]);
        let matrix = Value::f(&[1.0; 16]);
        for (value, name, ty, kept) in [
            (&int, "isSpectator", "bool", true),
            (&int, "isSpectator", "int", true),
            (&int, "worldTime", "bool", true),
            (&int, "worldTime", "uint", false),
            (&int, "worldTime", "float", false),
            (&Value::i(&[2048, 2048]), "atlasSize", "ivec2", true),
            (&Value::i(&[2048, 2048]), "atlasSize", "vec2", false),
            (&Value::i(&[2048, 2048]), "atlasSize", "int", false),
            (&Value::i(&[2048, 2048]), "atlasSize", "ivec3", false),
            (&float, "sunAngle", "float", true),
            (&float, "sunAngle", "vec2", false),
            (&float, "viewWidth", "int", false),
            (&Value::f(&[1.0, 2.0, 3.0]), "cameraPosition", "vec3", true),
            (&Value::f(&[1.0, 2.0, 3.0]), "cameraPosition", "vec4", false),
            (&Value::f(&[1.0, 2.0, 3.0]), "cameraPosition", "vec2", false),
            (&matrix, "gbufferModelView", "mat4", true),
            (&matrix, "gbufferModelView", "mat3", false),
            (&Value::i(&[1]), GBUFFERS_FOG_MODE, "int", true),
        ] {
            assert_eq!(value.read_as(name, ty), kept, "{ty} {name}");
        }
    }

    /// The load-time run of custom uniforms sees each built-in at its size: Iris 1.11.7 reads 2
    /// for the first line of our probes K and X (#9086) and 0 for the others.
    #[test]
    fn custom_uniforms_check_the_shapes_of_built_ins_before_the_first_frame() {
        let properties = horizon_shaders::Properties::parse(
            "uniform.vec3.a = skyColor * vec3(0.0, 0.0, 0.0) + vec3(2.0, 2.0, 2.0)\n\
             uniform.float.b = skyColor\n\
             uniform.float.c = gbufferModelView * 0.0 + 2.0\n",
            &horizon_shaders::Macros::standard(),
        );
        let mut custom = horizon_shaders::Custom::parse(&properties);
        custom.eval(0.0, &zero);
        let broken: Vec<&str> = custom.notes.iter().map(|n| &n[1..2]).collect();
        assert_eq!(broken, ["b", "c"], "{:?}", custom.notes);
    }

    /// Iris's status uniforms as its documentation defines them (shaders.properties, "Status").
    #[test]
    fn status_uniforms_follow_iris() {
        let float = |name: &str, inputs: &PackInputs| match status_value(name, inputs) {
            Some(Value::F(f)) => f[0],
            other => panic!("{name}: {other:?}"),
        };
        let int = |name: &str, inputs: &PackInputs| match status_value(name, inputs) {
            Some(Value::I(i)) => i[0],
            other => panic!("{name}: {other:?}"),
        };
        let mut inputs = PackInputs {
            status: PlayerStatus {
                survival: true,
                health: 15.0,
                max_health: 20.0,
                food: 10.0,
                air: 150.0,
                armor: 20.0,
                sneaking: true,
                ..Default::default()
            },
            ..Default::default()
        };
        // The vitals are fractions of their maximums.
        assert_eq!(float("currentPlayerHealth", &inputs), 0.75);
        assert_eq!(float("maxPlayerHealth", &inputs), 20.0);
        assert_eq!(float("currentPlayerHunger", &inputs), 0.5);
        assert_eq!(float("maxPlayerHunger", &inputs), 20.0);
        assert_eq!(float("currentPlayerAir", &inputs), 0.5);
        assert_eq!(float("maxPlayerAir", &inputs), 300.0);
        assert_eq!(float("currentPlayerArmor", &inputs), 0.4);
        assert_eq!(float("maxPlayerArmor", &inputs), 50.0);
        assert_eq!(int("is_sneaking", &inputs), 1);
        assert_eq!(int("is_invisible", &inputs), 0);
        // Creative and spectator: -1 for the vitals and the maximums of health and air;
        // spectators are invisible.
        inputs.status.survival = false;
        inputs.status.spectator = true;
        for name in [
            "currentPlayerHealth",
            "maxPlayerHealth",
            "currentPlayerHunger",
            "currentPlayerAir",
            "maxPlayerAir",
            "currentPlayerArmor",
        ] {
            assert_eq!(float(name, &inputs), -1.0, "{name}");
        }
        assert_eq!(float("maxPlayerHunger", &inputs), 20.0);
        assert_eq!(float("maxPlayerArmor", &inputs), 50.0);
        assert_eq!(int("isSpectator", &inputs), 1);
        assert_eq!(int("is_invisible", &inputs), 1);
        // Each is provided, not left at zero for the pack.
        for name in [
            "currentPlayerHealth",
            "isSpectator",
            "is_sprinting",
            "bedrockLevel",
        ] {
            assert!(index(name).is_some(), "{name} is not provided");
        }
        assert!(
            NAMES
                .iter()
                .filter(|n| status_value(n, &inputs).is_some())
                .count()
                == 18
        );
    }

    #[test]
    fn cloud_height_needs_a_cloud_color() {
        assert_eq!(cloud_height(0.8, 192.33), 192.33);
        assert_eq!(cloud_height(0.8, 120.0), 120.0);
        assert!(cloud_height(0.0, 192.33).is_nan());
    }

    #[test]
    fn elytra_and_cloud_height_uniforms_are_written() {
        assert!(NAMES.ends_with(&["isElytraFlying", "cloudHeight"]));
        let mut out = vec![0u8; 16];
        Value::i(&[1]).write("bool", &mut out, 0);
        assert_eq!(i32::from_ne_bytes(out[0..4].try_into().unwrap()), 1);
        Value::f(&[192.33]).write("float", &mut out, 4);
        assert_eq!(f32::from_ne_bytes(out[4..8].try_into().unwrap()), 192.33);
        Value::f(&[f32::NAN]).write("float", &mut out, 8);
        assert!(f32::from_ne_bytes(out[8..12].try_into().unwrap()).is_nan());
    }

    #[test]
    fn noon_is_straight_up_and_the_sun_sets_in_the_west() {
        assert!(body(0.0, 0.0).abs_diff_eq(Vec3::new(0.0, 100.0, 0.0), 1e-3));
        assert!(body(90.0, 0.0).abs_diff_eq(Vec3::new(-100.0, 0.0, 0.0), 1e-3));
        // sunPathRotation tilts noon toward -Z and leaves sunset on the horizon.
        assert!(body(0.0, 30.0).abs_diff_eq(Vec3::new(0.0, 86.60254, -50.0), 1e-3));
        assert!(body(90.0, 30.0).abs_diff_eq(Vec3::new(-100.0, 0.0, 0.0), 1e-3));
    }

    #[test]
    fn the_shadow_camera_looks_down_from_the_light_in_steps() {
        let shadow = Shadow::default();
        let mut camera = DVec3::new(1000.3, 70.0, -20.7);
        // Noon: the light is straight up.
        let (view, projection) = shadow_camera(Vec3::Y, camera, &shadow);
        let at = |p: Vec3| view.transform_point3(p);
        // Straight down: a block below the player is farther from the light.
        assert!(at(Vec3::new(0.0, -1.0, 0.0)).z < at(Vec3::ZERO).z);
        // The view's origin is interval / 2 - camera % interval from the player on each axis,
        // the remainder with the camera's sign: up to a step and a half on negative axes.
        let origin = view.inverse().w_axis.truncate();
        assert!(
            origin.abs_diff_eq(Vec3::new(0.7, 1.0, 1.7), 1e-3),
            "{origin}"
        );
        // Moving within a step keeps the world where it was in the map.
        camera.x = 1000.6;
        let (moved, _) = shadow_camera(Vec3::Y, camera, &shadow);
        let world = Vec3::new(5.0, -3.0, 9.0);
        let shift = Vec3::new(0.3, 0.0, 0.0);
        assert!(
            moved
                .transform_point3(world - shift)
                .abs_diff_eq(at(world), 1e-3)
        );
        // shadowDistance blocks reach the map's edge.
        let edge = projection.project_point3(Vec3::new(160.0, 0.0, 0.0));
        assert!((edge.x.abs() - 1.0).abs() < 1e-4, "{edge}");
    }

    /// Iris's matrices at 3000 ticks with `sunPathRotation = 30` and the camera at
    /// (0.5, -58.38, 0.5), read from its uniforms by a probe pack.
    #[test]
    fn the_shadow_camera_matches_iris() {
        let shadow = Shadow {
            sun_path_rotation: 30.0,
            ..Shadow::default()
        };
        let light = Vec3::new(0.56782, 0.71287, -0.41158).normalize();
        let (view, projection) = shadow_camera(light, DVec3::new(0.5, -58.38, 0.5), &shadow);
        let iris_view = Mat4::from_cols_array_2d(&[
            [0.82315, 0.0, 0.56782, 0.0],
            [-0.49175, -0.5, 0.71287, 0.0],
            [0.28391, -0.86603, -0.41158, 0.0],
            [0.12508, 1.12301, -1.06188, 1.0],
        ]);
        assert!(view.abs_diff_eq(iris_view, 2e-4), "{view}");
        assert!(
            (projection.z_axis.z + 0.0078110).abs() < 1e-6,
            "{projection}"
        );
        assert!((projection.w_axis.z + 0.21851).abs() < 1e-4, "{projection}");
        // Elsewhere the origin keeps to the steps: Iris put it at (-0.5, 1.38, 2.5) here.
        let (view, _) = shadow_camera(light, DVec3::new(3.5, -58.38, -1.5), &shadow);
        let origin = view.inverse().w_axis.truncate();
        assert!(
            origin.abs_diff_eq(Vec3::new(-0.5, 1.38, 2.5), 1e-3),
            "{origin}"
        );
    }
}

#[cfg(test)]
mod initial_history_tests {
    use super::*;
    use crate::hud::HudBatch;
    fn float(u: &FrameUniforms, name: &str) -> [f32; 16] {
        match u.values[index(name).unwrap()] {
            Value::F(v) => v,
            _ => panic!("float uniform expected"),
        }
    }
    /// The bits of a matrix uniform.
    fn bits(u: &FrameUniforms, name: &str) -> [u32; 16] {
        float(u, name).map(f32::to_bits)
    }

    /// `shadowProjection` and its inverse to the last bit, as a probe pack of ours read them
    /// back in Iris 1.11.7, for `shadowDistance`, `shadowNearPlane` and `shadowFarPlane` (none
    /// meaning the pack doesn't set it): a plane the pack sets is taken as it is, with no
    /// 100-block offset; one it doesn't set is -100.05 (near) or 156 (far).
    #[test]
    fn the_shadow_projection_is_iris_s_for_the_pack_s_planes() {
        let hud = HudBatch::new([1280, 720]);
        let f = frame(&hud);
        let (o, z, one) = (0, 0x80000000, 0x3f800000);
        type Case = (f32, Option<f32>, Option<f32>, [u32; 4], [u32; 4]);
        // (distance, near, far, [side, m22, m32, -], [inverse side, m22, m32, -])
        let cases: [Case; 5] = [
            (
                100.0,
                None,
                None,
                [0x3c23d70a, 0xbbfff335, 0xbe5fc198, 0],
                [0x42c80001, 0xc3000666, 0xc1dfccc8, 0],
            ),
            (
                100.0,
                Some(0.5),
                Some(128.0),
                [0x3c23d70a, 0xbc808081, 0xbf810101, 0],
                [0x42c80000, 0xc27f0000, 0xc2808000, 0],
            ),
            (
                100.0,
                None,
                Some(128.0),
                [0x3c23d70a, 0xbc0fb013, 0xbdfb0128, 0],
                [0x42c80000, 0xc2e40ccc, 0xc15f9994, 0],
            ),
            (
                100.0,
                Some(0.05),
                None,
                [0x3c23d70a, 0xbc521e5f, 0xbf801503, 0],
                [0x42c80000, 0xc29bf333, 0xc29c0ccd, 0],
            ),
            (
                127.3,
                Some(1.7),
                Some(333.3),
                [0x3c00b42f, 0xbbc5a2bf, 0xbf814ffb, 0],
                [0x42fe9999, 0xc325cccc, 0xc3277fff, 0],
            ),
        ];
        for (distance, near, far, [side, m22, m32, _], [i_side, i22, i32, _]) in cases {
            let mut u = FrameUniforms {
                shadow: Shadow {
                    distance,
                    near,
                    far,
                    ..Shadow::default()
                },
                ..FrameUniforms::default()
            };
            u.update(&f, &PackInputs::default(), [1280, 720]);
            assert_eq!(
                bits(&u, "shadowProjection"),
                [side, o, o, o, o, side, o, o, o, o, m22, o, o, o, m32, one],
                "{near:?} {far:?}"
            );
            assert_eq!(
                bits(&u, "shadowProjectionInverse"),
                [
                    i_side, z, z, o, z, i_side, z, o, z, z, i22, o, z, z, i32, one
                ],
                "{near:?} {far:?}"
            );
        }
    }

    /// Iris 1.11.7's camera and shadow matrices to the last bit, read back from its uniforms by
    /// a probe pack of ours: on a texel boundary of the shadow map the last bit decides which
    /// texel a fragment reads (#8464). Noon at 6000 ticks, looking west (yaw 90) with vanilla's
    /// flying field of view (76.99999°) at 1280×720, the camera at (13.5, 69.62, -13.5).
    #[test]
    fn the_matrices_are_iris_to_the_last_bit() {
        let hud = HudBatch::new([1280, 720]);
        let mut f = frame(&hud);
        f.camera = DVec3::new(13.5, 68.0 + f64::from(1.62f32), -13.5);
        f.yaw = 90.0;
        f.pitch = 0.0;
        f.fov_degrees = f32::from_bits(0x4299ffff);
        f.depth_far = 1024.0;
        f.sky.sun_angle = 0.0;
        f.sky.moon_angle = 180.0;
        let mut u = FrameUniforms::default();
        u.update(&f, &PackInputs::default(), [1280, 720]);
        let (o, one, minus) = (0, 0x3f800000, 0xbf800000);
        let (z, almost) = (0x80000000, 0x3f7fffff);
        assert_eq!(
            bits(&u, "gbufferModelView"),
            [
                o, o, almost, o, o, almost, o, o, 0xbf7fffff, o, o, o, o, o, o, one
            ]
        );
        assert_eq!(
            bits(&u, "gbufferModelViewInverse"),
            [
                o, o, 0xbf7fffff, o, o, almost, o, o, almost, o, o, o, z, z, z, one
            ]
        );
        assert_eq!(
            bits(&u, "gbufferProjection"),
            [
                0x3f350867, o, o, o, o, 0x3fa0eb07, o, o, o, o, 0xbf800333, minus, o, o,
                0xbdcccf5c, o
            ]
        );
        assert_eq!(
            bits(&u, "gbufferProjectionInverse"),
            [
                0x3fb5017f, z, z, z, z, 0x3f4ba1af, o, z, z, z, z, 0xc11ffe00, z, z, minus,
                0x41200200
            ]
        );
        // The origin's height is the camera's as a float, 69.620003, in steps of 2.
        assert_eq!(
            bits(&u, "shadowModelView"),
            [
                one, o, o, o, o, o, one, o, o, minus, z, o, 0x3f000000, 0x40200000, 0x3f1eb880, one
            ]
        );
        assert_eq!(
            bits(&u, "shadowModelViewInverse"),
            [
                one, o, o, o, o, o, minus, o, o, one, z, o, 0xbf000000, 0xbf1eb880, 0x40200000, one
            ]
        );
        assert_eq!(float(&u, "sunAngle")[0].to_bits(), 0x3e800000);

        // 9000 ticks (the sun 34.598564° past noon), yaw 200, pitch -30.
        f.yaw = 200.0;
        f.pitch = -30.0;
        f.sky.sun_angle = f32::from_bits(0x420a64ee);
        f.sky.moon_angle = f.sky.sun_angle + 180.0;
        u.update(&f, &PackInputs::default(), [1280, 720]);
        assert_eq!(
            bits(&u, "gbufferModelView"),
            [
                0x3f708fb1, 0xbe2f1d41, 0xbe97a745, o, o, 0x3f5db3d6, 0xbefffffe, o, 0x3eaf1d41,
                0x3ef08fb0, 0x3f50550b, o, o, o, o, one
            ]
        );
        assert_eq!(
            bits(&u, "shadowModelView"),
            [
                0x3f52b9ff, o, 0xbf115cdb, o, 0x3f115cdb, o, 0x3f52b9ff, o, o, minus, z, o,
                0x3f437d12, 0x40200000, 0x3e67e0f2, one
            ]
        );
        assert_eq!(float(&u, "sunAngle")[0].to_bits(), 0x3eb134f4);
    }

    /// At night the moon casts the shadows, here with `sunPathRotation = 30`: Iris 1.11.7 at
    /// 16000 ticks (the moon at 324.4897°), the camera at (-556.5, 76.62, 212.5), whose
    /// negative x keeps its sign in the remainder.
    #[test]
    fn the_moon_s_shadow_matrices_are_iris_to_the_last_bit() {
        let hud = HudBatch::new([1280, 720]);
        let mut f = frame(&hud);
        f.camera = DVec3::new(-556.5, 75.0 + f64::from(1.62f32), 212.5);
        f.sky.sun_angle = f32::from_bits(0x43107d5b);
        f.sky.moon_angle = f32::from_bits(0x43a23eae);
        let mut u = FrameUniforms {
            shadow: Shadow {
                sun_path_rotation: 30.0,
                ..Shadow::default()
            },
            ..FrameUniforms::default()
        };
        u.update(&f, &PackInputs::default(), [1280, 720]);
        let (o, one) = (0, 0x3f800000);
        assert_eq!(
            bits(&u, "shadowModelView"),
            [
                0x3f506307, o, 0x3f14b28c, o, 0xbf00c699, 0xbf000000, 0x3f3477e1, o, 0x3e94b28c,
                0xbf5db3d7, 0xbed06307, o, 0xbf9668fb, 0x3f1f7dac, 0xbf6f86e4, one
            ]
        );
        assert_eq!(
            bits(&u, "shadowModelViewInverse"),
            [
                0x3f506307, 0xbf00c699, 0x3e94b28c, o, o, 0xbf000000, 0xbf5db3d7, o, 0x3f14b28c,
                0x3f3477e1, 0xbed06307, o, 0x3fc00000, 0x3ec28f00, 0x3effffff, one
            ]
        );
        assert_eq!(float(&u, "sunAngle")[0].to_bits(), 0x3f26bf8b);
        assert_eq!(float(&u, "shadowAngle")[0].to_bits(), 0x3e1afe2e);
    }

    /// Between whole ticks of the timeline, Iris 1.11.7 turns `shadowModelView` by an angle made
    /// from `shadowAngle`, not by the sky's own angle: read back by a probe pack of ours at 7297,
    /// 7300 and 16000 ticks, and at 7303 with `sunPathRotation = 30`, the camera at
    /// (13.5, 69.62, -13.5). The sky angles are vanilla's timeline at those ticks.
    #[test]
    fn the_shadow_matrices_turn_by_iris_s_angle_between_ticks() {
        let hud = HudBatch::new([1280, 720]);
        let mut f = frame(&hud);
        f.camera = DVec3::new(13.5, 68.0 + f64::from(1.62f32), -13.5);
        let (o, z, one, minus) = (0, 0x80000000, 0x3f800000, 0xbf800000);
        let cases: [(f32, u32, u32, [u32; 16], [u32; 16]); 4] = [
            (
                0.0,
                0x415d5e10,
                0x4341d5e1,
                [
                    0x3f789291, o, 0xbe74dfb0, o, 0x3e74dfb0, o, 0x3f789291, o, o, minus, z, o,
                    0x3f223deb, 0x40200000, 0x3ef7034a, one,
                ],
                [
                    0x3f789291, 0x3e74dfb0, o, o, o, o, minus, o, 0xbe74dfb0, 0x3f789291, z, o,
                    0xbf000000, 0xbf1eb87f, 0x40200000, one,
                ],
            ),
            (
                0.0,
                0x415de96f,
                0x4341de97,
                [
                    0x3f788940, o, 0xbe7576d1, o, 0x3e7576d1, o, 0x3f788940, o, o, minus, z, o,
                    0x3f2250af, 0x40200000, 0x3ef6d1f4, one,
                ],
                [
                    0x3f788940, 0x3e7576d1, o, o, o, o, minus, o, 0xbe7576d1, 0x3f788940, z, o,
                    0xbeffffff, 0xbf1eb87f, 0x40200000, one,
                ],
            ),
            (
                0.0,
                0x43107d5b,
                0x43a23eae,
                [
                    0x3f506307, o, 0x3f14b28c, o, 0xbf14b28c, z, 0x3f506307, o, o, minus, z, o,
                    0x3d400268, 0x40200000, 0x3f4b8c94, one,
                ],
                [
                    0x3f506307, 0xbf14b28c, o, o, o, z, minus, o, 0x3f14b28c, 0x3f506307, z, o,
                    0xbf000000, 0xbf1eb880, 0x40200000, one,
                ],
            ),
            (
                30.0,
                0x415e74d7,
                0x4341e74d,
                [
                    0x3f787fe8, o, 0xbe760e06, o, 0x3e5516f6, 0xbf000000, 0x3f5734fd, o,
                    0xbdf60e06, 0xbf5db3d7, 0xbef87fe8, o, 0x3f6a2bc6, 0x3fed72ac, 0x3fcea5f8, one,
                ],
                [
                    0x3f787fe8, 0x3e5516f6, 0xbdf60e06, o, o, 0xbf000000, 0xbf5db3d7, o,
                    0xbe760e06, 0x3f5734fd, 0xbef87fe8, o, 0xbf000001, 0xbf1eb881, 0x40200000, one,
                ],
            ),
        ];
        for (tilt, sun, moon, view, inverse) in cases {
            f.sky.sun_angle = f32::from_bits(sun);
            f.sky.moon_angle = f32::from_bits(moon);
            let mut u = FrameUniforms {
                shadow: Shadow {
                    sun_path_rotation: tilt,
                    ..Shadow::default()
                },
                ..FrameUniforms::default()
            };
            u.update(&f, &PackInputs::default(), [1280, 720]);
            assert_eq!(bits(&u, "shadowModelView"), view, "{sun:x}");
            assert_eq!(bits(&u, "shadowModelViewInverse"), inverse, "{sun:x}");
        }
    }

    #[test]
    fn first_history_is_current_then_tracks_the_actual_previous_frame() {
        let hud = HudBatch::new([800, 450]);
        let mut f = frame(&hud);
        let mut u = FrameUniforms::default();
        f.camera = DVec3::new(1024.25, -3.5, 11.75);
        f.yaw = 21.0;
        f.pitch = -16.0;
        u.update(&f, &PackInputs::default(), [800, 450]);
        assert_eq!(
            float(&u, "previousCameraPosition"),
            float(&u, "cameraPosition")
        );
        for (prior, current) in [
            ("gbufferPreviousProjection", "gbufferProjection"),
            ("gbufferPreviousModelView", "gbufferModelView"),
        ] {
            assert_eq!(float(&u, prior), float(&u, current));
            assert!(float(&u, prior).iter().any(|v| *v != 0.0));
        }
        let camera = float(&u, "cameraPosition");
        let view = float(&u, "gbufferModelView");
        let projection = float(&u, "gbufferProjection");
        f.camera = DVec3::new(-100.0, 20.0, 40.0);
        f.yaw = 90.0;
        f.pitch = 50.0;
        f.fov_degrees = 100.0;
        u.update(&f, &PackInputs::default(), [1000, 500]);
        assert_eq!(float(&u, "previousCameraPosition"), camera);
        assert_eq!(float(&u, "gbufferPreviousModelView"), view);
        assert_eq!(float(&u, "gbufferPreviousProjection"), projection);
        assert_ne!(
            float(&u, "previousCameraPosition"),
            float(&u, "cameraPosition")
        );
    }
    /// Iris 1.11.7 (probe pack, a world saved at game time 5073 and loaded about ten seconds
    /// before): `cloudTime` read 157.98, 160.56 and 163.11, four seconds apart. That is the
    /// clouds' drift at 0.03 blocks a tick, 5266 ticks in.
    #[test]
    fn cloud_time_is_how_far_the_clouds_drifted() {
        let hud = HudBatch::new([800, 450]);
        let f = frame(&hud);
        let mut u = FrameUniforms::default();
        let drift = crate::clouds::drift(5266, 0.0, 256);
        let input = PackInputs {
            cloud_time: drift as f32 * 0.030_000_001,
            ..Default::default()
        };
        u.update(&f, &input, [800, 450]);
        assert!((float(&u, "cloudTime")[0] - 157.98).abs() < 1e-3);
        // A trip across the 256-cell texture takes 256 * 400 ticks, then starts again.
        assert_eq!(crate::clouds::drift(256 * 400 + 3, 0.25, 256), 3.25);
    }

    #[test]
    fn environmental_smoothing_starts_at_the_first_inputs_without_resetting_each_frame() {
        let hud = HudBatch::new([800, 450]);
        let f = frame(&hud);
        let mut u = FrameUniforms::default();
        let mut input = PackInputs {
            rain: 0.75,
            eye_light: [3, 14],
            ..Default::default()
        };
        u.update(&f, &input, [800, 450]);
        assert_eq!(float(&u, "wetness")[0], 0.75);
        match u.values[index("eyeBrightnessSmooth").unwrap()] {
            Value::I(v) => assert_eq!(&v[..2], &[48, 224]),
            _ => panic!("integer uniform expected"),
        }
        input.rain = 0.0;
        input.eye_light = [0, 0];
        u.update(&f, &input, [800, 450]);
        assert!(float(&u, "wetness")[0] > 0.0);
        assert!(u.eye_smooth.iter().all(|v| *v > 0.0));
    }
    /// Actual provider bytes through translated GLSL and a GPU pixel, with a nonidentity
    /// model-view/projection and a distant camera. Geometry is already camera-relative.
    #[test]
    #[ignore = "requires a host GPU; run with --ignored --nocapture"]
    fn core_matrix_inputs_render_provider_values() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        eprintln!("Core matrix GPU: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let hud = HudBatch::new([800, 400]);
        let mut f = frame(&hud);
        f.camera = DVec3::new(1024.25, 100.5, -2048.75);
        f.yaw = 180.0;
        f.pitch = 0.0;
        f.fov_degrees = 90.0;
        f.bob = Mat4::from_translation(Vec3::new(0.5, -0.25, 0.0));
        let mut values = FrameUniforms::default();
        values.update(&f, &PackInputs::default(), [800, 400]);
        for (declaration, setup, projection, view, expected) in [
            (
                "",
                "",
                "projectionMatrix",
                "modelViewMatrix",
                [64u8, 64, 128, 255],
            ),
            (
                "uniform mat4 projectionMatrix, modelViewMatrix;",
                "",
                "projectionMatrix",
                "modelViewMatrix",
                [64, 64, 128, 255],
            ),
            (
                "",
                "",
                "gl_ProjectionMatrix",
                "gl_ModelViewMatrix",
                [64, 64, 128, 255],
            ),
            (
                "",
                "mat4 projectionMatrix = mat4(2.0); mat4 modelViewMatrix = mat4(3.0);",
                "projectionMatrix",
                "modelViewMatrix",
                [128, 191, 255, 255],
            ),
            (
                "mat4 projectionMatrix = mat4(2.0), modelViewMatrix = mat4(3.0);",
                "",
                "projectionMatrix",
                "modelViewMatrix",
                [128, 191, 255, 255],
            ),
        ] {
            let vertex = format!(
                "#version 330 compatibility\n{declaration}\nout vec3 result;\n\
                 void main() {{\n{setup}\n\
                 vec4 q = {projection} * {view} * vec4(0.5, 0.75, -2.0, 1.0);\n\
                 result = vec3(q.xy / q.w, q.w / 4.0);\n\
                 vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);\n\
                 gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);\n}}"
            );
            let mapped = draw_line_program(&device, &queue, &values, &vertex);
            // Independent result: translated point (1, .5, -2), perspective (aspect 2,
            // vertical FOV 90): clip (.5, .5, z, 2), hence (.25, .25, .5, 1).
            for (actual, expected) in mapped[..4].iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "{projection}: {:?}",
                    &mapped[..4]
                );
            }
        }
    }

    /// `normalMatrix` in a core-profile gbuffers program is the view's rotation; in the
    /// compatibility profile it stays zero (the issue's Iris 1.11.7 probes, #9136).
    #[test]
    #[ignore = "requires a host GPU; run with --ignored --nocapture"]
    fn core_normal_matrix_renders_the_view_rotation() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        eprintln!("Normal matrix GPU: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let hud = HudBatch::new([800, 400]);
        let mut f = frame(&hud);
        // Facing west (-X), level: view space looks down -Z with +X to the right (north).
        f.yaw = 90.0;
        f.pitch = 0.0;
        let mut values = FrameUniforms::default();
        values.update(&f, &PackInputs::default(), [800, 400]);
        for (profile, world, expected) in [
            // East (behind the camera) is view +Z.
            ("330 core", "vec3(1.0, 0.0, 0.0)", [128u8, 128, 255, 255]),
            // South (to the left) is view -X; up stays up.
            ("330 core", "vec3(0.0, 0.0, 1.0)", [0, 128, 128, 255]),
            ("330 core", "vec3(0.0, 1.0, 0.0)", [128, 255, 128, 255]),
            (
                "330 compatibility",
                "vec3(1.0, 0.0, 0.0)",
                [128, 128, 128, 255],
            ),
        ] {
            let vertex = format!(
                "#version {profile}\nuniform mat3 normalMatrix;\nout vec3 result;\n\
                 void main() {{\n\
                 result = normalMatrix * {world} * 0.5 + 0.5;\n\
                 vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);\n\
                 gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);\n}}"
            );
            let pixel = draw_line_program(&device, &queue, &values, &vertex);
            eprintln!("{profile} {world}: {pixel:?}");
            for (actual, expected) in pixel.iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "{profile} {world}: {pixel:?}"
                );
            }
        }
    }

    /// Draws one fragment through a translated `gbuffers_line` program with `vertex` and the
    /// provider's `values`, and reads it back.
    fn draw_line_program(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        values: &FrameUniforms,
        vertex: &str,
    ) -> [u8; 4] {
        use horizon_shaders::{
            GbuffersInputs, Macros, Pack, Target, preprocess, translate_gbuffers,
        };
        use std::io::Write;
        use wgpu::util::DeviceExt;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, source) in [
            ("shaders/gbuffers_line.vsh", vertex),
            (
                "shaders/gbuffers_line.fsh",
                "#version 330\nin vec3 result; out vec4 color; void main() { color = vec4(result, 1.0); }",
            ),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(source.as_bytes()).unwrap();
        }
        let pack =
            Pack::from_zip_bytes("matrix-probe.zip", zip.finish().unwrap().into_inner()).unwrap();
        let macros = Macros::standard();
        let program = translate_gbuffers(
            &preprocess(&pack, "gbuffers_line.vsh", &macros),
            &preprocess(&pack, "gbuffers_line.fsh", &macros),
            Target::Buffers,
            &GbuffersInputs::default(),
        )
        .unwrap();
        let mut bytes = vec![0; program.interface.uniform_size as usize];
        for uniform in &program.interface.uniforms {
            if let Some(i) = index(&uniform.name) {
                values.values[i].write(&uniform.ty, &mut bytes, uniform.offset as usize);
            }
        }
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let module = |module| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(module)),
            })
        };
        let vs = module(program.vertex);
        let fs = module(program.fragment);
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        read.slice(..)
            .map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mapped = read.slice(..).get_mapped_range().unwrap();
        let pixel = mapped[..4].try_into().unwrap();
        drop(mapped);
        read.unmap();
        pixel
    }

    fn int(u: &FrameUniforms, at: usize) -> i32 {
        match u.values[at] {
            Value::I(v) => v[0],
            _ => panic!("integer uniform expected"),
        }
    }

    /// Iris 1.11.7, read back exactly with a probe pack on a clear day (render distance 12,
    /// cloud range 64): fogStart 0 and fogEnd 1024 (the environmental fog, not the render
    /// distance's 172.8 to 192), fogDensity 0; composite and final programs read alphaTestRef
    /// 0.1, blockEntityId -1, fogMode 0 and fogShape -1.
    #[test]
    fn fog_and_draw_uniforms_outside_a_draw_follow_iris() {
        let hud = HudBatch::new([800, 450]);
        let mut f = frame(&hud);
        f.fog = Some(crate::fog::FogData {
            color: [0.7, 0.8, 1.0, 1.0],
            environmental_start: 0.0,
            environmental_end: 1024.0,
            render_distance_start: 172.8,
            render_distance_end: 192.0,
            sky_end: 192.0,
            cloud_end: 1024.0,
        });
        let mut u = FrameUniforms::default();
        u.update(&f, &PackInputs::default(), [800, 450]);
        assert_eq!(float(&u, "fogStart")[0], 0.0);
        assert_eq!(float(&u, "fogEnd")[0], 1024.0);
        assert_eq!(float(&u, "fogDensity")[0], 0.0);
        assert_eq!(float(&u, "alphaTestRef")[0], 0.1);
        assert_eq!(int(&u, index("blockEntityId").unwrap()), -1);
        assert_eq!(int(&u, index("entityId").unwrap()), 0);
        assert_eq!(int(&u, index("fogMode").unwrap()), 0);
        assert_eq!(int(&u, index("fogShape").unwrap()), -1);
    }

    /// Iris 1.11.7, read back with a probe pack in gbuffers_terrain and in deferred, in a cave
    /// (fog 0 to 1024) and in the Nether (fog 10 to 96): `gl_Fog` holds the fog's colour, start
    /// and end, a scale of 1 / (end - start), and a density of 0, not 1.
    #[test]
    fn the_compatibility_gl_fog_has_no_density() {
        let hud = HudBatch::new([800, 450]);
        let mut f = frame(&hud);
        f.fog = Some(crate::fog::FogData {
            color: [0.09, 0.24, 0.23, 1.0],
            environmental_start: 10.0,
            environmental_end: 96.0,
            render_distance_start: 172.8,
            render_distance_end: 192.0,
            sky_end: 192.0,
            cloud_end: 1024.0,
        });
        let mut u = FrameUniforms::default();
        u.update(&f, &PackInputs::default(), [800, 450]);
        let [color, fog] = u.fog();
        assert_eq!(color.truncate(), Vec3::new(0.09, 0.24, 0.23));
        assert_eq!(fog, Vec4::new(0.0, 10.0, 96.0, 1.0 / 86.0));
    }

    /// Iris 1.11.7 (probe pack): gbuffers programs read fogMode GL_LINEAR (9729) and fogShape 1
    /// (cylinder) where composite and final programs read 0 and -1.
    #[test]
    fn programs_that_draw_geometry_read_their_own_fog_and_centre_depth() {
        let hud = HudBatch::new([800, 450]);
        let mut u = FrameUniforms::default();
        u.update(&frame(&hud), &PackInputs::default(), [800, 450]);
        let at = |name, geometry| index_in(name, geometry).unwrap();
        assert_eq!(int(&u, at("fogMode", true)), 9729);
        assert_eq!(int(&u, at("fogShape", true)), 1);
        assert_eq!(int(&u, at("fogMode", false)), 0);
        assert_eq!(int(&u, at("fogShape", false)), -1);
        assert_eq!(at("fogStart", true), at("fogStart", false));
        // Iris 1.11.7 (probe pack): `centerDepthSmooth` reads 0 in gbuffers programs.
        let depth = |geometry| match u.values[at("centerDepthSmooth", geometry)] {
            Value::F(v) => v[0],
            _ => panic!("float uniform expected"),
        };
        assert_eq!(depth(true), 0.0);
        assert_eq!(depth(false), 1.0);
    }

    /// Iris 1.11.7, read frame by frame with a probe pack: eyeBrightnessSmooth falls by half
    /// in 1 s and is rounded down; wetness takes 60 s to rise halfway and 20 s to fall; the
    /// centre depth 0.1 s.
    #[test]
    fn smoothed_uniforms_take_iris_half_lives() {
        let hud = HudBatch::new([800, 450]);
        let f = frame(&hud);
        let mut u = FrameUniforms::default();
        // The next frame comes `seconds` after the last (and a few microseconds of the test).
        let after = |u: &mut FrameUniforms, seconds: f32| {
            u.last = Some(std::time::Instant::now() - std::time::Duration::from_secs_f32(seconds));
        };
        let mut input = PackInputs {
            eye_light: [0, 15],
            ..Default::default()
        };
        u.center_depth_raw = Some(1.0);
        u.update(&f, &input, [800, 450]);
        input.eye_light = [0, 0];
        u.center_depth_raw = Some(0.0);
        after(&mut u, 0.1);
        u.update(&f, &input, [800, 450]);
        assert!((float(&u, "centerDepthSmooth")[0] - 0.5).abs() < 1e-3);
        after(&mut u, 0.6);
        u.update(&f, &input, [800, 450]);
        // 240 falling toward 0 for 0.7 s: 147.8, read as 147.
        match u.values[index("eyeBrightnessSmooth").unwrap()] {
            Value::I(v) => assert_eq!(v[1], 147),
            _ => panic!("integer uniform expected"),
        }
        input.rain = 1.0;
        after(&mut u, 60.0);
        u.update(&f, &input, [800, 450]);
        assert!((float(&u, "wetness")[0] - 0.5).abs() < 1e-3);
        input.rain = 0.0;
        after(&mut u, 20.0);
        u.update(&f, &input, [800, 450]);
        assert!((float(&u, "wetness")[0] - 0.25).abs() < 1e-3);
    }

    /// Under `HORIZON_STILL` the pack's clock is the world's ticks, whenever and however often
    /// the frame is drawn, and smoothed values are at their targets.
    #[test]
    fn a_still_frame_takes_the_world_ticks_as_its_clock() {
        let hud = HudBatch::new([800, 450]);
        let f = frame(&hud);
        let mut u = FrameUniforms::default();
        let mut input = PackInputs {
            eye_light: [0, 15],
            still_ticks: Some(47_811),
            ..Default::default()
        };
        u.center_depth_raw = Some(1.0);
        u.update(&f, &input, [800, 450]);
        input.eye_light = [0, 0];
        input.rain = 1.0;
        u.center_depth_raw = Some(0.0);
        u.last = Some(std::time::Instant::now() - std::time::Duration::from_secs(3));
        u.update(&f, &input, [800, 450]);
        assert_eq!(
            float(&u, "frameTimeCounter")[0],
            (47_811.0f32 / 20.0) % 3600.0
        );
        assert_eq!(float(&u, "frameTime")[0], 0.05);
        match u.values[index("frameCounter").unwrap()] {
            Value::I(v) => assert_eq!(v[0], 47_811),
            _ => panic!("integer uniform expected"),
        }
        match u.values[index("eyeBrightnessSmooth").unwrap()] {
            Value::I(v) => assert_eq!(v[1], 0),
            _ => panic!("integer uniform expected"),
        }
        assert_eq!(float(&u, "wetness")[0], 1.0);
        assert_eq!(float(&u, "centerDepthSmooth")[0], 0.0);
    }

    /// Iris 1.11.7 (probe pack): `ambientLight` is the dimension type's (0.1 in the Nether,
    /// 0.25 in the End), and `playerMood` and `constantMood` both the cave mood.
    #[test]
    fn ambient_light_and_mood_come_from_the_inputs() {
        let hud = HudBatch::new([800, 450]);
        let mut u = FrameUniforms::default();
        let input = PackInputs {
            ambient_light: 0.25,
            mood: 0.0235,
            ..Default::default()
        };
        u.update(&frame(&hud), &input, [800, 450]);
        assert_eq!(float(&u, "ambientLight")[0], 0.25);
        assert_eq!(float(&u, "playerMood")[0], 0.0235);
        assert_eq!(float(&u, "constantMood")[0], 0.0235);
    }

    fn frame(hud: &HudBatch) -> Frame<'_> {
        Frame {
            eye: glam::DVec3::ZERO,
            eye_rotation: [0.0; 2],
            first_person: true,
            shadow_scene_revision: (0, 0),
            shadow_cache: false,
            water_plane: None,
            weather: &[],
            weather_time: crate::weather::Time::new(0, 0.0),
            camera: glam::DVec3::new(0.5, 100.0, 0.5),
            yaw: 0.0,
            pitch: 10.0,
            fov_degrees: 85.0,
            bob: glam::Mat4::IDENTITY,
            spin: glam::Mat4::IDENTITY,
            sky: crate::environment::Sky::default(),
            distance: 12.0,
            detail_distance: 12.0,
            depth_far: 1000.0,
            underwater: false,
            cave_culling: false,
            title: true,
            sky_blocked: false,
            skybox: false,
            end_sky: false,
            end_sky_sprite: None,
            environment_fog: None,
            water_fog: [0.0; 3],
            fog: None,
            dark_disc: false,
            outline: Vec::new(),
            outline_after_terrain: false,
            outline_high_contrast: false,
            pvp: Default::default(),
            debug_lines: &[],
            debug_lines_on_top: &[],
            debug_crosshair: &[],
            hud,
            glint_alpha: 0.75,
            glint_ticks: 0.0,
            nether_lighting: false,
            entities: &[],
            static_block_entities: &[],
            visible_static_block_entity_sections: &[],
            static_block_entity_stats: crate::block_entity_render::StaticCacheFrameStats::default(),
            entity_motion: &[],
            colored_lights: &[],
            emissive_layers: &[],
            shadow_only: &[],
            entity_spans: &[],
            skin: None,
            glowing: &[],
            dynamic_lights: &[],
            clouds: None,
            lightning: Default::default(),
            additive: &[],
            see_through: &[],
            world_primitives: Default::default(),
            celestial: &[],
            stars: None,
            hand: &[],
            hand_items: &[],
            hand_fov_degrees: 70.0,
            screen: &[],
            pack: Default::default(),
            border: None,
            game_time: 0.0,
            shader_game_time: 0.0,
            end_portal_sprites: [[0.0; 4]; 2],
            crumbling_sprite: [0.0; 4],
            ticks: 0,
            darkness: (0.0, 0.0),
            night_vision: 0.0,
            block_flicker: 0.0,
            boss_darken: 0.0,
            menu_blur: None,
            chunk_fade: 0.0,
            post_effects: crate::post_chain::Request::none(),
        }
    }
}
