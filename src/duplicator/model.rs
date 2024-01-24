use std::sync::{Arc, RwLock};
use super::Priority;

#[derive(Clone)]
pub struct Payload {
    pub body: bytes::Bytes,
    pub headers: hyper::HeaderMap,
    pub method: hyper::Method,
}

pub struct Context {
    pub id: String,
    pub priority: Priority,
    pub payload: Payload,
}

impl Drop for Context {
    fn drop(&mut self) {
        tracing::info!("{} Finished", self.id);
    }
}

#[derive(Clone)]
pub struct Task {
    pub target: String,
    pub context: Arc<RwLock<Context>>,
    pub ttl: usize,
}

impl super::queue::Priorized for Task {
    fn priority(&self) -> Priority {
        self.context.read().unwrap().priority
    }
}

impl Task {
    pub fn drain(mut self) -> Option<Self> {
        self.ttl -= 1;

        if self.ttl == 0 {
            return None;
        }

        Some(self)
    }
}
