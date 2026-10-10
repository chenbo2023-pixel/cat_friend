use serde::{Deserialize, Serialize};

// ===== WebSocket 消息 schema =====
// JSON 文本帧，与串口协议帧（技术方案设计.md第7/8节）一一对应：
// JSON 层用可读字符串（面向前端与人工调试），字节层用紧凑编码（面向ESP32）。
// 三个翻译节点各翻一层：前端(JS)↔本schema、remote_cmd_node(schema↔ROS2 topic)、
// esp32_bridge_node(topic↔串口帧)。

// ---------- phone → server ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    /// 云台目标角度（度）。协议层允许±180，真实机械范围由ESP32兜底clamp——对应串口帧0x02
    #[serde(rename = "gimbal_cmd")]
    GimbalCmd { pan: i16, tilt: i16 },

    /// 执行器触发——对应串口帧0x03
    #[serde(rename = "actuator_trigger")]
    ActuatorTrigger { action: ActionKind },

    /// 应用层心跳：ts为客户端毫秒时间戳，服务端原样回显，客户端算RTT
    #[serde(rename = "ping")]
    Ping { ts: i64 },
}

#[allow(dead_code)] // SPEAKER对应喇叭硬件未接入，字段先按schema保留
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ActionKind {
    Wand,
    Speaker,
}

// ---------- server → phone ----------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    #[serde(rename = "pong")]
    Pong { ts: i64 },

    /// 动作完成确认——对应串口帧0x04/0x05（树莓派接入后由转发路径构造）
    #[allow(dead_code)]
    #[serde(rename = "action_completed")]
    ActionCompleted {
        kind: CompletedKind,
        status: CompletionStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        pan: Option<i16>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tilt: Option<i16>,
    },

    /// 下游状态（树莓派在线/离线/安全模式，含0x07状态帧的翻译结果）
    #[allow(dead_code)]
    #[serde(rename = "device_status")]
    DeviceStatus { state: DeviceState },

    #[serde(rename = "error")]
    Error { code: ErrorCode, msg: String },
}

#[allow(dead_code)] // 树莓派接入后由转发路径构造
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompletedKind {
    Gimbal,
    Wand,
    Speaker,
}

#[allow(dead_code)] // 同上
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompletionStatus {
    Ok,
    Fail,
}

#[allow(dead_code)] // 同上
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Online,
    Offline,
    SafetyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NoDownstream,
}
