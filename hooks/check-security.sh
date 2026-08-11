#!/bin/bash
# 安全检查: cargo audit (CVE) + cargo deny check (许可证/来源/重复版本)
# 与 CI security.yml 对齐: 两者都基于 Cargo.lock 全量扫描, 天然覆盖所有 feature 依赖,
# 因此不需要 --all-features (cargo deny check 也不支持该参数).
#
# 逃生门: 依赖漏洞修复前 (见 TEMP-RECORD-BUG.md), 可用 SKIP_SECURITY=1 跳过本检查.
#   用法: SKIP_SECURITY=1 git commit ...   (仅跳过 security, fmt/clippy 仍执行)
set -uo pipefail

# 逃生门: 显式跳过 security 检查 (应急用, 修复依赖漏洞后应移除)
if [ "${SKIP_SECURITY:-0}" = "1" ]; then
    echo "SKIP: SKIP_SECURITY=1, 跳过 security 检查 (应急逃生门, 修复依赖漏洞后应移除)."
    exit 0
fi

failed=0

# cargo-audit 未安装时跳过并提示 (CI 用 taiki-e/install-action 自动装, 本地需手动)
if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "WARN: 未找到 cargo-audit, 跳过 audit 检查. 安装: cargo install cargo-audit --locked"
else
    echo "=== cargo audit (CVE) ==="
    cargo audit || failed=1
fi

# cargo-deny 未安装时跳过并提示
if ! command -v cargo-deny >/dev/null 2>&1; then
    echo "WARN: 未找到 cargo-deny, 跳过 deny 检查. 安装: cargo install cargo-deny --locked"
else
    echo "=== cargo deny check (许可证/来源/重复版本) ==="
    cargo deny check || failed=1
fi

if [ "$failed" -ne 0 ]; then
    echo "ERROR: security 检查未通过 (见上方输出). 依赖漏洞修复前可临时 SKIP_SECURITY=1 git commit 跳过."
    exit 1
fi

echo "Security checks passed."
