use std::sync::atomic::AtomicUsize;
use crate::model::Priority;

pub static COUNTERS: Counters = Counters::new();

#[derive(Debug)]
pub struct Counter {
    pub queued: AtomicUsize,
    pub succeed: AtomicUsize,
    pub dropped: AtomicUsize,
    pub failed: AtomicUsize,
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
                dropped: AtomicUsize::new(0),
                failed: AtomicUsize::new(0),
            },
            low_priority: Counter {
                queued: AtomicUsize::new(0),
                succeed: AtomicUsize::new(0),
                dropped: AtomicUsize::new(0),
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
