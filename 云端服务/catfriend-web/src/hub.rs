use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::messages::ServerMsg;

/// 连接注册表：每个 phone 会话持有一个 mpsc 发送端，
/// 服务端任何协程要给某个会话推消息时往通道里塞即可，
/// 由该会话自己的写任务串行发出（避免并发写同一个 socket）。
/// 树莓派会话接入后在此追加 pi 槽位与“指令转发/下行广播”逻辑。
pub struct Hub {
    phones: Mutex<HashMap<u64, mpsc::Sender<ServerMsg>>>,
    next_id: AtomicU64,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            phones: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn register(&self, tx: mpsc::Sender<ServerMsg>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.phones.lock().unwrap().insert(id, tx);
        tracing::info!(id, "phone 会话接入");
        id
    }

    pub fn unregister(&self, id: u64) {
        self.phones.lock().unwrap().remove(&id);
        tracing::info!(id, "phone 会话断开");
    }

    /// 树莓派（下游）是否在线。接入前恒为 false——
    /// phone 发指令时据此决定“转发给pi”还是“回 no_downstream 错误”。
    pub fn downstream_connected(&self) -> bool {
        // TODO(树莓派接入): 由 pi 会话的注册/断开维护状态位
        false
    }

    /// 发给单个会话。通道满则丢弃——本服务不做离线暂存（P0 无此需求）。
    pub fn send_to(&self, id: u64, msg: &ServerMsg) {
        if let Some(tx) = self.phones.lock().unwrap().get(&id) {
            let _ = tx.try_send(msg.clone());
        }
    }
}
