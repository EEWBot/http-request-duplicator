use std::sync::Arc;
use std::collections::HashSet;

use async_channel::{unbounded, Sender, Receiver};
use tokio::sync::RwLock;

struct Context {
    target: String,
    payload: Arc<Payload>,
    ttl: usize,
}

pub struct DuplicatorInner {
    negative_cache: RwLock<HashSet<String>>,
}

async fn process_thread(client: reqwest::Client, tx: Sender<Context>, rx: Receiver<Context>, dup: Arc<Duplicator>) {
    loop {
        let mut ctx: Context = rx.recv().await.unwrap();
        ctx.ttl -= 1;

        let result = client
            .request(ctx.payload.method.clone(), &ctx.target)
            .headers(ctx.payload.headers.clone())
            .body(reqwest::Body::from(ctx.payload.body.clone()))
            .version(reqwest::Version::HTTP_2)
            .send()
            .await;

        let id = &ctx.payload.id;

        let mut need_to_resend = false;

        match result {
            Err(e) => {
                tracing::warn!("{id} ConnectionError {e} when {}", &ctx.target);
                need_to_resend = true;
            }
            Ok(resp) => {
                let status = resp.status();

                if !status.is_success() {
                    tracing::warn!("{id} {status} when {}", ctx.target);
                }

                if status.as_u16() == 404 {
                    dup.inner.negative_cache.write().await.insert(ctx.target.to_string());
                }

                if status.is_server_error() {
                    need_to_resend = true;
                }
            }
        }

        if need_to_resend {
            if ctx.ttl == 0 {
                tracing::error!("{id} Failed but ttl is 0. {}", ctx.target);
            } else {
                tx.send(ctx).await.unwrap();
            }
        }
    }
}

#[derive(Clone)]
pub struct Duplicator {
    inner: Arc<DuplicatorInner>,
    sender: Sender<Context>,
}

impl Duplicator {
    pub fn new(clients: Vec<reqwest::Client>, limit: usize) -> Self {
        let (sender, receiver) = unbounded();

        let dup = Self {
            inner: Arc::new(DuplicatorInner {
                negative_cache: RwLock::new(HashSet::new()),
            }),
            sender: sender.clone(),
        };

        for client in clients.into_iter() {
            for _ in 0..limit {
                let tx = sender.clone();
                let rx = receiver.clone();
                let client = client.clone();
                let dup = Arc::new(dup.clone());

                tokio::spawn(async move {
                    process_thread(client, tx, rx, dup).await
                });
            }
        }

        dup
    }

    pub async fn banned(&self) -> Vec<String> {
        Vec::from_iter(self.inner.negative_cache.read().await.iter().map(|s| s.to_string()))
    }

    pub async fn purge_banned_urls<'a>(&self, urls: Vec<String>) {
        let mut cache = self.inner.negative_cache.write().await;

        for url in urls {
            cache.remove(&url);
        }
    }

    pub async fn duplicate(&self, payload: Arc<Payload>, targets: Vec<String>, retry: usize) {
        let cache = self.inner.negative_cache.read().await;
        for target in targets.into_iter().filter(|url| !cache.contains(&*url)) {
            self.sender.send(Context {
                payload: payload.clone(),
                ttl: retry,
                target,
            }).await.unwrap();
        }
    }
}

#[derive(Clone)]
pub struct Payload {
    pub body: bytes::Bytes,
    pub headers: hyper::HeaderMap,
    pub method: hyper::Method,
    pub id: String,
}

impl Drop for Payload {
    fn drop(&mut self) {
        tracing::info!("{} Finished", self.id);
    }
}
