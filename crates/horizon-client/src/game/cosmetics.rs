//! Cosmetics on players ([docs/cosmetics.md](../../../../docs/cosmetics.md)): only Horizon
//! clients draw them, off unless [`enabled`]. What another player wears comes
//! from the cosmetics service by UUID (`horizon_cosmetics::Cosmetics`, its own thread); this file
//! puts the items on the avatar's bones, on the GPU skin path when the renderer takes it.
//!
//! Other players' cosmetics show by default only to an account that owns one (asked of the
//! service once a run, `OwnerCheck`): anyone else sees a vanilla game and the game asks nothing
//! about other players, until the player chooses otherwise in the settings.

mod fx;

use super::{Game, emotes};
use horizon_client::fast_hash::FxHashMap;
use horizon_client::mesher::Vertex;
use horizon_client::mobs::anim::Poses;
use horizon_client::mobs::{Mob, Part, Polygon};
use horizon_cosmetics::follow::Follow;
use horizon_cosmetics::prefs::{self, Others};
use horizon_cosmetics::trail::{Budget, Emitter};
use horizon_cosmetics::{Item, Slot};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// An item as the renderer takes it.
struct Baked {
    item: Arc<Item>,
    /// A model per bone it rides (a shirt's sleeves are on the arms).
    models: Vec<(horizon_cosmetics::Bone, Arc<Part>)>,
    /// Its texture's sprite (a 64×64 skin slot of the atlas).
    location: String,
    /// The frame it was last marked drawn in (its slot is kept).
    marked: Cell<u32>,
}

impl Baked {
    fn new(item: &Arc<Item>, frame: u32) -> Self {
        Self {
            item: item.clone(),
            models: item
                .roots()
                .map(|(bone, root)| (bone, Arc::new(part(root))))
                .collect(),
            location: format!("cosmetics/{:x}", Arc::as_ptr(item) as usize),
            marked: Cell::new(frame.wrapping_sub(1)),
        }
    }
}

#[derive(Default)]
pub(super) struct Cosmetics {
    client: Option<horizon_cosmetics::Cosmetics>,
    /// By the item's address; each entry holds its item, so the address isn't reused.
    baked: RefCell<FxHashMap<usize, Baked>>,
    /// Items drawn whose texture isn't in the atlas yet.
    uploads: RefCell<Vec<Arc<Item>>>,
    frame: Cell<u32>,
    /// Kept between players so a frame of avatars allocates none: the wearer's body, an item.
    poses: RefCell<(Poses, Poses)>,
    /// Item files from a folder (`HORIZON_COSMETICS_LOCAL`): mannequins wear them too.
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
    /// Companions, trails and auras.
    fx: RefCell<Fx>,
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

/// The key of the player's own model in `Fx`'s maps (others go by entity id).
const OWN: u32 = u32::MAX;
/// Full light, for what glows.
const FULL_LIGHT: u32 = 240 | 240 << 8;

/// What companions and trails keep between frames, per wearer (marked with the frame they were
/// last drawn in; the others are dropped), and the frame's trails and auras, drawn after the
/// players (`cosmetics_fx_geometry`).
#[derive(Default)]
struct Fx {
    companions: FxHashMap<u32, (Follow, u32)>,
    trails: FxHashMap<u32, (Emitter, u32)>,
    budget: Budget,
    clock: Option<(Instant, Instant)>,
    /// Ticks since the first frame, and the last frame's length in seconds.
    now: f64,
    dt: f32,
    /// Trails and auras: see-through.
    clear: Vec<Vertex>,
}

impl Fx {
    /// A new frame: the clock moves on and what no frame drew since the last one goes.
    fn frame(&mut self, frame: u32) {
        let now = Instant::now();
        let (start, last) = self.clock.get_or_insert((now, now));
        self.dt = now.duration_since(*last).as_secs_f32().min(0.25);
        self.now = now.duration_since(*start).as_secs_f64() * 20.0;
        *last = now;
        let seen = |mark: u32| frame.wrapping_sub(mark) <= 1;
        self.companions.retain(|_, (_, mark)| seen(*mark));
        let budget = &mut self.budget;
        self.trails.retain(|_, (e, mark)| {
            let keep = seen(*mark);
            if !keep {
                e.clear(budget);
            }
            keep
        });
        self.clear.clear();
    }
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
    extras: &horizon_client::extras::Extras,
    own: bool,
    uuid: impl FnOnce() -> Option<u128>,
) -> Option<horizon_cosmetics::Worn> {
    let shown = if own {
        extras.cosmetics_mine
    } else {
        extras.effective_others() != Others::Hidden
    };
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

/// The game's model of an item's part.
fn part(p: &horizon_cosmetics::Part) -> Part {
    Part {
        name: p.name.clone(),
        pose: p.pose,
        polygons: p
            .faces
            .iter()
            .map(|f| Polygon {
                normal: f.normal,
                vertices: f.vertices,
                cull_back: false,
            })
            .collect(),
        children: p.children.iter().map(part).collect(),
    }
}

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
fn covered(m: &Mob, slot: Slot) -> bool {
    match slot {
        Slot::Head => m.extra.head.is_some(),
        Slot::Back => m.cape.is_some() || m.extra.chest.as_deref() == Some("minecraft:elytra"),
        Slot::Shoulder => false,
        // A chestplate, not an elytra.
        Slot::Torso => m
            .extra
            .chest
            .as_deref()
            .is_some_and(|c| c != "minecraft:elytra"),
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
    extras: &horizon_client::extras::Extras,
    own: bool,
    worn: Option<&horizon_cosmetics::Worn>,
    covered: impl Fn(Slot) -> bool,
) -> Option<f32> {
    if !own && extras.effective_others() != Others::Shown {
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

impl Game<'_> {
    /// Once a frame, before drawing: starts the service's client on first use and puts the
    /// textures the last frame wanted into the atlas.
    pub(super) fn cosmetics_frame(&mut self) {
        if !enabled() {
            return;
        }
        let c = &mut self.cosmetics;
        if c.client.is_none() {
            let (client, local) = Cosmetics::client();
            c.client = Some(client);
            c.local = local;
            // Others' emotes come from the service only (a local folder has no one playing).
            if !local {
                c.feed = Some(horizon_cosmetics::Feed::new(http()));
            }
        }
        c.frame.set(c.frame.get().wrapping_add(1));
        c.fx.get_mut().frame(c.frame.get());
        // Item files from a folder (the art, the bench): shown as to an owner, nothing asked.
        if c.local {
            c.owner.owns = true;
        } else {
            // Whether the account owns a cosmetic: what the last run learnt, then the service.
            let o = &mut c.owner;
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
            if due && o.asking.is_none() && c.mine.sending.is_none() && c.listed.sending.is_none() {
                let (tx, rx) = channel();
                o.asking = Some(rx);
                std::thread::Builder::new()
                    .name("Cosmetics account".into())
                    .spawn(move || {
                        let _ = tx.send(check_owner());
                    })
                    .ok();
            }
        }
        // Read every frame: the settings button shows it, and it is never saved.
        self.settings.extras.cosmetics_owner = c.owner.owns;
        // Show my cosmetics: sent once per change of the wish, then retried until confirmed.
        // Show me as a Horizon player: also said again every hour while on.
        if !c.local {
            let every = horizon_cosmetics::account::PRESENCE_EVERY;
            c.mine.settle(None);
            c.listed.settle(Some(every));
            // One sign-in at a time, as for the ownership check above.
            let busy =
                c.owner.asking.is_some() || c.mine.sending.is_some() || c.listed.sending.is_some();
            let extras = &self.settings.extras;
            if let Some(done) = c
                .emote_out
                .sending
                .as_ref()
                .and_then(|rx| rx.try_recv().ok())
            {
                c.emote_out.sending = None;
                match done {
                    Ok(()) => c.emote_out.retry = None,
                    Err(_) => {
                        c.emote_out.retry = backoff(c.emote_out.retry);
                        c.emote_out.sent = None;
                    }
                }
            }
            // My emote: only while I show myself as a Horizon player (playing one says so).
            let playing = self.emotes.sending();
            let looping = playing.as_ref().is_some_and(|(_, looping)| *looping);
            let wish = playing.map(|(id, _)| id).filter(|_| extras.horizon_listed);
            let o = &mut c.emote_out;
            let changed = o.sent.as_ref() != Some(&wish);
            let again = looping && o.again.is_some_and(|at| Instant::now() >= at);
            let waited = o.retry.is_none_or(|(at, _)| Instant::now() >= at);
            // Nothing sent yet and nothing playing: nothing to say (a failed send forgets what
            // was sent: the service ends a one-shot by itself, a loop within a minute).
            let quiet = wish.is_none() && o.sent.is_none();
            let emote_due = !quiet && waited && (changed || again);
            // One sign-in at a time, the emote's included.
            if busy || o.sending.is_some() {
            } else if emote_due {
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
            } else if c.mine.due(extras.cosmetics_mine) {
                c.mine.start(extras.cosmetics_mine, send_mine);
            } else if c.listed.due(extras.horizon_listed) {
                c.listed.start(extras.horizon_listed, send_listed);
            }
        }
        let uploads = std::mem::take(&mut *c.uploads.borrow_mut());
        for item in uploads {
            let location = format!("cosmetics/{:x}", Arc::as_ptr(&item) as usize);
            if !self.gui.atlas.sprites.contains_key(&location) {
                self.store_skin(location, slot_image(&item));
            }
        }
        // Items no player wears any more.
        self.cosmetics
            .baked
            .borrow_mut()
            .retain(|_, b| Arc::strong_count(&b.item) > 1);
    }

    /// Once a tick: names the Horizon players in view to the emote feed and plays what it says
    /// they play on their models; one that moved ends theirs here at once. Others' emotes show
    /// with "Other players' cosmetics" on Shown only; otherwise nothing is asked.
    pub(super) fn tick_others_emotes(&mut self) {
        let shown = self.settings.extras.effective_others() == Others::Shown;
        // Items from a folder (the art, the bench): every mannequin plays its first emote, again
        // and again.
        if self.cosmetics.local && shown {
            let Some(first) = self.owned_emotes().first().cloned() else {
                return;
            };
            for m in self.mobs.iter_mut() {
                let age = m.age as f32;
                if &*m.kind.id == "minecraft:mannequin"
                    && emotes::playing(m).is_none_or(|p| p.at(age).is_none())
                {
                    let playing = horizon_cosmetics::Playing {
                        item: first.clone(),
                        // Out of step, so a crowd doesn't move as one.
                        start: age - (m.id % 40) as f32,
                        stop: None,
                    };
                    emotes::set_playing(m, Some(playing));
                }
            }
            return;
        }
        let (Some(feed), Some(client)) = (&self.cosmetics.feed, &self.cosmetics.client) else {
            return;
        };
        if !shown {
            feed.ask([]);
            for m in self.mobs.iter_mut() {
                emotes::set_playing(m, None);
            }
            return;
        }
        let mut frame = client.frame();
        let mut horizon = Vec::new();
        for m in self.mobs.iter() {
            if &*m.kind.id == "minecraft:player"
                && let Some(uuid) = self.remote_player_uuid(m.id)
                && frame.horizon(uuid)
            {
                horizon.push((m.id, uuid));
            }
        }
        drop(frame);
        feed.ask(horizon.iter().map(|(_, u)| *u));
        let now = Instant::now();
        for (id, uuid) in horizon {
            let seen = feed.playing(uuid);
            let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
                continue;
            };
            let age = m.age as f32;
            let moved = {
                let (dx, dz) = (m.position.x - m.previous.x, m.position.z - m.previous.z);
                dx * dx + dz * dz > 1e-4
            };
            let mut next = emotes::playing(m).cloned();
            match (seen, &mut next) {
                (Some(seen), Some(p)) if Arc::ptr_eq(&p.item, &seen.item) => {
                    if moved && p.stop.is_none() {
                        p.stop = Some(age);
                    }
                }
                (Some(seen), slot) if !moved => {
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
            emotes::set_playing(m, next.filter(|p| p.at(age).is_some()));
        }
    }

    /// Asks the service which emotes the account owns, for the wheel: when it opens, at most
    /// every 10 minutes (a folder's emotes need no asking; an offline account has none).
    pub(super) fn refresh_owned_emotes(&mut self) {
        const AGAIN: Duration = Duration::from_secs(600);
        let e = &mut self.emotes;
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
        let kept = self.cosmetics.emote_out.kept.clone();
        std::thread::Builder::new()
            .name("Cosmetics emotes owned".into())
            .spawn(move || {
                let _ = tx.send(load_owned_emotes(&kept));
            })
            .ok();
    }

    /// Items from a folder (`HORIZON_COSMETICS_LOCAL`): mannequins are dressed and marked too.
    pub(super) fn cosmetics_local(&self) -> bool {
        self.cosmetics.local
    }

    /// What goes by `m`'s name: Horizon's mark if they play Horizon (and the mark is on), the
    /// name tag style they wear if cosmetics are shown for them (name tags only counts), and how
    /// high what is drawn on their body gets (`body_top`). One look in the cache for all three.
    pub(super) fn cosmetics_marks(&self, m: &Mob, own: bool) -> super::names::Marks {
        let mut marks = super::names::Marks::default();
        let extras = &self.settings.extras;
        let styles = if own {
            extras.cosmetics_mine
        } else {
            extras.effective_others() != Others::Hidden
        };
        if !(styles || extras.horizon_marks) {
            return marks;
        }
        let (Some(client), Some(uuid)) = (&self.cosmetics.client, self.cosmetics_uuid(m, own))
        else {
            return marks;
        };
        let mut frame = client.frame();
        marks.horizon = extras.horizon_marks && frame.horizon(uuid);
        let worn = if styles { frame.worn(uuid) } else { None };
        drop(frame);
        if !horizon_client::mobs::trader::is_invisible(m) {
            marks.top = body_top(extras, own, worn.as_ref(), |slot| covered(m, slot));
        }
        marks.style = worn.and_then(|w| self.cosmetics_tag(&w));
        marks
    }

    /// Those of `uuids` who play Horizon, with the mark on (the player list): one look each.
    pub(super) fn horizon_players(
        &self,
        uuids: impl Iterator<Item = u128>,
    ) -> std::collections::HashSet<u128> {
        match &self.cosmetics.client {
            Some(c) if self.settings.extras.horizon_marks => {
                let mut frame = c.frame();
                uuids.filter(|u| frame.horizon(*u)).collect()
            }
            _ => Default::default(),
        }
    }

    /// The name tag style in `worn`: its badge's place in the atlas and its line's colour. None
    /// until the badge is in the atlas.
    fn cosmetics_tag(&self, worn: &horizon_cosmetics::Worn) -> Option<super::names::TagStyle> {
        let item = worn.iter().find(|i| i.slot == Slot::NameTag)?;
        let [r, g, b] = item.tag?.line;
        let frame = self.cosmetics.frame.get();
        let mut baked = self.cosmetics.baked.borrow_mut();
        let entry = baked
            .entry(Arc::as_ptr(item) as usize)
            .or_insert_with(|| Baked::new(item, frame));
        let Some(slot) = self.gui.atlas.sprites.get(&entry.location) else {
            self.cosmetics.uploads.borrow_mut().push(item.clone());
            return None;
        };
        if entry.marked.replace(frame) != frame {
            self.skin_downloads.borrow_mut().drawn(&entry.location);
        }
        // The mark sits in the slot's top left corner.
        let mut mark = *slot;
        mark.u1 = mark.u0 + (slot.u1 - slot.u0) * item.texture.width as f32 / 64.0;
        mark.v1 = mark.v0 + (slot.v1 - slot.v0) * item.texture.height as f32 / 64.0;
        Some(super::names::TagStyle {
            mark,
            line: u32::from_be_bytes([0, r, g, b]),
        })
    }

    /// What `m` wears, if cosmetics are shown for it: `own` for the player's own model.
    fn worn(&self, m: &Mob, own: bool) -> Option<horizon_cosmetics::Worn> {
        let client = self.cosmetics.client.as_ref()?;
        worn_of(client, &self.settings.extras, own, || {
            self.cosmetics_uuid(m, own)
        })
    }

    /// The UUID the service knows `m` by: a player's, or a mannequin's id with local items.
    fn cosmetics_uuid(&self, m: &Mob, own: bool) -> Option<u128> {
        if own {
            own_uuid(
                self.remote.as_ref().map(|r| r.profile()),
                self.integrated.is_some(),
                self.settings.extras.cosmetics_solo,
                || horizon_client::auth::current().map(|s| s.uuid),
            )
        } else if &*m.kind.id == "minecraft:player" {
            self.remote_player_uuid(m.id)
        } else if self.cosmetics.local && &*m.kind.id == "minecraft:mannequin" {
            Some(u128::from(m.id))
        } else {
            None
        }
    }

    /// The cosmetics `m` wears, on its bones. `gpu`: posed on the GPU (the mob pass's skin
    /// frame, begun); the bounds of what went there come back, for culling. Else the vertices
    /// go to `out`. `camera`: where the world view's camera is; None in a picture of the player
    /// (the inventory), which shows no companion, trail or aura.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn cosmetics_geometry(
        &self,
        m: &Mob,
        own: bool,
        base: glam::Mat4,
        camera: Option<glam::DVec3>,
        alpha: f32,
        light: u32,
        gpu: bool,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) -> Option<(glam::Vec3, glam::Vec3)> {
        use horizon_client::mobs::{anim, render};
        let worn = self.worn(m, own);
        // An emote's props: its time and weight while it plays.
        let ticks = m.age as f32 + alpha;
        let emote = emotes::playing(m).and_then(|p| {
            let (t, w) = p.at(ticks)?;
            (p.item.vertices > 0).then_some((&p.item, t, w))
        });
        if worn.is_none() && emote.is_none() || horizon_client::mobs::trader::is_invisible(m) {
            return None;
        }
        // The avatar's body, posed as it is drawn.
        let (key, _) = render::body(m);
        let body = m.kind.layer(&key).unwrap_or(&m.kind.model);
        let mut scratch = self.cosmetics.poses.borrow_mut();
        let (body_poses, poses) = &mut *scratch;
        body_poses.reset(body);
        let data = self.res.mob_data();
        anim::setup(m, &anim::state(m, alpha), body_poses, &data.animations);
        let mut bounds = None;
        // Items on one bone (a back item, a shoulder pet and a shirt ride the body) share its
        // matrix: the head, the body and the two arms at most.
        let mut bones: [Option<(horizon_cosmetics::Bone, glam::Mat4)>; 4] = [None; 4];
        // "Name tags only": other players' styles beside their names, nothing on their bodies.
        let bodies = own || self.settings.extras.effective_others() == Others::Shown;
        let worn_items = worn.iter().flat_map(|w| w.iter()).map(|i| (i, None));
        let emote_items = emote.map(|(i, t, w)| (i, Some((t, w))));
        for (item, emoting) in worn_items.chain(emote_items) {
            if emoting.is_none() && !bodies {
                continue;
            }
            if let (Some(camera), Slot::Companion | Slot::Trail | Slot::Aura) = (camera, item.slot)
            {
                // The player's own in first person: only its companion, drawn after the drain
                // of the first-person model (`cosmetics_fx_geometry`).
                if !(own && !self.own_body_in_view()) {
                    let at = fx::Wearer {
                        m,
                        own,
                        key: if own { OWN } else { m.id },
                        base,
                        camera,
                        alpha,
                        light,
                    };
                    bounds = self.cosmetics_fx(&at, item, gpu, bounds, poses, out);
                }
                continue;
            }
            if emoting.is_none() && covered(m, item.slot) {
                continue;
            }
            let Some((models, uv)) = self.cosmetics_baked(item) else {
                continue;
            };
            for (bone_of, model) in &models {
                let known = bones.iter().flatten().find(|(b, _)| b == bone_of);
                let bone = match known {
                    Some(&(_, matrix)) => matrix,
                    None => {
                        let Some(matrix) =
                            render::part_matrix(body, base, body_poses, bone_of.part(), 0.0)
                        else {
                            continue;
                        };
                        if let Some(free) = bones.iter_mut().find(|b| b.is_none()) {
                            *free = Some((*bone_of, matrix));
                        }
                        matrix
                    }
                };
                poses.reset(model);
                item.animate(ticks, |name, pose| *poses.get(name) = pose);
                if let (Some((t, w)), Some(e)) = (emoting, &item.emote) {
                    e.pose_props(t, |n| item.rest(n), |n, pose| *poses.get(n) = pose);
                    // Props grow in and shrink out with the emote's blend.
                    let root = poses.get(&model.name);
                    for s in &mut root[6..9] {
                        *s *= w;
                    }
                }
                bounds = self.cosmetics_model(model, bone, poses, uv, light, gpu, bounds, out);
            }
        }
        bounds
    }

    /// An item's models and its texture's corners in the atlas, once its texture is there (it is
    /// asked for meanwhile).
    #[allow(clippy::type_complexity)]
    fn cosmetics_baked(
        &self,
        item: &Arc<Item>,
    ) -> Option<(Vec<(horizon_cosmetics::Bone, Arc<Part>)>, [f32; 4])> {
        let frame = self.cosmetics.frame.get();
        let mut baked = self.cosmetics.baked.borrow_mut();
        let b = baked
            .entry(Arc::as_ptr(item) as usize)
            .or_insert_with(|| Baked::new(item, frame));
        let Some(rect) = self.gui.atlas.sprites.get(&b.location) else {
            self.cosmetics.uploads.borrow_mut().push(item.clone());
            return None;
        };
        if b.marked.replace(frame) != frame {
            self.skin_downloads.borrow_mut().drawn(&b.location);
        }
        // The texture fills the top-left of its 64×64 slot.
        let (w, h) = (
            item.texture.width as f32 / 64.0,
            item.texture.height as f32 / 64.0,
        );
        let uv = [
            rect.u0,
            rect.v0,
            rect.u0 + (rect.u1 - rect.u0) * w,
            rect.v0 + (rect.v1 - rect.v0) * h,
        ];
        Some((b.models.clone(), uv))
    }

    /// One model at `matrix`, posed by `poses`: on the GPU's skin path when `gpu` (the bounds
    /// grow to hold it), else its vertices go to `out`.
    #[allow(clippy::too_many_arguments)]
    fn cosmetics_model(
        &self,
        model: &Arc<Part>,
        matrix: glam::Mat4,
        poses: &horizon_client::mobs::anim::Poses,
        uv: [f32; 4],
        light: u32,
        gpu: bool,
        bounds: Option<(glam::Vec3, glam::Vec3)>,
        out: &mut Vec<Vertex>,
    ) -> Option<(glam::Vec3, glam::Vec3)> {
        if gpu {
            let mut models = self.skin_models.borrow_mut();
            let mut frame = self.skin_frame.borrow_mut();
            let mut skin = horizon_client::mobs::skin::Skin::new(&mut models, &mut frame);
            skin.bounds = bounds;
            let paint = horizon_client::mobs::skin::Paint {
                uv,
                scroll: 0.0,
                color: 0xFFFF_FFFF,
                light,
                overlay: 0,
            };
            if skin.draw(model, matrix, poses, paint) {
                return skin.bounds;
            }
        }
        horizon_client::mobs::render_model(
            model,
            matrix,
            &|p: &Part| poses.pose(p),
            uv,
            light,
            0,
            out,
        );
        bounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::extras::Extras;
    use std::sync::Mutex;

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
            let worn = worn_of(&client, extras, false, || Some(OTHER));
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
                let worn = worn_of(&client, &extras, true, uuid);
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
            body_top(extras, false, worn.as_ref(), |_| false)
        };
        // No cosmetics drawn on the body (a name tag style only): vanilla's height exactly.
        let (worn, _) = other_player(&shown, long);
        assert!(worn.is_some());
        assert_eq!(
            tag(body_top(&shown, false, worn.as_ref(), |_| false)),
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
        let helmet = body_top(&shown, false, worn.as_ref(), |slot| slot == Slot::Head);
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
}
