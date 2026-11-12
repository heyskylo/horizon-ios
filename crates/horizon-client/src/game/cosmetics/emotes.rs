//! Emotes ([docs/cosmetics.md](../../../../docs/cosmetics.md)): short animations of the player
//! model, played from the emote wheel. Visual only: the game protocol carries nothing, a game
//! server sees a player standing still. Moving, attacking, using an item, being hurt, riding,
//! sleeping, gliding or sneaking cancels one; the camera pulls back to the front view while it
//! plays (from first person only) and comes back when it ends.

use crate::game::Game;
use crate::game::perspective::CameraType;
use horizon_client::mobs::Mob;
use horizon_client::mobs::anim::BodyPose;
use horizon_cosmetics::account::{Kept, OwnedEmotes};
use horizon_cosmetics::{Item, Playing, Slot};
use horizon_hooks::players::PlayerModels;
use std::sync::Arc;

/// How far the player may drift (blocks) before the emote stops: a nudge, not a step.
const DRIFT: f64 = 0.01;

#[derive(Default)]
pub(in crate::game) struct Emotes {
    /// The emotes the wheel offers, in order.
    pub(in crate::game) owned: Vec<Arc<Item>>,
    /// Their names by locale, by id (the service's), when known.
    pub(in crate::game) names:
        std::collections::HashMap<String, std::collections::BTreeMap<String, String>>,
    pub(in crate::game) catalogue: Catalogue,
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
    pub(in crate::game) fn sending(&self) -> Option<(&str, bool)> {
        let p = self.playing.as_ref().filter(|p| p.stop.is_none())?;
        let looping = p.item.emote.as_ref().is_some_and(|e| e.looping);
        Some((&p.item.id, looping))
    }

    /// When the emote that pulled the camera back started (the own model's age), while it plays.
    pub(in crate::game) fn pull(&self) -> Option<f32> {
        self.camera_from.and(self.playing.as_ref()).map(|p| p.start)
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

/// The emote a model's pose plays, if it is one.
pub(in crate::game) fn pose_playing(pose: Option<&Arc<dyn BodyPose>>) -> Option<&Playing> {
    let any: &dyn std::any::Any = &**pose?;
    any.downcast_ref::<EmotePose>().map(|e| &e.0)
}

/// The pose that plays `next` where `now` is laid, unless it plays that already (None: keep
/// it; a new one allocates).
fn replaced(
    now: Option<&Arc<dyn BodyPose>>,
    next: Option<Playing>,
) -> Option<Option<Arc<dyn BodyPose>>> {
    let same = match (pose_playing(now), &next) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            Arc::ptr_eq(&a.item, &b.item) && a.start == b.start && a.stop == b.stop
        }
        _ => false,
    };
    (!same).then(|| next.map(|p| Arc::new(EmotePose(p)) as Arc<dyn BodyPose>))
}

/// Makes `m` play `next`, unless it plays that already.
pub(in crate::game) fn set_playing(m: &mut Mob, next: Option<Playing>) {
    if let Some(pose) = replaced(m.emote.as_ref(), next) {
        m.emote = pose;
    }
}

/// Makes model `id` play `next`, unless it plays that already.
pub(in crate::game) fn set_model_playing(
    models: &mut dyn PlayerModels,
    id: u32,
    next: Option<Playing>,
) {
    if let Some(pose) = replaced(models.body_pose(id), next) {
        models.set_body_pose(id, pose);
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
        self.settle_owned_emotes();
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
            .cosmetics
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

    /// Once a tick: my emote (the others' are `cosmetics::Seat::tick`'s, a hook's).
    pub(in crate::game) fn tick_emotes(&mut self) {
        self.settle_owned_emotes();
        self.tick_emote();
        self.tick_others_emotes();
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

// BEGIN OWNED_CATALOGUE_STATE
/// The service session and cache lifetime belong to one signed-in UUID. Replacing
/// the owner drops only the receiver; its producer remains owned by Workers.
#[derive(Default)]
pub(in crate::game) struct Catalogue {
    owner: Option<u128>,
    pub(in crate::game) loading: Option<std::sync::mpsc::Receiver<Result<OwnedEmotes, String>>>,
    loaded: Option<std::time::Instant>,
    retry: Option<(std::time::Instant, std::time::Duration)>,
    pub(in crate::game) kept: Arc<std::sync::Mutex<Kept>>,
}

impl Catalogue {
    pub(in crate::game) fn set_owner(&mut self, owner: Option<u128>) -> bool {
        if self.owner == owner {
            return false;
        }
        *self = Self {
            owner,
            ..Self::default()
        };
        true
    }

    fn failed(&mut self, now: std::time::Instant) {
        use std::time::Duration;
        let wait = self.retry.map_or(Duration::from_secs(15), |(_, wait)| {
            (wait * 2).min(Duration::from_secs(600))
        });
        self.retry = Some((now + wait, wait));
    }

    /// Poll from a frame as well as ticks; disconnected producers settle too.
    pub(in crate::game) fn poll(&mut self, now: std::time::Instant) -> Option<OwnedEmotes> {
        use std::sync::mpsc::TryRecvError;
        let done = match self.loading.as_ref()?.try_recv() {
            Ok(done) => done,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err("cosmetics catalogue worker disconnected".into())
            }
        };
        self.loading = None;
        match done {
            Ok(answer) => {
                if answer.complete {
                    self.loaded = Some(now);
                    self.retry = None;
                } else {
                    self.failed(now);
                }
                Some(answer)
            }
            Err(_) => {
                self.failed(now);
                None
            }
        }
    }

    pub(in crate::game) fn due(&self, now: std::time::Instant) -> bool {
        self.owner.is_some()
            && self.loading.is_none()
            && match self.retry {
                Some((at, _)) => now >= at,
                None => self.loaded.is_none_or(|at| {
                    now.saturating_duration_since(at) >= std::time::Duration::from_secs(600)
                }),
            }
    }

    pub(in crate::game) fn status(&self) -> Option<&'static str> {
        if self.loading.is_some() {
            Some("horizon.emote_wheel.loading")
        } else if self.retry.is_some() {
            Some("horizon.emote_wheel.retry")
        } else {
            None
        }
    }
}
// END OWNED_CATALOGUE_STATE

impl Game<'_> {
    pub(in crate::game) fn settle_owned_emotes(&mut self) {
        if std::env::var_os("HORIZON_COSMETICS_LOCAL").is_some() {
            return;
        }
        let owner = horizon_client::auth::current().map(|session| session.uuid);
        self.apply_owned_emotes(owner, std::time::Instant::now());
    }
    pub(in crate::game) fn apply_owned_emotes(
        &mut self,
        owner: Option<u128>,
        now: std::time::Instant,
    ) {
        if self.cosmetics.emotes.catalogue.set_owner(owner) {
            self.cosmetics.emotes.owned.clear();
            self.cosmetics.emotes.names.clear();
            self.stop_emote();
        }
        let Some(answer) = self.cosmetics.emotes.catalogue.poll(now) else {
            return;
        };
        if answer.complete {
            self.cosmetics.emotes.owned.clear();
            self.cosmetics.emotes.names.clear();
        }
        for (item, names) in answer.emotes {
            self.cosmetics.emotes.names.insert(item.id.clone(), names);
            let item = Arc::new(item);
            if let Some(old) = self
                .cosmetics
                .emotes
                .owned
                .iter_mut()
                .find(|old| old.id == item.id)
            {
                *old = item;
            } else {
                self.cosmetics.emotes.owned.push(item);
            }
        }
    }
}

impl Game<'_> {
    pub(in crate::game) fn emote_visibility_fixture(
        &mut self,
    ) -> horizon_client::Result<serde_json::Value> {
        use horizon_cosmetics::prefs::Others;
        let item = Arc::new(Item::parse(
            br#"{
            "format":1,"id":"visibility_control","slot":"emote","parts":[],
            "emote":{"length":40,"loop":true,"tracks":[
                {"part":"root","keys":[{"t":0,"offset":[0,-16,0]}]}
            ]}}
        "#,
        )?);
        let playing = Playing {
            item: item.clone(),
            start: 0.0,
            stop: None,
        };
        let player_type = self
            .res
            .player_type()
            .clone()
            .ok_or("prepared player required")?;
        let mannequin_type = self
            .res
            .mob_types()
            .get("minecraft:mannequin")
            .cloned()
            .ok_or("prepared mannequin required")?;
        let mut own = horizon_client::mobs::Mob::new(
            5500,
            player_type.clone(),
            horizon_core::geometry::Vec3::new(0.0, 100.0, 0.0),
            0.0,
        );
        own.age = 10;
        set_playing(&mut own, Some(playing.clone()));
        self.player_model = Some(own);
        self.cosmetics.emotes.local_read = true;
        self.cosmetics.emotes.owned = vec![item.clone()];
        self.cosmetics.emotes.playing = Some(playing.clone());
        let mut rows = Vec::new();
        for preference in [Others::Hidden, Others::NameTags] {
            for path in ["frame_without_tick", "tick_without_frame"] {
                super::service(|seat| seat.settings.others = Some(Others::Shown));
                self.cosmetics_frame();
                let mut mannequin = horizon_client::mobs::Mob::new(
                    5501,
                    mannequin_type.clone(),
                    horizon_core::geometry::Vec3::new(0.0, 100.0, 0.0),
                    0.0,
                );
                mannequin.age = 10;
                let mut remote = horizon_client::mobs::Mob::new(
                    5502,
                    player_type.clone(),
                    horizon_core::geometry::Vec3::new(1.0, 100.0, 0.0),
                    0.0,
                );
                remote.age = 10;
                set_playing(&mut remote, Some(playing.clone()));
                self.mobs = vec![mannequin, remote].into();
                self.tick_others_emotes();
                let started = self.mobs.iter().all(|m| {
                    pose_playing(m.emote.as_ref()).is_some_and(|p| p.at(m.age as f32).is_some())
                });
                super::service(|seat| seat.settings.others = Some(preference));
                // No simulation tick occurs in the frame case, matching paused settings.
                if path == "frame_without_tick" {
                    self.cosmetics_frame();
                } else {
                    self.tick_others_emotes();
                }
                let cleared = self.mobs.iter().all(|m| m.emote.is_none());
                let own_kept = self
                    .player_model
                    .as_ref()
                    .and_then(|m| pose_playing(m.emote.as_ref()))
                    .is_some_and(|p| Arc::ptr_eq(&p.item, &item))
                    && self.cosmetics.emotes.sending() == Some((item.id.as_str(), true));
                assert!(
                    started && cleared && own_kept,
                    "{preference:?}/{path}: started={started}, cleared={cleared}, own_kept={own_kept}"
                );
                rows.push(serde_json::json!({"preference":format!("{preference:?}"),"path":path,"localLoopStarted":started,"worldEmotesCleared":cleared,"ownEmotePreserved":own_kept,"worldAges":self.mobs.iter().map(|m|m.age).collect::<Vec<_>>()}));
            }
        }
        Ok(serde_json::json!(rows))
    }
}

#[cfg(test)]
mod catalogue_tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn answer(complete: bool) -> OwnedEmotes {
        let item = Item::parse(br#"{"format":1,"id":"wave","slot":"emote","parts":[],"emote":{"length":20,"tracks":[{"part":"head","keys":[{"t":0}]}]}}"#).unwrap();
        OwnedEmotes {
            emotes: vec![(item, Default::default())],
            complete,
        }
    }
    fn ready(value: Result<OwnedEmotes, String>) -> mpsc::Receiver<Result<OwnedEmotes, String>> {
        let (tx, rx) = mpsc::channel();
        tx.send(value).unwrap();
        rx
    }
    #[test]
    fn completion_is_applied_without_a_world_or_animation_tick() {
        let now = Instant::now();
        let mut state = Catalogue::default();
        state.set_owner(Some(1));
        state.loading = Some(ready(Ok(answer(true))));
        let result = state.poll(now).unwrap();
        assert_eq!(result.emotes[0].0.id, "wave");
        assert!(!state.due(now + Duration::from_secs(599)));
        assert!(state.due(now + Duration::from_secs(600)));
        assert_eq!(state.status(), None);
    }
    #[test]
    fn many_frames_and_reopens_do_not_bypass_failure_backoff() {
        let now = Instant::now();
        let mut state = Catalogue::default();
        state.set_owner(Some(1));
        let mut requests = 1;
        state.loading = Some(ready(Err("HTTP 503".into())));
        state.poll(now);
        for frame in 1..=1000 {
            assert!(
                !state.set_owner(Some(1)),
                "reopening preserves the owner and its deadline"
            );
            let frame_time = now + Duration::from_millis(frame);
            state.poll(frame_time);
            if state.due(frame_time) {
                requests += 1;
            }
        }
        assert_eq!(requests, 1);
        assert!(state.due(now + Duration::from_secs(15)));
        state.loading = Some(ready(Err("HTTP 503".into())));
        requests += 1;
        state.poll(now + Duration::from_secs(15));
        assert_eq!(requests, 2);
        assert!(!state.due(now + Duration::from_secs(44)));
        assert!(state.due(now + Duration::from_secs(45)));
    }
    #[test]
    fn incomplete_answer_is_retryable_without_a_ten_minute_freshness_stamp() {
        let now = Instant::now();
        let mut state = Catalogue::default();
        state.set_owner(Some(1));
        state.loading = Some(ready(Ok(answer(false))));
        assert!(!state.poll(now).unwrap().complete);
        assert_eq!(state.loaded, None);
        assert!(state.due(now + Duration::from_secs(15)));
        assert_eq!(state.status(), Some("horizon.emote_wheel.retry"));
    }
    #[test]
    fn owner_change_drops_stale_results_and_replaces_the_service_session() {
        let mut state = Catalogue::default();
        state.set_owner(Some(1));
        let previous = state.kept.clone();
        let (tx, rx) = mpsc::channel();
        state.loading = Some(rx);
        assert!(state.set_owner(Some(2)));
        assert!(tx.send(Ok(answer(true))).is_err());
        assert!(!Arc::ptr_eq(&previous, &state.kept));
        assert!(state.loading.is_none());
        assert!(state.due(Instant::now()));
        state.set_owner(None);
        assert!(!state.due(Instant::now()));
    }
    #[test]
    fn bounded_refusal_and_disconnection_settle_to_the_same_retry_policy() {
        let now = Instant::now();
        let workers = horizon_cosmetics::workers::Workers::default();
        assert!(workers.shutdown());
        let mut state = Catalogue::default();
        state.set_owner(Some(1));
        state.loading = Some(workers.spawn(|| Ok(answer(true))));
        assert!(state.poll(now).is_none());
        assert!(state.loading.is_none());
        assert!(!state.due(now));
        assert!(state.due(now + Duration::from_secs(15)));
        let (tx, rx) = mpsc::channel();
        state.loading = Some(rx);
        drop(tx);
        assert!(state.poll(now + Duration::from_secs(15)).is_none());
        assert!(state.loading.is_none());
        assert!(!state.due(now + Duration::from_secs(44)));
        assert!(state.due(now + Duration::from_secs(45)));
    }
}

impl Game<'_> {
    /// Apply visibility without waiting for a simulation tick, including local mannequins.
    pub(in crate::game) fn tick_others_emotes(&mut self) {
        if super::service(|seat| seat.others()) != horizon_cosmetics::prefs::Others::Shown {
            for mob in &mut self.mobs {
                set_playing(mob, None);
            }
            return;
        }
        if std::env::var_os("HORIZON_COSMETICS_LOCAL").is_some() {
            let first = self.owned_emotes().first().cloned();
            if let Some(item) = first {
                for mob in &mut self.mobs {
                    if &*mob.kind.id == "minecraft:mannequin"
                        && pose_playing(mob.emote.as_ref())
                            .is_none_or(|p| p.at(mob.age as f32).is_none())
                    {
                        let start = mob.age as f32 - (mob.id % 40) as f32;
                        set_playing(
                            mob,
                            Some(Playing {
                                item: item.clone(),
                                start,
                                stop: None,
                            }),
                        );
                    }
                }
            }
        }
    }
}
