---
name: releasing
description: 用于说明 AiDb crate 的手工验证, 候选版本和正式发布流程
---

# AiDb 发布手册

本手册适用于向 crates.io 发布 AiDb 稳定版本. 发布过程由维护者手工执行, crate
一旦发布便不可覆盖或撤换内容.

## 1. 准备发布内容

1. 确认 `Cargo.toml`, `Cargo.lock`, `README.md` 和 `CHANGELOG.md` 使用同一版本.
2. 确认 `CHANGELOG.md` 已将本次内容从 `[Unreleased]` 归入带 UTC 日期的版本节.
3. 检查 package 清单, 确认不包含本地配置, 过程制品或敏感文件:

   ```bash
   cargo package --list
   ```

4. 在 release commit 前可按第 3.2 至 3.6 节的顺序复用 package,
   feature matrix, 文档, consumer smoke 和 dry-run 检查. 预提交阶段的
   参数差异见对应步骤.

`--allow-dirty` 只用于预提交检查, 不得用于最终门禁或正式发布.

## 2. 创建 release commit 和 RC Tag

1. 完成审核后创建 release commit.
2. 从该 release commit 创建 RC Tag, 例如 `v1.0.0-rc.1`.
3. 由用户 push release commit 和 RC Tag.

最终门禁只能在 release commit, RC Tag 和用户 push 均完成后执行. 执行时工作区
必须干净, 且所有命令不得带 `--allow-dirty`.

## 3. 执行最终门禁

按照以下顺序验证 RC Tag 对应的源码.

### 3.1 确认 release commit 和 RC Tag

```bash
rc_tag="${RC_TAG:?请设置 RC_TAG, 例如 v1.0.0-rc.1}"
head_sha="$(git rev-parse HEAD)"
rc_commit_sha="$(git rev-parse "${rc_tag}^{commit}")"
remote_rc_sha="$(git ls-remote --tags origin "refs/tags/${rc_tag}^{}" | awk 'NR == 1 { print $1 }')"
test -n "$remote_rc_sha"
test "$head_sha" = "$rc_commit_sha"
test "$head_sha" = "$remote_rc_sha"
test -z "$(git status --porcelain)"
```

以上检查确保当前 `HEAD`, 动态指定的 RC Tag commit 和远端 annotated RC Tag 的
peeled SHA 完全一致, 且工作区干净. 任一检查失败时不得继续最终门禁.

### 3.2 检查 package 清单并构建 package

```bash
cargo package --list
cargo package
```

后续验证应针对 `cargo package` 在 `target/package/aidb-<version>/` 生成的
package 源码, 而不是工作区中的额外文件.

预提交阶段复用此步骤时, `cargo package` 可改用
`cargo package --allow-dirty`.

### 3.3 验证 package feature matrix

在已安装 `protoc` 的环境中依次执行:

```bash
cargo check --manifest-path target/package/aidb-<version>/Cargo.toml --no-default-features
cargo check --manifest-path target/package/aidb-<version>/Cargo.toml --no-default-features --features backup
cargo check --manifest-path target/package/aidb-<version>/Cargo.toml --no-default-features --features compression
cargo check --manifest-path target/package/aidb-<version>/Cargo.toml --no-default-features --features cluster
cargo check --manifest-path target/package/aidb-<version>/Cargo.toml --no-default-features --features monitoring
cargo check --manifest-path target/package/aidb-<version>/Cargo.toml --all-features
```

将 `<version>` 替换为待发布版本, 例如 `1.0.0`.

### 3.4 验证 package 文档

```bash
package_dir="target/package/aidb-<version>"
test -f "$package_dir/README.md"
test -f "$package_dir/SECURITY.md"
test -f "$package_dir/RELEASING.md"
test -f "$package_dir/LICENSE-MIT"
test -f "$package_dir/LICENSE-APACHE"
```

### 3.5 执行 consumer smoke

在仓库外创建临时 consumer crate, 仅依赖 package 目录, 并验证最小公共 API:

```bash
consumer_dir="$(mktemp -d)"
cargo init --quiet --bin "$consumer_dir"
cat >> "$consumer_dir/Cargo.toml" <<EOF
aidb = { path = "$(pwd)/target/package/aidb-<version>" }
EOF
cat > "$consumer_dir/src/main.rs" <<'EOF'
use aidb::{config::Options, DB};

fn main() -> aidb::Result<()> {
    let dir = std::env::var_os("AIDB_CONSUMER_DATA_DIR")
        .expect("AIDB_CONSUMER_DATA_DIR must be set");
    let db = DB::open(&dir, Options::default())?;
    db.put(b"key", b"value")?;
    assert_eq!(db.get(b"key")?, Some(b"value".to_vec()));
    db.close()
}
EOF
AIDB_CONSUMER_DATA_DIR="$consumer_dir/data" \
    cargo run --quiet --manifest-path "$consumer_dir/Cargo.toml"
```

consumer crate 和本次独立数据目录均位于 `$consumer_dir` 下. 完成并记录结果后
可删除该临时目录.

### 3.6 执行 publish dry-run

package 清单, feature matrix, 文档和 consumer smoke 全部通过后执行:

```bash
cargo publish --dry-run
```

预提交阶段复用此步骤时, dry-run 可改用
`cargo publish --dry-run --allow-dirty`.

最终门禁的任何命令均不得使用 `--allow-dirty`. 发布负责人应记录 package 清单,
feature matrix, 文档, consumer smoke 和 dry-run 的结果.

## 4. 发布 crate

仅在最终门禁全部通过后执行:

```bash
cargo publish
```

等待 crates.io 显示新版本, 并确认该版本可由独立 consumer 正常解析后, 再创建
指向同一 release commit 的 final Tag, 例如 `v1.0.0`, 由用户 push final Tag.

完整发布顺序不可调整:

1. 完成发布内容和预提交 package 检查.
2. 创建 release commit 和 RC Tag, 再由用户 push.
3. 确认 `HEAD`, RC Tag commit 和远端 peeled Tag SHA 一致.
4. 执行最终 package 清单和构建检查.
5. 验证最终 package feature matrix.
6. 验证 package 文档.
7. 执行最终 consumer smoke.
8. 执行 `cargo publish --dry-run`.
9. 执行 `cargo publish`.
10. crates.io 版本验证通过后创建并由用户 push final Tag.

## 5. 发布后修正

crates.io 上已发布的 crate 版本不可覆盖. 若已发布内容需要修正, 必须提升补丁版本
(例如从 `1.0.0` 提升到 `1.0.1`), 重新更新 Changelog, 并完整执行本手册流程.
