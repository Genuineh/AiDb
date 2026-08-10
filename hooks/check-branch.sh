#!/bin/bash
# 分支保护: 禁止在基础分支上直接提交
set -euo pipefail

# 可被 pre-commit 调用, 或直接运行并传入分支名模拟测试
branch="${1:-$(git rev-parse --abbrev-ref HEAD)}"
protected="^(new/main|main|new/wiqun)$"

if echo "$branch" | grep -qE "$protected"; then
    echo "ERROR: 禁止在基础分支 '$branch' 上直接提交."
    echo "请先创建功能分支: git switch -c <branch-name>"
    echo "如需强行提交 (不推荐): git commit --no-verify"
    exit 1
fi

exit 0
