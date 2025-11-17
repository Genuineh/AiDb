//! Primary node implementation
//!
//! A Primary node wraps the full DB instance and exposes it via RPC.
//! It handles all read and write operations with full persistence.

use parking_lot::RwLock;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::cluster::rpc::{
    self, proto,
    storage_server::{Storage, StorageServer},
};
use crate::DB;

/// Statistics for Primary node
#[derive(Debug, Default)]
pub struct PrimaryStats {
    /// Total number of requests received
    pub total_requests: u64,
    /// Number of GET requests
    pub get_requests: u64,
    /// Number of PUT requests
    pub put_requests: u64,
    /// Number of DELETE requests
    pub delete_requests: u64,
    /// Number of errors encountered
    pub errors: u64,
}

/// Primary node that hosts the full DB and serves RPC requests
pub struct PrimaryNode {
    db: Arc<DB>,
    stats: Arc<RwLock<PrimaryStats>>,
}

impl PrimaryNode {
    /// Create a new Primary node wrapping a DB instance
    pub fn new(db: Arc<DB>) -> Self {
        Self { db, stats: Arc::new(RwLock::new(PrimaryStats::default())) }
    }

    /// Get the statistics
    pub fn stats(&self) -> PrimaryStats {
        self.stats.read().clone()
    }

    /// Create a gRPC server
    pub fn into_server(self) -> StorageServer<Self> {
        StorageServer::new(self)
    }

    /// Start the RPC server on the given address
    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let server = self.into_server();

        tonic::transport::Server::builder().add_service(server).serve(addr).await?;

        Ok(())
    }
}

#[tonic::async_trait]
impl Storage for PrimaryNode {
    async fn get(
        &self,
        request: Request<proto::GetRequest>,
    ) -> Result<Response<proto::GetResponse>, Status> {
        let mut stats = self.stats.write();
        stats.total_requests += 1;
        stats.get_requests += 1;
        drop(stats);

        let req = request.into_inner();

        match self.db.get(&req.key) {
            Ok(Some(value)) => Ok(Response::new(proto::GetResponse { found: true, value })),
            Ok(None) => Ok(Response::new(proto::GetResponse { found: false, value: vec![] })),
            Err(e) => {
                self.stats.write().errors += 1;
                Err(rpc::to_status(e))
            }
        }
    }

    async fn put(
        &self,
        request: Request<proto::PutRequest>,
    ) -> Result<Response<proto::PutResponse>, Status> {
        let mut stats = self.stats.write();
        stats.total_requests += 1;
        stats.put_requests += 1;
        drop(stats);

        let req = request.into_inner();

        match self.db.put(&req.key, &req.value) {
            Ok(()) => Ok(Response::new(proto::PutResponse { success: true, error: String::new() })),
            Err(e) => {
                self.stats.write().errors += 1;
                Ok(Response::new(proto::PutResponse { success: false, error: e.to_string() }))
            }
        }
    }

    async fn delete(
        &self,
        request: Request<proto::DeleteRequest>,
    ) -> Result<Response<proto::DeleteResponse>, Status> {
        let mut stats = self.stats.write();
        stats.total_requests += 1;
        stats.delete_requests += 1;
        drop(stats);

        let req = request.into_inner();

        match self.db.delete(&req.key) {
            Ok(()) => {
                Ok(Response::new(proto::DeleteResponse { success: true, error: String::new() }))
            }
            Err(e) => {
                self.stats.write().errors += 1;
                Ok(Response::new(proto::DeleteResponse { success: false, error: e.to_string() }))
            }
        }
    }

    async fn batch_get(
        &self,
        request: Request<proto::BatchGetRequest>,
    ) -> Result<Response<proto::BatchGetResponse>, Status> {
        self.stats.write().total_requests += 1;

        let req = request.into_inner();
        let mut results = Vec::new();

        for key in req.keys {
            match self.db.get(&key) {
                Ok(Some(value)) => {
                    results.push(proto::KeyValue { key: key.clone(), found: true, value });
                }
                Ok(None) => {
                    results.push(proto::KeyValue { key: key.clone(), found: false, value: vec![] });
                }
                Err(_) => {
                    results.push(proto::KeyValue { key: key.clone(), found: false, value: vec![] });
                }
            }
        }

        Ok(Response::new(proto::BatchGetResponse { results }))
    }

    async fn write(
        &self,
        request: Request<proto::WriteRequest>,
    ) -> Result<Response<proto::WriteResponse>, Status> {
        self.stats.write().total_requests += 1;

        let req = request.into_inner();

        use crate::write_batch::WriteBatch;
        let mut batch = WriteBatch::new();

        for op in req.operations {
            match proto::write_op::OpType::try_from(op.op_type) {
                Ok(proto::write_op::OpType::Put) => {
                    batch.put(&op.key, &op.value);
                }
                Ok(proto::write_op::OpType::Delete) => {
                    batch.delete(&op.key);
                }
                Err(_) => {
                    return Ok(Response::new(proto::WriteResponse {
                        success: false,
                        error: "Invalid operation type".to_string(),
                    }));
                }
            }
        }

        match self.db.write(batch) {
            Ok(()) => {
                Ok(Response::new(proto::WriteResponse { success: true, error: String::new() }))
            }
            Err(e) => {
                self.stats.write().errors += 1;
                Ok(Response::new(proto::WriteResponse { success: false, error: e.to_string() }))
            }
        }
    }

    type ScanStream = tokio_stream::wrappers::ReceiverStream<Result<proto::ScanResponse, Status>>;

    async fn scan(
        &self,
        request: Request<proto::ScanRequest>,
    ) -> Result<Response<Self::ScanStream>, Status> {
        self.stats.write().total_requests += 1;

        let req = request.into_inner();

        let start_key = if req.start_key.is_empty() {
            None
        } else {
            Some(req.start_key.as_slice())
        };

        let end_key = if req.end_key.is_empty() {
            None
        } else {
            Some(req.end_key.as_slice())
        };

        let mut iter = match self.db.scan(start_key, end_key) {
            Ok(iter) => iter,
            Err(e) => return Err(rpc::to_status(e)),
        };

        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let limit = req.limit as usize;

        tokio::spawn(async move {
            let mut count = 0;
            while iter.valid() && (limit == 0 || count < limit) {
                let key = iter.key().to_vec();
                let value = iter.value().to_vec();

                if tx.send(Ok(proto::ScanResponse { key, value })).await.is_err() {
                    break;
                }

                iter.next();
                count += 1;
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn health_check(
        &self,
        _request: Request<proto::HealthCheckRequest>,
    ) -> Result<Response<proto::HealthCheckResponse>, Status> {
        Ok(Response::new(proto::HealthCheckResponse {
            status: proto::health_check_response::ServingStatus::Serving as i32,
        }))
    }

    async fn get_stats(
        &self,
        _request: Request<proto::GetStatsRequest>,
    ) -> Result<Response<proto::GetStatsResponse>, Status> {
        self.stats.write().total_requests += 1;

        // Get cache stats if available
        let cache_stats = self.db.cache_stats();

        Ok(Response::new(proto::GetStatsResponse {
            total_keys: 0,    // Would need to add this to DB
            total_size: 0,    // Would need to add this to DB
            memtable_size: 0, // Would need to add this to DB
            num_sstables: 0,  // Would need to add this to DB
            cache_stats: Some(proto::CacheStats {
                hits: cache_stats.hits,
                misses: cache_stats.misses,
                total_requests: cache_stats.lookups,
                hit_rate: cache_stats.hit_rate(),
            }),
        }))
    }
}

impl Clone for PrimaryStats {
    fn clone(&self) -> Self {
        Self {
            total_requests: self.total_requests,
            get_requests: self.get_requests,
            put_requests: self.put_requests,
            delete_requests: self.delete_requests,
            errors: self.errors,
        }
    }
}
