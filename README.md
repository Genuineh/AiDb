# AiDb

🚀 **高性能、可弹性扩展的LSM-Tree存储引擎**

[![CI](https://github.com/yourusername/aidb/workflows/CI/badge.svg)](https://github.com/yourusername/aidb/actions/workflows/ci.yml)
[![Security Audit](https://github.com/yourusername/aidb/workflows/Security%20Audit/badge.svg)](https://github.com/yourusername/aidb/actions/workflows/security.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Crates.io](https://img.shields.io/crates/v/aidb.svg)](https://crates.io/crates/aidb)

## 📖 项目简介

AiDb是一个用Rust从零实现的分布式KV存储引擎，基于LSM-Tree架构。项目的核心目标是：

- ⚡ **高性能**：借鉴RocksDB的成熟设计，达到其60-70%性能
- 🔧 **纯Rust实现**：避免C++依赖，简化API，降低复杂度
- 📈 **弹性扩展**：多Shard分片架构，线性扩展读写能力
- 💰 **成本优化**：无需全量数据复制，降低40-50%存储成本
- 🛡️ **生产可用**：完整的备份恢复、监控告警、运维工具

## 🎯 核心特性

### 单机版特性
- ✅ WAL（Write-Ahead Log）保证持久化
- ✅ MemTable（SkipList）高性能内存索引
- ✅ SSTable分层存储，支持前缀压缩和索引
- ✅ Flush机制：MemTable自动刷新到SSTable
- ✅ 数据持久化和恢复
- ✅ Bloom Filter加速查询
- ✅ Leveled Compaction优化空间利用
- ✅ Snappy压缩支持（可选）
- ✅ Block Cache缓存热数据
- ✅ WriteBatch原子批量写入

### 集群版特性
- 🔄 Primary-Replica架构，Replica作为缓存层
- 🌐 一致性哈希路由，负载均衡
- 📦 多Shard分片，水平扩展
- ☁️ 异步备份到对象存储（S3/OSS）
- 🔧 弹性伸缩，秒级添加节点
- 📊 Prometheus监控 + Grafana仪表盘

## 🏗️ 架构设计

### 单机架构
```
Write Path:  WAL → MemTable → (Flush) → SSTable(L0) → (Compaction) → SSTable(L1-N)
Read Path:   MemTable → Immutable MemTable → Block Cache → SSTable(L0-N)
```

### 集群架构
```
                 ┌──────────────┐
                 │ Coordinator  │
                 └──────┬───────┘
                        │
         ┌──────────────┼──────────────┐
         │              │              │
    ┌────▼───┐     ┌───▼────┐    ┌───▼────┐
    │Shard 1 │     │Shard 2 │    │Shard N │
    └────┬───┘     └────────┘    └────────┘
         │
    ┌────┴─────┐
    │          │
┌───▼──┐  ┌───▼──┐
│Primary│  │Replica│ (缓存+转发)
│(SSD) │  │(Cache)│
└───┬──┘  └──────┘
    │
    ▼ 异步备份
 [S3/OSS]
```

**设计亮点**：
- Primary独占本地SSD，完整LSM存储
- Replica只有内存缓存，通过RPC转发miss
- 无需实时数据复制，降低成本
- 异步备份到网盘，不影响性能

## 🚀 快速开始

### 前置要求
```bash
# Rust 1.70+
rustup update

# 编译
cargo build --release
```

### 基础使用（单机版）
```rust
use aidb::{DB, Options, WriteBatch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 打开数据库
    let options = Options::default();
    let db = DB::open("./data", options)?;

    // 写入数据
    db.put(b"key1", b"value1")?;
    db.put(b"key2", b"value2")?;
    
    // 批量写入（原子操作）
    let mut batch = WriteBatch::new();
    batch.put(b"key3", b"value3");
    batch.put(b"key4", b"value4");
    batch.delete(b"key1");
    db.write(batch)?;
    
    // 读取数据
    if let Some(value) = db.get(b"key2")? {
        println!("value: {:?}", value);
    }
    
    // 删除数据
    db.delete(b"key2")?;

    // 手动刷新到磁盘
    db.flush()?;

    // 关闭数据库（会自动flush）
    db.close()?;

    Ok(())
}
```

更多示例请查看 [examples/](examples/) 目录。

### 集群使用（待实现）
```rust
use aidb::cluster::{Coordinator, CoordinatorConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // 连接Coordinator
    let coordinator = Coordinator::connect("coordinator:8080").await?;
    
    // 使用方式与单机版相同
    coordinator.put(b"key", b"value").await?;
    let value = coordinator.get(b"key").await?;
    
    Ok(())
}
```

## 📊 性能目标

### 单机版（已规划）
| 操作 | 目标 | RocksDB对比 |
|------|------|------------|
| 顺序写入 | 140K ops/s | 70% |
| 随机写入 | 70K ops/s | 70% |
| 随机读取 | 140K ops/s | 70% |

### 集群版（10个Shard）
| 操作 | 目标 | 扩展倍数 |
|------|------|---------|
| 总写入 | 700K ops/s | 10× |
| 缓存命中读 | 5M ops/s | 50× |
| 缓存miss读 | 300K ops/s | 4× |

## 📅 项目状态

**当前阶段**: 🚧 阶段B - 性能优化

- ✅ 项目基础设施
- ✅ WAL实现
- ✅ MemTable实现
- ✅ SSTable实现
- ✅ Flush机制
- ✅ Compaction实现
- ✅ Bloom Filter实现
- ✅ Block Cache实现
- ✅ 压缩和优化（刚完成！）
- ⏳ 高级功能开发

**最新成就**: Week 13-14压缩和优化完成！WriteBatch、Snappy压缩集成、完整基准测试套件。

完整进度查看：[TODO.md](TODO.md) | [Week 13-14完成总结](WEEK_13_14_COMPLETION_SUMMARY.md)

## 📚 文档导航

### 核心文档
- **[架构设计](docs/ARCHITECTURE.md)** - 单机版和集群版完整架构
- **[实施计划](docs/IMPLEMENTATION.md)** - 48周详细开发计划
- **[设计决策](docs/DESIGN_DECISIONS.md)** - 为什么这样设计

### 开发文档
- **[开发指南](docs/DEVELOPMENT.md)** - 如何参与开发
- **[CI/CD 流程](docs/CICD.md)** - 持续集成和发布流程
- **[API文档](https://docs.rs/aidb)** - 代码API文档（待发布）
- **[任务清单](TODO.md)** - 当前开发任务

### 运维文档（待完成）
- **[部署指南](docs/DEPLOYMENT.md)** - 如何部署集群
- **[运维手册](docs/OPERATIONS.md)** - 日常运维操作
- **[故障排查](docs/TROUBLESHOOTING.md)** - 常见问题解决

## 🔧 开发

### 编译和测试
```bash
# 开发模式编译
cargo build

# 运行测试
cargo test

# 运行基准测试
cargo bench

# 代码检查
cargo clippy

# 代码格式化
cargo fmt
```

### 项目结构
```
aidb/
├── src/              # 源代码
│   ├── lib.rs       # 库入口
│   ├── error.rs     # 错误类型
│   ├── config.rs    # 配置
│   ├── wal/         # WAL实现 ✅
│   ├── memtable/    # MemTable实现 ✅
│   └── sstable/     # SSTable实现 ✅
├── tests/           # 集成测试
├── benches/         # 性能测试
├── examples/        # 示例代码
└── docs/            # 文档
```

## 🗺️ Roadmap

### 阶段0: 单机版 (Week 1-20) - 当前
- [x] 项目初始化
- [x] WAL实现
- [x] MemTable实现  
- [x] SSTable实现
- [x] DB引擎整合
- [x] Flush机制
- [x] Compaction实现
- [x] Bloom Filter实现
- [x] Block Cache实现
- [x] 压缩和优化 ✅ **刚完成**
- [ ] 高级功能
- [ ] 测试完善
- [ ] 文档和发布

### 阶段1: RPC网络层 (Week 21-24)
- [ ] gRPC框架
- [ ] Primary节点RPC服务
- [ ] Replica节点缓存和转发

### 阶段2: 分布式协调 (Week 25-34)
- [ ] Coordinator路由
- [ ] 一致性哈希
- [ ] 健康检查
- [ ] 多Shard协同

### 阶段3-6: 完善功能 (Week 35-48)
- [ ] 备份恢复
- [ ] 弹性伸缩
- [ ] 监控告警
- [ ] 运维工具

详细计划：[docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md)

## 🎯 设计理念

### 从RocksDB借鉴
- ✅ 成熟的LSM-Tree分层架构
- ✅ 高效的Compaction策略
- ✅ Bloom Filter优化查询
- ✅ 经过验证的数据格式

### 避免RocksDB的问题
- ❌ 配置复杂（200+选项）→ ✅ 简化到<20个
- ❌ API臃肿（100+方法）→ ✅ 简化到<30个
- ❌ C++依赖 → ✅ 纯Rust实现
- ❌ 编译慢 → ✅ 快速编译

### 创新点
- 🆕 Replica作为缓存层，非完整副本
- 🆕 异步备份替代实时复制，降低成本
- 🆕 多Shard分片，真正的水平扩展

详细说明：[docs/DESIGN_DECISIONS.md](docs/DESIGN_DECISIONS.md)

## 🤝 贡献

欢迎贡献！请查看 [CONTRIBUTING.md](CONTRIBUTING.md)

### 如何贡献
1. Fork项目
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 开启Pull Request

### 贡献指南
- 代码需通过 `cargo test`
- 代码需通过 `cargo clippy`
- 提交前运行 `cargo fmt`
- 为新功能添加测试
- 更新相关文档

## 📄 License

本项目采用双许可证：
- MIT License ([LICENSE-MIT](LICENSE))
- Apache License 2.0 ([LICENSE-APACHE](LICENSE))

## 🙏 致谢

本项目受以下项目启发：
- [RocksDB](https://github.com/facebook/rocksdb) - Meta的高性能存储引擎
- [LevelDB](https://github.com/google/leveldb) - Google的LSM-Tree实现
- [sled](https://github.com/spacejam/sled) - Rust嵌入式数据库
- [mini-lsm](https://github.com/skyzh/mini-lsm) - LSM教学项目

## 📞 联系方式

- 问题反馈：[GitHub Issues](https://github.com/yourusername/aidb/issues)
- 讨论交流：[GitHub Discussions](https://github.com/yourusername/aidb/discussions)

---

**⚠️ 注意**：本项目目前处于开发阶段，不建议用于生产环境。

**Star** ⭐ 本项目以获取最新进展！
