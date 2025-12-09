# Raft Consensus Testing Guide

## 测试概览 (Testing Overview)

AiDb的Raft共识实现包含comprehensive的测试套件，确保系统在各种场景下的正确性和可靠性。

## 测试文件结构 (Test File Structure)

### 1. `tests/raft_rpc_server_tests.rs` - RPC服务器集成测试

这是新增的comprehensive测试文件，专门测试带RPC服务器的完整Raft功能。

#### 测试用例 (Test Cases)

1. **test_rpc_server_startup_and_shutdown**
   - 测试RPC服务器的启动和关闭
   - 验证服务器可以正确绑定端口
   - 验证优雅关闭机制

2. **test_three_node_cluster_with_rpc**
   - 创建3节点集群并启动RPC服务器
   - 初始化集群配置
   - 验证Leader选举
   - 确认节点间可以通信

3. **test_write_operations_with_replication**
   - 测试写操作的日志复制
   - 执行多个Put操作
   - 验证日志索引增长
   - 确认操作被复制到所有节点

4. **test_delete_operations**
   - 测试删除操作
   - 先写入数据再删除
   - 验证删除操作成功

5. **test_write_batch_operations**
   - 测试批量写入操作
   - 使用WriteBatch一次写入10个键值对
   - 验证批量操作的性能优势

6. **test_leader_election_multiple_nodes**
   - 测试多节点Leader选举
   - 验证只有一个Leader被选出
   - 确认所有节点对Leader达成一致

7. **test_metrics_after_writes**
   - 测试metrics的准确性
   - 验证日志索引正确更新
   - 确认last_applied正确追踪

### 2. `tests/openraft_integration_tests.rs` - 基础集成测试

原有的测试文件，包含单节点和基础功能测试。

#### 特点 (Features)

- 单节点测试（不需要RPC服务器）
- API正确性验证
- 基本Leader选举测试
- 简单的日志操作测试

## 运行测试 (Running Tests)

### 运行所有Raft测试

```bash
cargo test --features raft-cluster
```

### 运行RPC服务器测试

```bash
cargo test --test raft_rpc_server_tests --features raft-cluster
```

### 运行特定测试

```bash
# 测试写操作复制
cargo test --test raft_rpc_server_tests test_write_operations_with_replication --features raft-cluster

# 测试Leader选举
cargo test --test raft_rpc_server_tests test_leader_election_multiple_nodes --features raft-cluster

# 测试批量操作
cargo test --test raft_rpc_server_tests test_write_batch_operations --features raft-cluster
```

### 查看测试列表

```bash
cargo test --test raft_rpc_server_tests --features raft-cluster -- --list
```

### 详细输出模式

```bash
cargo test --test raft_rpc_server_tests --features raft-cluster -- --nocapture
```

## 测试覆盖范围 (Test Coverage)

### ✅ 已覆盖功能 (Covered Features)

- [x] RPC服务器启动和关闭
- [x] 多节点集群初始化
- [x] Leader选举机制
- [x] 单个写操作
- [x] 批量写操作
- [x] 删除操作
- [x] 日志复制
- [x] Metrics追踪
- [x] 节点间通信

### 🔄 计划添加的测试 (Planned Tests)

- [ ] 网络分区恢复
- [ ] 节点崩溃和恢复
- [ ] Leader故障转移
- [ ] 快照创建和恢复
- [ ] 成员变更（添加/删除节点）
- [ ] 大规模数据写入
- [ ] 并发写入测试
- [ ] 读取一致性验证

## 测试最佳实践 (Testing Best Practices)

### 1. 端口管理

每个测试使用不同的端口范围，避免冲突:

```rust
// test_three_node_cluster_with_rpc
let addr1 = "127.0.0.1:50111".parse().unwrap();
let addr2 = "127.0.0.1:50112".parse().unwrap();
let addr3 = "127.0.0.1:50113".parse().unwrap();

// test_write_operations_with_replication
let addr1 = "127.0.0.1:50121".parse().unwrap();
let addr2 = "127.0.0.1:50122".parse().unwrap();
let addr3 = "127.0.0.1:50123".parse().unwrap();
```

### 2. 等待时间

给RPC服务器和Raft协议足够的时间:

```rust
// 等待服务器启动
sleep(Duration::from_millis(500)).await;

// 等待Leader选举
sleep(Duration::from_millis(1000)).await;

// 等待日志复制
sleep(Duration::from_millis(500)).await;
```

### 3. 清理资源

始终在测试结束时清理资源:

```rust
// 关闭节点
node1.shutdown().await.unwrap();
node2.shutdown().await.unwrap();
node3.shutdown().await.unwrap();

// 终止服务器任务
server1.abort();
server2.abort();
server3.abort();
```

### 4. 错误处理

使用断言验证操作成功:

```rust
let result = node1.put(b"key1".to_vec(), b"value1".to_vec()).await;
assert!(result.is_ok(), "Write operation should succeed");
```

## 性能测试 (Performance Testing)

### 基准测试建议

1. **吞吐量测试**:
   ```rust
   let start = Instant::now();
   for i in 0..1000 {
       node1.put(format!("key{}", i).into_bytes(), b"value".to_vec()).await?;
   }
   let duration = start.elapsed();
   println!("1000 writes in {:?}", duration);
   ```

2. **延迟测试**:
   ```rust
   let mut latencies = Vec::new();
   for i in 0..100 {
       let start = Instant::now();
       node1.put(format!("key{}", i).into_bytes(), b"value".to_vec()).await?;
       latencies.push(start.elapsed());
   }
   println!("P50: {:?}, P99: {:?}", p50(&latencies), p99(&latencies));
   ```

3. **并发测试**:
   ```rust
   let mut handles = Vec::new();
   for i in 0..10 {
       let node = node1.clone();
       let handle = tokio::spawn(async move {
           node.put(format!("key{}", i).into_bytes(), b"value".to_vec()).await
       });
       handles.push(handle);
   }
   futures::future::join_all(handles).await;
   ```

## 调试失败的测试 (Debugging Failed Tests)

### 1. 启用日志

```bash
RUST_LOG=aidb=debug,openraft=debug cargo test --test raft_rpc_server_tests --features raft-cluster -- --nocapture
```

### 2. 运行单个测试

```bash
cargo test --test raft_rpc_server_tests test_write_operations_with_replication --features raft-cluster -- --nocapture
```

### 3. 检查端口冲突

```bash
# 查看占用的端口
lsof -i :50111-50163
```

### 4. 增加超时时间

如果测试超时，增加等待时间:

```rust
// 从 500ms 增加到 1000ms
sleep(Duration::from_millis(1000)).await;
```

## CI/CD集成 (CI/CD Integration)

### GitHub Actions配置

```yaml
- name: Run Raft tests
  run: cargo test --features raft-cluster
  timeout-minutes: 10

- name: Run RPC server tests
  run: cargo test --test raft_rpc_server_tests --features raft-cluster
  timeout-minutes: 5
```

## 测试覆盖率 (Test Coverage)

使用tarpaulin生成覆盖率报告:

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --features raft-cluster --out Html
```

## 总结 (Summary)

AiDb的Raft测试套件提供了comprehensive的功能覆盖:

- ✅ 7个RPC服务器集成测试
- ✅ 多节点集群测试
- ✅ 完整的操作验证
- ✅ Metrics验证
- ✅ 清晰的测试结构

这些测试确保Raft共识实现的正确性和可靠性，为生产环境部署提供信心。
