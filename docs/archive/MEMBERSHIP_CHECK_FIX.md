# Membership Check Script Fix Documentation

## 问题描述

`deploy/membership_check.sh` 脚本在执行时遇到以下问题：

1. **缺少前置条件检查**：脚本假设基础集群（node1、2、3）已经运行并初始化，但没有验证
2. **集群无法找到 leader**：所有节点都处于 Learner 状态，没有 voters，无法选举 leader
3. **Raft 存储一致性问题**：节点在崩溃恢复后可能处于 "有 vote 但无 logs" 的不一致状态

## 根本原因分析

### 问题1：缺少初始化检查
`membership_check.sh` 直接启动 node4 并尝试查找 leader，但没有确保基础集群（1、2、3）已经：
- 正在运行
- 已经初始化（有 voters membership）
- 已经选举出 leader

### 问题2：Raft 存储状态不一致
在 `src/cluster/raft_storage.rs` 的 `load_state()` 函数中，存在一个一致性检查逻辑：
- 当检测到有 logs 但没有 `last_applied` 时（未完成初始化的崩溃恢复场景），会清理所有未应用的 logs
- 但**没有同时清理 vote 状态**
- 这导致节点处于 `vote: T1-N1:committed, last_log_id: None` 的不一致状态
- OpenRaft 拒绝在这种状态下进行初始化，报错：`not allowed to initialize due to current raft state: last_log_id: None vote: T1-N1:committed`

## 修复方案

### 修复1：增强 membership_check.sh 前置检查

在 [deploy/membership_check.sh](../deploy/membership_check.sh) 中添加：

```bash
# Ensure base cluster (nodes 1-3) is running and initialized
echo "Checking if base cluster (nodes 1-3) is running..."
$COMPOSE_CMD ps node1 node2 node3 | grep -q "Up" || {
  echo "Base cluster not fully running. Starting nodes 1-3..."
  $COMPOSE_CMD up -d node1 node2 node3
  # Wait for admin ports...
}

# Check if base cluster needs initialization
needs_init=false
for p in 8001 8002 8003; do
  if nc -z -w1 127.0.0.1 $p 2>/dev/null; then
    METRICS=$(python3 "$ADMIN_CHECK" --port ${p} --cmd METRICS --timeout 2 2>/dev/null || true)
    if echo "$METRICS" | grep -q "state=Learner"; then
      if ! echo "$METRICS" | grep -qE 'membership:.*voters:\[[0-9]'; then
        needs_init=true
        break
      fi
    fi
  fi
done

if [ "$needs_init" = true ]; then
  echo "Initializing base cluster (1,2,3)..."
  INIT_OUT=$(python3 "$ADMIN_CHECK" --port 8001 --cmd "INIT 1=http://node1:50001,2=http://node2:50002,3=http://node3:50003" --timeout 10)
  # ...
fi
```

### 修复2：改进 init_cluster.sh 初始化检测

在 [deploy/init_cluster.sh](../deploy/init_cluster.sh) 中改进状态检测逻辑：

```bash
# Check if cluster has voters in membership (indicates initialization)
if echo "$METRICS" | grep -qE 'membership:.*voters:\[[0-9]'; then
  echo "Cluster already has voters in membership, skipping initialization."
elif echo "$METRICS" | grep -q "leader=Some"; then
  echo "Cluster already has a leader, skipping initialization."
else
  # Cluster not initialized - proceed with INIT
  echo "Cluster not initialized. Sending INIT command..."
  # ...
fi
```

### 修复3：Raft 存储一致性修复

在 [src/cluster/raft_storage.rs](../src/cluster/raft_storage.rs) 的 `load_state()` 函数中，添加 vote 清理逻辑：

```rust
// Consistency check: if we have logs but no last_applied, delete the unapplied logs
// This can happen if node crashes after initialize() but before logs are applied
if state.last_applied.is_none() && state.last_log_id.is_some() {
    tracing::warn!(
        "Found logs (last_log={:?}) but no last_applied - cleaning up unapplied logs from incomplete initialization",
        state.last_log_id
    );
    // Delete all logs
    let last_index = state.last_log_id.as_ref().unwrap().index;
    for idx in 1..=last_index {
        let key = format!("raft:log:{}", idx);
        self.db.delete(key.as_bytes())?;
    }
    // Clear last_log_id
    self.db.delete(b"raft:last_log_id")?;
    state.last_log_id = None;
    
    // 🔑 NEW: Also clear vote if we're cleaning up logs to prevent "vote without logs" state
    // which would prevent re-initialization
    if state.vote.is_some() {
        tracing::warn!(
            "Clearing vote {:?} to maintain consistency after removing unapplied logs",
            state.vote
        );
        self.db.delete(b"raft:vote")?;
        state.vote = None;
    }
}
```

## 测试验证

### 测试步骤

1. 清理所有集群数据：
   ```bash
   cd /home/jerryg/github/AiDb
   docker-compose -f deploy/docker-compose.cluster.yml down -v
   rm -rf deploy/data/node{1,2,3,4}  # May need sudo
   ```

2. 重新构建 Docker 镜像（包含修复）：
   ```bash
   docker build -f deploy/Dockerfile -t aidb:cluster .
   ```

3. 运行 membership_check.sh：
   ```bash
   bash deploy/membership_check.sh
   ```

### 预期结果

- ✅ 基础集群（1、2、3）自动启动并初始化
- ✅ 成功选举出 leader
- ✅ node4 成功加入集群并接收数据
- ✅ node1 成功从集群移除
- ✅ 剩余节点选举新 leader 并继续工作
- ✅ 集群恢复到原始状态（1、2、3）

## 相关文件

- [deploy/membership_check.sh](../deploy/membership_check.sh) - 成员变更测试脚本
- [deploy/init_cluster.sh](../deploy/init_cluster.sh) - 集群初始化脚本
- [src/cluster/raft_storage.rs](../src/cluster/raft_storage.rs) - Raft 持久化存储实现

## 进一步改进建议

1. **添加数据清理脚本**：创建 `deploy/clean_cluster.sh` 来自动化集群数据清理
2. **改进错误消息**：在脚本中提供更清晰的错误提示和恢复建议
3. **添加超时重试**：为关键操作（如 INIT、CHANGE_MEMBERS）添加更智能的重试逻辑
4. **健康检查**：在脚本开始时增加全面的集群健康检查

## 总结

本次修复解决了三个关键问题：
1. 确保 `membership_check.sh` 在运行前验证并初始化基础集群
2. 改进 `init_cluster.sh` 的初始化状态检测逻辑
3. 修复 Raft 存储在清理未应用 logs 时的一致性问题

这些修复确保了集群成员变更测试能够在各种状态下可靠运行。
