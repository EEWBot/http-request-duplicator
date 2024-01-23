use std::sync::atomic::AtomicUsize;

pub struct LoadBalancer<T> {
    targets: Vec<T>,
    counter: AtomicUsize,
}

impl<T> LoadBalancer<T> {
    #[must_use]
    pub fn new(targets: Vec<T>) -> Self {
        if targets.len() == 0 {
            panic!("Cannot load balance 0 targets");
        }

        Self {
            targets,
            counter: AtomicUsize::new(0),
        }
    }

    #[must_use]
    pub fn fetch_next_ref(&self) -> &T {
        let n = self.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        &self.targets[n % self.targets.len()]
    }
}
