use std::sync::{Arc, RwLock};

use bytes::Bytes;
use hyper::{HeaderMap, Method};
use once_cell::sync::OnceCell;
use tokio::sync::mpsc::{Sender, UnboundedSender};

use crate::model::Priority;

pub static CHANNELS: OnceCell<Channels> = OnceCell::new();

#[derive(Debug)]
pub struct Channels {
    pub high_priority_sock: UnboundedSender<RequestContext>,
    pub low_priority_sock: UnboundedSender<RequestContext>,
    pub drop_low_priority_requests: Sender<()>,
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
    pub const fn get_req(&'static self, p: Priority) -> &'static UnboundedSender<RequestContext> {
        match p {
            Priority::High => &self.high_priority_sock,
            Priority::Low => &self.low_priority_sock,
        }
    }
}
