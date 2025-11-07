//! Block cache implementation for SSTable data blocks.
//!
//! Provides an LRU (Least Recently Used) cache to speed up repeated reads
//! of the same data blocks from SSTables.

mod lru;

pub use lru::{BlockCache, CacheKey, CacheStats};
