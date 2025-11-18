# AiDb 傻瓜式运维指南

本指南专为运维人员设计，通过**命令行工具 aidb-admin** 提供**一键式**集群管理，无需深入了解技术细节即可完成日常运维工作。

## 📋 目录

- [工具安装](#工具安装)
- [快速开始](#快速开始)
- [一键命令](#一键命令)
  - [quick-start - 一键启动集群](#quick-start---一键启动集群)
  - [quick-stop - 一键停止集群](#quick-stop---一键停止集群)
  - [quick-scale - 一键扩缩容](#quick-scale---一键扩缩容)
  - [quick-backup - 一键备份](#quick-backup---一键备份)
  - [quick-restore - 一键恢复](#quick-restore---一键恢复)
  - [quick-check - 一键健康检查](#quick-check---一键健康检查)
- [进阶命令](#进阶命令)
- [常见问题](#常见问题)
- [运维技巧](#运维技巧)

---

## 🚀 工具安装

### 方法一：从源码编译（推荐）

```bash
# 1. 克隆仓库
git clone https://github.com/yourusername/aidb.git
cd aidb

# 2. 安装 protobuf 编译器（如果未安装）
sudo apt-get install protobuf-compiler

# 3. 编译并安装管理工具
cargo install --path . --features admin-cli --bin aidb-admin

# 4. 验证安装
aidb-admin --version
aidb-admin --help
```

### 方法二：使用预编译的二进制文件

```bash
# 下载二进制文件
wget https://github.com/yourusername/aidb/releases/latest/download/aidb-admin

# 添加执行权限
chmod +x aidb-admin

# 移动到系统路径
sudo mv aidb-admin /usr/local/bin/

# 验证安装
aidb-admin --version
```

---

## ⚡ 快速开始

最快 5 分钟搭建一个 AiDb 集群：

```bash
# 1. 一键启动集群（默认：1 个 Primary + 2 个 Replica）
aidb-admin quick-start

# 2. 查看集群状态
aidb-admin quick-check

# 3. 查看节点列表
aidb-admin cluster nodes

# 完成！现在你已经有一个运行中的集群了
```

---

## 🎯 一键命令

### `quick-start` - 一键启动集群

**功能**：自动启动完整的 AiDb 集群，包括 Primary 和 Replica 节点。

**基础用法**：
```bash
# 使用默认配置启动（1 个 Primary + 2 个 Replica）
aidb-admin quick-start
```

**自定义配置**：
```bash
# 启动多个 Primary 节点
aidb-admin quick-start --primaries 2

# 每个 Primary 启动更多 Replica
aidb-admin quick-start --replicas 4

# 完整自定义
aidb-admin quick-start \
  --primaries 2 \
  --replicas 3 \
  --data-dir /data/aidb \
  --start-port 60000
```

**参数说明**：
- `-p, --primaries <N>`: Primary 节点数量（默认：1）
- `-r, --replicas <N>`: 每个 Primary 的 Replica 数量（默认：2）
- `--data-dir <PATH>`: 数据目录（默认：./data）
- `--start-port <PORT>`: 起始端口号（默认：50051）

**示例输出**：
```
🚀 启动 AiDb 集群...
================================================

📋 集群配置：
  Primary 节点数: 1
  每个 Primary 的 Replica 数: 2
  数据目录: ./data
  起始端口: 50051

确认启动集群？ [y/N]: y

📂 步骤 1/4: 创建数据目录...
   ✓ Primary-1 数据目录已创建
   ✓ Replica-1 数据目录已创建
   ✓ Replica-2 数据目录已创建

🔧 步骤 2/4: 启动 Primary 节点...
   💡 提示: Primary 节点负责存储完整数据
   ✓ Primary-1 启动成功 (127.0.0.1:50051)

🔧 步骤 3/4: 启动 Replica 节点...
   💡 提示: Replica 节点提供缓存和读取加速
   ✓ Replica-1 启动成功 (127.0.0.1:50052)
   ✓ Replica-2 启动成功 (127.0.0.1:50053)

🏥 步骤 4/4: 健康检查...
   ✓ 所有节点健康

================================================
✅ 集群启动完成！
================================================
```

---

### `quick-stop` - 一键停止集群

**功能**：优雅停止所有集群节点，确保数据安全。

**基础用法**：
```bash
# 优雅停止集群
aidb-admin quick-stop
```

**强制停止**：
```bash
# 紧急情况下强制停止（跳过确认）
aidb-admin quick-stop --force

# 停止并清理数据（危险！会删除所有数据）
aidb-admin quick-stop --force --clean
```

**参数说明**：
- `-f, --force`: 强制停止，跳过确认
- `--clean`: 停止后清理数据目录

**提示**：
- 💡 建议停止前先创建备份：`aidb-admin quick-backup`
- ⚠️ `--clean` 会删除所有数据，使用前请三思

---

### `quick-scale` - 一键扩缩容

**功能**：动态调整集群规模，添加或移除节点。

**添加 Replica 节点（扩容读能力）**：
```bash
# 添加 1 个 Replica 节点
aidb-admin quick-scale --add-replicas 1

# 添加 3 个 Replica 节点
aidb-admin quick-scale --add-replicas 3
```

**添加新分片（扩容写能力）**：
```bash
# 添加 1 个新的 Shard（包含 Primary）
aidb-admin quick-scale --add-shards 1

# 添加 2 个新分片
aidb-admin quick-scale --add-shards 2
```

**移除 Replica 节点（缩容）**：
```bash
# 移除 1 个 Replica 节点
aidb-admin quick-scale --remove-replicas 1
```

**参数说明**：
- `--add-replicas <N>`: 添加 N 个 Replica 节点
- `--add-shards <N>`: 添加 N 个新分片
- `--remove-replicas <N>`: 移除 N 个 Replica 节点

**使用场景**：
- 📈 **扩容场景**：流量增加时添加 Replica 提升读性能
- 📉 **缩容场景**：流量减少时移除 Replica 节约资源
- 🔄 **重新分片**：数据量增大时添加新 Shard

---

### `quick-backup` - 一键备份

**功能**：快速创建数据库备份，支持压缩。

**基础用法**：
```bash
# 备份 Primary 节点（自动生成时间戳目录）
aidb-admin --db ./data/primary1 quick-backup

# 指定备份位置
aidb-admin --db ./data/primary1 quick-backup \
  --output /backups/manual-backup

# 不压缩备份（更快但占用更多空间）
aidb-admin --db ./data/primary1 quick-backup --compress false
```

**参数说明**：
- `--db <PATH>`: 要备份的数据库路径（必需）
- `-o, --output <PATH>`: 备份输出目录（可选，默认：./backups/时间戳）
- `-c, --compress`: 是否压缩（默认：true）

**输出示例**：
```
💾 创建 AiDb 备份...
================================================

📋 备份配置：
  源数据库: ./data/primary1
  备份目录: ./backups/20251118-103000
  压缩: 是

确认创建备份？ [y/N]: y

📸 创建快照...
[00:00:03] ========================================> 100% 完成...

================================================
✅ 备份创建成功！

📊 备份信息：
  创建时间: 2025-11-18 10:30:00 UTC
  位置: ./backups/20251118-103000
  大小: 125 MB
  文件: 45 SSTables, 1 WAL, 元数据
================================================
```

**最佳实践**：
- 📅 定期备份（建议每天）
- 💾 使用压缩节省存储空间
- 🔒 将备份存储到远程位置（S3、OSS 等）

---

### `quick-restore` - 一键恢复

**功能**：从备份快速恢复数据库。

**恢复最新备份**：
```bash
# 自动查找并恢复最新备份
aidb-admin quick-restore

# 指定备份搜索路径
aidb-admin quick-restore --backup-path /backups
```

**恢复指定备份**：
```bash
# 恢复特定备份
aidb-admin quick-restore --backup /backups/20251118-103000

# 指定恢复目标位置
aidb-admin --db ./data/restored quick-restore \
  --backup /backups/20251118-103000
```

**参数说明**：
- `--db <PATH>`: 恢复目标路径（可选，默认：./data/restored）
- `-p, --backup-path <PATH>`: 备份搜索路径（默认：./backups）
- `--latest`: 使用最新备份（默认：true）
- `-b, --backup <PATH>`: 指定具体备份路径

**输出示例**：
```
🔄 恢复 AiDb 数据库...
================================================

🔍 查找最新备份...
   ✓ 找到最新备份: /backups/20251118-103000

📋 恢复配置：
  备份: /backups/20251118-103000
  目标: ./data/restored

确认恢复备份？ [y/N]: y

🔄 恢复中...
[00:00:03] ========================================> 100% 完成...

================================================
✅ 数据库恢复成功！

📊 恢复信息：
  恢复了 45 个 SSTables
  恢复了 1 个 WAL 文件
  总大小: 125 MB
  目标位置: ./data/restored
================================================
```

---

### `quick-check` - 一键健康检查

**功能**：全面检查集群健康状况，提供优化建议。

**基础用法**：
```bash
# 快速健康检查
aidb-admin quick-check

# 详细检查（包含性能指标）
aidb-admin quick-check -D

# 自动修复常见问题
aidb-admin quick-check --auto-fix
```

**参数说明**：
- `-D, --detailed`: 显示详细信息
- `--auto-fix`: 自动修复常见问题

**输出示例**：
```
🏥 AiDb 集群健康检查
================================================

🔍 检查集群状态...

✅ 节点状态：
+-----------+--------+----------+--------+
| 节点      | 状态   | 响应时间 | 问题   |
+========================================+
| Primary-1 | ✓ 健康 | 1.2ms    | -      |
| Replica-1 | ✓ 健康 | 0.8ms    | -      |
| Replica-2 | ⚠ 警告 | 15.3ms   | 高延迟 |
+-----------+--------+----------+--------+

⚠ 发现的问题：
  1. Replica-2 响应时间较高 (15.3ms)
     建议: 检查网络连接或增加缓存大小

📋 最近备份：
  ✓ 最后备份: 2 小时前
  备份大小: 125 MB
  备份状态: 成功

================================================
✅ 集群整体状态: 健康 (1 个警告)

💡 建议：
  1. 监控 Replica-2 的响应时间
  2. 考虑增加 Replica-2 的缓存容量
================================================
```

---

## 📚 进阶命令

除了一键命令，aidb-admin 还提供精细化的管理命令：

### 集群管理

```bash
# 查看集群状态
aidb-admin cluster status

# 查看所有节点
aidb-admin cluster nodes

# 查看分片信息
aidb-admin cluster shards

# 手动添加节点
aidb-admin cluster add-node \
  --address 127.0.0.1:50054 \
  --node-type replica \
  --shard-id 0

# 移除节点
aidb-admin cluster remove-node <node-id> --force
```

### 备份管理

```bash
# 列出所有备份
aidb-admin backup list --path /backups

# 删除旧备份
aidb-admin backup delete /backups/old-backup -y
```

### 统计和监控

```bash
# 查看数据库统计
aidb-admin --db ./data/primary1 stats

# 详细统计
aidb-admin --db ./data/primary1 stats --detailed

# 查看实时指标
aidb-admin metrics

# 持续监控（每 5 秒刷新）
aidb-admin metrics --watch 5
```

### 健康检查

```bash
# 检查特定组件
aidb-admin health --component memtable
```

---

## ❓ 常见问题

### Q1: 如何查看工具版本？

```bash
aidb-admin --version
```

### Q2: 如何启用详细日志？

```bash
# 在任何命令前添加 --verbose 或 -v
aidb-admin --verbose quick-start
aidb-admin -v cluster status
```

### Q3: 端口被占用怎么办？

```bash
# 使用不同的起始端口
aidb-admin quick-start --start-port 60000
```

### Q4: 如何停止特定节点？

```bash
# 先查看节点列表
aidb-admin cluster nodes

# 然后移除特定节点
aidb-admin cluster remove-node <node-id> --force
```

### Q5: 备份失败怎么办？

**常见原因**：
1. 磁盘空间不足
2. 没有写权限
3. 数据库正在被占用

**解决方法**：
```bash
# 检查磁盘空间
df -h

# 检查权限
ls -la /backups

# 确保数据库路径正确
ls -la ./data/primary1
```

### Q6: 恢复后数据不一致？

```bash
# 恢复前先做健康检查
aidb-admin --db ./data/restored quick-check

# 查看详细统计
aidb-admin --db ./data/restored stats --detailed
```

---

## 💡 运维技巧

### 1. 定时任务自动化

创建定时备份脚本：

```bash
# /usr/local/bin/aidb-daily-backup.sh
#!/bin/bash
DATE=$(date +%Y%m%d)
aidb-admin --db /data/aidb/primary1 quick-backup \
  --output /backups/daily/$DATE \
  --compress true

# 清理 7 天前的备份
find /backups/daily -type d -mtime +7 -exec rm -rf {} \;
```

添加到 crontab：
```bash
crontab -e
# 每天凌晨 2 点执行
0 2 * * * /usr/local/bin/aidb-daily-backup.sh
```

### 2. 快速诊断脚本

```bash
# /usr/local/bin/aidb-diagnose.sh
#!/bin/bash
echo "=== AiDb 快速诊断 ==="
echo ""
echo "1. 集群状态:"
aidb-admin cluster status
echo ""
echo "2. 健康检查:"
aidb-admin quick-check
echo ""
echo "3. 最近备份:"
aidb-admin backup list --path /backups | tail -3
```

### 3. 监控告警集成

```bash
# 检查集群健康并发送告警
#!/bin/bash
STATUS=$(aidb-admin quick-check | grep "集群整体状态" | awk '{print $3}')

if [ "$STATUS" != "健康" ]; then
    # 发送告警（邮件、钉钉、Slack 等）
    echo "AiDb 集群异常: $STATUS" | mail -s "AiDb Alert" ops@example.com
fi
```

### 4. 性能优化检查清单

定期运行以下检查：

```bash
# 1. 检查缓存命中率（应 > 80%）
aidb-admin --db ./data/primary1 stats | grep "Cache Hit Rate"

# 2. 检查 Level 0 文件数量（应 < 10）
aidb-admin --db ./data/primary1 stats | grep "SSTables (L0)"

# 3. 检查磁盘使用率（应 < 80%）
df -h | grep /data

# 4. 检查响应时间（P99 应 < 10ms）
aidb-admin quick-check -D | grep "P99 延迟"
```

### 5. 紧急恢复流程

```bash
# 1. 立即停止集群
aidb-admin quick-stop --force

# 2. 从最新备份恢复
aidb-admin quick-restore --latest

# 3. 验证恢复结果
aidb-admin --db ./data/restored quick-check

# 4. 启动集群
aidb-admin quick-start --data-dir ./data/restored
```

---

## 📖 相关文档

- **[用户指南](USER_GUIDE.md)** - 完整的 API 和功能说明
- **[管理工具详细指南](monitoring/ADMIN_TOOL_GUIDE.md)** - 所有命令的详细文档
- **[监控指南](monitoring/MONITORING_GUIDE.md)** - Prometheus 和 Grafana 配置
- **[最佳实践](BEST_PRACTICES.md)** - 生产环境部署建议
- **[架构文档](ARCHITECTURE.md)** - 系统架构设计
- **[备份恢复指南](BACKUP_RECOVERY.md)** - 详细的备份恢复流程

---

## 🎓 学习路径

### 第 1 天：入门
```bash
# 1. 安装工具
cargo install --path . --features admin-cli --bin aidb-admin

# 2. 查看帮助
aidb-admin --help

# 3. 启动第一个集群
aidb-admin quick-start

# 4. 查看状态
aidb-admin quick-check
```

### 第 2 天：日常运维
```bash
# 1. 创建备份
aidb-admin --db ./data/primary1 quick-backup

# 2. 查看统计
aidb-admin --db ./data/primary1 stats

# 3. 扩容集群
aidb-admin quick-scale --add-replicas 1

# 4. 健康检查
aidb-admin quick-check -D
```

### 第 3 天：进阶操作
```bash
# 1. 手动节点管理
aidb-admin cluster nodes
aidb-admin cluster add-node --address 127.0.0.1:50060 --node-type replica

# 2. 备份管理
aidb-admin backup list --path /backups
aidb-admin backup delete /backups/old -y

# 3. 实时监控
aidb-admin metrics --watch 5
```

---

## 🆘 获取帮助

- 📖 **文档**：查看 [docs/](.) 目录下的所有文档
- 💬 **社区讨论**：[GitHub Discussions](https://github.com/yourusername/aidb/discussions)
- 🐛 **问题反馈**：[GitHub Issues](https://github.com/yourusername/aidb/issues)
- 📧 **联系我们**：ops@example.com

---

**🎉 使用 aidb-admin，让运维变得简单！**

## 🎯 一键启动集群

### 方法一：使用脚本（推荐）

```bash
# 启动默认配置的集群（1 个 Primary + 2 个 Replica）
./scripts/start-cluster.sh

# 启动自定义配置的集群
./scripts/start-cluster.sh --primaries 2 --replicas 4
```

脚本会自动：
- ✅ 创建必要的数据目录
- ✅ 启动 Primary 节点
- ✅ 启动 Replica 节点
- ✅ 启动 Coordinator 协调器
- ✅ 进行健康检查
- ✅ 显示集群状态

**输出示例：**
```
🚀 启动 AiDb 集群...
================================================

📂 创建数据目录...
   ✓ ./data/primary1 已创建
   ✓ ./data/replica1 已创建
   ✓ ./data/replica2 已创建

🔧 启动 Primary 节点...
   ✓ Primary-1 启动成功 (127.0.0.1:50051)

🔧 启动 Replica 节点...
   ✓ Replica-1 启动成功 (127.0.0.1:50052)
   ✓ Replica-2 启动成功 (127.0.0.1:50053)

🎯 启动 Coordinator...
   ✓ Coordinator 启动成功

🏥 健康检查...
   ✓ 所有节点健康

================================================
✅ 集群启动完成！

集群信息：
  Primary 节点: 1 个
  Replica 节点: 2 个
  总节点数: 3 个

访问地址：
  Primary: http://127.0.0.1:50051
  Replica-1: http://127.0.0.1:50052
  Replica-2: http://127.0.0.1:50053

管理命令：
  查看状态: ./scripts/cluster-status.sh
  停止集群: ./scripts/stop-cluster.sh
  扩容集群: ./scripts/scale-cluster.sh --add-replicas 1
================================================
```

### 方法二：使用管理工具

```bash
# 使用 aidb-admin 查看集群状态
aidb-admin cluster status

# 查看所有节点
aidb-admin cluster nodes

# 查看分片信息
aidb-admin cluster shards
```

### 方法三：手动启动（适用于学习和调试）

#### 步骤 1: 启动 Primary 节点

```bash
# 在终端 1 中运行
cargo run --example primary_node --features cluster --release
```

#### 步骤 2: 启动 Replica 节点

```bash
# 在终端 2 中运行
cargo run --example replica_node --features cluster --release
```

#### 步骤 3: 启动 Coordinator（可选，多分片场景）

```bash
# 在终端 3 中运行
cargo run --example coordinator_demo --features cluster --release
```

---

## 🛑 一键停止集群

### 使用脚本（推荐）

```bash
# 优雅停止所有节点
./scripts/stop-cluster.sh

# 强制停止（紧急情况）
./scripts/stop-cluster.sh --force
```

脚本会自动：
- ✅ 优雅关闭所有节点
- ✅ 等待数据刷新到磁盘
- ✅ 清理进程和连接
- ✅ 验证所有节点已停止

**输出示例：**
```
🛑 停止 AiDb 集群...
================================================

停止 Replica 节点...
   ✓ Replica-2 已停止
   ✓ Replica-1 已停止

停止 Primary 节点...
   ✓ Primary-1 已停止

停止 Coordinator...
   ✓ Coordinator 已停止

验证所有进程已停止...
   ✓ 无残留进程

================================================
✅ 集群已完全停止
================================================
```

### 手动停止

如果需要手动停止节点：

```bash
# 查找并停止所有 AiDb 进程
pkill -f "aidb"

# 或者分别停止
pkill -f "primary_node"
pkill -f "replica_node"
pkill -f "coordinator"
```

---

## 📈 一键扩容集群

### 添加 Replica 节点

```bash
# 添加 1 个 Replica 节点
./scripts/scale-cluster.sh --add-replicas 1

# 添加多个 Replica 节点
./scripts/scale-cluster.sh --add-replicas 3
```

### 添加 Primary 节点（新分片）

```bash
# 添加 1 个新的 Shard（包含 1 个 Primary）
./scripts/scale-cluster.sh --add-shard 1

# 添加新分片并指定 Replica 数量
./scripts/scale-cluster.sh --add-shard 1 --shard-replicas 2
```

### 使用管理工具扩容

```bash
# 添加新的 Replica 节点
aidb-admin cluster add-node \
  --address 127.0.0.1:50054 \
  --node-type replica \
  --shard-id 0

# 验证节点已添加
aidb-admin cluster nodes
```

**扩容过程示例：**
```
📈 扩容 AiDb 集群...
================================================

当前集群状态：
  Primary 节点: 1 个
  Replica 节点: 2 个
  总节点数: 3 个

添加 Replica 节点...
   ✓ 分配端口: 50054
   ✓ 创建数据目录: ./data/replica3
   ✓ 启动 Replica-3
   ✓ 注册到 Coordinator
   ✓ 健康检查通过

================================================
✅ 扩容完成！

新的集群状态：
  Primary 节点: 1 个
  Replica 节点: 3 个 (↑ +1)
  总节点数: 4 个

新节点信息：
  Replica-3: http://127.0.0.1:50054
================================================
```

---

## 📉 一键缩容集群

### 移除 Replica 节点

```bash
# 移除最后添加的 Replica 节点
./scripts/scale-cluster.sh --remove-replicas 1

# 移除多个 Replica 节点
./scripts/scale-cluster.sh --remove-replicas 2
```

### 移除指定节点

```bash
# 使用管理工具移除特定节点
aidb-admin cluster remove-node replica3 --force
```

**缩容过程示例：**
```
📉 缩容 AiDb 集群...
================================================

当前集群状态：
  Primary 节点: 1 个
  Replica 节点: 3 个
  总节点数: 4 个

移除 Replica 节点...
   ✓ 选择节点: Replica-3 (127.0.0.1:50054)
   ✓ 从 Coordinator 注销
   ✓ 优雅停止节点
   ✓ 清理数据目录
   ✓ 验证移除成功

================================================
✅ 缩容完成！

新的集群状态：
  Primary 节点: 1 个
  Replica 节点: 2 个 (↓ -1)
  总节点数: 3 个
================================================
```

---

## 🏥 健康检查

### 快速健康检查

```bash
# 使用脚本进行全面健康检查
./scripts/health-check.sh

# 使用管理工具
aidb-admin health
aidb-admin cluster status
```

**健康检查输出示例：**
```
🏥 AiDb 集群健康检查
================================================

节点状态：
  ✓ Primary-1 (127.0.0.1:50051)  健康
  ✓ Replica-1 (127.0.0.1:50052)  健康
  ✓ Replica-2 (127.0.0.1:50053)  健康

性能指标：
  请求速率: 1,234 ops/sec
  P99 延迟: 5.8 ms
  错误率: 0.01%
  缓存命中率: 87.5%

资源使用：
  内存使用: 512 MB / 2 GB (25%)
  磁盘使用: 1.2 GB / 100 GB (1.2%)
  CPU 使用: 15%

最近备份：
  ✓ 最后备份: 2 小时前
  备份大小: 125 MB
  备份状态: 成功

================================================
✅ 集群整体状态: 健康
================================================
```

### 监控仪表盘

访问 Grafana 仪表盘查看详细监控：

```bash
# 启动监控服务（如果未启动）
./scripts/start-monitoring.sh

# 访问 Grafana
# URL: http://localhost:3000
# 默认用户名: admin
# 默认密码: admin
```

---

## ❓ 常见问题

### Q1: 集群启动失败怎么办？

**解决步骤：**

1. **检查端口是否被占用**
   ```bash
   # 检查端口
   netstat -tuln | grep 5005
   
   # 如果端口被占用，停止占用进程或修改配置文件中的端口
   ```

2. **检查数据目录权限**
   ```bash
   # 确保有读写权限
   ls -la ./data
   
   # 如果没有权限，修改权限
   chmod -R 755 ./data
   ```

3. **查看日志文件**
   ```bash
   # 查看启动日志
   tail -f ./logs/primary1.log
   tail -f ./logs/replica1.log
   ```

4. **重新启动**
   ```bash
   # 停止集群
   ./scripts/stop-cluster.sh --force
   
   # 清理数据（注意：会丢失数据）
   rm -rf ./data/*
   
   # 重新启动
   ./scripts/start-cluster.sh
   ```

### Q2: 如何进行数据备份？

```bash
# 创建备份
aidb-admin --db ./data/primary1 backup create \
  --output ./backups/$(date +%Y%m%d) \
  --description "Daily backup" \
  --compress

# 列出所有备份
aidb-admin backup list --path ./backups

# 恢复备份
aidb-admin backup restore \
  --backup ./backups/20251118 \
  --target ./data/primary1-restored \
  --force
```

### Q3: 性能下降怎么办？

**诊断步骤：**

1. **检查缓存命中率**
   ```bash
   aidb-admin --db ./data/primary1 stats --detailed
   ```
   
   如果缓存命中率 < 80%，考虑：
   - 增加 Block Cache 大小
   - 添加更多 Replica 节点

2. **检查 Compaction 状态**
   ```bash
   aidb-admin --db ./data/primary1 stats | grep "SSTables"
   ```
   
   如果 Level 0 文件过多（> 10），考虑：
   - 调整 Compaction 参数
   - 减少写入速率

3. **检查资源使用**
   ```bash
   # 使用系统工具
   top
   df -h
   iostat
   ```

### Q4: 节点失联怎么办？

```bash
# 1. 检查节点状态
aidb-admin cluster status --detailed

# 2. 尝试重启失联节点
./scripts/restart-node.sh <node-id>

# 3. 如果无法恢复，移除并重新添加
aidb-admin cluster remove-node <node-id> --force
./scripts/scale-cluster.sh --add-replicas 1
```

### Q5: 如何迁移数据？

```bash
# 1. 创建备份
aidb-admin --db ./data/primary1 backup create \
  --output /backup/migration \
  --compress

# 2. 在新服务器上安装 AiDb
# (按照"快速开始"部分的步骤)

# 3. 恢复数据
aidb-admin backup restore \
  --backup /backup/migration \
  --target ./data/primary1

# 4. 启动集群
./scripts/start-cluster.sh
```

### Q6: 磁盘空间不足怎么办？

```bash
# 1. 检查磁盘使用情况
df -h
du -sh ./data/*

# 2. 清理旧的 WAL 和 SSTable
aidb-admin --db ./data/primary1 stats --detailed

# 3. 删除旧备份
find ./backups -type d -mtime +7 -exec rm -rf {} \;

# 4. 执行手动 Compaction
# (通过 API 触发，具体参考 API 文档)

# 5. 考虑扩展磁盘或添加新的 Shard
```

---

## 📖 运维命令速查表

### 集群管理

| 操作 | 命令 |
|------|------|
| 启动集群 | `./scripts/start-cluster.sh` |
| 停止集群 | `./scripts/stop-cluster.sh` |
| 集群状态 | `aidb-admin cluster status` |
| 节点列表 | `aidb-admin cluster nodes` |
| 分片列表 | `aidb-admin cluster shards` |

### 扩缩容

| 操作 | 命令 |
|------|------|
| 添加 Replica | `./scripts/scale-cluster.sh --add-replicas 1` |
| 移除 Replica | `./scripts/scale-cluster.sh --remove-replicas 1` |
| 添加 Shard | `./scripts/scale-cluster.sh --add-shard 1` |
| 添加节点 | `aidb-admin cluster add-node --address <addr> --node-type <type>` |
| 移除节点 | `aidb-admin cluster remove-node <id> --force` |

### 健康检查

| 操作 | 命令 |
|------|------|
| 快速检查 | `./scripts/health-check.sh` |
| 详细健康检查 | `aidb-admin health` |
| 数据库统计 | `aidb-admin --db <path> stats` |
| 详细统计 | `aidb-admin --db <path> stats --detailed` |
| 查看指标 | `aidb-admin metrics` |
| 监控指标（持续） | `aidb-admin metrics --watch 5` |

### 备份恢复

| 操作 | 命令 |
|------|------|
| 创建备份 | `aidb-admin --db <path> backup create --output <dir> --compress` |
| 列出备份 | `aidb-admin backup list --path <dir>` |
| 恢复备份 | `aidb-admin backup restore --backup <path> --target <dir> --force` |
| 删除备份 | `aidb-admin backup delete <path> -y` |

### 日常运维

| 操作 | 命令 |
|------|------|
| 查看日志 | `tail -f ./logs/<node>.log` |
| 监控性能 | `aidb-admin metrics --watch 5` |
| 检查进程 | `ps aux | grep aidb` |
| 检查端口 | `netstat -tuln | grep 5005` |
| 查看磁盘 | `df -h` / `du -sh ./data/*` |

---

## 🔗 相关文档

- **[用户指南](USER_GUIDE.md)** - 完整的 API 和功能说明
- **[管理工具指南](monitoring/ADMIN_TOOL_GUIDE.md)** - aidb-admin 详细文档
- **[监控指南](monitoring/MONITORING_GUIDE.md)** - Prometheus 和 Grafana 配置
- **[最佳实践](BEST_PRACTICES.md)** - 生产环境部署建议
- **[架构文档](ARCHITECTURE.md)** - 系统架构设计
- **[备份恢复指南](BACKUP_RECOVERY.md)** - 详细的备份恢复流程

---

## 💡 运维技巧

### 1. 定时备份

创建 cron 任务进行定时备份：

```bash
# 编辑 crontab
crontab -e

# 添加每日凌晨 2 点备份任务
0 2 * * * /path/to/aidb/scripts/daily-backup.sh
```

### 2. 监控告警

集成到现有监控系统：

```bash
# Prometheus 抓取配置
# 添加到 prometheus.yml
scrape_configs:
  - job_name: 'aidb'
    static_configs:
      - targets: ['localhost:9090']
```

### 3. 日志轮转

配置日志轮转避免磁盘占满：

```bash
# 创建 logrotate 配置
sudo vim /etc/logrotate.d/aidb

# 添加配置
/path/to/aidb/logs/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
}
```

### 4. 性能优化

```bash
# 1. 调整内核参数
sudo sysctl -w net.core.somaxconn=2048
sudo sysctl -w net.ipv4.tcp_max_syn_backlog=2048

# 2. 增加文件描述符限制
ulimit -n 65535

# 3. 使用 SSD 存储数据
# 确保数据目录在 SSD 上
```

### 5. 安全加固

```bash
# 1. 使用防火墙限制访问
sudo ufw allow from <trusted-ip> to any port 50051

# 2. 启用 TLS（生产环境推荐）
# 配置 TLS 证书路径

# 3. 定期更新
git pull
cargo build --release --features cluster
```

---

## 📞 获取帮助

如果遇到本指南未涵盖的问题：

1. **查看文档**: [docs/](.)
2. **查看示例**: [examples/](../examples/)
3. **提交 Issue**: [GitHub Issues](https://github.com/yourusername/aidb/issues)
4. **讨论区**: [GitHub Discussions](https://github.com/yourusername/aidb/discussions)

---

**🎉 祝您运维愉快！**
