use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use crate::hub::Hub;
use crate::messages::{ClientMsg, ErrorCode, ServerMsg};
use crate::state::AppState;

/// WebSocket 升级入口
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// 单个会话的生命周期。
/// 写路径：hub(或其他协程) → mpsc通道 → 写任务 → socket（串行化出站消息）
/// 读路径：socket → 读循环 → 解析ClientMsg → 转发/回包
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (tx, mut rx) = mpsc::channel::<ServerMsg>(16);
    let id = state.hub.register(tx);
    let hub: Arc<Hub> = state.hub;

    let (mut sink, mut stream) = socket.split();

    // 写任务：串行化所有出站消息，避免并发写同一socket
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let text = serde_json::to_string(&msg).expect("序列化ServerMsg不会失败");
            if sink.send(Message::Text(text)).await.is_err() {
                break; // 连接已断，读循环那边也会退出并做清理
            }
        }
    });

    while let Some(frame) = stream.next().await {
        let msg = match frame {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(id, err = %e, "读帧错误，断开会话");
                break;
            }
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            // 协议层Ping/Pong由底层自动应答；本协议不使用Binary帧
            _ => continue,
        };

        let parsed: Result<ClientMsg, _> = serde_json::from_str(&text);
        match parsed {
            Ok(ClientMsg::Ping { ts }) => {
                hub.send_to(id, &ServerMsg::Pong { ts });
            }
            Ok(ClientMsg::GimbalCmd { pan, tilt }) => {
                tracing::info!(id, pan, tilt, "gimbal_cmd");
                if !hub.downstream_connected() {
                    reply_no_downstream(&hub, id);
                }
                // 树莓派接入后：在此转发给 pi 会话（remote_cmd_node 翻译为 ROS2 topic）
            }
            Ok(ClientMsg::ActuatorTrigger { action }) => {
                tracing::info!(id, action = ?action, "actuator_trigger");
                if !hub.downstream_connected() {
                    reply_no_downstream(&hub, id);
                }
            }
            Err(e) => {
                // 与ESP32帧解析器同款策略：无法解析的消息记录后忽略，不断连
                let preview: String = text.chars().take(120).collect();
                tracing::warn!(id, err = %e, "无法解析的消息: {preview}");
            }
        }
    }

    hub.unregister(id);
    writer.abort();
    tracing::debug!(id, "会话清理完成");
}

fn reply_no_downstream(hub: &Arc<Hub>, id: u64) {
    hub.send_to(
        id,
        &ServerMsg::Error {
            code: ErrorCode::NoDownstream,
            msg: "树莓派尚未接入，指令已丢弃".to_string(),
        },
    );
}
