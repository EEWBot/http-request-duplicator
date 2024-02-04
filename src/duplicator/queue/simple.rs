use std::sync::{
    atomic::{AtomicUsize, Ordering::Relaxed},
    Arc,
};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub(super) struct Shared {
    len: AtomicUsize,
}

#[derive(Clone)]
pub(super) struct Sender<T> {
    tx: UnboundedSender<T>,
    shared: Arc<Shared>,
}

pub(super) struct Receiver<T> {
    rx: UnboundedReceiver<T>,
    shared: Arc<Shared>,
}

impl<T> Receiver<T> {
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

impl<T> Sender<T> {
    pub(super) fn enqueue(&self, t: T) {
        self.shared.len.fetch_add(1, Relaxed);
        self.tx.send(t).unwrap();
    }
}

pub(super) fn queue<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared { len: AtomicUsize::new(0) });

    let sender_view = Sender { tx: tx.clone(), shared: shared.clone() };
    let receiver_view = Receiver { rx, shared: shared.clone() };

    (sender_view, receiver_view)
}
