//! The GUI atlas's fixed slots for runtime images (`horizon/skin{N}`,
//! `horizon/map{N}`): which image each slot holds, least recently shown
//! first. A new atlas (a resource pack reload) has every slot blank, so the
//! table is forgotten with it and each image uploads again.

pub(super) struct AtlasSlots<K> {
    count: usize,
    /// (image, slot, the frame it was last shown in), most recently shown last.
    held: Vec<(K, usize, u64)>,
    /// The frame being drawn (`next_frame`).
    frame: u64,
}

impl<K: PartialEq> AtlasSlots<K> {
    pub(super) fn new(count: usize) -> Self {
        Self {
            count,
            held: Vec::new(),
            frame: 0,
        }
    }
    /// A new frame starts: what was shown before it may give up its slot.
    pub(super) fn next_frame(&mut self) {
        self.frame += 1;
    }
    /// The slot holding `key`, marking it recently shown.
    pub(super) fn show<Q: PartialEq + ?Sized>(&mut self, key: &Q) -> Option<usize>
    where
        K: std::borrow::Borrow<Q>,
    {
        let i = self.held.iter().position(|e| e.0.borrow() == key)?;
        let mut e = self.held.remove(i);
        e.2 = self.frame;
        let slot = e.1;
        self.held.push(e);
        Some(slot)
    }
    /// A slot for `key`: a free one, else the least recently shown, returned
    /// with the image it held. `None` when every slot shows something this
    /// frame: taking one would put this image on what was already drawn.
    pub(super) fn claim(&mut self, key: K) -> Option<(usize, Option<K>)> {
        let (slot, evicted) = if self.held.len() < self.count {
            (self.held.len(), None)
        } else if self.held[0].2 != self.frame {
            let (old, slot, _) = self.held.remove(0);
            (slot, Some(old))
        } else {
            return None;
        };
        self.held.push((key, slot, self.frame));
        Some((slot, evicted))
    }
    /// The atlas was replaced: every slot is blank.
    pub(super) fn forget(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_shown_image_gives_up_its_slot() {
        let mut slots = AtlasSlots::new(2);
        assert_eq!(slots.claim("a"), Some((0, None)));
        assert_eq!(slots.claim("b"), Some((1, None)));
        slots.next_frame();
        assert_eq!(slots.show(&"a"), Some(0));
        assert_eq!(slots.claim("c"), Some((1, Some("b"))));
        assert_eq!(slots.show(&"b"), None);
    }

    #[test]
    fn an_image_shown_this_frame_keeps_its_slot() {
        // More maps in view than slots: the ones past the slots go undrawn
        // rather than taking a slot another map was drawn with, both showing
        // the later picture.
        let mut slots = AtlasSlots::new(2);
        assert_eq!(slots.claim("a"), Some((0, None)));
        assert_eq!(slots.claim("b"), Some((1, None)));
        assert_eq!(slots.claim("c"), None);
        slots.next_frame();
        assert_eq!(slots.show(&"b"), Some(1));
        assert_eq!(slots.claim("c"), Some((0, Some("a"))));
    }

    #[test]
    fn a_new_atlas_shows_no_stored_skin() {
        // #3385: a skin stored before a pack reload must upload again, not
        // name a sprite the new atlas does not have.
        let mut slots = AtlasSlots::new(4);
        slots.claim("skins/abc".to_string());
        assert_eq!(slots.show("skins/abc"), Some(0));
        slots.forget();
        assert_eq!(slots.show("skins/abc"), None);
        assert_eq!(slots.claim("skins/abc".to_string()), Some((0, None)));
    }
}
