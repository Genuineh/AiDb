# AiDb

🚀 **A high-performance LSM-Tree based key-value storage engine written in Rust**

[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

## 📖 Overview

AiDb is a persistent key-value storage engine inspired by [RocksDB](https://github.com/facebook/rocksdb) and [LevelDB](https://github.com/google/leveldb). It implements the Log-Structured Merge-Tree (LSM-Tree) architecture, providing:

- ⚡ **High write throughput** via sequential writes
- 🔍 **Efficient range queries** with sorted data
- 💾 **Persistent storage** with crash recovery
- 🔄 **Background compaction** for space optimization
- 📊 **MVCC snapshots** for consistent reads

## 🎯 Project Status

**Status**: 🚧 Under Active Development

This project is currently in the early development phase. See [TODO.md](TODO.md) for the current task list and [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) for the detailed roadmap.

## 🏗️ Architecture

AiDb follows the classic LSM-Tree architecture:

```
┌─────────────────────────────────────────────┐
│              Write Path                      │
├─────────────────────────────────────────────┤
│  Client Write → WAL → MemTable              │
│                      ↓                       │
│              Immutable MemTable              │
│                      ↓                       │
│                  Flush                       │
│                      ↓                       │
│              SSTable (Level 0)               │
└─────────────────────────────────────────────┘

┌─────────────────────────────────────────────┐
│              Read Path                       │
├─────────────────────────────────────────────┤
│  Client Read → MemTable                     │
│             → Immutable MemTables           │
│             → Block Cache                   │
│             → SSTable (Level 0 → Level N)   │
└─────────────────────────────────────────────┘

┌─────────────────────────────────────────────┐
│            Background Tasks                  │
├─────────────────────────────────────────────┤
│  • Flush: MemTable → SSTable                │
│  • Compaction: Merge SSTables               │
│  • Garbage Collection                        │
└─────────────────────────────────────────────┘
```

### Core Components

- **WAL (Write-Ahead Log)**: Ensures durability by logging writes before applying them
- **MemTable**: In-memory sorted structure (Skip List) for recent writes
- **SSTable**: Immutable on-disk sorted files organized in levels
- **Compaction**: Background process to merge and reorganize SSTables
- **Bloom Filter**: Probabilistic data structure to speed up lookups
- **Block Cache**: LRU cache for frequently accessed data blocks

## 🚀 Quick Start

> Note: AiDb is not yet ready for use. This section will be updated as development progresses.

### Installation

```bash
# Add to Cargo.toml
[dependencies]
aidb = "0.1"
```

### Basic Usage

```rust
use aidb::{DB, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open database
    let options = Options::default();
    let db = DB::open("./data", options)?;

    // Write
    db.put(b"key1", b"value1")?;
    db.put(b"key2", b"value2")?;

    // Read
    if let Some(value) = db.get(b"key1")? {
        println!("key1: {:?}", value);
    }

    // Delete
    db.delete(b"key1")?;

    // Iterate
    let mut iter = db.iter();
    while let Some((key, value)) = iter.next() {
        println!("{:?} => {:?}", key, value);
    }

    Ok(())
}
```

## 📚 Documentation

- [Implementation Plan](IMPLEMENTATION_PLAN.md) - Detailed development roadmap
- [TODO List](TODO.md) - Current task tracking
- [Architecture Guide](docs/architecture.md) - In-depth architecture explanation (coming soon)
- [API Documentation](https://docs.rs/aidb) - Generated API docs (coming soon)

## 🛠️ Development

### Prerequisites

- Rust 1.70 or later
- Cargo

### Build

```bash
# Build in debug mode
cargo build

# Build in release mode
cargo build --release

# Run tests
cargo test

# Run benchmarks
cargo bench

# Check code quality
cargo clippy
cargo fmt
```

### Project Structure

```
aidb/
├── src/
│   ├── lib.rs           # Library entry point
│   ├── error.rs         # Error types
│   ├── config.rs        # Configuration
│   ├── wal/             # Write-Ahead Log
│   ├── memtable/        # MemTable implementation
│   ├── sstable/         # SSTable implementation
│   ├── compaction/      # Compaction logic
│   ├── version/         # Version management
│   ├── iterator/        # Iterator implementations
│   └── db.rs            # Main DB interface
├── tests/               # Integration tests
├── benches/             # Benchmark tests
├── examples/            # Example code
└── docs/                # Documentation
```

## 🎯 Features & Roadmap

### Implemented
- [ ] Basic project structure
- [ ] WAL implementation
- [ ] MemTable with Skip List
- [ ] SSTable format and I/O

### In Progress
- [ ] Version management
- [ ] Compaction
- [ ] DB engine

### Planned
- [ ] Snapshot support
- [ ] Iterator interface
- [ ] Block cache
- [ ] Bloom filter
- [ ] Compression (Snappy/LZ4)
- [ ] Transaction support
- [ ] Performance optimization

## 📊 Performance

Performance benchmarks will be added as the project matures. Target performance:
- Sequential writes: > 100K ops/sec
- Random writes: > 50K ops/sec
- Random reads: > 100K ops/sec

## 🤝 Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

## 📄 License

This project is dual-licensed under:
- MIT License ([LICENSE-MIT](LICENSE) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE) or http://www.apache.org/licenses/LICENSE-2.0)

## 🙏 Acknowledgments

This project is inspired by:
- [RocksDB](https://github.com/facebook/rocksdb) - Facebook's embeddable persistent key-value store
- [LevelDB](https://github.com/google/leveldb) - Google's fast key-value storage library
- [mini-lsm](https://github.com/skyzh/mini-lsm) - Educational LSM-Tree implementation
- [sled](https://github.com/spacejam/sled) - Rust embedded database

## 📞 Contact

For questions or discussions, please open an issue on GitHub.

---

**Note**: AiDb is an educational project and is not yet production-ready. Use at your own risk.