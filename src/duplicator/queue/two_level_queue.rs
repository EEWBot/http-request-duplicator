use super::queue::{queue, QueueReceiver, QueueSender};

#[derive(Clone, Copy, Debug)]
pub enum Priority {
    High,
    Low
}

pub trait Priorized {
    fn priority(&self) -> Priority;
}

#[derive(Clone)]
pub struct TwoLevelQueueSender<T> {
    high_priority: QueueSender<T>,
    low_priority: QueueSender<T>,
}

pub struct TwoLevelQueueReceiver<T> {
    high_priority: QueueReceiver<T>,
    low_priority: QueueReceiver<T>,
}

impl<T> TwoLevelQueueReceiver<T> {
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

impl<T: Priorized> TwoLevelQueueSender<T> {
    pub fn enqueue(&self, t: T) {
        match t.priority() {
            Priority::High => self.high_priority.enqueue(t),
            Priority::Low => self.low_priority.enqueue(t),
        }
    }
}

pub fn two_level_queue<T>() -> (TwoLevelQueueSender<T>, TwoLevelQueueReceiver<T>) {
    let (high_tx, high_rx) = queue();
    let (low_tx, low_rx) = queue();

    (
        TwoLevelQueueSender {
            high_priority: high_tx,
            low_priority: low_tx,
        },
        TwoLevelQueueReceiver {
            high_priority: high_rx,
            low_priority: low_rx,
        },
    )
}
