#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/docker-compose.cluster.yml"

# Build & start containers (only nodes 1-3 for base cluster)
docker-compose -f "$COMPOSE_FILE" up -d node1 node2 node3

echo "Waiting for admin ports to come up..."
for port in 8001 8002 8003; do
  echo -n "Waiting for port $port... "
  while ! nc -z localhost $port; do
    sleep 1
  done
  echo "ok"
done

# Check if cluster needs initialization by querying METRICS
echo "Checking if cluster needs initialization..."
METRICS=$(echo "METRICS" | nc -w5 localhost 8001 2>/dev/null || echo "")

# Check if cluster has voters in membership (indicates initialization)
if echo "$METRICS" | grep -qE 'membership:.*voters:\[[0-9]'; then
  echo "Cluster already has voters in membership, skipping initialization."
  echo "METRICS: $METRICS"
elif echo "$METRICS" | grep -q "leader=Some"; then
  echo "Cluster already has a leader, skipping initialization."
  echo "METRICS: $METRICS"
else
  # Cluster not initialized (no voters, no leader)
  echo "Cluster not initialized. Sending INIT command to node1..."
  INIT_RESULT=$(echo "INIT 1=http://node1:50001,2=http://node2:50002,3=http://node3:50003" | nc -w10 localhost 8001 2>/dev/null || echo "FAILED")
  if echo "$INIT_RESULT" | grep -q "OK"; then
    echo "Cluster initialized successfully."
    sleep 2
  else
    echo "WARNING: INIT command returned: $INIT_RESULT"
    echo "This may indicate cluster already initialized or requires data cleanup (docker-compose down -v)"
  fi
fi

# Verify leader election
echo "Verifying leader election..."
for i in {1..30}; do
  METRICS=$(echo "METRICS" | nc -w5 localhost 8001 2>/dev/null || echo "")
  if echo "$METRICS" | grep -q "leader=Some"; then
    echo "Leader elected: $METRICS"
    break
  fi
  if [ $i -eq 30 ]; then
    echo "WARNING: No leader elected after 30 seconds"
  fi
  sleep 1
done

echo "Cluster should be up. Use deploy/verify_cluster.sh to validate." 
