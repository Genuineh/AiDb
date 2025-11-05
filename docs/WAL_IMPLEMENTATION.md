# WAL (Write-Ahead Log) 实现文档

## 概述

WAL（Write-Ahead Log）是AiDb存储引擎的核心组件之一，负责确保数据的持久化和崩溃恢复能力。

## 设计目标

- **持久性保证**：所有写入在应用到MemTable前先写入WAL
- **崩溃恢复**：系统崩溃后可以从WAL恢复所有未持久化的数据
- **高性能**：使用顺序写入和缓冲优化性能
- **数据完整性**：通过CRC32校验保证数据不被损坏

## 架构设计

### Record格式

每个WAL记录采用以下格式：

```
┌─────────────────────────────────────────┐
│  Checksum (4 bytes, u32 little-endian) │
├─────────────────────────────────────────┤
│  Length (2 bytes, u16 little-endian)   │
├─────────────────────────────────────────┤
│  Type (1 byte)                          │
│  - Full = 1                             │
│  - First = 2                            │
│  - Middle = 3                           │
│  - Last = 4                             │
├─────────────────────────────────────────┤
│  Data (variable length)                 │
└─────────────────────────────────────────┘
```

总头部大小：7字节（HEADER_SIZE）

### Record类型

WAL支持四种记录类型，用于处理大数据的分片：

1. **Full**: 完整的记录，适合小于32KB的数据
2. **First**: 多片段记录的第一个片段
3. **Middle**: 多片段记录的中间片段
4. **Last**: 多片段记录的最后一个片段

### 数据分片策略

- 单个Record的数据最大为32KB（MAX_RECORD_SIZE）
- 大于32KB的数据自动分片为多个Record
- 分片在写入时自动进行，在读取时自动重组

## 核心组件

### 1. Record (src/wal/record.rs)

负责记录的编码和解码：

```rust
pub struct Record {
    pub record_type: RecordType,
    pub data: Vec<u8>,
}

impl Record {
    // 编码为字节流
    pub fn encode(&self) -> Vec<u8>
    
    // 从字节流解码
    pub fn decode(data: &[u8]) -> Result<Self>
}
```

**关键特性**：
- CRC32校验保证数据完整性
- 自动计算和验证校验和
- 支持空数据

### 2. WALWriter (src/wal/writer.rs)

负责写入操作：

```rust
pub struct WALWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    file_size: u64,
}

impl WALWriter {
    // 创建新的Writer
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self>
    
    // 追加数据
    pub fn append(&mut self, data: &[u8]) -> Result<()>
    
    // 同步到磁盘
    pub fn sync(&mut self) -> Result<()>
}
```

**关键特性**：
- 使用BufWriter提高写入性能
- 自动处理大数据的分片
- 追踪文件大小
- 支持文件重新打开（追加模式）

### 3. WALReader (src/wal/reader.rs)

负责读取和恢复：

```rust
pub struct WALReader {
    reader: BufReader<File>,
    position: u64,
}

impl WALReader {
    // 创建新的Reader
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self>
    
    // 读取下一个完整条目
    pub fn read_next(&mut self) -> Result<Option<Vec<u8>>>
    
    // 恢复所有条目
    pub fn recover_all(&mut self) -> Result<Vec<Vec<u8>>>
}
```

**关键特性**：
- 自动重组分片的记录
- 验证记录完整性
- 检测和报告数据损坏
- 支持部分恢复（在遇到损坏时停止）

### 4. WAL Manager (src/wal/mod.rs)

统一的WAL管理接口：

```rust
pub struct WAL {
    writer: WALWriter,
}

impl WAL {
    // 打开或创建WAL
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self>
    
    // 追加条目
    pub fn append(&mut self, data: &[u8]) -> Result<()>
    
    // 同步到磁盘
    pub fn sync(&mut self) -> Result<()>
    
    // 恢复条目
    pub fn recover<P: AsRef<Path>>(path: P) -> Result<Vec<Vec<u8>>>
}
```

## 使用示例

### 基础使用

```rust
use aidb::wal::WAL;

// 创建或打开WAL
let mut wal = WAL::open("data.wal")?;

// 写入条目
wal.append(b"key1:value1")?;
wal.append(b"key2:value2")?;

// 确保数据持久化
wal.sync()?;

// 恢复数据
let entries = WAL::recover("data.wal")?;
for entry in entries {
    println!("Recovered: {:?}", entry);
}
```

### 高级使用

```rust
use aidb::wal::{WALWriter, WALReader};

// 写入大量数据
let mut writer = WALWriter::new("large.wal")?;
for i in 0..10000 {
    let data = format!("entry_{}", i);
    writer.append(data.as_bytes())?;
}
writer.sync()?;

// 流式读取
let mut reader = WALReader::new("large.wal")?;
while let Some(entry) = reader.read_next()? {
    process_entry(entry);
}
```

## 性能特性

### 写入性能

- **缓冲写入**：使用BufWriter减少系统调用
- **批量同步**：可以批量写入后统一sync
- **顺序I/O**：所有写入都是追加，充分利用磁盘顺序写入性能

### 读取性能

- **缓冲读取**：使用BufReader减少系统调用
- **流式处理**：支持逐条读取，内存友好
- **高效解码**：零拷贝设计，最小化内存分配

### 性能建议

1. **批量写入**：
   ```rust
   let mut wal = WAL::open("data.wal")?;
   for i in 0..1000 {
       wal.append(&data[i])?;
   }
   wal.sync()?; // 批量同步
   ```

2. **定期同步**：
   - 每写入N条记录同步一次
   - 或每隔T秒同步一次
   - 平衡性能和数据安全性

3. **合理的记录大小**：
   - 避免过小的记录（增加开销）
   - 避免过大的记录（增加延迟）
   - 建议：1KB - 10KB

## 错误处理

### 错误类型

WAL可能返回以下错误：

1. **Error::Io**: I/O操作失败
   ```rust
   // 文件不存在、权限不足等
   ```

2. **Error::Corruption**: 数据损坏
   ```rust
   // 校验和不匹配
   // 记录格式错误
   // 文件截断
   ```

### 恢复策略

```rust
let mut reader = WALReader::new("data.wal")?;
let entries = match reader.recover_all() {
    Ok(entries) => entries,
    Err(Error::Corruption(msg)) => {
        // 部分恢复：已读取的条目仍然有效
        log::warn!("WAL corruption: {}", msg);
        // 返回已恢复的数据
        entries
    }
    Err(e) => return Err(e),
};
```

## 文件管理

### 文件命名

```rust
use aidb::wal::{wal_filename, parse_wal_filename};

// 生成文件名
let filename = wal_filename(1);    // "000001.log"
let filename = wal_filename(123);  // "000123.log"

// 解析文件名
let seq = parse_wal_filename("000001.log"); // Some(1)
```

### 文件轮转（未来）

计划支持的功能：

```rust
// 自动轮转到新文件
if wal.size() > MAX_WAL_SIZE {
    wal.rotate()?;
}

// 删除旧的WAL文件
wal.delete_old_logs(retain_count)?;
```

## 测试覆盖

WAL实现包含全面的单元测试：

### Record测试
- ✅ 编码/解码
- ✅ 所有记录类型
- ✅ 校验和验证
- ✅ 空数据
- ✅ 大数据

### Writer测试
- ✅ 创建和追加
- ✅ 小记录
- ✅ 大记录（分片）
- ✅ 多次追加
- ✅ 空追加
- ✅ 重新打开

### Reader测试
- ✅ 读取单条记录
- ✅ 读取多条记录
- ✅ 读取大记录
- ✅ 完整恢复
- ✅ 空文件
- ✅ 位置跟踪

### 集成测试
- ✅ 写入后恢复
- ✅ 多次操作
- ✅ 空条目处理

运行测试：
```bash
cargo test wal
```

## 未来增强

### 短期（阶段A）
- [ ] WAL文件轮转
- [ ] 旧日志清理
- [ ] 集成到DB::open()
- [ ] 崩溃恢复测试

### 中期（阶段B）
- [ ] 压缩支持
- [ ] 批量写入优化
- [ ] 性能基准测试
- [ ] 更详细的统计信息

### 长期（阶段C+）
- [ ] 并发写入支持
- [ ] WAL归档到对象存储
- [ ] 增量备份支持
- [ ] 点对点恢复

## 参考资料

WAL设计参考了以下项目：

- **LevelDB WAL**: Google的经典实现
- **RocksDB WAL**: Meta的高性能实现
- **PostgreSQL WAL**: 成熟的WAL设计

## 相关文档

- [架构设计](ARCHITECTURE.md) - 整体架构
- [实施计划](IMPLEMENTATION.md) - 开发计划
- [API文档](../src/wal/mod.rs) - 代码文档

---

**最后更新**: 2025-11-05  
**状态**: ✅ 已完成并通过测试
