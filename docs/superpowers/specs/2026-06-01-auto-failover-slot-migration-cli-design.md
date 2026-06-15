# Auto Failover & Slot Migration CLI Visibility Design

**Date**: 2026-06-01
**Status**: Approved
**Scope**: AiDb (lib) + AiKv (bin)

## 1. Summary

经过全面代码审查, AiDb 集群功能已基本完整. 本次设计仅填补两个可观测性/运维缺口:

1. **Group 级自动 Failover**: 基于 Raft 原生 leader 选举, 补齐"配置暴露 + 变更检测 + 路由更新"这一层
2. **槽迁移进度可见性**: CLUSTER SLOTS 响应中标记 migrating/importing 状态, 兼容 redis-cli --cluster check

不增加新协议、新算法、新 state machine 操作.

## 2. 设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| Failover 粒度 | Group 级别 | 复用 Raft 原生选举, 每 Group 独立切换 |
| 故障超时配置 | 复用 `election_timeout_min/max` | Raft 已保证安全性, 零额外复杂度 |
| 路由更新方式 | 被动 (MOVED 重定向) | Raft ForwardToLeader 已处理转发 |
| 备份恢复 | 维持当前单节点方案 | Raft 日志复制保证恢复后追上集群 |
| 槽迁移 | 仅补充 CLI 可见性 | 迁移引擎已完整实现 |

## 3. Feature 1: Group 级自动 Failover

### 3.1 当前状态

- OpenRaft 在 Group 内自动进行 leader 选举 (election_timeout 到期后 follower 发起投票)
- `RaftNodeConfig` 已定义 `election_timeout_min`/`max`/`heartbeat_interval`
- `AiKv main.rs` 硬编码了这些值, 未暴露为 CLI 参数
- 没有后台任务监听 leader 变更并更新 MetaRaft 路由表

### 3.2 设计

#### 3.2.1 CLI 参数 (AiKv)

```bash
aikv \
  --engine aidb --data-dir /data \
  --cluster-node-id 1 --cluster-rpc-addr 127.0.0.1:7001 \
  --raft-election-timeout-min 3000 \   # 新增, 默认 500ms
  --raft-election-timeout-max 6000      # 新增, 默认 1000ms
```

新增字段在 `Args` struct 中 (feature-gated).

#### 3.2.2 LeaderChangeWatcher (AiDb 新增模块)

```
AiDb/src/cluster/leader_watcher.rs
```

核心逻辑:

```rust
pub struct LeaderChangeWatcher {
    node_id: NodeId,
    multi_raft: Arc<MultiRaftNode>,
    meta_raft: Arc<MetaRaftNode>,
    leader_cache: RwLock<HashMap<u64, Option<NodeId>>>, // group_id → leader_node_id
    tick_interval: Duration,
}

impl LeaderChangeWatcher {
    /// 执行一次检测 tick, 返回发生 leader 变更的 Group ID 列表
    pub async fn tick(&self) -> Vec<u64> {
        let groups = self.multi_raft.get_groups().read();
        let mut changed = Vec::new();

        for (gid, node) in groups.iter() {
            let current_leader = node.get_leader().await;
            let prev_leader = self.leader_cache.read().get(gid).copied().flatten();

            if current_leader != prev_leader {
                // 1. 更新本地缓存
                self.leader_cache.write().insert(*gid, current_leader);

                // 2. 通过 MetaRaft 更新 ReplicaInfo.is_leader 标志
                if let Some(leader_id) = current_leader {
                    self.meta_raft.propose(MetaRequest::ChangeGroupMembership {
                        group_id: *gid,
                        new_replicas: self.build_replica_info(*gid, leader_id),
                        config_version: self.current_config_version(*gid) + 1,
                    }).await.ok();
                }

                changed.push(*gid);
                tracing::info!(group_id = *gid, ?prev_leader, ?current_leader, "leader changed");
            }
        }
        changed
    }
}
```

**模块接口**:

- `LeaderChangeWatcher::new(node_id, multi_raft, meta_raft, tick_interval)` — 创建
- `LeaderChangeWatcher::tick() → Vec<u64>` — 单次检测, 返回变更的 Group IDs
- `LeaderChangeWatcher::run(shutdown_rx) → JoinHandle<()>` — 后台循环, 每隔 tick_interval 执行一次 tick, 接收 shutdown 信号退出

**可观测性**:

```rust
#[instrument(name = "leader_watch_tick", skip(self), fields(node_id = %self.node_id))]
pub async fn tick(&self) -> Vec<u64> { ... }
```

- `tracing::info!` 记录每次 leader 变更 (group_id, prev_leader, current_leader)
- `tracing::debug!` 记录每次 tick (group_count, no_change_count)
- Metric: `aikv_failover_total` (已存在, 每次 leader 变更时调用 `metrics.on_failover()`)

#### 3.2.3 AiKv 集成

在 `init_cluster()` 中:

```rust
// 17. 启动 LeaderChangeWatcher
let leader_watcher = LeaderChangeWatcher::new(
    node_id,
    multi_raft.clone(),
    meta_raft.clone(),
    Duration::from_millis(raft_config.election_timeout_min / 2), // tick 频率 = 超时的一半
);
let shutdown_rx = multi_raft.start_lifecycle(...); // 复用现有 shutdown 信号
tokio::spawn(async move {
    leader_watcher.run(shutdown_rx).await;
});
```

### 3.3 MetaRequest 扩展

无需新增 MetaRequest variant. 复用现有的 `ChangeGroupMembership`:

```rust
MetaRequest::ChangeGroupMembership {
    group_id,
    new_replicas: Vec<(NodeId, bool)>,  // (node_id, is_leader)
    config_version,
}
```

当前 `new_replicas[0]` 被约定为 leader (is_leader=true). LeaderChangeWatcher 在检测到变更后构造正确的 `new_replicas` 列表.

### 3.4 约束

- `tick_interval` 必须小于 `election_timeout_min`, 否则可能在两次 tick 之间错过 leader 变更. 默认取 `election_timeout_min / 2`.
- Watcher 只在 leader 变更时才调用 MetaRaft propose, 避免常态下的写入压力
- 若 MetaRaft 本身正在选举中 (无 leader), propose 会失败; Watcher 应吞掉此错误并在下次 tick 重试

## 4. Feature 2: 槽迁移 CLI 可见性

### 4.1 当前状态

- `SlotMigrationManager` 完整实现了迁移流程
- `MetaStateMachine` 在 `SlotMigrationState` 中记录了迁移状态
- `CLUSTER SLOTS` 响应只包含已分配的 slot 范围, 不标记 migrating/importing
- `CLUSTER INFO` 中 `cluster_slots_migrating` 字段缺失 (始终为 0)

### 4.2 设计

#### 4.2.1 CLUSTER SLOTS 增强

当前格式 (Redis 兼容):

```
[start, end, [ip, port, node_id], [replica_ip, replica_port, replica_id], ...]
```

增强后, 对迁移中的 slot 追加 migrating/importing 节点信息:

```
// importing slot (目标节点视角)
[start, end, [dst_ip, dst_port, dst_node_id], [src_ip, src_port, src_node_id]]

// migrating slot (源节点视角)  
[start, end, [src_ip, src_port, src_node_id], [dst_ip, dst_port, dst_node_id]]
```

实现方式: 在 `cluster_slots()` 函数中, 构建 slot 范围后查询 `meta_raft.get_migration_state()`. 若当前节点涉及迁移, 在对应的 slot 范围的第三个元素之后追加第四个元素.

#### 4.2.2 CLUSTER INFO 增强

新增 `cluster_slots_migrating:N` 字段, 统计 `SlotStatus::Migrating(_)` 的 slot 数量.

```rust
let migrating_count = slot_table
    .iter()
    .filter(|s| matches!(s, SlotStatus::Migrating(_)))
    .count();
```

#### 4.2.3 CLUSTER NODES 增强

对涉及迁移的节点, 在 flags 中追加 migrating/importing 标记:

```
// 源节点 (migrating)
<id> <ip>:<port>@<cport> myself,master,migrating - 0 0 0 <epoch> connected

// 目标节点 (importing)
<id> <ip>:<port>@<cport> master,importing - 0 0 0 <epoch> connected
```

### 4.3 涉及文件

仅 `AiKv/src/cluster/commands.rs`:

- `cluster_slots()` — 约 +30 行
- `cluster_info()` — 约 +3 行
- `cluster_nodes()` — 约 +15 行

无需改动 AiDb.

## 5. 文件变更清单

| 仓库 | 文件 | 操作 | 估算行数 |
|------|------|------|----------|
| AiDb | `src/cluster/leader_watcher.rs` | 新增 | ~120 |
| AiDb | `src/cluster/mod.rs` | 编辑 (+2) | 2 |
| AiDb | `src/lib.rs` | 编辑 (+1 re-export) | 1 |
| AiKv | `src/main.rs` | 编辑 (+CLI args + watcher 启动) | ~40 |
| AiKv | `src/cluster/commands.rs` | 编辑 (SLOTS/INFO/NODES) | ~50 |

总估算: ~213 行新增/修改.

## 6. 测试计划

### 6.1 单元测试 (AiDb)

- `leader_watcher::tests::test_no_change_when_leader_stable` — 缓存命中, 无变更
- `leader_watcher::tests::test_detect_leader_change` — 模拟 leader 切换, 验证 tick 返回变更 Group
- `leader_watcher::tests::test_multiple_groups_independent` — 多 Group 独立检测

### 6.2 集成测试 (AiDb)

- `test_leader_change_updates_meta_raft` — 端到端: leader 切换 → Watcher 检测 → MetaRaft propose 成功

### 6.3 CLI 测试 (AiKv)

- `test_cluster_slots_shows_migrating_state` — 验证迁移中的 slot 在 CLUSTER SLOTS 中有正确标记
- `test_cluster_info_counts_migrating_slots` — 验证 cluster_slots_migrating 字段
- `test_cluster_nodes_shows_migrating_flags` — 验证 migrating/importing flags

### 6.4 E2E 增强

- `test_cluster_failover.sh` — 增强现有脚本: kill leader 节点后验证数据可读写 (不仅仅是 PING 存活)

## 7. 不做的事项

| 事项 | 原因 | 替代方案 |
|------|------|----------|
| PFAIL→FAIL 两阶段故障检测 | 复杂度高 | Raft election timeout 已足够 |
| 主动广播 leader 变更 | 增加网络开销 | MOVED 重定向被动发现 |
| 集群级协调备份 | 跨节点协调复杂 | 单节点 backup + Raft 日志追数据 |
| `CLUSTER REBALANCE` | 均衡算法 + 自动编排工作量大 | 手动 SETSLOT MIGRATING 已可用 |
| 新 gossip 协议 | 与现有设计重复 | MetaRaft 心跳已覆盖故障检测 |
| `failover_timeout` 独立配置 | YAGNI | 复用 election_timeout |

## 8. 可观测性

- **LeaderChangeWatcher**: 每次 tick 输出 `tracing::debug!` (group_count, no_change); 每次 leader 变更输出 `tracing::info!` (group_id, prev_leader, new_leader)
- **Metric**: 复用现有 `aikv_failover_total` counter, 每次 leader 变更 +1
- **CLUSTER INFO**: 新增 `cluster_slots_migrating` 字段
- **tracing span**: LeaderChangeWatcher 方法标记 `#[instrument]`
