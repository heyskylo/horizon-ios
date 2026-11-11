//! Entity model layers built in code, as vanilla's
//! `net.minecraft.client.model.geom.builders` build them: a mod's
//! `createBodyLayer()` (a [`MeshDefinition`] of named parts, each a
//! [`CubeListBuilder`]'s boxes at a [`PartPose`], made a [`LayerDefinition`]
//! with its texture's size) bakes into a [`ModelPart`] tree
//! (`LayerDefinition.bakeRoot`). A mod turns its parts each frame, as
//! `setupAnim` does, and draws it with `native_draw_model_part`
//! (`ModelPart.render`). Mods register their layers by id
//! (`native_layer_definitions`, NeoForge's
//! `EntityRenderersEvent.RegisterLayerDefinitions`), and `native_bake_layer`
//! bakes one of them or a vanilla layer (`EntityModelSet.bakeLayer`).

use super::Part;
use horizon_core::geometry::Direction;
use std::{collections::HashMap, sync::OnceLock};

/// `CubeDeformation`: how far a box grows past its size on each side, in
/// texels, with its texture unchanged: a hat or an armour layer over a head.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CubeDeformation {
    pub grow_x: f32,
    pub grow_y: f32,
    pub grow_z: f32,
}

impl CubeDeformation {
    pub const NONE: Self = Self::xyz(0.0, 0.0, 0.0);

    /// The same growth on every axis (`new CubeDeformation(grow)`).
    pub const fn new(grow: f32) -> Self {
        Self::xyz(grow, grow, grow)
    }

    pub const fn xyz(grow_x: f32, grow_y: f32, grow_z: f32) -> Self {
        Self {
            grow_x,
            grow_y,
            grow_z,
        }
    }

    /// This growth and `by` more on every axis (`extend`).
    pub fn extend(self, by: f32) -> Self {
        self.extend_xyz(by, by, by)
    }

    pub fn extend_xyz(self, x: f32, y: f32, z: f32) -> Self {
        Self::xyz(self.grow_x + x, self.grow_y + y, self.grow_z + z)
    }
}

/// `PartPose`: where a part sits in its parent, in texels from the parent's
/// pivot (+y down, as a model's frame is), its rotation in radians (about z,
/// then y, then x, as `ModelPart.translateAndRotate` turns it) and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartPose {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub x_rot: f32,
    pub y_rot: f32,
    pub z_rot: f32,
    pub x_scale: f32,
    pub y_scale: f32,
    pub z_scale: f32,
}

impl Default for PartPose {
    fn default() -> Self {
        Self::ZERO
    }
}

impl PartPose {
    pub const ZERO: Self = Self::offset_and_rotation(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

    pub const fn offset(x: f32, y: f32, z: f32) -> Self {
        Self::offset_and_rotation(x, y, z, 0.0, 0.0, 0.0)
    }

    pub const fn rotation(x_rot: f32, y_rot: f32, z_rot: f32) -> Self {
        Self::offset_and_rotation(0.0, 0.0, 0.0, x_rot, y_rot, z_rot)
    }

    pub const fn offset_and_rotation(
        x: f32,
        y: f32,
        z: f32,
        x_rot: f32,
        y_rot: f32,
        z_rot: f32,
    ) -> Self {
        Self {
            x,
            y,
            z,
            x_rot,
            y_rot,
            z_rot,
            x_scale: 1.0,
            y_scale: 1.0,
            z_scale: 1.0,
        }
    }

    /// This pose moved by `x`, `y` and `z` texels (`translated`).
    pub fn translated(self, x: f32, y: f32, z: f32) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
            z: self.z + z,
            ..self
        }
    }

    /// This pose at `scale` on every axis (`withScale`).
    pub fn with_scale(self, scale: f32) -> Self {
        Self {
            x_scale: scale,
            y_scale: scale,
            z_scale: scale,
            ..self
        }
    }
}

/// One box of a part (`CubeDefinition`), as a [`CubeListBuilder`] adds it.
#[derive(Clone, Debug, PartialEq)]
pub struct CubeDefinition {
    /// Its corner at the least x, y and z, in texels from the part's pivot.
    pub origin: [f32; 3],
    /// Its size in texels, which is also the size of its faces on the texture.
    pub dimensions: [f32; 3],
    pub grow: CubeDeformation,
    /// Drawn with its texture flipped left to right, as a left limb draws a
    /// right one's.
    pub mirror: bool,
    /// The top left corner of its faces on the texture, in texels (`texOffs`).
    pub tex_coord: [i32; 2],
    /// How many of the texture's texels one of its own texels takes, 1 for most.
    pub tex_scale: [f32; 2],
    /// The faces it has, in [`Direction::ALL`]'s order; all six for most.
    pub visible_faces: [bool; 6],
}

/// `CubeListBuilder`: the boxes of one part, each placed on the texture from
/// the last `tex_offs` and mirrored after `mirror`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CubeListBuilder {
    cubes: Vec<CubeDefinition>,
    tex: [i32; 2],
    mirror: bool,
}

impl CubeListBuilder {
    pub fn create() -> Self {
        Self::default()
    }

    /// Where the next boxes' faces start on the texture, in texels (`texOffs`).
    pub fn tex_offs(mut self, u: i32, v: i32) -> Self {
        self.tex = [u, v];
        self
    }

    /// The next boxes are mirrored (`mirror()`).
    pub fn mirror(self) -> Self {
        self.set_mirror(true)
    }

    /// Whether the next boxes are mirrored (`mirror(boolean)`).
    pub fn set_mirror(mut self, mirror: bool) -> Self {
        self.mirror = mirror;
        self
    }

    /// A box at `origin` of `size` texels (`addBox(x, y, z, w, h, d)`).
    pub fn add_box(self, origin: [f32; 3], size: [f32; 3]) -> Self {
        self.add_box_with(origin, size, CubeDeformation::NONE)
    }

    /// A box grown by `grow` (`addBox(x, y, z, w, h, d, deformation)`).
    pub fn add_box_with(self, origin: [f32; 3], size: [f32; 3], grow: CubeDeformation) -> Self {
        self.add_box_tex_scaled(origin, size, grow, [1.0, 1.0])
    }

    /// A box mirrored or not by `mirror`, whatever `mirror()` said
    /// (`addBox(x, y, z, w, h, d, mirror)`).
    pub fn add_box_mirrored(mut self, origin: [f32; 3], size: [f32; 3], mirror: bool) -> Self {
        let tex = self.tex;
        self.cubes
            .push(cube(origin, size, CubeDeformation::NONE, mirror, tex));
        self
    }

    /// A box with only `faces` (`addBox(x, y, z, w, h, d, visibleFaces)`).
    pub fn add_box_faces(mut self, origin: [f32; 3], size: [f32; 3], faces: &[Direction]) -> Self {
        let mut cube = cube(origin, size, CubeDeformation::NONE, self.mirror, self.tex);
        cube.visible_faces = Direction::ALL.map(|d| faces.contains(&d));
        self.cubes.push(cube);
        self
    }

    /// A box whose texels take `tex_scale` of the texture's
    /// (`addBox(x, y, z, w, h, d, deformation, texScaleU, texScaleV)`).
    pub fn add_box_tex_scaled(
        mut self,
        origin: [f32; 3],
        size: [f32; 3],
        grow: CubeDeformation,
        tex_scale: [f32; 2],
    ) -> Self {
        let mut cube = cube(origin, size, grow, self.mirror, self.tex);
        cube.tex_scale = tex_scale;
        self.cubes.push(cube);
        self
    }

    /// A box with its own place on the texture, leaving `tex_offs` as it is
    /// (`addBox(comment, x, y, z, w, h, d, deformation, texU, texV)`).
    pub fn add_box_at(
        mut self,
        origin: [f32; 3],
        size: [f32; 3],
        grow: CubeDeformation,
        tex: [i32; 2],
    ) -> Self {
        self.cubes.push(cube(origin, size, grow, self.mirror, tex));
        self
    }

    /// The boxes added so far (`getCubes`).
    pub fn cubes(&self) -> &[CubeDefinition] {
        &self.cubes
    }
}

fn cube(
    origin: [f32; 3],
    dimensions: [f32; 3],
    grow: CubeDeformation,
    mirror: bool,
    tex_coord: [i32; 2],
) -> CubeDefinition {
    CubeDefinition {
        origin,
        dimensions,
        grow,
        mirror,
        tex_coord,
        tex_scale: [1.0, 1.0],
        visible_faces: [true; 6],
    }
}

impl CubeDefinition {
    /// `ModelPart.Cube`: the box's faces on a `texture` of that many texels,
    /// in vanilla's order (down, up, west, north, east, south), each face's
    /// corners and texture net as `Cube`'s constructor makes them.
    fn bake_into(&self, texture: [f32; 2], out: &mut Vec<ModelPolygon>) {
        let [w, h, d] = self.dimensions;
        let [mut x0, mut y0, mut z0] = self.origin;
        let (mut x1, mut y1, mut z1) = (x0 + w, y0 + h, z0 + d);
        let g = self.grow;
        x0 -= g.grow_x;
        y0 -= g.grow_y;
        z0 -= g.grow_z;
        x1 += g.grow_x;
        y1 += g.grow_y;
        z1 += g.grow_z;
        if self.mirror {
            std::mem::swap(&mut x0, &mut x1);
        }
        // The constructor's eight vertices, v7 v v1 v2 v3 v4 v5 v6.
        let corner = [
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ];
        let tex_w = texture[0] * self.tex_scale[0];
        let tex_h = texture[1] * self.tex_scale[1];
        let [u, v] = self.tex_coord.map(|t| t as f32);
        let (u1, u2, u3) = (u + d, u + d + w, u + d + w + w);
        let (u4, u5) = (u + d + w + d, u + d + w + d + w);
        let (v1, v2) = (v + d, v + d + h);
        let faces = [
            (Direction::Down, [5, 4, 0, 1], [u1, v, u2, v1]),
            (Direction::Up, [2, 3, 7, 6], [u2, v1, u3, v]),
            (Direction::West, [0, 4, 7, 3], [u, v1, u1, v2]),
            (Direction::North, [1, 0, 3, 2], [u1, v1, u2, v2]),
            (Direction::East, [5, 1, 2, 6], [u2, v1, u4, v2]),
            (Direction::South, [4, 5, 6, 7], [u4, v1, u5, v2]),
        ];
        for (direction, corners, [ua, va, ub, vb]) in faces {
            let shown = Direction::ALL
                .iter()
                .position(|d| *d == direction)
                .is_some_and(|i| self.visible_faces[i]);
            if !shown {
                continue;
            }
            // `Polygon`'s remap: right top, left top, left bottom, right bottom.
            let uv = [(ub, va), (ua, va), (ua, vb), (ub, vb)];
            let mut vertices = std::array::from_fn(|k| ModelVertex {
                pos: corner[corners[k]],
                u: uv[k].0 / tex_w,
                v: uv[k].1 / tex_h,
            });
            let [nx, ny, nz] = direction.normal();
            let mut normal = [nx as f32, ny as f32, nz as f32];
            if self.mirror {
                vertices.reverse();
                normal[0] = -normal[0];
            }
            out.push(ModelPolygon { vertices, normal });
        }
    }
}

/// A part of a [`MeshDefinition`], as `add_or_replace_child` gives it
/// (`PartDefinition`): a handle into that mesh only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PartId(usize);

#[derive(Clone, Debug, PartialEq)]
struct PartNode {
    cubes: Vec<CubeDefinition>,
    pose: PartPose,
    children: Vec<(String, usize)>,
}

/// `MeshDefinition`: a model's parts, from a root part with no boxes.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshDefinition {
    parts: Vec<PartNode>,
}

impl Default for MeshDefinition {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshDefinition {
    pub fn new() -> Self {
        Self {
            parts: vec![PartNode {
                cubes: Vec::new(),
                pose: PartPose::ZERO,
                children: Vec::new(),
            }],
        }
    }

    /// The root part (`getRoot`).
    pub fn root(&self) -> PartId {
        PartId(0)
    }

    /// Adds part `name` to `parent`, with `cubes` at `pose`
    /// (`PartDefinition.addOrReplaceChild`). A part of that name already
    /// there is replaced and its children kept.
    pub fn add_or_replace_child(
        &mut self,
        parent: PartId,
        name: &str,
        cubes: CubeListBuilder,
        pose: PartPose,
    ) -> PartId {
        let node = PartNode {
            cubes: cubes.cubes,
            pose,
            children: Vec::new(),
        };
        if let Some(at) = self.child(parent, name) {
            let old = std::mem::replace(&mut self.parts[at.0], node);
            self.parts[at.0].children = old.children;
            return at;
        }
        self.parts.push(node);
        let at = self.parts.len() - 1;
        self.parts[parent.0].children.push((name.to_owned(), at));
        PartId(at)
    }

    /// `parent`'s part `name` (`getChild`).
    pub fn child(&self, parent: PartId, name: &str) -> Option<PartId> {
        self.parts[parent.0]
            .children
            .iter()
            .find(|(n, _)| n == name)
            .map(|&(_, at)| PartId(at))
    }

    fn bake(&self, at: usize, texture: [f32; 2]) -> ModelPart {
        let node = &self.parts[at];
        let mut polygons = Vec::new();
        for cube in &node.cubes {
            cube.bake_into(texture, &mut polygons);
        }
        let children = node
            .children
            .iter()
            .map(|(name, child)| (name.clone(), self.bake(*child, texture)))
            .collect();
        ModelPart::new(polygons, children, node.pose)
    }
}

/// `LayerDefinition`: a mesh and the size of the texture its boxes are on.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerDefinition {
    pub mesh: MeshDefinition,
    pub texture_width: u32,
    pub texture_height: u32,
}

impl LayerDefinition {
    /// `LayerDefinition.create(mesh, 64, 64)`.
    pub fn create(mesh: MeshDefinition, texture_width: u32, texture_height: u32) -> Self {
        Self {
            mesh,
            texture_width,
            texture_height,
        }
    }

    /// The part tree, each part at its pose (`bakeRoot`).
    pub fn bake_root(&self) -> ModelPart {
        let texture = [self.texture_width as f32, self.texture_height as f32];
        self.mesh.bake(0, texture)
    }
}

/// A corner of a baked face (`ModelPart.Vertex`): in texels in its part's
/// frame, and where it falls on the texture, 0 to 1 across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelVertex {
    pub pos: [f32; 3],
    pub u: f32,
    pub v: f32,
}

/// A baked face (`ModelPart.Polygon`), with the normal it faces in its
/// part's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelPolygon {
    pub vertices: [ModelVertex; 4],
    pub normal: [f32; 3],
}

/// `ModelPart`: a baked part, its children by name, and the pose a mod sets
/// each frame before drawing it (`setupAnim`), from the pose it was baked
/// with (`reset_pose`).
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPart {
    /// Where it sits in its parent, in texels.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Its rotation in radians, applied about z, then y, then x.
    pub x_rot: f32,
    pub y_rot: f32,
    pub z_rot: f32,
    pub x_scale: f32,
    pub y_scale: f32,
    pub z_scale: f32,
    /// False draws neither it nor its children.
    pub visible: bool,
    /// True draws its children but not its own faces (`skipDraw`).
    pub skip_draw: bool,
    /// Its boxes' faces.
    pub polygons: Vec<ModelPolygon>,
    children: Vec<(String, ModelPart)>,
    initial_pose: PartPose,
}

impl ModelPart {
    /// A part with `polygons` and `children` at `pose`, which it is reset to.
    pub fn new(
        polygons: Vec<ModelPolygon>,
        children: Vec<(String, ModelPart)>,
        pose: PartPose,
    ) -> Self {
        let mut part = Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            x_rot: 0.0,
            y_rot: 0.0,
            z_rot: 0.0,
            x_scale: 1.0,
            y_scale: 1.0,
            z_scale: 1.0,
            visible: true,
            skip_draw: false,
            polygons,
            children,
            initial_pose: pose,
        };
        part.load_pose(pose);
        part
    }

    /// A vanilla layer's part tree from `entity-models.json`.
    pub fn from_part(part: &Part) -> Self {
        let p = part.pose;
        let pose = PartPose {
            x: p[0],
            y: p[1],
            z: p[2],
            x_rot: p[3],
            y_rot: p[4],
            z_rot: p[5],
            x_scale: p[6],
            y_scale: p[7],
            z_scale: p[8],
        };
        let polygons = part
            .polygons
            .iter()
            .map(|poly| ModelPolygon {
                vertices: poly.vertices.map(|v| ModelVertex {
                    pos: [v[0], v[1], v[2]],
                    u: v[3],
                    v: v[4],
                }),
                normal: poly.normal,
            })
            .collect();
        let children = part
            .children
            .iter()
            .map(|c| (c.name.clone(), Self::from_part(c)))
            .collect();
        Self::new(polygons, children, pose)
    }

    /// Its part `name` (`getChild`).
    pub fn get_child(&self, name: &str) -> Option<&ModelPart> {
        self.children
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)
    }

    pub fn get_child_mut(&mut self, name: &str) -> Option<&mut ModelPart> {
        self.children
            .iter_mut()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)
    }

    /// Its parts by name, in the order they draw.
    pub fn children(&self) -> impl Iterator<Item = (&str, &ModelPart)> {
        self.children.iter().map(|(n, c)| (n.as_str(), c))
    }

    pub fn children_mut(&mut self) -> impl Iterator<Item = (&str, &mut ModelPart)> {
        self.children.iter_mut().map(|(n, c)| (n.as_str(), c))
    }

    /// Whether it has no faces of its own (`isEmpty`).
    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    /// Its pose now (`storePose`).
    pub fn store_pose(&self) -> PartPose {
        PartPose {
            x: self.x,
            y: self.y,
            z: self.z,
            x_rot: self.x_rot,
            y_rot: self.y_rot,
            z_rot: self.z_rot,
            x_scale: self.x_scale,
            y_scale: self.y_scale,
            z_scale: self.z_scale,
        }
    }

    /// Puts it at `pose` (`loadPose`).
    pub fn load_pose(&mut self, pose: PartPose) {
        self.x = pose.x;
        self.y = pose.y;
        self.z = pose.z;
        self.x_rot = pose.x_rot;
        self.y_rot = pose.y_rot;
        self.z_rot = pose.z_rot;
        self.x_scale = pose.x_scale;
        self.y_scale = pose.y_scale;
        self.z_scale = pose.z_scale;
    }

    /// The pose it was baked with, unless set since (`getInitialPose`).
    pub fn initial_pose(&self) -> PartPose {
        self.initial_pose
    }

    pub fn set_initial_pose(&mut self, pose: PartPose) {
        self.initial_pose = pose;
    }

    /// Puts it back at its initial pose (`resetPose`), its children as they are.
    pub fn reset_pose(&mut self) {
        self.load_pose(self.initial_pose);
    }

    /// Puts it and every part under it back at their initial poses, as
    /// `Model.resetPose` does before each frame's `setupAnim`.
    pub fn reset_all_poses(&mut self) {
        self.reset_pose();
        for (_, child) in &mut self.children {
            child.reset_all_poses();
        }
    }

    /// Takes `other`'s position, rotation and scale (`copyFrom`).
    pub fn copy_from(&mut self, other: &ModelPart) {
        self.load_pose(other.store_pose());
    }

    pub fn set_pos(&mut self, x: f32, y: f32, z: f32) {
        (self.x, self.y, self.z) = (x, y, z);
    }

    pub fn set_rotation(&mut self, x_rot: f32, y_rot: f32, z_rot: f32) {
        (self.x_rot, self.y_rot, self.z_rot) = (x_rot, y_rot, z_rot);
    }

    /// Moves it by `by` texels (`offsetPos`), as keyframe animations do.
    pub fn offset_pos(&mut self, by: [f32; 3]) {
        self.x += by[0];
        self.y += by[1];
        self.z += by[2];
    }

    /// Turns it by `by` radians (`offsetRotation`).
    pub fn offset_rotation(&mut self, by: [f32; 3]) {
        self.x_rot += by[0];
        self.y_rot += by[1];
        self.z_rot += by[2];
    }

    /// Scales it by `by` more (`offsetScale`: added, as vanilla adds it).
    pub fn offset_scale(&mut self, by: [f32; 3]) {
        self.x_scale += by[0];
        self.y_scale += by[1];
        self.z_scale += by[2];
    }
}

static REGISTERED: OnceLock<HashMap<String, LayerDefinition>> = OnceLock::new();

/// Keeps the layers native mods registered (`native_layer_definitions`),
/// once at start. An id is `<namespace>:<path>#<layer>` (NeoForge's
/// `ModelLayerLocation`) in a namespace other than vanilla's; one that isn't,
/// or that another layer has already, is logged and left out.
pub fn register(layers: Vec<(String, LayerDefinition)>) {
    let mut kept = HashMap::new();
    for (id, layer) in layers {
        if !is_mod_layer_id(&id) {
            log::warn!("model layer {id:?} is not <namespace>:<path>#<layer> in a mod's namespace");
            continue;
        }
        if kept.contains_key(&id) {
            log::warn!("model layer {id} is registered twice; the first is kept");
            continue;
        }
        kept.insert(id, layer);
    }
    if REGISTERED.set(kept).is_err() {
        log::warn!("model layers were registered already; these are left out");
    }
}

/// The layer a native mod registered as `id`.
pub fn registered(id: &str) -> Option<&'static LayerDefinition> {
    REGISTERED.get()?.get(id)
}

fn is_mod_layer_id(id: &str) -> bool {
    let Some((location, layer)) = id.split_once('#') else {
        return false;
    };
    let Some((namespace, path)) = location.split_once(':') else {
        return false;
    };
    let ok = |s: &str, extra: &[char]| {
        !s.is_empty()
            && s.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || "_.-".contains(c)
                    || extra.contains(&c)
            })
    };
    namespace != "minecraft"
        && ok(namespace, &[])
        && ok(path, &['/'])
        && !layer.is_empty()
        && !layer.contains(['#', ':'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polygon(part: &ModelPart, i: usize) -> ([[f32; 3]; 4], [[f32; 2]; 4], [f32; 3]) {
        let p = &part.polygons[i];
        (
            p.vertices.map(|v| v.pos),
            p.vertices.map(|v| [v.u, v.v]),
            p.normal,
        )
    }

    /// One 8-texel head box on a 64 by 64 texture (`HumanoidModel`'s head),
    /// worked out by hand from `ModelPart.Cube`'s constructor: its top face
    /// is the net's first strip, 8 to 16 across and 0 to 8 down, at the
    /// box's least y (a model's +y is down).
    #[test]
    fn a_box_bakes_its_faces_on_the_texture_as_vanilla_cubes_do() {
        let mut mesh = MeshDefinition::new();
        let root = mesh.root();
        let cubes = CubeListBuilder::create()
            .tex_offs(0, 0)
            .add_box([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]);
        mesh.add_or_replace_child(root, "head", cubes, PartPose::offset(0.0, 2.0, 0.0));
        let baked = LayerDefinition::create(mesh, 64, 64).bake_root();
        let head = baked.get_child("head").unwrap();
        assert_eq!(head.polygons.len(), 6);
        assert_eq!((head.x, head.y, head.z), (0.0, 2.0, 0.0));

        let (pos, uv, normal) = polygon(head, 0);
        assert_eq!(normal, [0.0, -1.0, 0.0]);
        assert_eq!(
            pos,
            [
                [4.0, -8.0, 4.0],
                [-4.0, -8.0, 4.0],
                [-4.0, -8.0, -4.0],
                [4.0, -8.0, -4.0]
            ]
        );
        assert_eq!(
            uv,
            [[0.25, 0.0], [0.125, 0.0], [0.125, 0.125], [0.25, 0.125]]
        );

        // The face, north (-z): 8 to 16 across, 8 to 16 down.
        let (pos, uv, normal) = polygon(head, 3);
        assert_eq!(normal, [0.0, 0.0, -1.0]);
        assert_eq!(
            pos,
            [
                [4.0, -8.0, -4.0],
                [-4.0, -8.0, -4.0],
                [-4.0, 0.0, -4.0],
                [4.0, 0.0, -4.0]
            ]
        );
        assert_eq!(
            uv,
            [[0.25, 0.125], [0.125, 0.125], [0.125, 0.25], [0.25, 0.25]]
        );

        // The back, south: the net's last strip, 24 to 32 across.
        let (_, uv, normal) = polygon(head, 5);
        assert_eq!(normal, [0.0, 0.0, 1.0]);
        assert_eq!(
            uv,
            [[0.5, 0.125], [0.375, 0.125], [0.375, 0.25], [0.5, 0.25]]
        );
    }

    #[test]
    fn mirroring_swaps_the_sides_and_growing_keeps_the_texture() {
        let plain = CubeListBuilder::create()
            .tex_offs(40, 16)
            .add_box([-1.0, -2.0, -2.0], [4.0, 12.0, 4.0]);
        let mirrored = CubeListBuilder::create()
            .tex_offs(40, 16)
            .mirror()
            .add_box([-1.0, -2.0, -2.0], [4.0, 12.0, 4.0]);
        let grown = CubeListBuilder::create().tex_offs(40, 16).add_box_with(
            [-1.0, -2.0, -2.0],
            [4.0, 12.0, 4.0],
            CubeDeformation::new(0.25),
        );
        let bake = |cubes| {
            let mut mesh = MeshDefinition::new();
            let root = mesh.root();
            mesh.add_or_replace_child(root, "arm", cubes, PartPose::ZERO);
            LayerDefinition::create(mesh, 64, 64)
                .bake_root()
                .get_child("arm")
                .unwrap()
                .clone()
        };
        let (plain, mirrored, grown) = (bake(plain), bake(mirrored), bake(grown));

        // West (index 2): mirrored, its corners sit at the other x and
        // wind the other way, and it faces east; its net stays the same.
        let (pos, uv, normal) = polygon(&plain, 2);
        let (mpos, muv, mnormal) = polygon(&mirrored, 2);
        assert_eq!(normal, [-1.0, 0.0, 0.0]);
        assert_eq!(mnormal, [1.0, 0.0, 0.0]);
        assert_eq!(pos[0], [-1.0, -2.0, -2.0]);
        assert_eq!(mpos[3], [3.0, -2.0, -2.0]);
        assert_eq!(muv, [uv[3], uv[2], uv[1], uv[0]]);

        // Grown a quarter texel on each side, with the same texture.
        let (gpos, guv, _) = polygon(&grown, 2);
        assert_eq!(gpos[0], [-1.25, -2.25, -2.25]);
        assert_eq!(guv, uv);
    }

    #[test]
    fn hidden_faces_and_texture_scale_change_what_bakes() {
        let cubes = CubeListBuilder::create()
            .tex_offs(0, 0)
            .add_box_faces([0.0; 3], [2.0, 2.0, 2.0], &[Direction::North])
            .add_box_tex_scaled([0.0; 3], [2.0, 2.0, 2.0], CubeDeformation::NONE, [2.0, 2.0]);
        let mut mesh = MeshDefinition::new();
        let root = mesh.root();
        mesh.add_or_replace_child(root, "p", cubes, PartPose::ZERO);
        let part = LayerDefinition::create(mesh, 16, 16).bake_root();
        let part = part.get_child("p").unwrap();
        // One face of the first box, six of the second.
        assert_eq!(part.polygons.len(), 7);
        assert_eq!(part.polygons[0].normal, [0.0, 0.0, -1.0]);
        // North of the second: 2 to 4 across, on a texture taken as 32 wide.
        let (_, uv, _) = polygon(part, 4);
        assert_eq!(uv[0], [4.0 / 32.0, 2.0 / 32.0]);
    }

    #[test]
    fn a_replaced_part_keeps_its_children_and_poses_reset() {
        let mut mesh = MeshDefinition::new();
        let root = mesh.root();
        let body =
            mesh.add_or_replace_child(root, "body", CubeListBuilder::create(), PartPose::ZERO);
        mesh.add_or_replace_child(
            body,
            "arm",
            CubeListBuilder::create(),
            PartPose::offset(5.0, 2.0, 0.0),
        );
        let again = mesh.add_or_replace_child(
            root,
            "body",
            CubeListBuilder::create().add_box([0.0; 3], [1.0; 3]),
            PartPose::rotation(0.5, 0.0, 0.0),
        );
        assert_eq!(again, body);
        assert!(mesh.child(body, "arm").is_some());

        let mut model = LayerDefinition::create(mesh, 64, 32).bake_root();
        let body = model.get_child_mut("body").unwrap();
        assert_eq!(body.polygons.len(), 6);
        assert_eq!(body.x_rot, 0.5);
        body.x_rot = 1.5;
        let arm = body.get_child_mut("arm").unwrap();
        arm.offset_pos([1.0, 0.0, 0.0]);
        assert_eq!(arm.x, 6.0);
        model.reset_all_poses();
        let body = model.get_child("body").unwrap();
        assert_eq!(body.x_rot, 0.5);
        assert_eq!(body.get_child("arm").unwrap().x, 5.0);
    }

    #[test]
    fn layer_ids_are_a_mods_own_model_and_layer() {
        assert!(is_mod_layer_id("mymod:golem#main"));
        assert!(is_mod_layer_id("my_mod:mobs/golem.v2#outer_armor"));
        for bad in [
            "minecraft:zombie#main",
            "mymod:golem",
            "golem#main",
            "mymod:#main",
            "mymod:golem#",
            "MyMod:golem#main",
            "mymod:golem#a#b",
        ] {
            assert!(!is_mod_layer_id(bad), "{bad}");
        }
    }

    /// `HumanoidModel.createMesh(CubeDeformation.NONE, 0)` built with these
    /// builders bakes what vanilla baked for the zombie
    /// (`minecraft:zombie#main` in the prepared `entity-models.json`):
    /// `cargo test -p horizon-client-lib --lib model_layers -- --ignored`
    /// with HORIZON_TEST_RESOURCES set to the prepared resources.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn the_humanoid_mesh_bakes_as_vanillas_zombie() {
        let root = std::path::PathBuf::from(
            std::env::var_os("HORIZON_TEST_RESOURCES").expect("HORIZON_TEST_RESOURCES"),
        );
        let vanilla =
            super::super::load_model(&root, "minecraft:zombie#main").expect("zombie#main");
        let vanilla = ModelPart::from_part(&vanilla);

        // `HumanoidModel.createMesh`: (part, parent, texOffs, mirror, origin, size, grow, pose).
        let half = CubeDeformation::new(0.5);
        let none = CubeDeformation::NONE;
        let (z4, limb) = ([-4.0, -8.0, -4.0], [4.0, 12.0, 4.0]);
        #[rustfmt::skip]
        let parts = [
            ("head", "", (0, 0), false, z4, [8.0; 3], none, [0.0; 3]),
            ("hat", "head", (32, 0), false, z4, [8.0; 3], half, [0.0; 3]),
            ("body", "", (16, 16), false, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], none, [0.0; 3]),
            ("right_arm", "", (40, 16), false, [-3.0, -2.0, -2.0], limb, none, [-5.0, 2.0, 0.0]),
            ("left_arm", "", (40, 16), true, [-1.0, -2.0, -2.0], limb, none, [5.0, 2.0, 0.0]),
            ("right_leg", "", (0, 16), false, [-2.0, 0.0, -2.0], limb, none, [-1.9, 12.0, 0.0]),
            ("left_leg", "", (0, 16), true, [-2.0, 0.0, -2.0], limb, none, [1.9, 12.0, 0.0]),
        ];
        let mut mesh = MeshDefinition::new();
        for (name, parent, (u, v), mirror, origin, size, grow, [x, y, z]) in parts {
            let parent = mesh.child(mesh.root(), parent).unwrap_or(mesh.root());
            let cubes = CubeListBuilder::create()
                .tex_offs(u, v)
                .set_mirror(mirror)
                .add_box_with(origin, size, grow);
            mesh.add_or_replace_child(parent, name, cubes, PartPose::offset(x, y, z));
        }
        let ours = LayerDefinition::create(mesh, 64, 64).bake_root();

        fn same(path: &str, ours: &ModelPart, vanilla: &ModelPart) {
            assert_eq!(ours.store_pose(), vanilla.store_pose(), "{path} pose");
            assert_eq!(ours.polygons, vanilla.polygons, "{path} faces");
            let mut names: Vec<_> = ours.children().map(|(n, _)| n).collect();
            let mut theirs: Vec<_> = vanilla.children().map(|(n, _)| n).collect();
            names.sort_unstable();
            theirs.sort_unstable();
            assert_eq!(names, theirs, "{path} children");
            for (name, child) in ours.children() {
                same(
                    &format!("{path}/{name}"),
                    child,
                    vanilla.get_child(name).unwrap(),
                );
            }
        }
        same("root", &ours, &vanilla);
    }
}
