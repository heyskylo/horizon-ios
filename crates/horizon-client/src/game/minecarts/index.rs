//! The carts' and mobs' boxes for a tick's cart pushes ([`CartIndex`]).
use super::*;

/// The carts' and mobs' boxes for one tick's cart pushes (`push_from_cart`,
/// `pickup_from_cart`), so a cart reads its neighbours instead of every cart and
/// every mob of the level. Both answer in slot order, as the scans did.
///
/// The cart grid follows each ticked cart ([`CartIndex::moved`]); it is
/// rebuilt once the list's length changed (a cart gone or added), and dropped
/// after a command, which can move or replace any cart
/// ([`CartIndex::forget_carts`]). The mob grid holds until the mob
/// list's next write ([`ListStamp`]). A native mod's hook can move either kind
/// unseen, so with mods loaded both scans stay.
#[derive(Default)]
pub(in crate::game) struct CartIndex {
    pub(super) carts: Option<(SeenGrid, Vec<u32>)>,
    /// The grid, and the carts a mob rides.
    pub(super) mobs: Option<(ListStamp, SeenGrid, FxHashSet<u32>)>,
    pub(super) near: Vec<u32>,
}
impl CartIndex {
    pub(super) fn usable() -> bool {
        horizon_client::mod_api::mods().is_empty()
    }
    /// The cart in slot `i` has these bounds now.
    pub(super) fn moved(&mut self, i: usize, bounds: Aabb) {
        if let Some((grid, ids)) = &mut self.carts
            && i < ids.len()
        {
            grid.update(i, bounds);
        }
    }
    pub(super) fn forget_carts(&mut self) {
        self.carts = None;
    }
    /// The slots of the carts whose box may meet `area`, for the cart in slot
    /// `i` (just moved); false: scan them all.
    pub(super) fn carts_near(
        &mut self,
        carts: &[Minecart],
        i: usize,
        area: Aabb,
        out: &mut Vec<u32>,
    ) -> bool {
        if !Self::usable() {
            return false;
        }
        let fresh = |ids: &[u32]| ids.len() == carts.len() && ids[i] == carts[i].id;
        if !self.carts.as_ref().is_some_and(|(_, ids)| fresh(ids)) {
            self.carts = Some((
                SeenGrid::new(carts.iter().map(Minecart::bounds)),
                carts.iter().map(|c| c.id).collect(),
            ));
        }
        let Some((grid, ids)) = &mut self.carts else {
            return false;
        };
        grid.update(i, carts[i].bounds());
        if !grid.candidates(area, out) {
            return false;
        }
        if out.iter().any(|&j| ids[j as usize] != carts[j as usize].id) {
            // A cart was replaced in place: start over next time.
            self.carts = None;
            return false;
        }
        true
    }
    /// The slots of the mobs whose box may meet `area` into `out`, and
    /// whether a mob rides cart `cart`; None: scan them all.
    pub(super) fn mobs_near(
        &mut self,
        mobs: &JoinList<Mob>,
        cart: u32,
        area: Aabb,
        out: &mut Vec<u32>,
    ) -> Option<bool> {
        if !Self::usable() {
            return None;
        }
        if !self
            .mobs
            .as_ref()
            .is_some_and(|(stamp, ..)| *stamp == mobs.stamp())
        {
            self.mobs = Some((
                mobs.stamp(),
                SeenGrid::new(mobs.iter().map(Mob::bounds)),
                mobs.iter().filter_map(|m| m.extra.mount).collect(),
            ));
        }
        let (_, grid, mounted) = self.mobs.as_ref()?;
        grid.candidates(area, out).then(|| mounted.contains(&cart))
    }
}
/// Where the cart `id` is in `carts`: at `hint` (where it was a moment ago,
/// the usual case while carts tick in order) or found by a scan. A world
/// with hundreds of mineshaft chest carts asked this of every cart several
/// times a tick.
pub(super) fn cart_at(carts: &[Minecart], id: u32, hint: usize) -> Option<usize> {
    if carts.get(hint).is_some_and(|c| c.id == id) {
        return Some(hint);
    }
    carts.iter().position(|c| c.id == id)
}
