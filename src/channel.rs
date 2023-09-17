use std::sync::{Arc, RwLock};

use bytes::Bytes;
use hyper::{HeaderMap, Method};
use once_cell::sync::OnceCell;
use tokio::sync::mpsc::{Sender, UnboundedSender};

use crate::model::Priority;

pub static CHANNELS: OnceCell<Channels> = OnceCell::new();

#[derive(Debug)]
pub struct Channels {
    high_priority_queue: UnboundedSender<RequestContext>,
    low_priority_queue: UnboundedSender<RequestContext>,
    pub flush_low_priority_queue: Sender<()>,
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

    pub const fn get_queue(&'static self, p: Priority) -> &'static UnboundedSender<RequestContext> {
        match p {
            Priority::High => &self.high_priority_queue,
            Priority::Low => &self.low_priority_queue,
        }
    }
}
