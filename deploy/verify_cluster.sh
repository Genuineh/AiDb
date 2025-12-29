#!/usr/bin/env bash
set -euo pipefail

# Wait for admin ports to become available (timeout ~30s)
for p in 8001 8002 8003; do
  printf "Waiting for admin port %s... " "$p"
  for i in {1..30}; do
    if nc -z -w1 127.0.0.1 $p 2>/dev/null; then
      echo "ok"
      break
    fi
    sleep 1
  done
done

# Print initial per-node status for diagnostics
for p in 8001 8002 8003; do
  echo "--- Node admin:$p diagnostics ---"
  echo "LEADER: "
  timeout 1 bash -c "echo LEADER | nc -w1 127.0.0.1 ${p}" || true
  echo "IS_LEADER: "
  timeout 1 bash -c "echo IS_LEADER | nc -w1 127.0.0.1 ${p}" || true
  echo "METRICS: "
  timeout 1 bash -c "echo METRICS | nc -w1 127.0.0.1 ${p}" || true
done

# Helper: try to get METRICS for a node with retries
get_metrics() {
  local port="$1"
  local tries=3
  local t=1
  while [ $t -le $tries ]; do
    echo "    - metrics try $t/$tries for ${port}"
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ ! -x "${SCRIPT_DIR}/admin_check.py" && ! -f "${SCRIPT_DIR}/admin_check.py" ]]; then
  echo "Error: admin_check.py not found in ${SCRIPT_DIR}" >&2
  exit 1
fi
resp=$(python3 "${SCRIPT_DIR}/admin_check.py" --host 127.0.0.1 --port ${port} --cmd METRICS --timeout 2 2>/dev/null | sed -n 's/^METRICS: //p' || true)
    if [[ -n "$resp" && "$resp" != "<timeout>" && "$resp" != "<no response>" ]]; then
      echo "    - got metrics: ${resp}"
      printf "%s" "$resp"
      return 0
    fi
    echo "    - metrics attempt response: ${resp}"
    t=$((t+1))
    sleep 0.2
  done
  return 1
}

# Helper: try to get IS_LEADER with retries
get_is_leader() {
  local port="$1"
  local tries=3
  local t=1
  while [ $t -le $tries ]; do
    echo "    - is_leader try $t/$tries for ${port}"
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ ! -x "${SCRIPT_DIR}/admin_check.py" && ! -f "${SCRIPT_DIR}/admin_check.py" ]]; then
  echo "Error: admin_check.py not found in ${SCRIPT_DIR}" >&2
  exit 1
fi
resp=$(python3 "${SCRIPT_DIR}/admin_check.py" --host 127.0.0.1 --port ${port} --cmd IS_LEADER --timeout 1 2>/dev/null | sed -n 's/^IS_LEADER: //p' || true)
    if [[ -n "$resp" && "$resp" != "<timeout>" && "$resp" != "<no response>" ]]; then
      echo "    - got is_leader: ${resp}"
      printf "%s" "$resp"
      return 0
    fi
    echo "    - is_leader attempt response: ${resp}"
    t=$((t+1))
    sleep 0.2
  done
  return 1
}

# Wait for a leader to be elected (timeout ~60s). Query per-node with retries.
leader_info=""
for i in {1..60}; do
  echo "Attempt $i/60: checking for leader via METRICS (per-node retries)..."
  for p in 8001 8002 8003; do
    echo "  node ${p}:"
    resp=""
    if metrics=$(get_metrics ${p}); then
      resp="$metrics"
      echo "  node ${p} METRICS: ${resp}"
      if echo "${resp}" | grep -qE 'leader=Some\([0-9]+\)'; then
        leader_id=$(echo "${resp}" | grep -oE 'leader=Some\([0-9]+\)' | head -n1 | sed -E 's/leader=Some\(([0-9]+)\)/\1/')
        leader_info="OK ${leader_id}"
        echo "Found leader via METRICS: ${leader_info}"
        break 2
      fi
    else
      echo "  node ${p} METRICS: <no response after retries>"
    fi

    # Fallback to IS_LEADER
    if is_leader_resp=$(get_is_leader ${p}); then
      echo "  node ${p} IS_LEADER: ${is_leader_resp}"
      if [[ "${is_leader_resp}" == "OK true" ]]; then
        leader_id=$(echo ${p} | sed -E 's/800([0-9]+)/\1/')
        leader_info="OK ${leader_id}"
        echo "Found leader via IS_LEADER: ${leader_info}"
        break 2
      fi
    else
      echo "  node ${p} IS_LEADER: <no response after retries>"
    fi
  done
  sleep 1
done

if [[ -z "$leader_info" || "$leader_info" == *"None"* ]]; then
  echo "No leader detected after timeout" >&2
  echo "Gathering docker-compose status and recent logs..."
  if command -v docker >/dev/null 2>&1; then
    echo "--- docker compose ps ---"
    docker compose -f docker-compose.cluster.yml ps || docker-compose -f docker-compose.cluster.yml ps || true
    echo "--- docker compose logs (last 200 lines) ---"
    docker compose -f docker-compose.cluster.yml logs --tail 200 || docker-compose -f docker-compose.cluster.yml logs --tail 200 || true
  else
    echo "Docker not found; cannot show container logs"
  fi
  exit 1
fi

# Extract leader id from response: "OK <id>" or "OK None"
leader_id=$(echo "$leader_info" | awk '{print $2}')
if [[ "$leader_id" == "None" || -z "$leader_id" ]]; then
  echo "No leader present" >&2
  exit 1
fi

echo "Found leader: $leader_id"
admin_port="800${leader_id}"

# Write a key to the leader (with timeout)
put_resp=$(timeout 2 bash -c "echo \"PUT mykey hello_from_leader\" | nc -w1 127.0.0.1 ${admin_port}" 2>/dev/null) || put_resp=""
echo "PUT response: ${put_resp}"

sleep 1

# Verify replication: read from all nodes (with timeouts)
for p in 8001 8002 8003; do
  echo "Checking node admin $p for key..."
  get_resp=$(timeout 1 bash -c "echo \"GET mykey\" | nc -w1 127.0.0.1 ${p}" 2>/dev/null) || get_resp=""
  echo "Node ${p} GET response: ${get_resp}"
done

# Test adding a new learner node 4
# Start a fourth node (on different ports) if desired (manual step)
# For membership change, you can run: echo "ADD_LEARNER 4=http://node4:50004" | nc localhost 8001

# Basic election check: stop the leader and see new leader emerges (manual)

echo "Verification completed. Review outputs above to ensure replication and election behavior." 
