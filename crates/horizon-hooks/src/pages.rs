//! Pages (docs/layers.md, "Platform registries"): a crate above the engine adds a settings page to
//! the options screens without an engine file naming it. The engine draws its rows with the same
//! widgets, tooltips, narration, controller focus and `horizon-ctl` ids as its own pages, and asks
//! for them only while the page is open.
use crate::TooLate;
use crate::registry::Registry;

/// Where a page's entry is.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A button on the Horizon Settings page, after the engine's own pages.
    HorizonSettings,
    /// A full-width button of the pause menu, above Options (under Splitscreen's settings). The
    /// pause menu has it only when a page registered there.
    Pause,
}

/// One page. Its values live in the crate (statics, often its settings [`crate::settings::Store`]);
/// the engine rebuilds the rows when it draws the page and after each act.
pub trait Page: Sync {
    /// Stable and unique: `<crate>.<page>`, as `horizon-ctl` and logs name it.
    fn id(&self) -> &'static str;
    /// The page's title and its entry's label: a translation key.
    fn title(&self) -> &'static str;
    /// Where its entry is.
    fn place(&self) -> Place;
    /// Its rows, top to bottom.
    fn rows(&self, rows: &mut Vec<Row>);
    /// The player acted on row `row` (its index in [`Page::rows`]). The options screen saves the
    /// settings when it closes, as for every setting.
    fn act(&self, row: usize, act: Act);
    /// What row `row` says on hover (and the narrator after its label), which may follow its
    /// value. None by default.
    fn tooltip(&self, _row: usize) -> Option<Label> {
        None
    }
}

/// A row of a page. Labels are translation keys.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A section's heading.
    Heading(&'static str),
    /// A line of grey text: a value such as a path, shown as it is.
    Note(String),
    /// `<label>: ON` or `OFF`; a press flips it.
    Toggle { label: &'static str, on: bool },
    /// `<label>: <value>`; a press steps forward, a shift-press back ([`Act::PressBack`]).
    Cycle { label: &'static str, value: Label },
    /// `<label>: <text>` on a slider at `value` (0 to 1); dragging or the arrow keys send
    /// [`Act::Slide`].
    Slider {
        label: &'static str,
        value: f32,
        text: Label,
    },
    /// A button that does something.
    Button { label: &'static str },
    /// A key binding, as Key Binds shows one: `key` is its key name (`key.keyboard.z`,
    /// `key.mouse.left`). A press waits for the next key or mouse button ([`Act::Bind`]).
    Key { label: &'static str, key: String },
}

/// A value shown on a row: a translation key, or text shown as it is (a number, a name).
#[derive(Clone, Debug, PartialEq)]
pub enum Label {
    Key(&'static str),
    Value(String),
}

/// What the player did to a row.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// A click, or Enter or the controller's select on the row.
    Press,
    /// The same with Shift held: a cycle steps back.
    PressBack,
    /// A slider moved to this position (0 to 1).
    Slide(f32),
    /// A key row got its new key: its key name (`key.keyboard.z`).
    Bind(String),
}

static PAGES: Registry<dyn Page> = Registry::new();

/// Adds `page` to the game's options screens. Before the game starts, as [`crate::register`];
/// entries keep the order pages registered in.
pub fn register(page: &'static dyn Page) -> Result<(), TooLate> {
    PAGES.register(page)
}

/// The engine's side. Only the engine calls it.
pub mod engine {
    use super::*;

    /// Every registered page, sealing the list on the first call.
    pub fn all() -> &'static [&'static dyn Page] {
        PAGES.all()
    }
}
