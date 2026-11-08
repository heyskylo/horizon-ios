//! Crafting a recipe straight into the inventory with a controller (Horizon's own;
//! docs/controllers.md): LT + A crafts once, and with both still held past the repeat delay it
//! goes on until either is let go, the items run out or the inventory is full.
//!
//! Each craft is what a player does by hand with the recipe book, one request at a time: the
//! recipe placed (`ServerboundPlaceRecipePacket`), its result quick-moved
//! (`ServerboundContainerClickPacket`), and whatever stays in the grid, remainders included,
//! quick-moved back. The next request waits for the server's answer to the last one, nothing is
//! predicted, so the items are always the server's.
//!
//! The book's numbers are worked out here, and a server with recipes of its own can number them
//! differently, so a placement may make something else. A result that isn't the recipe's stops the
//! craft with nothing moved: the grid and the result stay as the click on the recipe left them,
//! for the player to take or put back by hand.
//!
//! No answer in time ends the craft at once and nothing more goes out: a quick move can't return
//! what a late answer has yet to place, and a server that isn't answering would leave it waiting
//! for ever. So what the server placed stays in the grid. The player sees it there, and the way
//! out is what it always is: closing the screen returns the grid, and the next LT + A returns it
//! before it places anything.
//!
//! Vanilla's quick move throws what doesn't fit on the ground (`CraftingMenu.quickMoveStack`'s
//! `player.drop`), so a result only moves with room for all of it as the client last heard the
//! inventory. A full inventory stops the craft with the ingredients back where they came from, or
//! in the grid if they can't go back. The server's inventory can be ahead of that: a pickup the
//! client hasn't heard of yet can fill the last room, and then the server's quick move drops the
//! rest of the result, as a vanilla shift-click on the result does.
//!
//! Requests to any server but the window's own (single player) keep their distance: placements
//! `PLACE_GAP` apart, under Paper's limit on them, and on someone else's server every request
//! `REQUEST_GAP` apart as well.
//!
//! However a craft ends while the server can still answer (a button let go, another press but
//! LT + A on its recipe, which asks for one more, the pad gone, focus lost), it ends one way: no new craft starts, the step in flight finishes, and
//! what's in the grid goes back ([`DirectCraft::stop`]).
use super::Game;
use horizon_client::{gamepad, items::ItemStack};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How long a request waits for the server's answer before the craft ends.
const ANSWER_WAIT: Duration = Duration::from_secs(2);
/// The gap between any two requests to someone else's server: one a game tick (50 ms) at most,
/// so a held craft never sends faster than the game ticks.
const REQUEST_GAP: Duration = Duration::from_millis(50);
/// The gap between two placements on any server but the window's own. Paper's default
/// `packet-limiter` drops a `ServerboundPlaceRecipePacket` beyond 5 a second averaged over 4
/// seconds (`GlobalConfiguration.PacketLimiter`'s override for it: interval 4.0, max-packet-rate
/// 5.0, action DROP); a dropped one would go unanswered and end the craft. One every 250 ms is 4 a
/// second, under it.
const PLACE_GAP: Duration = Duration::from_millis(250);
/// The most crafts one craft makes: a full inventory of stacks, one item a craft.
const MOST_CRAFTS: u32 = 36 * 64;

/// What the last request is waiting to see.
#[derive(Clone, Debug, PartialEq)]
enum Answer {
    /// The recipe placed: a result shows.
    Placed,
    /// The result moved: the slot is empty and the inventory holds this many of the item.
    Moved { item: Arc<str>, total: u32 },
    /// This many items were in this grid cell: fewer are now.
    Returned { cell: usize, count: u32 },
    /// The grid is empty: the old result goes.
    NoResult,
}

/// The next thing to do.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Wait,
    Stop,
    Place,
    MoveResult,
    Return(usize),
}

/// The open menu as the client last heard it from the server.
struct Seen<'a> {
    grid: &'a [Option<ItemStack>],
    result: Option<&'a ItemStack>,
    /// What the recipe makes (`Entry::results`).
    recipe_results: &'a [ItemStack],
    inventory: &'a [Option<ItemStack>],
    craftable: bool,
    max_stack: &'a dyn Fn(&ItemStack) -> u32,
}

impl Seen<'_> {
    /// `result` is what the recipe makes. Item and count only: a recipe can pass its input's
    /// components on to its result (`TransmuteRecipe`), so those can differ from the book's.
    fn makes(&self, result: &ItemStack) -> bool {
        self.recipe_results
            .iter()
            .any(|made| made.item == result.item && made.count == result.count)
    }
    fn count_of(&self, item: &str) -> u32 {
        self.inventory
            .iter()
            .flatten()
            .filter(|s| &*s.item == item)
            .map(|s| s.count)
            .sum()
    }
    /// How many of `stack` fit in the inventory (`moveItemStackTo`).
    fn room_for(&self, stack: &ItemStack) -> u32 {
        let max = (self.max_stack)(stack);
        self.inventory
            .iter()
            .map(|slot| match slot {
                None => max,
                Some(s) if s.same_item_same_components(stack) => max.saturating_sub(s.count),
                Some(_) => 0,
            })
            .sum()
    }
}

/// A crafting book's recipe by what stays the same when the book is rebuilt: its recipe id, and
/// its display id among the entries that share the id. Its index in the book can move.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RecipeKey {
    pub(super) id: String,
    pub(super) display: i32,
}

/// A craft under way.
pub(super) struct DirectCraft {
    recipe: RecipeKey,
    /// The window it started in: another one ends it.
    menu: i32,
    /// The grid's size it started with: another one ends it.
    cells: usize,
    /// The crafts the presses asked for, and the crafts made.
    asked: u32,
    done: u32,
    /// Both buttons are still down from the press: past `repeat_at` it keeps crafting.
    holding: bool,
    repeat_at: Instant,
    /// The grid holds the recipe this craft placed, so its result is the craft's to take.
    placed: bool,
    answer: Option<(Answer, Instant)>,
    next_request: Instant,
    /// The gap kept after every request: `REQUEST_GAP` on someone else's server, else none.
    request_gap: Duration,
    next_place: Instant,
    /// The gap kept after every placement: `PLACE_GAP` on any server but the window's own.
    place_gap: Duration,
}

impl DirectCraft {
    fn new(recipe: RecipeKey, menu: i32, cells: usize, repeat_at: Instant, now: Instant) -> Self {
        Self {
            recipe,
            menu,
            cells,
            asked: 1,
            done: 0,
            holding: true,
            repeat_at,
            placed: false,
            answer: None,
            next_request: now,
            request_gap: Duration::ZERO,
            next_place: now,
            place_gap: Duration::ZERO,
        }
    }

    /// A button of the press came up: what was asked for still gets made, and no more.
    fn let_go(&mut self) {
        self.holding = false;
    }

    /// The end of a craft while the server can still answer: no new craft starts, the one in
    /// flight finishes, and `step` returns the grid before it says `Stop`.
    pub(super) fn stop(&mut self) {
        self.holding = false;
        let in_flight = self.placed || matches!(self.answer, Some((Answer::Placed, _)));
        self.asked = self.done + u32::from(in_flight);
    }

    /// The server said the placement it was asked for can't be made: nothing's in flight.
    fn refused(&mut self) {
        if matches!(self.answer, Some((Answer::Placed, _))) {
            self.answer = None;
        }
        self.stop();
    }

    /// LT + A on this recipe again before the last has finished: one more, on the same footing as
    /// the first, and still one request at a time.
    fn again(&mut self, repeat_at: Instant) {
        self.asked = self.asked.saturating_add(1);
        self.holding = true;
        self.repeat_at = repeat_at;
    }

    /// The next step, and the gaps it leaves before the next request and placement.
    fn step(&mut self, seen: &Seen, now: Instant) -> Step {
        let step = self.next_step(seen, now);
        if step != Step::Wait {
            self.next_request = now + self.request_gap;
        }
        if step == Step::Place {
            self.next_place = now + self.place_gap;
        }
        step
    }

    fn next_step(&mut self, seen: &Seen, now: Instant) -> Step {
        if let Some((answer, deadline)) = &self.answer {
            let answered = match answer {
                Answer::Placed => seen.result.is_some(),
                Answer::Moved { item, total } => {
                    seen.result.is_none() && seen.count_of(item) >= *total
                }
                Answer::Returned { cell, count } => {
                    let left = seen.grid.get(*cell).and_then(Option::as_ref);
                    left.map_or(0, |s| s.count) < *count
                }
                Answer::NoResult => seen.result.is_none(),
            };
            if !answered {
                return if now >= *deadline {
                    Step::Stop
                } else {
                    Step::Wait
                };
            }
            match answer {
                Answer::Placed => self.placed = true,
                Answer::Moved { .. } => {
                    self.placed = false;
                    self.done += 1;
                }
                Answer::Returned { .. } | Answer::NoResult => {}
            }
            self.answer = None;
        }
        if now < self.next_request {
            return Step::Wait;
        }
        let deadline = now + ANSWER_WAIT;
        if self.placed {
            let Some(result) = seen.result else {
                return Step::Stop;
            };
            if !seen.makes(result) {
                return Step::Stop;
            }
            if seen.room_for(result) >= result.count {
                let total = seen.count_of(&result.item) + result.count;
                let item = result.item.clone();
                self.answer = Some((Answer::Moved { item, total }, deadline));
                return Step::MoveResult;
            }
            // No room: what was placed goes back, and that's the end of it.
            self.placed = false;
            self.stop();
        }
        // What's in the grid leaves first (a quick move drops nothing), so the place below can't
        // send it to `clearGrid`, which a creative player's menu may throw away.
        if let Some((cell, stack)) = seen
            .grid
            .iter()
            .enumerate()
            .find_map(|(i, s)| s.as_ref().map(|s| (i, s)))
        {
            if seen.room_for(stack) == 0 {
                return Step::Stop;
            }
            let count = stack.count;
            self.answer = Some((Answer::Returned { cell, count }, deadline));
            return Step::Return(cell);
        }
        if seen.result.is_some() {
            self.answer = Some((Answer::NoResult, deadline));
            return Step::Wait;
        }
        let asked = self.done < self.asked;
        if (!asked && !self.holding) || self.done >= MOST_CRAFTS || !seen.craftable {
            return Step::Stop;
        }
        if (!asked && now < self.repeat_at) || now < self.next_place {
            return Step::Wait;
        }
        self.answer = Some((Answer::Placed, deadline));
        Step::Place
    }
}

impl Game<'_> {
    /// A server direct crafting needs no say of the player's on: one found through LAN discovery
    /// (the server list's announced worlds, `server_is_lan`), kept through a reconnect, or a split
    /// guest's link by memory to the host's world on this window (`"local"`). One reached by
    /// address isn't, even on this computer or the local network: an address can be a proxy, a
    /// tunnel or a friend's relay as well.
    fn direct_craft_free_server(&self) -> bool {
        self.server_is_lan()
            || self
                .remote
                .as_ref()
                .is_some_and(|r| r.session.address == "local")
    }

    /// Someone else's server: neither single player's own nor a free one
    /// (`direct_craft_free_server`). Every request keeps its distance there, and crafting waits for
    /// the player's say (`craft_on_servers`).
    fn direct_craft_elsewhere(&self) -> bool {
        self.integrated.is_none() && !self.direct_craft_free_server()
    }

    /// Whether direct crafting is allowed where the player is: only through a server, as every
    /// world is played (its quick moves are the server's), and on someone else's only if the
    /// player allowed it (`craft_on_servers`).
    fn direct_craft_allowed(&self) -> bool {
        self.remote.is_some()
            && (!self.direct_craft_elsewhere() || self.settings.controller.craft_on_servers)
    }

    /// The pad can direct-craft here, with nothing carried: the inventory's guide says so on a
    /// recipe. Whether the recipe can be made isn't checked, as that counts the inventory.
    pub(super) fn direct_craft_usable(&self) -> bool {
        self.pad.in_use
            && self.carried.is_none()
            && self.mod_recipe_screen()
            && self.direct_craft_allowed()
    }

    /// This press is another LT + A on the running craft's recipe: it asks for one more craft
    /// rather than ending the craft.
    pub(super) fn direct_craft_takes_press(&self, button: gamepad::Button) -> bool {
        button == gamepad::Button::South
            && self.pad.edges.is_held(gamepad::Button::LeftTrigger)
            && self.screen == super::Screen::Inventory
            && self.title.is_none()
            && !self.prompt_open()
            && self.pad.menu.craft.as_ref().is_some_and(|craft| {
                self.recipe_under_pointer()
                    .and_then(|entry| self.recipe_stable_key(entry))
                    .is_some_and(|key| key == craft.recipe)
            })
    }

    /// LT + A on a recipe: whether it started a craft, or asked the running one for another.
    /// Refused with an item carried, on a book other than crafting, when the recipe can't be made,
    /// where it isn't allowed, or while another recipe's craft is still finishing (one request at
    /// a time); a refused press is plain A's.
    pub(super) fn direct_craft_start(&mut self, entry: usize) -> bool {
        let now = Instant::now();
        let repeat_delay = self.settings.controller.repeat_delay;
        let repeat_at = now + Duration::from_secs_f32(repeat_delay);
        let Some(recipe) = self.recipe_stable_key(entry) else {
            return false;
        };
        if let Some(craft) = &mut self.pad.menu.craft {
            if craft.recipe != recipe {
                return false;
            }
            craft.again(repeat_at);
            return true;
        }
        if self.carried.is_some()
            || !self.mod_recipe_screen()
            || !self.direct_craft_allowed()
            || !self.recipe_craftable(entry)
        {
            return false;
        }
        let menu = self.remote_menu().0;
        let cells = self.craft_size * self.craft_size;
        let mut craft = DirectCraft::new(recipe, menu, cells, repeat_at, now);
        if self.direct_craft_elsewhere() {
            craft.request_gap = REQUEST_GAP;
        }
        if self.integrated.is_none() {
            craft.place_gap = PLACE_GAP;
        }
        self.pad.menu.craft = Some(craft);
        true
    }

    /// Anything that ends a running craft while the server can still answer; a click or key
    /// behind it goes on to the screen.
    pub(super) fn direct_craft_stop(&mut self) {
        self.pad.menu.stop_craft();
    }

    /// The server said a placement can't be made.
    pub(super) fn direct_craft_refused(&mut self) {
        if let Some(craft) = &mut self.pad.menu.craft {
            craft.refused();
        }
    }

    /// Once per poll: the running craft's next step, when the last one has been answered.
    pub(super) fn direct_craft_poll(&mut self) {
        let Some(mut craft) = self.pad.menu.craft.take() else {
            return;
        };
        let cells = self.craft_size * self.craft_size;
        // The screen went, or isn't the one the craft's grid was on.
        if !self.mod_recipe_screen() || self.remote_menu().0 != craft.menu || cells != craft.cells {
            return;
        }
        // Something came into the hand: a quick move still takes the grid back.
        if self.carried.is_some() {
            craft.stop();
        }
        if !self.pad.edges.is_held(gamepad::Button::South)
            || !self.pad.edges.is_held(gamepad::Button::LeftTrigger)
        {
            craft.let_go();
        }
        // The book was rebuilt without the recipe: what's in the grid still goes back, and a
        // result can't be told for the recipe's, so it stays.
        let entry = self.recipe_entry_index(&craft.recipe);
        if entry.is_none() {
            craft.stop();
        }
        let craftable = entry.is_some_and(|entry| self.recipe_craftable(entry));
        let result = self.slot_view(200);
        let now = Instant::now();
        let step = {
            // Room counted too high would drop part of a result.
            let max_stack = |s: &ItemStack| self.stack_max_size(s);
            craft.step(
                &Seen {
                    grid: &self.craft_grid[..cells],
                    result: result.as_ref(),
                    recipe_results: entry.map_or(&[], |entry| self.recipe_results(entry)),
                    inventory: &self.inventory[..36],
                    craftable,
                    max_stack: &max_stack,
                },
                now,
            )
        };
        match (step, entry) {
            (Step::Wait, _) => {}
            (Step::Place, Some(entry)) => self.recipe_place_once(entry),
            (Step::Stop, _) | (Step::Place, None) => return,
            // Only ever started with a server (`direct_craft_allowed`): its quick moves are the
            // server's, unpredicted.
            (Step::MoveResult, _) => {
                self.remote_quick_move_unpredicted(200);
            }
            (Step::Return(cell), _) => {
                self.remote_quick_move_unpredicted(100 + cell);
            }
        }
        self.pad.menu.craft = Some(craft);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(item: &str, count: u32) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }

    /// A server's menu, as `Seen` shows it to the craft.
    struct Menu {
        grid: Vec<Option<ItemStack>>,
        result: Option<ItemStack>,
        recipe_results: Vec<ItemStack>,
        inventory: Vec<Option<ItemStack>>,
        craftable: bool,
    }

    impl Menu {
        fn new(inventory: Vec<Option<ItemStack>>) -> Self {
            Self {
                grid: vec![None; 4],
                result: None,
                // Every result the tests' crafts make.
                recipe_results: vec![
                    ItemStack::new("minecraft:oak_planks", 4),
                    ItemStack::new("minecraft:stick", 4),
                    ItemStack::new("minecraft:cake", 1),
                ],
                inventory,
                craftable: true,
            }
        }
        fn step(&self, craft: &mut DirectCraft, now: Instant) -> Step {
            let max_stack = |s: &ItemStack| {
                if &*s.item == "minecraft:bucket" {
                    16
                } else {
                    64
                }
            };
            craft.step(
                &Seen {
                    grid: &self.grid,
                    result: self.result.as_ref(),
                    recipe_results: &self.recipe_results,
                    inventory: &self.inventory,
                    craftable: self.craftable,
                    max_stack: &max_stack,
                },
                now,
            )
        }
    }

    /// A press that came up at once: it asks for one craft.
    fn tap(now: Instant) -> DirectCraft {
        let mut craft = held(now, now);
        craft.let_go();
        craft
    }

    /// A press still down, going on from `repeat_at`.
    fn held(repeat_at: Instant, now: Instant) -> DirectCraft {
        let recipe = RecipeKey {
            id: "minecraft:stick".into(),
            display: 0,
        };
        DirectCraft::new(recipe, 0, 4, repeat_at, now)
    }

    #[test]
    fn nothing_goes_out_until_the_server_answers() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![stack("minecraft:oak_log", 1), None]);
        let mut craft = tap(now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.inventory[0] = None;
        menu.grid[0] = stack("minecraft:oak_log", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.result = stack("minecraft:oak_planks", 4);
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        // The result went, but the slots holding it haven't come in yet.
        menu.grid[0] = None;
        menu.result = None;
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.inventory[0] = stack("minecraft:oak_planks", 4);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn no_answer_ends_the_craft_without_asking_again() {
        let now = Instant::now();
        let menu = Menu::new(vec![stack("minecraft:oak_log", 1)]);
        let mut craft = held(now, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        assert_eq!(menu.step(&mut craft, now + ANSWER_WAIT / 2), Step::Wait);
        assert_eq!(menu.step(&mut craft, now + ANSWER_WAIT), Step::Stop);
    }

    #[test]
    fn a_craft_that_times_out_with_a_full_grid_sends_nothing_and_the_next_press_clears_it() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(now, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        // The ingredients came, the result never did: no return is asked of a silent server.
        menu.grid[0] = stack("minecraft:oak_log", 1);
        assert_eq!(menu.step(&mut craft, now + ANSWER_WAIT), Step::Stop);
        // The way out: another LT + A takes the grid back before it places anything.
        let later = now + ANSWER_WAIT;
        let mut next = tap(later);
        assert_eq!(menu.step(&mut next, later), Step::Return(0));
        menu.grid[0] = None;
        menu.inventory[0] = stack("minecraft:oak_log", 1);
        assert_eq!(menu.step(&mut next, later), Step::Place);
    }

    #[test]
    fn a_result_that_is_not_the_recipes_stops_the_craft_with_nothing_moved() {
        let now = Instant::now();
        // Another item, then the recipe's item in another count: neither is what was asked for.
        for other in [
            ItemStack::new("minecraft:diamond", 1),
            ItemStack::new("minecraft:oak_planks", 2),
        ] {
            let mut menu = Menu::new(vec![None; 2]);
            let mut craft = held(now, now);
            assert_eq!(menu.step(&mut craft, now), Step::Place);
            menu.grid[0] = stack("minecraft:oak_log", 1);
            menu.result = Some(other);
            // A stop, neither `MoveResult` nor a `Return`: the grid is the player's to see to.
            assert_eq!(menu.step(&mut craft, now), Step::Stop);
        }
    }

    #[test]
    fn a_stop_with_the_placement_in_flight_still_moves_nothing_unexpected() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(now, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        craft.stop();
        menu.grid[0] = stack("minecraft:oak_log", 1);
        menu.result = stack("minecraft:diamond", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn any_of_a_recipes_results_is_its_own() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        // A recipe of many results, as a tipped arrow's: each potion is the same recipe.
        menu.recipe_results = vec![
            ItemStack::new("minecraft:tipped_arrow", 8),
            ItemStack::new("minecraft:spectral_arrow", 8),
        ];
        let mut craft = tap(now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        menu.result = stack("minecraft:spectral_arrow", 8);
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
    }

    #[test]
    fn a_hold_goes_on_until_the_items_run_out() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(now, now);
        for done in 1..=3 {
            assert_eq!(menu.step(&mut craft, now), Step::Place);
            menu.result = stack("minecraft:stick", 4);
            assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
            menu.result = None;
            menu.inventory[0] = stack("minecraft:stick", 4 * done);
        }
        menu.craftable = false;
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn a_result_without_room_for_all_of_it_never_moves() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![
            stack("minecraft:oak_planks", 62),
            stack("minecraft:oak_log", 2),
        ]);
        let mut craft = held(now, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        menu.inventory[1] = stack("minecraft:oak_log", 1);
        menu.grid[0] = stack("minecraft:oak_log", 1);
        menu.result = stack("minecraft:oak_planks", 4);
        // Room for two of the four: the log goes back, and the craft ends there.
        assert_eq!(menu.step(&mut craft, now), Step::Return(0));
        menu.grid[0] = None;
        menu.result = None;
        menu.inventory[1] = stack("minecraft:oak_log", 2);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn a_full_inventory_leaves_the_remainder_in_the_grid() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![stack("minecraft:dirt", 64)]);
        let mut craft = held(now, now);
        menu.grid[2] = stack("minecraft:bucket", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn a_remainder_goes_back_before_the_next_placement() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None, None]);
        let mut craft = held(now, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        menu.result = stack("minecraft:cake", 1);
        menu.grid[1] = stack("minecraft:milk_bucket", 1);
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
        menu.result = None;
        menu.grid[1] = stack("minecraft:bucket", 1);
        menu.inventory[0] = stack("minecraft:cake", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Return(1));
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.grid[1] = None;
        menu.inventory[1] = stack("minecraft:bucket", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
    }

    #[test]
    fn what_the_player_left_in_the_grid_goes_back_rather_than_being_crafted() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None]);
        menu.grid[0] = stack("minecraft:oak_log", 1);
        menu.result = stack("minecraft:oak_planks", 4);
        let mut craft = tap(now);
        assert_eq!(menu.step(&mut craft, now), Step::Return(0));
        menu.grid[0] = None;
        menu.inventory[0] = stack("minecraft:oak_log", 1);
        // The old result hasn't gone yet: no placement over it.
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.result = None;
        assert_eq!(menu.step(&mut craft, now), Step::Place);
    }

    /// One craft from its placement to its result in the inventory, `total` of them by then.
    fn make_one(menu: &mut Menu, craft: &mut DirectCraft, total: u32, now: Instant) {
        assert_eq!(menu.step(craft, now), Step::Place);
        menu.result = stack("minecraft:stick", 4);
        assert_eq!(menu.step(craft, now), Step::MoveResult);
        menu.result = None;
        menu.inventory[0] = stack("minecraft:stick", total);
    }

    #[test]
    fn a_tap_makes_one_and_a_hold_waits_for_the_delay() {
        let now = Instant::now();
        let later = now + Duration::from_millis(350);
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = tap(now);
        make_one(&mut menu, &mut craft, 4, now);
        assert_eq!(menu.step(&mut craft, later), Step::Stop);
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(later, now);
        make_one(&mut menu, &mut craft, 4, now);
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        assert_eq!(menu.step(&mut craft, later), Step::Place);
    }

    #[test]
    fn letting_go_before_the_delay_ends_the_wait_at_once() {
        let now = Instant::now();
        let later = now + Duration::from_secs(60);
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(later, now);
        make_one(&mut menu, &mut craft, 4, now);
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        craft.let_go();
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn letting_go_finishes_the_craft_in_flight_and_empties_the_grid() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 3]);
        let mut craft = held(now, now);
        make_one(&mut menu, &mut craft, 4, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        // Let go with the placement unanswered.
        craft.let_go();
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.result = stack("minecraft:cake", 1);
        menu.grid[1] = stack("minecraft:milk_bucket", 1);
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
        menu.result = None;
        menu.grid[1] = stack("minecraft:bucket", 1);
        menu.inventory[1] = stack("minecraft:cake", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Return(1));
        menu.grid[1] = None;
        menu.inventory[2] = stack("minecraft:bucket", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn stopping_before_anything_went_out_sends_nothing() {
        let now = Instant::now();
        let menu = Menu::new(vec![None]);
        let mut craft = tap(now);
        craft.stop();
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn stopping_keeps_only_the_craft_in_flight() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(now, now);
        make_one(&mut menu, &mut craft, 4, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        craft.stop();
        menu.result = stack("minecraft:stick", 4);
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
        craft.stop();
        menu.result = None;
        menu.inventory[0] = stack("minecraft:stick", 8);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn a_refused_placement_ends_it_with_the_grid_returned() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(now, now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        craft.refused();
        menu.grid[0] = stack("minecraft:oak_log", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Return(0));
        menu.grid[0] = None;
        menu.inventory[0] = stack("minecraft:oak_log", 1);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn another_press_of_the_recipe_waits_its_turn() {
        let now = Instant::now();
        let later = now + Duration::from_secs(60);
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = tap(now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        craft.stop();
        craft.again(later);
        craft.let_go();
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.result = stack("minecraft:stick", 4);
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
        menu.result = None;
        menu.inventory[0] = stack("minecraft:stick", 4);
        // Asked for, so it needs no delay.
        make_one(&mut menu, &mut craft, 8, now);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn quick_taps_while_one_is_in_flight_each_make_a_craft() {
        let now = Instant::now();
        let later = now + Duration::from_secs(60);
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = tap(now);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        // Two more taps of LT + A on the recipe, both up again before the first craft answers.
        for _ in 0..2 {
            craft.again(later);
            craft.let_go();
        }
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        menu.result = stack("minecraft:stick", 4);
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
        menu.result = None;
        menu.inventory[0] = stack("minecraft:stick", 4);
        make_one(&mut menu, &mut craft, 8, now);
        make_one(&mut menu, &mut craft, 12, now);
        assert_eq!(menu.step(&mut craft, now), Step::Stop);
    }

    #[test]
    fn placements_keep_their_gap_while_the_other_requests_do_not() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(now, now);
        craft.place_gap = PLACE_GAP;
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        menu.result = stack("minecraft:stick", 4);
        // The result moves as soon as it shows.
        assert_eq!(menu.step(&mut craft, now), Step::MoveResult);
        menu.result = None;
        menu.inventory[0] = stack("minecraft:stick", 4);
        // The next placement waits out the gap from the last.
        assert_eq!(menu.step(&mut craft, now + PLACE_GAP / 2), Step::Wait);
        assert_eq!(menu.step(&mut craft, now + PLACE_GAP), Step::Place);
    }

    #[test]
    fn every_request_keeps_its_gap_when_asked_to() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None; 2]);
        let mut craft = held(now, now);
        craft.request_gap = REQUEST_GAP;
        assert_eq!(menu.step(&mut craft, now), Step::Place);
        menu.result = stack("minecraft:stick", 4);
        assert_eq!(menu.step(&mut craft, now), Step::Wait);
        assert_eq!(menu.step(&mut craft, now + REQUEST_GAP), Step::MoveResult);
    }

    #[test]
    fn a_grid_that_shrank_is_no_panic() {
        let now = Instant::now();
        let mut menu = Menu::new(vec![None]);
        menu.grid[3] = stack("minecraft:oak_log", 2);
        let mut craft = tap(now);
        assert_eq!(menu.step(&mut craft, now), Step::Return(3));
        menu.grid.truncate(1);
        assert_eq!(menu.step(&mut craft, now), Step::Place);
    }

    #[test]
    fn room_counts_only_stacks_that_can_take_it() {
        let inventory = vec![
            stack("minecraft:stick", 60),
            stack("minecraft:bucket", 16),
            None,
        ];
        let max_stack = |s: &ItemStack| {
            if &*s.item == "minecraft:bucket" {
                16
            } else {
                64
            }
        };
        let seen = Seen {
            grid: &[],
            result: None,
            recipe_results: &[],
            inventory: &inventory,
            craftable: true,
            max_stack: &max_stack,
        };
        assert_eq!(seen.room_for(&ItemStack::new("minecraft:stick", 1)), 68);
        assert_eq!(seen.room_for(&ItemStack::new("minecraft:bucket", 1)), 16);
        let damaged = ItemStack {
            damage: 3,
            ..ItemStack::new("minecraft:stick", 1)
        };
        assert_eq!(seen.room_for(&damaged), 64);
    }
}
