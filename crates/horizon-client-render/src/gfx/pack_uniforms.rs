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
    /// Interpolated player head position, before the third-person camera offset.
    pub eye_position: DVec3,
    /// World-space head direction, independent of front/rear camera perspective.
    pub player_look: Vec3,
    pub elytra_flying: bool,
    pub player_mood: f32,
    /// Height of the dimension's cloud plane; NaN when the dimension has none.
    pub cloud_height: f32,
    /// `BossOverlay::should_create_world_fog`: a boss event asks for world fog (`heavyFog`).
    pub heavy_fog: bool,
    /// The End flash's centre in the sky's world axes (camera-centred, before the view), as
    /// the sky draws it; zero outside the End. `endFlashPosition` turns it into view space.
    pub end_flash_position: Vec3,
    /// The End flash's interpolated intensity, 0 outside the End.
    pub end_flash_intensity: f32,
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
    /// The values native mods hand their packs (`frame.mods`, `native_shader_params`).
    pub mod_params: [[f32; 4]; 8],
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
    "cameraPositionInt",
    "cameraPositionFract",
    "previousCameraPositionInt",
    "previousCameraPositionFract",
    "eyePosition",
    "relativeEyePosition",
    "playerLookVector",
    "isElytraFlying",
    "playerMood",
    "cloudHeight",
    "lightningBoltPosition",
    "heavyFog",
    "endFlashPosition",
    "endFlashIntensity",
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
    "playerBodyVector",
    "firstPersonCamera",
    "isRightHanded",
    "is_on_ground",
    "bedrockLevel",
    "heightLimit",
    "logicalHeightLimit",
    "hasSkylight",
    "hasCeiling",
];

/// Values carried from frame to frame.
pub struct FrameUniforms {
    pub values: Vec<Value>,
    /// The block atlas's size (`atlasSize`), 0 without one.
    pub atlas: [u32; 2],
    start: std::time::Instant,
    last: Option<std::time::Instant>,
    frame: u32,
    previous: Option<(DVec3, Mat4, Mat4)>,
    camera_origin: DVec3,
    wetness: f32,
    eye_smooth: [f32; 2],
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
            previous: None,
            camera_origin: DVec3::ZERO,
            wetness: 0.0,
            eye_smooth: [0.0; 2],
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

/// How far toward the light the shadow projection starts, in blocks: part of the projection,
/// not of `shadowModelView` (Iris's matrices, measured).
pub const SHADOW_EYE: f32 = 100.0;

/// The shadow camera as Iris places it (7.1; measured from the outside): its view (player
/// space to shadow view space) and GL projection. The view undoes `sunPathRotation`, turns the
/// light's path about Z by the light's angle from the zenith, then X by 90 degrees, so the
/// light lies on the view's +Z. Its origin is `shadowIntervalSize / 2 - camera % interval`
/// (the remainder takes the camera's sign) from the player on each world axis, so the map moves
/// in whole steps. The orthographic projection covers `shadowDistance` each side and depths
/// from `-near` to `far`, counted from `SHADOW_EYE` blocks toward the light; or perspective
/// with `shadowMapFov`.
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
    let (near, far) = (shadow.near, shadow.far);
    let eye = Mat4::from_translation(Vec3::new(0.0, 0.0, -SHADOW_EYE));
    let projection = match shadow.fov {
        Some(fov) => Mat4::perspective_rh_gl(fov.to_radians(), 1.0, near, far) * eye,
        None => {
            let d = shadow.distance;
            Mat4::orthographic_rh_gl(-d, d, -d, d, -near, far) * eye
        }
    };
    (view, projection)
}

/// Extended camera uniform contract: public written API documentation only, not Iris code.
/// https://shaders.properties/current/reference/uniforms/camera/
/// Split BEFORE converting to f32, including negative positions (fraction stays in [0,1)).
fn split_position(position: DVec3) -> ([i32; 3], Vec3) {
    let integer = position.floor();
    // A fraction just below 1 can round up during the f64 -> f32 conversion.
    let fraction = (position - integer)
        .as_vec3()
        .min(Vec3::splat(f32::from_bits(1.0f32.to_bits() - 1)));
    (integer.as_ivec3().to_array(), fraction)
}

/// Keep horizontal float coordinates near the origin. The public camera contract permits
/// renderer-specific wrapping. Preserve the absolute coordinates for the split uniforms,
/// and subtract the new origin from BOTH frames, including after a teleport.
fn update_camera_origin(origin: &mut DVec3, camera: DVec3, previous: DVec3) {
    for axis in [0, 2] {
        if (camera[axis] - origin[axis]).abs() > 30_000.0
            || (camera[axis] - previous[axis]).abs() > 1_000.0
        {
            origin[axis] = camera[axis].floor();
        }
    }
}

/// `lightningBoltPosition`: the player-space (camera-relative, world axes) position of a bolt
/// the frame draws, with w = 1; all zero when none is drawn. Several bolts: the nearest to the
/// camera. The subtraction runs in f64 so far-from-origin bolts keep their sub-block offset.
fn lightning_bolt_position(bolts: &[super::lightning::Bolt], camera: DVec3) -> Vec4 {
    bolts
        .iter()
        .map(|bolt| DVec3::from_array(bolt.at) - camera)
        .min_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
        .map_or(Vec4::ZERO, |d| d.as_vec3().extend(1.0))
}

/// `endFlashPosition`: the flash's sky-axis centre in view space, rotated by the same view
/// matrix the sky is drawn with (the sun and moon above go through `to_view` the same way).
fn end_flash_view_position(view: &Mat4, sky_position: Vec3) -> Vec3 {
    view.transform_vector3(sky_position)
}

/// `value` moved toward `target` with a half-life of `half_life` seconds over `dt` (5.6).
fn smooth(value: f32, target: f32, half_life: f32, dt: f32) -> f32 {
    target + (value - target) * 0.5f32.powf(dt / half_life)
}

/// Keep the pack projection sparse: packs reconstruct depth from its diagonal and final
/// column. Camera bob belongs in model-view, preserving the world's P * bob * rotation.
fn camera_matrices(
    yaw: f32,
    pitch: f32,
    bob: Mat4,
    fov_degrees: f32,
    aspect: f32,
    far: f32,
) -> (Mat4, Mat4) {
    let view = bob * crate::camera::view_matrix(yaw, pitch);
    let projection = Mat4::perspective_rh_gl(fov_degrees.to_radians(), aspect, 0.05, far);
    (view, projection)
}

impl FrameUniforms {
    fn camera_history(
        &mut self,
        camera: DVec3,
        view: Mat4,
        projection: Mat4,
    ) -> (DVec3, Mat4, Mat4) {
        let previous = self.previous.unwrap_or((camera, view, projection));
        self.previous = Some((camera, view, projection));
        previous
    }

    /// This frame's values for a render size of `size` pixels.
    pub fn update(&mut self, frame: &Frame, inputs: &PackInputs, size: [u32; 2]) {
        let now = std::time::Instant::now();
        let dt = self.last.map_or(0.0, |l| (now - l).as_secs_f32());
        self.last = Some(now);
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let (view, projection) = camera_matrices(
            frame.yaw,
            frame.pitch,
            frame.bob * frame.spin,
            frame.fov_degrees,
            aspect,
            frame.depth_far,
        );
        let (previous_absolute, previous_view, previous_projection) =
            self.camera_history(frame.camera, view, projection);
        update_camera_origin(&mut self.camera_origin, frame.camera, previous_absolute);
        // Both frames use the SAME origin: changing origins must not create camera motion
        // in temporal reprojection. Geometry and gbuffer matrices are camera-relative.
        let camera = (frame.camera - self.camera_origin).as_vec3();
        let previous_camera = (previous_absolute - self.camera_origin).as_vec3();
        let (camera_int, camera_fract) = split_position(frame.camera);
        let (previous_int, previous_fract) = split_position(previous_absolute);

        let to_view = |v: Vec3| view.transform_vector3(v);
        let sun_angle = (frame.sky.sun_angle / 360.0 + 0.25).rem_euclid(1.0);
        let day = sun_angle < 0.5;
        let tilt = self.shadow.sun_path_rotation;
        let sun = to_view(body(frame.sky.sun_angle, tilt));
        let moon = to_view(body(frame.sky.moon_angle, tilt));
        // Half-lives are given in ticks (7.2); smoothing runs per frame in seconds (OQ-26).
        let half_life = if inputs.rain > self.wetness {
            600.0
        } else {
            200.0
        };
        self.wetness = smooth(self.wetness, inputs.rain, half_life / 20.0, dt);
        let eye = inputs.eye_light.map(|l| (l * 16) as f32);
        for (smoothed, target) in self.eye_smooth.iter_mut().zip(eye) {
            *smoothed = smooth(*smoothed, target, 10.0 / 20.0, dt);
        }
        // Vanilla fades by the stronger of two linear fogs; a pack gets one: the pair (start
        // and end together) of the one that is complete nearer (OQ-36). Mixing the nearer start
        // of one with the nearer end of the other made fog begin at the camera on clear days.
        let (fog_color, fog_start, fog_end) = match &frame.fog {
            Some(f) => {
                let (start, end) = if f.environmental_end < f.render_distance_end {
                    (f.environmental_start, f.environmental_end)
                } else {
                    (f.render_distance_start, f.render_distance_end)
                };
                ([f.color[0], f.color[1], f.color[2]], start, end)
            }
            None => (frame.sky.fog_color, frame.distance * 0.9, frame.distance),
        };
        let light = shadow_light(frame, &self.shadow);
        let (shadow_view, shadow_projection) = shadow_camera(light, frame.camera, &self.shadow);
        let day_time = inputs.day_time.rem_euclid(24000) as i32;
        let world_day = inputs.day_time.div_euclid(24000);
        let seconds = (now - self.start).as_secs_f32() % 3600.0;

        let lightning = lightning_bolt_position(frame.lightning.bolts, frame.camera);
        let end_flash = end_flash_view_position(&view, inputs.end_flash_position);

        let value = |name: &str| -> Value {
            let v3 = |v: Vec3| Value::f(&v.to_array());
            let m4 = |m: Mat4| Value::f(&m.to_cols_array());
            match name {
                "cameraPosition" => v3(camera),
                "previousCameraPosition" => v3(previous_camera),
                "cameraPositionInt" => Value::i(&camera_int),
                "cameraPositionFract" => v3(camera_fract),
                "previousCameraPositionInt" => Value::i(&previous_int),
                "previousCameraPositionFract" => v3(previous_fract),
                "eyePosition" => v3((inputs.eye_position - self.camera_origin).as_vec3()),
                "relativeEyePosition" => v3((frame.camera - inputs.eye_position).as_vec3()),
                "playerLookVector" | "playerBodyVector" => v3(inputs.player_look),
                "isElytraFlying" => Value::i(&[inputs.elytra_flying as i32]),
                "playerMood" => Value::f(&[inputs.player_mood]),
                "cloudHeight" => Value::f(&[inputs.cloud_height]),
                "lightningBoltPosition" => Value::f(&lightning.to_array()),
                "heavyFog" => Value::i(&[inputs.heavy_fog as i32]),
                "endFlashPosition" => v3(end_flash),
                "endFlashIntensity" => Value::f(&[inputs.end_flash_intensity]),
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
                "moonPosition" => v3(moon),
                "shadowLightPosition" => v3(if day { sun } else { moon }),
                "sunAngle" => Value::f(&[sun_angle]),
                "shadowAngle" => Value::f(&[if day { sun_angle } else { sun_angle - 0.5 }]),
                "worldTime" => Value::i(&[day_time]),
                "worldDay" => Value::i(&[world_day as i32]),
                "moonPhase" => Value::i(&[world_day.rem_euclid(8) as i32]),
                "frameCounter" => Value::i(&[self.frame as i32]),
                "shadowModelView" => m4(shadow_view),
                "shadowModelViewInverse" => m4(shadow_view.inverse()),
                "shadowProjection" => m4(shadow_projection),
                "shadowProjectionInverse" => m4(shadow_projection.inverse()),
                "atlasSize" => Value::i(&[self.atlas[0] as i32, self.atlas[1] as i32]),
                "frameTime" => Value::f(&[dt]),
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
                // GL_LINEAR: vanilla's fog is linear.
                "fogMode" => Value::i(&[9729]),
                "fogStart" => Value::f(&[fog_start]),
                "fogEnd" => Value::f(&[fog_end]),
                "fogDensity" => Value::f(&[1.0]),
                "fogShape" => Value::i(&[0]),
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
                "eyeBrightnessSmooth" => Value::i(&self.eye_smooth.map(|v| v.round() as i32)),
                "gbufferModelView" => m4(view),
                "gbufferModelViewInverse" => m4(view.inverse()),
                "gbufferProjection" => m4(projection),
                "gbufferProjectionInverse" => m4(projection.inverse()),
                "gbufferPreviousModelView" => m4(previous_view),
                "gbufferPreviousProjection" => m4(previous_projection),
                "heldItemId" => Value::i(&[inputs.held_items[0]]),
                "heldItemId2" => Value::i(&[inputs.held_items[1]]),
                "heldBlockLightValue" => Value::i(&[inputs.held_light[0]]),
                "heldBlockLightValue2" => Value::i(&[inputs.held_light[1]]),
                // renderStage is NONE outside gbuffers draws.
                _ => Value::i(&[0]),
            }
        };
        self.values.clear();
        self.values.extend(NAMES.iter().map(|n| value(n)));
        self.frame = (self.frame + 1) % 720_720;
    }

    /// `hz_fog_color` and `hz_fog` of the translator's block (the compatibility `gl_Fog`).
    pub fn fog(&self) -> [Vec4; 2] {
        let get = |name| match index(name).and_then(|i| self.values.get(i)) {
            Some(Value::F(f)) => *f,
            _ => [0.0; 16],
        };
        let (color, start, end) = (get("fogColor"), get("fogStart")[0], get("fogEnd")[0]);
        [
            Vec4::new(color[0], color[1], color[2], 1.0),
            Vec4::new(1.0, start, end, 1.0 / (end - start).max(1e-3)),
        ]
    }
}

impl FrameUniforms {
    /// Evaluates the pack's custom uniforms with this frame's values and puts them after
    /// `NAMES`' values, in `custom.uniforms()` order.
    pub fn eval_custom(&mut self, custom: &mut horizon_shaders::Custom) {
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
        | "endFlashPosition"
        | "fogColor"
        | "skyColor" => 3,
        "lightningBoltPosition" => 4,
        "eyeBrightness" | "eyeBrightnessSmooth" | "atlasSize" => 2,
        n if n.starts_with("gbuffer") || n.starts_with("shadow") && n != "shadowAngle" => 16,
        _ => 1,
    }
}

/// The position of `name` in [`NAMES`].
pub fn index(name: &str) -> Option<usize> {
    NAMES.iter().position(|n| *n == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walking_bob(phase: f32) -> Mat4 {
        Mat4::from_translation(Vec3::new(
            phase.sin() * 0.08,
            -phase.cos().abs() * 0.07,
            0.0,
        )) * Mat4::from_rotation_z(phase.sin() * 0.035)
            * Mat4::from_rotation_x(phase.cos() * 0.025)
    }

    // The sparse inverse-projection calculation used by Solas ToNDC and Complementary
    // ScreenToView: off-diagonal matrix elements are deliberately not read by those packs.
    fn pack_screen_to_view(projection: Mat4, ndc: Vec3) -> Vec3 {
        let inverse = projection.inverse();
        let diagonal = Vec4::new(
            inverse.x_axis.x,
            inverse.y_axis.y,
            inverse.z_axis.z,
            inverse.z_axis.w,
        );
        let homogeneous = diagonal * Vec4::new(ndc.x, ndc.y, ndc.z, ndc.z) + inverse.w_axis;
        homogeneous.truncate() / homogeneous.w
    }

    #[test]
    fn bobbed_pack_matrices_preserve_native_geometry_and_pure_projection() {
        let (yaw, pitch, fov, aspect, far) = (28.0, 17.0, 73.0_f32, 1.7, 512.0);
        let native_projection = Mat4::perspective_rh_gl(fov.to_radians(), aspect, 0.05, far);
        let rotation = crate::camera::view_matrix(yaw, pitch);
        for bob in [
            Mat4::IDENTITY,
            walking_bob(0.4),
            walking_bob(1.8),
            walking_bob(3.2),
        ] {
            let (view, projection) = camera_matrices(yaw, pitch, bob, fov, aspect, far);
            assert!(projection.abs_diff_eq(native_projection, 1e-6));
            assert!((projection * view).abs_diff_eq(native_projection * bob * rotation, 1e-5));
            assert!((view * view.inverse()).abs_diff_eq(Mat4::IDENTITY, 1e-5));
            assert!((projection * projection.inverse()).abs_diff_eq(Mat4::IDENTITY, 1e-5));
        }
    }

    #[test]
    fn walking_bob_keeps_pack_depth_reconstruction_correct() {
        let rotation = crate::camera::view_matrix(31.0, 12.0);
        let points = [
            Vec3::new(-1.0, 0.4, -4.0),
            Vec3::new(4.0, -2.0, -40.0),
            Vec3::new(-30.0, 10.0, -120.0),
        ];
        let mut old_split_error = 0.0_f32;
        for bob in [
            Mat4::IDENTITY,
            walking_bob(0.4),
            walking_bob(1.8),
            walking_bob(3.2),
        ] {
            let (view, projection) = camera_matrices(31.0, 12.0, bob, 73.0, 1.7, 512.0);
            for point in points {
                let world = rotation.inverse().transform_point3(point);
                let expected = view.transform_point3(world);
                let clip = projection * expected.extend(1.0);
                let ndc = clip.truncate() / clip.w;
                let reconstructed = pack_screen_to_view(projection, ndc);
                assert!(
                    reconstructed.abs_diff_eq(expected, 0.03),
                    "{reconstructed:?} != {expected:?}"
                );
                // Demonstrate that moving bob into projection, as before the repair, makes
                // the packs' sparse formula disagree with the rotation-only view position.
                let old_projection = projection * bob;
                old_split_error =
                    old_split_error.max(pack_screen_to_view(old_projection, ndc).distance(point));
            }
        }
        assert!(
            old_split_error > 0.5,
            "regression fixture did not exercise the old split: {old_split_error}"
        );
    }

    #[test]
    fn previous_camera_matrices_reproject_successive_bobbed_frames() {
        let mut uniforms = FrameUniforms::default();
        let world_point = Vec3::new(2.0, 1.0, 20.0);
        let mut expected_previous: Option<(DVec3, Mat4, Mat4)> = None;
        for (index, phase) in [0.4, 0.8, 1.3].into_iter().enumerate() {
            let camera = DVec3::new(index as f64 * 0.1, 0.0, index as f64 * 0.15);
            let (view, projection) = camera_matrices(
                3.0 + index as f32,
                4.0,
                walking_bob(phase),
                73.0,
                1.7,
                512.0,
            );
            let previous = uniforms.camera_history(camera, view, projection);
            let expected = expected_previous.unwrap_or((camera, view, projection));
            assert_eq!(previous, expected);
            let current_position = world_point - camera.as_vec3();
            let clip = projection * view * current_position.extend(1.0);
            let ndc = clip.truncate() / clip.w;
            let current_view = pack_screen_to_view(projection, ndc);
            let current_player = view.inverse().transform_point3(current_view);
            let previous_player = current_player + (camera - previous.0).as_vec3();
            let reprojected = previous.2 * previous.1 * previous_player.extend(1.0);
            let reference =
                previous.2 * previous.1 * (world_point - previous.0.as_vec3()).extend(1.0);
            assert!(
                (reprojected.truncate() / reprojected.w)
                    .abs_diff_eq(reference.truncate() / reference.w, 0.002)
            );
            expected_previous = Some((camera, view, projection));
        }
    }

    #[test]
    fn camera_split_preserves_negative_and_distant_sub_block_positions() {
        // f32 conversion before splitting loses these fractional blocks at world borders.
        for position in [
            DVec3::new(-0.25, -64.125, -30_000_000.75),
            DVec3::new(30_000_000.125, 320.5, 0.0),
        ] {
            let (integer, fraction) = split_position(position);
            assert!(fraction.cmpge(Vec3::ZERO).all());
            assert!(fraction.cmplt(Vec3::ONE).all());
            let reconstructed = DVec3::from_array(integer.map(f64::from)) + fraction.as_dvec3();
            assert_eq!(position, reconstructed);
        }
        assert_eq!(split_position(DVec3::splat(-0.25)).0, [-1; 3]);
        // Negative positions arbitrarily close to a block boundary must not round to 1.
        let (integer, fraction) = split_position(DVec3::splat(-1e-12));
        assert_eq!(integer, [-1; 3]);
        assert!(fraction.cmplt(Vec3::ONE).all());
    }

    #[test]
    fn rebasing_keeps_both_frames_in_the_same_coordinate_system() {
        let mut origin = DVec3::ZERO;
        let positions = [
            DVec3::new(29_999.875, 70.25, -29_999.875),
            DVec3::new(30_000.125, 70.5, -30_000.125),
            // A teleport also rebases even when still inside the walking threshold.
            DVec3::new(28_000.25, 64.0, -28_000.5),
        ];
        let mut previous = positions[0];
        for camera in positions {
            update_camera_origin(&mut origin, camera, previous);
            let current_relative = (camera - origin).as_vec3();
            let previous_relative = (previous - origin).as_vec3();
            assert!(current_relative.abs().max_element() <= 30_000.0);
            assert!(
                (current_relative - previous_relative)
                    .abs_diff_eq((camera - previous).as_vec3(), 1e-5)
            );
            // Absolute split history is unaffected by origin shifts.
            let (integer, fraction) = split_position(previous);
            assert_eq!(
                DVec3::from_array(integer.map(f64::from)) + fraction.as_dvec3(),
                previous
            );
            previous = camera;
        }
        assert_eq!(origin, DVec3::new(28_000.0, 0.0, -28_001.0));
    }

    #[test]
    fn new_vector_uniforms_are_available_to_custom_expressions() {
        for name in [
            "cameraPositionInt",
            "cameraPositionFract",
            "previousCameraPositionInt",
            "previousCameraPositionFract",
            "eyePosition",
            "relativeEyePosition",
            "playerLookVector",
        ] {
            assert!(index(name).is_some(), "missing {name}");
            assert_eq!(
                components(name),
                3,
                "custom expression would truncate {name}"
            );
        }
        assert_eq!(components("isElytraFlying"), 1);
    }

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

    #[test]
    fn lightning_position_is_zero_without_a_bolt_and_camera_relative_with_one() {
        use crate::gfx::lightning::Bolt;
        let camera = DVec3::new(8.5, 70.0, -5.5);
        assert_eq!(lightning_bolt_position(&[], camera), Vec4::ZERO);
        let bolt = |at: [f64; 3]| Bolt { at, seed: 1 };
        // World axes (not view axes), relative to the camera, with w = 1.
        assert_eq!(
            lightning_bolt_position(&[bolt([10.0, 64.0, -5.0])], camera),
            Vec4::new(1.5, -6.0, 0.5, 1.0)
        );
        // The nearest of several bolts, wherever it is in the list.
        let near = bolt([8.5, 70.0, 4.5]);
        let far = bolt([100.0, 70.0, 100.0]);
        for bolts in [[near, far], [far, near]] {
            assert_eq!(
                lightning_bolt_position(&bolts, camera),
                Vec4::new(0.0, 0.0, 10.0, 1.0)
            );
        }
        // A bolt on the camera still reports itself: w, not xyz, says it exists.
        assert_eq!(
            lightning_bolt_position(&[bolt(camera.to_array())], camera),
            Vec4::new(0.0, 0.0, 0.0, 1.0)
        );
        // Subtracting in f64 keeps a quarter block that f32 positions would lose.
        let far_camera = DVec3::new(30_000_000.25, 70.0, 0.0);
        assert_eq!(
            lightning_bolt_position(&[bolt([30_000_000.5, 70.0, 0.0])], far_camera),
            Vec4::new(0.25, 0.0, 0.0, 1.0)
        );
    }

    #[test]
    fn end_flash_position_is_in_view_space_and_the_new_uniforms_have_their_types() {
        // Facing south (yaw 0, vanilla's +Z): a flash straight south is dead ahead (-Z in
        // view space), one to the east is on the left, one overhead stays up.
        let south = crate::camera::view_matrix(0.0, 0.0);
        let at = |view: &Mat4, sky: Vec3| end_flash_view_position(view, sky);
        let close = |a: Vec3, b: Vec3| assert!(a.abs_diff_eq(b, 1e-3), "{a} != {b}");
        close(
            at(&south, Vec3::new(0.0, 0.0, 100.0)),
            Vec3::new(0.0, 0.0, -100.0),
        );
        close(
            at(&south, Vec3::new(100.0, 0.0, 0.0)),
            Vec3::new(-100.0, 0.0, 0.0),
        );
        close(
            at(&south, Vec3::new(0.0, 100.0, 0.0)),
            Vec3::new(0.0, 100.0, 0.0),
        );
        // Turning to face west puts a flash in the west ahead of the player.
        let west = crate::camera::view_matrix(90.0, 0.0);
        close(
            at(&west, Vec3::new(-100.0, 0.0, 0.0)),
            Vec3::new(0.0, 0.0, -100.0),
        );
        // Same rotation the sun's view-space position takes (`to_view`), nothing else.
        let sky = Vec3::new(30.0, 60.0, -70.0);
        assert_eq!(at(&west, sky), west.transform_vector3(sky));
        // The default (no pack, or outside the End) is a zero flash.
        let none = PackInputs::default();
        assert_eq!(at(&west, none.end_flash_position), Vec3::ZERO);
        assert_eq!(none.end_flash_intensity, 0.0);

        for (name, count) in [
            ("lightningBoltPosition", 4),
            ("heavyFog", 1),
            ("endFlashPosition", 3),
            ("endFlashIntensity", 1),
        ] {
            assert!(index(name).is_some(), "missing {name}");
            assert_eq!(components(name), count, "custom expressions see {name}");
        }
        // heavyFog is an int that also reads as a declared float or bool; the vec4 is whole.
        let mut out = vec![0u8; 32];
        Value::i(&[1]).write("float", &mut out, 0);
        assert_eq!(f32::from_ne_bytes(out[0..4].try_into().unwrap()), 1.0);
        Value::i(&[1]).write("bool", &mut out, 4);
        assert_eq!(i32::from_ne_bytes(out[4..8].try_into().unwrap()), 1);
        Value::f(&[1.5, -6.0, 0.5, 1.0]).write("vec4", &mut out, 16);
        let w = f32::from_ne_bytes(out[28..32].try_into().unwrap());
        assert_eq!(w, 1.0);
    }

    #[test]
    fn cloud_height_needs_a_cloud_color() {
        assert_eq!(cloud_height(0.8, 192.33), 192.33);
        assert_eq!(cloud_height(0.8, 120.0), 120.0);
        assert!(cloud_height(0.0, 192.33).is_nan());
    }

    #[test]
    fn elytra_and_cloud_height_uniforms_are_written() {
        assert!(NAMES.contains(&"isElytraFlying") && NAMES.contains(&"cloudHeight"));
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
