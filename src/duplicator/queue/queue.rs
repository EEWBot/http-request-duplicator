use std::sync::{
    atomic::{AtomicUsize, Ordering::Relaxed},
    Arc,
};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub(super) struct Shared {
    len: AtomicUsize,
}

#[derive(Clone)]
pub(super) struct QueueSender<T> {
    tx: UnboundedSender<T>,
    shared: Arc<Shared>,
}

pub(super) struct QueueReceiver<T> {
    rx: UnboundedReceiver<T>,
    shared: Arc<Shared>,
}

impl<T> QueueReceiver<T> {
    pub(super) async fn dequeue(&mut self) -> Option<T> {
        self.rx.recv().await
    }

    pub(super) fn resolve(&mut self) {
        self.shared.len.fetch_sub(1, Relaxed);
    }

    pub(super) fn is_empty(&mut self) -> bool {
        self.shared.len.load(Relaxed) == 0
    }
}

impl<T> QueueSender<T> {
    pub(super) fn enqueue(&self, t: T) {
        self.shared.len.fetch_add(1, Relaxed);
        self.tx.send(t).unwrap();
    }
}

pub fn queue<T>() -> (QueueSender<T>, QueueReceiver<T>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared { len: AtomicUsize::new(0) });

    let sender_view = QueueSender { tx: tx.clone(), shared: shared.clone() };
    let receiver_view = QueueReceiver { rx, shared: shared.clone() };

    (sender_view, receiver_view)
}
