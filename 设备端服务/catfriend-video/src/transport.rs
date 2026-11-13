//! 在 TCP 写入层限制无进展时间；Body 内部超时不能处理停止读取的客户端。
use axum::Router;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpListener,
    sync::watch,
    task::JoinSet,
    time::Sleep,
};

struct WriteTimeout<T> {
    inner: T,
    duration: Duration,
    deadline: Option<Pin<Box<Sleep>>>,
}

impl<T> WriteTimeout<T> {
    fn new(inner: T, duration: Duration) -> Self {
        Self {
            inner,
            duration,
            deadline: None,
        }
    }

    fn pending_write(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        use std::future::Future;
        let deadline = self
            .deadline
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(self.duration)));
        match deadline.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "观看连接写入超时",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for WriteTimeout<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for WriteTimeout<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write(cx, bytes) {
            Poll::Ready(result) => {
                if result.as_ref().is_ok_and(|n| *n > 0) {
                    this.deadline = None;
                }
                Poll::Ready(result)
            }
            Poll::Pending => this.pending_write(cx).map(|result| result.map(|_| 0)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Pending => this.pending_write(cx),
            ready => ready,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_shutdown(cx) {
            Poll::Pending => this.pending_write(cx),
            ready => ready,
        }
    }
}

pub async fn serve(
    listener: TcpListener,
    app: Router,
    write_timeout: Duration,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    let result = loop {
        if *stop.borrow() {
            break Ok(());
        }
        tokio::select! {
            _ = stop.changed() => break Ok(()),
            result = listener.accept() => {
                let (socket, address) = match result {
                    Ok(connection) => connection,
                    Err(error) => break Err(error),
                };
                let service = TowerToHyperService::new(app.clone());
                let mut connection_stop = stop.clone();
                connections.spawn(async move {
                    if *connection_stop.borrow() {
                        return;
                    }
                    let io = TokioIo::new(WriteTimeout::new(socket, write_timeout));
                    let connection = hyper::server::conn::http1::Builder::new()
                        .timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(5))
                        .serve_connection(io, service);
                    tokio::pin!(connection);
                    tokio::select! {
                        result = &mut connection => {
                            if let Err(error) = result {
                                tracing::debug!(%address, %error, "观看连接已关闭");
                            }
                        }
                        _ = connection_stop.changed() => {
                            connection.as_mut().graceful_shutdown();
                            let _ = tokio::time::timeout(Duration::from_secs(3), connection).await;
                        }
                    }
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    };
    // 即使客户端不读响应，整个服务也必须能在有界时间内退出。
    if tokio::time::timeout(Duration::from_secs(4), async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn nonreading_peer_times_out_at_io_layer() {
        let (writer, _reader) = tokio::io::duplex(8);
        let mut writer = WriteTimeout::new(writer, Duration::from_millis(30));
        let result = tokio::time::timeout(Duration::from_secs(1), writer.write_all(&[1; 1024]))
            .await
            .expect("写超时没有唤醒网络任务");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn progress_keeps_connection_usable() {
        let (writer, mut reader) = tokio::io::duplex(8);
        let mut writer = WriteTimeout::new(writer, Duration::from_millis(100));
        let reading = tokio::spawn(async move {
            let mut received = vec![0; 1024];
            reader.read_exact(&mut received).await.unwrap();
            received
        });
        writer.write_all(&[7; 1024]).await.unwrap();
        assert_eq!(reading.await.unwrap(), vec![7; 1024]);
    }

    #[tokio::test]
    async fn blocked_http_client_releases_viewer_and_server_can_stop() {
        use crate::{config::Config, http, stream::Video};
        let mut config = Config::default();
        config.server.max_viewers = 1;
        let video = Video::new(config);
        // 大响应填满 TCP 缓冲；客户端保持连接但不读取 Body。
        video.publish(bytes::Bytes::from(vec![1; 16 * 1024 * 1024]));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, receiver) = watch::channel(false);
        let server = tokio::spawn(serve(
            listener,
            http::router(video.clone()),
            Duration::from_millis(100),
            receiver,
        ));
        let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
        client
            .write_all(b"GET /stream.mjpg HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut first_byte = [0];
        client.read_exact(&mut first_byte).await.unwrap();
        assert!(video.acquire_viewer().is_none());
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if video.acquire_viewer().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("停止读取的 HTTP 客户端未释放观看名额");
        stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
