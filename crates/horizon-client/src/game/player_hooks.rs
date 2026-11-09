//! Where the game hands the player models to crates above the engine
//! (`horizon_hooks::Hooks::frame` and `tick`, spec #14091): only a window's game calls them,
//! never the integrated server's thread or a dedicated server. With no hook registered nothing is
//! built.

use super::Game;
use horizon_client::join_seq::JoinList;
use horizon_client::mobs::Mob;
use horizon_client::mobs::anim::BodyPose;
use horizon_hooks::engine;
use horizon_hooks::players::{Frame, Kind, PlayerModel, PlayerModels};
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
            _ => self.mobs.iter_mut().find(|m| m.id == id && kind(m).is_some()),
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

fn model(m: &Mob, kind: Kind, uuid: Option<u128>) -> PlayerModel {
    let (dx, dz) = (m.position.x - m.previous.x, m.position.z - m.previous.z);
    PlayerModel::new(m.id, kind, uuid, m.age, dx * dx + dz * dz > 1e-4)
}

impl PlayerModels for Models<'_> {
    fn connection(&self) -> u64 {
        self.remote.session.serial()
    }

    fn for_each(&self, f: &mut dyn FnMut(PlayerModel)) {
        if let Some(m) = self.own.as_deref() {
            f(model(m, Kind::Own, None));
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

impl Game<'_> {
    /// Once a frame, before drawing.
    pub(super) fn client_hooks_frame(&mut self) {
        debug_assert!(self.serving.is_none(), "a server thread calls no client hook");
        if !engine::any() {
            return;
        }
        self.cosmetics_say();
        let connection = self.remote.as_ref().map_or(0, |r| r.session.serial());
        engine::frame(&Frame::new(connection));
        self.cosmetics_frame();
    }

    /// Once a client tick in a world: the player's own emote, then the hooks.
    pub(super) fn client_hooks_tick(&mut self) {
        debug_assert!(self.serving.is_none(), "a server thread calls no client hook");
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
}

