//! The gameplay guide: what the buttons do right now, in the corners of the HUD while a pad plays
//! and of an inventory it drives (Horizon's; vanilla has no gamepad input). The game reads the facts
//! from its own hit results and capabilities (`game/pad/guide.rs`); this turns them into rows, and
//! places the rows around what was drawn.
use crate::gamepad::{Button, GlyphOf, GuideDetail, MenuJob, PadSettings, Rect};

/// What a use does, by what takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Use,
    Open,
    Ride,
    Trade,
    Feed,
    Place,
    Eat,
    Drink,
    Shoot,
    Load,
    Throw,
    Raise,
    Equip,
    Fish,
    Zoom,
    Boost,
}
impl Verb {
    fn caption(self) -> &'static str {
        match self {
            Verb::Use => "horizon.controller.guide.use",
            Verb::Open => "horizon.controller.guide.open",
            Verb::Ride => "horizon.controller.guide.ride",
            Verb::Trade => "horizon.controller.guide.trade",
            Verb::Feed => "horizon.controller.guide.feed",
            Verb::Place => "horizon.controller.guide.place",
            Verb::Eat => "horizon.controller.guide.eat",
            Verb::Drink => "horizon.controller.guide.drink",
            Verb::Shoot => "horizon.controller.guide.shoot",
            Verb::Load => "horizon.controller.guide.load",
            Verb::Throw => "horizon.controller.guide.throw",
            Verb::Raise => "horizon.controller.guide.raise",
            Verb::Equip => "horizon.controller.guide.equip",
            Verb::Fish => "horizon.controller.guide.fish",
            Verb::Zoom => "horizon.controller.guide.zoom",
            Verb::Boost => "horizon.controller.guide.boost",
        }
    }
}

/// `Minecraft.hitResult`, with what each button would do to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    None,
    Block {
        breakable: bool,
        /// The block takes the use before the held item gets its turn.
        takes: Option<Verb>,
    },
    Entity {
        attackable: bool,
        takes: Option<Verb>,
    },
}

/// An item in use, by what letting go of it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Active {
    Shoot,
    Throw,
    Stop,
}

/// What the guide knows of the player this moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Facts {
    pub spectator: bool,
    pub target: Target,
    /// What `Item.use` does on the target or in the air: the main hand's, else the off hand's.
    pub item_use: Option<Verb>,
    /// Pick Block finds something it can select or, in creative, add.
    pub can_pick: bool,
    pub using: Option<Active>,
    /// `LocalPlayer.isHandsBusy`: paddling a boat.
    pub hands_busy: bool,
    pub riding: bool,
    /// The vehicle jumps when told (a saddled horse).
    pub vehicle_jumps: bool,
    pub on_ground: bool,
    pub in_water: bool,
    pub flying: bool,
    pub may_fly: bool,
    pub gliding: bool,
    /// In the air with a glider on: a jump would start gliding.
    pub can_glide: bool,
    /// Sneaking that the button latched, so the next press stands up.
    pub sneak_latched: bool,
    pub can_sprint: bool,
    pub main_hand_item: bool,
    pub off_hand_item: bool,
}

/// One button's line: what the button does (its glyph follows the binding and the pad's letters)
/// and the translation key of the caption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// Shown before `of`: held with it, or its other half (LB and RB for pages).
    pub with: Option<GlyphOf>,
    pub of: GlyphOf,
    pub caption: &'static str,
}
impl Row {
    fn new(of: impl Into<GlyphOf>, caption: &'static str) -> Self {
        Row {
            with: None,
            of: of.into(),
            caption,
        }
    }
    fn pair(with: MenuJob, job: MenuJob, caption: &'static str) -> Self {
        Row {
            with: Some(GlyphOf::Menu(with)),
            of: GlyphOf::Menu(job),
            caption,
        }
    }
}

/// The two columns, most useful row first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guide {
    /// Top left: movement and the player's own actions.
    pub left: Vec<Row>,
    /// Top right: break, use and pick block.
    pub right: Vec<Row>,
}

impl Guide {
    /// The rows this detail keeps. Basic is the right column: what the buttons do to what's in
    /// front of the player, or the plain face buttons in an inventory.
    pub fn at_detail(self, detail: GuideDetail) -> Self {
        match detail {
            GuideDetail::Full => self,
            // Nothing else in play names the radial menu.
            GuideDetail::Basic => Guide {
                left: self
                    .left
                    .into_iter()
                    .filter(|row| row.caption == crate::gamepad::RADIAL_CAPTION)
                    .collect(),
                right: self.right,
            },
            GuideDetail::Off => Guide {
                left: Vec::new(),
                right: Vec::new(),
            },
        }
    }
}

/// The rows for these facts on these bindings. An unbound action has no row, and neither has one
/// on Start, which is always pause.
pub fn resolve(facts: &Facts, settings: &PadSettings) -> Guide {
    let row = |action: &'static str, caption: Option<&'static str>| {
        settings
            .binding(action)
            .filter(|button| *button != Button::Start)?;
        Some(Row::new(GlyphOf::Bound(action), caption?))
    };
    let player = !facts.spectator;
    let any_item = facts.main_hand_item || facts.off_hand_item;
    Guide {
        left: [
            row("key.jump", jump_caption(facts)),
            row("key.sneak", sneak_caption(facts)),
            row("key.sprint", facts.can_sprint.then_some("key.sprint")),
            row("key.inventory", player.then_some("key.inventory")),
            row(
                "key.drop",
                (player && facts.main_hand_item).then_some("horizon.controller.guide.drop"),
            ),
            row(
                "key.swapOffhand",
                (player && any_item).then_some("horizon.controller.guide.swap"),
            ),
            // Nothing else in play says the menu is there; it opens for a spectator too.
            row(crate::gamepad::RADIAL, Some(crate::gamepad::RADIAL_CAPTION)),
        ]
        .into_iter()
        .flatten()
        .collect(),
        right: [
            row("key.attack", attack_caption(facts)),
            row("key.use", use_caption(facts)),
            row("key.pickItem", pick_caption(facts)),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }
}

fn jump_caption(f: &Facts) -> Option<&'static str> {
    if f.riding {
        f.vehicle_jumps.then_some("key.jump")
    } else if f.flying {
        Some("horizon.controller.guide.fly_up")
    } else if f.gliding {
        None
    } else if f.in_water {
        Some("horizon.controller.guide.swim_up")
    } else if f.on_ground {
        Some("key.jump")
    } else if f.can_glide {
        Some("horizon.controller.guide.glide")
    } else {
        f.may_fly.then_some("horizon.controller.guide.fly")
    }
}

fn sneak_caption(f: &Facts) -> Option<&'static str> {
    if f.riding {
        Some("horizon.controller.guide.dismount")
    } else if f.flying {
        Some("horizon.controller.guide.fly_down")
    } else if f.gliding {
        None
    } else if f.in_water && !f.on_ground {
        Some("horizon.controller.guide.swim_down")
    } else if f.sneak_latched {
        Some("horizon.controller.guide.stand")
    } else {
        Some("key.sneak")
    }
}

/// `Minecraft.handleKeybinds` spends attack clicks while an item is in use, and
/// `startAttack` does nothing with busy hands.
fn attack_caption(f: &Facts) -> Option<&'static str> {
    if f.using.is_some() || f.hands_busy {
        return None;
    }
    match f.target {
        Target::Entity {
            attackable: true, ..
        } if f.spectator => Some("horizon.controller.guide.spectate"),
        Target::Entity {
            attackable: true, ..
        } => Some("horizon.controller.guide.attack"),
        Target::Block {
            breakable: true, ..
        } => Some("horizon.controller.guide.break"),
        _ => None,
    }
}

/// `Minecraft.startUseItem`: what is aimed at first, then the item.
fn use_caption(f: &Facts) -> Option<&'static str> {
    match f.using {
        Some(Active::Shoot) => return Some("horizon.controller.guide.release_shoot"),
        Some(Active::Throw) => return Some("horizon.controller.guide.release_throw"),
        Some(Active::Stop) => return Some("horizon.controller.guide.release_stop"),
        None => {}
    }
    if f.hands_busy {
        return None;
    }
    let takes = match f.target {
        Target::Block { takes, .. } | Target::Entity { takes, .. } => takes,
        Target::None => None,
    };
    takes.or(f.item_use).map(Verb::caption)
}

fn pick_caption(f: &Facts) -> Option<&'static str> {
    if f.spectator {
        // key.spectatorHotbar is this button: it opens the teleport menu.
        return Some("horizon.controller.guide.spectator_menu");
    }
    (f.can_pick && f.using.is_none()).then_some("key.pickItem")
}

/// What the crosshair is on in an inventory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Under {
    #[default]
    Nothing,
    /// A slot. `same` is the carried stack being its kind; `result` a crafting result, which is
    /// only ever taken whole.
    Slot {
        item: bool,
        same: bool,
        result: bool,
        /// The carried stack can go here.
        accepts: bool,
    },
    Recipe {
        direct_craft: bool,
        alternatives: bool,
    },
    /// One of the recipe book's other controls.
    Control,
}

/// What an inventory's rows are resolved from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuFacts {
    pub under: Under,
    pub carrying: bool,
    /// The recipe book is showing: the bumpers turn its pages, the triggers its categories.
    pub book: bool,
    /// The bumpers switch tabs.
    pub tabs: bool,
}

/// An inventory's rows, by what they ask of the player: at the bottom right one face button, at the
/// bottom left anything else (a trigger held with a button, the bumpers, View, a stick click). What
/// the crosshair is on comes first in its column, the screen's own rows after, except Close, which
/// stays lowest so the rest don't move it. Menu buttons aren't rebindable, so these name menu jobs.
pub fn resolve_menu(f: &MenuFacts) -> Guide {
    use MenuJob::{Alternate, Back, Confirm, Extra, GroupNext, GroupPrev};
    use MenuJob::{LeftStick, PageNext, PagePrev, View};
    const TAKE: &str = "horizon.controller.guide.take";
    const MOVE: &str = "horizon.controller.guide.quick_move";
    let under = match f.under {
        // A result takes only into empty hands or the same stack.
        Under::Slot {
            item: true,
            result: true,
            same,
            ..
        } if !f.carrying || same => vec![Row::new(Confirm, TAKE), Row::new(Extra, MOVE)],
        Under::Slot { result: true, .. } => Vec::new(),
        Under::Slot { accepts: false, .. } if f.carrying => Vec::new(),
        Under::Slot { item, same, .. } if f.carrying => vec![
            Row::new(
                Confirm,
                if item && !same {
                    "horizon.controller.guide.swap_stack"
                } else {
                    "horizon.controller.guide.put"
                },
            ),
            Row::new(Alternate, "horizon.controller.guide.put_one"),
        ],
        Under::Slot { item: true, .. } => vec![
            Row::new(Confirm, TAKE),
            Row::new(Alternate, "horizon.controller.guide.take_half"),
            Row::new(Extra, MOVE),
            Row::new(View, "horizon.controller.guide.drop"),
            Row::new(LeftStick, "horizon.controller.guide.drop_stack"),
        ],
        Under::Nothing | Under::Slot { .. } => Vec::new(),
        Under::Recipe {
            direct_craft,
            alternatives,
        } => [
            Some(Row::new(Confirm, "horizon.controller.guide.place_recipe")),
            direct_craft.then_some(Row::pair(
                GroupPrev,
                Confirm,
                "horizon.controller.direct_craft",
            )),
            Some(Row::new(Extra, "horizon.controller.guide.place_recipe_all")),
            alternatives.then_some(Row::new(Alternate, "horizon.controller.guide.alternatives")),
        ]
        .into_iter()
        .flatten()
        .collect(),
        Under::Control => vec![Row::new(Confirm, "horizon.controller.guide.select")],
    };
    let screen = [
        f.book.then_some(Row::pair(
            PagePrev,
            PageNext,
            "horizon.controller.guide.pages",
        )),
        f.book.then_some(Row::pair(
            GroupPrev,
            GroupNext,
            "horizon.controller.guide.categories",
        )),
        (f.tabs && !f.book).then_some(Row::pair(
            PagePrev,
            PageNext,
            "horizon.controller.guide.tabs",
        )),
    ];
    let close = Row::new(Back, "horizon.controller.guide.close");
    let rows = std::iter::once(close)
        .chain(under)
        .chain(screen.into_iter().flatten());
    let (right, left) = rows.partition(|r| {
        r.with.is_none() && matches!(r.of, GlyphOf::Menu(Confirm | Back | Alternate | Extra))
    });
    Guide { left, right }
}

/// A row's height and the view's margin, in GUI pixels.
pub const ROW_HEIGHT: f32 = 11.0;
const MARGIN: f32 = 2.0;
/// GUI px kept between the two columns where a row had to be cut to fit beside the other.
pub const COLUMN_GAP: f32 = 4.0;

/// The most a row may be on a view `width` GUI px wide: the view less its margins.
pub fn row_room(width: f32) -> f32 {
    width - 2.0 * MARGIN
}

/// How wide a left-column row may be cut to beside right-column rows up to `right` GUI px wide
/// (0 with none): what they leave, less the gap between the columns.
pub fn left_room(width: f32, right: f32) -> f32 {
    row_room(width) - COLUMN_GAP - right
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// Where a column's rows go, down from a top corner of the `[width, height]` view. A row goes as
/// wide as `widths` says where it overlaps nothing in `taken`, else cut to `room` (its caption
/// gets shorter) where that overlaps nothing, else under whatever it would overlap; nowhere
/// when that leaves the view's upper three quarters (the row is dropped: the rows come in the
/// order they matter). A row wider than the view is always cut to it. The placed rows join
/// `taken`; each rect's width is the row's, cut or not.
pub fn place_column(
    view: [f32; 2],
    left: bool,
    widths: &[f32],
    room: f32,
    taken: &mut Vec<Rect>,
) -> Vec<Option<Rect>> {
    place(view, left, false, widths, room, taken)
}

/// `place_column`'s rows up from a bottom corner, over what they would overlap, within the view's
/// lower quarter: the first row is the lowest.
pub fn place_column_up(
    view: [f32; 2],
    left: bool,
    widths: &[f32],
    room: f32,
    taken: &mut Vec<Rect>,
) -> Vec<Option<Rect>> {
    place(view, left, true, widths, room, taken)
}

fn place(
    view: [f32; 2],
    left: bool,
    up: bool,
    widths: &[f32],
    room: f32,
    taken: &mut Vec<Rect>,
) -> Vec<Option<Rect>> {
    let mut y = if up {
        view[1] - MARGIN - ROW_HEIGHT
    } else {
        MARGIN
    };
    let most = row_room(view[0]);
    widths
        .iter()
        .map(|&width| {
            // As it is (no wider than the view), then cut to `room`: the narrowest last, to go
            // past what even that overlaps.
            let whole = width.min(most);
            let cut = whole.min(room).max(0.0);
            let tries = if cut < whole {
                vec![whole, cut]
            } else {
                vec![whole]
            };
            let rect = |width: f32, at: f32| {
                let x = if left {
                    MARGIN
                } else {
                    view[0] - MARGIN - width
                };
                [x, at, width, ROW_HEIGHT]
            };
            let mut at = y;
            loop {
                let outside = if up {
                    at < view[1] * 0.75
                } else {
                    at + ROW_HEIGHT > view[1] * 0.75
                };
                if outside {
                    return None;
                }
                let free = tries
                    .iter()
                    .map(|&w| rect(w, at))
                    .find(|r| !taken.iter().any(|t| overlap(r, t)));
                if let Some(placed) = free {
                    taken.push(placed);
                    y = if up {
                        at - ROW_HEIGHT - 1.0
                    } else {
                        at + ROW_HEIGHT + 1.0
                    };
                    return Some(placed);
                }
                let narrowest = rect(cut, at);
                let hit = taken.iter().filter(|t| overlap(&narrowest, t));
                let past = if up {
                    hit.map(|t| t[1]).fold(f32::INFINITY, f32::min)
                } else {
                    hit.map(|t| t[1] + t[3]).fold(f32::NEG_INFINITY, f32::max)
                };
                at = if up {
                    past.floor() - ROW_HEIGHT - 1.0
                } else {
                    past.ceil() + 1.0
                };
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walking() -> Facts {
        Facts {
            on_ground: true,
            can_sprint: true,
            main_hand_item: true,
            ..Facts::default()
        }
    }
    fn on_block(f: Facts, breakable: bool, takes: Option<Verb>) -> Facts {
        Facts {
            target: Target::Block { breakable, takes },
            ..f
        }
    }
    /// Each row's button on these settings, and its caption.
    fn rows(rows: &[Row], settings: &PadSettings) -> Vec<(Button, &'static str)> {
        rows.iter()
            .map(|r| (r.of.button(settings).unwrap(), r.caption))
            .collect()
    }
    fn captions(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.caption).collect()
    }

    #[test]
    fn survival_on_a_block_shows_break_use_and_pick_on_the_default_buttons() {
        let facts = Facts {
            can_pick: true,
            item_use: Some(Verb::Place),
            ..on_block(walking(), true, None)
        };
        let settings = PadSettings::default();
        let guide = resolve(&facts, &settings);
        assert_eq!(
            rows(&guide.right, &settings),
            [
                (Button::RightTrigger, "horizon.controller.guide.break"),
                (Button::LeftTrigger, "horizon.controller.guide.place"),
                (Button::DPadLeft, "key.pickItem"),
            ]
        );
        assert_eq!(
            rows(&guide.left, &settings),
            [
                (Button::South, "key.jump"),
                (Button::RightStick, "key.sneak"),
                (Button::LeftStick, "key.sprint"),
                (Button::North, "key.inventory"),
                (Button::DPadDown, "horizon.controller.guide.drop"),
                (Button::West, "horizon.controller.guide.swap"),
                (Button::DPadRight, crate::gamepad::RADIAL_CAPTION),
            ]
        );
    }

    #[test]
    fn a_rebound_action_moves_its_row_and_an_unbound_one_loses_it() {
        let mut settings = PadSettings::default();
        settings.bind("key.attack", Some(Button::North));
        settings.bind("key.pickItem", None);
        settings.bind("key.jump", Some(Button::Start));
        let facts = Facts {
            can_pick: true,
            ..on_block(walking(), true, None)
        };
        let guide = resolve(&facts, &settings);
        assert_eq!(
            rows(&guide.right, &settings),
            [(Button::North, "horizon.controller.guide.break")]
        );
        // Start only ever pauses.
        assert!(!captions(&guide.left).contains(&"key.jump"));
    }

    #[test]
    fn what_a_block_or_entity_takes_comes_before_the_held_item() {
        let settings = PadSettings::default();
        let chest = Facts {
            item_use: Some(Verb::Place),
            ..on_block(walking(), true, Some(Verb::Open))
        };
        assert_eq!(
            captions(&resolve(&chest, &settings).right),
            [
                "horizon.controller.guide.break",
                "horizon.controller.guide.open"
            ]
        );
        let villager = Facts {
            item_use: Some(Verb::Eat),
            target: Target::Entity {
                attackable: true,
                takes: Some(Verb::Trade),
            },
            ..walking()
        };
        assert_eq!(
            captions(&resolve(&villager, &settings).right),
            [
                "horizon.controller.guide.attack",
                "horizon.controller.guide.trade"
            ]
        );
        // A mob the item means nothing to: the item is used on.
        let cow = Facts {
            target: Target::Entity {
                attackable: true,
                takes: None,
            },
            ..villager
        };
        assert_eq!(
            captions(&resolve(&cow, &settings).right)[1],
            "horizon.controller.guide.eat"
        );
    }

    #[test]
    fn nothing_to_do_leaves_the_right_column_empty() {
        let settings = PadSettings::default();
        assert!(resolve(&walking(), &settings).right.is_empty());
        // An unbreakable block too (bedrock, a creative sword, adventure without can_break).
        assert!(
            resolve(&on_block(walking(), false, None), &settings)
                .right
                .is_empty()
        );
    }

    #[test]
    fn an_item_in_use_leaves_only_letting_go() {
        let facts = Facts {
            using: Some(Active::Shoot),
            can_pick: true,
            ..on_block(walking(), true, Some(Verb::Open))
        };
        assert_eq!(
            captions(&resolve(&facts, &PadSettings::default()).right),
            ["horizon.controller.guide.release_shoot"]
        );
    }

    #[test]
    fn busy_hands_neither_attack_nor_use() {
        let facts = Facts {
            hands_busy: true,
            riding: true,
            item_use: Some(Verb::Eat),
            ..on_block(Facts::default(), true, None)
        };
        assert!(resolve(&facts, &PadSettings::default()).right.is_empty());
    }

    #[test]
    fn movement_follows_what_the_player_is_doing() {
        let settings = PadSettings::default();
        let first_two = |f: Facts| {
            let left = resolve(&f, &settings).left;
            captions(&left).into_iter().take(2).collect::<Vec<_>>()
        };
        assert_eq!(
            first_two(Facts {
                flying: true,
                may_fly: true,
                ..Facts::default()
            }),
            [
                "horizon.controller.guide.fly_up",
                "horizon.controller.guide.fly_down"
            ]
        );
        assert_eq!(
            first_two(Facts {
                in_water: true,
                ..Facts::default()
            }),
            [
                "horizon.controller.guide.swim_up",
                "horizon.controller.guide.swim_down"
            ]
        );
        // In the air, only a glider or creative flight makes a jump do anything.
        assert_eq!(first_two(Facts::default()), ["key.sneak", "key.inventory"]);
        assert_eq!(
            first_two(Facts {
                can_glide: true,
                ..Facts::default()
            })[0],
            "horizon.controller.guide.glide"
        );
        assert_eq!(
            first_two(Facts {
                sneak_latched: true,
                on_ground: true,
                ..Facts::default()
            })[1],
            "horizon.controller.guide.stand"
        );
        // A boat can't jump; a saddled horse can; both dismount.
        assert_eq!(
            first_two(Facts {
                riding: true,
                on_ground: true,
                ..Facts::default()
            })[0],
            "horizon.controller.guide.dismount"
        );
        assert_eq!(
            first_two(Facts {
                riding: true,
                vehicle_jumps: true,
                ..Facts::default()
            }),
            ["key.jump", "horizon.controller.guide.dismount"]
        );
        // Gliding: neither jump nor sneak does anything.
        assert_eq!(
            first_two(Facts {
                gliding: true,
                ..Facts::default()
            }),
            ["key.inventory", crate::gamepad::RADIAL_CAPTION]
        );
    }

    #[test]
    fn a_spectator_only_flies_opens_spectates_and_has_its_menus() {
        let facts = Facts {
            spectator: true,
            flying: true,
            may_fly: true,
            main_hand_item: true,
            target: Target::Entity {
                attackable: true,
                takes: None,
            },
            ..Facts::default()
        };
        let guide = resolve(&facts, &PadSettings::default());
        assert_eq!(
            captions(&guide.right),
            [
                "horizon.controller.guide.spectate",
                "horizon.controller.guide.spectator_menu"
            ]
        );
        assert_eq!(
            captions(&guide.left),
            [
                "horizon.controller.guide.fly_up",
                "horizon.controller.guide.fly_down",
                crate::gamepad::RADIAL_CAPTION
            ]
        );
    }

    #[test]
    fn rows_stack_from_their_corner() {
        let mut taken = Vec::new();
        let left = place_column(
            [200.0, 200.0],
            true,
            &[50.0, 30.0],
            f32::INFINITY,
            &mut taken,
        );
        assert_eq!(left[0], Some([2.0, 2.0, 50.0, 11.0]));
        assert_eq!(left[1], Some([2.0, 14.0, 30.0, 11.0]));
        let right = place_column([200.0, 200.0], false, &[40.0], f32::INFINITY, &mut taken);
        assert_eq!(right[0], Some([158.0, 2.0, 40.0, 11.0]));
    }

    #[test]
    fn a_row_goes_under_what_it_would_overlap() {
        // The effect icons in the top right corner.
        let mut taken = vec![[150.0, 1.0, 50.0, 24.0]];
        let right = place_column(
            [200.0, 200.0],
            false,
            &[40.0, 40.0],
            f32::INFINITY,
            &mut taken,
        );
        assert_eq!(right[0], Some([158.0, 26.0, 40.0, 11.0]));
        assert_eq!(right[1], Some([158.0, 38.0, 40.0, 11.0]));
    }

    #[test]
    fn the_columns_of_a_narrow_view_never_overlap_and_late_rows_drop() {
        // One quarter of a small window, a boss bar across its top: the rows have 31..67 left.
        let view = [160.0, 90.0];
        let bar = [0.0, 12.0, 160.0, 19.0];
        let mut taken = vec![bar];
        let right = place_column(view, false, &[90.0, 90.0, 90.0], f32::INFINITY, &mut taken);
        let left = place_column(view, true, &[90.0; 6], f32::INFINITY, &mut taken);
        let placed: Vec<Rect> = right.iter().chain(&left).flatten().copied().collect();
        for (i, a) in placed.iter().enumerate() {
            assert!(a[1] + a[3] <= 67.5 && a[0] >= 0.0 && a[0] + a[2] <= 160.0);
            assert!(!overlap(a, &bar));
            for b in &placed[i + 1..] {
                assert!(!overlap(a, b), "{a:?} overlaps {b:?}");
            }
        }
        // Three rows' worth of room, and the right column came first.
        assert_eq!(right.iter().flatten().count(), 3);
        assert_eq!(left.iter().flatten().count(), 0);
    }

    #[test]
    fn a_left_row_that_would_overlap_the_right_column_is_cut_beside_it() {
        // The narrow view above, without the bar: rows 90 wide don't fit two abreast.
        let view = [160.0, 90.0];
        let mut taken = Vec::new();
        let right = place_column(view, false, &[90.0; 3], row_room(view[0]), &mut taken);
        let room = left_room(view[0], 90.0);
        assert_eq!(room, 62.0);
        // The last left row is short: it keeps its width.
        let left = place_column(view, true, &[90.0, 90.0, 40.0], room, &mut taken);
        // Every row in its corner, in order, none pushed under the other column.
        for (k, (r, l)) in right.iter().zip(&left).enumerate() {
            let y = 2.0 + 12.0 * k as f32;
            assert_eq!(*r, Some([68.0, y, 90.0, 11.0]));
            assert_eq!(l.map(|l| l[1]), Some(y));
        }
        assert_eq!(left[0].unwrap()[2], 62.0);
        assert_eq!(left[2].unwrap()[2], 40.0);
    }

    #[test]
    fn a_row_keeps_its_width_where_it_overlaps_nothing() {
        // A long left row below a short right column: nothing beside it, so nothing to cut.
        let view = [320.0, 240.0];
        let mut taken = Vec::new();
        place_column(view, false, &[100.0], row_room(view[0]), &mut taken);
        let room = left_room(view[0], 100.0);
        let left = place_column(view, true, &[50.0, 250.0], room, &mut taken);
        assert_eq!(left[1], Some([2.0, 14.0, 250.0, 11.0]));
    }

    #[test]
    fn rows_stack_up_from_a_bottom_corner_and_drop_above_its_quarter() {
        let view = [200.0, 100.0];
        let mut taken = vec![[0.0, 80.0, 60.0, 5.0]];
        let left = place_column_up(view, true, &[50.0, 50.0], f32::INFINITY, &mut taken);
        assert_eq!(left[0], Some([2.0, 87.0, 50.0, 11.0]));
        // Over the panel at 80 would be 68, under the lower quarter's 75.
        assert_eq!(left[1], None);
        let right = place_column_up(view, false, &[40.0], f32::INFINITY, &mut taken);
        assert_eq!(right[0], Some([158.0, 87.0, 40.0, 11.0]));
    }

    fn short(rows: &[Row]) -> Vec<&str> {
        rows.iter()
            .map(|r| r.caption.rsplit('.').next().unwrap_or(""))
            .collect()
    }

    #[test]
    fn an_inventorys_face_buttons_go_right_and_the_rest_left() {
        let on = |under, carrying| {
            resolve_menu(&MenuFacts {
                under,
                carrying,
                ..MenuFacts::default()
            })
        };
        let slot = |item, same, result| Under::Slot {
            item,
            same,
            result,
            accepts: true,
        };
        let close_only = ["close"];
        let empty: [&str; 0] = [];
        for quiet in [Under::Nothing, slot(false, false, false)] {
            let guide = on(quiet, false);
            assert_eq!(short(&guide.right), close_only);
            assert_eq!(short(&guide.left), empty);
        }
        let item = on(slot(true, false, false), false);
        assert_eq!(
            short(&item.right),
            ["close", "take", "take_half", "quick_move"]
        );
        assert_eq!(short(&item.left), ["drop", "drop_stack"]);
        assert_eq!(
            short(&on(slot(false, false, false), true).right),
            ["close", "put", "put_one"]
        );
        assert_eq!(
            short(&on(slot(true, false, false), true).right),
            ["close", "swap_stack", "put_one"]
        );
        assert_eq!(
            short(&on(slot(true, true, false), true).right),
            ["close", "put", "put_one"]
        );
        assert_eq!(
            short(&on(slot(true, false, true), false).right),
            ["close", "take", "quick_move"]
        );
        let recipe = on(
            Under::Recipe {
                direct_craft: true,
                alternatives: true,
            },
            false,
        );
        assert_eq!(
            short(&recipe.right),
            ["close", "place_recipe", "place_recipe_all", "alternatives"]
        );
        assert_eq!(recipe.left.len(), 1);
        assert_eq!(recipe.left[0].with, Some(GlyphOf::Menu(MenuJob::GroupPrev)));
        assert_eq!(recipe.left[0].of, GlyphOf::Menu(MenuJob::Confirm));
        let plain = on(
            Under::Recipe {
                direct_craft: false,
                alternatives: false,
            },
            false,
        );
        assert_eq!(
            short(&plain.right),
            ["close", "place_recipe", "place_recipe_all"]
        );
        assert!(plain.left.is_empty());
    }

    #[test]
    fn an_inventorys_screen_rows_follow_its_book_and_tabs() {
        let left = |book, tabs| {
            resolve_menu(&MenuFacts {
                book,
                tabs,
                ..MenuFacts::default()
            })
            .left
        };
        assert!(left(false, false).is_empty());
        assert_eq!(short(&left(true, false)), ["pages", "categories"]);
        assert_eq!(short(&left(false, true)), ["tabs"]);
    }

    #[test]
    fn a_row_wider_than_the_view_is_cut_to_it() {
        let mut taken = Vec::new();
        assert_eq!(
            place_column(
                [50.0, 200.0],
                true,
                &[60.0, 20.0],
                f32::INFINITY,
                &mut taken
            ),
            [Some([2.0, 2.0, 46.0, 11.0]), Some([2.0, 14.0, 20.0, 11.0])]
        );
        // To the view, not to a smaller room, with nothing beside it.
        let mut taken = Vec::new();
        assert_eq!(
            place_column([50.0, 200.0], true, &[60.0], 30.0, &mut taken),
            [Some([2.0, 2.0, 46.0, 11.0])]
        );
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;

    fn rows() -> Guide {
        Guide {
            left: vec![Row::new(MenuJob::Confirm, "left")],
            right: vec![Row::new(MenuJob::Back, "right")],
        }
    }

    #[test]
    fn full_keeps_both_columns() {
        let guide = rows().at_detail(GuideDetail::Full);
        assert_eq!((guide.left.len(), guide.right.len()), (1, 1));
    }

    #[test]
    fn basic_keeps_the_right_column() {
        let guide = rows().at_detail(GuideDetail::Basic);
        assert_eq!((guide.left.len(), guide.right.len()), (0, 1));
    }

    #[test]
    fn off_keeps_nothing() {
        let guide = rows().at_detail(GuideDetail::Off);
        assert_eq!((guide.left.len(), guide.right.len()), (0, 0));
    }

    #[test]
    fn a_file_saved_with_the_old_switch_keeps_its_meaning() {
        assert_eq!(GuideDetail::parse("true"), GuideDetail::Full);
        assert_eq!(GuideDetail::parse("false"), GuideDetail::Off);
        assert_eq!(GuideDetail::parse("basic"), GuideDetail::Basic);
    }
}

#[cfg(test)]
mod slot_tests {
    use super::*;

    /// The rows for the slot: Confirm or Alternate, whichever column they're in.
    fn rows(item: bool, same: bool, result: bool, accepts: bool, carrying: bool) -> usize {
        let guide = resolve_menu(&MenuFacts {
            under: Under::Slot {
                item,
                same,
                result,
                accepts,
            },
            carrying,
            ..MenuFacts::default()
        });
        [guide.left, guide.right]
            .iter()
            .flatten()
            .filter(|row| matches!(row.of, GlyphOf::Menu(MenuJob::Confirm | MenuJob::Alternate)))
            .count()
    }

    #[test]
    fn a_slot_that_refuses_the_carried_stack_offers_nothing() {
        assert_eq!(rows(false, false, false, false, true), 0);
        assert_eq!(rows(true, true, false, false, true), 0);
        assert!(rows(false, false, false, true, true) > 0);
    }

    #[test]
    fn a_result_is_taken_only_into_empty_hands_or_the_same_stack() {
        assert!(rows(true, false, true, true, false) > 0);
        assert!(rows(true, true, true, true, true) > 0);
        assert_eq!(rows(true, false, true, true, true), 0);
    }
}
