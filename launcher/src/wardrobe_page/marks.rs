//! Name tag styles' marks, baked once for an item's file at the window's scale.
use gpui::RenderImage;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) struct Marks<T = Arc<RenderImage>> {
    scale: f32,
    /// By item id: the hash of the file it was baked from, and the mark.
    baked: HashMap<String, (String, T)>,
    /// Marks no longer drawn, the atlas's to free.
    replaced: Vec<T>,
}

impl<T> Default for Marks<T> {
    fn default() -> Self {
        Self {
            scale: 0.,
            baked: HashMap::new(),
            replaced: Vec::new(),
        }
    }
}

impl<T> Marks<T> {
    /// `id`'s mark for the file `hash` at `scale`, baked here the first time.
    pub(super) fn get_or_bake(
        &mut self,
        id: &str,
        hash: &str,
        scale: f32,
        bake: impl FnOnce() -> T,
    ) -> &T {
        if self.scale != scale {
            self.scale = scale;
            self.replaced
                .extend(self.baked.drain().map(|(_, (_, mark))| mark));
        }
        if !self.baked.get(id).is_some_and(|(h, _)| h == hash) {
            let fresh = (hash.to_owned(), bake());
            if let Some((_, old)) = self.baked.insert(id.to_owned(), fresh) {
                self.replaced.push(old);
            }
        }
        &self.baked[id].1
    }

    fn take_replaced(&mut self) -> Vec<T> {
        std::mem::take(&mut self.replaced)
    }
}

impl Marks {
    /// Frees the marks replaced since the last frame (GPUI keeps them in the sprite atlas until
    /// they are dropped there).
    pub(crate) fn free_replaced(&mut self, window: &mut gpui::Window) {
        for image in self.take_replaced() {
            let _ = window.drop_image(image);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache whose bakes number themselves, so each answer tells which bake made it.
    fn counting() -> (Marks<u32>, u32) {
        (Marks::default(), 0)
    }

    fn mark(marks: &mut Marks<u32>, made: &mut u32, id: &str, hash: &str, scale: f32) -> u32 {
        *marks.get_or_bake(id, hash, scale, || {
            *made += 1;
            *made
        })
    }

    #[test]
    fn the_same_id_and_scale_reuse_the_mark() {
        let (mut marks, mut made) = counting();
        assert_eq!(mark(&mut marks, &mut made, "gold", "h1", 1.), 1);
        assert_eq!(mark(&mut marks, &mut made, "gold", "h1", 1.), 1);
        assert_eq!(mark(&mut marks, &mut made, "ruby", "h9", 1.), 2);
        assert_eq!(mark(&mut marks, &mut made, "gold", "h1", 1.), 1);
        assert!(marks.take_replaced().is_empty());
    }

    #[test]
    fn a_new_scale_bakes_again_and_hands_back_every_old_mark_once() {
        let (mut marks, mut made) = counting();
        mark(&mut marks, &mut made, "gold", "h1", 1.);
        mark(&mut marks, &mut made, "ruby", "h9", 1.);
        assert_eq!(mark(&mut marks, &mut made, "gold", "h1", 2.), 3);
        let mut replaced = marks.take_replaced();
        replaced.sort();
        assert_eq!(replaced, [1, 2]);
        assert!(marks.take_replaced().is_empty());
    }

    #[test]
    fn a_changed_file_under_the_same_id_replaces_the_mark() {
        let (mut marks, mut made) = counting();
        mark(&mut marks, &mut made, "gold", "h1", 1.);
        assert_eq!(mark(&mut marks, &mut made, "gold", "h2", 1.), 2);
        assert_eq!(mark(&mut marks, &mut made, "gold", "h2", 1.), 2);
        assert_eq!(marks.take_replaced(), [1]);
    }
}
