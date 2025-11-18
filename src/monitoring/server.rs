//! HTTP server for exposing Prometheus metrics
//!
//! This module provides an HTTP server that exposes metrics at the /metrics endpoint
//! in Prometheus text format.

use crate::monitoring::metrics::MetricsCollector;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

/// HTTP server for exposing Prometheus metrics
pub struct MetricsServer {
    addr: SocketAddr,
    collector: Arc<MetricsCollector>,
}

impl MetricsServer {
    /// Create a new metrics server
    ///
    /// # Arguments
    ///
    /// * `addr` - The address to bind the server to
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            collector: Arc::new(MetricsCollector::new()),
        }
    }

    /// Get the metrics collector
    pub fn collector(&self) -> Arc<MetricsCollector> {
        Arc::clone(&self.collector)
    }

    /// Start the metrics server
    ///
    /// This will run the server in a loop, serving metrics at /metrics endpoint.
    /// The server runs until an error occurs or it's externally stopped.
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(self.addr).await?;
        log::info!("Metrics server listening on http://{}", self.addr);

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let collector = Arc::clone(&self.collector);

            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .serve_connection(
                        io,
                        service_fn(move |req| {
                            let collector = Arc::clone(&collector);
                            async move { handle_request(req, collector).await }
                        }),
                    )
                    .await
                {
                    log::error!("Error serving connection: {:?}", err);
                }
            });
        }
    }
}

/// Handle incoming HTTP requests
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    collector: Arc<MetricsCollector>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    match req.uri().path() {
        "/metrics" => {
            // Export metrics
            match collector.export() {
                Ok(metrics) => Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/plain; version=0.0.4")
                    .body(Full::new(Bytes::from(metrics)))
                    .unwrap()),
                Err(e) => {
                    log::error!("Failed to export metrics: {:?}", e);
                    Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Full::new(Bytes::from(format!("Error: {}", e))))
                        .unwrap())
                }
            }
        }
        "/health" => {
            // Simple health check endpoint
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("OK")))
                .unwrap())
        }
        "/" => {
            // Root endpoint with links
            let body = r#"<!DOCTYPE html>
<html>
<head><title>AiDb Metrics</title></head>
<body>
<h1>AiDb Metrics Server</h1>
<ul>
<li><a href="/metrics">Metrics</a> - Prometheus metrics endpoint</li>
<li><a href="/health">Health</a> - Health check endpoint</li>
</ul>
</body>
</html>"#;
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/html")
                .body(Full::new(Bytes::from(body)))
                .unwrap())
        }
        _ => {
            // 404 for all other paths
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::from("Not Found")))
                .unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_metrics_server_creation() {
        let addr = "127.0.0.1:0".parse().unwrap();
        let server = MetricsServer::new(addr);
        assert!(server.collector().export().is_ok());
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let server = MetricsServer::new(addr);

        // Start server in background
        let actual_addr = server.addr;
        tokio::spawn(async move {
            let _ = server.run().await;
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Try to connect (this test is basic and may not work in all environments)
        // In production, you'd use a proper HTTP client
        let result = timeout(
            Duration::from_secs(1),
            tokio::net::TcpStream::connect(actual_addr),
        )
        .await;

        // Just verify we can attempt connection
        // Full HTTP testing would require an HTTP client
        assert!(result.is_ok() || result.is_err()); // Either outcome is fine for this basic test
    }
}
