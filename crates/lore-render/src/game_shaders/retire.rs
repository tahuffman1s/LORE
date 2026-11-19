//! Things kept until the graphics card is surely done with them.

/// Things waiting to be dropped, each stamped with the frame it was put away.
///
/// Dropping a `T` is what destroys its GPU objects, so a pipeline a frame in
/// flight may still use is put here and dropped a few frames later.
pub(crate) struct Retiring<T> {
    /// How many frames an item waits.
    delay: u64,
    items: Vec<(u64, T)>,
}

impl<T> Retiring<T> {
    /// A queue whose items wait `delay` frames. `now` in the other calls
    /// counts frames submitted, not frames started: a frame can wait on its
    /// fence and then give up.
    pub(crate) fn new(delay: u64) -> Self {
        Self {
            delay,
            items: Vec::new(),
        }
    }

    /// Puts `item` away at frame `now`.
    pub(crate) fn push(&mut self, now: u64, item: T) {
        self.items.push((now, item));
    }

    /// Drops every item put away `delay` or more frames before `now`.
    pub(crate) fn release(&mut self, now: u64) {
        let delay = self.delay;
        self.items
            .retain(|(stamp, _)| stamp.saturating_add(delay) > now);
    }

    /// Drops everything: for when the device is idle.
    pub(crate) fn clear(&mut self) {
        self.items.clear();
    }

    /// How many items are waiting.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts how many of its kind have been dropped.
    struct Counted(Arc<AtomicUsize>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn an_item_is_released_after_the_delay() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut retiring = Retiring::new(3);
        retiring.push(10, Counted(dropped.clone()));
        retiring.push(11, Counted(dropped.clone()));
        retiring.release(12);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        retiring.release(13);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(retiring.len(), 1);
        retiring.release(14);
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
        assert_eq!(retiring.len(), 0);
    }

    #[test]
    fn clear_drops_everything() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut retiring = Retiring::new(3);
        retiring.push(10, Counted(dropped.clone()));
        retiring.push(10, Counted(dropped.clone()));
        retiring.clear();
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
        assert_eq!(retiring.len(), 0);
    }
}
