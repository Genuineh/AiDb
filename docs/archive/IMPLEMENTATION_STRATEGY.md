# AiDb 实施策略总览

## 📁 文档说明

本项目包含多个规划文档，各有侧重：

### 1. **OPTIMIZED_PLAN.md** ⭐ **推荐阅读**
- **用途**：优化后的实施计划，平衡借鉴RocksDB优点和避免其问题
- **特点**：三阶段渐进式实施（MVP → 优化 → 生产）
- **适合**：实际开发执行

### 2. **IMPLEMENTATION_PLAN.md** 
- **用途**：完整的14阶段详细计划
- **特点**：全面覆盖LSM-Tree所有组件
- **适合**：深入了解技术细节和长期规划

### 3. **TODO.md**
- **用途**：任务清单，跟踪开发进度
- **特点**：可勾选的任务列表
- **适合**：日常开发跟踪

### 4. **PROJECT_OVERVIEW.md**
- **用途**：项目概览和快速开始
- **特点**：全局视角
- **适合**：新人了解项目

---

## 🎯 推荐实施路径

### 阶段A：MVP（4-6周）⭐ **当前阶段**

**目标**：快速实现基础功能，验证架构

```
Week 1-2: WAL + MemTable
├─ Day 1-2:   项目准备
├─ Day 3-5:   WAL实现
├─ Day 6-9:   MemTable实现
└─ Day 10-14: SSTable基础

Week 3-4: DB引擎整合
├─ Day 15-18: DB核心逻辑
├─ Day 19-21: Flush实现
└─ Day 22-28: 测试和修复
```

**关键决策**：
- ✅ 使用crossbeam-skiplist（不自己实现）
- ✅ 简化SSTable格式（无压缩、无Bloom Filter）
- ✅ 单层结构（不做Compaction）
- ✅ 单线程Flush

**成功标准**：
```rust
// 能运行这段代码
let db = DB::open("./data", Options::default())?;
db.put(b"key", b"value")?;
assert_eq!(db.get(b"key")?, Some(b"value".to_vec()));
// 重启后数据仍在
```

---

### 阶段B：性能优化（6-8周）

**目标**：添加优化功能，达到实用性能

**新增**：
- Leveled Compaction
- Bloom Filter
- Block Cache
- 压缩支持
- 批量操作
- 多线程

**性能目标**：
- 顺序写：100K ops/s
- 随机写：50K ops/s
- 随机读：120K ops/s

---

### 阶段C：生产就绪（4-6周）

**目标**：完善功能，可用于生产

**新增**：
- Snapshot
- Iterator完整支持
- 监控指标
- 完整测试
- 详细文档

**成功标准**：
- 测试覆盖率 > 80%
- 性能达到RocksDB 60-70%
- 文档完整

---

## 🔄 与RocksDB的对比

### 借鉴RocksDB的优点

| 特性 | RocksDB实现 | AiDb实现 | 借鉴程度 |
|------|-----------|----------|---------|
| LSM架构 | 多层分级 | 相同 | 100% |
| WAL格式 | 自定义高效格式 | 简化版 | 80% |
| MemTable | SkipList | crossbeam-skiplist | 90% |
| SSTable格式 | 复杂多块 | 简化版 | 70% |
| Compaction | Leveled | 相同策略 | 80% |
| Bloom Filter | 分块BF | 简化版 | 70% |
| Block Cache | LRU | 相同 | 90% |
| 并发控制 | 复杂锁 | 简化锁 | 60% |

### 避免RocksDB的问题

| RocksDB问题 | 问题表现 | AiDb解决方案 |
|-----------|---------|------------|
| C++依赖 | Rust绑定复杂 | ✅ 纯Rust实现 |
| 配置复杂 | 200+配置项 | ✅ <20个配置项 |
| API复杂 | 学习曲线陡 | ✅ 简化API设计 |
| 编译慢 | 大量C++代码 | ✅ Rust快速编译 |
| 过度设计 | Column Families等 | ✅ 单一实例，避免复杂性 |
| Binary大 | >50MB | ✅ 目标<5MB |
| 代码庞大 | 难以理解 | ✅ 清晰架构，模块化 |

---

## 🏗️ 架构设计原则

### 1. 简单性 (Simplicity)

**反例（RocksDB）**：
```cpp
// RocksDB有多种MemTable实现
class MemTableRep {
  // SkipList, HashSkipList, Vector, etc.
};
```

**AiDb方案**：
```rust
// 单一实现，清晰明确
pub struct MemTable {
    data: SkipList<InternalKey, Value>,
    size: AtomicUsize,
}
```

### 2. 实用性 (Pragmatism)

**不追求**：
- ❌ 最极致的性能（90%即可）
- ❌ 最全面的功能（满足核心需求）
- ❌ 最完美的代码（可维护即可）

**追求**：
- ✅ 快速验证和迭代
- ✅ 清晰的代码结构
- ✅ 合理的性能
- ✅ 实际可用

### 3. Rust优势 (Rust Strengths)

**利用Rust特性**：
```rust
// 1. 零成本抽象
trait StorageLayer {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
}
// 编译后无运行时开销

// 2. 类型安全
// 编译期防止数据竞争
fn concurrent_read(db: Arc<DB>) { 
    // 编译器保证线程安全
}

// 3. 优秀的错误处理
pub type Result<T> = std::result::Result<T, Error>;
// 强制处理错误

// 4. 内存安全
// 无需手动管理内存，无内存泄漏
```

---

## 📊 关键指标跟踪

### 开发进度

| 阶段 | 预计时间 | 当前状态 | 完成度 |
|------|---------|---------|--------|
| 项目初始化 | 1周 | ✅ 已完成 | 100% |
| **阶段A (MVP)** | **4-6周** | **🚀 进行中** | **5%** |
| 阶段B (优化) | 6-8周 | ⏳ 等待 | 0% |
| 阶段C (生产) | 4-6周 | ⏳ 等待 | 0% |

### 性能指标

| 操作类型 | 当前 | 阶段A目标 | 阶段B目标 | 阶段C目标 | RocksDB |
|---------|-----|----------|----------|----------|---------|
| 顺序写 | - | 30K | 100K | 140K | 200K ops/s |
| 随机写 | - | 20K | 50K | 70K | 100K ops/s |
| 随机读 | - | 30K | 120K | 140K | 200K ops/s |

### 代码质量

| 指标 | 当前 | 目标 |
|-----|------|------|
| 测试覆盖率 | 80% | 80%+ |
| Clippy警告 | 0 | 0 |
| 文档覆盖率 | 100% | 100% |

---

## 🛠️ 开发流程

### 每个功能的开发步骤

```
1. 设计
   ├─ 参考RocksDB设计
   ├─ 简化到核心需求
   └─ 确定API接口

2. 实现
   ├─ TDD：先写测试
   ├─ 实现核心逻辑
   └─ 处理边界情况

3. 测试
   ├─ 单元测试
   ├─ 集成测试
   └─ 性能测试

4. 优化
   ├─ Profiling分析
   ├─ 针对性优化
   └─ 验证改进

5. 文档
   ├─ API文档
   ├─ 示例代码
   └─ 设计说明
```

### 代码审查清单

- [ ] 功能正确性
- [ ] 测试覆盖
- [ ] 错误处理
- [ ] 性能考虑
- [ ] 代码清晰度
- [ ] 文档完整性
- [ ] Clippy通过
- [ ] 格式化

---

## 📚 学习路径

### 对于新加入的开发者

**第1天**：了解全局
- 阅读 PROJECT_OVERVIEW.md
- 阅读 OPTIMIZED_PLAN.md
- 运行现有测试

**第2-3天**：深入理解
- 阅读 IMPLEMENTATION_PLAN.md（详细技术）
- 学习LSM-Tree论文
- 浏览RocksDB Wiki

**第4-5天**：动手实践
- 阅读现有代码
- 尝试添加测试
- 修复小bug

**第2周起**：参与开发
- 认领TODO任务
- 提交代码
- 参与代码审查

---

## 🚀 快速开始（开发者）

### 环境准备

```bash
# 克隆仓库
git clone https://github.com/yourusername/aidb.git
cd aidb

# 安装依赖
cargo build

# 运行测试
cargo test

# 代码检查
cargo clippy
cargo fmt
```

### 认领任务

1. 查看 TODO.md
2. 选择标记为 "P0" 或 "当前Sprint" 的任务
3. 在GitHub创建issue
4. 创建分支开发
5. 提交PR

### 提交代码

```bash
# 创建分支
git checkout -b feature/your-feature

# 开发 + 测试
cargo test

# 提交
git add .
git commit -m "feat: add your feature"
git push origin feature/your-feature

# 创建PR
gh pr create
```

---

## 🎯 里程碑

### Milestone 1: MVP功能 (Week 6)
- ✅ WAL + MemTable + SSTable
- ✅ 基础读写功能
- ✅ 崩溃恢复
- ✅ 基础测试通过

**Demo**：
```rust
// 能运行的完整示例
let db = DB::open("./data", Options::default())?;
for i in 0..10000 {
    db.put(&format!("key{}", i).as_bytes(), b"value")?;
}
println!("Write complete!");
```

### Milestone 2: 性能优化 (Week 14)
- ✅ Compaction实现
- ✅ Bloom Filter
- ✅ Block Cache
- ✅ 性能达到50%目标

**Demo**：
```bash
# 运行benchmark
cargo bench
# 结果显示：50K+ write ops/s
```

### Milestone 3: 生产就绪 (Week 20)
- ✅ 所有功能完整
- ✅ 测试覆盖率达标
- ✅ 文档完善
- ✅ 性能达到70%目标

**Demo**：
```bash
# 运行完整测试套件
cargo test --all
# 所有测试通过
```

---

## 📞 获取帮助

### 遇到问题时

1. **查看文档**
   - 本目录下的所有.md文件
   - 代码中的注释和文档

2. **参考资料**
   - RocksDB Wiki
   - LSM-Tree论文
   - mini-lsm教程

3. **工具调试**
   - `cargo test -- --nocapture`
   - `rust-gdb`
   - `flamegraph`

4. **寻求帮助**
   - GitHub Issues
   - 项目讨论区

---

## 总结

### 核心要点

1. **目标明确**：自研Rust LSM-Tree存储引擎
2. **借鉴学习**：从RocksDB学习成熟设计
3. **避免陷阱**：不照搬RocksDB的复杂性
4. **渐进实施**：MVP → 优化 → 生产
5. **务实导向**：功能优先，避免过度设计

### 当前行动

**立即开始阶段A**：
1. ✅ 确认项目结构（已完成）
2. 🚀 实现WAL（Day 3-5）
3. 🚀 实现MemTable（Day 6-9）
4. 🚀 实现SSTable（Day 10-14）
5. 🚀 整合DB引擎（Day 15-28）

**预期4-6周后**：
- 可运行的MVP
- 基础功能完整
- 测试验证通过
- 为阶段B做好准备

---

*让我们开始构建高性能的Rust存储引擎吧！*

*最后更新: 2025-11-04*
