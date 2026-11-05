//! Emotes ([docs/cosmetics.md](../../../../docs/cosmetics.md)): short animations of the player
//! model, played from the emote wheel. Visual only: the game protocol carries nothing, a game
//! server sees a player standing still. Moving, attacking, using an item, being hurt, riding,
//! sleeping, gliding or sneaking cancels one; the camera pulls back to the front view while it
//! plays (from first person only) and comes back when it ends.

use super::Game;
use super::perspective::CameraType;
use horizon_cosmetics::account::OwnedEmote;
use horizon_cosmetics::{Item, Playing, Slot};
use std::sync::Arc;

/// How far the player may drift (blocks) before the emote stops: a nudge, not a step.
const DRIFT: f64 = 0.01;

#[derive(Default)]
pub(super) struct Emotes {
    /// The emotes the wheel offers, in order.
    pub(super) owned: Vec<Arc<Item>>,
    /// Their names by locale, by id (the service's), when known.
    pub(super) names: std::collections::HashMap<String, std::collections::BTreeMap<String, String>>,
    /// The account's emotes on their way from the service, and when they last came.
    pub(super) loading: Option<std::sync::mpsc::Receiver<Result<Vec<OwnedEmote>, String>>>,
    pub(super) loaded: Option<std::time::Instant>,
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
    pub(super) fn sending(&self) -> Option<(String, bool)> {
        let p = self.playing.as_ref().filter(|p| p.stop.is_none())?;
        let looping = p.item.emote.as_ref().is_some_and(|e| e.looping);
        Some((p.item.id.clone(), looping))
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
    pub(super) fn owned_emotes(&mut self) -> &[Arc<Item>] {
        if !self.emotes.local_read {
            self.emotes.local_read = true;
            if let Some(dir) = std::env::var_os("HORIZON_COSMETICS_LOCAL") {
                self.emotes.owned = read_folder(std::path::Path::new(&dir));
            }
        }
        &self.emotes.owned
    }

    /// An emote's name: the catalogue's, else its id in words (`wave_hello`: "Wave hello").
    pub(super) fn emote_name(&self, id: &str) -> String {
        let names = self.emotes.names.get(id);
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
        horizon_cosmetics::enabled()
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
    pub(super) fn play_emote(&mut self, id: &str) -> bool {
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
        self.emotes.from = Some(p.position);
        self.emotes.playing = Some(Playing {
            item,
            start,
            stop: None,
        });
        if self.camera_type.first_person() {
            self.emotes.camera_from = Some(self.camera_type);
            self.camera_type = CameraType::ThirdPersonFront;
            self.check_entity_post_effect(false);
        }
        true
    }

    /// Stops the emote playing, fading it out.
    pub(super) fn stop_emote(&mut self) {
        let now = self.player_model.as_ref().map_or(0.0, |m| m.age as f32);
        if let Some(p) = self.emotes.playing.as_mut()
            && p.stop.is_none()
        {
            p.stop = Some(now);
        }
    }

    /// The emote playing, if any (its id).
    pub(super) fn emote_playing(&self) -> Option<&str> {
        let p = self.emotes.playing.as_ref()?;
        p.stop.is_none().then_some(p.item.id.as_str())
    }

    /// Once a tick after the player's model: cancels the emote on movement or action, ends it,
    /// puts the camera back, and hands it to the model.
    fn tick_emote(&mut self) {
        if self.emotes.playing.is_none() {
            if let Some(m) = self.player_model.as_mut() {
                m.emote = None;
            }
            return;
        }
        let moved = match (self.player.as_ref(), self.emotes.from) {
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
            self.emotes.playing = None;
            self.emotes.from = None;
            // The pulled-back view goes back to first person, unless the player turned it.
            if let Some(from) = self.emotes.camera_from.take()
                && self.camera_type == CameraType::ThirdPersonFront
            {
                self.camera_type = from;
                self.check_entity_post_effect(from.first_person());
            }
        }
        if let Some(m) = self.player_model.as_mut() {
            m.emote = self.emotes.playing.clone();
        }
    }

    /// Once a tick: my emote, then the other players'.
    pub(super) fn tick_emotes(&mut self) {
        if let Some(done) = self
            .emotes
            .loading
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.emotes.loading = None;
            if let Ok(owned) = done {
                self.emotes.loaded = Some(std::time::Instant::now());
                self.emotes.names.clear();
                self.emotes.owned.clear();
                for (item, names) in owned {
                    self.emotes.names.insert(item.id.clone(), names);
                    self.emotes.owned.push(Arc::new(item));
                }
            }
        }
        self.tick_emote();
        self.tick_others_emotes();
    }

    /// How far the camera stands back while an emote pulls it (1: the usual distance): it eases
    /// out over the emote's first quarter second.
    pub(super) fn emote_camera_pull(&self, partial: f32) -> f32 {
        let (Some(p), Some(m)) = (&self.emotes.playing, &self.player_model) else {
            return 1.0;
        };
        if self.emotes.camera_from.is_none() {
            return 1.0;
        }
        let t = ((m.age as f32 + partial - p.start) / 5.0).clamp(0.0, 1.0);
        0.35 + 0.65 * t * t * (3.0 - 2.0 * t)
    }
}
