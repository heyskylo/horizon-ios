//! Where the game hands the player models to crates above the engine (`horizon_hooks::Hooks`'s
//! `frame`, `tick`, `name_decoration`, `player_mark`, `camera`; spec #14091): only a window's
//! game calls them, never the integrated server's thread or a dedicated server. With no hook
//! registered nothing is built.

use super::Game;
use horizon_client::fast_hash::FxHashMap;
use horizon_client::join_seq::JoinList;
use horizon_client::mesher::Vertex;
use horizon_client::mobs::anim::{self, BodyPose, PooledPoses, Poses};
use horizon_client::mobs::{Mob, Part, Polygon, render};
use horizon_hooks::engine;
use horizon_hooks::players::{
    Bone, CameraView, Frame, Gear, Kind, Mesh, PartPoses, Pass, PlayerDraw, PlayerModel,
    PlayerModels, SpriteStore, Sprites,
};
use std::sync::Arc;

/// Registers the hooks of the Horizon parts still in this crate (cosmetics: spec #14091 step 5
/// moves them out). Before the hooks are sealed.
pub(crate) fn register() {
    if super::cosmetics::enabled()
        && let Err(e) = horizon_hooks::register(&super::cosmetics::HOOKS)
    {
        log::error!("Cosmetics: their hooks came too late: {e}");
    }
}

/// The models of the world a window shows: its own first, then the other players and mannequins.
struct Models<'a> {
    own: Option<&'a mut Mob>,
    mobs: &'a mut JoinList<Mob>,
    remote: &'a super::remote::Remote,
}

impl Models<'_> {
    fn find(&mut self, id: u32) -> Option<&mut Mob> {
        match self.own.as_deref_mut() {
            Some(m) if m.id == id => Some(m),
            _ => self
                .mobs
                .iter_mut()
                .find(|m| m.id == id && kind(m).is_some()),
        }
    }
}

/// What kind of player model `m` is, if it is one (the window's own model aside).
fn kind(m: &Mob) -> Option<Kind> {
    match &*m.kind.id {
        "minecraft:player" => Some(Kind::Player),
        "minecraft:mannequin" => Some(Kind::Mannequin),
        _ => None,
    }
}

/// What `m` wears where a crate may draw: a helmet or head item, a chestplate, a cape or elytra.
pub(super) fn gear(m: &Mob) -> Gear {
    let elytra = m.extra.chest.as_deref() == Some("minecraft:elytra");
    Gear::new(
        m.extra.head.is_some(),
        m.extra.chest.is_some() && !elytra,
        m.cape.is_some() || elytra,
    )
}

fn model(m: &Mob, kind: Kind, uuid: Option<u128>) -> PlayerModel {
    let (dx, dz) = (m.position.x - m.previous.x, m.position.z - m.previous.z);
    PlayerModel::new(m.id, kind, uuid, m.age, dx * dx + dz * dz > 1e-4)
        .looks(gear(m), horizon_client::mobs::trader::is_invisible(m))
}

impl PlayerModels for Models<'_> {
    fn connection(&self) -> u64 {
        self.remote.session.serial()
    }

    fn for_each(&self, f: &mut dyn FnMut(PlayerModel)) {
        if let Some(m) = self.own.as_deref() {
            f(model(m, Kind::Own, Some(self.remote.profile())));
        }
        for m in self.mobs.iter() {
            if let Some(kind) = kind(m) {
                let uuid = (kind == Kind::Player)
                    .then(|| self.remote.player_uuid(m.id))
                    .flatten();
                f(model(m, kind, uuid));
            }
        }
    }

    fn body_pose(&self, id: u32) -> Option<&Arc<dyn BodyPose>> {
        match self.own.as_deref() {
            Some(m) if m.id == id => m.emote.as_ref(),
            _ => self
                .mobs
                .iter()
                .find(|m| m.id == id && kind(m).is_some())?
                .emote
                .as_ref(),
        }
    }

    fn set_body_pose(&mut self, id: u32, pose: Option<Arc<dyn BodyPose>>) {
        if let Some(m) = self.find(id) {
            m.emote = pose;
        }
    }
}

/// Sprite `id`'s place in the atlas's names (`hooks/<hex>`), written into `buf`: no allocation.
fn location(id: u64, buf: &mut [u8; 22]) -> &str {
    use std::io::Write;
    let mut out = std::io::Cursor::new(&mut buf[..]);
    let _ = write!(out, "hooks/{id:x}");
    let len = out.position() as usize;
    std::str::from_utf8(&buf[..len]).unwrap_or_default()
}

/// The crates' sprites in the GUI atlas, in the skin slots: read while drawing.
struct HookSprites<'a, 'g>(&'a Game<'g>);

impl Sprites for HookSprites<'_, '_> {
    fn get(&self, id: u64) -> Option<[f32; 4]> {
        let mut buf = [0; 22];
        let at = location(id, &mut buf);
        let r = self.0.gui.atlas.sprites.get(at)?;
        // Kept this frame (`skin_slots.next_frame`).
        self.0.skin_downloads.borrow_mut().drawn(at);
        Some([r.u0, r.v0, r.u1, r.v1])
    }
}

/// Where a crate's frame hook puts its sprites.
struct HookStore<'a, 'g>(&'a mut Game<'g>);

impl SpriteStore for HookStore<'_, '_> {
    fn has(&self, id: u64) -> bool {
        self.0
            .gui
            .atlas
            .sprites
            .contains_key(location(id, &mut [0; 22]))
    }

    fn put(&mut self, id: u64, rgba: Vec<u8>) {
        let at = location(id, &mut [0; 22]).to_owned();
        self.0.store_skin(at, rgba);
    }
}

thread_local! {
    /// The crates' meshes as the renderer takes them, by the mesh's address (each entry holds its
    /// mesh, so the address isn't reused); render thread only.
    static MESHES: std::cell::RefCell<FxHashMap<usize, (Arc<Mesh>, Arc<Part>)>> =
        Default::default();
}

/// The game's model of `mesh`, made once.
fn part_of(mesh: &Arc<Mesh>) -> Arc<Part> {
    fn part(m: &Mesh) -> Part {
        Part {
            name: m.name.clone(),
            pose: m.pose,
            polygons: m
                .quads
                .iter()
                .map(|q| Polygon {
                    normal: q.normal,
                    vertices: q.vertices,
                    cull_back: false,
                })
                .collect(),
            children: m.children.iter().map(part).collect(),
        }
    }
    MESHES.with_borrow_mut(|meshes| {
        let (_, part_of) = meshes
            .entry(Arc::as_ptr(mesh) as usize)
            .or_insert_with(|| (mesh.clone(), Arc::new(part(mesh))));
        part_of.clone()
    })
}

/// Forgets the meshes no crate holds any more (once a frame).
fn forget_meshes() {
    let _ = MESHES.try_with(|m| {
        m.borrow_mut()
            .retain(|_, (mesh, _)| Arc::strong_count(mesh) > 1)
    });
}

/// `Poses` as a crate poses a mesh's parts.
struct PosesOf<'a>(&'a mut Poses);

impl PartPoses for PosesOf<'_> {
    fn pose(&mut self, name: &str) -> &mut [f32; 9] {
        self.0.get(name)
    }
}

/// A player model drawn, as a crate draws on it: posed on the GPU's skin path
/// when `gpu` (the mob pass's skin frame, begun; the bounds of what went there grow, for
/// culling), else its vertices go to `out`.
struct DrawPlayer<'a, 'g> {
    game: &'a Game<'g>,
    m: &'a Mob,
    pass: Pass,
    /// The model's matrix (`render::base_transform`).
    base: glam::Mat4,
    alpha: f32,
    light: u32,
    gpu: bool,
    out: &'a mut Vec<Vertex>,
    bounds: Option<(glam::Vec3, glam::Vec3)>,
    /// The body as it is drawn, posed on the first `bone`.
    body: Option<PooledPoses>,
    poses: PooledPoses,
}

impl DrawPlayer<'_, '_> {
    fn body(&self) -> &Part {
        let (key, _) = render::body(self.m);
        self.m.kind.layer(&key).unwrap_or(&self.m.kind.model)
    }
}

impl PlayerDraw for DrawPlayer<'_, '_> {
    fn pass(&self) -> Pass {
        self.pass
    }

    fn ticks(&self) -> f32 {
        self.m.age as f32 + self.alpha
    }

    fn bone(&mut self, bone: Bone) -> Option<[[f32; 4]; 4]> {
        if self.body.is_none() {
            let mut poses = Poses::pooled();
            poses.reset(self.body());
            let data = self.game.res.mob_data();
            let state = anim::state(self.m, self.alpha);
            anim::setup(self.m, &state, &mut poses, &data.animations);
            self.body = Some(poses);
        }
        let poses = self.body.as_ref()?;
        let at = render::part_matrix(self.body(), self.base, poses, bone.part(), 0.0)?;
        Some(at.to_cols_array_2d())
    }

    fn body_pose(&self) -> Option<&Arc<dyn BodyPose>> {
        self.m.emote.as_ref()
    }

    fn sprite(&self, id: u64) -> Option<[f32; 4]> {
        HookSprites(self.game).get(id)
    }

    fn draw(
        &mut self,
        mesh: &Arc<Mesh>,
        matrix: [[f32; 4]; 4],
        uv: [f32; 4],
        pose: &mut dyn FnMut(&mut dyn PartPoses),
    ) {
        let model = part_of(mesh);
        let matrix = glam::Mat4::from_cols_array_2d(&matrix);
        self.poses.reset(&model);
        pose(&mut PosesOf(&mut self.poses));
        if self.gpu {
            let mut models = self.game.skin_models.borrow_mut();
            let mut frame = self.game.skin_frame.borrow_mut();
            let mut skin = horizon_client::mobs::skin::Skin::new(&mut models, &mut frame);
            skin.bounds = self.bounds;
            let paint = horizon_client::mobs::skin::Paint {
                uv,
                scroll: 0.0,
                color: 0xFFFF_FFFF,
                light: self.light,
                overlay: 0,
            };
            if skin.draw(&model, matrix, &self.poses, paint) {
                self.bounds = skin.bounds;
                return;
            }
        }
        let poses = &self.poses;
        horizon_client::mobs::render_model(
            &model,
            matrix,
            &|p: &Part| poses.pose(p),
            uv,
            self.light,
            0,
            self.out,
        );
    }
}

impl Game<'_> {
    /// What crates draw on player model `m` in the world pass (`creatures.rs`), then the
    /// cosmetics still drawn here: see [`DrawPlayer`]. The bounds of what went to the GPU.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn player_layers_geometry(
        &self,
        m: &Mob,
        base: glam::Mat4,
        camera: glam::DVec3,
        alpha: f32,
        light: u32,
        gpu: bool,
        out: &mut Vec<Vertex>,
    ) -> Option<(glam::Vec3, glam::Vec3)> {
        debug_assert!(
            self.serving.is_none(),
            "a server thread calls no client hook"
        );
        let kind = kind(m)?;
        let mut bounds = None;
        if engine::any() {
            let model = model(m, kind, self.remote_player_uuid(m.id));
            let mut draw = DrawPlayer {
                game: self,
                m,
                pass: Pass::World,
                base,
                alpha,
                light,
                gpu,
                out: &mut *out,
                bounds: None,
                body: None,
                poses: Poses::pooled(),
            };
            engine::player_layers(&model, &mut draw);
            bounds = draw.bounds;
        }
        let worn = super::cosmetics::enabled()
            .then(|| self.cosmetics_fx_of(m, false, base, camera, alpha, light, gpu, out))
            .flatten();
        match (bounds, worn) {
            (Some((l, h)), Some((lo, hi))) => Some((l.min(lo), h.max(hi))),
            (b, w) => b.or(w),
        }
    }

    /// What crates draw on the window's own model `m` (its vertices to `out`): in the world in
    /// third person (`Pass::World`, then the cosmetics' companion, trail and aura still drawn
    /// here), or in the inventory's picture of it (`Pass::Picture`). `camera`: the world view's
    /// camera, or the picture's.
    pub(super) fn own_layers_geometry(
        &self,
        m: &Mob,
        pass: Pass,
        camera: glam::DVec3,
        alpha: f32,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        debug_assert!(
            self.serving.is_none(),
            "a server thread calls no client hook"
        );
        let fx = pass == Pass::World && super::cosmetics::enabled();
        if !engine::any() && !fx {
            return;
        }
        let base = render::base_transform(m, alpha, camera);
        if engine::any() {
            let model = model(m, Kind::Own, self.remote.as_ref().map(|r| r.profile()));
            let mut draw = DrawPlayer {
                game: self,
                m,
                pass,
                base,
                alpha,
                light,
                gpu: false,
                out: &mut *out,
                bounds: None,
                body: None,
                poses: Poses::pooled(),
            };
            engine::player_layers(&model, &mut draw);
        }
        if fx {
            self.cosmetics_fx_of(m, true, base, camera, alpha, light, false, out);
        }
    }

    /// Once a frame, before drawing.
    pub(super) fn client_hooks_frame(&mut self) {
        debug_assert!(
            self.serving.is_none(),
            "a server thread calls no client hook"
        );
        if !engine::any() {
            return;
        }
        self.cosmetics_say();
        let connection = self.remote.as_ref().map_or(0, |r| r.session.serial());
        let frame = Frame::new(connection, self.integrated.is_some());
        engine::frame(&frame, &mut HookStore(self));
        forget_meshes();
        self.cosmetics_frame();
    }

    /// Once a client tick in a world: the player's own emote, then the hooks.
    pub(super) fn client_hooks_tick(&mut self) {
        debug_assert!(
            self.serving.is_none(),
            "a server thread calls no client hook"
        );
        self.tick_emotes();
        if !engine::any() {
            return;
        }
        self.cosmetics_say();
        let Some(remote) = self.remote.as_ref() else {
            return;
        };
        engine::tick(&mut Models {
            own: self.player_model.as_mut(),
            mobs: &mut self.mobs,
            remote,
        });
    }

    /// What goes by `m`'s name (`own`: the window's own player), from the hooks.
    pub(super) fn hooks_name_marks(&self, m: &Mob, own: bool) -> super::names::Marks {
        debug_assert!(
            self.serving.is_none(),
            "a server thread calls no client hook"
        );
        let mut marks = super::names::Marks::default();
        if !engine::any() {
            return marks;
        }
        let model = match (own, kind(m)) {
            (true, _) => model(m, Kind::Own, self.remote.as_ref().map(|r| r.profile())),
            (false, Some(kind)) => model(m, kind, self.remote_player_uuid(m.id)),
            (false, None) => return marks,
        };
        if let Some(d) = engine::name_decoration(&model, &HookSprites(self)) {
            marks.mark = d.mark;
            marks.top = d.top;
            marks.style = d.badge.map(|b| {
                let [u0, v0, u1, v1] = b.uv;
                super::names::TagStyle {
                    mark: horizon_client::hud::Rect {
                        u0,
                        v0,
                        u1,
                        v1,
                        w: 8,
                        h: 8,
                    },
                    line: b.line,
                }
            });
        }
        marks
    }

    /// How far the third-person camera stands back, as a share of the usual distance: a hook's
    /// (an emote pulls it back), else 1.
    pub(super) fn hooks_camera_distance(&self, partial: f32) -> f32 {
        debug_assert!(
            self.serving.is_none(),
            "a server thread calls no client hook"
        );
        match &self.player_model {
            Some(m) if engine::any() => {
                engine::camera(&CameraView::new(m.age as f32 + partial)).map_or(1.0, |c| c.distance)
            }
            _ => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_locations_fit_the_buffer_and_differ_by_id() {
        let mut a = [0; 22];
        let mut b = [0; 22];
        assert_eq!(location(u64::MAX, &mut a), "hooks/ffffffffffffffff");
        assert_eq!(location(10, &mut b), "hooks/a");
    }
}
