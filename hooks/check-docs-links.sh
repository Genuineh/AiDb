#!/bin/bash
# 文档链接检查: 校验 .md 文件中的相对链接指向的本地文件存在
# 用法: 无参数 = 检查 staged .md; --all = 检查全部已跟踪 .md (基线扫描用)
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)

if [ "${1:-}" = "--all" ]; then
    targets=$(git ls-files '*.md')
else
    targets=$(git diff --cached --name-only --diff-filter=ACM | grep '\.md$' || true)
fi
if [ -z "$targets" ]; then
    exit 0
fi

broken=0
while IFS= read -r file; do
    [ -z "$file" ] && continue
    dir=$(dirname "$file")
    while IFS= read -r link; do
        link="${link%%#*}"  # 去掉锚点部分
        case "$link" in
            http://*|https://*|mailto:*|/*|"") continue ;;  # 外链/绝对路径/空
        esac
        # 解析相对链接 (含 ../), 判定是否越出仓库根; 越出则跳过 (跨仓链接在 sibling 布局下存在)
        target=$(realpath -m "$dir/$link" 2>/dev/null || echo "$dir/$link")
        case "$target" in
            "$repo_root"/*) ;;
            *) continue ;;
        esac
        if [ ! -e "$target" ]; then
            echo "BROKEN LINK: $file -> $link"
            broken=1
        fi
    done < <(grep -oE '\[[^]]*\]\([^)]*\)' "$file" | sed -E 's/.*\]\((.*)\)/\1/')
done <<< "$targets"

if [ "$broken" -ne 0 ]; then
    echo "ERROR: 存在失效文档链接, 请修复后再提交."
    exit 1
fi

exit 0
