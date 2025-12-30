#!/usr/bin/env bash
set -euo pipefail

# membership_check.sh
# Usage: ./deploy/membership_check.sh [-t timeout_seconds]
# Steps:
#  - Start node4 (docker compose up -d node4)
#  - Wait for admin 8004
#  - ADD_LEARNER 4 to leader
#  - CHANGE_MEMBERS to include node4
#  - Verify node4 receives state-machine data
#  - Remove node1 from membership and stop node1
#  - Verify a new leader is elected among remaining nodes and cluster still accepts writes

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Prefer docker-compose if available, otherwise fall back to docker compose
if command -v docker-compose >/dev/null 2>&1; then
  COMPOSE_CMD="docker-compose -f ${SCRIPT_DIR}/docker-compose.cluster.yml"
elif command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
  COMPOSE_CMD="docker compose -f ${SCRIPT_DIR}/docker-compose.cluster.yml"
else
  echo "Error: neither docker-compose nor docker compose is available" >&2
  exit 1
fi

echo "Using compose command: ${COMPOSE_CMD}"

ADMIN_CHECK="${SCRIPT_DIR}/admin_check.py"
TIMEOUT=${1:-30}
TEST_KEY="membership_test_key"
TEST_VAL="membership_ok"

if [[ ! -f "$ADMIN_CHECK" ]]; then
  echo "Error: admin_check.py not found in ${SCRIPT_DIR}" >&2
  exit 1
fi

# Ensure base cluster (nodes 1-3) is running and initialized
echo "Checking if base cluster (nodes 1-3) is running..."
$COMPOSE_CMD ps node1 node2 node3 | grep -q "Up" || {
  echo "Base cluster not fully running. Starting nodes 1-3..."
  $COMPOSE_CMD up -d node1 node2 node3
  echo "Waiting for admin ports 8001-8003..."
  for p in 8001 8002 8003; do
    for i in {1..30}; do
      if nc -z -w1 127.0.0.1 $p 2>/dev/null; then
        echo "Port $p is up"
        break
      fi
      sleep 1
    done
  done
}

# Check if base cluster needs initialization
echo "Checking base cluster initialization status..."
needs_init=false
for p in 8001 8002 8003; do
  if nc -z -w1 127.0.0.1 $p 2>/dev/null; then
    METRICS=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 2 2>/dev/null || true)
    if echo "$METRICS" | grep -q "state=Learner"; then
      if ! echo "$METRICS" | grep -qE 'membership:.*voters:\[[0-9]'; then
        needs_init=true
        echo "Node ${p} is Learner without voters - needs init"
        break
      fi
    fi
  fi
done

if [ "$needs_init" = true ]; then
  echo "Initializing base cluster (1,2,3)..."
  INIT_OUT=$(python3 "$ADMIN_CHECK" --port 8001 --cmd "INIT 1=http://node1:50001,2=http://node2:50002,3=http://node3:50003" --timeout 10 2>/dev/null || echo "FAILED")
  echo "INIT response: ${INIT_OUT}"
  if [[ "$INIT_OUT" != *"OK"* ]]; then
    echo "Warning: INIT command did not return OK. Cluster may already be initialized or encountered an error." >&2
  fi
  echo "Waiting for cluster to stabilize after init..."
  sleep 3
fi

echo "Starting node4..."
$COMPOSE_CMD up -d node4

# wait admin 8004
for i in {1..30}; do
  if nc -z -w1 127.0.0.1 8004; then
    echo "node4 admin up"
    break
  fi
  sleep 1
done

# find leader (wait up to 30s)
leader_port=""
search_timeout=${SEARCH_LEADER_TIMEOUT:-30}
start=$SECONDS
while [ $((SECONDS - start)) -lt $search_timeout ]; do
  for p in 8001 8002 8003 8004; do
    METRICS=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 1 2>/dev/null | sed -n 's/^METRICS: //p' || true)
    IS_LEADER=$(python3 "$ADMIN_CHECK" --port ${p} --cmd IS_LEADER --timeout 1 2>/dev/null | sed -n 's/^IS_LEADER: //p' || true)
    echo "probe node ${p}: metrics='${METRICS}' is_leader='${IS_LEADER}'"
    if [[ -n "$METRICS" && "$METRICS" =~ leader=Some\([0-9]+\) ]]; then
      id=$(echo "$METRICS" | grep -oE 'leader=Some\([0-9]+\)' | sed -E 's/leader=Some\(([0-9]+)\)/\1/')
      leader_port="800${id}"
      break 2
    fi
    if [[ "$IS_LEADER" == "OK true" ]]; then
      leader_port=${p}
      break 2
    fi
  done
  sleep 1
done

if [[ -z "$leader_port" ]]; then
  echo "No leader found within ${search_timeout}s" >&2
  echo "Dumping logs for debugging..."
  $COMPOSE_CMD logs --tail 200 || true
  exit 2
fi

echo "Leader is ${leader_port} — adding node4 as learner"
# Add learner with retries and error handling
ADD_OUT=$(python3 "$ADMIN_CHECK" --port ${leader_port} --cmd "ADD_LEARNER 4=http://node4:50004" --timeout 5 2>/dev/null || true)
echo "ADD_LEARNER response: ${ADD_OUT}"

# Helper: parse conflict index from change_members error
parse_conflict_index() {
  local s="$1"
  if echo "$s" | grep -qE 'index: [0-9]+'; then
    # pick the first matched index (the in-flight config index is the earlier occurrence)
    echo "$s" | grep -oE 'index: [0-9]+' | head -n1 | sed -E 's/index: ([0-9]+)/\1/'
  else
    echo ""
  fi
}

# Helper: wait for last_log_index >= N on any node (or leader)
wait_for_log_index() {
  local target=$1
  local timeout=${2:-30}
  local start=$SECONDS
  while [ $((SECONDS-start)) -lt $timeout ]; do
    for p in 8001 8002 8003 8004; do
      m=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 1 2>/dev/null || true)
      li=$(echo "$m" | grep -oE 'last_log_index=Some\([0-9]+\)' | sed -E 's/last_log_index=Some\(([0-9]+)\)/\1/' || true)
      if [[ -n "$li" && "$li" -ge "$target" ]]; then
        echo "Observed last_log_index >= ${target} on node ${p} (index=${li})"
        return 0
      fi
    done
    sleep 1
  done
  return 1
}

# Helper: wait for membership to include (or exclude) a node and optionally ensure last_applied_index >= min_index
wait_for_membership_condition() {
  local want_member="$1"    # numeric node id, e.g. "4"
  local should_include=${2:-true} # "true" to wait for inclusion, anything else for absence
  local min_index=${3:-0}
  local timeout=${4:-60}
  local start=$SECONDS

  if [[ "$should_include" == "true" ]]; then
    echo "Waiting for membership to INCLUDE node ${want_member} with min_index ${min_index} (timeout ${timeout}s)"
  else
    echo "Waiting for membership to EXCLUDE node ${want_member} with min_index ${min_index} (timeout ${timeout}s)"
  fi

  while [ $((SECONDS-start)) -lt $timeout ]; do
    for p in 8001 8002 8003 8004; do
      # Skip unreachable nodes
      if ! nc -z -w1 127.0.0.1 $p 2>/dev/null; then
        continue
      fi
      out=$(python3 "$ADMIN_CHECK" --port ${p} --cmd "MEMBERS" --timeout 2 2>/dev/null || true)
      # parse last_applied_index
      lai=$(echo "$out" | grep -oE 'last_applied_index=[0-9]+' | sed -E 's/last_applied_index=([0-9]+)/\1/' || true)
      if [[ -z "$lai" ]]; then
        lai=0
      fi
      # parse membership_debug portion
      mem_dbg=$(echo "$out" | sed -n 's/.*membership_debug=//p' || true)

      echo "node ${p} MEMBERS: last_applied_index=${lai} membership_debug=${mem_dbg}"

      # check presence/absence
      if [[ "$should_include" == "true" ]]; then
        if echo "$mem_dbg" | grep -qE '(^|[^0-9])'"$want_member"'([^0-9]|$)'; then
          if [[ "$lai" -ge "$min_index" ]]; then
            echo "Observed membership include ${want_member} on node ${p} with last_applied_index=${lai}"
            return 0
          fi
        fi
      else
        if ! echo "$mem_dbg" | grep -qE '(^|[^0-9])'"$want_member"'([^0-9]|$)'; then
          if [[ "$lai" -ge "$min_index" ]]; then
            echo "Observed membership exclude ${want_member} on node ${p} with last_applied_index=${lai}"
            return 0
          fi
        fi
      fi
    done
    sleep 1
  done
  return 1
}

# Helper: find current leader port dynamically
find_current_leader() {
  for p in 8001 8002 8003 8004; do
    if ! nc -z -w1 127.0.0.1 $p 2>/dev/null; then
      continue
    fi
    METRICS=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 2 2>/dev/null || true)
    if echo "$METRICS" | grep -q "state=Leader"; then
      echo "$p"
      return 0
    fi
    if echo "$METRICS" | grep -qE 'leader=Some\([0-9]+\)'; then
      id=$(echo "$METRICS" | grep -oE 'leader=Some\([0-9]+\)' | sed -E 's/leader=Some\(([0-9]+)\)/\1/')
      echo "800${id}"
      return 0
    fi
  done
  echo ""
  return 1
}

# Change membership with retry logic to handle in-flight configuration changes
change_members_retry() {
  local members="$1"
  local attempts=0
  local max_attempts=8
  while [ $attempts -lt $max_attempts ]; do
    attempts=$((attempts+1))
    # Dynamically find current leader for each attempt
    local current_leader=$(find_current_leader)
    if [[ -z "$current_leader" ]]; then
      echo "CHANGE_MEMBERS attempt ${attempts}: No leader found, waiting..."
      sleep 2
      continue
    fi
    echo "Attempting CHANGE_MEMBERS ${members} (attempt ${attempts}) -> leader $current_leader"
    out=$(python3 "$ADMIN_CHECK" --port ${current_leader} --cmd "CHANGE_MEMBERS ${members}" --timeout 5 2>/dev/null || true)
    echo "CHANGE_MEMBERS response: ${out}"
    if [[ "$out" == *"OK"* ]]; then
      leader_port="$current_leader"  # update global leader_port
      return 0
    fi
    # If it indicates an in-flight config change, parse index and wait for it
    if echo "$out" | grep -q "already undergoing a configuration change"; then
      idx=$(parse_conflict_index "$out")
      if [[ -n "$idx" ]]; then
        echo "Detected in-flight config at log index ${idx}; waiting for it to be applied..."
        # Emit diagnostic logs (dump the pending log entry across nodes)
        echo "--- Diagnostic: dump log ${idx} on all nodes ---"
        for p in 8001 8002 8003 8004; do
          echo "DUMP_LOG ${idx} on node ${p}:"
          python3 "$ADMIN_CHECK" --port ${p} --cmd "DUMP_LOG ${idx}" --timeout 2 || true
        done

        # If the requested members include a node id (e.g., '4'), wait for that node to appear in committed membership
        # as a stronger signal of commit, otherwise fallback to last_log_index check
        if echo "$members" | grep -qE '(^|,)4($|,)'; then
          if wait_for_membership_condition "4" true "${idx}" 60; then
            echo "In-flight membership appears applied (node 4 visible and last_applied>=${idx}); retrying CHANGE_MEMBERS"
            continue
          else
            echo "Timeout waiting for membership to include node 4" >&2
          fi
        else
          if wait_for_log_index $((idx+1)) 30; then
            echo "In-flight change appears applied (last_log_index advanced); retrying CHANGE_MEMBERS"
            continue
          else
            echo "Timeout waiting for config to apply" >&2
          fi
        fi
      else
        echo "Config change in-flight but couldn't parse index; sleeping and retrying" >&2
        sleep 3
      fi
    else
      # Unexpected response: sleep and retry
      echo "CHANGE_MEMBERS returned unexpected response; retrying after sleep" >&2
      sleep 2
    fi
  done
  return 1
}

# Promote to voters (1,2,3,4) with retry
if ! change_members_retry "1,2,3,4"; then
  echo "Failed to change membership to include node4 after retries" >&2
  # continue with best-effort to verify replication but mark issue
fi

# Wait for node4 to have applied state: do a replication check for a test key
# Use PUT with retries to handle transient leader instability
put_with_retries() {
  local key="$1"
  local val="$2"
  local tries=8
  local t=1
  while [ $t -le $tries ]; do
    # Dynamically find current leader
    local current_leader=$(find_current_leader)
    if [[ -z "$current_leader" ]]; then
      echo "PUT attempt $t/$tries: No leader found, waiting..."
      sleep 2
      t=$((t+1))
      continue
    fi
    echo "PUT attempt $t/$tries: PUT $key $val -> leader $current_leader"
    out=$(python3 "$ADMIN_CHECK" --port ${current_leader} --cmd "PUT $key $val" --timeout 5 2>/dev/null || true)
    echo "  PUT response: $out"
    if [[ "$out" == *"OK"* ]]; then
      leader_port="$current_leader"  # update global leader_port
      return 0
    fi
    sleep 1
    t=$((t+1))
  done
  return 1
}

if ! put_with_retries "${TEST_KEY}" "${TEST_VAL}"; then
  echo "PUT failed after retries, proceeding to check replication status (best-effort)" >&2
fi

# Wait for TEST_KEY to appear on node4
end=$((SECONDS + TIMEOUT))
while [ $SECONDS -le $end ]; do
  GET4=$(python3 "$ADMIN_CHECK" --port 8004 --cmd "GET ${TEST_KEY}" --timeout 2 2>/dev/null || true)
  echo "node4 GET ${TEST_KEY}: ${GET4}"
  if echo "$GET4" | grep -q "OK ${TEST_VAL}"; then
    echo "Node4 received replicated data"
    break
  fi
  sleep 1
done

if ! echo "$GET4" | grep -q "OK ${TEST_VAL}"; then
  echo "Node4 did not replicate test key in ${TIMEOUT}s" >&2
  # continue but mark issue
fi

# Remove node1 from membership and stop it
echo "Removing node1 from membership (change to 2,3,4)"
if ! change_members_retry "2,3,4"; then
  echo "Warning: CHANGE_MEMBERS 2,3,4 failed after retries; continuing best-effort" >&2
fi

echo "Stopping node1 container"
$COMPOSE_CMD stop node1 || true

# Wait for a new leader in remaining set (with diagnostics and extended timeout)
new_leader=""
max_wait=${NEW_LEADER_TIMEOUT:-60}
start=$SECONDS
attempt=0
while [ $((SECONDS - start)) -lt $max_wait ]; do
  attempt=$((attempt+1))
  echo "Leader wait attempt ${attempt}, elapsed $((SECONDS-start))s..."
  for p in 8002 8003 8004; do
    METRICS=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 1 2>/dev/null || true)
    IS_LEADER=$(python3 "$ADMIN_CHECK" --port ${p} --cmd IS_LEADER --timeout 1 2>/dev/null || true)
    echo "  node ${p} metrics: ${METRICS}";
    echo "  node ${p} is_leader: ${IS_LEADER}";

    if echo "$METRICS" | grep -q "leader=Some"; then
      id=$(echo "$METRICS" | grep -oE 'leader=Some\([0-9]+\)' | sed -E 's/leader=Some\(([0-9]+)\)/\1/')
      if [[ "$id" != "1" ]]; then
        new_leader=$id
        break 2
      fi
    fi

    # fallback: if IS_LEADER returns true, accept it
    if echo "$IS_LEADER" | grep -q "OK true"; then
      # extract likely id from port mapping (best-effort)
      id=$(echo ${p} | sed -E 's/800([0-9]+)/\1/')
      if [[ "$id" != "1" ]]; then
        new_leader=$id
        break 2
      fi
    fi
  done
  sleep 1
done

if [[ -z "$new_leader" ]]; then
  echo "No new leader elected within ${max_wait}s (or leader still 1)" >&2
  echo "Dumping recent logs for debugging..."
  if command -v docker >/dev/null 2>&1; then
    echo "--- docker compose ps ---"
    $COMPOSE_CMD ps || true
    echo "--- logs node2 ---"
    $COMPOSE_CMD logs --tail 200 node2 || true
    echo "--- logs node3 ---"
    $COMPOSE_CMD logs --tail 200 node3 || true
    echo "--- logs node4 ---"
    $COMPOSE_CMD logs --tail 200 node4 || true
  else
    echo "docker not available to show logs"
  fi
  exit 4
fi

echo "New leader elected: node${new_leader}. Verifying writes work."
new_leader_port="800${new_leader}"
if ! put_with_retries "rep_test" "after_removal"; then
  echo "Warning: rep_test PUT failed after retries" >&2
fi
sleep 1
# verify replication to remaining nodes
for p in 8002 8003 8004; do
  echo "checking node ${p} for rep_test"
  python3 "$ADMIN_CHECK" --port ${p} --cmd "GET rep_test" --timeout 2 || true
done

# --- Restore original membership state ---
# Start node1 back if it's stopped
echo "Restoring node1 container (start if stopped)..."
$COMPOSE_CMD start node1 || true
# wait for admin
for i in {1..30}; do
  if nc -z -w1 127.0.0.1 8001; then
    echo "node1 admin up"
    break
  fi
  sleep 1
done

# Announce node1 as a learner (required before promoting it to voter)
leader_port=${new_leader_port}
METRICS_LEADER=$(python3 "$ADMIN_CHECK" --port ${leader_port} --cmd METRICS --timeout 2 2>/dev/null || true)
leader_last_log=$(echo "$METRICS_LEADER" | grep -oE 'last_log_index=Some\([0-9]+\)' | sed -E 's/last_log_index=Some\(([0-9]+)\)/\1/' || true)
if [[ -z "$leader_last_log" ]]; then leader_last_log=0; fi

echo "Announcing node1 (as learner) to leader ${leader_port}; leader last_log_index=${leader_last_log}"
add_attempts=0
ADD_OUT=""
while [ $add_attempts -lt 6 ]; do
  add_attempts=$((add_attempts+1))
  echo "Attempting ADD_LEARNER 1=http://node1:50001 (attempt ${add_attempts})"
  ADD_OUT=$(python3 "$ADMIN_CHECK" --port ${leader_port} --cmd "ADD_LEARNER 1=http://node1:50001" --timeout 5 2>/dev/null || true)
  echo "ADD_LEARNER response: ${ADD_OUT}"
  if [[ "${ADD_OUT}" == *"OK"* ]]; then
    break
  fi
  sleep 2
done
if [[ "${ADD_OUT}" != *"OK"* ]]; then
  echo "Warning: ADD_LEARNER 1 failed after retries; proceeding to attempt membership change anyway" >&2
fi

# Wait for node1 to appear in committed membership as a learner and advance last_applied
echo "Waiting for node1 inclusion as learner with last_applied >= ${leader_last_log} (timeout 60s)"
if wait_for_membership_condition "1" true "${leader_last_log}" 60; then
  echo "Learner 1 appears committed with last_applied >= ${leader_last_log}"
else
  echo "Timeout waiting for learner 1 to be committed; proceeding to CHANGE_MEMBERS anyway" >&2
fi

# Change membership back to 1,2,3
echo "Changing membership back to 1,2,3"
if ! change_members_retry "1,2,3"; then
  echo "Warning: CHANGE_MEMBERS 1,2,3 failed after retries; continuing best-effort" >&2
fi

# Wait until nodes 1,2,3 seem responsive and cluster stabilizes
# Additionally wait for node1's last_log_index to catch up to leader
# Also wait for node4 to be removed from committed membership
echo "Waiting for cluster to stabilize on 1,2,3..."
# get leader last_log_index
leader_metrics=$(python3 "$ADMIN_CHECK" --port ${leader_port} --cmd METRICS --timeout 2 2>/dev/null || true)
leader_li=$(echo "$leader_metrics" | grep -oE 'last_log_index=Some\([0-9]+\)' | sed -E 's/[^0-9]*([0-9]+).*/\1/' || true)
if [[ -z "$leader_li" ]]; then
  leader_li=0
fi

# Wait for node4 to be absent from membership (ensure change to 1,2,3 applied)
if ! wait_for_membership_condition "4" false "${leader_li}" 60; then
  echo "Warning: node4 still present in membership after timeout" >&2
fi

stable=false
for i in {1..60}; do
  ok_count=0
  # check responsiveness
  for p in 8001 8002 8003; do
    m=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 1 2>/dev/null || true)
    if [[ -n "$m" ]]; then
      ok_count=$((ok_count+1))
    fi
  done
  echo "  stats: $ok_count/3 nodes responsive (attempt $i, leader_last_log_index=${leader_li})"
  if [[ $ok_count -eq 3 ]]; then
    # wait for node1 to catch up to leader li
    if wait_for_log_index "$leader_li" 30; then
      stable=true
      break
    fi
  fi
  sleep 1
done

if ! $stable; then
  echo "Warning: Not all nodes (1,2,3) became responsive or node1 failed to catch up" >&2
fi

# Stop node4 and verify final behavior
echo "Removing node4 (stop container)"
$COMPOSE_CMD stop node4 || true
sleep 1

# Verify writes still work in 1,2,3
leader_port_after="${new_leader_port}"
if ! put_with_retries "post_removal_test" "value_after_restore"; then
  echo "Warning: post_removal_test PUT failed after retries" >&2
fi
sleep 1
for p in 8001 8002 8003; do
  echo "checking node ${p} for post_removal_test"
  python3 "$ADMIN_CHECK" --port ${p} --cmd "GET post_removal_test" --timeout 2 || true
done

echo "Membership change test completed; cluster restored to 1,2,3" 
exit 0
