use super::model::{Priority, Task};
use super::queue::{queue, QueueReceiver, QueueSender};

#[derive(Clone)]
pub(super) struct TwoLevelQueueSender {
    high_priority: QueueSender<Task>,
    low_priority: QueueSender<Task>,
}

pub(super) struct TwoLevelQueueReceiver {
    high_priority: QueueReceiver<Task>,
    low_priority: QueueReceiver<Task>,
}

impl TwoLevelQueueReceiver {
    pub(super) async fn fetch(&mut self) -> Task {
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

impl TwoLevelQueueSender {
    pub(super) fn enqueue(&self, t: Task) {
        let p = t.context.read().unwrap().priority;
        match p {
            Priority::High => self.high_priority.enqueue(t),
            Priority::Low => self.low_priority.enqueue(t),
        }
    }
}

pub fn two_level_queue() -> (TwoLevelQueueSender, TwoLevelQueueReceiver) {
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
