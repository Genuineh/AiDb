#!/usr/bin/env bash
set -euo pipefail

# Quick test script for membership_check fixes
# This script doesn't wait for Docker build, uses existing binary if available

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

echo "=== Quick Membership Check Test ==="
echo ""

# Step 1: Clean everything
echo "1. Cleaning all cluster data..."
docker-compose -f deploy/docker-compose.cluster.yml down -v 2>/dev/null || true
sudo rm -rf deploy/data/node{1,2,3,4} 2>/dev/null || true
echo "✓ Cleaned"
echo ""

# Step 2: Check if Docker image exists
echo "2. Checking Docker image..."
if ! docker images | grep -q "aidb.*cluster"; then
  echo "❌ Error: aidb:cluster image not found"
  echo "Please run: docker build -f deploy/Dockerfile -t aidb:cluster ."
  exit 1
fi
echo "✓ Docker image found"
echo ""

# Step 3: Run membership_check.sh
echo "3. Running membership_check.sh..."
echo "   (This will test the fixes)"
echo ""
bash deploy/membership_check.sh
EXIT_CODE=$?

echo ""
echo "=== Test Result ==="
if [ $EXIT_CODE -eq 0 ]; then
  echo "✅ SUCCESS: membership_check.sh completed successfully"
else
  echo "❌ FAILED: membership_check.sh failed with exit code $EXIT_CODE"
fi

exit $EXIT_CODE
