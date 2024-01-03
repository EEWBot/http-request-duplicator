use std::collections::HashSet;

use tokio::sync::RwLock;

pub struct NegativeCache {
    cache: RwLock<HashSet<String>>,
}

impl NegativeCache {
    pub fn new() -> Self {
        NegativeCache { cache: RwLock::new(HashSet::new()) }
    }

    pub async fn ban(&self, url: &str) {
        self.cache.write().await.insert(url.to_owned());
    }

    pub async fn is_banned(&self, url: &str) -> bool {
        self.cache.read().await.contains(url)
    }

    pub async fn clear(&self) {
        self.cache.write().await.clear()
    }
}
