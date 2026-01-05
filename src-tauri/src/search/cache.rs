//! Search result caching

use super::SearchResult;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// LRU cache for search results
pub struct SearchCache {
    cache: RwLock<HashMap<String, CacheEntry>>,
    max_size: usize,
    ttl: Duration,
}

struct CacheEntry {
    results: Vec<SearchResult>,
    created_at: Instant,
    hits: u32,
}

impl SearchCache {
    /// Create a new cache with specified max size and TTL
    pub fn new(max_size: usize, ttl_seconds: u64) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_size,
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    /// Get cached results for a query
    pub fn get(&self, query: &str) -> Option<Vec<SearchResult>> {
        let mut cache = self.cache.write().ok()?;

        if let Some(entry) = cache.get_mut(query) {
            // Check if entry is still valid
            if entry.created_at.elapsed() < self.ttl {
                entry.hits += 1;
                return Some(entry.results.clone());
            } else {
                // Entry expired, remove it
                cache.remove(query);
            }
        }

        None
    }

    /// Store results in cache
    pub fn set(&self, query: &str, results: Vec<SearchResult>) {
        let mut cache = match self.cache.write() {
            Ok(c) => c,
            Err(_) => return,
        };

        // Evict old entries if cache is full
        if cache.len() >= self.max_size {
            self.evict(&mut cache);
        }

        cache.insert(
            query.to_string(),
            CacheEntry {
                results,
                created_at: Instant::now(),
                hits: 0,
            },
        );
    }

    /// Evict least recently used entries
    fn evict(&self, cache: &mut HashMap<String, CacheEntry>) {
        // Remove expired entries first
        let now = Instant::now();
        cache.retain(|_, entry| now.duration_since(entry.created_at) < self.ttl);

        // If still too large, remove least hit entries
        if cache.len() >= self.max_size {
            // Find entry with lowest hit count
            if let Some(key) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.hits)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&key);
            }
        }
    }

    /// Clear all cached entries
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    /// Invalidate cache (call after database changes)
    pub fn invalidate(&self) {
        self.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let cache = match self.cache.read() {
            Ok(c) => c,
            Err(_) => return CacheStats::default(),
        };

        let total_hits: u32 = cache.values().map(|e| e.hits).sum();

        CacheStats {
            entries: cache.len(),
            total_hits,
        }
    }
}

#[derive(Debug, Default)]
pub struct CacheStats {
    pub entries: usize,
    pub total_hits: u32,
}

impl Default for SearchCache {
    fn default() -> Self {
        Self::new(1000, 300) // 1000 entries, 5 minutes TTL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_set_get() {
        let cache = SearchCache::new(10, 60);

        let results = vec![SearchResult {
            id: 1,
            path: "/test/file.txt".to_string(),
            name: "file.txt".to_string(),
            extension: Some("txt".to_string()),
            size: 100,
            modified_at: Some(1234567890),
            rank: 1.0,
            snippet: None,
        }];

        cache.set("test query", results.clone());

        let cached = cache.get("test query");
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }

    #[test]
    fn test_cache_miss() {
        let cache = SearchCache::new(10, 60);
        assert!(cache.get("nonexistent").is_none());
    }
}
