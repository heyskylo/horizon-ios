//! The engine's extension points for resource pack formats vanilla doesn't read (OptiFine's,
//! Polytone's: platform code, docs/layers.md). The engine asks [`formats`] once per pack load and
//! gets engine types back (tables, or an object it calls at most once a frame), never a format's
//! own types. The platform installs its formats at startup ([`install`]); with none installed,
//! packs load as vanilla reads them.

use std::path::Path;
use std::sync::{Arc, OnceLock};

/// What the pack formats beyond vanilla's add to a pack. Each method reads one overlay of a pack
/// (its folder at `root`); the default is "nothing in this format".
pub trait PackFormats: Send + Sync {
    /// The colours the overlay replaces where vanilla hardcodes them.
    fn colors(&self, _root: &Path) -> Option<crate::pack_palette::PackColors> {
        None
    }

    /// The sky layers the overlay draws over the vanilla sky, if it has any.
    fn custom_sky(&self, _root: &Path) -> Option<Arc<dyn crate::custom_sky::SkyLayers>> {
        None
    }

    /// The durability bar colours the overlay sets by item, if any.
    fn item_bars(&self, _root: &Path) -> Option<Arc<dyn crate::pack_palette::ItemBars>> {
        None
    }
}

/// No format beyond vanilla's.
struct Vanilla;

impl PackFormats for Vanilla {}

static FORMATS: OnceLock<&'static dyn PackFormats> = OnceLock::new();

/// Installs the pack formats, before the first pack loads. The first call wins: later ones are
/// ignored, so each test or probe that needs the formats may install them.
pub fn install(formats: &'static dyn PackFormats) {
    let _ = FORMATS.set(formats);
}

/// The installed pack formats, or vanilla's alone.
pub fn formats() -> &'static dyn PackFormats {
    FORMATS.get().copied().unwrap_or(&Vanilla)
}
