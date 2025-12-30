# Copilot / AI Agent Instructions for AiDb

Short, actionable orientation so an AI coding agent becomes productive quickly.

## Quick orientation
- Project: a Rust-based LSM-Tree distributed KV engine (single-node + Multi-Raft cluster). See `README.md` and `docs/ARCHITECTURE.md` for the big picture.
- Key dirs: `src/` (implementation), `tests/` (integration tests), `benches/` (benchmarks), `examples/` (usage examples), `docs/` (design + dev + CI docs), `deploy/` (ops scripts).

## Architecture pointers (read these first)
- High-level design: `docs/ARCHITECTURE.md` (WAL → MemTable → SSTable, compaction, Multi-Raft cluster).
- Cluster architecture: Multi-Raft with OpenRaft (see `docs/MULTI_RAFT_ARCHITECTURE.md` for details).
- Redis Cluster compatibility: 16384 slots, CRC16 routing (see `docs/REDIS_CLUSTER_COMPATIBILITY.md`).
- Internal key ordering and SSTable formats are documented in `docs/ARCHITECTURE.md` — changes here require careful compatibility consideration.

## Developer workflows & important commands
- Build: `cargo build` (dev) / `cargo build --release` (optimized).
- Feature-aware builds: use `--features raft-cluster` when working on cluster/raft code.
- Tests: `cargo test` or `cargo test --all-features` for CI parity; run single tests with `cargo test <name>` and see output with `-- --nocapture`.
- Benchmarks: `cargo bench` (local) and `cargo bench --no-run` (CI compile check). Use `criterion` benches in `benches/`.
- Formatting & linting (enforced by CI): `cargo fmt --all` and `cargo clippy --all-targets --all-features -- -D warnings`.
- Profiling: `cargo flamegraph` and `perf` usage described in `docs/DEVELOPMENT.md`.

## CI behavior you must respect
- CI (see `docs/CICD.md` and `.github/workflows/ci.yml`) runs only when PRs are marked "Ready for review" and on pushes to `main`.
- Doc-only PRs are detected and skip code tests; modify docs under `docs/` or `*.md` → CI runs docs-check only.
- CI auto-formats code and will push a `[skip ci]` commit back for same-repo PRs; for forked PRs, CI will fail formatting and ask the contributor to run `cargo fmt --all` locally.
- Always assume `cargo clippy` is strict (`-D warnings`) in CI — fix warnings.

## Project-specific code patterns & conventions
- Prefer TDD: many modules have tests-first examples in `docs/DEVELOPMENT.md` and `tests/` use `tempdir` heavily.
- Error handling: use `Result` and `?`; avoid `unwrap()`/`expect()` in library code (see `docs/DEVELOPMENT.md`).
- Data structures: `MemTable` uses a `SkipList` (`src/memtable`), WAL format and rotation in `src/wal`, SSTable layout in `src/sstable` — these are sensitive areas for correctness and compatibility.
- Concurrency: many modules use `Arc` + locking primitives; be careful with blocking calls in async contexts (cluster code uses `tokio`).
- Naming: Rust conventional style (types CamelCase, functions snake_case).

## Where to add tests & what to assert
- Unit tests belong alongside modules (`#[cfg(test)] mod tests`) and should use `tempfile::tempdir()` when touching fs.
- Integration tests go into `tests/` and should exercise persistence/recovery paths (persistence is critical: WAL + replay + Manifest).
- Add a benchmark to `benches/` for any performance-sensitive change.

## Cross-component and infra pointers
- Cluster code: `src/cluster/` with Multi-Raft implementation (`raft_*.rs`, `multi_raft_*.rs`, `sharded_*.rs`).
- Examples: `examples/cluster/` with `openraft_demo.rs`, `node_runner.rs`, `sharded_multi_raft_demo.rs`.
- Admin scripts and quick checks: `deploy/admin_check.py`, `deploy/verify_cluster.sh`, `deploy/membership_check.sh`.
- CI secrets & release flow: `docs/CICD.md` (how tagging and crate publishing works; `CARGO_TOKEN` required for crates.io).

## Small PR checklist for AI-driven changes
1. Run `cargo test` (or `cargo test --all-features`).
2. Fix clippy warnings: `cargo clippy -- -D warnings`.
3. Ensure `cargo fmt --all` (CI will auto-format but fix locally when possible).
4. Add/adjust unit or integration tests covering the change.
5. Update `docs/` or `README.md` if behavior or configuration changed.
6. Mention relevant files in PR description (e.g., `src/sstable/*`, `src/wal/*`, `docs/ARCHITECTURE.md`).

## When to ask for human review / escalate
- Changes to on-disk formats (`src/sstable`, `MANIFEST`) or network RPC schemas (proto files in `proto/`) — require design discussion and migration plan.
- Backwards-incompatible API or behavior changes (bump version + changelog + tests).
- Performance affecting changes without benchmarks and flamegraphs.

---
If any of these items are unclear or you'd like a stricter template for PR text or unit test structure, tell me which part to expand and I will iterate.
