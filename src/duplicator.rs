use std::sync::Arc;
use std::collections::HashSet;

use tokio::sync::{Semaphore, RwLock};
use itertools::Itertools;
use rand::seq::SliceRandom;

pub struct DuplicatorInner {
    negative_cache: RwLock<HashSet<String>>,
    clients: Vec<(reqwest::Client, Arc<Semaphore>)>,
}

#[derive(Clone)]
pub struct Duplicator {
    inner: Arc<DuplicatorInner>,
}

impl Duplicator {
    pub fn new(clients: Vec<reqwest::Client>, limit: usize) -> Self {
        let clients = clients.into_iter().map(|c| (c, Arc::new(Semaphore::new(limit)))).collect();

        

        Self {
            inner: Arc::new(DuplicatorInner {
                negative_cache: RwLock::new(HashSet::new()),
                clients,
            })
        }
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
        // filter targets by negative_cache
        let mut new_targets: Vec<String> = Vec::with_capacity(targets.len());

        let cache = self.inner.negative_cache.read().await;
        new_targets.extend(targets.into_iter().filter(|url| !cache.contains(&*url)));
        drop(cache);

        let targets = new_targets;

        let split_point = targets.len() / self.inner.clients.len();

        for (chunk_nth, targets) in targets.into_iter().chunks(split_point).into_iter().enumerate() {
            let targets: Vec<String> = targets.into_iter().collect();
            let payl = payload.clone();
            let dup = self.clone();

            tokio::spawn(async move {
                let (client, semaphore) = match dup.inner.clients.get(chunk_nth) {
                    Some(clients) => clients,
                    None => dup.inner.clients.choose(&mut rand::thread_rng()).unwrap(),
                };

                for target in targets {
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let payl = payl.clone();
                    let client = client.clone();
                    let dup = dup.clone();

                    tokio::spawn(async move {
                        let mut ttl = retry;

                        loop {
                            ttl -= 1;

                            let result = client
                                .request(payl.method.clone(), &target)
                                .headers(payl.headers.clone())
                                .body(reqwest::Body::from(payl.body.clone()))
                                .version(reqwest::Version::HTTP_2)
                                .send()
                                .await;

                            let id = payl.id.to_string();

                            match result {
                                Err(e) => {
                                    tracing::warn!("{id} ConnectionError {e} when {}", target);
                                }
                                Ok(resp) => {
                                    let status = resp.status();
                                    drop(resp);

                                    if status.is_success() {
                                        drop(permit);
                                        break;
                                    }

                                    tracing::warn!("{id} {status} when {}", target);

                                    if status.as_u16() == 404 {
                                        dup.inner.negative_cache.write().await.insert(target.to_string());
                                    }

                                    if status.is_client_error() {
                                        drop(permit);
                                        break;
                                    }
                                }
                            }

                            if ttl == 0 {
                                tracing::error!("{id} Failed but ttl is 0. {}", target);
                            }
                        }
                    });
                }
            });
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
