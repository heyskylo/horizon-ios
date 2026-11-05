//! Retain complete owners until polling has joined all of their producers.
pub(super) fn poll<T>(owners: &mut Vec<T>, mut stopped: impl FnMut(&mut T) -> bool) -> bool {
    owners.retain_mut(|owner| !stopped(owner));
    owners.is_empty()
}
