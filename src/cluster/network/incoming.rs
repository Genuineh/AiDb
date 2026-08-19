//! 统一 gRPC 入站: accept 后设 TCP_NODELAY, 以及 Raft 用的 Server builder.

use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::StreamExt as _;
use tonic::transport::Server;

/// 将 `TcpListener` 包成 tonic `serve_with_incoming` 可用的流, 每条连接 `set_nodelay(true)`.
pub fn tcp_incoming(
    listener: TcpListener,
) -> impl tokio_stream::Stream<Item = Result<TcpStream, std::io::Error>> {
    TcpListenerStream::new(listener).map(|res| {
        let stream = res?;
        stream.set_nodelay(true)?;
        Ok(stream)
    })
}

/// Raft gRPC server builder. HTTP/2 PING interval 10s; timeout 用 tonic 默认.
pub fn raft_server() -> Server {
    Server::builder().http2_keepalive_interval(Some(Duration::from_secs(10)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn accepted_stream_has_nodelay() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut incoming = std::pin::pin!(tcp_incoming(listener));
        let client = tokio::spawn(async move {
            TcpStream::connect(addr).await.unwrap();
        });
        let stream = incoming.next().await.unwrap().unwrap();
        assert!(stream.nodelay().unwrap());
        client.await.unwrap();
    }
}
