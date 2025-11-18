# AiDb 运维命令速查表

> 快速查找常用的 aidb-admin 命令

## 🚀 一键命令（推荐）

| 命令 | 说明 | 示例 |
|------|------|------|
| `quick-start` | 一键启动集群 | `aidb-admin quick-start` |
| `quick-stop` | 一键停止集群 | `aidb-admin quick-stop` |
| `quick-scale` | 一键扩缩容 | `aidb-admin quick-scale --add-replicas 2` |
| `quick-backup` | 一键备份 | `aidb-admin --db ./data/primary1 quick-backup` |
| `quick-restore` | 一键恢复 | `aidb-admin quick-restore` |
| `quick-check` | 一键健康检查 | `aidb-admin quick-check` |

## 📊 集群管理

| 命令 | 说明 | 示例 |
|------|------|------|
| `cluster status` | 查看集群状态 | `aidb-admin cluster status` |
| `cluster nodes` | 列出所有节点 | `aidb-admin cluster nodes` |
| `cluster shards` | 列出所有分片 | `aidb-admin cluster shards` |
| `cluster add-node` | 添加节点 | `aidb-admin cluster add-node --address 127.0.0.1:50060 --node-type replica` |
| `cluster remove-node` | 移除节点 | `aidb-admin cluster remove-node node3 --force` |

## 💾 备份管理

| 命令 | 说明 | 示例 |
|------|------|------|
| `backup create` | 创建备份 | `aidb-admin --db ./data/primary1 backup create --output /backups --compress` |
| `backup list` | 列出备份 | `aidb-admin backup list --path /backups` |
| `backup restore` | 恢复备份 | `aidb-admin backup restore --backup /backups/20251118 --target ./data/restored --force` |
| `backup delete` | 删除备份 | `aidb-admin backup delete /backups/old -y` |

## 📈 统计和监控

| 命令 | 说明 | 示例 |
|------|------|------|
| `stats` | 查看数据库统计 | `aidb-admin --db ./data/primary1 stats` |
| `stats --detailed` | 详细统计 | `aidb-admin --db ./data/primary1 stats --detailed` |
| `health` | 健康检查 | `aidb-admin --db ./data/primary1 health` |
| `metrics` | 查看指标 | `aidb-admin metrics` |
| `metrics --watch` | 持续监控 | `aidb-admin metrics --watch 5` |

## 🔧 常用参数

| 参数 | 说明 | 示例 |
|------|------|------|
| `-v, --verbose` | 启用详细日志 | `aidb-admin -v quick-start` |
| `-d, --db <PATH>` | 指定数据库路径 | `aidb-admin --db ./data/primary1 stats` |
| `-h, --help` | 显示帮助 | `aidb-admin --help` |
| `-V, --version` | 显示版本 | `aidb-admin --version` |

## 💡 常用场景

### 启动新集群

```bash
# 1. 安装工具
cargo install --path . --features admin-cli --bin aidb-admin

# 2. 启动集群
aidb-admin quick-start

# 3. 验证
aidb-admin quick-check
```

### 日常备份

```bash
# 创建备份
aidb-admin --db ./data/primary1 quick-backup

# 查看备份列表
aidb-admin backup list --path ./backups

# 删除旧备份（保留最近 7 天）
find ./backups -type d -mtime +7 | xargs -I {} aidb-admin backup delete {} -y
```

### 扩容集群

```bash
# 查看当前状态
aidb-admin cluster status

# 添加 Replica 节点
aidb-admin quick-scale --add-replicas 2

# 验证新节点
aidb-admin cluster nodes
```

### 性能诊断

```bash
# 1. 快速健康检查
aidb-admin quick-check

# 2. 查看详细统计
aidb-admin --db ./data/primary1 stats --detailed

# 3. 持续监控指标
aidb-admin metrics --watch 5

# 4. 检查缓存命中率
aidb-admin --db ./data/primary1 stats | grep "Cache Hit Rate"
```

### 灾难恢复

```bash
# 1. 停止集群
aidb-admin quick-stop --force

# 2. 恢复最新备份
aidb-admin quick-restore --latest

# 3. 验证数据
aidb-admin --db ./data/restored quick-check

# 4. 重新启动
aidb-admin quick-start --data-dir ./data/restored
```

## 🔐 安全提示

- ⚠️ `quick-stop --clean` 会删除所有数据，谨慎使用
- ⚠️ `backup restore --force` 会覆盖现有数据
- ⚠️ `cluster remove-node --force` 跳过安全检查
- ✅ 重要操作前先做备份：`aidb-admin quick-backup`

## 📱 快速联系

- 文档：`docs/FOOLPROOF_OPS_GUIDE.md`
- 帮助：`aidb-admin <command> --help`
- Issues：https://github.com/yourusername/aidb/issues

---

**提示**：将此文档打印或保存为书签，方便日常运维查阅。
