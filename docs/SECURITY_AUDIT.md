# Security Vulnerabilities Status

## ✅ RESOLVED - All Critical Issues Fixed

**Last Migration:** 2025-11-20 - Migrated from raft-rs to openraft

### Migration Summary

We have successfully migrated from `raft 0.7.0` (tikv/raft-rs) to `openraft 0.9`, resolving all critical security vulnerabilities.

**Key Changes:**
- ❌ Removed: `raft 0.7.0`
- ❌ Removed: `protobuf 2.28.0` (had RUSTSEC-2024-0437)
- ❌ Removed: `slog` and `slog-stdlog` dependencies
- ❌ Removed: `fxhash` transitive dependency
- ✅ Added: `openraft 0.9` (uses prost for protobuf 3.x)

---

## Previously Known Issues (NOW RESOLVED)

### 1. ~~Protobuf 2.28.0 (RUSTSEC-2024-0437)~~ - **✅ RESOLVED**

**Severity:** High (DoS via uncontrolled recursion)

**Status:** ✅ FIXED by migrating to openraft

**Resolution:**
- Migrated from raft-rs (which uses protobuf 2.28) to openraft (which uses prost/protobuf 3.x)
- openraft uses modern prost library which doesn't have this vulnerability
- No longer using protobuf 2.x in any dependency

**Date Resolved:** 2025-11-20

---

### 2. ~~fxhash 0.2.1 (RUSTSEC-2025-0057)~~ - **✅ RESOLVED**

**Severity:** Warning (unmaintained)

**Status:** ✅ FIXED by migrating to openraft

**Resolution:**
- fxhash was a transitive dependency from raft-rs
- openraft doesn't depend on fxhash
- Dependency tree is now clean

**Date Resolved:** 2025-11-20

---

### 3. number_prefix 0.4.0 (RUSTSEC-2025-0119) - **ACKNOWLEDGED**

**Severity:** Warning (unmaintained)

**Status:** Optional feature only (CLI)

**Description:**
The `number_prefix` crate is unmaintained.

**Impact on AiDb:**
- **NEGLIGIBLE** - Only used in optional CLI feature via `indicatif`
- Not used in core library or production code
- Only for developer/admin CLI progress bars

**Plan:**
- Wait for `indicatif` to update or switch to alternative
- Consider different progress bar library for CLI
- No immediate action required (optional feature)

---

## Current Security Status

✅ **No known critical or high-severity vulnerabilities**

✅ **All Raft-related security issues resolved**

✅ **Clean dependency tree for core and cluster features**

⚠️ **One low-priority warning** (number_prefix in optional CLI feature)

---

## Audit Configuration

The project uses `cargo-audit` for dependency security scanning:

```bash
# Run security audit
cargo audit

# Audit with ignored advisories
cargo audit --deny warnings --ignore advisories-from audit-ignore.toml
```

## Policy

1. **Critical vulnerabilities** in core dependencies: Fix immediately or find alternatives ✅ Done
2. **High severity** with low actual risk: Document mitigation and monitor ✅ Resolved
3. **Transitive dependencies**: Track upstream fixes, contribute PRs if needed ✅ Clean
4. **Optional features**: Lower priority, document and plan upgrade path ⚠️ Monitoring

## Benefits of openraft Migration

1. ✅ **Security:** No known vulnerabilities, uses protobuf 3.x
2. ✅ **Modern API:** Fully async/await, trait-based design
3. ✅ **Active Maintenance:** Regular updates and community support
4. ✅ **Type Safety:** Better compile-time guarantees
5. ✅ **Simplicity:** Cleaner API reduces code complexity

## Update Schedule

- Weekly check for security advisories
- Monthly review of all acknowledged issues
- Quarterly audit of all dependencies

---

**Last Updated:** 2025-11-20
**Last Migration:** raft-rs → openraft (2025-11-20)
**Next Review:** 2025-12-20
**Current Status:** ✅ All critical issues resolved
