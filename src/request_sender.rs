use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Semaphore;
use tokio::time::sleep;

use crate::{
    model::{self, Priority},
    negative_cache::NegativeCache,
};

struct RequestSenderInner {
    clients: Vec<reqwest::Client>,
    client_selection: AtomicUsize,
    limiter: Arc<Semaphore>,
    state: Arc<model::AppState>,
    retry_delay: u64,
    negcache_404: NegativeCache,
}

#[derive(Clone)]
pub struct RequestSender {
    inner: Arc<RequestSenderInner>,
}

enum RequestError {
    ConnectionError(reqwest::Error),
    HttpError(reqwest::StatusCode),
}

impl RequestSender {
    pub fn new(
        state: Arc<model::AppState>,
        clients: Vec<reqwest::Client>,
        parallels: usize,
        retry_delay: u64,
    ) -> Self {
        if clients.len() == 0 {
            panic!();
        }

        Self {
            inner: Arc::new(RequestSenderInner {
                state,
                client_selection: AtomicUsize::new(0),
                clients,
                limiter: Arc::new(Semaphore::new(parallels)),
                retry_delay,
                negcache_404: NegativeCache::new(),
            }),
        }
    }

    async fn request(
        &self,
        ctx: &model::RequestContext,
    ) -> Result<reqwest::StatusCode, RequestError> {
        // delayed clone
        let shared = ctx.readonly_objects.read().unwrap().clone();

        use std::sync::atomic::Ordering;
        let n = self.inner.client_selection.fetch_add(1, Ordering::Relaxed);
        let n = n % self.inner.clients.len();

        match self.inner.clients[n]
            .request(shared.method, &ctx.target)
            .headers(shared.headers)
            .body(reqwest::Body::from(ctx.body.clone()))
            .send()
            .await
        {
            Err(e) => Err(RequestError::ConnectionError(e)),
            Ok(resp) => {
                let status = resp.status();
                self.inner
                    .state
                    .log
                    .append(resp.status().into(), &ctx.target)
                    .await;
                if status.is_success() {
                    Ok(status)
                } else {
                    Err(RequestError::HttpError(status))
                }
            }
        }
    }

    async fn retry(&self, mut ctx: model::RequestContext, p: Priority) {
        ctx.ttl -= 1;

        if ctx.ttl != 0 {
            tracing::warn!("retring {}...", ctx.target);
            sleep(Duration::from_secs(self.inner.retry_delay)).await;
            self.inner.state.counters.get(p).enqueue();
            self.inner.state.channels.get_queue(p).send(ctx).unwrap();
        } else {
            self.inner.state.counters.get(p).failed();
        }
    }

    async fn process_request(&self, ctx: model::RequestContext, p: Priority) {
        if self.inner.negcache_404.is_banned(&ctx.target).await {
            return;
        }

        self.inner.state.counters.get(p).resolve();
        let permit = self.inner.limiter.clone().acquire_owned().await;
        let cloned_self = self.clone();

        tokio::spawn(async move {
            match cloned_self.request(&ctx).await {
                Ok(status) => {
                    tracing::debug!("{status} when {}", ctx.target);
                    cloned_self.inner.state.counters.get(p).succeed();
                }
                Err(RequestError::HttpError(status)) => {
                    tracing::warn!("{status} when {}", ctx.target);

                    if status == hyper::StatusCode::NOT_FOUND {
                        cloned_self.inner.negcache_404.ban(&ctx.target).await;
                    }

                    if status.is_client_error() {
                        cloned_self.inner.state.counters.get(p).failed();
                    } else {
                        cloned_self.retry(ctx, p).await;
                    }
                }
                Err(RequestError::ConnectionError(e)) => {
                    tracing::warn!("{e} when {}", ctx.target);
                    cloned_self.retry(ctx, p).await;
                }
            }

            drop(permit);
        });
    }

    pub async fn event_loop(
        &self,
        mut high_priority_rx: UnboundedReceiver<model::RequestContext>,
        mut low_priority_rx: UnboundedReceiver<model::RequestContext>,
    ) {
        loop {
            tokio::select! {
                Some(ctx) = high_priority_rx.recv() => {
                    self.process_request(ctx, Priority::High).await;
                }
                Some(ctx) = low_priority_rx.recv(), if self.inner.state.counters.get(Priority::High).is_queue_empty() => {
                    self.process_request(ctx, Priority::Low).await;
                }
            }
        }
    }
}
