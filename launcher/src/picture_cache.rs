//! The pictures Home draws from the player's disk (screenshots, their blurred copies, the
//! background), made in the background at the size they are drawn and kept under a byte budget
//! (`horizon_launcher::pictures`).
//!
//! GPUI's `img(path)` keeps every picture it ever drew, at full size, until the launcher quits,
//! in RAM and in its sprite atlas: one 32768 × 16384 screenshot is 2 GiB, and Overview draws the
//! three newest as soon as Home opens (24 GiB reported on Linux, 2026-11-05).
use super::*;
use gpui::RenderImage;
use horizon_launcher::pictures::{self, Lru};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A screenshot tile (196 to 335 px wide, twice that on a Retina screen).
pub(super) const THUMB: u32 = 640;
/// The viewer and the background: a 4K screen.
pub(super) const FULL: u32 = 3840;
/// What the made pictures may take together, unless more than that is on screen at once.
const BUDGET: u64 = 256 << 20;
/// A picture drawn this recently is on screen: it stays even over the budget.
const GRACE: Duration = Duration::from_secs(2);
/// Pictures made at the same time; the others wait for the next frame.
const AT_ONCE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Look {
    /// Fitted inside this many pixels square.
    Fit(u32),
    /// Small and blurred (`glass_shot_blur`).
    Blurred,
}

type Key = (PathBuf, Look);

pub(super) struct Pictures {
    /// None: the file couldn't be read as a picture.
    made: Lru<Key, Option<Arc<RenderImage>>>,
    making: HashSet<Key>,
    /// Gone from `made`; freed from the window's sprite atlas on the next frame.
    retired: Vec<Arc<RenderImage>>,
}

impl Default for Pictures {
    fn default() -> Self {
        Pictures {
            made: Lru::new(BUDGET, GRACE),
            making: HashSet::new(),
            retired: Vec::new(),
        }
    }
}

impl Pictures {
    fn retire(&mut self, gone: Vec<Option<Arc<RenderImage>>>) {
        self.retired.extend(gone.into_iter().flatten());
    }

    /// The picture if it is already made, without making it.
    pub(super) fn made(&self, path: &Path, look: Look) -> Option<Arc<RenderImage>> {
        self.made
            .peek(&(path.to_path_buf(), look))
            .cloned()
            .flatten()
    }

    /// Frees the pictures dropped since the last frame where GPUI keeps them too.
    pub(super) fn free_retired(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
    }
}

impl Launcher {
    /// The picture at `path` as `look`; None while it is being made, or when it can't be read
    /// (draw a plain fill then).
    pub(super) fn picture(
        &mut self,
        path: &Path,
        look: Look,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let now = Instant::now();
        let key = (path.to_path_buf(), look);
        if let Some(made) = self.pictures.made.get(&key, now).cloned() {
            if self.pictures.made.over_budget() {
                let gone = self.pictures.made.trim(now);
                self.pictures.retire(gone);
            }
            return made;
        }
        if self.pictures.making.len() < AT_ONCE && self.pictures.making.insert(key.clone()) {
            let task = cx.background_executor().spawn(async move {
                let image = make(&key.0, look);
                (key, image)
            });
            cx.spawn(async move |this, cx| {
                let (key, image) = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.pictures.making.remove(&key);
                    let bytes = image
                        .as_ref()
                        .and_then(|i| i.as_bytes(0))
                        .map_or(0, |b| b.len() as u64);
                    let gone = this.pictures.made.insert(key, image, bytes, Instant::now());
                    this.pictures.retire(gone);
                    cx.notify();
                });
            })
            .detach();
        }
        None
    }
}

/// Makes the picture as GPUI draws it (BGRA, as its own loader does). Runs off the UI thread.
fn make(path: &Path, look: Look) -> Option<Arc<RenderImage>> {
    match look {
        Look::Blurred => glass_shot_blur::blurred(path),
        Look::Fit(max) => {
            let mut image = pictures::load_fitted(path, max).ok()?;
            for p in image.pixels_mut() {
                p.0.swap(0, 2);
            }
            Some(Arc::new(RenderImage::new([image::Frame::new(image)])))
        }
    }
}
