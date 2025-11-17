//! RPC protocol implementation using tonic and gRPC
//!
//! This module contains the generated protobuf code and utilities
//! for RPC communication between cluster nodes.

use tonic::Status;

// Include the generated protobuf code
#[allow(missing_docs)]
pub mod proto {
    tonic::include_proto!("aidb");
}

pub use proto::*;

/// Convert AiDb errors to gRPC Status
pub fn to_status(err: crate::error::Error) -> Status {
    Status::internal(err.to_string())
}
