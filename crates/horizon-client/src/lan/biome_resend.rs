//! The `ChunksBiomes` bodies `resend_biomes` sends: the columns a guest has been
//! sent, encoded once for every set of columns the guests share.

use crate::fast_hash::FxHashSet;
use horizon_protocol::packets::play::{self, Clientbound as C};

/// The body each receiver is sent: its columns of `data` that are in `sent`, in
/// `data`'s order; `None` when it has none of them. Receivers with the same
/// columns get the same bytes, encoded once.
pub(super) fn bodies<'a>(
    sent: impl Iterator<Item = &'a FxHashSet<(i32, i32)>>,
    data: &[((i32, i32), play::ChunkBiomeData)],
) -> Vec<Option<Vec<u8>>> {
    let mut encoded: Vec<(Vec<usize>, Vec<u8>)> = Vec::new();
    sent.map(|sent| {
        let picks: Vec<usize> = data
            .iter()
            .enumerate()
            .filter(|(_, (key, _))| sent.contains(key))
            .map(|(i, _)| i)
            .collect();
        if picks.is_empty() {
            return None;
        }
        let at = match encoded.iter().position(|(p, _)| *p == picks) {
            Some(at) => at,
            None => {
                let chunks: Vec<_> = picks.iter().map(|&i| data[i].1.clone()).collect();
                encoded.push((
                    picks,
                    C::ChunksBiomes(play::ChunksBiomes { chunks }).encode(),
                ));
                encoded.len() - 1
            }
        };
        Some(encoded[at].1.clone())
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packet each receiver got before: its columns cloned and encoded for it alone.
    fn per_receiver(
        sent: &FxHashSet<(i32, i32)>,
        data: &[((i32, i32), play::ChunkBiomeData)],
    ) -> Option<Vec<u8>> {
        let chunks: Vec<_> = data
            .iter()
            .filter(|(key, _)| sent.contains(key))
            .map(|(_, d)| d.clone())
            .collect();
        (!chunks.is_empty()).then(|| C::ChunksBiomes(play::ChunksBiomes { chunks }).encode())
    }

    #[test]
    fn shared_bodies_are_the_bytes_each_receiver_got_before() {
        let column = |x: i32, z: i32| {
            (
                (x, z),
                play::ChunkBiomeData {
                    pos: [x, z],
                    buffer: vec![x as u8, z as u8, 7, 0, 9],
                },
            )
        };
        // Data is not in the order the sets list the columns: the packet keeps data's order.
        let data = vec![
            column(2, 0),
            column(0, 0),
            column(1, 0),
            column(0, 1),
            column(1, 1),
            column(3, 3),
        ];
        let all: FxHashSet<(i32, i32)> = [(0, 0), (1, 0), (0, 1), (1, 1), (2, 0)]
            .into_iter()
            .collect();
        let some: FxHashSet<(i32, i32)> = [(1, 0), (3, 3), (0, 1)].into_iter().collect();
        let none: FxHashSet<(i32, i32)> = [(9, 9)].into_iter().collect();
        // Three receivers share the five columns.
        let sets = [&all, &all, &some, &none, &all];
        let got = bodies(sets.iter().copied(), &data);
        let want: Vec<_> = sets.iter().map(|s| per_receiver(s, &data)).collect();
        assert_eq!(got, want);
        assert!(got[3].is_none());
        assert_eq!(got[0], got[1]);
        assert_eq!(got[0], got[4]);
    }
}
