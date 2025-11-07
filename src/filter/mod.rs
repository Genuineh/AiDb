//! Filter implementations for efficient key existence checking.
//!
//! This module provides filter implementations to speed up SSTable lookups
//! by quickly determining if a key is definitely not present.

pub mod bloom;

pub use bloom::BloomFilter;

/// Filter trait for key existence checking
pub trait Filter {
    /// Check if a key may exist (can have false positives)
    fn may_contain(&self, key: &[u8]) -> bool;

    /// Add a key to the filter
    fn add(&mut self, key: &[u8]);

    /// Get the serialized representation of the filter
    fn encode(&self) -> Vec<u8>;

    /// Create a filter from serialized data
    fn decode(data: &[u8]) -> crate::Result<Self>
    where
        Self: Sized;
}
