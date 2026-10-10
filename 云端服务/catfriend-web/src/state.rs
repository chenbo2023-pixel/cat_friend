use std::sync::Arc;

use crate::hub::Hub;

/// 全局共享状态。目前只有连接注册表；树莓派会话等接入后在此追加。
#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<Hub>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            hub: Arc::new(Hub::new()),
        }
    }
}
