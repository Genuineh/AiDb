# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- GitHub Actions CI/CD pipeline
  - Automated testing on multiple platforms (Linux, macOS, Windows)
  - Multi-version Rust testing (stable, beta, nightly)
  - Code quality checks (clippy, rustfmt)
  - Security scanning (cargo-audit, cargo-deny, CodeQL)
  - Code coverage reporting with Codecov
- Automated release workflow
  - Multi-platform binary builds (x86_64, ARM64)
  - Automatic GitHub Releases
  - crates.io publishing
- Dependabot configuration for automated dependency updates
- PR and Issue templates for better workflow management
- Comprehensive CI/CD documentation
- Flush functionality implementation
  - MemTable freeze mechanism
  - Automatic flush when MemTable is full
  - Manual flush API
  - WAL rotation after flush
  - Data persistence and recovery
- 5 additional tests for empty SSTable prevention (total: 96 tests)
- Example program demonstrating tombstone flush behavior

### Changed
- Updated README with CI badges
- Enhanced documentation structure
- Improved README with flush functionality documentation

### Fixed
- **Empty SSTable Prevention**: Fixed bug where `flush_memtable_to_sstable()` was creating empty SSTable files when MemTable contained only tombstones or filtered entries
  - Now properly checks `entry_count` before creating SSTable
  - Abandons builder and removes incomplete files when no entries to flush
  - Prevents wasted disk space and improves read performance
  - Added comprehensive test coverage (5 new tests)
  - See `BUG_FIX_EMPTY_SSTABLE.md` for details

## [0.1.0] - 2024-01-XX

### Added
- Initial project setup
- WAL (Write-Ahead Log) implementation
  - WAL writer with batch support
  - WAL reader with recovery
  - Record format with CRC32 checksum
  - Sync and fsync support
- Basic error handling
- Configuration management
- Examples and benchmarks

### Documentation
- Architecture design
- Implementation plan
- Development guide
- Contributing guide

---

## Release Types

- **Major (X.0.0)**: Breaking changes
- **Minor (0.X.0)**: New features, backwards compatible
- **Patch (0.0.X)**: Bug fixes, backwards compatible

## Categories

- **Added**: New features
- **Changed**: Changes in existing functionality
- **Deprecated**: Soon-to-be removed features
- **Removed**: Removed features
- **Fixed**: Bug fixes
- **Security**: Security improvements
- **Performance**: Performance improvements

[Unreleased]: https://github.com/yourusername/aidb/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/yourusername/aidb/releases/tag/v0.1.0
