'use strict';

// ===== 常量（阈值与《技术方案设计.md》第11节一致） =====
const PING_INTERVAL_MS = 5000;     // 心跳周期
const LATENCY_HIGH_MS = 300;       // RTT超过此值显示"延迟较高"
const PING_MISSES_OFFLINE = 3;     // 连续3次无响应（约15s）判定离线
const ACK_TIMEOUT_MS = 3000;       // 指令确认超时兜底（P0产品设计第5节）
const PAN_STEP = 5, TILT_STEP = 5; // 方向按钮步长
const PAN_MIN = -180, PAN_MAX = 180;
const TILT_MIN = -45, TILT_MAX = 90;
const RECONNECT_DELAYS = [1000, 2000, 5000, 10000, 30000]; // 断线重连退避

// ===== 运行时状态 =====
let ws = null;
let wsOpen = false;
let pingTimer = null;
let reconnectTimer = null;
let reconnectAttempt = 0;
let missedPongs = 0;
let lastRtt = null;
let deviceState = null; // 下游状态：'online'|'offline'|'safety_mode'（树莓派接入后生效）

// 云台角度：前端维护"当前角度"，用ack回传的真实角度校正，防多次点击后漂移
let pan = 0, tilt = 0;
const pending = { gimbal: false, wand: false, speaker: false };
const ackTimers = { gimbal: null, wand: null, speaker: null };

// ===== DOM =====
const $ = (id) => document.getElementById(id);
const statusDot = $('status-dot');
const statusText = $('status-text');
const latencyEl = $('latency');
const angleEl = $('angle-display');
const flashEl = $('flash');

// ===== 状态条（优先级：离线 > 安全模式 > 延迟高 > 在线，第11节） =====
function updateStatus() {
  let cls, text;
  if (!wsOpen || missedPongs >= PING_MISSES_OFFLINE) {
    cls = 'offline'; text = '离线';
  } else if (deviceState === 'safety_mode') {
    cls = 'safety_mode'; text = '安全模式：装置已归中待命';
  } else if (lastRtt !== null && lastRtt > LATENCY_HIGH_MS) {
    cls = 'latency_high'; text = '网络延迟较高';
  } else {
    cls = 'online'; text = '在线-手动模式';
  }
  statusDot.className = 'dot ' + cls;
  statusText.textContent = text;
  latencyEl.textContent = (wsOpen && lastRtt !== null) ? lastRtt + ' ms' : '';
}

// ===== WebSocket 连接管理 =====
function connect() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  ws = new WebSocket(proto + '://' + location.host + '/ws');

  ws.onopen = () => {
    wsOpen = true;
    reconnectAttempt = 0;
    startPing();
    updateStatus();
  };
  ws.onmessage = (ev) => {
    let m;
    try { m = JSON.parse(ev.data); } catch { return; }
    onMessage(m);
  };
  ws.onclose = () => {
    wsOpen = false;
    stopPing();
    missedPongs = 0;
    lastRtt = null;
    updateStatus();
    scheduleReconnect();
  };
  ws.onerror = () => { /* onclose 会随后触发，统一在那里处理 */ };
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  const delay = RECONNECT_DELAYS[Math.min(reconnectAttempt, RECONNECT_DELAYS.length - 1)];
  reconnectAttempt++;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, delay);
}

function send(obj) {
  if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(obj));
}

// ===== 心跳与RTT（第3层链路检测） =====
function startPing() {
  stopPing();
  missedPongs = 0;
  lastRtt = null;
  pingTimer = setInterval(() => {
    missedPongs++;
    send({ type: 'ping', ts: Date.now() });
    // 网络黑洞时onclose不会自己来：连续丢pong达阈值就主动断开走重连
    if (missedPongs >= PING_MISSES_OFFLINE) {
      updateStatus();
      ws.close();
    }
  }, PING_INTERVAL_MS);
}
function stopPing() {
  clearInterval(pingTimer);
  pingTimer = null;
}

// ===== 服务端消息处理 =====
function onMessage(m) {
  switch (m.type) {
    case 'pong':
      lastRtt = Date.now() - m.ts;
      missedPongs = 0;
      updateStatus();
      break;
    case 'action_completed':
      onAck(m);
      break;
    case 'device_status':
      deviceState = m.state;
      updateStatus();
      break;
    case 'error':
      onError(m);
      break;
  }
}

function onAck(m) {
  if (m.kind === 'gimbal') {
    if (typeof m.pan === 'number') { pan = m.pan; tilt = m.tilt; updateAngle(); }
    clearPending('gimbal');
  } else if (m.kind === 'wand' || m.kind === 'speaker') {
    clearPending(m.kind);
  }
  flash(m.status === 'ok' ? '✓ 已完成' : '执行失败');
}

function onError(m) {
  if (m.code === 'no_downstream') {
    ['gimbal', 'wand', 'speaker'].forEach(clearPending);
    flash('下游未连接（树莓派未接入）');
  } else {
    flash(m.msg || '服务错误');
  }
}

// ===== 指令发送与按钮反馈（P0产品设计第5节三层反馈） =====
function targetGimbal(p, t) {
  if (pending.gimbal || !wsOpen) return;
  pending.gimbal = true;
  setPendingUI('gimbal', true);
  send({ type: 'gimbal_cmd', pan: p, tilt: t });
  armAckTimeout('gimbal');
}

function triggerActuator(kind) {
  if (pending[kind] || !wsOpen) return;
  pending[kind] = true;
  setPendingUI(kind, true);
  send({ type: 'actuator_trigger', action: kind === 'wand' ? 'WAND' : 'SPEAKER' });
  armAckTimeout(kind);
}

function armAckTimeout(kind) {
  clearTimeout(ackTimers[kind]);
  ackTimers[kind] = setTimeout(() => {
    if (pending[kind]) {
      clearPending(kind);
      flash('响应超时');
      // 超时不回滚本地角度：物理云台可能已动作，状态以之后收到的信息为准
    }
  }, ACK_TIMEOUT_MS);
}

function clearPending(kind) {
  pending[kind] = false;
  clearTimeout(ackTimers[kind]);
  ackTimers[kind] = null;
  setPendingUI(kind, false);
}

function setPendingUI(kind, on) {
  if (kind === 'gimbal') {
    document.querySelectorAll('#gimbal button').forEach((b) => { b.disabled = on; });
  } else {
    const b = $(kind === 'wand' ? 'btn-wand' : 'btn-speaker');
    b.disabled = on;
    b.textContent = on ? '执行中…' : (kind === 'wand' ? '逗猫棒' : '喇叭');
  }
}

// ===== 小工具 =====
function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)); }
function updateAngle() { angleEl.textContent = 'pan ' + pan + '° · tilt ' + tilt + '°'; }
let flashTimer = null;
function flash(text) {
  flashEl.textContent = text;
  flashEl.classList.add('show');
  clearTimeout(flashTimer);
  flashTimer = setTimeout(() => flashEl.classList.remove('show'), 2000);
}

// ===== 初始化 =====
document.querySelectorAll('#gimbal [data-dir]').forEach((btn) => {
  btn.addEventListener('click', () => {
    let p = pan, t = tilt;
    switch (btn.dataset.dir) {
      case 'left': p -= PAN_STEP; break;
      case 'right': p += PAN_STEP; break;
      case 'up': t += TILT_STEP; break;
      case 'down': t -= TILT_STEP; break;
    }
    targetGimbal(clamp(p, PAN_MIN, PAN_MAX), clamp(t, TILT_MIN, TILT_MAX));
  });
});
$('btn-center').addEventListener('click', () => targetGimbal(0, 0));
$('btn-wand').addEventListener('click', () => triggerActuator('wand'));
$('btn-speaker').addEventListener('click', () => triggerActuator('speaker'));

updateAngle();
connect();
