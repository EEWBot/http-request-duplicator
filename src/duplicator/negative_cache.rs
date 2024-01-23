use std::collections::HashSet;

use tokio::sync::RwLock;

pub(super) struct NegativeCache {
    cache: RwLock<HashSet<String>>,
}

impl NegativeCache {
    pub(super) fn new() -> Self {
        NegativeCache { cache: RwLock::new(HashSet::new()) }
    }

    pub(super) async fn ban(&self, url: &str) {
        self.cache.write().await.insert(url.to_owned());
    }

    pub(super) async fn is_banned(&self, url: &str) -> bool {
        self.cache.read().await.contains(url)
    }
}
