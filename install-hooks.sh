#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

for hook in hooks/*; do
    hook_name=$(basename "$hook")
    ln -sf "../../$hook" ".git/hooks/$hook_name"
    chmod +x "$hook"
done

echo "Git hooks installed."
