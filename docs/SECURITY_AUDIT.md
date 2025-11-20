# Security Vulnerabilities Status

## Known Issues

### 1. Protobuf 2.28.0 (RUSTSEC-2024-0437) - **ACKNOWLEDGED**

**Severity:** High (DoS via uncontrolled recursion)

**Status:** Waiting for upstream fix

**Description:**
The `protobuf 2.28.0` crate has a vulnerability where maliciously crafted messages can cause uncontrolled recursion leading to stack overflow.

**Impact on AiDb:**
- **LOW** - The vulnerability affects Raft cluster communication
- Raft messages are internal and only accepted from authenticated cluster peers
- Not exposed to untrusted external input

**Root Cause:**
- `raft 0.7.0` (from TiKV/raft-rs) depends on `protobuf 2.28.0`
- No newer version of raft-rs available with protobuf 3.x support
- protobuf 3.7.2+ contains the fix

**Mitigation:**
1. Raft cluster communication is peer-authenticated
2. Input validation and message size limits in place
3. Rate limiting on cluster messages
4. Monitoring for CPU/memory anomalies

**Plan:**
- [ ] Monitor raft-rs repository for protobuf 3.x migration
- [ ] Consider contributing PR to raft-rs for protobuf upgrade
- [ ] Upgrade when raft 0.8+ is released with protobuf 3.x
- [ ] Alternative: Fork raft-rs and update protobuf (only if critical)

**References:**
- https://rustsec.org/advisories/RUSTSEC-2024-0437
- https://github.com/tikv/raft-rs/issues

---

### 2. fxhash 0.2.1 (RUSTSEC-2025-0057) - **ACKNOWLEDGED**

**Severity:** Warning (unmaintained)

**Status:** Transitive dependency

**Description:**
The `fxhash` crate is no longer maintained.

**Impact on AiDb:**
- **VERY LOW** - Simple hash function, stable codebase
- Transitive dependency from `raft 0.7.0`
- No known security vulnerabilities in the code itself

**Plan:**
- Will be resolved when raft-rs updates dependencies
- No immediate action required

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

## Audit Configuration

The project uses `cargo-audit` for dependency security scanning:

```bash
# Run security audit
cargo audit

# Audit with ignored advisories
cargo audit --deny warnings --ignore advisories-from audit-ignore.toml
```

## Policy

1. **Critical vulnerabilities** in core dependencies: Fix immediately or find alternatives
2. **High severity** with low actual risk: Document mitigation and monitor
3. **Transitive dependencies**: Track upstream fixes, contribute PRs if needed
4. **Optional features**: Lower priority, document and plan upgrade path

## Update Schedule

- Weekly check for security advisories
- Monthly review of all acknowledged issues
- Quarterly audit of all dependencies

---

**Last Updated:** 2025-11-20
**Next Review:** 2025-12-20
