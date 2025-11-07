//! Configuration options for AiDb storage engine.

/// Configuration options for opening a database.
#[derive(Debug, Clone)]
pub struct Options {
    /// Create the database if it doesn't exist.
    /// Default: true
    pub create_if_missing: bool,

    /// Error if the database already exists.
    /// Default: false
    pub error_if_exists: bool,

    /// Size threshold for flushing MemTable to SSTable (in bytes).
    /// Default: 4MB
    pub memtable_size: usize,

    /// Maximum number of Level 0 files before triggering compaction.
    /// Default: 4
    pub level0_compaction_threshold: usize,

    /// Size multiplier between levels.
    /// Default: 10 (Level N+1 is 10x larger than Level N)
    pub level_size_multiplier: usize,

    /// Base level size (Level 1 target size in bytes).
    /// Default: 10MB
    pub base_level_size: usize,

    /// Maximum number of levels.
    /// Default: 7 (Level 0 through Level 6)
    pub max_levels: usize,

    /// Block size for SSTables (in bytes).
    /// Default: 4KB
    pub block_size: usize,

    /// Block cache size (in bytes).
    /// Set to 0 to disable caching.
    /// Default: 8MB
    pub block_cache_size: usize,

    /// Enable bloom filter for SSTables.
    /// Default: true
    pub use_bloom_filter: bool,

    /// Bloom filter false positive rate.
    /// Default: 0.01 (1%)
    pub bloom_filter_fp_rate: f64,

    /// Compression algorithm for SSTables.
    /// Default: CompressionType::Snappy
    pub compression: CompressionType,

    /// Enable write-ahead log (WAL).
    /// Disabling reduces durability but increases performance.
    /// Default: true
    pub use_wal: bool,

    /// Sync WAL writes to disk.
    /// Default: true
    pub sync_wal: bool,

    /// Number of background compaction threads.
    /// Default: 1
    pub compaction_threads: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            create_if_missing: true,
            error_if_exists: false,
            memtable_size: 4 * 1024 * 1024, // 4MB
            level0_compaction_threshold: 4,
            level_size_multiplier: 10,
            base_level_size: 10 * 1024 * 1024, // 10MB
            max_levels: 7,
            block_size: 4 * 1024,              // 4KB
            block_cache_size: 8 * 1024 * 1024, // 8MB
            use_bloom_filter: true,
            bloom_filter_fp_rate: 0.01,
            compression: CompressionType::Snappy,
            use_wal: true,
            sync_wal: true,
            compaction_threads: 1,
        }
    }
}

/// Compression algorithms supported by AiDb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionType {
    /// No compression.
    None = 0,

    /// Snappy compression (fast, moderate compression ratio).
    #[cfg(feature = "snappy")]
    Snappy = 1,

    /// LZ4 compression (very fast, lower compression ratio).
    #[cfg(feature = "lz4-compression")]
    Lz4 = 2,
}

impl CompressionType {
    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(CompressionType::None),
            #[cfg(feature = "snappy")]
            1 => Some(CompressionType::Snappy),
            #[cfg(feature = "lz4-compression")]
            2 => Some(CompressionType::Lz4),
            _ => None,
        }
    }
}

impl Default for CompressionType {
    fn default() -> Self {
        #[cfg(feature = "snappy")]
        return CompressionType::Snappy;

        #[cfg(not(feature = "snappy"))]
        CompressionType::None
    }
}

impl Options {
    /// Creates a new Options with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether to create the database if it doesn't exist.
    pub fn create_if_missing(mut self, value: bool) -> Self {
        self.create_if_missing = value;
        self
    }

    /// Sets the MemTable size threshold.
    pub fn memtable_size(mut self, size: usize) -> Self {
        self.memtable_size = size;
        self
    }

    /// Sets the block size for SSTables.
    pub fn block_size(mut self, size: usize) -> Self {
        self.block_size = size;
        self
    }

    /// Sets the block cache size.
    pub fn block_cache_size(mut self, size: usize) -> Self {
        self.block_cache_size = size;
        self
    }

    /// Sets the compression algorithm.
    pub fn compression(mut self, compression: CompressionType) -> Self {
        self.compression = compression;
        self
    }

    /// Enables or disables the write-ahead log.
    pub fn use_wal(mut self, value: bool) -> Self {
        self.use_wal = value;
        self
    }

    /// Validates the options and returns an error if any are invalid.
    pub fn validate(&self) -> crate::Result<()> {
        if self.memtable_size == 0 {
            return Err(crate::Error::invalid_argument("memtable_size must be > 0"));
        }
        if self.block_size == 0 {
            return Err(crate::Error::invalid_argument("block_size must be > 0"));
        }
        if self.max_levels == 0 {
            return Err(crate::Error::invalid_argument("max_levels must be > 0"));
        }
        if self.bloom_filter_fp_rate <= 0.0 || self.bloom_filter_fp_rate >= 1.0 {
            return Err(crate::Error::invalid_argument(
                "bloom_filter_fp_rate must be between 0 and 1",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = Options::default();
        assert!(opts.create_if_missing);
        assert!(!opts.error_if_exists);
        assert_eq!(opts.memtable_size, 4 * 1024 * 1024);
    }

    #[test]
    fn test_options_builder() {
        let opts = Options::new()
            .memtable_size(8 * 1024 * 1024)
            .block_size(8 * 1024)
            .use_wal(false);

        assert_eq!(opts.memtable_size, 8 * 1024 * 1024);
        assert_eq!(opts.block_size, 8 * 1024);
        assert!(!opts.use_wal);
    }

    #[test]
    fn test_options_validation() {
        let mut opts = Options::default();
        assert!(opts.validate().is_ok());

        opts.memtable_size = 0;
        assert!(opts.validate().is_err());

        opts.memtable_size = 1024;
        opts.bloom_filter_fp_rate = 1.5;
        assert!(opts.validate().is_err());
    }
}
