use std::sync::Arc;

use tokio::sync::Semaphore;

use super::processor::{Enqueuer, InnerRunner, Runner};
use super::load_balancer::LoadBalancer;
use super::negative_cache::NegativeCache;
use super::model::Task;
use super::queue::priority_queue;

pub struct Builder {
    clients: Vec<reqwest::Client>,
    global_limit: usize,
    ttl: usize,
    retry_after: std::time::Duration,
}

impl Builder {
    #[must_use]
    pub fn new(clients: Vec<reqwest::Client>) -> Self {
        Self {
            global_limit: usize::MAX,
            clients,
            retry_after: std::time::Duration::from_secs(3),
            ttl: 3,
        }
    }

    #[must_use]
    pub fn retry_after(mut self, duration: std::time::Duration) -> Self {
        self.retry_after = duration;
        self
    }

    #[must_use]
    pub fn global_limit(mut self, count: usize) -> Self {
        self.global_limit = count;
        self
    }

    #[must_use]
    pub fn ttl(mut self, count: usize) -> Self {
        self.ttl = count;
        self
    }

    #[must_use]
    pub fn build(self) -> (Enqueuer, NegativeCache, Runner) {
        let (task_tx, task_rx) = priority_queue::<Task>();

        let enqueuer = Enqueuer::new(task_tx, self.ttl);
        let global_limit = Arc::new(Semaphore::new(self.global_limit));

        let negative_cache = NegativeCache::new();

        let runner = Runner::new(
            Arc::new(InnerRunner::new(
                LoadBalancer::new(self.clients),
                negative_cache.clone(),
                enqueuer.clone(),
                self.retry_after,
            )),
            global_limit,
            task_rx,
        );

        (enqueuer, negative_cache, runner)
    }
}
