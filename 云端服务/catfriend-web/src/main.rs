// 猫岗哨云端Web服务：静态页面(PWA) + WebSocket消息转发
// 职责边界（技术方案设计.md第1节）：只做网络层转发与连接管理，不跑业务逻辑
//（仲裁在树莓派 cmd_mux_node）。
// 监听：127.0.0.1:8080 —— 公网流量由 Nginx(443, mTLS) proxy_pass 转进来，
// 本进程不直接暴露公网，准入由 mTLS 层完成。
// 树莓派接入后追加监听 WireGuard 网卡 10.0.0.1:8080（隧道内直连，隧道即鉴权）。
// 本地开发：cargo run 后浏览器打开 http://localhost:8080（static目录按CWD解析）。

mod hub;
mod messages;
mod state;
mod ws;

use state::AppState;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let app = axum::Router::new()
        .route("/ws", axum::routing::get(ws::ws_handler))
        .fallback_service(ServeDir::new("static"))
        .with_state(AppState::new());

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("绑定 {addr} 失败: {e}"));

    tracing::info!("catfriend-web 已启动: http://{addr}（WebSocket: /ws）");
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!("服务异常退出: {e}");
        std::process::exit(1);
    }
    tracing::info!("已停止");
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("收到 Ctrl-C，准备退出");
}
