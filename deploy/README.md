# Deploy / Cluster verification

This directory contains tools to run a small 3-node Raft cluster for manual verification/testing.

Files:
- `Dockerfile` - builds a minimal image with the `examples/cluster/node_runner` example
- `docker-compose.cluster.yml` - three-node compose configuration
- `init_cluster.sh` - bring up compose, wait for admin ports
- `verify_cluster.sh` - simple checks: leader election, replication

Quickstart:
1. Build & start cluster:
   ./deploy/init_cluster.sh

2. Verify leader election and replication:
   ./deploy/verify_cluster.sh

Admin TCP commands (connect to admin port, newline-terminated):
- `INIT <peers>` - initialize cluster (peers format: `1=http://node1:50001,2=http://node2:50002`)
- `ADD_LEARNER <id>=<addr>`
- `CHANGE_MEMBERS <id,id,...>`
- `PUT <key> <value>`
- `GET <key>`
- `IS_LEADER` - returns boolean
- `LEADER` - returns current leader id or `None`
- `METRICS` - some raft metrics
- `SHUTDOWN` - graceful shutdown

Notes & caveats:
- The `node_runner` example is intended for manual debugging and functional verification, not production use.
- The admin protocol is intentionally simple (plain TCP text) so you can use `nc` / `telnet` for quick checks.
- For membership changes and node add/remove tests, you may need to start additional containers manually and use `ADD_LEARNER` / `CHANGE_MEMBERS` commands.

Happy testing! ✨
