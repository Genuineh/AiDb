//! RPC protocol implementation using tonic and gRPC
//!
//! This module contains the generated protobuf code and utilities
//! for RPC communication between cluster nodes.

use tonic::Status;

// Include the generated protobuf code
pub mod proto {
    tonic::include_proto!("aidb");
}

pub use proto::*;

/// Convert AiDb errors to gRPC Status
pub fn to_status(err: crate::error::Error) -> Status {
    Status::internal(err.to_string())
}

/// Convert Result to gRPC Result with Status
pub fn to_result<T>(result: Result<T, crate::error::Error>) -> Result<T, Status> {
    result.map_err(to_status)
}
