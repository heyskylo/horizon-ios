use std::collections::HashMap;

use crate::v1::{Side, StringEntry};

/// Interns strings to ids from 1 (0 means "none" in `CounterDef.unit`) and hands out the new entries to append.
#[derive(Default)]
pub struct Interner {
    ids: HashMap<String, u32>,
    pending: Vec<(u32, String)>,
}

impl Interner {
    pub fn intern(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        let id = self.ids.len() as u32 + 1;
        self.ids.insert(text.to_owned(), id);
        self.pending.push((id, text.to_owned()));
        id
    }

    /// The entries interned since the last call, in id order; a live capture appends them before their first use.
    pub fn take_new(&mut self, side: Side) -> Vec<StringEntry> {
        self.pending
            .drain(..)
            .map(|(id, text)| StringEntry {
                side: side as i32,
                id,
                text,
            })
            .collect()
    }
}
