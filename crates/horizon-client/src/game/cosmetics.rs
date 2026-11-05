//! Cosmetics on players ([docs/cosmetics.md](../../../../docs/cosmetics.md)): only Horizon
//! clients draw them, off unless `horizon_cosmetics::enabled()`. What another player wears comes
//! from the cosmetics service by UUID (`horizon_cosmetics::Cosmetics`, its own thread); this file
//! puts the items on the avatar's bones, on the GPU skin path when the renderer takes it.
//!
//! Other players' cosmetics show by default only to an account that owns one (asked of the
//! service once a run, `OwnerCheck`): anyone else sees a vanilla game and the game asks nothing
//! about other players, until the player chooses otherwise in the settings.

use super::Game;
use horizon_client::fast_hash::FxHashMap;
use horizon_client::mobs::anim::Poses;
use horizon_client::mobs::{Mob, Part, Polygon};
use horizon_cosmetics::prefs::{self, Others};
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
        Slot::NameTag => true,
    }
}

impl Game<'_> {
    /// Once a frame, before drawing: starts the service's client on first use and puts the
    /// textures the last frame wanted into the atlas.
    pub(super) fn cosmetics_frame(&mut self) {
        if !horizon_cosmetics::enabled() {
            return;
        }
        let c = &mut self.cosmetics;
        if c.client.is_none() {
            let (client, local) = Cosmetics::client();
            c.client = Some(client);
            c.local = local;
        }
        c.frame.set(c.frame.get().wrapping_add(1));
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
            if busy {
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

    /// Items from a folder (`HORIZON_COSMETICS_LOCAL`): mannequins are dressed and marked too.
    pub(super) fn cosmetics_local(&self) -> bool {
        self.cosmetics.local
    }

    /// What goes by `m`'s name: Horizon's mark if they play Horizon (and the mark is on), and the
    /// name tag style they wear if cosmetics are shown for them (name tags only counts). One
    /// look in the cache for both.
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
    /// go to `out`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn cosmetics_geometry(
        &self,
        m: &Mob,
        own: bool,
        base: glam::Mat4,
        alpha: f32,
        light: u32,
        gpu: bool,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) -> Option<(glam::Vec3, glam::Vec3)> {
        use horizon_client::mobs::{anim, render};
        let worn = self.worn(m, own)?;
        if horizon_client::mobs::trader::is_invisible(m) {
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
        let ticks = m.age as f32 + alpha;
        let frame = self.cosmetics.frame.get();
        let mut bounds = None;
        // Items on one bone (a back item, a shoulder pet and a shirt ride the body) share its
        // matrix: the head, the body and the two arms at most.
        let mut bones: [Option<(horizon_cosmetics::Bone, glam::Mat4)>; 4] = [None; 4];
        // "Name tags only": other players' styles beside their names, nothing on their bodies.
        let bodies = own || self.settings.extras.effective_others() == Others::Shown;
        for item in worn.iter() {
            if !bodies || covered(m, item.slot) {
                continue;
            }
            let mut baked = self.cosmetics.baked.borrow_mut();
            let b = baked
                .entry(Arc::as_ptr(item) as usize)
                .or_insert_with(|| Baked::new(item, frame));
            let Some(rect) = self.gui.atlas.sprites.get(&b.location) else {
                self.cosmetics.uploads.borrow_mut().push(item.clone());
                continue;
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
            for (bone_of, model) in &b.models {
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
                    if skin.draw(model, bone, poses, paint) {
                        bounds = skin.bounds;
                        continue;
                    }
                }
                horizon_client::mobs::render_model(
                    model,
                    bone,
                    &|p: &Part| poses.pose(p),
                    uv,
                    light,
                    0,
                    out,
                );
            }
        }
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

    /// The service: every player asked about wears `TAG`; each request is logged.
    #[derive(Clone, Default)]
    struct Service(Arc<Mutex<Vec<String>>>);

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
                    (p, serde_json::json!([{"id": "tag", "hash": "h"}]))
                })
                .collect();
            Ok(serde_json::json!({"ttl": 60, "players": players})
                .to_string()
                .into_bytes())
        }

        fn get(&self, path: &str) -> Result<Vec<u8>, String> {
            self.0.lock().unwrap().push(format!("GET {path}"));
            Ok(TAG.as_bytes().to_vec())
        }
    }

    const OTHER: u128 = 0x1234;

    /// What another player wears as `extras` shows it, drawn frame after frame for up to
    /// `patience` (the service answers on its own thread), and what was asked of the service.
    fn other_player(
        extras: &Extras,
        patience: Duration,
    ) -> (Option<horizon_cosmetics::Worn>, Vec<String>) {
        let service = Service::default();
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
}
