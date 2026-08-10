#!/usr/bin/env bash
# 幂等创建 GitHub labels (需 gh 已登录且有 repo 权限)
set -euo pipefail
repo="${1:?用法: ./setup-labels.sh <owner/repo>}"
# 类型 labels
gh label create bug --repo "$repo" --color d73a4a --description "缺陷" --force
gh label create feature --repo "$repo" --color a2eeef --description "新功能" --force
gh label create refactor --repo "$repo" --color d4c5f9 --description "重构 / 技术债" --force
gh label create docs --repo "$repo" --color 0075ca --description "文档" --force
gh label create perf --repo "$repo" --color 0e8a16 --description "性能优化" --force
# 状态 labels
gh label create ready --repo "$repo" --color 7057ff --description "待开发" --force
gh label create in-progress --repo "$repo" --color fbca04 --description "开发中" --force
gh label create blocked --repo "$repo" --color 000000 --description "被阻塞" --force
echo "labels 已就绪"
