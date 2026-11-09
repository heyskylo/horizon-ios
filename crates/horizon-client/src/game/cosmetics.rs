//! Cosmetics on players ([docs/cosmetics.md](../../../../docs/cosmetics.md)): only Horizon
//! clients draw them, off unless [`enabled`]. What another player wears comes
//! from the cosmetics service by UUID (`horizon_cosmetics::Cosmetics`, its own thread); this file
//! puts the items on the avatar's bones, on the GPU skin path when the renderer takes it.
//!
//! Other players' cosmetics show by default only to an account that owns one (asked of the
//! service once a run, `OwnerCheck`): anyone else sees a vanilla game and the game asks nothing
//! about other players, until the player chooses otherwise in the settings.

pub(super) mod emote_wheel;
pub(super) mod emotes;
mod fx;

use super::Game;
use horizon_client::fast_hash::FxHashMap;
use horizon_cosmetics::prefs::{self, Others};
use horizon_cosmetics::{Item, Slot};
use horizon_hooks::players::{
    Badge, Bone, CameraOverride, CameraView, Frame, Gear, Kind, Look, Mark, Mesh, NameDecoration,
    Pass, PlayerDraw, PlayerModel, PlayerModels, Quad, SpriteStore, Sprites,
};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Horizon's mark before a Horizon player's name, over their name tag and in the player list.
const MARK: Mark = &horizon_cosmetics::mark::MARK;

#[derive(Default)]
pub(super) struct Cosmetics {
    /// Item files from a folder (`HORIZON_COSMETICS_LOCAL`): mannequins wear them too.
    local: bool,
    /// Emotes: those the wheel offers and the one playing (`cosmetics/emotes.rs`).
    emotes: emotes::Emotes,
    /// The emote wheel (`cosmetics/emote_wheel.rs`).
    wheel: emote_wheel::Wheel,
}

/// The emote the player plays, told to the service: the last wish sent (`Some(None)`: stopped),
/// the wish waiting, the request on its way, and the session kept for it.
#[derive(Default)]
struct EmoteOut {
    sent: Option<Option<String>>,
    /// When a looping emote is said again (`/v1/emote` keeps one a minute).
    again: Option<Instant>,
    sending: Option<Receiver<Result<(), String>>>,
    retry: Option<(Instant, Duration)>,
    kept: Arc<std::sync::Mutex<horizon_cosmetics::account::Kept>>,
}

/// Tells the service the player plays `emote` (`None`: stopped).
fn send_emote(
    kept: &std::sync::Mutex<horizon_cosmetics::account::Kept>,
    emote: Option<&str>,
) -> Result<(), String> {
    let Some(session) = horizon_client::auth::current() else {
        return Ok(());
    };
    let mut kept = kept
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    horizon_cosmetics::account::play_emote(
        &mut kept,
        &http(),
        &session.name,
        |id| horizon_client::auth::join_server(&session, id).map_err(|e| format!("{e:?}")),
        emote,
    )
}

/// The emotes the signed-in account owns, for the wheel (none signed in: none).
fn load_owned_emotes(
    kept: &std::sync::Mutex<horizon_cosmetics::account::Kept>,
) -> Result<Vec<horizon_cosmetics::account::OwnedEmote>, String> {
    let Some(session) = horizon_client::auth::current() else {
        return Ok(Vec::new());
    };
    let service = http();
    let mut kept = kept
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    horizon_cosmetics::account::owned_emotes(
        &mut kept,
        &service,
        &session.name,
        |id| horizon_client::auth::join_server(&session, id).map_err(|e| format!("{e:?}")),
        |path| horizon_cosmetics::Transport::get(&service, path),
    )
}

/// Whether the signed-in account owns a cosmetic: the answer kept from the last run first
/// (`prefs::owned`), then the service's, asked once a run and retried after a failure like Show my
/// cosmetics. Not known yet, or no session: not owning.
#[derive(Default)]
struct OwnerCheck {
    started: bool,
    owns: bool,
    /// The service answered this run.
    known: bool,
    asking: Option<Receiver<Result<bool, String>>>,
    retry: Option<(Instant, Duration)>,
}

/// The wait after another failure: 15 s, doubling to 10 minutes.
fn backoff(last: Option<(Instant, Duration)>) -> Option<(Instant, Duration)> {
    let wait = last.map_or(Duration::from_secs(15), |(_, w)| {
        (w * 2).min(Duration::from_secs(600))
    });
    Some((Instant::now() + wait, wait))
}

/// The signed-in account's UUID as the service writes it, and the game folder.
fn account() -> Option<(
    Arc<horizon_client::auth::AuthSession>,
    String,
    std::path::PathBuf,
)> {
    let session = horizon_client::auth::current()?;
    let uuid = format!("{:032x}", session.uuid);
    Some((session, uuid, std::env::current_dir().ok()?))
}

/// Asks the service whether the signed-in account owns a cosmetic and keeps the answer in the
/// game folder. An offline account owns none and asks nothing.
fn check_owner() -> Result<bool, String> {
    let Some((session, uuid, home)) = account() else {
        return Ok(false);
    };
    let owns = horizon_cosmetics::account::owns(&http(), &session.name, |id| {
        horizon_client::auth::join_server(&session, id).map_err(|e| format!("{e:?}"))
    })?;
    if let Err(e) = prefs::set_owned(&home, &uuid, owns) {
        log::warn!("Cosmetics: the ownership answer is not saved: {e}");
    }
    Ok(owns)
}

fn http() -> horizon_cosmetics::Http {
    let api =
        std::env::var("HORIZON_COSMETICS_API").unwrap_or_else(|_| horizon_cosmetics::API.into());
    horizon_cosmetics::Http::new(&api, &format!("Horizon/{}", env!("CARGO_PKG_VERSION")))
}

/// Sending one of the account's switches: the wish it last looked at, the request on its way,
/// when to try again after a failure (15 s, doubling to 10 minutes), and when to say an unchanged
/// wish again.
#[derive(Default)]
struct AccountSync {
    wish: Option<bool>,
    sending: Option<Receiver<Result<(), String>>>,
    retry: Option<(Instant, Duration)>,
    again: Option<Instant>,
}

impl AccountSync {
    /// Takes the answer of the request on its way, if it came; `every`: how long a sent `true`
    /// stands before it is sent again.
    fn settle(&mut self, every: Option<Duration>) {
        let Some(done) = self.sending.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.sending = None;
        match done {
            Ok(()) => {
                self.retry = None;
                self.again = every
                    .filter(|_| self.wish == Some(true))
                    .map(|e| Instant::now() + e);
            }
            Err(_) => self.retry = backoff(self.retry),
        }
    }

    /// Whether `wish` is to be sent now: it changed, a failure's wait is over, or `again` is due.
    fn due(&self, wish: bool) -> bool {
        let timer = self.retry.map(|(at, _)| at).or(self.again);
        self.sending.is_none()
            && (self.wish != Some(wish) || timer.is_some_and(|at| Instant::now() >= at))
    }

    /// Sends `wish` on a thread.
    fn start(&mut self, wish: bool, send: fn(bool) -> Result<(), String>) {
        self.wish = Some(wish);
        self.again = None;
        let (tx, rx) = channel();
        self.sending = Some(rx);
        std::thread::Builder::new()
            .name("Cosmetics account".into())
            .spawn(move || {
                let _ = tx.send(send(wish));
            })
            .ok();
    }
}

/// Tells the service whether the signed-in account may be shown as a Horizon player (listed: it
/// plays now). An offline account has nothing to send.
fn send_listed(listed: bool) -> Result<(), String> {
    let Some(session) = horizon_client::auth::current() else {
        return Ok(());
    };
    horizon_cosmetics::account::set_listed(
        &http(),
        &session.name,
        |id| horizon_client::auth::join_server(&session, id).map_err(|e| format!("{e:?}")),
        listed,
    )
    .map(|_| ())
}

/// Makes the service keep `wish` for the signed-in account, unless it already confirmed it
/// (`prefs::confirmed`, in the game folder). An offline account has nothing to send.
fn send_mine(wish: bool) -> Result<(), String> {
    let Some((session, uuid, home)) = account() else {
        return Ok(());
    };
    if prefs::confirmed(&home, &uuid).unwrap_or(true) == wish {
        return Ok(());
    }
    let kept = horizon_cosmetics::account::set_visible(
        &http(),
        &session.name,
        |id| horizon_client::auth::join_server(&session, id).map_err(|e| format!("{e:?}")),
        wish,
    )?;
    prefs::set_confirmed(&home, &uuid, kept).map_err(|e| e.to_string())
}

impl Cosmetics {
    fn client() -> (horizon_cosmetics::Cosmetics, bool) {
        if let Some(dir) = std::env::var_os("HORIZON_COSMETICS_LOCAL") {
            let local = horizon_cosmetics::Local::new(dir.into());
            return (horizon_cosmetics::Cosmetics::new(local, None), true);
        }
        let api = std::env::var("HORIZON_COSMETICS_API")
            .unwrap_or_else(|_| horizon_cosmetics::API.to_owned());
        let agent = format!("Horizon/{}", env!("CARGO_PKG_VERSION"));
        // Beside the downloaded skins (`assets/skins` in the game folder).
        let cache = std::env::current_dir()
            .unwrap_or_default()
            .join("assets/cosmetics");
        let http = horizon_cosmetics::Http::new(&api, &agent);
        (horizon_cosmetics::Cosmetics::new(http, Some(cache)), false)
    }
}

/// What the player whose UUID `uuid` gives wears, if the game shows it: the player's own model
/// (`own`) while Show my cosmetics is on, another player unless others' cosmetics are hidden (by
/// choice, or by owning none). Hidden, the service is never asked about them.
fn worn_of(
    client: &horizon_cosmetics::Cosmetics,
    said: &Said,
    own: bool,
    uuid: impl FnOnce() -> Option<u128>,
) -> Option<horizon_cosmetics::Worn> {
    let shown = if own { said.mine } else { !said.others_hidden };
    if !shown {
        return None;
    }
    client.frame().worn(uuid()?)
}

/// The UUID the service knows the player's own model by. On a server, the profile it gave
/// (`profile`). In single player the window's own server (`solo`) names the player by their
/// offline profile, which the service doesn't know: the signed-in account's (`account`) then, only
/// with *Cosmetics in single player* on (`shown`); off, nothing is asked or drawn.
fn own_uuid(
    profile: Option<u128>,
    solo: bool,
    shown: bool,
    account: impl FnOnce() -> Option<u128>,
) -> Option<u128> {
    match solo {
        false => profile,
        true if shown && profile.is_some() => account(),
        true => None,
    }
}

/// How often a looping emote is said again (the service keeps one a minute).
const LOOP_AGAIN: Duration = Duration::from_secs(30);

/// The texture in the top-left corner of a skin slot's 64×64 RGBA.
fn slot_image(item: &Item) -> Vec<u8> {
    let t = &item.texture;
    let mut rgba = vec![0; 64 * 64 * 4];
    for y in 0..t.height as usize {
        let row = &t.rgba[y * t.width as usize * 4..(y + 1) * t.width as usize * 4];
        rgba[y * 64 * 4..y * 64 * 4 + row.len()].copy_from_slice(row);
    }
    rgba
}

/// Which slots an avatar's gear covers: a helmet or head item hides a hat, an elytra or a cape
/// what is worn on the back. A name tag style is never drawn on the body.
fn covered(gear: Gear, slot: Slot) -> bool {
    match slot {
        Slot::Head => gear.head,
        Slot::Back => gear.back,
        Slot::Shoulder => false,
        // A chestplate, not an elytra.
        Slot::Torso => gear.chest,
        Slot::NameTag | Slot::Emote => true,
        // On no bone: drawn by a pass of their own, never on the body.
        Slot::Companion | Slot::Trail | Slot::Aura => true,
    }
}

/// How high the tallest item drawn on a body gets above the neck (pixels, `Item::top`): what
/// lifts the wearer's name tag. None when nothing is drawn on it: the wearer's own cosmetics hidden
/// (`worn` is None), another player's at "name tags only" or hidden, or every item `covered` (a
/// helmet over a hat). One max over at most five items, read when the item was parsed.
fn body_top(
    others: Others,
    own: bool,
    worn: Option<&horizon_cosmetics::Worn>,
    covered: impl Fn(Slot) -> bool,
) -> Option<f32> {
    if !own && others != Others::Shown {
        return None;
    }
    worn?
        .iter()
        .filter(|i| !covered(i.slot))
        .map(|i| i.top)
        .reduce(f32::max)
}

/// Whether cosmetics, emotes and the "Horizon player" presence run at all: the `horizon-extras`
/// feature's gate for all three (docs/forking.md), then `horizon_cosmetics::enabled()`. The game
/// asks this, never `horizon_cosmetics::enabled()` itself.
pub(super) fn enabled() -> bool {
    cfg!(feature = "horizon-extras") && horizon_cosmetics::enabled()
}

/// What the cosmetics' hooks keep for one splitscreen player (`horizon_hooks::seat`): the
/// service's client, the emote feed and what the account tells the service.
#[derive(Default)]
struct Seat {
    client: Option<Arc<horizon_cosmetics::Cosmetics>>,
    /// Item files from a folder (`HORIZON_COSMETICS_LOCAL`).
    local: bool,
    /// Show my cosmetics on its way to the service.
    mine: AccountSync,
    /// Show me as a Horizon player: sent at start, on each change and every hour while on.
    listed: AccountSync,
    /// Whether the signed-in account owns a cosmetic.
    owner: OwnerCheck,
    /// Which other players play an emote (`horizon_cosmetics::Feed`), started with the client.
    feed: Option<horizon_cosmetics::Feed>,
    /// My emote on its way to the service.
    emote_out: EmoteOut,
    /// What the game's settings and the player's emote say (`Game::cosmetics_say`).
    said: Said,
    /// Kept between ticks so a tick allocates nothing: the player models in view.
    models: Vec<PlayerModel>,
    /// The world is the window's own single-player one (`Frame::solo`).
    solo: bool,
    /// The items the hooks show, by the item's address.
    items: FxHashMap<usize, Kept>,
    last_sprite: u64,
    /// Sprites drawing missed, put in the atlas next frame.
    wanted: Vec<u64>,
    /// Companions, trails and auras (`cosmetics/fx.rs`).
    fx: fx::Fx,
}

/// An item the hooks show: its sprite's id (never reused) and a mesh per bone it rides, held
/// with the item so its address isn't reused while it is here.
struct Kept {
    sprite: u64,
    item: Arc<Item>,
    meshes: Vec<(Bone, Arc<Mesh>)>,
}

/// The hooks' model of an item's part.
fn mesh(p: &horizon_cosmetics::Part) -> Mesh {
    let quads = p.faces.iter().map(|f| Quad::new(f.normal, f.vertices));
    Mesh::new(
        p.name.clone(),
        p.pose,
        quads.collect(),
        p.children.iter().map(mesh).collect(),
    )
}

/// The hooks' name of a bone.
fn bone(b: horizon_cosmetics::Bone) -> Bone {
    use horizon_cosmetics::Bone as B;
    match b {
        B::Head => Bone::Head,
        B::Body => Bone::Body,
        B::LeftArm => Bone::LeftArm,
        B::RightArm => Bone::RightArm,
        B::LeftLeg => Bone::LeftLeg,
        B::RightLeg => Bone::RightLeg,
    }
}

/// What the game tells the hooks before each call, until the settings move here (spec #14091
/// step 5a) and the player's emote with them (step 4).
#[derive(Default)]
struct Said {
    mine: bool,
    listed: bool,
    /// Other players' cosmetics are shown (not "name tags only", not hidden).
    others_shown: bool,
    /// Other players' cosmetics are hidden (not even name tag styles).
    others_hidden: bool,
    /// Horizon's mark by Horizon players' names.
    marks: bool,
    /// Other players' trails and auras are hidden.
    hide_fx: bool,
    /// Cosmetics in single player: the signed-in account's on the player's own model.
    solo: bool,
    /// My emote pulled the camera back: when it started (the own model's age).
    pull: Option<f32>,
    /// My emote while it plays: its id and whether it loops.
    playing: Option<(String, bool)>,
    /// A folder's first emote, which mannequins play.
    first: Option<Arc<Item>>,
}

impl Said {
    /// What the game's settings say.
    fn settings(&mut self, extras: &horizon_client::extras::Extras) {
        self.mine = extras.cosmetics_mine;
        self.listed = extras.horizon_listed;
        self.others_shown = extras.effective_others() == Others::Shown;
        self.others_hidden = extras.effective_others() == Others::Hidden;
        self.marks = extras.horizon_marks;
        self.hide_fx = extras.cosmetics_hide_fx;
        self.solo = extras.cosmetics_solo;
    }
}

/// One [`Seat`] per splitscreen player.
static SERVICES: Mutex<Vec<Seat>> = Mutex::new(Vec::new());

/// Runs `f` on this seat's [`Seat`]. Never called inside itself.
fn service<R>(f: impl FnOnce(&mut Seat) -> R) -> R {
    let seat = horizon_hooks::seat();
    let mut all = SERVICES.lock().unwrap_or_else(PoisonError::into_inner);
    if all.len() <= seat {
        all.resize_with(seat + 1, Seat::default);
    }
    f(&mut all[seat])
}

/// The cosmetics' hooks, registered when [`enabled`].
pub(super) struct Hooks;
pub(super) static HOOKS: Hooks = Hooks;

impl horizon_hooks::Hooks for Hooks {
    fn frame(&self, frame: &Frame, sprites: &mut dyn SpriteStore) {
        service(|s| {
            s.solo = frame.solo;
            s.frame();
            s.fx.frame();
            s.put_sprites(sprites);
        });
    }

    fn tick(&self, models: &mut dyn PlayerModels) {
        service(|s| s.tick(models));
    }

    fn name_decoration(
        &self,
        model: &PlayerModel,
        sprites: &dyn Sprites,
    ) -> Option<NameDecoration> {
        service(|s| s.name_decoration(model, sprites))
    }

    fn player_mark(&self, uuid: u128) -> Option<Mark> {
        service(|s| {
            let client = s.client.as_ref().filter(|_| s.said.marks)?;
            client.frame().horizon(uuid).then_some(MARK)
        })
    }

    /// An emote pulls the camera back: it eases out over the emote's first quarter second.
    fn player_layers(&self, model: &PlayerModel, draw: &mut dyn PlayerDraw) {
        service(|s| s.player_layers(model, draw));
    }

    /// The player's own companion in first person.
    fn first_person(&self, own: &PlayerModel, draw: &mut dyn PlayerDraw) {
        service(|s| s.first_person(own, draw));
    }

    fn camera(&self, own: &CameraView) -> Option<CameraOverride> {
        let start = service(|s| s.said.pull)?;
        let t = ((own.age - start) / 5.0).clamp(0.0, 1.0);
        Some(CameraOverride::new(0.35 + 0.65 * t * t * (3.0 - 2.0 * t)))
    }
}

impl Seat {
    /// Once a frame: starts the service's client on first use, asks whether the account owns a
    /// cosmetic, and tells the service what the account shows.
    fn frame(&mut self) {
        if self.client.is_none() {
            let (client, local) = Cosmetics::client();
            self.client = Some(Arc::new(client));
            self.local = local;
            // Others' emotes come from the service only (a local folder has no one playing).
            if !local {
                self.feed = Some(horizon_cosmetics::Feed::new(http()));
            }
        }
        // Item files from a folder (the art, the bench): shown as to an owner, nothing asked.
        if self.local {
            self.owner.owns = true;
            return;
        }
        // Whether the account owns a cosmetic: what the last run learnt, then the service.
        let o = &mut self.owner;
        if !o.started {
            o.started = true;
            o.owns = account()
                .is_some_and(|(_, uuid, home)| prefs::owned(&home, &uuid).unwrap_or(false));
        }
        if let Some(done) = o.asking.as_ref().and_then(|rx| rx.try_recv().ok()) {
            o.asking = None;
            match done {
                Ok(owns) => (o.owns, o.known, o.retry) = (owns, true, None),
                Err(_) => o.retry = backoff(o.retry),
            }
        }
        // One sign-in at a time: Mojang keeps one joined server id per account.
        let due = !o.known && o.retry.is_none_or(|(at, _)| Instant::now() >= at);
        if due && o.asking.is_none() && self.mine.sending.is_none() && self.listed.sending.is_none()
        {
            let (tx, rx) = channel();
            o.asking = Some(rx);
            std::thread::Builder::new()
                .name("Cosmetics account".into())
                .spawn(move || {
                    let _ = tx.send(check_owner());
                })
                .ok();
        }
        // Show my cosmetics: sent once per change of the wish, then retried until confirmed.
        // Show me as a Horizon player: also said again every hour while on.
        let every = horizon_cosmetics::account::PRESENCE_EVERY;
        self.mine.settle(None);
        self.listed.settle(Some(every));
        // One sign-in at a time, as for the ownership check above.
        let busy = self.owner.asking.is_some()
            || self.mine.sending.is_some()
            || self.listed.sending.is_some();
        let said = &self.said;
        if let Some(done) = self
            .emote_out
            .sending
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.emote_out.sending = None;
            match done {
                Ok(()) => self.emote_out.retry = None,
                Err(_) => {
                    self.emote_out.retry = backoff(self.emote_out.retry);
                    self.emote_out.sent = None;
                }
            }
        }
        // My emote: only while I show myself as a Horizon player (playing one says so).
        let playing = said.playing.as_ref();
        let looping = playing.is_some_and(|(_, looping)| *looping);
        let wish = playing.map(|(id, _)| id.as_str()).filter(|_| said.listed);
        let o = &mut self.emote_out;
        let changed = o.sent.as_ref().map(Option::as_deref) != Some(wish);
        let again = looping && o.again.is_some_and(|at| Instant::now() >= at);
        let waited = o.retry.is_none_or(|(at, _)| Instant::now() >= at);
        // Nothing sent yet and nothing playing: nothing to say (a failed send forgets what
        // was sent: the service ends a one-shot by itself, a loop within a minute).
        let quiet = wish.is_none() && o.sent.is_none();
        let emote_due = !quiet && waited && (changed || again);
        // One sign-in at a time, the emote's included.
        if busy || o.sending.is_some() {
        } else if emote_due {
            let wish = wish.map(str::to_owned);
            o.sent = Some(wish.clone());
            o.again = looping.then(|| Instant::now() + LOOP_AGAIN);
            let (tx, rx) = channel();
            o.sending = Some(rx);
            let kept = o.kept.clone();
            std::thread::Builder::new()
                .name("Cosmetics emote".into())
                .spawn(move || {
                    let _ = tx.send(send_emote(&kept, wish.as_deref()));
                })
                .ok();
        } else if self.mine.due(said.mine) {
            self.mine.start(said.mine, send_mine);
        } else if self.listed.due(said.listed) {
            self.listed.start(said.listed, send_listed);
        }
    }

    /// What goes by `m`'s name: Horizon's mark if they play Horizon (and the mark is on), the
    /// name tag style they wear if cosmetics are shown for them (name tags only counts), and how
    /// high what is drawn on their body gets (`body_top`). One look in the cache for all three.
    fn name_decoration(
        &mut self,
        m: &PlayerModel,
        sprites: &dyn Sprites,
    ) -> Option<NameDecoration> {
        let said = &self.said;
        let own = m.kind == Kind::Own;
        let styles = if own { said.mine } else { !said.others_hidden };
        if !(styles || said.marks) {
            return None;
        }
        let uuid = self.uuid_of(m)?;
        let mut frame = self.client.as_ref()?.frame();
        let mark = (said.marks && frame.horizon(uuid)).then_some(MARK);
        let worn = if styles { frame.worn(uuid) } else { None };
        drop(frame);
        let others = if said.others_shown {
            Others::Shown
        } else {
            Others::NameTags
        };
        let top = body_top(others, own, worn.as_ref(), |slot| covered(m.gear, slot))
            .filter(|_| !m.invisible);
        let badge = worn.and_then(|w| self.badge(&w, sprites));
        Some(NameDecoration::new(mark, badge, top))
    }

    /// The UUID the service knows `m` by: a player's, or a mannequin's id with local items.
    fn uuid_of(&self, m: &PlayerModel) -> Option<u128> {
        match m.kind {
            Kind::Own => own_uuid(m.uuid, self.solo, self.said.solo, || {
                horizon_client::auth::current().map(|s| s.uuid)
            }),
            Kind::Mannequin if self.local => Some(u128::from(m.id)),
            Kind::Player => m.uuid,
            _ => None,
        }
    }

    /// `item` as the hooks show it, kept from its first sight.
    fn kept(&mut self, item: &Arc<Item>) -> &Kept {
        let next = self.last_sprite + 1;
        let kept = self
            .items
            .entry(Arc::as_ptr(item) as usize)
            .or_insert_with(|| Kept {
                sprite: next,
                item: item.clone(),
                meshes: item
                    .roots()
                    .map(|(b, root)| (bone(b), Arc::new(mesh(root))))
                    .collect(),
            });
        self.last_sprite = self.last_sprite.max(kept.sprite);
        kept
    }

    /// Where `item`'s texture is in the atlas (`get` reads the crate's sprites): the top left
    /// corner of its slot. None until it is there (it is put there next frame).
    fn uv(&mut self, item: &Arc<Item>, get: impl Fn(u64) -> Option<[f32; 4]>) -> Option<[f32; 4]> {
        let id = self.kept(item).sprite;
        let Some([u0, v0, u1, v1]) = get(id) else {
            if !self.wanted.contains(&id) {
                self.wanted.push(id);
            }
            return None;
        };
        let (w, h) = (
            item.texture.width as f32 / 64.0,
            item.texture.height as f32 / 64.0,
        );
        Some([u0, v0, u0 + (u1 - u0) * w, v0 + (v1 - v0) * h])
    }

    /// The name tag style in `worn`: its badge and its line's colour. None until the badge is in
    /// the atlas.
    fn badge(&mut self, worn: &horizon_cosmetics::Worn, sprites: &dyn Sprites) -> Option<Badge> {
        let item = worn.iter().find(|i| i.slot == Slot::NameTag)?;
        let [r, g, b] = item.tag?.line;
        let uv = self.uv(item, |id| sprites.get(id))?;
        Some(Badge::new(uv, u32::from_be_bytes([0, r, g, b])))
    }

    /// Puts the sprites drawing missed in the atlas; forgets the items no one wears.
    fn put_sprites(&mut self, store: &mut dyn SpriteStore) {
        for id in self.wanted.drain(..) {
            let kept = self.items.values().find(|k| k.sprite == id);
            if let Some(kept) = kept.filter(|_| !store.has(id)) {
                store.put(id, slot_image(&kept.item));
            }
        }
        self.items.retain(|_, k| Arc::strong_count(&k.item) > 1);
    }

    /// The cosmetics `m` wears, on its bones and around it in the world (companions, trails,
    /// auras: `cosmetics/fx.rs`), and an emote's props.
    fn player_layers(&mut self, m: &PlayerModel, draw: &mut dyn PlayerDraw) {
        if m.invisible {
            return;
        }
        let said = &self.said;
        let own = m.kind == Kind::Own;
        // "Name tags only": other players' styles beside their names, nothing on their bodies.
        let bodies = own || said.others_shown;
        let ticks = draw.ticks();
        // An emote's props: its time and weight while it plays.
        let emote = emotes::pose_playing(draw.body_pose()).and_then(|p| {
            let (t, w) = p.at(ticks)?;
            (p.item.vertices > 0).then(|| (p.item.clone(), t, w))
        });
        let client = self.client.as_ref().filter(|_| bodies);
        let worn = client.and_then(|c| worn_of(c, &self.said, own, || self.uuid_of(m)));
        // Items on one bone (a back item, a shoulder pet and a shirt ride the body) share its
        // matrix: the head, the body and the two arms at most.
        let mut bones: [Option<(Bone, [[f32; 4]; 4])>; 4] = [None; 4];
        let worn_items = worn.iter().flat_map(|w| w.iter()).map(|i| (i, None));
        let emote_items = emote.iter().map(|(i, t, w)| (i, Some((*t, *w))));
        for (item, emoting) in worn_items.chain(emote_items) {
            // On no bone, around the model in the world.
            if let (Slot::Companion | Slot::Trail | Slot::Aura, None) = (item.slot, emoting) {
                if draw.pass() == Pass::World {
                    self.fx(m, item, draw);
                }
                continue;
            }
            // Covered by gear, or drawn by no bone (a name tag style).
            if emoting.is_none() && covered(m.gear, item.slot) {
                continue;
            }
            let Some(uv) = self.uv(item, |id| draw.sprite(id)) else {
                continue;
            };
            for i in 0..self.kept(item).meshes.len() {
                let (bone_of, mesh) = self.kept(item).meshes[i].clone();
                let known = bones.iter().flatten().find(|(b, _)| *b == bone_of);
                let matrix = match known {
                    Some(&(_, matrix)) => matrix,
                    None => {
                        let Some(matrix) = draw.bone(bone_of) else {
                            continue;
                        };
                        if let Some(free) = bones.iter_mut().find(|b| b.is_none()) {
                            *free = Some((bone_of, matrix));
                        }
                        matrix
                    }
                };
                draw.draw(&mesh, matrix, Look::new(uv), &mut |p| {
                    item.animate(ticks, |name, pose| *p.pose(name) = pose);
                    if let (Some((t, w)), Some(e)) = (emoting, &item.emote) {
                        e.pose_props(t, |n| item.rest(n), |n, pose| *p.pose(n) = pose);
                        // Props grow in and shrink out with the emote's blend.
                        let root = p.pose(&mesh.name);
                        for s in &mut root[6..9] {
                            *s *= w;
                        }
                    }
                });
            }
        }
    }

    /// The window's own companion while its body is out of view.
    fn first_person(&mut self, m: &PlayerModel, draw: &mut dyn PlayerDraw) {
        let Some(client) = self.client.as_ref().filter(|_| !m.invisible) else {
            return;
        };
        let Some(worn) = worn_of(client, &self.said, true, || self.uuid_of(m)) else {
            return;
        };
        if let Some(item) = worn.iter().find(|i| i.slot == Slot::Companion) {
            self.fx(m, item, draw);
        }
    }

    /// Once a tick: names the Horizon players in view to the emote feed and plays what it says
    /// they play on their models; one that moved ends theirs here at once. Others' emotes show
    /// with "Other players' cosmetics" on Shown only; otherwise nothing is asked.
    fn tick(&mut self, models: &mut dyn PlayerModels) {
        let mut seen = std::mem::take(&mut self.models);
        seen.clear();
        models.for_each(&mut |m| {
            if m.kind != Kind::Own {
                seen.push(m);
            }
        });
        self.play_others(models, &mut seen);
        self.models = seen;
    }

    fn play_others(&self, models: &mut dyn PlayerModels, view: &mut Vec<PlayerModel>) {
        let shown = self.said.others_shown;
        // Items from a folder (the art, the bench): every mannequin plays its first emote, again
        // and again.
        if self.local && shown {
            let Some(first) = &self.said.first else {
                return;
            };
            for m in view.iter().filter(|m| m.kind == Kind::Mannequin) {
                let age = m.age as f32;
                let now = emotes::pose_playing(models.body_pose(m.id));
                if now.is_none_or(|p| p.at(age).is_none()) {
                    let playing = horizon_cosmetics::Playing {
                        item: first.clone(),
                        // Out of step, so a crowd doesn't move as one.
                        start: age - (m.id % 40) as f32,
                        stop: None,
                    };
                    emotes::set_model_playing(models, m.id, Some(playing));
                }
            }
            return;
        }
        let (Some(feed), Some(client)) = (&self.feed, &self.client) else {
            return;
        };
        if !shown {
            feed.ask([]);
            for m in view.iter() {
                emotes::set_model_playing(models, m.id, None);
            }
            return;
        }
        let mut frame = client.frame();
        view.retain(|m| m.kind == Kind::Player && m.uuid.is_some_and(|u| frame.horizon(u)));
        drop(frame);
        feed.ask(view.iter().filter_map(|m| m.uuid));
        let now = Instant::now();
        for m in view.iter() {
            let Some(uuid) = m.uuid else { continue };
            let seen = feed.playing(uuid);
            let age = m.age as f32;
            let mut next = emotes::pose_playing(models.body_pose(m.id)).cloned();
            match (seen, &mut next) {
                (Some(seen), Some(p)) if Arc::ptr_eq(&p.item, &seen.item) => {
                    if m.moved && p.stop.is_none() {
                        p.stop = Some(age);
                    }
                }
                (Some(seen), slot) if !m.moved => {
                    let ticks = now.duration_since(seen.started).as_secs_f32() * 20.0;
                    *slot = Some(horizon_cosmetics::Playing {
                        item: seen.item,
                        start: age - ticks,
                        stop: None,
                    });
                }
                (_, Some(p)) if p.stop.is_none() => p.stop = Some(age),
                _ => {}
            }
            emotes::set_model_playing(models, m.id, next.filter(|p| p.at(age).is_some()));
        }
    }
}

impl Game<'_> {
    /// Hands the hooks what the game's settings and the player's emote say (allocating only when
    /// the emote changes).
    pub(super) fn cosmetics_say(&mut self) {
        if !enabled() {
            return;
        }
        let first = if self.cosmetics.local {
            service(|s| s.said.first.is_none())
                .then(|| self.owned_emotes().first().cloned())
                .flatten()
        } else {
            None
        };
        let extras = &self.settings.extras;
        let playing = self.cosmetics.emotes.sending();
        let pull = self.cosmetics.emotes.pull();
        service(|s| {
            let said = &mut s.said;
            said.settings(extras);
            said.pull = pull;
            if said.playing.as_ref().map(|(id, l)| (id.as_str(), *l)) != playing {
                said.playing = playing.map(|(id, l)| (id.to_owned(), l));
            }
            if first.is_some() {
                said.first = first;
            }
        });
    }

    /// Once a frame, after the hooks: what the game reads of them (the settings' owner flag,
    /// whether items come from a folder).
    pub(super) fn cosmetics_frame(&mut self) {
        if !enabled() {
            return;
        }
        let (owns, local) = service(|s| (s.owner.owns, s.local));
        // Read every frame: the settings button shows it, and it is never saved.
        self.settings.extras.cosmetics_owner = owns;
        self.cosmetics.local = local;
    }

    /// Asks the service which emotes the account owns, for the wheel: when it opens, at most
    /// every 10 minutes (a folder's emotes need no asking; an offline account has none).
    pub(super) fn refresh_owned_emotes(&mut self) {
        const AGAIN: Duration = Duration::from_secs(600);
        let e = &mut self.cosmetics.emotes;
        if self.cosmetics.local
            || !enabled()
            || horizon_client::auth::current().is_none()
            || e.loading.is_some()
            || e.loaded.is_some_and(|at| at.elapsed() < AGAIN)
        {
            return;
        }
        let (tx, rx) = channel();
        e.loading = Some(rx);
        let kept = service(|s| s.emote_out.kept.clone());
        std::thread::Builder::new()
            .name("Cosmetics emotes owned".into())
            .spawn(move || {
                let _ = tx.send(load_owned_emotes(&kept));
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::extras::Extras;
    use horizon_client::mobs::anim::BodyPose;

    /// A name tag style with a 1×1 mark.
    const TAG: &str = r##"{"format": 1, "id": "tag", "slot": "name_tag", "parts": [],
        "texture": {"png": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="},
        "tag": {"line": "#a98fff"}}"##;

    /// A crown whose star tops out 20 pixels above the neck: a 2-pixel box on a head item
    /// attached 18 pixels up (a 16×16 texture).
    const CROWN: &str = r##"{"format": 1, "id": "crown", "slot": "head", "bone": "head",
        "attach": {"offset": [0, -18, 0]},
        "texture": {"png": "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAGklEQVR4nGM4scXmPyWYYdSAUQNGDRguBgAA+D23H0jXFFsAAAAASUVORK5CYII="},
        "parts": [{"name": "star", "cubes": [{"origin": [-1, -2, -1], "size": [2, 2, 2], "uv": [0, 0]}]}]}"##;

    /// The service: every player asked about wears its items (`TAG` by default); each request is
    /// logged.
    #[derive(Clone)]
    struct Service(
        Arc<Mutex<Vec<String>>>,
        &'static [(&'static str, &'static str)],
    );

    impl Default for Service {
        fn default() -> Self {
            Service(Arc::default(), &[("tag", TAG)])
        }
    }

    impl horizon_cosmetics::Transport for Service {
        fn post(&self, path: &str, body: &[u8]) -> Result<Vec<u8>, String> {
            let body: serde_json::Value = serde_json::from_slice(body).unwrap();
            self.0.lock().unwrap().push(format!("POST {path} {body}"));
            let players: serde_json::Map<_, _> = body["players"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let p = p.as_str().unwrap().to_owned();
                    let items: Vec<_> = (self.1.iter())
                        .map(|(id, _)| serde_json::json!({"id": id, "hash": "h"}))
                        .collect();
                    (p, serde_json::Value::from(items))
                })
                .collect();
            Ok(serde_json::json!({"ttl": 60, "players": players})
                .to_string()
                .into_bytes())
        }

        fn get(&self, path: &str) -> Result<Vec<u8>, String> {
            self.0.lock().unwrap().push(format!("GET {path}"));
            let (_, file) = (self.1.iter())
                .find(|(id, _)| path.starts_with(&format!("/v1/items/{id}?")))
                .ok_or_else(|| format!("no item at {path}"))?;
            Ok(file.as_bytes().to_vec())
        }
    }

    const OTHER: u128 = 0x1234;

    /// What `extras` tell the hooks.
    fn said_of(extras: &Extras) -> Said {
        let mut said = Said::default();
        said.settings(extras);
        said
    }

    /// What another player wears as `extras` shows it, drawn frame after frame for up to
    /// `patience` (the service answers on its own thread), and what was asked of the service.
    fn other_player(
        extras: &Extras,
        patience: Duration,
    ) -> (Option<horizon_cosmetics::Worn>, Vec<String>) {
        other_player_from(Service::default(), extras, patience)
    }

    fn other_player_from(
        service: Service,
        extras: &Extras,
        patience: Duration,
    ) -> (Option<horizon_cosmetics::Worn>, Vec<String>) {
        let client = horizon_cosmetics::Cosmetics::new(service.clone(), None);
        let deadline = Instant::now() + patience;
        let worn = loop {
            let worn = worn_of(&client, &said_of(extras), false, || Some(OTHER));
            if worn.is_some() || Instant::now() >= deadline {
                break worn;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let asked = service.0.lock().unwrap().clone();
        (worn, asked)
    }

    #[test]
    fn a_player_who_owns_none_sees_and_asks_nothing_until_they_choose() {
        use horizon_cosmetics::prefs::Others;
        // No line, owns nothing: a vanilla game, and no request about the other player.
        let (short, long) = (Duration::from_millis(300), Duration::from_secs(10));
        let (worn, asked) = other_player(&Extras::default(), short);
        assert!(worn.is_none());
        assert_eq!(asked, Vec::<String>::new());
        // The service says this account owns a cosmetic: the other player's style shows.
        let owner = Extras {
            cosmetics_owner: true,
            ..Extras::default()
        };
        let (worn, asked) = other_player(&owner, long);
        assert_eq!(
            worn.map(|w| w.iter().map(|i| i.id.clone()).collect::<Vec<_>>()),
            Some(vec!["tag".to_owned()])
        );
        assert_eq!(
            asked[0],
            format!("POST /v1/worn {{\"players\":[\"{OTHER:032x}\"]}}")
        );
        // A choice wins over ownership both ways.
        let chose = Extras {
            cosmetics_others: Some(Others::Shown),
            ..Extras::default()
        };
        assert!(other_player(&chose, long).0.is_some());
        let hid = Extras {
            cosmetics_others: Some(Others::Hidden),
            ..owner
        };
        let (worn, asked) = other_player(&hid, short);
        assert!(worn.is_none() && asked.is_empty());
    }

    #[test]
    fn single_player_shows_my_cosmetics_only_with_its_setting_and_asks_by_my_account() {
        // The window's own server names the player by their offline profile.
        const OFFLINE: u128 = 0x0ff1;
        const ACCOUNT: u128 = 0xacc0;
        let mine = |solo: bool, shown: bool, patience: Duration| {
            let service = Service::default();
            let client = horizon_cosmetics::Cosmetics::new(service.clone(), None);
            let extras = Extras {
                cosmetics_solo: shown,
                ..Extras::default()
            };
            let uuid = || own_uuid(Some(OFFLINE), solo, extras.cosmetics_solo, || Some(ACCOUNT));
            let deadline = Instant::now() + patience;
            let worn = loop {
                let worn = worn_of(&client, &said_of(&extras), true, uuid);
                if worn.is_some() || Instant::now() >= deadline {
                    break worn;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let asked = service.0.lock().unwrap().clone();
            (worn.is_some(), asked)
        };
        let (short, long) = (Duration::from_millis(300), Duration::from_secs(10));
        // Off by default: nothing drawn, nothing asked.
        assert_eq!(mine(true, false, short), (false, vec![]));
        // On: asked by the signed-in account, never by the offline profile.
        let (shown, asked) = mine(true, true, long);
        assert!(shown);
        assert_eq!(
            asked[0],
            format!("POST /v1/worn {{\"players\":[\"{ACCOUNT:032x}\"]}}")
        );
        // On a server the setting changes nothing: the server's profile.
        let (shown, asked) = mine(false, false, long);
        assert!(shown);
        assert_eq!(
            asked[0],
            format!("POST /v1/worn {{\"players\":[\"{OFFLINE:032x}\"]}}")
        );
        // An offline account in single player: nothing to ask about.
        assert_eq!(own_uuid(Some(OFFLINE), true, true, || None), None);
    }

    #[test]
    fn a_tall_cosmetic_lifts_the_name_tag_only_while_it_is_drawn() {
        use super::super::names::{NAME_TAG_UNIT, cosmetic_lift};
        use horizon_cosmetics::prefs::Others;
        // Vanilla: EntityAttachment.NAME_TAG of a standing player, 1.8 + 0.5 above the feet;
        // the tag's lowest pixel 10 text pixels under it.
        const VANILLA: f64 = 2.3;
        let unit = f64::from(NAME_TAG_UNIT);
        let tag = |top: Option<f32>| VANILLA + top.map_or(0.0, |t| cosmetic_lift(t, 1.0));
        // The star's top: 20 pixels above the neck, which is 1.501 blocks up the model, scaled
        // by the avatar's 0.9375.
        let star = 0.9375 * (1.501 + 20.0 / 16.0);
        let both: &'static [(&str, &str)] = &[("tag", TAG), ("crown", CROWN)];
        let shown = Extras {
            cosmetics_others: Some(Others::Shown),
            ..Extras::default()
        };
        let (short, long) = (Duration::from_millis(300), Duration::from_secs(10));
        let top_of = |extras: &Extras, patience| {
            let (worn, _) = other_player_from(Service(Arc::default(), both), extras, patience);
            body_top(extras.effective_others(), false, worn.as_ref(), |_| false)
        };
        // No cosmetics drawn on the body (a name tag style only): vanilla's height exactly.
        let (worn, _) = other_player(&shown, long);
        assert!(worn.is_some());
        assert_eq!(
            tag(body_top(
                shown.effective_others(),
                false,
                worn.as_ref(),
                |_| false
            )),
            VANILLA
        );
        // A small hat under the tag: vanilla's height too.
        assert_eq!(cosmetic_lift(10.0, 1.0), 0.0);
        // The crown: the whole tag just over the star.
        let crowned = tag(top_of(&shown, long));
        let bottom = crowned - 10.0 * unit;
        assert!(
            bottom > star && bottom < star + 2.0 * unit,
            "{bottom} over {star}"
        );
        // A helmet over it: vanilla's height.
        let (worn, _) = other_player_from(Service(Arc::default(), both), &shown, long);
        let helmet = body_top(shown.effective_others(), false, worn.as_ref(), |slot| {
            slot == Slot::Head
        });
        assert_eq!(tag(helmet), VANILLA);
        // Others' cosmetics on name tags only, or hidden: nothing on the body, vanilla's height.
        for others in [Others::NameTags, Others::Hidden] {
            let extras = Extras {
                cosmetics_others: Some(others),
                ..Extras::default()
            };
            let patience = if others == Others::Hidden {
                short
            } else {
                long
            };
            assert_eq!(tag(top_of(&extras, patience)), VANILLA, "{others:?}");
        }
    }

    /// Player models as the hooks see them, with the pose laid over each.
    struct View(Vec<(PlayerModel, Option<Arc<dyn BodyPose>>)>);

    impl PlayerModels for View {
        fn connection(&self) -> u64 {
            1
        }
        fn for_each(&self, f: &mut dyn FnMut(PlayerModel)) {
            self.0.iter().for_each(|(m, _)| f(*m));
        }
        fn body_pose(&self, id: u32) -> Option<&Arc<dyn BodyPose>> {
            self.0.iter().find(|(m, _)| m.id == id)?.1.as_ref()
        }
        fn set_body_pose(&mut self, id: u32, pose: Option<Arc<dyn BodyPose>>) {
            if let Some((_, p)) = self.0.iter_mut().find(|(m, _)| m.id == id) {
                *p = pose;
            }
        }
    }

    #[test]
    fn with_a_folder_mannequins_play_its_first_emote_out_of_step_and_players_do_not() {
        let item = Arc::new(
            Item::parse(
                br#"{"format": 1, "id": "up", "slot": "emote", "parts": [],
                    "emote": {"length": 20, "blend": 0, "tracks": [
                      {"part": "right_arm", "keys": [{"t": 10, "rotation": [0, 0, 90]}]}]}}"#,
            )
            .unwrap(),
        );
        let mut service = Seat {
            local: true,
            said: Said {
                others_shown: true,
                first: Some(item.clone()),
                ..Said::default()
            },
            ..Seat::default()
        };
        let model = |id, kind, uuid| (PlayerModel::new(id, kind, uuid, 50, false), None);
        let mut view = View(vec![
            model(1, Kind::Own, None),
            model(41, Kind::Mannequin, None),
            model(2, Kind::Player, Some(7)),
        ]);
        service.tick(&mut view);
        let playing = emotes::pose_playing(view.0[1].1.as_ref()).expect("the mannequin plays");
        assert!(Arc::ptr_eq(&playing.item, &item));
        // Out of step by its id: 50 - 41 % 40.
        assert_eq!(playing.start, 49.0);
        assert!(view.0[0].1.is_none() && view.0[2].1.is_none());
        // Still playing: the same pose stays.
        let first = view.0[1].1.clone().unwrap();
        service.tick(&mut view);
        assert!(Arc::ptr_eq(&first, view.0[1].1.as_ref().unwrap()));
        // Others' cosmetics not shown: a new mannequin plays nothing.
        service.said.others_shown = false;
        view.0.push(model(3, Kind::Mannequin, None));
        service.tick(&mut view);
        assert!(view.0[3].1.is_none());
    }

    /// A player model as `player_layers` draws on it: one bone, the sprites it is given; logs
    /// each draw (mesh, matrix, uv, its root's scale once posed).
    #[derive(Default)]
    struct Canvas {
        pose: Option<Arc<dyn BodyPose>>,
        sprite: Option<[f32; 4]>,
        drawn: Vec<(String, [[f32; 4]; 4], [f32; 4], [f32; 3])>,
        /// The model's matrix (`LEFT_ARM` if None) and how it moves.
        base: Option<glam::Mat4>,
        motion: horizon_hooks::players::Motion,
        /// Each draw's look: see-through alpha and light.
        looks: Vec<(Option<f32>, horizon_hooks::players::Light)>,
    }

    const LEFT_ARM: [[f32; 4]; 4] = [[2.0, 0.0, 0.0, 0.0]; 4];

    impl PlayerDraw for Canvas {
        fn pass(&self) -> horizon_hooks::players::Pass {
            horizon_hooks::players::Pass::World
        }
        fn ticks(&self) -> f32 {
            102.0
        }
        fn base(&self) -> [[f32; 4]; 4] {
            self.base.map_or(LEFT_ARM, |b| b.to_cols_array_2d())
        }
        fn motion(&self) -> horizon_hooks::players::Motion {
            self.motion
        }
        fn view(&self) -> horizon_hooks::players::View {
            Default::default()
        }
        fn particles(
            &mut self,
            _: [f32; 4],
            _: &[horizon_hooks::players::Particle],
            _: horizon_hooks::players::Light,
        ) {
        }
        fn bone(&mut self, bone: Bone) -> Option<[[f32; 4]; 4]> {
            (bone == Bone::LeftArm).then_some(LEFT_ARM)
        }
        fn body_pose(&self) -> Option<&Arc<dyn BodyPose>> {
            self.pose.as_ref()
        }
        fn sprite(&self, _: u64) -> Option<[f32; 4]> {
            self.sprite
        }
        fn draw(
            &mut self,
            mesh: &Arc<Mesh>,
            matrix: [[f32; 4]; 4],
            look: Look,
            pose: &mut dyn FnMut(&mut dyn horizon_hooks::players::PartPoses),
        ) {
            struct Rest(std::collections::HashMap<String, [f32; 9]>);
            impl horizon_hooks::players::PartPoses for Rest {
                fn pose(&mut self, name: &str) -> &mut [f32; 9] {
                    self.0.entry(name.to_owned()).or_insert([0.0; 9])
                }
            }
            let mut rest = Rest([(mesh.name.clone(), mesh.pose)].into());
            pose(&mut rest);
            let root = rest.0[&mesh.name];
            self.looks.push((look.see_through, look.light));
            self.drawn.push((
                mesh.name.clone(),
                matrix,
                look.uv,
                [root[6], root[7], root[8]],
            ));
        }
    }

    #[test]
    fn an_emotes_prop_goes_on_its_bone_once_its_texture_is_in_scaled_by_the_blend() {
        let pop = CROWN
            .replace(
                r#""slot": "head", "bone": "head""#,
                r#""slot": "emote", "bone": "left_arm""#,
            )
            .replace(
                r#""uv": [0, 0]}]}]}"#,
                r#""uv": [0, 0]}]}],
                "emote": {"length": 40, "blend": 4, "tracks": [
                  {"part": "right_arm", "keys": [{"t": 0}, {"t": 40}]}]}}"#,
            );
        let item = Arc::new(Item::parse(pop.as_bytes()).unwrap());
        let playing = horizon_cosmetics::Playing {
            item: item.clone(),
            start: 100.0,
            stop: None,
        };
        let (_, weight) = playing.at(102.0).unwrap();
        assert!(weight > 0.0 && weight < 1.0, "{weight}");
        let player = PlayerModel::new(9, Kind::Player, Some(5), 102, false);
        let mut view = View(vec![(player, None)]);
        emotes::set_model_playing(&mut view, 9, Some(playing));
        let mut canvas = Canvas {
            pose: view.0[0].1.clone(),
            ..Canvas::default()
        };
        // Others' cosmetics hidden, no service: the player's own emote still shows.
        let mut seat = Seat {
            said: Said {
                others_hidden: true,
                ..Said::default()
            },
            ..Seat::default()
        };
        // Its texture not in the atlas yet: nothing drawn, the sprite asked for.
        seat.player_layers(&player, &mut canvas);
        assert!(canvas.drawn.is_empty());
        assert_eq!(seat.wanted.len(), 1);
        canvas.sprite = Some([0.0, 0.0, 0.5, 0.5]);
        seat.player_layers(&player, &mut canvas);
        // A 16×16 texture: the slot's top left quarter of it.
        let quarter = 0.5 * 16.0 / 64.0;
        assert_eq!(canvas.drawn.len(), 1);
        let (_, matrix, uv, scale) = &canvas.drawn[0];
        assert_eq!((matrix, uv), (&LEFT_ARM, &[0.0, 0.0, quarter, quarter]));
        for s in scale {
            assert!((s - weight).abs() < 1e-6, "{scale:?} by {weight}");
        }
    }

    #[test]
    fn an_aura_goes_see_through_at_the_feet_unless_swimming_far_or_hidden() {
        use horizon_hooks::players::{Light, Motion};
        let ring = CROWN
            .replace(r#""slot": "head", "bone": "head""#, r#""slot": "aura""#)
            .replace(
                r#""uv": [0, 0]}]}]}"#,
                r#""uv": [0, 0]}]}], "aura": {"at": "feet", "alpha": 0.5}}"#,
            );
        let ring = Arc::new(Item::parse(ring.as_bytes()).unwrap());
        let at = glam::Vec3::new(3.0, 0.0, 4.0);
        let mut canvas = Canvas {
            sprite: Some([0.0, 0.0, 0.5, 0.5]),
            base: Some(glam::Mat4::from_translation(at)),
            ..Canvas::default()
        };
        let other = PlayerModel::new(9, Kind::Player, Some(5), 102, false);
        let own = PlayerModel::new(1, Kind::Own, None, 102, false);
        let mut seat = Seat::default();
        seat.fx(&other, &ring, &mut canvas);
        // Half see-through, lit as the wearer, 24 pixels under the neck.
        assert_eq!(canvas.looks, [(Some(0.5), Light::Model)]);
        let feet = glam::Mat4::from_cols_array_2d(&canvas.drawn[0].1).w_axis;
        assert_eq!(feet.truncate(), at + glam::Vec3::new(0.0, 1.5, 0.0));
        // Swimming, it would tip over with the body: none.
        canvas.motion = Motion::default().poses(false, false, false, true);
        seat.fx(&other, &ring, &mut canvas);
        canvas.motion = Motion::default();
        // Past 48 blocks: none.
        canvas.base = Some(glam::Mat4::from_translation(glam::Vec3::X * 49.0));
        seat.fx(&other, &ring, &mut canvas);
        assert_eq!(canvas.looks.len(), 1);
        // Others' trails and auras hidden: theirs go, the player's own stays.
        canvas.base = Some(glam::Mat4::from_translation(at));
        seat.said.hide_fx = true;
        seat.fx(&other, &ring, &mut canvas);
        assert_eq!(canvas.looks.len(), 1);
        seat.fx(&own, &ring, &mut canvas);
        assert_eq!(canvas.looks.len(), 2);
    }
}
