use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use crate::model::Priority;

pub static COUNTERS: Counters = Counters::new();

#[derive(Debug)]
pub struct Counter {
    queued: AtomicUsize,
    succeed: AtomicUsize,
    failed: AtomicUsize,
}

impl Counter {
    pub fn enqueue(&self) {
        self.queued.fetch_add(1, Ordering::Relaxed);
    }

    pub fn resolve(&self) {
        self.resolve_n(1);
    }

    pub fn resolve_n(&self, n: usize) {
        self.queued.fetch_sub(n, Ordering::Relaxed);
    }

    pub fn failed(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn succeed(&self) {
        self.succeed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn is_queue_empty(&self) -> bool {
        self.queued.load(Ordering::Relaxed) == 0
    }
}

#[derive(Debug)]
pub struct Counters {
    high_priority: Counter,
    low_priority: Counter,
}

impl Counters {
    const fn new() -> Self {
        Self {
            high_priority: Counter {
                queued: AtomicUsize::new(0),
                succeed: AtomicUsize::new(0),
                failed: AtomicUsize::new(0),
            },
            low_priority: Counter {
                queued: AtomicUsize::new(0),
                succeed: AtomicUsize::new(0),
                failed: AtomicUsize::new(0),
            },
        }
    }
}

impl Counters {
    pub const fn get(&'static self, p: Priority) -> &'static Counter {
        match p {
            Priority::High => &self.high_priority,
            Priority::Low => &self.low_priority,
        }
    }
}
