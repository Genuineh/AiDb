#!/bin/bash
# 安全检查: cargo audit (CVE) + cargo deny check (许可证/来源/重复版本)
# 与 CI security.yml 对齐: 两者都基于 Cargo.lock 全量扫描, 天然覆盖所有 feature 依赖,
# 因此不需要 --all-features (cargo deny check 也不支持该参数).
set -uo pipefail

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
    echo "ERROR: security 检查未通过 (见上方输出)."
    exit 1
fi

echo "Security checks passed."
