mod capture;
mod config;
mod frame;
mod http;
mod stream;
mod transport;

use std::{path::PathBuf, time::Duration};
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let mut args = std::env::args().skip(1);
    let path = match (args.next().as_deref(), args.next(), args.next()) {
        (Some("--config"), Some(path), None) => PathBuf::from(path),
        (Some("--help" | "-h"), None, None) => {
            println!("用法：catfriend-video --config <TOML文件>\nMac测试：--config config/mac.example.toml");
            return Ok(());
        }
        _ => return Err("用法：catfriend-video --config <TOML文件>".into()),
    };
    let config = config::Config::load(&path)?;
    let write_timeout = Duration::from_millis(config.server.write_timeout_ms);
    let listener = tokio::net::TcpListener::bind(config.server.listen).await?;
    tracing::info!(address = %listener.local_addr()?, source = ?config.capture.source, "设备端视频服务启动");
    let video = stream::Video::new(config);
    let app = http::router(video.clone());
    let (stop_sender, stop_receiver) = watch::channel(false);
    let capture_task = tokio::spawn(capture::run(video.clone(), stop_receiver));
    let server = transport::serve(listener, app, write_timeout, stop_sender.subscribe());
    tokio::pin!(server);
    let result = tokio::select! {
        result = &mut server => result,
        _ = shutdown_signal() => {
            tracing::info!("准备停止采集和观看连接");
            video.set_state(stream::CaptureState::Stopped, None);
            stop_sender.send_replace(true);
            server.await
        }
    };
    stop_sender.send_replace(true);
    video.set_state(stream::CaptureState::Stopped, None);
    let _ = capture_task.await;
    result?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("无法注册 SIGTERM");
        let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            .expect("无法注册 SIGHUP");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = hangup.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
