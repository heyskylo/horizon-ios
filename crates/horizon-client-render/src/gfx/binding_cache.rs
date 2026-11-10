//! A bind group's validity depends on the identity of its buffers and the
//! bound slice, not on the bytes written into those buffers.

pub(super) struct BindingKey<B, const N: usize> {
    pub first: u64,
    pub count: u64,
    pub buffers: [B; N],
}

impl<B: PartialEq, const N: usize> BindingKey<B, N> {
    pub fn matches(&self, first: u64, count: u64, buffers: [&B; N]) -> bool {
        self.first == first
            && self.count == count
            && self
                .buffers
                .iter()
                .zip(buffers)
                .all(|(old, new)| old == new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_to_any_resource_or_range_invalidate_the_binding() {
        let key = BindingKey {
            first: 16,
            count: 8,
            buffers: [1u64, 2, 3, 4],
        };
        assert!(key.matches(16, 8, [&1, &2, &3, &4]));
        assert!(!key.matches(0, 8, [&1, &2, &3, &4]));
        assert!(!key.matches(16, 9, [&1, &2, &3, &4]));
        for i in 0..4 {
            let mut resources = [1u64, 2, 3, 4];
            resources[i] += 100;
            assert!(!key.matches(16, 8, resources.each_ref()));
        }
    }
}
