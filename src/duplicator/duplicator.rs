use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::Semaphore;

use super::load_balancer::LoadBalancer;
use super::model::{Context, Payload, Task};
use super::negative_cache::NegativeCache;
use super::queue::{Priority, TwoLevelQueueSender, TwoLevelQueueReceiver};

#[derive(Clone)]
pub struct Enqueuer {
    task_tx: TwoLevelQueueSender<Task>,
    ttl: usize,
}

impl Enqueuer {
    pub(super) fn new(task_tx: TwoLevelQueueSender<Task>, ttl: usize) -> Self {
        Self { task_tx, ttl }
    }

    pub fn enqueue<T>(&self, payload: Payload, priority: Priority, targets: &[T]) -> String
    where
        T: AsRef<str>,
    {
        let id = ulid::Ulid::new().to_string();

        let context = Arc::new(RwLock::new(Context {
            id: id.clone(),
            priority,
            payload,
        }));

        for target in targets {
            let task = Task {
                target: target.as_ref().to_string(),
                context: context.clone(),
                ttl: self.ttl,
            };

            self.task_tx.enqueue(task);
        }

        tracing::info!("{id} Enqueued {priority:?}/{}", targets.len());

        id
    }

    fn requeue(&self, task: Task) {
        self.task_tx.enqueue(task);
    }
}

pub struct InnerRunner {
    lb_clients: LoadBalancer<reqwest::Client>,
    cache404: NegativeCache,
    enqueuer: Enqueuer,
    retry_after: Duration,
}

impl InnerRunner {
    pub(super) fn new(
        lb_clients: LoadBalancer<reqwest::Client>,
        cache404: NegativeCache,
        enqueuer: Enqueuer,
        retry_after: Duration,
    ) -> Self {
        Self {
            lb_clients,
            cache404,
            enqueuer,
            retry_after,
        }
    }
}

pub struct Runner {
    inner: Arc<InnerRunner>,
    global_limit: Arc<Semaphore>,
    task_rx: TwoLevelQueueReceiver<Task>,
}

impl Runner {
    pub(super) fn new(
        inner: Arc<InnerRunner>,
        global_limit: Arc<Semaphore>,
        task_rx: TwoLevelQueueReceiver<Task>,
    ) -> Self {
        Self {
            inner,
            global_limit,
            task_rx,
        }
    }

    pub async fn event_loop(mut self) -> ! {
        loop {
            let task = self.task_rx.fetch().await;

            if self.inner.cache404.is_banned(&task.target).await {
                let id = task.context.read().unwrap().id.to_string();
                tracing::warn!("{id} Ignored {}", &task.target);
                continue;
            }

            let permit = self.global_limit.clone().acquire_owned().await;

            let inner = self.inner.clone();
            tokio::spawn(async move {
                let permit = permit;

                let client = inner.lb_clients.fetch_next_ref();
                let payl = task.context.read().unwrap().payload.clone();

                let mut requeue = false;

                let result = client
                    .request(payl.method, &task.target)
                    .headers(payl.headers)
                    .body(reqwest::Body::from(payl.body))
                    .send()
                    .await;

                let id = task.context.read().unwrap().id.to_string();
                match result {
                    Err(e) => {
                        tracing::error!("{id} ConnectionError {e} when {}", task.target);
                        requeue = true;
                    }
                    Ok(resp) => {
                        let status = resp.status();

                        if !status.is_success() {
                            tracing::error!("{id} {status} when {}", task.target);
                        }

                        if status.as_u16() == 404 {
                            inner.cache404.ban(&task.target).await;
                        }

                        if status.is_server_error() {
                            requeue = true;
                        }
                    }
                }

                drop(permit);

                if !requeue {
                    return;
                }

                if let Some(task) = task.drain() {
                    tokio::time::sleep(inner.retry_after).await;
                    inner.enqueuer.requeue(task);
                }
            });
        }
    }
}
