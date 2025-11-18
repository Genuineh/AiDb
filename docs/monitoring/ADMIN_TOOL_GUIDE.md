# AiDb Admin Tool - User Guide

The `aidb-admin` command-line tool provides comprehensive management capabilities for AiDb instances and clusters.

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Command Reference](#command-reference)
  - [Cluster Management](#cluster-management)
  - [Backup & Recovery](#backup--recovery)
  - [Database Statistics](#database-statistics)
  - [Health Checks](#health-checks)
  - [Metrics Viewing](#metrics-viewing)
- [Usage Examples](#usage-examples)
- [Troubleshooting](#troubleshooting)

## Installation

### Build from Source

```bash
# Build with admin CLI feature
cargo build --release --features admin-cli --bin aidb-admin

# The binary will be at target/release/aidb-admin
```

### Install System-wide

```bash
cargo install --path . --features admin-cli --bin aidb-admin
```

## Quick Start

```bash
# Check database health
aidb-admin --db /path/to/db health

# View database statistics
aidb-admin --db /path/to/db stats

# List cluster nodes
aidb-admin cluster nodes

# Create a backup
aidb-admin --db /path/to/db backup create --output /backups

# View metrics
aidb-admin metrics
```

## Command Reference

### Global Options

All commands support these global options:

- `-v, --verbose`: Enable verbose output
- `-d, --db <PATH>`: Specify database path
- `-h, --help`: Show help information
- `-V, --version`: Show version

### Cluster Management

#### `cluster status`

Show overall cluster status and health.

```bash
aidb-admin cluster status [--detailed]
```

**Options:**
- `--detailed`: Show detailed node information

**Example:**
```bash
$ aidb-admin cluster status
Cluster Status:
==============

Status: ✓ Healthy
Total Nodes: 3
Primary Nodes: 1
Replica Nodes: 2
Total Shards: 1
```

#### `cluster nodes`

List all nodes in the cluster.

```bash
aidb-admin cluster nodes [--node-type <TYPE>]
```

**Options:**
- `-t, --node-type <TYPE>`: Filter by node type (primary/replica)

**Example:**
```bash
$ aidb-admin cluster nodes
Cluster Nodes:
=============

+----+---------+----------------+---------+----------+
| ID | Type    | Address        | Status  | Shard ID |
+====================================================+
| 1  | Primary | 127.0.0.1:5000 | Healthy | 0        |
| 2  | Replica | 127.0.0.1:5001 | Healthy | 0        |
| 3  | Replica | 127.0.0.1:5002 | Healthy | 0        |
+----+---------+----------------+---------+----------+
```

#### `cluster shards`

List all shards in the cluster.

```bash
aidb-admin cluster shards [--detailed]
```

**Options:**
- `-d, --detailed`: Show detailed shard information

**Example:**
```bash
$ aidb-admin cluster shards
Cluster Shards:
==============

+----------+---------+----------+----------+--------+
| Shard ID | Primary | Replicas | Keys     | Size   |
+========================================================+
| 0        | Node 1  | 2        | 1,000,000| 500 MB |
+----------+---------+----------+----------+--------+
```

#### `cluster add-node`

Add a new node to the cluster.

```bash
aidb-admin cluster add-node \
  --address <ADDR> \
  --node-type <TYPE> \
  [--shard-id <ID>]
```

**Options:**
- `-a, --address <ADDR>`: Node address (e.g., 127.0.0.1:5003)
- `-t, --node-type <TYPE>`: Node type (primary or replica)
- `-s, --shard-id <ID>`: Shard ID (optional)

**Example:**
```bash
$ aidb-admin cluster add-node \
  --address 127.0.0.1:5003 \
  --node-type replica \
  --shard-id 0

Adding node to cluster...
  Address: 127.0.0.1:5003
  Type: replica
  Shard ID: 0

[00:00:02] ========================================> 100% Finalizing...
✓ Node added to cluster
```

#### `cluster remove-node`

Remove a node from the cluster.

```bash
aidb-admin cluster remove-node <NODE_ID> [--force]
```

**Options:**
- `-f, --force`: Force removal without safety checks

**Example:**
```bash
$ aidb-admin cluster remove-node node3 --force
Removing node from cluster...
  Node ID: node3
  Force: true

[00:00:02] ========================================> 100% Finalizing...
✓ Node removed from cluster
```

### Backup & Recovery

#### `backup create`

Create a new backup of the database.

```bash
aidb-admin --db <PATH> backup create \
  --output <DIR> \
  [--description <DESC>] \
  [--compress]
```

**Options:**
- `-o, --output <DIR>`: Output directory for backup
- `-d, --description <DESC>`: Backup description
- `-c, --compress`: Compress backup files

**Example:**
```bash
$ aidb-admin --db /data/aidb backup create \
  --output /backups/daily \
  --description "Daily backup" \
  --compress

Creating backup...
  Database: /data/aidb
  Output: /backups/daily
  Description: Daily backup
  Compress: true

[00:00:03] ========================================> 100% Finalizing...
✓ Backup completed at 2025-11-18 10:30:00 UTC
  Location: /backups/daily
  Size: 125 MB
  Files: 45 SSTables, 1 WAL, metadata
```

#### `backup list`

List available backups.

```bash
aidb-admin backup list --path <DIR> [--detailed]
```

**Options:**
- `-p, --path <DIR>`: Backup directory
- `-d, --detailed`: Show detailed backup information

**Example:**
```bash
$ aidb-admin backup list --path /backups
Available Backups:
=================

+----+---------------------+--------+------------------+
| ID | Timestamp           | Size   | Description      |
+==========================================================+
| 1  | 2025-11-18 10:30:00 | 125 MB | Daily backup     |
| 2  | 2025-11-17 10:30:00 | 120 MB | Daily backup     |
| 3  | 2025-11-16 10:30:00 | 118 MB | Daily backup     |
+----+---------------------+--------+------------------+

Total: 3 backups (363 MB)
```

#### `backup restore`

Restore database from backup.

```bash
aidb-admin backup restore \
  --backup <PATH> \
  --target <DIR> \
  [--force]
```

**Options:**
- `-b, --backup <PATH>`: Backup path
- `-t, --target <DIR>`: Target database path
- `-f, --force`: Force restore (overwrite existing)

**Example:**
```bash
$ aidb-admin backup restore \
  --backup /backups/daily/backup-001 \
  --target /data/aidb-restored \
  --force

Restoring from backup...
  Backup: /backups/daily/backup-001
  Target: /data/aidb-restored
  Force: true

[00:00:03] ========================================> 100% Finalizing...
✓ Database restored
  Restored 45 SSTables
  Restored 1 WAL file
  Total size: 125 MB
```

#### `backup delete`

Delete a backup.

```bash
aidb-admin backup delete <PATH> [--yes]
```

**Options:**
- `-y, --yes`: Skip confirmation

**Example:**
```bash
$ aidb-admin backup delete /backups/old-backup -y
Deleting backup: /backups/old-backup

Deleting backup files...
✓ Backup deleted
```

### Database Statistics

#### `stats`

Show database statistics and metrics.

```bash
aidb-admin --db <PATH> stats [--detailed]
```

**Options:**
- `-d, --detailed`: Show detailed statistics

**Example:**
```bash
$ aidb-admin --db /data/aidb stats
Database Statistics:
===================
  Path: /data/aidb

+-------------------+-----------+
| Metric            | Value     |
+=====================================+
| Total Keys        | 1,234,567 |
| Total Size        | 1.2 GB    |
| SSTables (L0)     | 5         |
| SSTables (L1)     | 10        |
| SSTables (L2)     | 20        |
| WAL Size          | 15 MB     |
| MemTable Size     | 8 MB      |
| Cache Hit Rate    | 87.5%     |
| Compactions (24h) | 42        |
+-------------------+-----------+
```

With `--detailed`:
```bash
$ aidb-admin --db /data/aidb stats --detailed
[Same as above, plus:]

Performance Metrics (last 5 min):
  Read QPS: 1,234 ops/sec
  Write QPS: 456 ops/sec
  P50 Latency: 1.2 ms
  P99 Latency: 5.8 ms

Storage Details:
  Level 0: 5 files, 50 MB
  Level 1: 10 files, 200 MB
  Level 2: 20 files, 950 MB
  Total: 35 files, 1.2 GB
```

### Health Checks

#### `health`

Perform health check on database and components.

```bash
aidb-admin --db <PATH> health [--component <NAME>]
```

**Options:**
- `-c, --component <NAME>`: Check specific component

**Example:**
```bash
$ aidb-admin --db /data/aidb health
Health Check:
============
  Database: /data/aidb

+------------+-----------+-----------------------+
| Component  | Status    | Details               |
+================================================+
| Database   | ✓ Healthy | All checks passed     |
| MemTable   | ✓ Healthy | 8 MB / 64 MB (12%)    |
| WAL        | ✓ Healthy | 15 MB, synced         |
| SSTables   | ✓ Healthy | 35 files, 1.2 GB      |
| Cache      | ✓ Healthy | 256 MB / 512 MB (50%) |
| Compaction | ✓ Healthy | Not needed            |
| Backup     | ✓ Healthy | Last: 2 hours ago     |
+------------+-----------+-----------------------+

Overall Status: ✓ All systems healthy
```

### Metrics Viewing

#### `metrics`

View current metrics from the metrics server.

```bash
aidb-admin metrics [--url <URL>] [--watch <SECONDS>]
```

**Options:**
- `-u, --url <URL>`: Metrics endpoint URL (default: http://localhost:9090/metrics)
- `-w, --watch <SECONDS>`: Watch mode (refresh every N seconds)

**Example:**
```bash
$ aidb-admin metrics
Fetching metrics from: http://localhost:9090/metrics

Current Metrics:
===============

+-----------------+----------------+--------+
| Metric          | Value          | Trend  |
+================================================+
| Request Rate    | 1,234 ops/sec  | ↑ +5%  |
| P99 Latency     | 5.8 ms         | → stable|
| Error Rate      | 0.01%          | ↓ -50% |
| Cache Hit Rate  | 87.5%          | ↑ +2%  |
| Memory Usage    | 512 MB         | → stable|
| Disk Usage      | 1.2 GB         | ↑ +0.1%|
+-----------------+----------------+--------+

Last updated: 2025-11-18 10:30:00 UTC
```

Watch mode:
```bash
$ aidb-admin metrics --watch 5
Fetching metrics from: http://localhost:9090/metrics

Watch mode enabled (refresh every 5 seconds)
Press Ctrl+C to exit

[Screen refreshes every 5 seconds...]
```

## Usage Examples

### Daily Operations

```bash
# Morning health check
aidb-admin --db /data/aidb health

# Check stats before business hours
aidb-admin --db /data/aidb stats

# Monitor during peak hours
aidb-admin metrics --watch 10
```

### Backup Routine

```bash
# Create daily backup
aidb-admin --db /data/aidb backup create \
  --output /backups/daily/$(date +%Y%m%d) \
  --description "Daily backup $(date)" \
  --compress

# List recent backups
aidb-admin backup list --path /backups/daily

# Cleanup old backups (keep last 7 days)
find /backups/daily -type d -mtime +7 -exec aidb-admin backup delete {} -y \;
```

### Disaster Recovery

```bash
# 1. Stop the database (if running)
# 2. Find latest backup
aidb-admin backup list --path /backups/daily --detailed

# 3. Restore from backup
aidb-admin backup restore \
  --backup /backups/daily/latest \
  --target /data/aidb-recovered \
  --force

# 4. Verify restored database
aidb-admin --db /data/aidb-recovered health
aidb-admin --db /data/aidb-recovered stats

# 5. Start database with recovered data
```

### Cluster Scaling

```bash
# Add new replica for read scaling
aidb-admin cluster add-node \
  --address 10.0.1.100:5000 \
  --node-type replica \
  --shard-id 0

# Verify node is healthy
aidb-admin cluster status --detailed

# Monitor cluster
aidb-admin cluster nodes
```

### Performance Investigation

```bash
# Check overall stats
aidb-admin --db /data/aidb stats --detailed

# Examine health of all components
aidb-admin --db /data/aidb health

# Watch live metrics
aidb-admin metrics --watch 5

# Check if compaction is needed
aidb-admin --db /data/aidb stats | grep "SSTables"
```

## Troubleshooting

### Command Not Found

If you get "command not found":

1. Ensure you built with the correct feature:
   ```bash
   cargo build --release --features admin-cli --bin aidb-admin
   ```

2. Add to PATH or use full path:
   ```bash
   export PATH=$PATH:$(pwd)/target/release
   # or
   ./target/release/aidb-admin --help
   ```

### Database Path Issues

If you get "Database path required":

- Always specify `--db` flag for database operations:
  ```bash
  aidb-admin --db /path/to/db stats
  ```

### Permission Denied

If you get permission errors:

- Ensure you have read/write access to database directory
- Run with appropriate user permissions
- Check file ownership: `ls -la /path/to/db`

### Cluster Commands Not Working

Currently, cluster commands show simulated data for demonstration. To use with real clusters:

1. Ensure cluster feature is enabled
2. Configure cluster endpoints
3. Future versions will support live cluster management

### Backup/Restore Errors

Common issues:

- **Backup target exists**: Use `--force` flag
- **Insufficient disk space**: Check available space
- **Corrupted backup**: Verify backup integrity
- **Permission denied**: Check directory permissions

### Metrics Not Available

If metrics command fails:

1. Ensure metrics server is running
2. Check URL is correct (default: http://localhost:9090/metrics)
3. Verify network connectivity
4. Check firewall rules

## Advanced Usage

### Scripting and Automation

The tool is designed to be script-friendly:

```bash
#!/bin/bash
# Daily backup script

DB_PATH="/data/aidb"
BACKUP_DIR="/backups/daily"
DATE=$(date +%Y%m%d)

# Create backup
aidb-admin --db "$DB_PATH" backup create \
  --output "$BACKUP_DIR/$DATE" \
  --description "Automated daily backup" \
  --compress

# Check if successful
if [ $? -eq 0 ]; then
  echo "Backup successful: $BACKUP_DIR/$DATE"
  
  # Cleanup old backups (keep 7 days)
  find "$BACKUP_DIR" -type d -mtime +7 -delete
else
  echo "Backup failed!" >&2
  exit 1
fi
```

### Monitoring Integration

Export metrics to monitoring systems:

```bash
# Export metrics to file
aidb-admin metrics > /tmp/aidb-metrics.txt

# Parse and send to monitoring
cat /tmp/aidb-metrics.txt | your-monitoring-tool
```

### JSON Output (Future)

Future versions may support JSON output for easier parsing:

```bash
aidb-admin --output json cluster nodes
```

## See Also

- [Monitoring Guide](MONITORING_GUIDE.md) - Detailed monitoring setup
- [Operations Manual](OPERATIONS_MANUAL.md) - Day-to-day operations
- [Troubleshooting Guide](TROUBLESHOOTING.md) - Common issues and solutions
- [API Documentation](https://docs.rs/aidb) - Library API reference
