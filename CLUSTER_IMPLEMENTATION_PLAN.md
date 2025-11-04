# AiDb 弹性集群实施计划

## 🎯 架构确认

### 核心架构

```
                    ┌──────────────────────┐
                    │   Coordinator        │
                    │ ┌──────────────────┐ │
                    │ │ Router           │ │
                    │ │ Load Balancer    │ │
                    │ │ Health Checker   │ │
                    │ └──────────────────┘ │
                    └──────────┬───────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
         ┌────▼─────┐     ┌───▼──────┐    ┌───▼──────┐
         │ Shard 1  │     │ Shard 2  │    │ Shard N  │
         │  Group   │     │  Group   │    │  Group   │
         └────┬─────┘     └────┬─────┘    └────┬─────┘
              │                │                │
    ┌─────────┴─────────┐      │                │
    │                   │      │                │
┌───▼────┐         ┌───▼────┐  ...             ...
│Primary │         │Replica │
│        │         │        │
│┌──────┐│         │┌──────┐│
││Local ││  RPC    ││Cache ││
││ SSD  ││◄────────┤│(LRU) ││
││      ││         ││      ││
││LSM-  ││         │└──────┘│
││Tree  ││         │        │
│└──┬───┘│         └────────┘
│   │    │
│   │异步 │
│   ▼    │
│┌──────┐│
││Backup││
││ Mgr  ││
│└──┬───┘│
└───┼────┘
    │
    ▼
┌────────────┐
│  Network   │
│  Storage   │
│ (S3/OSS)   │
└────────────┘
```

### 关键组件

**1. Coordinator（协调器）**
- 路由：一致性哈希
- 负载均衡：读请求分发
- 健康检查：节点状态监控

**2. Shard Group（分片组）**
- Primary：完整存储 + RPC服务
- Replicas：缓存 + 转发

**3. Primary Node（主节点）**
- LSM存储引擎（我们的AiDb）
- RPC服务端
- 备份管理器

**4. Replica Node（从节点）**
- LRU缓存
- RPC客户端
- 无状态设计

---

## 📅 分阶段实施计划

### 总览

```
阶段0: 单机版 (Week 1-20) ✅ 已规划
阶段1: RPC和网络层 (Week 21-24)
阶段2: Coordinator (Week 25-28)
阶段3: Shard Group (Week 29-34)
阶段4: Backup和恢复 (Week 35-40)
阶段5: 弹性伸缩 (Week 41-44)
阶段6: 监控和运维 (Week 45-48)
```

---

## 📋 阶段0: 单机版（已规划，Week 1-20）

**目标**：完成单机LSM存储引擎

**参考**：`OPTIMIZED_PLAN.md` 阶段A/B/C

**交付物**：
- ✅ 完整的LSM-Tree实现
- ✅ WAL + MemTable + SSTable
- ✅ Compaction + Bloom Filter
- ✅ 性能达标（60-70% RocksDB）

**验收标准**：
```rust
// 能稳定运行
let db = DB::open("./data", Options::default())?;
for i in 0..1_000_000 {
    db.put(&format!("key{}", i).as_bytes(), b"value")?;
}
// 性能、稳定性测试通过
```

---

## 📋 阶段1: RPC和网络层（Week 21-24）

### Week 21: RPC框架搭建

**目标**：建立RPC通信基础

**技术选型**：
```toml
[dependencies]
# 推荐: tonic (gRPC for Rust)
tonic = "0.10"
prost = "0.12"
tokio = { version = "1", features = ["full"] }

# 或者: tarpc (纯Rust RPC)
# tarpc = "0.33"
```

**定义服务接口**：

```protobuf
// proto/aidb.proto
syntax = "proto3";

package aidb;

service Storage {
  // 基础操作
  rpc Get(GetRequest) returns (GetResponse);
  rpc Put(PutRequest) returns (PutResponse);
  rpc Delete(DeleteRequest) returns (DeleteResponse);
  
  // 批量操作
  rpc BatchGet(BatchGetRequest) returns (BatchGetResponse);
  rpc BatchPut(BatchPutRequest) returns (BatchPutResponse);
  
  // 范围查询
  rpc Scan(ScanRequest) returns (stream ScanResponse);
  
  // 健康检查
  rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
  
  // 统计信息
  rpc GetStats(GetStatsRequest) returns (GetStatsResponse);
}

message GetRequest {
  bytes key = 1;
}

message GetResponse {
  bool found = 1;
  bytes value = 2;
}

message PutRequest {
  bytes key = 1;
  bytes value = 2;
}

message PutResponse {
  bool success = 1;
}

// ... 其他消息定义
```

**实现RPC服务端**：

```rust
// src/rpc/server.rs
use tonic::{transport::Server, Request, Response, Status};

pub struct StorageService {
    db: Arc<DB>,
}

#[tonic::async_trait]
impl Storage for StorageService {
    async fn get(&self, request: Request<GetRequest>) 
        -> Result<Response<GetResponse>, Status> {
        let key = &request.get_ref().key;
        
        match self.db.get(key) {
            Ok(Some(value)) => Ok(Response::new(GetResponse {
                found: true,
                value,
            })),
            Ok(None) => Ok(Response::new(GetResponse {
                found: false,
                value: vec![],
            })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }
    
    async fn put(&self, request: Request<PutRequest>) 
        -> Result<Response<PutResponse>, Status> {
        let req = request.get_ref();
        
        match self.db.put(&req.key, &req.value) {
            Ok(_) => Ok(Response::new(PutResponse { success: true })),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }
    
    // 实现其他方法...
}

pub async fn start_server(db: Arc<DB>, addr: SocketAddr) -> Result<()> {
    let service = StorageService { db };
    
    Server::builder()
        .add_service(StorageServer::new(service))
        .serve(addr)
        .await?;
    
    Ok(())
}
```

**实现RPC客户端**：

```rust
// src/rpc/client.rs
use tonic::transport::Channel;

pub struct StorageClient {
    inner: StorageClient<Channel>,
}

impl StorageClient {
    pub async fn connect(addr: &str) -> Result<Self> {
        let inner = StorageClient::connect(addr).await?;
        Ok(Self { inner })
    }
    
    pub async fn get(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let request = GetRequest {
            key: key.to_vec(),
        };
        
        let response = self.inner.get(request).await?;
        let reply = response.into_inner();
        
        if reply.found {
            Ok(Some(reply.value))
        } else {
            Ok(None)
        }
    }
    
    pub async fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        let request = PutRequest {
            key: key.to_vec(),
            value: value.to_vec(),
        };
        
        let response = self.inner.put(request).await?;
        let reply = response.into_inner();
        
        if reply.success {
            Ok(())
        } else {
            Err(Error::internal("put failed"))
        }
    }
    
    // 其他方法...
}
```

**任务清单**：
- [ ] 定义完整的protobuf接口
- [ ] 实现RPC服务端
- [ ] 实现RPC客户端
- [ ] 连接池管理
- [ ] 错误处理和重试
- [ ] 超时控制
- [ ] 单元测试

---

### Week 22: Primary节点实现

**目标**：包装DB为Primary节点，提供RPC服务

```rust
// src/cluster/primary.rs
pub struct PrimaryNode {
    // 本地DB实例
    db: Arc<DB>,
    
    // RPC服务器
    rpc_server: RpcServer,
    
    // 配置
    config: PrimaryConfig,
    
    // 统计信息
    stats: Arc<RwLock<PrimaryStats>>,
}

pub struct PrimaryConfig {
    // 监听地址
    listen_addr: SocketAddr,
    
    // DB路径
    db_path: PathBuf,
    
    // DB配置
    db_options: Options,
    
    // RPC配置
    max_connections: usize,
    request_timeout: Duration,
}

impl PrimaryNode {
    pub async fn new(config: PrimaryConfig) -> Result<Self> {
        // 1. 打开本地DB
        let db = Arc::new(DB::open(&config.db_path, config.db_options)?);
        
        // 2. 创建RPC服务器
        let rpc_server = RpcServer::new(db.clone(), config.listen_addr);
        
        // 3. 初始化统计
        let stats = Arc::new(RwLock::new(PrimaryStats::default()));
        
        Ok(Self {
            db,
            rpc_server,
            config,
            stats,
        })
    }
    
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting primary node at {}", self.config.listen_addr);
        
        // 启动RPC服务器
        self.rpc_server.start().await?;
        
        Ok(())
    }
    
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping primary node");
        
        // 停止RPC服务器
        self.rpc_server.stop().await?;
        
        // 关闭DB
        self.db.close()?;
        
        Ok(())
    }
    
    // 本地写入（也可通过RPC）
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let start = Instant::now();
        
        self.db.put(key, value)?;
        
        // 更新统计
        self.stats.write().record_put(start.elapsed());
        
        Ok(())
    }
    
    // 本地读取
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let start = Instant::now();
        
        let result = self.db.get(key)?;
        
        // 更新统计
        self.stats.write().record_get(start.elapsed(), result.is_some());
        
        Ok(result)
    }
}

#[derive(Default)]
pub struct PrimaryStats {
    pub total_gets: u64,
    pub total_puts: u64,
    pub get_latency: LatencyStats,
    pub put_latency: LatencyStats,
    // ... 更多统计
}
```

**任务清单**：
- [ ] 实现PrimaryNode结构
- [ ] 集成RPC服务器
- [ ] 统计信息收集
- [ ] 健康检查端点
- [ ] 优雅关闭
- [ ] 集成测试

---

### Week 23: Replica节点实现

**目标**：实现轻量级缓存节点

```rust
// src/cluster/replica.rs
use lru::LruCache;

pub struct ReplicaNode {
    // LRU缓存
    cache: Arc<RwLock<LruCache<Vec<u8>, CachedValue>>>,
    
    // Primary的RPC客户端
    primary_client: Arc<Mutex<StorageClient>>,
    
    // 配置
    config: ReplicaConfig,
    
    // 统计
    stats: Arc<RwLock<ReplicaStats>>,
}

pub struct ReplicaConfig {
    // Primary地址
    primary_addr: String,
    
    // 缓存大小（字节）
    cache_size: usize,
    
    // 预热策略
    warmup_strategy: WarmupStrategy,
    
    // 缓存失效策略
    invalidation_policy: InvalidationPolicy,
}

#[derive(Clone)]
pub struct CachedValue {
    value: Vec<u8>,
    cached_at: Instant,
    access_count: u64,
}

impl ReplicaNode {
    pub async fn new(config: ReplicaConfig) -> Result<Self> {
        // 1. 连接Primary
        let primary_client = Arc::new(Mutex::new(
            StorageClient::connect(&config.primary_addr).await?
        ));
        
        // 2. 创建缓存
        let cache_capacity = config.cache_size / 1024; // 假设平均1KB/entry
        let cache = Arc::new(RwLock::new(
            LruCache::new(cache_capacity.try_into().unwrap())
        ));
        
        // 3. 初始化统计
        let stats = Arc::new(RwLock::new(ReplicaStats::default()));
        
        let node = Self {
            cache,
            primary_client,
            config,
            stats,
        };
        
        // 4. 预热（如果配置）
        node.warmup().await?;
        
        Ok(node)
    }
    
    // 读取操作
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let start = Instant::now();
        
        // 1. 查缓存
        {
            let mut cache = self.cache.write();
            if let Some(cached) = cache.get_mut(key) {
                // 缓存命中
                cached.access_count += 1;
                self.stats.write().record_cache_hit(start.elapsed());
                return Ok(Some(cached.value.clone()));
            }
        }
        
        // 2. 缓存miss，转发到Primary
        let mut client = self.primary_client.lock().await;
        let value = client.get(key).await?;
        
        // 3. 更新缓存
        if let Some(ref v) = value {
            let mut cache = self.cache.write();
            cache.put(key.to_vec(), CachedValue {
                value: v.clone(),
                cached_at: Instant::now(),
                access_count: 1,
            });
        }
        
        self.stats.write().record_cache_miss(start.elapsed());
        Ok(value)
    }
    
    // 写操作（转发到Primary）
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 转发到Primary
        let mut client = self.primary_client.lock().await;
        client.put(key, value).await?;
        
        // 2. 使缓存失效
        let mut cache = self.cache.write();
        cache.pop(key);
        
        Ok(())
    }
    
    // 预热
    async fn warmup(&self) -> Result<()> {
        match &self.config.warmup_strategy {
            WarmupStrategy::None => Ok(()),
            
            WarmupStrategy::HotKeys(keys) => {
                info!("Warming up {} hot keys", keys.len());
                let mut client = self.primary_client.lock().await;
                
                for key in keys {
                    if let Some(value) = client.get(key).await? {
                        let mut cache = self.cache.write();
                        cache.put(key.clone(), CachedValue {
                            value,
                            cached_at: Instant::now(),
                            access_count: 0,
                        });
                    }
                }
                
                Ok(())
            }
            
            WarmupStrategy::RangeScan { start, end, limit } => {
                info!("Warming up with range scan");
                // 实现范围扫描预热
                // ...
                Ok(())
            }
        }
    }
    
    // 缓存统计
    pub fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.read();
        let stats = self.stats.read();
        
        CacheStats {
            size: cache.len(),
            capacity: cache.cap(),
            hit_rate: stats.hit_rate(),
            // ...
        }
    }
}

#[derive(Default)]
pub struct ReplicaStats {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub forwarded_gets: u64,
    pub forwarded_puts: u64,
    // ...
}

impl ReplicaStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }
}

pub enum WarmupStrategy {
    None,
    HotKeys(Vec<Vec<u8>>),
    RangeScan {
        start: Vec<u8>,
        end: Vec<u8>,
        limit: usize,
    },
}
```

**任务清单**：
- [ ] 实现ReplicaNode结构
- [ ] LRU缓存集成
- [ ] RPC客户端连接管理
- [ ] 预热策略实现
- [ ] 缓存失效策略
- [ ] 统计信息
- [ ] 单元测试和集成测试

---

### Week 24: 网络层优化

**目标**：连接池、超时、重试、监控

```rust
// src/rpc/pool.rs
pub struct ConnectionPool {
    addr: String,
    pool: Vec<StorageClient>,
    max_size: usize,
    idle_timeout: Duration,
}

impl ConnectionPool {
    pub async fn new(addr: String, max_size: usize) -> Result<Self> {
        let mut pool = Vec::with_capacity(max_size);
        
        // 预创建一些连接
        for _ in 0..max_size.min(4) {
            let client = StorageClient::connect(&addr).await?;
            pool.push(client);
        }
        
        Ok(Self {
            addr,
            pool,
            max_size,
            idle_timeout: Duration::from_secs(60),
        })
    }
    
    pub async fn get_client(&mut self) -> Result<StorageClient> {
        if let Some(client) = self.pool.pop() {
            Ok(client)
        } else if self.pool.len() < self.max_size {
            StorageClient::connect(&self.addr).await
        } else {
            // 等待可用连接
            Err(Error::internal("Connection pool exhausted"))
        }
    }
    
    pub fn return_client(&mut self, client: StorageClient) {
        if self.pool.len() < self.max_size {
            self.pool.push(client);
        }
        // else: drop connection
    }
}

// src/rpc/retry.rs
pub struct RetryPolicy {
    max_retries: usize,
    backoff: ExponentialBackoff,
}

impl RetryPolicy {
    pub async fn execute<F, T>(&self, mut f: F) -> Result<T>
    where
        F: FnMut() -> BoxFuture<'static, Result<T>>,
    {
        let mut retries = 0;
        let mut delay = self.backoff.initial_delay;
        
        loop {
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) if retries < self.max_retries && e.is_retryable() => {
                    retries += 1;
                    warn!("Retry {} after error: {}", retries, e);
                    sleep(delay).await;
                    delay *= 2; // 指数退避
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

**任务清单**：
- [ ] 连接池实现
- [ ] 超时控制
- [ ] 重试策略
- [ ] 错误分类（可重试/不可重试）
- [ ] 指标收集（延迟、错误率）
- [ ] 压力测试

**阶段1交付物**：
- ✅ 完整的RPC框架
- ✅ Primary节点可通过RPC访问
- ✅ Replica节点可缓存和转发
- ✅ 性能测试通过

---

## 📋 阶段2: Coordinator（Week 25-28）

### Week 25: 一致性哈希实现

**目标**：实现路由基础

```rust
// src/cluster/consistent_hash.rs
use std::collections::BTreeMap;

pub struct ConsistentHashRing {
    // 虚拟节点映射到实际节点
    ring: BTreeMap<u64, ShardId>,
    
    // 虚拟节点数量（每个实际节点）
    virtual_nodes: usize,
    
    // 节点列表
    nodes: HashMap<ShardId, NodeInfo>,
}

impl ConsistentHashRing {
    pub fn new(virtual_nodes: usize) -> Self {
        Self {
            ring: BTreeMap::new(),
            virtual_nodes,
            nodes: HashMap::new(),
        }
    }
    
    pub fn add_node(&mut self, shard_id: ShardId, info: NodeInfo) {
        // 添加虚拟节点
        for i in 0..self.virtual_nodes {
            let key = format!("{}-{}", shard_id, i);
            let hash = hash_key(key.as_bytes());
            self.ring.insert(hash, shard_id);
        }
        
        self.nodes.insert(shard_id, info);
    }
    
    pub fn remove_node(&mut self, shard_id: ShardId) {
        // 移除虚拟节点
        for i in 0..self.virtual_nodes {
            let key = format!("{}-{}", shard_id, i);
            let hash = hash_key(key.as_bytes());
            self.ring.remove(&hash);
        }
        
        self.nodes.remove(&shard_id);
    }
    
    pub fn get_node(&self, key: &[u8]) -> Option<ShardId> {
        if self.ring.is_empty() {
            return None;
        }
        
        let hash = hash_key(key);
        
        // 找到第一个 >= hash 的节点
        self.ring.range(hash..)
            .next()
            .or_else(|| self.ring.iter().next())
            .map(|(_, shard_id)| *shard_id)
    }
}

fn hash_key(key: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}
```

**任务清单**：
- [ ] 一致性哈希实现
- [ ] 虚拟节点支持
- [ ] 节点增删
- [ ] 负载均衡验证
- [ ] 单元测试

---

### Week 26: Coordinator核心逻辑

**目标**：实现路由和分发

```rust
// src/cluster/coordinator.rs
pub struct Coordinator {
    // 一致性哈希环
    hash_ring: Arc<RwLock<ConsistentHashRing>>,
    
    // Shard Group映射
    shard_groups: Arc<RwLock<HashMap<ShardId, ShardGroup>>>,
    
    // 配置
    config: CoordinatorConfig,
    
    // 统计
    stats: Arc<RwLock<CoordinatorStats>>,
}

pub struct ShardGroup {
    pub shard_id: ShardId,
    pub primary: PrimaryInfo,
    pub replicas: Vec<ReplicaInfo>,
    pub health: ShardHealth,
}

pub struct PrimaryInfo {
    pub addr: String,
    pub client: Arc<Mutex<StorageClient>>,
}

pub struct ReplicaInfo {
    pub id: ReplicaId,
    pub addr: String,
    pub client: Arc<Mutex<StorageClient>>,
    pub load: AtomicU64, // 当前连接数/请求数
}

impl Coordinator {
    pub async fn new(config: CoordinatorConfig) -> Result<Self> {
        let hash_ring = Arc::new(RwLock::new(
            ConsistentHashRing::new(config.virtual_nodes)
        ));
        
        let shard_groups = Arc::new(RwLock::new(HashMap::new()));
        
        let stats = Arc::new(RwLock::new(CoordinatorStats::default()));
        
        Ok(Self {
            hash_ring,
            shard_groups,
            config,
            stats,
        })
    }
    
    // 注册Shard
    pub async fn register_shard(&self, shard: ShardGroup) -> Result<()> {
        let shard_id = shard.shard_id;
        
        // 1. 添加到hash ring
        self.hash_ring.write().add_node(shard_id, NodeInfo {
            addr: shard.primary.addr.clone(),
        });
        
        // 2. 注册shard group
        self.shard_groups.write().insert(shard_id, shard);
        
        info!("Registered shard {}", shard_id);
        Ok(())
    }
    
    // 路由key到对应shard
    fn route_key(&self, key: &[u8]) -> Result<ShardId> {
        let hash_ring = self.hash_ring.read();
        hash_ring.get_node(key)
            .ok_or_else(|| Error::internal("No shard available"))
    }
    
    // 写操作（路由到Primary）
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let start = Instant::now();
        
        // 1. 路由到shard
        let shard_id = self.route_key(key)?;
        
        // 2. 获取shard
        let shard_groups = self.shard_groups.read();
        let shard = shard_groups.get(&shard_id)
            .ok_or_else(|| Error::internal("Shard not found"))?;
        
        // 3. 写入Primary
        let mut client = shard.primary.client.lock().await;
        client.put(key, value).await?;
        
        // 4. 统计
        self.stats.write().record_put(start.elapsed());
        
        Ok(())
    }
    
    // 读操作（负载均衡到Replica或Primary）
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let start = Instant::now();
        
        // 1. 路由到shard
        let shard_id = self.route_key(key)?;
        
        // 2. 获取shard
        let shard_groups = self.shard_groups.read();
        let shard = shard_groups.get(&shard_id)
            .ok_or_else(|| Error::internal("Shard not found"))?;
        
        // 3. 选择节点（负载均衡）
        let node = self.select_read_node(shard)?;
        
        // 4. 读取
        let mut client = node.lock().await;
        let result = client.get(key).await?;
        
        // 5. 统计
        self.stats.write().record_get(start.elapsed(), result.is_some());
        
        Ok(result)
    }
    
    // 负载均衡选择读节点
    fn select_read_node(&self, shard: &ShardGroup) 
        -> Result<Arc<Mutex<StorageClient>>> {
        // 策略1: 轮询
        // 策略2: 最少连接
        // 策略3: 响应时间加权
        
        match self.config.load_balance_strategy {
            LoadBalanceStrategy::RoundRobin => {
                self.round_robin_select(shard)
            }
            LoadBalanceStrategy::LeastConnections => {
                self.least_connections_select(shard)
            }
            LoadBalanceStrategy::Random => {
                self.random_select(shard)
            }
        }
    }
    
    fn random_select(&self, shard: &ShardGroup) 
        -> Result<Arc<Mutex<StorageClient>>> {
        if shard.replicas.is_empty() {
            // 没有replica，用primary
            return Ok(shard.primary.client.clone());
        }
        
        // 随机选择replica
        let idx = rand::random::<usize>() % (shard.replicas.len() + 1);
        
        if idx == shard.replicas.len() {
            Ok(shard.primary.client.clone())
        } else {
            Ok(shard.replicas[idx].client.clone())
        }
    }
    
    fn least_connections_select(&self, shard: &ShardGroup) 
        -> Result<Arc<Mutex<StorageClient>>> {
        // 选择当前负载最低的节点
        let mut min_load = u64::MAX;
        let mut selected = None;
        
        // 检查primary
        // （假设primary也有load统计）
        
        // 检查replicas
        for replica in &shard.replicas {
            let load = replica.load.load(Ordering::Relaxed);
            if load < min_load {
                min_load = load;
                selected = Some(replica.client.clone());
            }
        }
        
        selected.or(Some(shard.primary.client.clone()))
            .ok_or_else(|| Error::internal("No node available"))
    }
}

pub enum LoadBalanceStrategy {
    RoundRobin,
    LeastConnections,
    Random,
}
```

**任务清单**：
- [ ] Coordinator核心结构
- [ ] Shard注册和管理
- [ ] 路由实现
- [ ] 负载均衡策略
- [ ] 统计收集
- [ ] 集成测试

---

### Week 27-28: 健康检查和故障处理

**目标**：监控节点健康，处理故障

```rust
// src/cluster/health.rs
pub struct HealthChecker {
    coordinator: Arc<Coordinator>,
    check_interval: Duration,
    timeout: Duration,
}

impl HealthChecker {
    pub async fn start(&self) {
        let mut interval = tokio::time::interval(self.check_interval);
        
        loop {
            interval.tick().await;
            self.check_all_shards().await;
        }
    }
    
    async fn check_all_shards(&self) {
        let shard_groups = self.coordinator.shard_groups.read().clone();
        
        for (shard_id, shard) in shard_groups {
            // 检查Primary
            if !self.check_primary(&shard).await {
                warn!("Primary of shard {} is unhealthy", shard_id);
                self.handle_primary_failure(shard_id).await;
            }
            
            // 检查Replicas
            for replica in &shard.replicas {
                if !self.check_replica(replica).await {
                    warn!("Replica {} of shard {} is unhealthy", 
                          replica.id, shard_id);
                    self.handle_replica_failure(shard_id, replica.id).await;
                }
            }
        }
    }
    
    async fn check_primary(&self, shard: &ShardGroup) -> bool {
        let mut client = shard.primary.client.lock().await;
        
        match timeout(self.timeout, client.health_check()).await {
            Ok(Ok(_)) => true,
            _ => false,
        }
    }
    
    async fn handle_primary_failure(&self, shard_id: ShardId) {
        // 1. 标记为不健康
        // 2. 停止路由写入到此shard
        // 3. 触发告警
        // 4. 等待Primary恢复或手动干预
        
        error!("Primary failure for shard {}, stopping writes", shard_id);
        
        // 可选：自动从备份恢复（后续实现）
    }
    
    async fn handle_replica_failure(&self, shard_id: ShardId, replica_id: ReplicaId) {
        // 从可用列表中移除
        let mut shard_groups = self.coordinator.shard_groups.write();
        if let Some(shard) = shard_groups.get_mut(&shard_id) {
            shard.replicas.retain(|r| r.id != replica_id);
        }
        
        warn!("Removed failed replica {} from shard {}", replica_id, shard_id);
    }
}
```

**任务清单**：
- [ ] 健康检查实现
- [ ] 定期检测
- [ ] 故障处理
- [ ] 告警集成
- [ ] 自动恢复（可选）
- [ ] 故障注入测试

**阶段2交付物**：
- ✅ Coordinator可以路由请求
- ✅ 负载均衡工作正常
- ✅ 健康检查和故障处理
- ✅ 多shard测试通过

---

## 📋 阶段3: Shard Group（Week 29-34）

### Week 29-30: 完整的Shard Group实现

**目标**：整合Primary和Replica，形成完整的Shard

```rust
// src/cluster/shard_group.rs
pub struct ShardGroupManager {
    config: ShardGroupConfig,
    primary: Option<PrimaryNode>,
    replicas: Vec<ReplicaNode>,
    state: ShardState,
}

pub struct ShardGroupConfig {
    pub shard_id: ShardId,
    pub data_dir: PathBuf,
    pub primary_config: PrimaryConfig,
    pub replica_configs: Vec<ReplicaConfig>,
}

#[derive(Debug, Clone)]
pub enum ShardState {
    Initializing,
    Running,
    Degraded,  // Primary或部分Replica不可用
    Stopped,
}

impl ShardGroupManager {
    pub async fn new(config: ShardGroupConfig) -> Result<Self> {
        Ok(Self {
            config,
            primary: None,
            replicas: Vec::new(),
            state: ShardState::Initializing,
        })
    }
    
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting shard group {}", self.config.shard_id);
        
        // 1. 启动Primary
        let mut primary = PrimaryNode::new(self.config.primary_config.clone()).await?;
        primary.start().await?;
        self.primary = Some(primary);
        
        // 2. 启动Replicas
        for replica_config in &self.config.replica_configs {
            let replica = ReplicaNode::new(replica_config.clone()).await?;
            self.replicas.push(replica);
        }
        
        self.state = ShardState::Running;
        info!("Shard group {} is running", self.config.shard_id);
        
        Ok(())
    }
    
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping shard group {}", self.config.shard_id);
        
        // 1. 停止Replicas
        self.replicas.clear();
        
        // 2. 停止Primary
        if let Some(mut primary) = self.primary.take() {
            primary.stop().await?;
        }
        
        self.state = ShardState::Stopped;
        Ok(())
    }
    
    // 添加Replica
    pub async fn add_replica(&mut self, config: ReplicaConfig) -> Result<()> {
        let replica = ReplicaNode::new(config).await?;
        self.replicas.push(replica);
        
        info!("Added replica to shard {}", self.config.shard_id);
        Ok(())
    }
    
    // 移除Replica
    pub async fn remove_replica(&mut self, replica_id: ReplicaId) -> Result<()> {
        self.replicas.retain(|r| r.id() != replica_id);
        
        info!("Removed replica {} from shard {}", replica_id, self.config.shard_id);
        Ok(())
    }
}
```

**任务清单**：
- [ ] ShardGroupManager实现
- [ ] Primary和Replica生命周期管理
- [ ] 动态添加/移除Replica
- [ ] 状态管理
- [ ] 集成测试

---

### Week 31-32: 多Shard集成测试

**目标**：验证多个Shard同时运行

```rust
// tests/multi_shard_test.rs
#[tokio::test]
async fn test_multi_shard_cluster() -> Result<()> {
    // 1. 启动Coordinator
    let coordinator = Coordinator::new(CoordinatorConfig::default()).await?;
    
    // 2. 创建3个Shard Groups
    for i in 0..3 {
        let shard_id = ShardId::new(i);
        let primary_addr = format!("127.0.0.1:{}", 9000 + i);
        
        // 启动Primary
        let primary_config = PrimaryConfig {
            listen_addr: primary_addr.parse()?,
            db_path: format!("/tmp/test_shard_{}", i).into(),
            ..Default::default()
        };
        let mut primary = PrimaryNode::new(primary_config).await?;
        primary.start().await?;
        
        // 创建Shard Group
        let shard_group = ShardGroup {
            shard_id,
            primary: PrimaryInfo {
                addr: primary_addr.clone(),
                client: Arc::new(Mutex::new(
                    StorageClient::connect(&primary_addr).await?
                )),
            },
            replicas: vec![],
            health: ShardHealth::Healthy,
        };
        
        // 注册到Coordinator
        coordinator.register_shard(shard_group).await?;
    }
    
    // 3. 写入测试数据
    for i in 0..1000 {
        let key = format!("key{}", i).into_bytes();
        let value = format!("value{}", i).into_bytes();
        coordinator.put(&key, &value).await?;
    }
    
    // 4. 验证读取
    for i in 0..1000 {
        let key = format!("key{}", i).into_bytes();
        let value = coordinator.get(&key).await?;
        assert_eq!(value, Some(format!("value{}", i).into_bytes()));
    }
    
    // 5. 验证数据分布
    // 检查每个shard的数据量大致均衡
    
    Ok(())
}
```

**任务清单**：
- [ ] 多shard启动测试
- [ ] 数据分布验证
- [ ] 负载均衡测试
- [ ] 故障场景测试
- [ ] 性能测试

---

### Week 33-34: 性能优化和压力测试

**目标**：达到性能目标

**性能测试**：
```rust
// benches/cluster_bench.rs
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn bench_cluster_write(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let coordinator = runtime.block_on(setup_cluster()).unwrap();
    
    let mut group = c.benchmark_group("cluster_write");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("put", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let key = format!("key{}", rand::random::<u64>()).into_bytes();
                let value = vec![0u8; 1024]; // 1KB value
                coordinator.put(&key, &value).await.unwrap();
            });
        });
    });
    
    group.finish();
}

fn bench_cluster_read(c: &mut Criterion) {
    // 类似实现...
}

criterion_group!(benches, bench_cluster_write, bench_cluster_read);
criterion_main!(benches);
```

**性能目标**：

| 场景 | 目标 | 说明 |
|------|------|------|
| 单shard写入 | 50K ops/s | 受单机DB性能限制 |
| 10 shards写入 | 500K ops/s | 线性扩展 |
| Replica缓存命中读 | 500K ops/s | 内存缓存 |
| Replica缓存miss读 | 30K ops/s | RPC转发开销 |

**任务清单**：
- [ ] 性能基准测试
- [ ] 瓶颈识别和优化
- [ ] 压力测试（长时间运行）
- [ ] 内存泄漏检查
- [ ] 性能文档

**阶段3交付物**：
- ✅ 完整的Shard Group
- ✅ 多shard协同工作
- ✅ 性能达标
- ✅ 稳定性验证

---

## 📋 阶段4: Backup和恢复（Week 35-40）

### Week 35-36: 备份管理器

**目标**：实现异步备份到网盘

```rust
// src/backup/manager.rs
pub struct BackupManager {
    db: Arc<DB>,
    storage: Arc<dyn BackupStorage>,
    config: BackupConfig,
    state: Arc<RwLock<BackupState>>,
}

pub struct BackupConfig {
    // 快照间隔
    snapshot_interval: Duration,
    
    // WAL归档间隔
    wal_archive_interval: Duration,
    
    // 保留策略
    retention_policy: RetentionPolicy,
    
    // 并发度
    concurrent_uploads: usize,
}

pub struct RetentionPolicy {
    // 保留最近N个快照
    keep_snapshots: usize,
    
    // 保留N天内的WAL
    keep_wal_days: u32,
}

// 备份存储抽象
#[async_trait]
pub trait BackupStorage: Send + Sync {
    async fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()>;
    async fn download_file(&self, remote_path: &str, local_path: &Path) -> Result<()>;
    async fn list_files(&self, prefix: &str) -> Result<Vec<String>>;
    async fn delete_file(&self, remote_path: &str) -> Result<()>;
}

// S3/OSS实现
pub struct S3Storage {
    bucket: String,
    client: aws_sdk_s3::Client,
}

#[async_trait]
impl BackupStorage for S3Storage {
    async fn upload_file(&self, local_path: &Path, remote_path: &str) -> Result<()> {
        let body = ByteStream::from_path(local_path).await?;
        
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(remote_path)
            .body(body)
            .send()
            .await?;
        
        Ok(())
    }
    
    // 其他方法实现...
}

impl BackupManager {
    pub async fn new(
        db: Arc<DB>,
        storage: Arc<dyn BackupStorage>,
        config: BackupConfig
    ) -> Result<Self> {
        Ok(Self {
            db,
            storage,
            config,
            state: Arc::new(RwLock::new(BackupState::default())),
        })
    }
    
    // 启动后台备份任务
    pub async fn start(&self) -> Result<()> {
        let manager = self.clone();
        
        tokio::spawn(async move {
            manager.backup_loop().await;
        });
        
        Ok(())
    }
    
    async fn backup_loop(&self) {
        let mut snapshot_timer = tokio::time::interval(self.config.snapshot_interval);
        let mut wal_timer = tokio::time::interval(self.config.wal_archive_interval);
        
        loop {
            tokio::select! {
                _ = snapshot_timer.tick() => {
                    if let Err(e) = self.create_snapshot().await {
                        error!("Snapshot failed: {}", e);
                    }
                }
                _ = wal_timer.tick() => {
                    if let Err(e) = self.archive_wal().await {
                        error!("WAL archive failed: {}", e);
                    }
                }
            }
        }
    }
    
    // 创建快照
    pub async fn create_snapshot(&self) -> Result<SnapshotId> {
        let snapshot_id = SnapshotId::new();
        info!("Creating snapshot {}", snapshot_id);
        
        // 1. 触发DB checkpoint
        let checkpoint = self.db.create_checkpoint()?;
        
        // 2. 上传SSTable文件
        for sstable_path in &checkpoint.sstables {
            let remote_path = format!("snapshots/{}/{}", 
                                     snapshot_id, 
                                     sstable_path.file_name().unwrap().to_str().unwrap());
            self.storage.upload_file(sstable_path, &remote_path).await?;
        }
        
        // 3. 上传Manifest
        let manifest_remote = format!("snapshots/{}/MANIFEST", snapshot_id);
        self.storage.upload_file(&checkpoint.manifest, &manifest_remote).await?;
        
        // 4. 写入元数据
        let metadata = SnapshotMetadata {
            id: snapshot_id.clone(),
            created_at: SystemTime::now(),
            file_count: checkpoint.sstables.len(),
            total_size: checkpoint.total_size,
        };
        self.save_snapshot_metadata(&metadata).await?;
        
        // 5. 更新状态
        self.state.write().last_snapshot = Some(snapshot_id.clone());
        
        info!("Snapshot {} created successfully", snapshot_id);
        Ok(snapshot_id)
    }
    
    // 归档WAL
    pub async fn archive_wal(&self) -> Result<()> {
        let last_archived = self.state.read().last_archived_wal;
        
        // 1. 获取需要归档的WAL文件
        let wal_files = self.db.get_wal_files_since(last_archived)?;
        
        if wal_files.is_empty() {
            return Ok(());
        }
        
        info!("Archiving {} WAL files", wal_files.len());
        
        // 2. 并发上传
        let mut tasks = Vec::new();
        for wal_file in &wal_files {
            let storage = self.storage.clone();
            let file_path = wal_file.path.clone();
            let remote_path = format!("wal/{}", wal_file.id);
            
            let task = tokio::spawn(async move {
                storage.upload_file(&file_path, &remote_path).await
            });
            
            tasks.push(task);
        }
        
        // 3. 等待全部完成
        for task in tasks {
            task.await??;
        }
        
        // 4. 更新状态
        if let Some(last_wal) = wal_files.last() {
            self.state.write().last_archived_wal = Some(last_wal.id);
        }
        
        info!("WAL archiving completed");
        Ok(())
    }
    
    // 清理旧备份
    pub async fn cleanup_old_backups(&self) -> Result<()> {
        // 根据retention policy清理
        // ...
        Ok(())
    }
}
```

**任务清单**：
- [ ] BackupManager实现
- [ ] S3/OSS存储适配
- [ ] 快照创建
- [ ] WAL归档
- [ ] 保留策略
- [ ] 测试

---

### Week 37-38: 恢复机制

**目标**：从备份恢复数据

```rust
// src/backup/recovery.rs
pub struct RecoveryManager {
    storage: Arc<dyn BackupStorage>,
    target_dir: PathBuf,
}

impl RecoveryManager {
    pub async fn recover_from_snapshot(
        &self,
        snapshot_id: SnapshotId
    ) -> Result<()> {
        info!("Recovering from snapshot {}", snapshot_id);
        
        // 1. 下载snapshot元数据
        let metadata = self.load_snapshot_metadata(&snapshot_id).await?;
        
        // 2. 下载所有SSTable文件
        let sstable_files = self.storage
            .list_files(&format!("snapshots/{}/", snapshot_id))
            .await?;
        
        info!("Downloading {} files", sstable_files.len());
        
        let mut tasks = Vec::new();
        for remote_file in sstable_files {
            let local_file = self.target_dir.join(
                Path::new(&remote_file).file_name().unwrap()
            );
            let storage = self.storage.clone();
            
            let task = tokio::spawn(async move {
                storage.download_file(&remote_file, &local_file).await
            });
            
            tasks.push(task);
        }
        
        for task in tasks {
            task.await??;
        }
        
        // 3. 下载Manifest
        let manifest_remote = format!("snapshots/{}/MANIFEST", snapshot_id);
        let manifest_local = self.target_dir.join("MANIFEST");
        self.storage.download_file(&manifest_remote, &manifest_local).await?;
        
        info!("Snapshot recovery completed");
        Ok(())
    }
    
    pub async fn replay_wal(
        &self,
        from_wal: Option<WalId>,
        db: &mut DB
    ) -> Result<()> {
        info!("Replaying WAL from {:?}", from_wal);
        
        // 1. 列出需要replay的WAL文件
        let wal_files = if let Some(from) = from_wal {
            self.storage.list_files(&format!("wal/{}*", from)).await?
        } else {
            self.storage.list_files("wal/").await?
        };
        
        info!("Found {} WAL files to replay", wal_files.len());
        
        // 2. 按顺序下载和replay
        for wal_file in wal_files {
            let local_file = self.target_dir.join("temp_wal");
            self.storage.download_file(&wal_file, &local_file).await?;
            
            // Replay到DB
            db.replay_wal(&local_file)?;
            
            // 删除临时文件
            fs::remove_file(&local_file)?;
        }
        
        info!("WAL replay completed");
        Ok(())
    }
    
    // 完整恢复流程
    pub async fn full_recovery(&self, snapshot_id: Option<SnapshotId>) -> Result<DB> {
        // 1. 恢复快照（如果指定）
        let snapshot_id = if let Some(id) = snapshot_id {
            id
        } else {
            // 找到最新的快照
            self.find_latest_snapshot().await?
        };
        
        self.recover_from_snapshot(snapshot_id.clone()).await?;
        
        // 2. 打开DB
        let mut db = DB::open(&self.target_dir, Options::default())?;
        
        // 3. Replay快照之后的WAL
        self.replay_wal(Some(snapshot_id.wal_position), &mut db).await?;
        
        Ok(db)
    }
}
```

**任务清单**：
- [ ] RecoveryManager实现
- [ ] 快照下载
- [ ] WAL replay
- [ ] 完整恢复流程
- [ ] 恢复测试
- [ ] 灾难恢复演练

---

### Week 39-40: 备份恢复集成测试

**目标**：验证端到端备份恢复

```rust
#[tokio::test]
async fn test_backup_and_recovery() -> Result<()> {
    // 1. 创建DB并写入数据
    let primary_config = PrimaryConfig {
        db_path: "/tmp/test_primary".into(),
        ..Default::default()
    };
    let mut primary = PrimaryNode::new(primary_config).await?;
    primary.start().await?;
    
    // 写入测试数据
    for i in 0..10000 {
        let key = format!("key{}", i).into_bytes();
        let value = format!("value{}", i).into_bytes();
        primary.put(&key, &value).await?;
    }
    
    // 2. 创建备份
    let storage = Arc::new(S3Storage::new("test-bucket"));
    let backup_config = BackupConfig {
        snapshot_interval: Duration::from_secs(60),
        ..Default::default()
    };
    let backup_mgr = BackupManager::new(
        primary.db.clone(),
        storage.clone(),
        backup_config
    ).await?;
    
    let snapshot_id = backup_mgr.create_snapshot().await?;
    
    // 3. 停止原DB
    primary.stop().await?;
    fs::remove_dir_all("/tmp/test_primary")?;
    
    // 4. 从备份恢复
    let recovery_mgr = RecoveryManager {
        storage,
        target_dir: "/tmp/test_recovery".into(),
    };
    
    let recovered_db = recovery_mgr.full_recovery(Some(snapshot_id)).await?;
    
    // 5. 验证数据
    for i in 0..10000 {
        let key = format!("key{}", i).into_bytes();
        let value = recovered_db.get(&key)?;
        assert_eq!(value, Some(format!("value{}", i).into_bytes()));
    }
    
    Ok(())
}
```

**任务清单**：
- [ ] 端到端测试
- [ ] 故障注入测试
- [ ] 大数据量测试
- [ ] 性能测试（备份和恢复速度）
- [ ] 文档

**阶段4交付物**：
- ✅ 异步备份到网盘
- ✅ 从备份恢复
- ✅ 完整的灾难恢复方案
- ✅ 测试验证

---

## 📋 阶段5: 弹性伸缩（Week 41-44）

### Week 41-42: 动态扩展实现

**目标**：支持在线添加/移除节点

```rust
// src/cluster/scaling.rs
pub struct ScalingManager {
    coordinator: Arc<Coordinator>,
}

impl ScalingManager {
    // 添加新Shard
    pub async fn add_shard(
        &self,
        primary_addr: String,
        data_dir: PathBuf
    ) -> Result<ShardId> {
        let shard_id = ShardId::new_random();
        
        info!("Adding new shard {}", shard_id);
        
        // 1. 启动新的Primary
        let primary_config = PrimaryConfig {
            listen_addr: primary_addr.parse()?,
            db_path: data_dir,
            ..Default::default()
        };
        let mut primary = PrimaryNode::new(primary_config).await?;
        primary.start().await?;
        
        // 2. 创建Shard Group
        let shard_group = ShardGroup {
            shard_id,
            primary: PrimaryInfo {
                addr: primary_addr.clone(),
                client: Arc::new(Mutex::new(
                    StorageClient::connect(&primary_addr).await?
                )),
            },
            replicas: vec![],
            health: ShardHealth::Healthy,
        };
        
        // 3. 注册到Coordinator
        self.coordinator.register_shard(shard_group).await?;
        
        info!("Shard {} added successfully", shard_id);
        Ok(shard_id)
    }
    
    // 添加Replica到现有Shard
    pub async fn add_replica(
        &self,
        shard_id: ShardId,
        replica_addr: String
    ) -> Result<ReplicaId> {
        let replica_id = ReplicaId::new_random();
        
        info!("Adding replica {} to shard {}", replica_id, shard_id);
        
        // 1. 获取Primary地址
        let primary_addr = {
            let shard_groups = self.coordinator.shard_groups.read();
            let shard = shard_groups.get(&shard_id)
                .ok_or_else(|| Error::not_found("Shard not found"))?;
            shard.primary.addr.clone()
        };
        
        // 2. 创建Replica
        let replica_config = ReplicaConfig {
            primary_addr,
            cache_size: 1024 * 1024 * 1024, // 1GB
            warmup_strategy: WarmupStrategy::None, // 懒加载
            ..Default::default()
        };
        let replica = ReplicaNode::new(replica_config).await?;
        
        // 3. 添加到Shard Group
        let mut shard_groups = self.coordinator.shard_groups.write();
        let shard = shard_groups.get_mut(&shard_id)
            .ok_or_else(|| Error::not_found("Shard not found"))?;
        
        shard.replicas.push(ReplicaInfo {
            id: replica_id,
            addr: replica_addr,
            client: Arc::new(Mutex::new(
                StorageClient::connect(&shard.primary.addr).await?
            )),
            load: AtomicU64::new(0),
        });
        
        info!("Replica {} added to shard {}", replica_id, shard_id);
        Ok(replica_id)
    }
    
    // 移除Replica
    pub async fn remove_replica(
        &self,
        shard_id: ShardId,
        replica_id: ReplicaId
    ) -> Result<()> {
        info!("Removing replica {} from shard {}", replica_id, shard_id);
        
        let mut shard_groups = self.coordinator.shard_groups.write();
        let shard = shard_groups.get_mut(&shard_id)
            .ok_or_else(|| Error::not_found("Shard not found"))?;
        
        shard.replicas.retain(|r| r.id != replica_id);
        
        info!("Replica {} removed from shard {}", replica_id, shard_id);
        Ok(())
    }
    
    // 移除Shard（需要谨慎操作）
    pub async fn remove_shard(&self, shard_id: ShardId) -> Result<()> {
        warn!("Removing shard {} - this will make its data inaccessible", shard_id);
        
        // 1. 从hash ring移除（停止新请求）
        self.coordinator.hash_ring.write().remove_node(shard_id);
        
        // 2. 等待现有请求完成
        tokio::time::sleep(Duration::from_secs(5)).await;
        
        // 3. 创建最终备份
        let shard_groups = self.coordinator.shard_groups.read();
        let shard = shard_groups.get(&shard_id)
            .ok_or_else(|| Error::not_found("Shard not found"))?;
        
        // TODO: 触发备份
        
        // 4. 从Coordinator移除
        drop(shard_groups);
        self.coordinator.shard_groups.write().remove(&shard_id);
        
        warn!("Shard {} removed", shard_id);
        Ok(())
    }
}
```

**任务清单**：
- [ ] 添加Shard实现
- [ ] 添加Replica实现
- [ ] 移除节点实现
- [ ] 安全检查
- [ ] 测试

---

### Week 43-44: 自动伸缩（可选）

**目标**：基于负载自动扩缩容

```rust
// src/cluster/autoscaler.rs
pub struct AutoScaler {
    coordinator: Arc<Coordinator>,
    scaling_mgr: Arc<ScalingManager>,
    config: AutoScalerConfig,
}

pub struct AutoScalerConfig {
    // Replica伸缩阈值
    replica_scale_up_threshold: f64,    // CPU > 80%
    replica_scale_down_threshold: f64,  // CPU < 20%
    
    // Shard伸缩阈值
    shard_scale_up_threshold: f64,      // 所有shard负载高
    
    // 检查间隔
    check_interval: Duration,
    
    // 冷却时间
    cooldown: Duration,
}

impl AutoScaler {
    pub async fn start(&self) {
        let mut interval = tokio::time::interval(self.config.check_interval);
        
        loop {
            interval.tick().await;
            
            if let Err(e) = self.evaluate_and_scale().await {
                error!("Auto-scaling error: {}", e);
            }
        }
    }
    
    async fn evaluate_and_scale(&self) -> Result<()> {
        // 1. 收集所有shard的指标
        let metrics = self.collect_metrics().await?;
        
        // 2. 评估是否需要扩展Replica
        for (shard_id, shard_metrics) in &metrics.shards {
            if shard_metrics.avg_cpu > self.config.replica_scale_up_threshold {
                info!("Shard {} is overloaded, adding replica", shard_id);
                // 添加新replica
                self.scaling_mgr.add_replica(*shard_id, "...".to_string()).await?;
            } else if shard_metrics.avg_cpu < self.config.replica_scale_down_threshold
                && shard_metrics.replica_count > 1 {
                info!("Shard {} is underloaded, removing replica", shard_id);
                // 移除一个replica
                // ...
            }
        }
        
        // 3. 评估是否需要添加Shard
        if metrics.overall_load > self.config.shard_scale_up_threshold {
            info!("Overall load is high, adding new shard");
            self.scaling_mgr.add_shard("...".to_string(), PathBuf::new()).await?;
        }
        
        Ok(())
    }
}
```

**任务清单**：
- [ ] 指标收集
- [ ] 伸缩策略实现
- [ ] 冷却时间控制
- [ ] 测试
- [ ] 文档

**阶段5交付物**：
- ✅ 手动添加/移除节点
- ✅ 自动伸缩（可选）
- ✅ 测试验证
- ✅ 运维文档

---

## 📋 阶段6: 监控和运维（Week 45-48）

### Week 45-46: 监控指标

**目标**：完整的可观测性

```rust
// src/metrics/mod.rs
use prometheus::{Registry, Counter, Histogram, Gauge};

pub struct Metrics {
    registry: Registry,
    
    // 请求指标
    pub requests_total: Counter,
    pub request_duration: Histogram,
    
    // 数据指标
    pub keys_total: Gauge,
    pub data_size_bytes: Gauge,
    
    // Shard指标
    pub shards_total: Gauge,
    pub replicas_per_shard: Histogram,
    
    // 缓存指标
    pub cache_hits_total: Counter,
    pub cache_misses_total: Counter,
    pub cache_size_bytes: Gauge,
    
    // 备份指标
    pub backup_last_success_timestamp: Gauge,
    pub backup_duration_seconds: Histogram,
}

impl Metrics {
    pub fn new() -> Result<Self> {
        let registry = Registry::new();
        
        // 注册所有指标...
        
        Ok(Self {
            registry,
            // ...
        })
    }
    
    pub fn export(&self) -> String {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        
        let mut buffer = vec![];
        encoder.encode(&self.registry.gather(), &mut buffer).unwrap();
        
        String::from_utf8(buffer).unwrap()
    }
}

// HTTP服务导出指标
pub async fn metrics_server(metrics: Arc<Metrics>, addr: SocketAddr) {
    let app = Router::new()
        .route("/metrics", get(|| async move {
            metrics.export()
        }));
    
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
```

**任务清单**：
- [ ] Prometheus指标定义
- [ ] 指标收集点埋点
- [ ] HTTP metrics endpoint
- [ ] Grafana dashboard
- [ ] 告警规则
- [ ] 文档

---

### Week 47-48: 运维工具

**目标**：方便的运维命令

```rust
// src/bin/aidb-admin.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aidb-admin")]
#[command(about = "AiDb cluster administration tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all shards
    ListShards {
        #[arg(short, long)]
        coordinator_addr: String,
    },
    
    /// Add a new shard
    AddShard {
        #[arg(long)]
        coordinator_addr: String,
        #[arg(long)]
        primary_addr: String,
        #[arg(long)]
        data_dir: String,
    },
    
    /// Add a replica to a shard
    AddReplica {
        #[arg(long)]
        coordinator_addr: String,
        #[arg(long)]
        shard_id: String,
        #[arg(long)]
        replica_addr: String,
    },
    
    /// Remove a replica
    RemoveReplica {
        #[arg(long)]
        coordinator_addr: String,
        #[arg(long)]
        shard_id: String,
        #[arg(long)]
        replica_id: String,
    },
    
    /// Show cluster status
    Status {
        #[arg(short, long)]
        coordinator_addr: String,
    },
    
    /// Create a backup
    Backup {
        #[arg(long)]
        shard_id: String,
    },
    
    /// Recover from backup
    Recover {
        #[arg(long)]
        snapshot_id: String,
        #[arg(long)]
        target_dir: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::ListShards { coordinator_addr } => {
            // 连接coordinator并列出所有shard
            let client = AdminClient::connect(&coordinator_addr).await?;
            let shards = client.list_shards().await?;
            
            for shard in shards {
                println!("Shard {}: {} replicas, status: {:?}", 
                        shard.id, shard.replica_count, shard.status);
            }
        }
        
        Commands::AddShard { coordinator_addr, primary_addr, data_dir } => {
            let client = AdminClient::connect(&coordinator_addr).await?;
            let shard_id = client.add_shard(primary_addr, data_dir).await?;
            
            println!("Shard {} added successfully", shard_id);
        }
        
        Commands::Status { coordinator_addr } => {
            let client = AdminClient::connect(&coordinator_addr).await?;
            let status = client.get_status().await?;
            
            println!("Cluster Status:");
            println!("  Shards: {}", status.shard_count);
            println!("  Total replicas: {}", status.replica_count);
            println!("  Healthy shards: {}", status.healthy_shards);
            println!("  Total keys: {}", status.total_keys);
            println!("  Total size: {} GB", status.total_size_gb);
        }
        
        // 其他命令...
        _ => {}
    }
    
    Ok(())
}
```

**任务清单**：
- [ ] 命令行工具实现
- [ ] 运维脚本
- [ ] 部署文档
- [ ] 故障排查指南
- [ ] 最佳实践文档

**阶段6交付物**：
- ✅ 完整的监控系统
- ✅ 运维工具
- ✅ 文档完善
- ✅ 生产就绪

---

## 📊 总体时间表

```
Week 1-20:  阶段0 - 单机版LSM引擎 ✅ 已规划
Week 21-24: 阶段1 - RPC和网络层
Week 25-28: 阶段2 - Coordinator
Week 29-34: 阶段3 - Shard Group
Week 35-40: 阶段4 - Backup和恢复
Week 41-44: 阶段5 - 弹性伸缩
Week 45-48: 阶段6 - 监控和运维

总计: 48周 (约12个月)
```

---

## 🎯 成功标准

### 功能完整性
- ✅ 多Shard分片存储
- ✅ Primary + Replicas架构
- ✅ 路由和负载均衡
- ✅ 异步备份和恢复
- ✅ 弹性伸缩

### 性能目标

| 指标 | 目标 |
|------|------|
| 单shard写入 | 50K ops/s |
| 10 shards写入 | 500K ops/s |
| Replica缓存命中读 | 500K ops/s |
| Replica转发读 | 30K ops/s |
| P99延迟(写) | < 5ms |
| P99延迟(缓存命中读) | < 1ms |

### 可用性目标
- 单shard恢复时间：< 5分钟
- Replica故障影响：无（自动切换）
- 数据丢失窗口：< 10分钟（WAL归档频率）

### 成本目标
- 相比全量复制：存储成本降低50-60%
- 网络成本降低80%+

---

## 📚 参考文档

### 已有文档
- `OPTIMIZED_PLAN.md` - 单机版实施计划
- `ROCKSDB_LESSONS.md` - RocksDB经验借鉴
- `SCALABLE_CLUSTER_DESIGN.md` - 集群架构设计
- `SHARED_STORAGE_REEVALUATION.md` - 架构评估

### 待完成文档
- API文档
- 运维手册
- 故障排查指南
- 性能调优指南

---

## 下一步行动

1. **确认计划**：这个实施计划是否符合预期？
2. **资源分配**：需要几个开发人员？
3. **开始实施**：从阶段0（单机版）开始

---

*完整的弹性集群实施计划制定完成！*

*预计12个月完成单机版 + 完整集群功能*
