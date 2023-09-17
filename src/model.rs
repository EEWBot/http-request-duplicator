use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

use axum::http::{HeaderMap, Method};
use bytes::Bytes;
use serde::Serialize;
use tokio::sync::mpsc::{Sender, UnboundedSender};

#[derive(Debug, Clone, Copy)]
pub enum Priority {
    High,
    Low,
}

#[derive(Debug, Clone)]
pub struct ReadonlySharedObjectsBetweenContexts {
    pub headers: HeaderMap,
    pub method: Method,
}

#[derive(Debug)]
pub struct RequestContext {
    pub target: String,
    pub readonly_objects: Arc<RwLock<ReadonlySharedObjectsBetweenContexts>>,
    pub body: Bytes,
    pub ttl: usize,
}

#[derive(Debug)]
pub struct Channels {
    high_priority_queue: UnboundedSender<RequestContext>,
    low_priority_queue: UnboundedSender<RequestContext>,
    pub flush_low_priority_queue: Sender<()>,
}

impl Channels {
    pub fn new(
        high_priority_queue: &UnboundedSender<RequestContext>,
        low_priority_queue: &UnboundedSender<RequestContext>,
        flush_low_priority_queue: &Sender<()>,
    ) -> Self {
        Self {
            high_priority_queue: high_priority_queue.clone(),
            low_priority_queue: low_priority_queue.clone(),
            flush_low_priority_queue: flush_low_priority_queue.clone(),
        }
    }

    pub const fn get_queue<'a>(&'a self, p: Priority) -> &'a UnboundedSender<RequestContext> {
        match p {
            Priority::High => &self.high_priority_queue,
            Priority::Low => &self.low_priority_queue,
        }
    }
}

#[derive(Serialize, Debug)]
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

    pub fn read_queue_count(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }
}

#[derive(Serialize, Debug)]
pub struct Counters {
    high_priority: Counter,
    low_priority: Counter,
}

impl Counters {
    pub const fn new() -> Self {
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
    pub const fn get<'a>(&'a self, p: Priority) -> &'a Counter {
        match p {
            Priority::High => &self.high_priority,
            Priority::Low => &self.low_priority,
        }
    }
}

pub struct AppState {
    pub channels: Channels,
    pub counters: Counters,
}
