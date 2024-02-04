use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::RwLock;

struct Inner {
    cache: RwLock<HashSet<String>>,
}

#[derive(Clone)]
pub struct NegativeCache {
    inner: Arc<Inner>,
}

impl NegativeCache {
    pub(super) fn new() -> Self {
        NegativeCache {
            inner: Arc::new(Inner {
                cache: RwLock::new(HashSet::new()),
            }),
        }
    }

    pub(super) async fn ban(&self, url: &str) {
        self.inner.cache.write().await.insert(url.to_owned());
    }

    pub(super) async fn is_banned(&self, url: &str) -> bool {
        self.inner.cache.read().await.contains(url)
    }

    pub async fn list(&self) -> Vec<String> {
        Vec::from_iter(self.inner.cache.read().await.iter().map(|s| s.to_owned()))
    }

    pub async fn delete(&self, url: &str) {
        self.inner.cache.write().await.remove(url);
    }
}
