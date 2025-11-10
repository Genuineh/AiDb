//! SSTable (Sorted String Table) implementation.
//!
//! SSTable is an immutable, sorted file format for storing key-value pairs.
//! The format is designed for efficient sequential writes and random reads.
//!
//! ## File Format
//!
//! ```text
//! [Data Block 1]
//! [Data Block 2]
//! ...
//! [Data Block N]
//! [Meta Block]      // Bloom Filter (optional)
//! [Index Block]     // Index for data blocks
//! [Meta Index Block] // Index for meta blocks
//! [Footer: 48B]     // Points to index blocks
//! ```
//!
//! ## Block Format
//!
//! Each block contains:
//! - Multiple key-value entries
//! - Restart points for prefix compression
//! - Block metadata
//!
//! ## Index Format
//!
//! The index block contains entries that map keys to data blocks:
//! - Key: The largest key in the block
//! - Offset: File offset of the block
//! - Size: Size of the block in bytes

pub mod block;
pub mod builder;
pub mod footer;
pub mod index;
pub mod reader;

pub use block::{Block, BlockBuilder, BlockIterator};
pub use builder::SSTableBuilder;
pub use footer::{BlockHandle, Footer};
pub use index::IndexBlock;
pub use reader::SSTableReader;

// Re-export CompressionType from config
pub use crate::config::CompressionType;

/// Default block size (4KB)
pub const DEFAULT_BLOCK_SIZE: usize = 4096;

/// Footer size in bytes (fixed)
pub const FOOTER_SIZE: usize = 48;

/// Magic number for SSTable files
pub const MAGIC_NUMBER: u64 = 0x5441424c455f5353; // "SSTABLE_" in hex
