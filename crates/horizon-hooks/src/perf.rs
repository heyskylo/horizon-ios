//! Open and share a saved performance capture; with none registered the game offers neither.
use std::path::Path;

pub trait Captures: Sync {
    /// The viewer page, for the Open link.
    fn viewer(&self) -> &str;
    /// Serves `file` once on 127.0.0.1 and opens the viewer on it.
    fn open(&self, file: &Path) -> Result<(), String>;
    /// Uploads `file`; the share link.
    fn share(&self, file: &Path) -> Result<String, String>;
}
