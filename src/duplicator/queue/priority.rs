use super::simple::{queue, Receiver, Sender};

#[derive(Clone, Copy, Debug)]
pub enum Priority {
    High,
    Low
}

pub trait Priorized {
    fn priority(&self) -> Priority;
}

#[derive(Clone)]
pub struct PriorizedSender<T> {
    high_priority: Sender<T>,
    low_priority: Sender<T>,
}

pub struct PriorizedReceiver<T> {
    high_priority: Receiver<T>,
    low_priority: Receiver<T>,
}

impl<T> PriorizedReceiver<T> {
    pub async fn fetch(&mut self) -> T {
        let idle = self.high_priority.is_empty();

        tokio::select! {
            Some(task) = self.high_priority.dequeue() => {
                self.high_priority.resolve();
                task
            },
            Some(task) = self.low_priority.dequeue(), if idle => {
                self.low_priority.resolve();
                task
            },
        }
    }
}

impl<T: Priorized> PriorizedSender<T> {
    pub fn enqueue(&self, t: T) {
        match t.priority() {
            Priority::High => self.high_priority.enqueue(t),
            Priority::Low => self.low_priority.enqueue(t),
        }
    }
}

pub fn priority_queue<T>() -> (PriorizedSender<T>, PriorizedReceiver<T>) {
    let (high_tx, high_rx) = queue();
    let (low_tx, low_rx) = queue();

    (
        PriorizedSender {
            high_priority: high_tx,
            low_priority: low_tx,
        },
        PriorizedReceiver {
            high_priority: high_rx,
            low_priority: low_rx,
        },
    )
}
