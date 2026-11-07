//! Horizon Extras' dynamic lights (`horizon_client::dynamic_light`): the light sources near the
//! camera this frame. Nothing is gathered while the extra is off.
use super::{Game, shield};
use glam::DVec3;
use horizon_client::dynamic_light::{self, BURNING, Luminance, Source};

/// The brighter hand (the second wins ties), or fire if it outshines both. Item names
/// borrow the entity's stack; even sources outside the selected MAX need no allocation.
fn brighter<'a>(
    lum: &Luminance,
    items: [Option<&'a str>; 2],
    on_fire: bool,
) -> (u8, Option<&'a str>) {
    let (level, item) = items
        .into_iter()
        .flatten()
        .map(|i| (lum.of(i), Some(i)))
        .max_by_key(|(level, _)| *level)
        .unwrap_or((0, None));
    if on_fire && BURNING > level {
        (BURNING, None)
    } else {
        (level, item.filter(|_| level > 0))
    }
}

impl Game<'_> {
    /// How brightly each item glows, built on first use.
    pub(super) fn luminance(&self) -> &Luminance {
        self.luminance
            .get_or_init(|| Luminance::new(self.res.facts(), self.pack.assets.registry))
    }
    /// The player's, mobs' and dropped items' sources in their original order, with borrowed
    /// item names (none for fire). Nothing, including luminance, is visited while disabled.
    pub(super) fn dynamic_light_sources(
        &self,
        alpha: f64,
    ) -> impl Iterator<Item = (Source, Option<&str>)> {
        let lum = self
            .settings
            .extras
            .dynamic_lights
            .then(|| self.luminance());
        lum.into_iter().flat_map(move |lum| {
            let player = self.player.as_ref().filter(|p| !p.spectator).map(move |p| {
                let feet = self
                    .previous_position
                    .lerp(p.position, self.clock.camera_fraction());
                let (level, item) = brighter(
                    lum,
                    [
                        self.stack_in(self.selected).map(|s| &*s.item),
                        self.stack_in(shield::OFFHAND_SLOT).map(|s| &*s.item),
                    ],
                    p.fire_ticks > 0,
                );
                (
                    Source {
                        at: DVec3::new(feet.x, feet.y + 1.2, feet.z),
                        level,
                    },
                    item,
                )
            });
            let mobs = self
                .mobs
                .iter()
                .filter(|m| !m.removed)
                .filter_map(move |m| {
                    let (level, item) = brighter(
                        lum,
                        [m.extra.main_hand.as_deref(), m.extra.off_hand.as_deref()],
                        m.fire_ticks > 0,
                    );
                    if level == 0 {
                        return None;
                    }
                    let at = m.previous.lerp(m.position, alpha);
                    Some((
                        Source {
                            at: DVec3::new(at.x, at.y + f64::from(m.height()) * 0.6, at.z),
                            level,
                        },
                        item,
                    ))
                });
            let dropped = self.dropped.iter().filter_map(move |dropped| {
                let (level, item) = brighter(
                    lum,
                    [Some(&*dropped.stack.item), None],
                    dropped.fire_ticks > 0,
                );
                if level == 0 {
                    return None;
                }
                let at = dropped.previous.lerp(dropped.position, alpha);
                Some((
                    Source {
                        at: DVec3::new(at.x, at.y + 0.25, at.z),
                        level,
                    },
                    item,
                ))
            });
            player.into_iter().chain(mobs).chain(dropped)
        })
    }
    /// The nearest MAX sources, without building an intermediate list or owning item names.
    pub(super) fn gather_dynamic_lights(&mut self, camera: DVec3, alpha: f64) {
        let _span = horizon_client::spans::guard("pr_light_gather");
        let mut out = std::mem::take(&mut self.dynamic_lights);
        dynamic_light::select_iter(
            self.dynamic_light_sources(alpha).map(|(s, _)| s),
            camera,
            &mut out,
        );
        self.dynamic_lights = out;
    }
}
