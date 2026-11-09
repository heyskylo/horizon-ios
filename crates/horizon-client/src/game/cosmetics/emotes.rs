//! Emotes ([docs/cosmetics.md](../../../../docs/cosmetics.md)): short animations of the player
//! model, played from the emote wheel. Visual only: the game protocol carries nothing, a game
//! server sees a player standing still. Moving, attacking, using an item, being hurt, riding,
//! sleeping, gliding or sneaking cancels one; the camera pulls back to the front view while it
//! plays (from first person only) and comes back when it ends.

use crate::game::Game;
use crate::game::perspective::CameraType;
use horizon_client::mobs::Mob;
use horizon_client::mobs::anim::BodyPose;
use horizon_cosmetics::account::OwnedEmote;
use horizon_cosmetics::{Item, Playing, Slot};
use std::sync::Arc;

/// How far the player may drift (blocks) before the emote stops: a nudge, not a step.
const DRIFT: f64 = 0.01;

#[derive(Default)]
pub(in crate::game) struct Emotes {
    /// The emotes the wheel offers, in order.
    pub(in crate::game) owned: Vec<Arc<Item>>,
    /// Their names by locale, by id (the service's), when known.
    pub(in crate::game) names: std::collections::HashMap<String, std::collections::BTreeMap<String, String>>,
    /// The account's emotes on their way from the service, and when they last came.
    pub(in crate::game) loading: Option<std::sync::mpsc::Receiver<Result<Vec<OwnedEmote>, String>>>,
    pub(in crate::game) loaded: Option<std::time::Instant>,
    /// `owned` was read from `HORIZON_COSMETICS_LOCAL`'s folder.
    local_read: bool,
    playing: Option<Playing>,
    /// The camera the emote pulled back from, put back when it ends (unless the player chose
    /// another meanwhile).
    camera_from: Option<CameraType>,
    /// Where the player stood when it started.
    from: Option<horizon_core::geometry::Vec3>,
}

impl Emotes {
    /// The emote to tell the service about, while it plays: its id and whether it loops.
    pub(in crate::game) fn sending(&self) -> Option<(String, bool)> {
        let p = self.playing.as_ref().filter(|p| p.stop.is_none())?;
        let looping = p.item.emote.as_ref().is_some_and(|e| e.looping);
        Some((p.item.id.clone(), looping))
    }
}

/// An emote on a player's model: the engine's pose hook (`Mob::emote`), which names no cosmetics
/// type.
#[derive(Debug)]
struct EmotePose(Playing);

impl BodyPose for EmotePose {
    fn pose(&self, age: f32, at_rest: &[[f32; 9]; 6], poses: &mut [[f32; 9]; 6]) -> bool {
        let (Some(emote), Some((t, weight))) = (&self.0.item.emote, self.0.at(age)) else {
            return false;
        };
        emote.pose_body(t, weight, at_rest, poses);
        true
    }
}

/// The emote `m` plays, if any.
pub(in crate::game) fn playing(m: &Mob) -> Option<&Playing> {
    let any: &dyn std::any::Any = &**m.emote.as_ref()?;
    any.downcast_ref::<EmotePose>().map(|e| &e.0)
}

/// Makes `m` play `next`, unless it plays that already (a new one allocates).
pub(in crate::game) fn set_playing(m: &mut Mob, next: Option<Playing>) {
    let same = match (playing(m), &next) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            Arc::ptr_eq(&a.item, &b.item) && a.start == b.start && a.stop == b.stop
        }
        _ => false,
    };
    if !same {
        m.emote = next.map(|p| Arc::new(EmotePose(p)) as Arc<dyn BodyPose>);
    }
}

/// The emotes among a folder's item files, by id.
fn read_folder(dir: &std::path::Path) -> Vec<Arc<Item>> {
    let mut emotes: Vec<Arc<Item>> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Item::parse(&std::fs::read(e.path()).ok()?).ok())
        .filter(|i| i.slot == Slot::Emote)
        .map(Arc::new)
        .collect();
    emotes.sort_by(|a, b| a.id.cmp(&b.id));
    emotes
}

impl Game<'_> {
    /// The emotes the wheel offers: a local folder's (`HORIZON_COSMETICS_LOCAL`), else those the
    /// account owns.
    pub(in crate::game) fn owned_emotes(&mut self) -> &[Arc<Item>] {
        if !self.cosmetics.emotes.local_read {
            self.cosmetics.emotes.local_read = true;
            if let Some(dir) = std::env::var_os("HORIZON_COSMETICS_LOCAL") {
                self.cosmetics.emotes.owned = read_folder(std::path::Path::new(&dir));
            }
        }
        &self.cosmetics.emotes.owned
    }

    /// An emote's name: the catalogue's, else its id in words (`wave_hello`: "Wave hello").
    pub(in crate::game) fn emote_name(&self, id: &str) -> String {
        let names = self.cosmetics.emotes.names.get(id);
        let lang = horizon_client::lang::code();
        if let Some(name) = names.and_then(|n| n.get(&lang).or_else(|| n.get("en_us"))) {
            return name.clone();
        }
        let words = id.replace('_', " ");
        let mut chars = words.chars();
        chars
            .next()
            .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
    }

    /// Whether the player could start an emote now: in a world, standing, alive, not a spectator.
    fn may_emote(&self) -> bool {
        let Some(p) = self.player.as_ref() else {
            return false;
        };
        super::enabled()
            && self.player_model.is_some()
            && !p.health.dead_or_dying()
            && !p.fall_flying
            && p.pose == horizon_client::pose::Pose::Standing
            && self.riding.is_none()
            && self.sleeping.is_none()
            && !self.spectator()
    }

    /// Plays the owned emote `id` (again from its start if it plays already). False if it isn't
    /// owned or the player can't emote now.
    pub(in crate::game) fn play_emote(&mut self, id: &str) -> bool {
        let Some(item) = self.owned_emotes().iter().find(|i| i.id == id).cloned() else {
            return false;
        };
        if !self.may_emote() {
            return false;
        }
        let (Some(m), Some(p)) = (self.player_model.as_ref(), self.player.as_ref()) else {
            return false;
        };
        let start = m.age as f32;
        self.cosmetics.emotes.from = Some(p.position);
        self.cosmetics.emotes.playing = Some(Playing {
            item,
            start,
            stop: None,
        });
        if self.camera_type.first_person() {
            self.cosmetics.emotes.camera_from = Some(self.camera_type);
            self.camera_type = CameraType::ThirdPersonFront;
            self.check_entity_post_effect(false);
        }
        true
    }

    /// Stops the emote playing, fading it out.
    pub(in crate::game) fn stop_emote(&mut self) {
        let now = self.player_model.as_ref().map_or(0.0, |m| m.age as f32);
        if let Some(p) = self.cosmetics.emotes.playing.as_mut()
            && p.stop.is_none()
        {
            p.stop = Some(now);
        }
    }

    /// The emote playing, if any (its id).
    pub(in crate::game) fn emote_playing(&self) -> Option<&str> {
        let p = self.cosmetics.emotes.playing.as_ref()?;
        p.stop.is_none().then_some(p.item.id.as_str())
    }

    /// Once a tick after the player's model: cancels the emote on movement or action, ends it,
    /// puts the camera back, and hands it to the model.
    fn tick_emote(&mut self) {
        if self.cosmetics.emotes.playing.is_none() {
            if let Some(m) = self.player_model.as_mut() {
                set_playing(m, None);
            }
            return;
        }
        let moved = match (self.player.as_ref(), self.cosmetics.emotes.from) {
            (Some(p), Some(from)) => {
                let (dx, dy, dz) = (
                    p.position.x - from.x,
                    p.position.y - from.y,
                    p.position.z - from.z,
                );
                dx * dx + dy * dy + dz * dz > DRIFT * DRIFT
            }
            _ => true,
        };
        let hurt = self
            .player
            .as_ref()
            .is_some_and(|p| p.health.hurt_animation_time > 0);
        if moved || hurt || self.swing.swinging() || self.using.is_some() || !self.may_emote() {
            self.stop_emote();
        }
        let now = self.player_model.as_ref().map_or(0.0, |m| m.age as f32);
        if self
            .emotes
            .playing
            .as_ref()
            .and_then(|p| p.at(now))
            .is_none()
        {
            self.cosmetics.emotes.playing = None;
            self.cosmetics.emotes.from = None;
            // The pulled-back view goes back to first person, unless the player turned it.
            if let Some(from) = self.cosmetics.emotes.camera_from.take()
                && self.camera_type == CameraType::ThirdPersonFront
            {
                self.camera_type = from;
                self.check_entity_post_effect(from.first_person());
            }
        }
        if let Some(m) = self.player_model.as_mut() {
            set_playing(m, self.cosmetics.emotes.playing.clone());
        }
    }

    /// Once a tick: my emote, then the other players'.
    pub(in crate::game) fn tick_emotes(&mut self) {
        if let Some(done) = self
            .emotes
            .loading
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.cosmetics.emotes.loading = None;
            if let Ok(owned) = done {
                self.cosmetics.emotes.loaded = Some(std::time::Instant::now());
                self.cosmetics.emotes.names.clear();
                self.cosmetics.emotes.owned.clear();
                for (item, names) in owned {
                    self.cosmetics.emotes.names.insert(item.id.clone(), names);
                    self.cosmetics.emotes.owned.push(Arc::new(item));
                }
            }
        }
        self.tick_emote();
        self.tick_others_emotes();
    }

    /// How far the camera stands back while an emote pulls it (1: the usual distance): it eases
    /// out over the emote's first quarter second.
    pub(in crate::game) fn emote_camera_pull(&self, partial: f32) -> f32 {
        let (Some(p), Some(m)) = (&self.cosmetics.emotes.playing, &self.player_model) else {
            return 1.0;
        };
        if self.cosmetics.emotes.camera_from.is_none() {
            return 1.0;
        }
        let t = ((m.age as f32 + partial - p.start) / 5.0).clamp(0.0, 1.0);
        0.35 + 0.65 * t * t * (3.0 - 2.0 * t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_emote_poses_the_body_over_the_animation_until_it_ends() {
        // The right arm up to 90° about z at tick 10, the body hopping 4 px.
        let item = Item::parse(
            br#"{"format": 1, "id": "up", "slot": "emote", "parts": [],
                "emote": {"length": 20, "blend": 0, "tracks": [
                  {"part": "right_arm", "keys": [{"t": 10, "rotation": [0, 0, 90]}]},
                  {"part": "root", "keys": [{"t": 0, "offset": [0, -4, 0]}]}]}}"#,
        )
        .unwrap();
        let rest = |x: f32, y: f32| [x, y, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let mut at_rest = [rest(0.0, 0.0); 6];
        at_rest[2] = rest(-5.0, 2.0);
        // What the walk animation did: the head looks aside, the arm swings.
        let mut poses = at_rest;
        poses[0][4] = 0.4;
        poses[2][3] = 0.7;
        let pose = EmotePose(Playing {
            item: Arc::new(item),
            start: 100.0,
            stop: None,
        });
        assert!(pose.pose(110.0, &at_rest, &mut poses));
        let arm = poses[2];
        assert!(
            (arm[5] - std::f32::consts::FRAC_PI_2).abs() < 1e-5 && arm[3].abs() < 1e-5,
            "{arm:?}"
        );
        assert!((arm[0] + 5.0).abs() < 1e-5 && (arm[1] - (2.0 - 4.0)).abs() < 1e-5);
        // The head keeps looking aside, lifted with the body.
        let head = poses[0];
        assert!((head[4] - 0.4).abs() < 1e-5 && (head[1] + 4.0).abs() < 1e-5);
        // Past its end the emote poses nothing.
        assert!(!pose.pose(125.0, &at_rest, &mut poses));
    }
}
