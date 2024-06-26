use std::sync::atomic::AtomicUsize;

pub(super) struct LoadBalancer<T> {
    targets: Vec<T>,
    mask: usize,
    counter: AtomicUsize,
}

impl<T> LoadBalancer<T> {
    #[must_use]
    pub(super) fn new(targets: Vec<T>) -> Self {
        if targets.len().count_ones() != 1 {
            panic!("Unsupported client count");
        }

        if targets.is_empty() {
            panic!("Cannot load balance 0 targets");
        }

        let len = targets.len();
        Self {
            targets,
            mask: !(usize::MAX << len.ilog2()),
            counter: AtomicUsize::new(0),
        }
    }

    #[must_use]
    #[inline]
    pub(super) fn fetch_next_ref(&self) -> &T {
        let n = self.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        &self.targets[n & self.mask]
    }
}
