#!/usr/bin/env bash
set -euo pipefail

# replication_check.sh
# Usage: replication_check.sh [-k key] [-v value] [-t timeout_seconds]
# Writes key/value to leader, polls nodes until all return the value or timeout.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ADMIN_CHECK="${SCRIPT_DIR}/admin_check.py"
if [[ ! -f "$ADMIN_CHECK" ]]; then
  echo "Error: admin_check.py not found in ${SCRIPT_DIR}" >&2
  exit 1
fi

KEY="mykey"
VALUE="hello_replication"
TIMEOUT=${TIMEOUT:-15}
NODES=(8001 8002 8003)

while getopts ":k:v:t:n:" opt; do
  case ${opt} in
    k) KEY=${OPTARG} ;;
    v) VALUE=${OPTARG} ;;
    t) TIMEOUT=${OPTARG} ;;
    n) IFS=',' read -r -a ports <<< "${OPTARG}"; NODES=("${ports[@]}") ;;
    \?) echo "Invalid option: -${OPTARG}"; exit 2 ;;
  esac
done

echo "Replication check: key=${KEY} value=${VALUE} timeout=${TIMEOUT}s nodes=${NODES[*]}"

# Find leader
LEADER_PORT=""
for i in {1..10}; do
  echo "Searching for leader (attempt $i)..."
  for p in "${NODES[@]}"; do
    METRICS=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 1 2>/dev/null | sed -n 's/^METRICS: //p' || true)
    if [[ -n "$METRICS" && "$METRICS" =~ leader=Some\([0-9]+\) ]]; then
      id=$(echo "$METRICS" | grep -oE 'leader=Some\([0-9]+\)' | sed -E 's/leader=Some\(([0-9]+)\)/\1/')
      LEADER_PORT="800${id}"
      echo "Found leader via METRICS: node ${id} (admin ${LEADER_PORT})"
      break 2
    fi
    IS_LEADER=$(python3 "$ADMIN_CHECK" --port ${p} --cmd IS_LEADER --timeout 1 2>/dev/null | sed -n 's/^IS_LEADER: //p' || true)
    if [[ "$IS_LEADER" == "OK true" ]]; then
      LEADER_PORT=${p}
      echo "Found leader via IS_LEADER: admin ${LEADER_PORT}"
      break 2
    fi
  done
  sleep 1
done

if [[ -z "$LEADER_PORT" ]]; then
  echo "ERROR: no leader found" >&2
  exit 3
fi

# PUT to leader
echo "Putting ${KEY}=${VALUE} to leader ${LEADER_PORT}..."
PUT_RESP=$(python3 "$ADMIN_CHECK" --port ${LEADER_PORT} --cmd "PUT ${KEY} ${VALUE}" --timeout 3 2>/dev/null || true)
echo "PUT response: ${PUT_RESP}"

# Poll GET on each node until all return value or until timeout
end=$((SECONDS + TIMEOUT))
declare -A results
for p in "${NODES[@]}"; do results[$p]="pending"; done

while [ $SECONDS -le $end ]; do
  all_ok=true
  for p in "${NODES[@]}"; do
    if [[ "${results[$p]}" == "ok" ]]; then
      continue
    fi
    GET_RESP=$(python3 "$ADMIN_CHECK" --port ${p} --cmd "GET ${KEY}" --timeout 3 2>/dev/null || true)
    echo "Node ${p} GET ${KEY}: ${GET_RESP}"
    # Strip leading "<cmd>:\s" prefix (handles both "GET:" and "GET mykey:")
    GET_BODY=$(echo "$GET_RESP" | sed -n 's/.*: //p' || true)
    if [[ "$GET_BODY" =~ ^OK\ (.*)$ ]]; then
      val=$(echo "$GET_BODY" | sed -n 's/^OK \(.*\)$/\1/p')
      if [[ "$val" == "None" ]]; then
        # Try state-machine prefixed key (sm:<key>) used by Raft storage
        SM_KEY="sm:${KEY}"
        GET_RESP2=$(python3 "$ADMIN_CHECK" --port ${p} --cmd "GET ${SM_KEY}" --timeout 3 2>/dev/null || true)
        echo "Node ${p} GET ${SM_KEY}: ${GET_RESP2}"
        GET_BODY2=$(echo "$GET_RESP2" | sed -n 's/.*: //p' || true)
        if [[ "$GET_BODY2" =~ ^OK\ (.*)$ ]]; then
          val2=$(echo "$GET_BODY2" | sed -n 's/^OK \(.*\)$/\1/p')
          if [[ "$val2" == "$VALUE" ]]; then
            results[$p]="ok"
            echo "    -> marked node ${p} ok"
            continue
          else
            results[$p]="mismatch(${val2})"
            all_ok=false
            continue
          fi
        else
          results[$p]="none"
          all_ok=false
          continue
        fi
      else
        if [[ "$val" == "$VALUE" ]]; then
          results[$p]="ok"
          continue
        else
          results[$p]="mismatch(${val})"
          all_ok=false
          continue
        fi
      fi
    fi

    # If we reach here, there was no OK response
    # Try state-machine prefixed key anyway
    SM_KEY="sm:${KEY}"
    GET_RESP2=$(python3 "$ADMIN_CHECK" --port ${p} --cmd "GET ${SM_KEY}" --timeout 3 2>/dev/null || true)
    echo "Node ${p} GET ${SM_KEY}: ${GET_RESP2}"
    GET_BODY2=$(echo "$GET_RESP2" | sed -n 's/.*: //p' || true)
    if [[ "$GET_BODY2" =~ ^OK\ (.*)$ ]]; then
      val2=$(echo "$GET_BODY2" | sed -n 's/^OK \(.*\)$/\1/p')
      if [[ "$val2" == "$VALUE" ]]; then
        results[$p]="ok"
        continue
      else
        results[$p]="mismatch(${val2})"
        all_ok=false
        continue
      fi
    fi

    all_ok=false
    if [[ "$GET_RESP" == *"OK None"* || "$GET_RESP2" == *"OK None"* ]]; then
      results[$p]="none"
    else
      results[$p]="err"
    fi
  done

  if $all_ok; then
    echo "SUCCESS: All nodes have key ${KEY}=${VALUE}"
    for p in "${NODES[@]}"; do echo "  node ${p}: ${results[$p]}"; done
    exit 0
  fi
  sleep 1
done

# Timeout
echo "TIMEOUT: Not all nodes replicated within ${TIMEOUT}s"
for p in "${NODES[@]}"; do echo "  node ${p}: ${results[$p]}"; done
exit 4
